//! Sidebar search state and logic for Home and Reports tabs.
//!
//! This module provides search functionality that appears in the sidebar
//! when activated via Ctrl+T + /. It searches conversations and their replies,
//! highlighting matching terms.
//!
//! # Hierarchical Search Results
//! Search results are displayed hierarchically, showing the full parent chain
//! for each matching conversation. For example, if conversation 3 (child of 2,
//! which is child of 1) has a matching message, the results show:
//! ```text
//! Conversation 1 title (context ancestor - dimmed)
//!     Conversation 2 title (context ancestor - dimmed)
//!         Conversation 3 title (has matches - normal)
//!             -> matching message content here
//! ```

use crate::models::Thread;
use crate::store::AppDataStore;
use std::cell::Ref;
use std::collections::{HashMap, HashSet};

/// State for the sidebar search input
#[derive(Debug, Clone, Default)]
pub struct SidebarSearchState {
    /// Whether the search input is visible
    pub visible: bool,
    /// Current search query
    pub query: String,
    /// Cursor position within query
    pub cursor: usize,
    /// Cached search results for conversations (legacy flat format)
    pub results: Vec<SearchResult>,
    /// Hierarchical search results (new format with ancestor context)
    pub hierarchical_results: Vec<HierarchicalSearchItem>,
    /// Cached search results for reports
    pub report_results: Vec<tenex_core::models::Report>,
    /// Selected result index
    pub selected_index: usize,
    // Note: scroll_offset is computed fresh each frame in the renderer
    // using real layout data, not stored in state
}

impl SidebarSearchState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Toggle visibility of the search input
    pub fn toggle(&mut self) {
        self.visible = !self.visible;
        if !self.visible {
            // Clear state when hiding
            self.query.clear();
            self.cursor = 0;
            self.results.clear();
            self.hierarchical_results.clear();
            self.report_results.clear();
            self.selected_index = 0;
        }
    }

    /// Insert a character at the cursor position (cursor is char index, not byte index)
    pub fn insert_char(&mut self, c: char) {
        // Convert char index to byte index for insert
        let byte_idx = self.char_to_byte_index(self.cursor);
        self.query.insert(byte_idx, c);
        self.cursor += 1;
    }

    /// Delete character before cursor (cursor is char index, not byte index)
    pub fn delete_char(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            // Convert char index to byte index for remove
            let byte_idx = self.char_to_byte_index(self.cursor);
            self.query.remove(byte_idx);
        }
    }

    /// Move cursor left (cursor is char index)
    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor right (cursor is char index)
    pub fn move_cursor_right(&mut self) {
        let char_count = self.query.chars().count();
        if self.cursor < char_count {
            self.cursor += 1;
        }
    }

    /// Convert a character index to a byte index
    fn char_to_byte_index(&self, char_idx: usize) -> usize {
        self.query
            .char_indices()
            .nth(char_idx)
            .map(|(byte_idx, _)| byte_idx)
            .unwrap_or(self.query.len())
    }

    /// Move selection up in results
    /// Note: scroll offset adjustment happens in the renderer where we have real layout data
    pub fn move_selection_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down in results (for conversations)
    pub fn move_selection_down(&mut self) {
        let count = self.hierarchical_results.len();
        if count > 0 && self.selected_index < count - 1 {
            self.selected_index += 1;
        }
    }

    /// Move selection down for reports
    pub fn move_selection_down_reports(&mut self) {
        let count = self.report_results.len();
        if count > 0 && self.selected_index < count - 1 {
            self.selected_index += 1;
        }
    }


    /// Get currently selected result (clamped to valid range) - legacy flat format
    pub fn selected_result(&self) -> Option<&SearchResult> {
        if self.results.is_empty() {
            None
        } else {
            let idx = self.selected_index.min(self.results.len() - 1);
            self.results.get(idx)
        }
    }

    /// Get currently selected hierarchical result (clamped to valid range)
    pub fn selected_hierarchical_result(&self) -> Option<&HierarchicalSearchItem> {
        if self.hierarchical_results.is_empty() {
            None
        } else {
            let idx = self.selected_index.min(self.hierarchical_results.len() - 1);
            self.hierarchical_results.get(idx)
        }
    }

    /// Get currently selected report (clamped to valid range)
    pub fn selected_report(&self) -> Option<&tenex_core::models::Report> {
        if self.report_results.is_empty() {
            None
        } else {
            let idx = self.selected_index.min(self.report_results.len() - 1);
            self.report_results.get(idx)
        }
    }

    /// Clamp selected_index to valid range based on current results
    /// Call this after updating results to ensure index stays valid
    pub fn clamp_selection(&mut self, for_reports: bool) {
        let max_len = if for_reports {
            self.report_results.len()
        } else {
            self.hierarchical_results.len()
        };
        if max_len == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= max_len {
            self.selected_index = max_len - 1;
        }
    }

    /// Check if search has a non-empty query
    pub fn has_query(&self) -> bool {
        !self.query.trim().is_empty()
    }
}

