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
//!
//! # Multi-term Search with '+' Operator
//! The '+' operator allows combining multiple search terms with AND semantics:
//! - Query: "error+timeout" finds conversations where BOTH "error" AND "timeout"
//!   appear (each term can be in different messages within the same conversation)
//! - Under each matching conversation, ALL messages matching ANY term are shown
//! - Example: A conversation with one message containing "error" and another
//!   containing "timeout" would match, and both messages would be displayed

use crate::models::Thread;
use crate::store::AppDataStore;
use std::cell::Ref;
use std::collections::{HashMap, HashSet};
// Re-use shared search utilities from tenex_core to prevent semantic drift
pub use tenex_core::search::parse_search_terms;
use tenex_core::search::text_contains_term;

/// State for the sidebar search input
#[derive(Debug, Clone, Default)]
pub struct SidebarSearchState {
    /// Whether the search input is visible
    pub visible: bool,
    /// Current search query
    pub query: String,
    /// Cursor position within query
    pub cursor: usize,
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
            // Convert cursor (char index) to byte index
            let cursor_byte_idx = self.char_to_byte_index(self.cursor);
            // Find the previous character boundary by looking at the slice before cursor
            let prev_boundary = self.query[..cursor_byte_idx]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.query.remove(prev_boundary);
            self.cursor -= 1;
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

    /// Check if search has a non-empty query
    pub fn has_query(&self) -> bool {
        !self.query.trim().is_empty()
    }
}

/// A single matching message within a conversation
#[derive(Debug, Clone)]
pub struct MatchingMessage {
    /// Message content
    pub content: String,
    /// Author pubkey
    pub author_pubkey: String,
}

/// An item in the hierarchical search result display
#[derive(Debug, Clone)]
pub enum HierarchicalSearchItem {
    /// A context ancestor conversation (no matches, just providing hierarchy)
    /// Displayed dimmed to show it's not a direct match
    ContextAncestor {
        thread: Thread,
        thread_title: String,
        project_a_tag: String,
        depth: usize,
    },
    /// A conversation with actual search matches
    MatchedConversation {
        thread: Thread,
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
        /// The search terms that were matched (for multi-term highlighting)
        /// For single-term searches, this contains one term
        matched_terms: Vec<String>,
    },
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

    for report in store.reports.get_reports() {
        // Skip reports not in visible_projects
        if !visible_projects.contains(&report.project_a_tag) {
            continue;
        }

        // Check title, summary, content, and hashtags
        let title_matches = report.title.to_lowercase().contains(&filter);
        let summary_matches = report.summary.to_lowercase().contains(&filter);
        let content_matches = report.content.to_lowercase().contains(&filter);
        let hashtag_matches = report
            .hashtags
            .iter()
            .any(|h| h.to_lowercase().contains(&filter));

        if title_matches || summary_matches || content_matches || hashtag_matches {
            results.push(report.clone());
        }
    }

    // Sort by created_at (most recent first)
    results.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    results
}

// NOTE: parse_search_terms and text_contains_term are now imported from tenex_core::search
// to ensure consistent search semantics across the codebase.

/// Check if text starts with a prefix (ASCII case-insensitive)
/// Used for conversation ID prefix matching (TUI-specific feature)
fn text_starts_with_ascii(text: &str, prefix: &str) -> bool {
    let text_chars: Vec<char> = text.chars().collect();
    let prefix_chars: Vec<char> = prefix.chars().collect();

    if prefix_chars.is_empty() {
        return true;
    }

    if text_chars.len() < prefix_chars.len() {
        return false;
    }

    prefix_chars.iter().enumerate().all(|(i, pc)| {
        text_chars
            .get(i)
            .is_some_and(|c| c.eq_ignore_ascii_case(pc))
    })
}

