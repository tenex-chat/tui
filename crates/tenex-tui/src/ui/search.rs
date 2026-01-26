//! Sidebar search state and logic for Home and Reports tabs.
//!
//! This module provides search functionality that appears in the sidebar
//! when activated via Ctrl+T + /. It searches conversations and their replies,
//! highlighting matching terms.

use crate::models::Thread;
use crate::store::AppDataStore;
use std::cell::Ref;
use std::collections::HashSet;

/// State for the sidebar search input
#[derive(Debug, Clone, Default)]
pub struct SidebarSearchState {
    /// Whether the search input is visible
    pub visible: bool,
    /// Current search query
    pub query: String,
    /// Cursor position within query
    pub cursor: usize,
    /// Cached search results for conversations
    pub results: Vec<SearchResult>,
    /// Cached search results for reports
    pub report_results: Vec<tenex_core::models::Report>,
    /// Selected result index
    pub selected_index: usize,
    /// Scroll offset for long results lists
    pub scroll_offset: usize,
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
            self.report_results.clear();
            self.selected_index = 0;
            self.scroll_offset = 0;
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
    pub fn move_selection_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down in results (for conversations)
    pub fn move_selection_down(&mut self) {
        let count = self.results.len();
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

    /// Get currently selected result
    pub fn selected_result(&self) -> Option<&SearchResult> {
        self.results.get(self.selected_index)
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
            // Check conversation ID
            let id_matches = thread.id.to_lowercase().contains(&filter);

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