/// A search result representing a thread with a matching reply
#[derive(Debug, Clone)]
pub struct SearchResult {
    /// The thread containing the match
    pub thread: Thread,
    /// Thread ID
    pub thread_id: String,
    /// Thread title
    pub thread_title: String,
    /// Project a_tag
    pub project_a_tag: String,
    /// Project name for display
    pub project_name: String,
    /// The matching reply content (if match was in a reply)
    pub matching_reply: Option<MatchingReply>,
    /// Match type
    pub match_type: SearchMatchType,
    /// When the match was created (for sorting)
    pub created_at: u64,
}

/// Information about a matching reply
#[derive(Debug, Clone)]
pub struct MatchingReply {
    /// Reply content
    pub content: String,
    /// Author pubkey
    pub author_pubkey: String,
}

/// A single matching message within a conversation
#[derive(Debug, Clone)]
pub struct MatchingMessage {
    /// Message content
    pub content: String,
    /// Author pubkey
    pub author_pubkey: String,
    /// When the message was created
    pub created_at: u64,
}

/// An item in the hierarchical search result display
#[derive(Debug, Clone)]
pub enum HierarchicalSearchItem {
    /// A context ancestor conversation (no matches, just providing hierarchy)
    /// Displayed dimmed to show it's not a direct match
    ContextAncestor {
        thread: Thread,
        thread_id: String,
        thread_title: String,
        project_a_tag: String,
        depth: usize,
    },
    /// A conversation with actual search matches
    MatchedConversation {
        thread: Thread,
        thread_id: String,
        thread_title: String,
        project_a_tag: String,
        project_name: String,
        /// All matching messages in this conversation
        matching_messages: Vec<MatchingMessage>,
        /// Whether the title itself matched
        title_matched: bool,
        /// Whether the root content matched
        content_matched: bool,
        /// Whether matched by conversation ID
        id_matched: bool,
        depth: usize,
    },
}

/// Type of search match
#[derive(Debug, Clone, PartialEq)]
pub enum SearchMatchType {
    /// Match in thread title
    ThreadTitle,
    /// Match in thread content
    ThreadContent,
    /// Match in a reply message
    Reply,
    /// Match in conversation ID
    ConversationId,
}