/// Result of scanning a thread and its messages for search term matches
#[derive(Debug, Clone)]
pub struct TermMatchResult {
    /// Which term indices matched in the title
    pub terms_in_title: HashSet<usize>,
    /// Which term indices matched in the content
    pub terms_in_content: HashSet<usize>,
    /// Which term indices matched in the ID (prefix match)
    pub terms_in_id: HashSet<usize>,
    /// Messages that matched any search term (for display)
    pub matching_messages: Vec<MatchingMessage>,
    /// Whether all terms were found (respects multi-term AND logic)
    pub all_terms_matched: bool,
}

impl TermMatchResult {
    /// Check if any match occurred in title
    pub fn title_matched(&self) -> bool {
        !self.terms_in_title.is_empty()
    }

    /// Check if any match occurred in content
    pub fn content_matched(&self) -> bool {
        !self.terms_in_content.is_empty()
    }

    /// Check if any match occurred in ID
    pub fn id_matched(&self) -> bool {
        !self.terms_in_id.is_empty()
    }
}

/// A message with content and author for matching purposes
pub struct MessageContent<'a> {
    pub id: &'a str,
    pub content: &'a str,
    pub pubkey: &'a str,
    pub created_at: u64,
}

/// Scan a thread (title, content, ID) and its messages for search term matches.
/// Returns detailed match information including which terms hit where.
///
/// This is the shared implementation used by both flat and hierarchical search.
pub fn scan_thread_for_terms(
    thread_id: &str,
    thread_title: &str,
    thread_content: &str,
    messages: &[MessageContent],
    terms: &[String],
) -> TermMatchResult {
    let is_multi_term = terms.len() > 1;

    let mut terms_in_title: HashSet<usize> = HashSet::new();
    let mut terms_in_content: HashSet<usize> = HashSet::new();
    let mut terms_in_id: HashSet<usize> = HashSet::new();
    let mut terms_in_messages: HashSet<usize> = HashSet::new();
    let mut matching_messages: Vec<MatchingMessage> = Vec::new();

    // Check each term against title, content, and ID
    for (term_idx, term) in terms.iter().enumerate() {
        if text_contains_term(thread_title, term) {
            terms_in_title.insert(term_idx);
        }
        if text_contains_term(thread_content, term) {
            terms_in_content.insert(term_idx);
        }
        if text_starts_with_ascii(thread_id, term) {
            terms_in_id.insert(term_idx);
        }
    }

    // Scan messages for term matches
    for msg in messages {
        // Skip root message (already covered by thread content)
        if msg.id == thread_id {
            continue;
        }

        let mut msg_matches_any = false;
        for (term_idx, term) in terms.iter().enumerate() {
            if text_contains_term(msg.content, term) {
                terms_in_messages.insert(term_idx);
                msg_matches_any = true;
            }
        }

        if msg_matches_any {
            matching_messages.push(MatchingMessage {
                content: msg.content.to_string(),
                author_pubkey: msg.pubkey.to_string(),
            });
        }
    }

    // Check if all terms are found (AND logic for multi-term, OR for single term)
    let all_terms_matched = if is_multi_term {
        let mut all_found_terms: HashSet<usize> = HashSet::new();
        all_found_terms.extend(&terms_in_title);
        all_found_terms.extend(&terms_in_content);
        all_found_terms.extend(&terms_in_id);
        all_found_terms.extend(&terms_in_messages);
        all_found_terms.len() == terms.len()
    } else {
        !terms_in_title.is_empty()
            || !terms_in_content.is_empty()
            || !terms_in_id.is_empty()
            || !terms_in_messages.is_empty()
    };

    TermMatchResult {
        terms_in_title,
        terms_in_content,
        terms_in_id,
        matching_messages,
        all_terms_matched,
    }
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
    /// For multi-term searches, tracks which terms matched in this conversation
    matched_terms: Vec<String>,
}

