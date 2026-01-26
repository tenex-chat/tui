//! Sidebar search state and logic for Home and Reports tabs.
//!
//! This module provides search functionality that appears in the sidebar
//! when activated via Ctrl+T + /. It searches conversations and their replies,
//! highlighting matching terms.

use crate::models::Thread;
use crate::store::AppDataStore;
use ratatui::style::Style;
use ratatui::text::Span;
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
    /// Cached search results
    pub results: Vec<SearchResult>,
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
            self.selected_index = 0;
            self.scroll_offset = 0;
        }
    }

    /// Insert a character at the cursor position
    pub fn insert_char(&mut self, c: char) {
        self.query.insert(self.cursor, c);
        self.cursor += 1;
    }

    /// Delete character before cursor
    pub fn delete_char(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
            self.query.remove(self.cursor);
        }
    }

    /// Move cursor left
    pub fn move_cursor_left(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    /// Move cursor right
    pub fn move_cursor_right(&mut self) {
        if self.cursor < self.query.len() {
            self.cursor += 1;
        }
    }

    /// Move selection up in results
    pub fn move_selection_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down in results
    pub fn move_selection_down(&mut self) {
        if !self.results.is_empty() && self.selected_index < self.results.len() - 1 {
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
    /// Reply event ID
    pub event_id: String,
    /// Reply content
    pub content: String,
    /// Author pubkey
    pub author_pubkey: String,
    /// Character ranges where matches occur
    pub match_ranges: Vec<(usize, usize)>,
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
pub fn search_conversations(
    query: &str,
    store: &Ref<AppDataStore>,
    visible_projects: &HashSet<String>,
) -> Vec<SearchResult> {
    if query.trim().is_empty() {
        return vec![];
    }

    let filter = query.to_lowercase();
    let mut results = Vec::new();
    let mut seen_threads: HashSet<String> = HashSet::new();

    // Search through all projects
    for project in store.get_projects() {
        let a_tag = project.a_tag();

        // Skip projects not in visible_projects (if any are selected)
        if !visible_projects.is_empty() && !visible_projects.contains(&a_tag) {
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
                                    event_id: msg.id.clone(),
                                    content: msg.content.clone(),
                                    author_pubkey: msg.pubkey.clone(),
                                    match_ranges: find_match_ranges(&msg.content, &filter),
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
                                event_id: msg.id.clone(),
                                content: msg.content.clone(),
                                author_pubkey: msg.pubkey.clone(),
                                match_ranges: find_match_ranges(&msg.content, &filter),
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
pub fn search_reports(
    query: &str,
    store: &Ref<AppDataStore>,
    visible_projects: &HashSet<String>,
) -> Vec<tenex_core::models::Report> {
    if query.trim().is_empty() {
        return vec![];
    }

    let filter = query.to_lowercase();
    let mut results = Vec::new();

    for report in store.get_reports() {
        // Skip reports not in visible_projects (if any are selected)
        if !visible_projects.is_empty() && !visible_projects.contains(&report.project_a_tag) {
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

/// Find all character ranges where the query matches in the content
fn find_match_ranges(content: &str, query: &str) -> Vec<(usize, usize)> {
    let content_lower = content.to_lowercase();
    let query_lower = query.to_lowercase();
    let mut ranges = Vec::new();
    let mut start = 0;

    while let Some(pos) = content_lower[start..].find(&query_lower) {
        let match_start = start + pos;
        let match_end = match_start + query.len();
        ranges.push((match_start, match_end));
        start = match_end;
    }

    ranges
}

/// Create highlighted spans for text with match ranges
///
/// Returns a vector of Spans with matches highlighted
pub fn highlight_matches<'a>(
    text: &'a str,
    query: &str,
    normal_style: Style,
    highlight_style: Style,
    max_chars: usize,
) -> Vec<Span<'a>> {
    let mut spans = Vec::new();

    if query.is_empty() {
        let truncated: String = text.chars().take(max_chars).collect();
        spans.push(Span::styled(truncated, normal_style));
        return spans;
    }

    let text_lower = text.to_lowercase();
    let query_lower = query.to_lowercase();

    // Find first match to center the preview around it
    if let Some(first_match_pos) = text_lower.find(&query_lower) {
        // Calculate window to show around the match
        let context_chars = max_chars.saturating_sub(query.len()) / 2;
        let window_start = first_match_pos.saturating_sub(context_chars);
        let window_end = (first_match_pos + query.len() + context_chars).min(text.len());

        // Add ellipsis if we're not starting from the beginning
        if window_start > 0 {
            spans.push(Span::styled("...", normal_style));
        }

        // Extract the window of text
        let window: String = text.chars().skip(window_start).take(window_end - window_start).collect();
        let window_lower = window.to_lowercase();

        // Highlight matches within the window
        let mut last_end = 0;
        let mut start = 0;

        while let Some(pos) = window_lower[start..].find(&query_lower) {
            let match_start = start + pos;
            let match_end = match_start + query.len();

            // Text before match
            if match_start > last_end {
                let before: String = window.chars().skip(last_end).take(match_start - last_end).collect();
                spans.push(Span::styled(before, normal_style));
            }

            // The match itself (preserve original case)
            let matched: String = window.chars().skip(match_start).take(query.len()).collect();
            spans.push(Span::styled(matched, highlight_style));

            last_end = match_end;
            start = match_end;
        }

        // Remaining text after last match
        if last_end < window.len() {
            let after: String = window.chars().skip(last_end).collect();
            spans.push(Span::styled(after, normal_style));
        }

        // Add ellipsis if we're not showing the end
        if window_end < text.len() {
            spans.push(Span::styled("...", normal_style));
        }
    } else {
        // No match found, just show truncated text
        let truncated: String = text.chars().take(max_chars).collect();
        spans.push(Span::styled(truncated, normal_style));
    }

    spans
}