/// Search conversations and messages for a query
///
/// Returns results matching the query in thread titles, content, or reply messages.
/// Respects project filters (visible_projects) but NOT time filters.
/// Empty visible_projects = show nothing (consistent with other lists)
pub fn search_conversations(
    query: &str,
    store: &Ref<AppDataStore>,
    visible_projects: &HashSet<String>,
) -> Vec<SearchResult> {
    // Empty visible_projects = show nothing (consistent with other lists)
    if visible_projects.is_empty() {
        return vec![];
    }

    if query.trim().is_empty() {
        return vec![];
    }

    let filter = query.to_lowercase();
    let mut results = Vec::new();
    let mut seen_threads: HashSet<String> = HashSet::new();

    // Search through all projects
    for project in store.get_projects() {
        let a_tag = project.a_tag();

        // Skip projects not in visible_projects
        if !visible_projects.contains(&a_tag) {
            continue;
        }

        let project_name = project.name.clone();

        // Search threads
        for thread in store.get_threads(&a_tag) {
            // Check thread title
            let title_matches = thread.title.to_lowercase().contains(&filter);
            // Check thread content
            let content_matches = thread.content.to_lowercase().contains(&filter);
            // Check conversation ID (prefix match for shortened IDs like 12-char or full 64-char)
            let id_matches = thread.id.to_lowercase().starts_with(&filter);

            if title_matches || content_matches || id_matches {
                seen_threads.insert(thread.id.clone());

                let match_type = if id_matches {
                    SearchMatchType::ConversationId
                } else if title_matches {
                    SearchMatchType::ThreadTitle
                } else {
                    SearchMatchType::ThreadContent
                };

                results.push(SearchResult {
                    thread: thread.clone(),
                    thread_id: thread.id.clone(),
                    thread_title: thread.title.clone(),
                    project_a_tag: a_tag.clone(),
                    project_name: project_name.clone(),
                    matching_reply: None,
                    match_type,
                    created_at: thread.last_activity,
                });
            }

            // Search messages/replies in this thread
            let messages = store.get_messages(&thread.id);
            for msg in messages {
                // Skip the root message (already covered by thread search)
                if msg.id == thread.id {
                    continue;
                }

                let content_lower = msg.content.to_lowercase();
                if content_lower.contains(&filter) {
                    // Don't add duplicate thread entries
                    if seen_threads.contains(&thread.id) {
                        // Update existing result with the reply info if it's a better match
                        if let Some(existing) = results.iter_mut().find(|r| r.thread_id == thread.id) {
                            if existing.matching_reply.is_none() {
                                existing.matching_reply = Some(MatchingReply {
                                    content: msg.content.clone(),
                                    author_pubkey: msg.pubkey.clone(),
                                });
                                existing.match_type = SearchMatchType::Reply;
                            }
                        }
                    } else {
                        seen_threads.insert(thread.id.clone());

                        results.push(SearchResult {
                            thread: thread.clone(),
                            thread_id: thread.id.clone(),
                            thread_title: thread.title.clone(),
                            project_a_tag: a_tag.clone(),
                            project_name: project_name.clone(),
                            matching_reply: Some(MatchingReply {
                                content: msg.content.clone(),
                                author_pubkey: msg.pubkey.clone(),
                            }),
                            match_type: SearchMatchType::Reply,
                            created_at: msg.created_at,
                        });
                    }
                }
            }
        }
    }

    // Sort by last activity (most recent first)
    results.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    results
}

/// Search reports for a query
/// Empty visible_projects = show nothing (consistent with other lists)
pub fn search_reports(
    query: &str,
    store: &Ref<AppDataStore>,
    visible_projects: &HashSet<String>,
) -> Vec<tenex_core::models::Report> {
    // Empty visible_projects = show nothing (consistent with other lists)
    if visible_projects.is_empty() {
        return vec![];
    }

    if query.trim().is_empty() {
        return vec![];
    }

    let filter = query.to_lowercase();
    let mut results = Vec::new();

    for report in store.get_reports() {
        // Skip reports not in visible_projects
        if !visible_projects.contains(&report.project_a_tag) {
            continue;
        }

        // Check title, summary, content, and hashtags
        let title_matches = report.title.to_lowercase().contains(&filter);
        let summary_matches = report.summary.to_lowercase().contains(&filter);
        let content_matches = report.content.to_lowercase().contains(&filter);
        let hashtag_matches = report.hashtags.iter().any(|h| h.to_lowercase().contains(&filter));

        if title_matches || summary_matches || content_matches || hashtag_matches {
            results.push(report.clone());
        }
    }

    // Sort by created_at (most recent first)
    results.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    results
}

/// Raw search match data for a conversation (internal use)
#[derive(Debug, Clone)]
struct ConversationMatch {
    thread: Thread,
    project_a_tag: String,
    project_name: String,
    matching_messages: Vec<MatchingMessage>,
    title_matched: bool,
    content_matched: bool,
    id_matched: bool,
}