/// Search conversations and build hierarchical results
///
/// Returns a flat list of HierarchicalSearchItem that represents the tree structure
/// with proper depth values for indentation. Context ancestors (conversations that
/// don't match but are parents of matching conversations) are included with dimmed styling.
///
/// # Multi-term Search ('+' operator)
/// When the query contains '+', it's split into multiple terms:
/// - A conversation matches only if ALL terms are found somewhere in the conversation
///   (title, content, or any reply) - AND semantics at conversation level
/// - Under matching conversations, ALL messages that match ANY term are shown - OR semantics
///   for reply display
pub fn search_conversations_hierarchical(
    query: &str,
    store: &Ref<AppDataStore>,
    visible_projects: &HashSet<String>,
) -> Vec<HierarchicalSearchItem> {
    if visible_projects.is_empty() || query.trim().is_empty() {
        return vec![];
    }

    // Parse search terms (supports '+' operator for AND semantics)
    let terms = parse_search_terms(query);
    if terms.is_empty() {
        return vec![];
    }

    // Step 1: Collect all matching conversations with ALL their matching messages
    let mut matches_by_conv: HashMap<String, ConversationMatch> = HashMap::new();

    for project in store.get_projects() {
        let a_tag = project.a_tag();
        if !visible_projects.contains(&a_tag) {
            continue;
        }

        let project_name = project.title.clone();

        for thread in store.get_threads(&a_tag) {
            // Build message list for the shared matcher
            let messages = store.get_messages(&thread.id);
            let msg_contents: Vec<MessageContent> = messages
                .iter()
                .map(|msg| MessageContent {
                    id: &msg.id,
                    content: &msg.content,
                    pubkey: &msg.pubkey,
                    created_at: msg.created_at,
                })
                .collect();

            // Use shared matcher
            let match_result = scan_thread_for_terms(
                &thread.id,
                &thread.title,
                &thread.content,
                &msg_contents,
                &terms,
            );

            if match_result.all_terms_matched {
                // Extract booleans before moving matching_messages
                let title_matched = match_result.title_matched();
                let content_matched = match_result.content_matched();
                let id_matched = match_result.id_matched();

                matches_by_conv.insert(
                    thread.id.clone(),
                    ConversationMatch {
                        thread: thread.clone(),
                        project_a_tag: a_tag.clone(),
                        project_name: project_name.clone(),
                        matching_messages: match_result.matching_messages,
                        title_matched,
                        content_matched,
                        id_matched,
                        matched_terms: terms.clone(),
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
            if !matches_by_conv.contains_key(&ancestor_id) && all_threads.contains_key(&ancestor_id)
            {
                ancestor_ids.insert(ancestor_id);
            }
        }
    }

    // Step 3: Build the hierarchical tree structure
    // Find root nodes (conversations with matches or as ancestors that have no parent in our set)
    let all_relevant_ids: HashSet<&String> =
        matches_by_conv.keys().chain(ancestor_ids.iter()).collect();

    // Build parent -> children map for our relevant conversations
    let mut children_map: HashMap<String, Vec<String>> = HashMap::new();
    let mut has_parent: HashSet<String> = HashSet::new();

    for id in &all_relevant_ids {
        if let Some((thread, _)) = all_threads.get(*id) {
            if let Some(ref parent_id) = thread.parent_conversation_id {
                if all_relevant_ids.contains(parent_id) {
                    children_map
                        .entry(parent_id.clone())
                        .or_default()
                        .push((*id).clone());
                    has_parent.insert((*id).clone());
                }
            } else {
                // Also check runtime hierarchy for parent
                if let Some(parent_id) = store.get_runtime_ancestors(id).first() {
                    if all_relevant_ids.contains(parent_id) {
                        children_map
                            .entry(parent_id.clone())
                            .or_default()
                            .push((*id).clone());
                        has_parent.insert((*id).clone());
                    }
                }
            }
        }
    }

    // Sort children by last_activity descending
    for children in children_map.values_mut() {
        children.sort_by(|a, b| {
            let a_activity = all_threads
                .get(a)
                .map(|(t, _)| t.last_activity)
                .unwrap_or(0);
            let b_activity = all_threads
                .get(b)
                .map(|(t, _)| t.last_activity)
                .unwrap_or(0);
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
        let a_activity = all_threads
            .get(a)
            .map(|(t, _)| t.last_activity)
            .unwrap_or(0);
        let b_activity = all_threads
            .get(b)
            .map(|(t, _)| t.last_activity)
            .unwrap_or(0);
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
                thread_title: conv_match.thread.title.clone(),
                project_a_tag: conv_match.project_a_tag.clone(),
                project_name: conv_match.project_name.clone(),
                matching_messages: conv_match.matching_messages.clone(),
                title_matched: conv_match.title_matched,
                content_matched: conv_match.content_matched,
                id_matched: conv_match.id_matched,
                depth,
                matched_terms: conv_match.matched_terms.clone(),
            });
        } else if ancestor_ids.contains(conv_id) {
            // This is a context ancestor
            if let Some((thread, a_tag)) = all_threads.get(conv_id) {
                result.push(HierarchicalSearchItem::ContextAncestor {
                    thread: thread.clone(),
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
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_search_terms_single() {
        let terms = parse_search_terms("error");
        assert_eq!(terms, vec!["error"]);
    }

    #[test]
    fn test_parse_search_terms_multiple() {
        let terms = parse_search_terms("error+timeout");
        assert_eq!(terms, vec!["error", "timeout"]);
    }

    #[test]
    fn test_parse_search_terms_with_spaces() {
        let terms = parse_search_terms("  error + timeout  ");
        assert_eq!(terms, vec!["error", "timeout"]);
    }

    #[test]
    fn test_parse_search_terms_empty_parts() {
        // Empty parts between + should be ignored
        let terms = parse_search_terms("error++timeout");
        assert_eq!(terms, vec!["error", "timeout"]);
    }

    #[test]
    fn test_parse_search_terms_empty_query() {
        let terms = parse_search_terms("");
        assert!(terms.is_empty());
    }

    #[test]
    fn test_parse_search_terms_whitespace_only() {
        let terms = parse_search_terms("   ");
        assert!(terms.is_empty());
    }

    #[test]
    fn test_parse_search_terms_three_terms() {
        let terms = parse_search_terms("error+warning+info");
        assert_eq!(terms, vec!["error", "warning", "info"]);
    }

    #[test]
    fn test_parse_search_terms_lowercased() {
        let terms = parse_search_terms("Error+WARNING+Info");
        assert_eq!(terms, vec!["error", "warning", "info"]);
    }

    #[test]
    fn test_text_contains_term() {
        assert!(text_contains_term("This is an ERROR message", "error"));
        assert!(text_contains_term("ERROR at start", "error"));
        assert!(text_contains_term("at the end ERROR", "error"));
        assert!(!text_contains_term("This has no match", "error"));
    }

    #[test]
    fn test_text_starts_with_ascii() {
        assert!(text_starts_with_ascii("abc123", "ABC"));
        assert!(text_starts_with_ascii("ABC123", "abc"));
        assert!(text_starts_with_ascii("test", "TEST"));
        assert!(!text_starts_with_ascii("test", "abc"));
        assert!(!text_starts_with_ascii("ab", "abc"));
        assert!(text_starts_with_ascii("anything", ""));
    }

    // ========== Behavioral tests for scan_thread_for_terms ==========

    #[test]
    fn test_scan_single_term_matches_title() {
        let terms = vec!["error".to_string()];
        let messages: Vec<MessageContent> = vec![];

        let result = scan_thread_for_terms(
            "thread-123",
            "Error in production",
            "Some content without match",
            &messages,
            &terms,
        );

        assert!(result.all_terms_matched);
        assert!(result.title_matched());
        assert!(!result.content_matched());
        assert!(!result.id_matched());
        assert!(result.matching_messages.is_empty());
    }

    #[test]
    fn test_scan_single_term_matches_content() {
        let terms = vec!["timeout".to_string()];
        let messages: Vec<MessageContent> = vec![];

        let result = scan_thread_for_terms(
            "thread-123",
            "No match here",
            "Connection timeout occurred",
            &messages,
            &terms,
        );

        assert!(result.all_terms_matched);
        assert!(!result.title_matched());
        assert!(result.content_matched());
    }

    #[test]
    fn test_scan_single_term_matches_message() {
        let terms = vec!["critical".to_string()];
        let messages = vec![MessageContent {
            id: "msg-1",
            content: "This is critical information",
            pubkey: "author-1",
            created_at: 1000,
        }];

        let result = scan_thread_for_terms("thread-123", "No match", "No match", &messages, &terms);

        assert!(result.all_terms_matched);
        assert!(!result.title_matched());
        assert!(!result.content_matched());
        assert_eq!(result.matching_messages.len(), 1);
        assert!(result.matching_messages[0].content.contains("critical"));
    }

    #[test]
    fn test_scan_multi_term_and_semantics_all_in_title() {
        // Both terms in title: should match
        let terms = vec!["error".to_string(), "timeout".to_string()];
        let messages: Vec<MessageContent> = vec![];

        let result = scan_thread_for_terms(
            "thread-123",
            "Error due to timeout",
            "No match",
            &messages,
            &terms,
        );

        assert!(result.all_terms_matched);
        assert!(result.title_matched());
    }

    #[test]
    fn test_scan_multi_term_and_semantics_split_across_locations() {
        // "error" in title, "timeout" in message: should match (AND across conversation)
        let terms = vec!["error".to_string(), "timeout".to_string()];
        let messages = vec![MessageContent {
            id: "msg-1",
            content: "Timeout occurred",
            pubkey: "author-1",
            created_at: 1000,
        }];

        let result = scan_thread_for_terms(
            "thread-123",
            "Error in system",
            "No match in content",
            &messages,
            &terms,
        );

        assert!(result.all_terms_matched);
        assert!(result.title_matched());
        assert_eq!(result.matching_messages.len(), 1);
    }

    #[test]
    fn test_scan_multi_term_title_and_content_combined() {
        // "error" in title, "timeout" in content: both terms found so should match
        let terms = vec!["error".to_string(), "timeout".to_string()];
        let messages: Vec<MessageContent> = vec![];

        let result = scan_thread_for_terms(
            "thread-123",
            "Error in system",
            "No timeout here",
            &messages,
            &terms,
        );

        assert!(result.all_terms_matched);
    }

    #[test]
    fn test_scan_multi_term_and_semantics_truly_missing() {
        // Only "error" found, "missing" truly missing: should NOT match
        let terms = vec!["error".to_string(), "missing".to_string()];
        let messages: Vec<MessageContent> = vec![];

        let result = scan_thread_for_terms(
            "thread-123",
            "Error in system",
            "Some content here",
            &messages,
            &terms,
        );

        assert!(!result.all_terms_matched);
    }

    #[test]
    fn test_scan_multi_term_collects_all_matching_messages() {
        // Multi-term search: all messages matching ANY term should be collected
        let terms = vec!["error".to_string(), "warning".to_string()];
        let messages = vec![
            MessageContent {
                id: "msg-1",
                content: "This has an error",
                pubkey: "author-1",
                created_at: 1000,
            },
            MessageContent {
                id: "msg-2",
                content: "This has a warning",
                pubkey: "author-2",
                created_at: 2000,
            },
            MessageContent {
                id: "msg-3",
                content: "This is unrelated",
                pubkey: "author-3",
                created_at: 3000,
            },
        ];

        let result = scan_thread_for_terms("thread-123", "No match", "No match", &messages, &terms);

        assert!(result.all_terms_matched);
        // Should have 2 matching messages (error + warning), not the unrelated one
        assert_eq!(result.matching_messages.len(), 2);
    }

    #[test]
    fn test_scan_skips_root_message() {
        // Message with same ID as thread should be skipped
        let terms = vec!["secret".to_string()];
        let messages = vec![MessageContent {
            id: "thread-123", // Same as thread ID - should be skipped
            content: "This contains secret",
            pubkey: "author-1",
            created_at: 1000,
        }];

        let result = scan_thread_for_terms("thread-123", "No match", "No match", &messages, &terms);

        // Root message is skipped, so no match in messages
        assert!(!result.all_terms_matched);
        assert!(result.matching_messages.is_empty());
    }

    #[test]
    fn test_scan_id_prefix_match() {
        let terms = vec!["abc12".to_string()];
        let messages: Vec<MessageContent> = vec![];

        let result =
            scan_thread_for_terms("ABC123456789", "No match", "No match", &messages, &terms);

        assert!(result.all_terms_matched);
        assert!(result.id_matched());
    }

    #[test]
    fn test_scan_ascii_case_insensitive() {
        // Verify ASCII case-insensitive matching
        let terms = vec!["error".to_string()];
        let messages: Vec<MessageContent> = vec![];

        let result =
            scan_thread_for_terms("thread-123", "ERROR MESSAGE", "No match", &messages, &terms);

        assert!(result.all_terms_matched);
        assert!(result.title_matched());
    }

    #[test]
    fn test_scan_three_terms_all_required() {
        // Three terms: all must be found
        let terms = vec![
            "error".to_string(),
            "warning".to_string(),
            "info".to_string(),
        ];
        let messages = vec![
            MessageContent {
                id: "msg-1",
                content: "Error happened",
                pubkey: "author-1",
                created_at: 1000,
            },
            MessageContent {
                id: "msg-2",
                content: "Warning issued",
                pubkey: "author-2",
                created_at: 2000,
            },
        ];

        let result = scan_thread_for_terms(
            "thread-123",
            "Info available",
            "No match",
            &messages,
            &terms,
        );

        assert!(result.all_terms_matched);
        // All 3 terms found: info in title, error and warning in messages
    }

    #[test]
    fn test_scan_three_terms_one_missing() {
        // Three terms: if one is missing, no match
        let terms = vec![
            "error".to_string(),
            "warning".to_string(),
            "critical".to_string(),
        ];
        let messages = vec![MessageContent {
            id: "msg-1",
            content: "Error happened",
            pubkey: "author-1",
            created_at: 1000,
        }];

        let result =
            scan_thread_for_terms("thread-123", "Warning here", "No match", &messages, &terms);

        // "critical" is missing - should not match
        assert!(!result.all_terms_matched);
    }

    // ==========================================================================
    // Integration-level behavioral tests for search functions
    // ==========================================================================
    // Note: Full integration tests for search_conversations and search_conversations_hierarchical
    // would require a complete AppDataStore with nostrdb. These tests verify the core matching
    // behavior through the shared scan_thread_for_terms function, which both search functions use.
    // The tests below verify the key behaviors: AND semantics, message collection, and term tracking.

    #[test]
    fn test_and_semantics_term_in_title_only() {
        // Verify AND semantics: both terms must appear somewhere in the conversation
        // This tests the core behavior used by both search_conversations and search_conversations_hierarchical
        let terms = vec!["api".to_string(), "auth".to_string()];

        // Only "api" in title, no "auth" anywhere
        let result = scan_thread_for_terms(
            "conv-1",
            "API Design Document",
            "Some content about endpoints",
            &[],
            &terms,
        );

        // Should NOT match because "auth" is missing
        assert!(
            !result.all_terms_matched,
            "Should not match when one term is missing"
        );
    }

    #[test]
    fn test_and_semantics_term_in_reply_only() {
        // Verify AND semantics: a term can be found in a reply to satisfy the AND
        let terms = vec!["api".to_string(), "auth".to_string()];
        let messages = vec![MessageContent {
            id: "reply-1",
            content: "Added auth middleware",
            pubkey: "dev-1",
            created_at: 1000,
        }];

        // "api" in title, "auth" only in reply
        let result =
            scan_thread_for_terms("conv-1", "API Design", "Some content", &messages, &terms);

        // Should match because both terms are found across conversation
        assert!(
            result.all_terms_matched,
            "Should match when both terms found across title and reply"
        );
        assert!(result.title_matched(), "Title should be marked as matched");
        assert_eq!(
            result.matching_messages.len(),
            1,
            "Reply containing auth should be in matching_messages"
        );
    }

    #[test]
    fn test_message_collection_all_matches() {
        // Verify that ALL messages matching ANY term are collected (OR for display)
        // This is important for search_conversations_hierarchical result display
        let terms = vec!["error".to_string(), "crash".to_string()];
        let messages = vec![
            MessageContent {
                id: "msg-1",
                content: "First error occurred",
                pubkey: "dev-1",
                created_at: 1000,
            },
            MessageContent {
                id: "msg-2",
                content: "Unrelated message",
                pubkey: "dev-2",
                created_at: 2000,
            },
            MessageContent {
                id: "msg-3",
                content: "App crash detected",
                pubkey: "dev-1",
                created_at: 3000,
            },
            MessageContent {
                id: "msg-4",
                content: "Another error here and crash too",
                pubkey: "dev-3",
                created_at: 4000,
            },
        ];

        let result =
            scan_thread_for_terms("conv-1", "Bug Report", "System issues", &messages, &terms);

        assert!(result.all_terms_matched);
        // Should collect 3 messages: msg-1 (error), msg-3 (crash), msg-4 (both)
        assert_eq!(
            result.matching_messages.len(),
            3,
            "Should collect all messages that match any term"
        );
    }

    #[test]
    fn test_term_tracking_across_locations() {
        // Verify term indices are properly tracked (used for highlighting)
        let terms = vec!["bug".to_string(), "fix".to_string(), "test".to_string()];
        let messages = vec![MessageContent {
            id: "msg-1",
            content: "Added a test for the issue",
            pubkey: "dev-1",
            created_at: 1000,
        }];

        let result = scan_thread_for_terms(
            "conv-1",
            "Bug Fix Required", // "bug" and "fix" here
            "Some content",
            &messages,
            &terms,
        );

        assert!(result.all_terms_matched);
        assert!(
            result.terms_in_title.contains(&0),
            "Term 'bug' should be tracked in title"
        );
        assert!(
            result.terms_in_title.contains(&1),
            "Term 'fix' should be tracked in title"
        );
        assert!(
            !result.matching_messages.is_empty(),
            "Term 'test' should be found in messages"
        );
    }

    #[test]
    fn test_empty_terms_no_match() {
        // Verify empty terms result in no match (edge case)
        let terms: Vec<String> = vec![];

        let result = scan_thread_for_terms("conv-1", "Any Title", "Any content", &[], &terms);

        assert!(!result.all_terms_matched, "Empty terms should not match");
    }

    #[test]
    fn test_single_term_or_semantics() {
        // Verify single-term search uses OR semantics (any location matches)
        let terms = vec!["error".to_string()];

        // Term only in content
        let result =
            scan_thread_for_terms("conv-1", "System Report", "An error occurred", &[], &terms);

        assert!(
            result.all_terms_matched,
            "Single term in content should match"
        );
        assert!(result.content_matched());
        assert!(!result.title_matched());
    }

    #[test]
    fn test_delete_char_at_end() {
        let mut state = SidebarSearchState::new();
        state.query = "test".to_string();
        state.cursor = 4;
        state.delete_char();
        assert_eq!(state.query, "tes");
        assert_eq!(state.cursor, 3);
    }

    #[test]
    fn test_delete_char_multibyte() {
        let mut state = SidebarSearchState::new();
        state.query = "hello🔥".to_string();
        state.cursor = 6;
        state.delete_char();
        assert_eq!(state.query, "hello");
        assert_eq!(state.cursor, 5);
    }

    #[test]
    fn test_delete_char_with_misaligned_cursor() {
        // Reproduces the panic scenario: cursor beyond string length
        let mut state = SidebarSearchState::new();
        state.query = "test".to_string();
        state.cursor = 5; // Beyond actual length
        state.delete_char();
        assert_eq!(state.query, "tes");
        assert_eq!(state.cursor, 4);
    }
}