/// Search conversations and build hierarchical results
///
/// Returns a flat list of HierarchicalSearchItem that represents the tree structure
/// with proper depth values for indentation. Context ancestors (conversations that
/// don't match but are parents of matching conversations) are included with dimmed styling.
pub fn search_conversations_hierarchical(
    query: &str,
    store: &Ref<AppDataStore>,
    visible_projects: &HashSet<String>,
) -> Vec<HierarchicalSearchItem> {
    if visible_projects.is_empty() || query.trim().is_empty() {
        return vec![];
    }

    let filter = query.to_lowercase();

    // Step 1: Collect all matching conversations with ALL their matching messages
    let mut matches_by_conv: HashMap<String, ConversationMatch> = HashMap::new();

    for project in store.get_projects() {
        let a_tag = project.a_tag();
        if !visible_projects.contains(&a_tag) {
            continue;
        }

        let project_name = project.name.clone();

        for thread in store.get_threads(&a_tag) {
            let title_matched = thread.title.to_lowercase().contains(&filter);
            let content_matched = thread.content.to_lowercase().contains(&filter);
            let id_matched = thread.id.to_lowercase().starts_with(&filter);

            let mut matching_messages: Vec<MatchingMessage> = Vec::new();

            // Collect ALL matching messages in this thread
            let messages = store.get_messages(&thread.id);
            for msg in messages {
                // Skip root message if we're counting it via content_matched
                if msg.id == thread.id {
                    continue;
                }

                if msg.content.to_lowercase().contains(&filter) {
                    matching_messages.push(MatchingMessage {
                        content: msg.content.clone(),
                        author_pubkey: msg.pubkey.clone(),
                        created_at: msg.created_at,
                    });
                }
            }

            // If there's any match, record this conversation
            if title_matched || content_matched || id_matched || !matching_messages.is_empty() {
                matches_by_conv.insert(
                    thread.id.clone(),
                    ConversationMatch {
                        thread: thread.clone(),
                        project_a_tag: a_tag.clone(),
                        project_name: project_name.clone(),
                        matching_messages,
                        title_matched,
                        content_matched,
                        id_matched,
                    },
                );
            }
        }
    }

    if matches_by_conv.is_empty() {
        return vec![];
    }

    // Step 2: For each matching conversation, get its ancestor chain
    // Build a map of threads from visible projects only for quick lookup
    let mut all_threads: HashMap<String, (Thread, String)> = HashMap::new();
    for project in store.get_projects() {
        let a_tag = project.a_tag();
        // Only include threads from visible projects to prevent context leakage
        if !visible_projects.contains(&a_tag) {
            continue;
        }
        for thread in store.get_threads(&a_tag) {
            all_threads.insert(thread.id.clone(), (thread.clone(), a_tag.clone()));
        }
    }

    // Get ancestors for each matching conversation
    let mut ancestor_ids: HashSet<String> = HashSet::new();
    for conv_id in matches_by_conv.keys() {
        let ancestors = store.get_runtime_ancestors(conv_id);
        for ancestor_id in ancestors {
            // Only include if not already a match and we have the thread data (from visible projects)
            if !matches_by_conv.contains_key(&ancestor_id) && all_threads.contains_key(&ancestor_id) {
                ancestor_ids.insert(ancestor_id);
            }
        }
    }

    // Step 3: Build the hierarchical tree structure
    // Find root nodes (conversations with matches or as ancestors that have no parent in our set)
    let all_relevant_ids: HashSet<&String> = matches_by_conv
        .keys()
        .chain(ancestor_ids.iter())
        .collect();

    // Build parent -> children map for our relevant conversations
    let mut children_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut has_parent: HashSet<String> = HashSet::new();

    for id in &all_relevant_ids {
        if let Some((thread, _)) = all_threads.get(*id) {
            if let Some(ref parent_id) = thread.parent_conversation_id {
                if all_relevant_ids.contains(parent_id) {
                    children_map.entry(parent_id.clone()).or_default().push((*id).clone());
                    has_parent.insert((*id).clone());
                }
            } else {
                // Also check runtime hierarchy for parent
                if let Some(parent_id) = store.get_runtime_ancestors(*id).first() {
                    if all_relevant_ids.contains(parent_id) {
                        children_map.entry(parent_id.clone()).or_default().push((*id).clone());
                        has_parent.insert((*id).clone());
                    }
                }
            }
        }
    }

    // Sort children by last_activity descending
    for children in children_map.values_mut() {
        children.sort_by(|a, b| {
            let a_activity = all_threads.get(a).map(|(t, _)| t.last_activity).unwrap_or(0);
            let b_activity = all_threads.get(b).map(|(t, _)| t.last_activity).unwrap_or(0);
            b_activity.cmp(&a_activity)
        });
    }

    // Find root nodes (those with no parent in our relevant set)
    let mut root_ids: Vec<String> = all_relevant_ids
        .iter()
        .filter(|id| !has_parent.contains(**id))
        .map(|id| (*id).clone())
        .collect();

    // Sort roots by last_activity descending
    root_ids.sort_by(|a, b| {
        let a_activity = all_threads.get(a).map(|(t, _)| t.last_activity).unwrap_or(0);
        let b_activity = all_threads.get(b).map(|(t, _)| t.last_activity).unwrap_or(0);
        b_activity.cmp(&a_activity)
    });

    // Step 4: Build the flattened hierarchical list
    let mut result: Vec<HierarchicalSearchItem> = Vec::new();

    fn add_node(
        conv_id: &str,
        depth: usize,
        matches_by_conv: &HashMap<String, ConversationMatch>,
        ancestor_ids: &HashSet<String>,
        all_threads: &HashMap<String, (Thread, String)>,
        children_map: &HashMap<String, Vec<String>>,
        result: &mut Vec<HierarchicalSearchItem>,
    ) {
        if let Some(conv_match) = matches_by_conv.get(conv_id) {
            // This is a matched conversation
            result.push(HierarchicalSearchItem::MatchedConversation {
                thread: conv_match.thread.clone(),
                thread_id: conv_id.to_string(),
                thread_title: conv_match.thread.title.clone(),
                project_a_tag: conv_match.project_a_tag.clone(),
                project_name: conv_match.project_name.clone(),
                matching_messages: conv_match.matching_messages.clone(),
                title_matched: conv_match.title_matched,
                content_matched: conv_match.content_matched,
                id_matched: conv_match.id_matched,
                depth,
            });
        } else if ancestor_ids.contains(conv_id) {
            // This is a context ancestor
            if let Some((thread, a_tag)) = all_threads.get(conv_id) {
                result.push(HierarchicalSearchItem::ContextAncestor {
                    thread: thread.clone(),
                    thread_id: conv_id.to_string(),
                    thread_title: thread.title.clone(),
                    project_a_tag: a_tag.clone(),
                    depth,
                });
            }
        }

        // Add children recursively
        if let Some(children) = children_map.get(conv_id) {
            for child_id in children {
                add_node(
                    child_id,
                    depth + 1,
                    matches_by_conv,
                    ancestor_ids,
                    all_threads,
                    children_map,
                    result,
                );
            }
        }
    }

    for root_id in root_ids {
        add_node(
            &root_id,
            0,
            &matches_by_conv,
            &ancestor_ids,
            &all_threads,
            &children_map,
            &mut result,
        );
    }

    result
}

impl HierarchicalSearchItem {
    /// Get the thread ID for this item
    pub fn thread_id(&self) -> &str {
        match self {
            HierarchicalSearchItem::ContextAncestor { thread_id, .. } => thread_id,
            HierarchicalSearchItem::MatchedConversation { thread_id, .. } => thread_id,
        }
    }

    /// Get the depth (indentation level) for this item
    pub fn depth(&self) -> usize {
        match self {
            HierarchicalSearchItem::ContextAncestor { depth, .. } => *depth,
            HierarchicalSearchItem::MatchedConversation { depth, .. } => *depth,
        }
    }

    /// Check if this item is a context ancestor (no direct matches)
    pub fn is_context_ancestor(&self) -> bool {
        matches!(self, HierarchicalSearchItem::ContextAncestor { .. })
    }

    /// Get the thread title
    pub fn title(&self) -> &str {
        match self {
            HierarchicalSearchItem::ContextAncestor { thread_title, .. } => thread_title,
            HierarchicalSearchItem::MatchedConversation { thread_title, .. } => thread_title,
        }
    }

    /// Get the project a_tag
    pub fn project_a_tag(&self) -> &str {
        match self {
            HierarchicalSearchItem::ContextAncestor { project_a_tag, .. } => project_a_tag,
            HierarchicalSearchItem::MatchedConversation { project_a_tag, .. } => project_a_tag,
        }
    }
}
