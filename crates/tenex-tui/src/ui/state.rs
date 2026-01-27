//! Extracted state types for the App struct.
//!
//! This module contains self-contained state machines that have been extracted
//! from the monolithic App struct to improve encapsulation and testability.

use crate::ui::text_editor::TextEditor;

/// State for in-conversation search mode.
///
/// This is a self-contained state machine that manages searching within
/// a conversation. It tracks the current query, match locations, and
/// navigation through matches.
#[derive(Debug, Clone, Default)]
pub struct ChatSearchState {
    /// Whether search mode is active
    pub active: bool,
    /// Current search query
    pub query: String,
    /// Index of current match being viewed (0-based)
    pub current_match: usize,
    /// Total number of matches found
    pub total_matches: usize,
    /// Message IDs that contain matches, with match positions
    pub match_locations: Vec<ChatSearchMatch>,
}

impl ChatSearchState {
    /// Create a new inactive search state
    pub fn new() -> Self {
        Self::default()
    }

    /// Enter search mode
    pub fn enter(&mut self) {
        self.active = true;
        self.query.clear();
        self.current_match = 0;
        self.total_matches = 0;
        self.match_locations.clear();
    }

    /// Exit search mode and reset state
    pub fn exit(&mut self) {
        self.active = false;
        self.query.clear();
        self.current_match = 0;
        self.total_matches = 0;
        self.match_locations.clear();
    }

    /// Check if there are any matches
    pub fn has_matches(&self) -> bool {
        self.total_matches > 0
    }

    /// Navigate to the next match
    pub fn next_match(&mut self) {
        if self.total_matches > 0 {
            self.current_match = (self.current_match + 1) % self.total_matches;
        }
    }

    /// Navigate to the previous match
    pub fn prev_match(&mut self) {
        if self.total_matches > 0 {
            if self.current_match == 0 {
                self.current_match = self.total_matches - 1;
            } else {
                self.current_match -= 1;
            }
        }
    }

    /// Get the current match location, if any
    pub fn current_location(&self) -> Option<&ChatSearchMatch> {
        self.match_locations.get(self.current_match)
    }

    /// Update match locations from a new search
    pub fn set_matches(&mut self, matches: Vec<ChatSearchMatch>) {
        self.total_matches = matches.len();
        self.match_locations = matches;
        // Reset to first match if we had a selection beyond new count
        if self.current_match >= self.total_matches && self.total_matches > 0 {
            self.current_match = 0;
        }
    }
}

/// A single search match location in a conversation
#[derive(Debug, Clone)]
pub struct ChatSearchMatch {
    /// Message ID containing the match
    pub message_id: String,
    /// Character offset where match starts in the message content
    pub start_offset: usize,
    /// Length of the match
    pub length: usize,
}

impl ChatSearchMatch {
    pub fn new(message_id: String, start_offset: usize, length: usize) -> Self {
        Self {
            message_id,
            start_offset,
            length,
        }
    }
}

// =============================================================================
// TAB MANAGEMENT
// =============================================================================

/// Maximum number of open tabs (matches 1-9 shortcuts)
pub const MAX_TABS: usize = 9;

/// Entry in the delegation navigation stack.
/// Stores state needed to return to a parent conversation.
#[derive(Debug, Clone)]
pub struct NavigationStackEntry {
    /// Thread ID of the parent conversation
    pub thread_id: String,
    /// Thread title
    pub thread_title: String,
    /// Project a_tag
    pub project_a_tag: String,
    /// Scroll offset when navigating away
    pub scroll_offset: usize,
    /// Selected message index when navigating away
    pub selected_message_index: usize,
}

/// Per-tab message history state (isolated from other tabs)
#[derive(Debug, Clone, Default)]
pub struct TabMessageHistory {
    /// Sent message history for this tab (most recent last, max 50)
    pub messages: Vec<String>,
    /// Current index in history (None = typing new message)
    pub index: Option<usize>,
    /// Draft preserved when browsing history
    pub draft: Option<String>,
}

impl TabMessageHistory {
    /// Maximum number of messages to keep in history
    pub const MAX_HISTORY: usize = 50;

    /// Add a message to history
    pub fn add(&mut self, message: String) {
        if message.trim().is_empty() {
            return;
        }
        // Avoid duplicates at the end
        if self.messages.last().map(|s| s.as_str()) != Some(message.trim()) {
            self.messages.push(message);
            // Limit to max entries
            if self.messages.len() > Self::MAX_HISTORY {
                self.messages.remove(0);
            }
        }
        // Reset history navigation
        self.index = None;
        self.draft = None;
    }

    /// Check if currently browsing history
    pub fn is_browsing(&self) -> bool {
        self.index.is_some()
    }

    /// Exit history mode
    pub fn exit(&mut self) {
        self.index = None;
        self.draft = None;
    }
}

/// An open tab representing a thread or draft conversation
#[derive(Debug, Clone)]
pub struct OpenTab {
    /// Thread ID (empty string for draft tabs)
    pub thread_id: String,
    /// Title displayed in the tab bar
    pub thread_title: String,
    /// Project this tab belongs to
    pub project_a_tag: String,
    /// Whether this tab has unread messages
    pub has_unread: bool,
    /// Whether the last message in this tab p-tags the current user (waiting for response)
    /// This takes priority over `has_unread` for visual indicators
    pub waiting_for_user: bool,
    /// Draft ID for new conversations not yet sent (None for real threads)
    pub draft_id: Option<String>,
    /// Navigation stack for drilling into delegations.
    /// Each entry represents a parent conversation we can return to with Esc.
    pub navigation_stack: Vec<NavigationStackEntry>,
    /// Per-tab message history (isolated from other tabs)
    pub message_history: TabMessageHistory,
    /// Per-tab chat search state (isolated from other tabs)
    pub chat_search: ChatSearchState,
    /// Per-tab selected nudge IDs (isolated from other tabs)
    pub selected_nudge_ids: Vec<String>,
    /// Per-tab text editor for chat input (ISOLATED from other tabs)
    /// This ensures each tab has its own input state - no cross-tab contamination
    pub editor: TextEditor,
    /// Reference conversation ID for the "context" tag when creating a new thread
    /// This is set when using "Reference conversation" command and consumed when sending
    /// NOTE: Uses "context" instead of "q" because "q" is reserved for delegation/child links
    pub reference_conversation_id: Option<String>,
}

impl OpenTab {
    /// Check if this is a draft tab (no thread created yet)
    pub fn is_draft(&self) -> bool {
        self.draft_id.is_some()
    }

    /// Clear attention flags (unread and waiting_for_user) when user views this tab
    pub fn clear_attention_flags(&mut self) {
        self.has_unread = false;
        self.waiting_for_user = false;
    }

    /// Create a new tab for an existing thread
    pub fn for_thread(thread_id: String, thread_title: String, project_a_tag: String) -> Self {
        Self {
            thread_id,
            thread_title,
            project_a_tag,
            has_unread: false,
            waiting_for_user: false,
            draft_id: None,
            navigation_stack: Vec::new(),
            message_history: TabMessageHistory::default(),
            chat_search: ChatSearchState::default(),
            selected_nudge_ids: Vec::new(),
            editor: TextEditor::new(),
            reference_conversation_id: None,
        }
    }

    /// Create a draft tab for a new conversation
    pub fn draft(project_a_tag: String, project_name: String) -> Self {
        let draft_id = format!(
            "draft-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
        Self {
            thread_id: String::new(),
            thread_title: format!("New: {}", project_name),
            project_a_tag,
            has_unread: false,
            waiting_for_user: false,
            draft_id: Some(draft_id),
            navigation_stack: Vec::new(),
            message_history: TabMessageHistory::default(),
            chat_search: ChatSearchState::default(),
            selected_nudge_ids: Vec::new(),
            editor: TextEditor::new(),
            reference_conversation_id: None,
        }
    }
}

/// Represents a location in the view history (either Home or a specific tab)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewLocation {
    /// The Home view
    Home,
    /// A specific tab by its index
    Tab(usize),
}

/// Manages open tabs with LRU eviction and history tracking.
///
/// This is a self-contained state machine for tab management.
/// It handles:
/// - Opening and closing tabs (max 9)
/// - Tab history for Alt+Tab cycling
/// - Draft tab management
/// - Unread indicators
#[derive(Debug, Clone, Default)]
pub struct TabManager {
    /// Open tabs (max 9, LRU eviction)
    tabs: Vec<OpenTab>,
    /// Index of the active tab
    active_index: usize,
    /// Tab visit history for Alt+Tab cycling (most recent last)
    history: Vec<usize>,
    /// View navigation history including Home (most recent last)
    /// Used to navigate back to previous view when closing a tab
    view_history: Vec<ViewLocation>,
    /// Whether the tab modal is showing
    pub modal_open: bool,
    /// Selected index in tab modal
    pub modal_index: usize,
}

impl TabManager {
    /// Maximum history entries to keep
    const MAX_HISTORY: usize = 20;

    pub fn new() -> Self {
        Self::default()
    }

    /// Get all open tabs
    pub fn tabs(&self) -> &[OpenTab] {
        &self.tabs
    }

    /// Get mutable reference to all open tabs
    pub fn tabs_mut(&mut self) -> &mut Vec<OpenTab> {
        &mut self.tabs
    }

    /// Get the active tab index
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Set the active tab index directly (use with care)
    pub fn set_active_index(&mut self, index: usize) {
        if index < self.tabs.len() {
            self.active_index = index;
        }
    }

    /// Get the currently active tab
    pub fn active_tab(&self) -> Option<&OpenTab> {
        self.tabs.get(self.active_index)
    }

    /// Get mutable reference to the active tab
    pub fn active_tab_mut(&mut self) -> Option<&mut OpenTab> {
        self.tabs.get_mut(self.active_index)
    }

    /// Check if there are any open tabs
    pub fn is_empty(&self) -> bool {
        self.tabs.is_empty()
    }

    /// Get the number of open tabs
    pub fn len(&self) -> usize {
        self.tabs.len()
    }

    /// Find a tab by thread ID
    pub fn find_by_thread_id(&self, thread_id: &str) -> Option<usize> {
        self.tabs.iter().position(|t| t.thread_id == thread_id)
    }

    /// Find a draft tab for a project
    pub fn find_draft_for_project(&self, project_a_tag: &str) -> Option<(usize, &str)> {
        self.tabs.iter().enumerate().find_map(|(idx, t)| {
            if t.is_draft() && t.project_a_tag == project_a_tag {
                t.draft_id.as_ref().map(|id| (idx, id.as_str()))
            } else {
                None
            }
        })
    }

    /// Open a thread in a tab (or switch to it if already open).
    /// Returns the tab index.
    pub fn open_thread(
        &mut self,
        thread_id: String,
        thread_title: String,
        project_a_tag: String,
    ) -> usize {
        // Check if already open
        if let Some(idx) = self.find_by_thread_id(&thread_id) {
            self.tabs[idx].clear_attention_flags();
            self.active_index = idx;
            return idx;
        }

        // Create new tab
        let tab = OpenTab::for_thread(thread_id, thread_title, project_a_tag);

        // Evict if at capacity
        self.evict_if_needed(false);

        self.tabs.push(tab);
        self.active_index = self.tabs.len() - 1;
        self.active_index
    }

    /// Open a draft tab for a new conversation.
    /// Returns the tab index.
    /// Always creates a new draft tab - multiple drafts per project are allowed.
    pub fn open_draft(&mut self, project_a_tag: String, project_name: String) -> usize {
        // Create new draft tab (always - allow multiple drafts per project)
        let tab = OpenTab::draft(project_a_tag, project_name);

        // Evict if at capacity (prefer non-drafts)
        self.evict_if_needed(true);

        self.tabs.push(tab);
        self.active_index = self.tabs.len() - 1;
        self.active_index
    }

    /// Convert a draft tab to a real tab when thread is created
    pub fn convert_draft(&mut self, draft_id: &str, thread_id: String, thread_title: String) {
        if let Some(tab) = self
            .tabs
            .iter_mut()
            .find(|t| t.draft_id.as_deref() == Some(draft_id))
        {
            tab.thread_id = thread_id;
            tab.thread_title = thread_title;
            tab.draft_id = None;
        }
    }

    /// Switch to a specific tab by index.
    /// Returns true if switch was successful.
    pub fn switch_to(&mut self, index: usize) -> bool {
        if index >= self.tabs.len() {
            return false;
        }

        self.push_history(index);
        self.push_view_history(ViewLocation::Tab(index));
        self.active_index = index;
        self.tabs[index].clear_attention_flags();
        true
    }

    /// Record that the user navigated to Home view
    /// This is called by the App when navigating to Home to track view history
    pub fn record_home_visit(&mut self) {
        self.push_view_history(ViewLocation::Home);
    }

    /// Close the current tab.
    /// Returns a tuple of (removed_tab, previous_view_location).
    /// previous_view_location is the view to navigate back to (from history).
    /// Returns None for previous_view_location if no tabs remain (should go to Home).
    pub fn close_current(&mut self) -> (Option<OpenTab>, Option<ViewLocation>) {
        if self.tabs.is_empty() {
            return (None, None);
        }

        let removed_index = self.active_index;
        let removed_tab = self.tabs.remove(removed_index);
        self.cleanup_history(removed_index);

        // Get the previous view from history BEFORE cleaning up view history
        let previous_view = self.pop_previous_view();

        // Clean up view history for the removed tab
        self.cleanup_view_history(removed_index);

        if self.tabs.is_empty() {
            self.active_index = 0;
            // No tabs remain, should go to Home
            return (Some(removed_tab), Some(ViewLocation::Home));
        }

        // Determine where to go based on history
        let target_view = match previous_view {
            Some(ViewLocation::Home) => {
                // Go back to Home
                Some(ViewLocation::Home)
            }
            Some(ViewLocation::Tab(idx)) => {
                // Go back to the previous tab (index already adjusted by cleanup_view_history)
                if idx < self.tabs.len() {
                    self.active_index = idx;
                    Some(ViewLocation::Tab(idx))
                } else {
                    // Fallback: tab index is now invalid, go to last tab
                    self.active_index = self.tabs.len() - 1;
                    Some(ViewLocation::Tab(self.active_index))
                }
            }
            None => {
                // No history, fallback to adjacent tab behavior
                if self.active_index >= self.tabs.len() {
                    self.active_index = self.tabs.len() - 1;
                }
                Some(ViewLocation::Tab(self.active_index))
            }
        };

        (Some(removed_tab), target_view)
    }

    /// Close a tab at a specific index.
    /// Returns a tuple of (removed_tab, new_active_index).
    /// removed_tab is None if the index was out of bounds.
    /// new_active_index is None if no tabs remain.
    pub fn close_at(&mut self, index: usize) -> (Option<OpenTab>, Option<usize>) {
        if index >= self.tabs.len() {
            return (None, Some(self.active_index));
        }

        let removed_tab = self.tabs.remove(index);
        self.cleanup_history(index);
        self.cleanup_view_history(index);

        if self.tabs.is_empty() {
            self.active_index = 0;
            return (Some(removed_tab), None);
        }

        // Adjust active index if needed
        if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        } else if self.active_index > index {
            self.active_index -= 1;
        }

        // Adjust modal index
        if self.modal_index >= self.tabs.len() {
            self.modal_index = self.tabs.len().saturating_sub(1);
        }

        (Some(removed_tab), Some(self.active_index))
    }

    /// Switch to next tab (wraps around)
    pub fn next(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        let next = (self.active_index + 1) % self.tabs.len();
        self.switch_to(next);
    }

    /// Switch to previous tab (wraps around)
    pub fn prev(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        let prev = if self.active_index == 0 {
            self.tabs.len() - 1
        } else {
            self.active_index - 1
        };
        self.switch_to(prev);
    }

    /// Cycle through tab history (Alt+Tab behavior)
    pub fn cycle_history_forward(&mut self) {
        if self.history.len() < 2 {
            self.next();
            return;
        }

        let history_len = self.history.len();
        if history_len >= 2 {
            let prev_index = self.history[history_len - 2];
            if prev_index < self.tabs.len() {
                self.switch_to(prev_index);
            }
        }
    }

    /// Mark a thread as having unread messages (if open but not active)
    pub fn mark_unread(&mut self, thread_id: &str) {
        for (idx, tab) in self.tabs.iter_mut().enumerate() {
            if tab.thread_id == thread_id && idx != self.active_index {
                tab.has_unread = true;
            }
        }
    }

    /// Mark a thread as waiting for user response (if open but not active)
    /// This is triggered when the last message p-tags the current user
    pub fn mark_waiting_for_user(&mut self, thread_id: &str) {
        for (idx, tab) in self.tabs.iter_mut().enumerate() {
            if tab.thread_id == thread_id && idx != self.active_index {
                tab.waiting_for_user = true;
            }
        }
    }

    /// Clear the waiting_for_user state for a thread
    /// Called when the user views the tab
    pub fn clear_waiting_for_user(&mut self, thread_id: &str) {
        for tab in self.tabs.iter_mut() {
            if tab.thread_id == thread_id {
                tab.waiting_for_user = false;
            }
        }
    }

    /// Open the tab modal
    pub fn open_modal(&mut self) {
        self.modal_open = true;
        self.modal_index = self.active_index;
    }

    /// Close the tab modal
    pub fn close_modal(&mut self) {
        self.modal_open = false;
    }

    // --- Private helpers ---

    fn push_history(&mut self, index: usize) {
        self.history.retain(|&i| i != index);
        self.history.push(index);
        if self.history.len() > Self::MAX_HISTORY {
            self.history.remove(0);
        }
    }

    fn cleanup_history(&mut self, removed_index: usize) {
        self.history.retain(|&i| i != removed_index);
        for idx in self.history.iter_mut() {
            if *idx > removed_index {
                *idx -= 1;
            }
        }
    }

    /// Push a view location to the view history
    fn push_view_history(&mut self, location: ViewLocation) {
        // Don't push duplicates if already the last entry
        if self.view_history.last() == Some(&location) {
            return;
        }
        self.view_history.push(location);
        if self.view_history.len() > Self::MAX_HISTORY {
            self.view_history.remove(0);
        }
    }

    /// Clean up view history after a tab is removed
    fn cleanup_view_history(&mut self, removed_index: usize) {
        // Remove references to the removed tab
        self.view_history.retain(|loc| *loc != ViewLocation::Tab(removed_index));
        // Adjust indices for tabs that shifted down
        for loc in self.view_history.iter_mut() {
            if let ViewLocation::Tab(idx) = loc {
                if *idx > removed_index {
                    *idx -= 1;
                }
            }
        }
    }

    /// Get the previous view location from history (for navigating back when closing a tab)
    fn pop_previous_view(&mut self) -> Option<ViewLocation> {
        // Pop the current location (which is the tab being closed)
        self.view_history.pop();
        // Return the previous location (don't pop it - it becomes current)
        self.view_history.last().copied()
    }

    fn evict_if_needed(&mut self, prefer_non_drafts: bool) {
        if self.tabs.len() < MAX_TABS {
            return;
        }

        if prefer_non_drafts {
            // Prefer removing non-draft tabs first
            if let Some(idx) = self.tabs.iter().position(|t| !t.is_draft()) {
                self.tabs.remove(idx);
                if self.active_index > 0 && self.active_index >= idx {
                    self.active_index -= 1;
                }
                self.cleanup_history(idx);
                return;
            }
        }

        // Remove oldest (leftmost) tab
        self.tabs.remove(0);
        if self.active_index > 0 {
            self.active_index -= 1;
        }
        self.cleanup_history(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chat_search_navigation() {
        let mut state = ChatSearchState::new();
        state.enter();

        // Add some matches
        state.set_matches(vec![
            ChatSearchMatch::new("msg1".to_string(), 0, 5),
            ChatSearchMatch::new("msg2".to_string(), 10, 5),
            ChatSearchMatch::new("msg3".to_string(), 20, 5),
        ]);

        assert_eq!(state.current_match, 0);
        assert!(state.has_matches());

        state.next_match();
        assert_eq!(state.current_match, 1);

        state.next_match();
        assert_eq!(state.current_match, 2);

        state.next_match();
        assert_eq!(state.current_match, 0); // Wraps around

        state.prev_match();
        assert_eq!(state.current_match, 2); // Wraps backward
    }

    #[test]
    fn test_tab_manager_basic() {
        let mut tabs = TabManager::new();
        assert!(tabs.is_empty());

        // Open a thread
        let idx = tabs.open_thread(
            "thread1".to_string(),
            "Thread 1".to_string(),
            "project1".to_string(),
        );
        assert_eq!(idx, 0);
        assert_eq!(tabs.len(), 1);
        assert_eq!(tabs.active_index(), 0);

        // Open another thread
        let idx = tabs.open_thread(
            "thread2".to_string(),
            "Thread 2".to_string(),
            "project1".to_string(),
        );
        assert_eq!(idx, 1);
        assert_eq!(tabs.len(), 2);
        assert_eq!(tabs.active_index(), 1);

        // Reopen first thread should switch to it
        let idx = tabs.open_thread(
            "thread1".to_string(),
            "Thread 1".to_string(),
            "project1".to_string(),
        );
        assert_eq!(idx, 0);
        assert_eq!(tabs.len(), 2); // No new tab created
        assert_eq!(tabs.active_index(), 0);
    }

    #[test]
    fn test_tab_manager_drafts() {
        let mut tabs = TabManager::new();

        // Open a draft
        let idx = tabs.open_draft("project1".to_string(), "Project 1".to_string());
        assert_eq!(idx, 0);
        assert!(tabs.active_tab().unwrap().is_draft());

        // Opening same project draft creates a new draft (multiple drafts allowed)
        let idx = tabs.open_draft("project1".to_string(), "Project 1".to_string());
        assert_eq!(idx, 1);
        assert_eq!(tabs.len(), 2);

        // Different project should also create new draft
        let idx = tabs.open_draft("project2".to_string(), "Project 2".to_string());
        assert_eq!(idx, 2);
        assert_eq!(tabs.len(), 3);

        // Convert first draft to real tab
        let draft_id = tabs.tabs()[0].draft_id.clone().unwrap();
        tabs.convert_draft(&draft_id, "thread1".to_string(), "Real Thread".to_string());
        assert!(!tabs.tabs()[0].is_draft());
        assert_eq!(tabs.tabs()[0].thread_id, "thread1");
    }

    #[test]
    fn test_tab_manager_navigation() {
        let mut tabs = TabManager::new();

        tabs.open_thread("t1".to_string(), "T1".to_string(), "p".to_string());
        tabs.open_thread("t2".to_string(), "T2".to_string(), "p".to_string());
        tabs.open_thread("t3".to_string(), "T3".to_string(), "p".to_string());

        assert_eq!(tabs.active_index(), 2);

        tabs.prev();
        assert_eq!(tabs.active_index(), 1);

        tabs.prev();
        assert_eq!(tabs.active_index(), 0);

        tabs.prev(); // Wraps around
        assert_eq!(tabs.active_index(), 2);

        tabs.next();
        assert_eq!(tabs.active_index(), 0);
    }

    #[test]
    fn test_tab_manager_close() {
        let mut tabs = TabManager::new();

        tabs.open_thread("t1".to_string(), "T1".to_string(), "p".to_string());
        tabs.open_thread("t2".to_string(), "T2".to_string(), "p".to_string());
        tabs.open_thread("t3".to_string(), "T3".to_string(), "p".to_string());

        // Close middle tab - should go back to previously viewed tab
        tabs.switch_to(0); // View t1
        tabs.switch_to(1); // View t2
        let (removed_tab, prev_view) = tabs.close_current();
        assert!(removed_tab.is_some());
        assert_eq!(removed_tab.unwrap().thread_id, "t2");
        // Should return to previous view (t1, now at index 0)
        assert_eq!(prev_view, Some(ViewLocation::Tab(0)));
        assert_eq!(tabs.len(), 2);

        // Close all
        tabs.close_current();
        tabs.close_current();
        assert!(tabs.is_empty());
        let (removed_tab, prev_view) = tabs.close_current();
        assert!(removed_tab.is_none());
        assert_eq!(prev_view, None);
    }

    #[test]
    fn test_tab_manager_close_returns_to_home() {
        let mut tabs = TabManager::new();

        // Record Home visit first
        tabs.record_home_visit();

        // Open a tab and switch to it
        tabs.open_thread("t1".to_string(), "T1".to_string(), "p".to_string());
        tabs.switch_to(0);

        // Close the tab - should go back to Home
        let (removed_tab, prev_view) = tabs.close_current();
        assert!(removed_tab.is_some());
        assert_eq!(prev_view, Some(ViewLocation::Home));
    }

    #[test]
    fn test_tab_manager_max_tabs() {
        let mut tabs = TabManager::new();

        // Fill to capacity
        for i in 0..MAX_TABS {
            tabs.open_thread(format!("t{}", i), format!("T{}", i), "p".to_string());
        }
        assert_eq!(tabs.len(), MAX_TABS);

        // Add one more - should evict oldest
        tabs.open_thread("tnew".to_string(), "TNew".to_string(), "p".to_string());
        assert_eq!(tabs.len(), MAX_TABS);
        assert!(tabs.find_by_thread_id("t0").is_none()); // First was evicted
        assert!(tabs.find_by_thread_id("tnew").is_some());
    }

}

// =============================================================================
// CONVERSATION STATE
// =============================================================================

use crate::models::{Message, ProjectAgent, Thread, TimeFilter};
use std::collections::HashMap;

/// Buffer for local streaming content (per conversation)
#[derive(Default, Clone)]
pub struct LocalStreamBuffer {
    pub agent_pubkey: String,
    pub text_content: String,
    pub reasoning_content: String,
    pub is_complete: bool,
}

/// State for conversation view - thread/agent selection, subthread navigation, and message display.
///
/// This consolidates conversation-related state that was previously scattered across App.
/// It manages:
/// - Currently selected thread and agent
/// - Subthread navigation (viewing replies to a specific message)
/// - Message selection within the conversation
/// - Local streaming buffers for real-time message updates
/// - LLM metadata display toggle
#[derive(Default)]
pub struct ConversationState {
    /// Currently selected thread
    pub selected_thread: Option<Thread>,
    /// Currently selected agent for sending messages
    pub selected_agent: Option<ProjectAgent>,
    /// Subthread root message ID (when viewing replies to a specific message)
    pub subthread_root: Option<String>,
    /// The root message when viewing a subthread (for display and reply tagging)
    pub subthread_root_message: Option<Message>,
    /// Index of selected message in chat view (for navigation)
    pub selected_message_index: usize,
    /// Local streaming buffers by conversation_id
    pub local_stream_buffers: HashMap<String, LocalStreamBuffer>,
    /// Toggle for showing/hiding LLM metadata on messages (model, tokens, cost)
    pub show_llm_metadata: bool,
}

impl ConversationState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enter a subthread view rooted at the given message
    pub fn enter_subthread(&mut self, message: Message) {
        self.subthread_root = Some(message.id.clone());
        self.subthread_root_message = Some(message);
        self.selected_message_index = 0;
    }

    /// Exit the current subthread view and return to parent
    pub fn exit_subthread(&mut self) {
        self.subthread_root = None;
        self.subthread_root_message = None;
        self.selected_message_index = 0;
    }

    /// Check if we're currently viewing a subthread
    pub fn in_subthread(&self) -> bool {
        self.subthread_root.is_some()
    }

    /// Get current conversation ID (thread ID)
    pub fn current_conversation_id(&self) -> Option<String> {
        self.selected_thread.as_ref().map(|t| t.id.clone())
    }

    /// Get streaming content for current conversation
    pub fn local_streaming_content(&self) -> Option<&LocalStreamBuffer> {
        let conv_id = self.current_conversation_id()?;
        self.local_stream_buffers.get(&conv_id)
    }

    /// Update streaming buffer from local chunk
    pub fn handle_local_stream_chunk(
        &mut self,
        agent_pubkey: String,
        conversation_id: String,
        text_delta: Option<String>,
        reasoning_delta: Option<String>,
        is_finish: bool,
    ) {
        let buffer = self.local_stream_buffers
            .entry(conversation_id)
            .or_default();

        buffer.agent_pubkey = agent_pubkey;
        if let Some(delta) = text_delta {
            buffer.text_content.push_str(&delta);
        }
        if let Some(delta) = reasoning_delta {
            buffer.reasoning_content.push_str(&delta);
        }
        if is_finish {
            buffer.is_complete = true;
        }
    }

    /// Clear the local stream buffer for a conversation
    pub fn clear_local_stream_buffer(&mut self, conversation_id: &str) {
        self.local_stream_buffers.remove(conversation_id);
    }

    /// Toggle LLM metadata display
    pub fn toggle_llm_metadata(&mut self) {
        self.show_llm_metadata = !self.show_llm_metadata;
    }

    /// Reset message selection to the beginning
    pub fn reset_message_selection(&mut self) {
        self.selected_message_index = 0;
    }

    /// Clear thread and agent selection (e.g., when navigating away)
    pub fn clear_selection(&mut self) {
        self.selected_thread = None;
        self.selected_agent = None;
        self.subthread_root = None;
        self.subthread_root_message = None;
        self.selected_message_index = 0;
    }
}

#[cfg(test)]
mod conversation_state_tests {
    use super::*;

    #[test]
    fn test_conversation_state_new() {
        let state = ConversationState::new();
        assert!(state.selected_thread.is_none());
        assert!(state.selected_agent.is_none());
        assert!(state.subthread_root.is_none());
        assert!(state.subthread_root_message.is_none());
        assert_eq!(state.selected_message_index, 0);
        assert!(state.local_stream_buffers.is_empty());
        assert!(!state.show_llm_metadata);
    }

    #[test]
    fn test_subthread_navigation() {
        let mut state = ConversationState::new();

        // Initially not in subthread
        assert!(!state.in_subthread());

        // Create a mock message for testing
        let message = Message {
            id: "msg-123".to_string(),
            pubkey: "test-pubkey".to_string(),
            content: "Test message".to_string(),
            created_at: 1234567890,
            thread_id: "thread-456".to_string(),
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            a_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: vec![],
            delegation_tag: None,
            branch: None,
        };

        // Enter subthread
        state.enter_subthread(message.clone());
        assert!(state.in_subthread());
        assert_eq!(state.subthread_root, Some("msg-123".to_string()));
        assert_eq!(state.selected_message_index, 0);

        // Exit subthread
        state.exit_subthread();
        assert!(!state.in_subthread());
        assert!(state.subthread_root.is_none());
        assert!(state.subthread_root_message.is_none());
    }

    #[test]
    fn test_streaming_buffer() {
        let mut state = ConversationState::new();

        // Handle stream chunk
        state.handle_local_stream_chunk(
            "agent-pubkey".to_string(),
            "conv-123".to_string(),
            Some("Hello ".to_string()),
            None,
            false,
        );

        // Check buffer state
        let buffer = state.local_stream_buffers.get("conv-123").unwrap();
        assert_eq!(buffer.agent_pubkey, "agent-pubkey");
        assert_eq!(buffer.text_content, "Hello ");
        assert!(!buffer.is_complete);

        // Add more content
        state.handle_local_stream_chunk(
            "agent-pubkey".to_string(),
            "conv-123".to_string(),
            Some("World!".to_string()),
            Some("Reasoning text".to_string()),
            true,
        );

        let buffer = state.local_stream_buffers.get("conv-123").unwrap();
        assert_eq!(buffer.text_content, "Hello World!");
        assert_eq!(buffer.reasoning_content, "Reasoning text");
        assert!(buffer.is_complete);

        // Clear buffer
        state.clear_local_stream_buffer("conv-123");
        assert!(state.local_stream_buffers.get("conv-123").is_none());
    }

    #[test]
    fn test_toggle_llm_metadata() {
        let mut state = ConversationState::new();
        assert!(!state.show_llm_metadata);

        state.toggle_llm_metadata();
        assert!(state.show_llm_metadata);

        state.toggle_llm_metadata();
        assert!(!state.show_llm_metadata);
    }

    #[test]
    fn test_clear_selection() {
        let mut state = ConversationState::new();

        // Set some state
        state.selected_message_index = 5;
        state.subthread_root = Some("root-msg".to_string());
        state.show_llm_metadata = true;

        // Clear selection
        state.clear_selection();

        assert!(state.selected_thread.is_none());
        assert!(state.selected_agent.is_none());
        assert!(state.subthread_root.is_none());
        assert!(state.subthread_root_message.is_none());
        assert_eq!(state.selected_message_index, 0);
        // Note: show_llm_metadata is NOT cleared - it's a display preference
        assert!(state.show_llm_metadata);
    }
}

// =============================================================================
// HOME VIEW STATE
// =============================================================================

/// State for home view navigation - time filters, archive toggle, and agent browser.
///
/// This consolidates home-screen related navigation state that was previously
/// scattered across the App struct. It manages:
/// - Time filter for conversation filtering
/// - Archived conversations toggle
/// - Agent browser navigation and filtering
///
/// # Agent Browser State
/// The agent browser has two modes: list view and detail view.
/// Detail view is active when `viewing_agent_id` is `Some(id)`.
/// Use `enter_agent_detail()` and `exit_agent_detail()` to transition between modes.
#[derive(Debug, Clone, Default)]
pub struct HomeViewState {
    /// Filter by time since last activity
    pub time_filter: Option<TimeFilter>,
    /// Whether to show archived conversations in Recent/Inbox
    pub show_archived: bool,
    /// Selected index in agent browser list
    pub agent_browser_index: usize,
    /// Search filter for agent browser
    pub agent_browser_filter: String,
    /// ID of agent being viewed in detail (None = list view, Some = detail view)
    pub viewing_agent_id: Option<String>,
}

impl HomeViewState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Cycle through time filter options
    pub fn cycle_time_filter(&mut self) {
        self.time_filter = TimeFilter::cycle_next(self.time_filter);
    }

    /// Toggle showing archived conversations
    pub fn toggle_archived(&mut self) {
        self.show_archived = !self.show_archived;
    }

    /// Check if currently viewing agent detail (derived from viewing_agent_id)
    pub fn in_agent_detail(&self) -> bool {
        self.viewing_agent_id.is_some()
    }

    /// Enter agent detail view for the specified agent
    pub fn enter_agent_detail(&mut self, agent_id: String) {
        self.viewing_agent_id = Some(agent_id);
    }

    /// Exit agent detail view and return to list
    pub fn exit_agent_detail(&mut self) {
        self.viewing_agent_id = None;
    }

    /// Reset agent browser state completely (index, filter, exit detail view)
    pub fn reset_agent_browser(&mut self) {
        self.agent_browser_index = 0;
        self.agent_browser_filter.clear();
        self.viewing_agent_id = None;
    }

    /// Set the agent browser filter text
    pub fn set_agent_filter(&mut self, filter: String) {
        self.agent_browser_filter = filter;
    }

    /// Append a character to the agent browser filter and reset index to 0
    pub fn append_to_filter(&mut self, c: char) {
        self.agent_browser_filter.push(c);
        self.agent_browser_index = 0;
    }

    /// Remove the last character from the filter (backspace behavior)
    pub fn backspace_filter(&mut self) {
        self.agent_browser_filter.pop();
        self.agent_browser_index = 0;
    }

    /// Clear the agent browser filter
    pub fn clear_agent_filter(&mut self) {
        self.agent_browser_filter.clear();
    }

    /// Set the selected agent index in the browser list
    pub fn set_agent_index(&mut self, index: usize) {
        self.agent_browser_index = index;
    }

    /// Move selection up in the agent browser list
    pub fn select_prev_agent(&mut self) {
        if self.agent_browser_index > 0 {
            self.agent_browser_index -= 1;
        }
    }

    /// Move selection down in the agent browser list, bounded by count
    pub fn select_next_agent(&mut self, count: usize) {
        if self.agent_browser_index < count.saturating_sub(1) {
            self.agent_browser_index += 1;
        }
    }
}

#[cfg(test)]
mod home_view_state_tests {
    use super::*;

    #[test]
    fn test_home_view_state_new() {
        let state = HomeViewState::new();
        assert!(state.time_filter.is_none());
        assert!(!state.show_archived);
        assert_eq!(state.agent_browser_index, 0);
        assert!(state.agent_browser_filter.is_empty());
        assert!(state.viewing_agent_id.is_none());
        // in_agent_detail is derived from viewing_agent_id
        assert!(!state.in_agent_detail());
    }

    #[test]
    fn test_cycle_time_filter() {
        let mut state = HomeViewState::new();

        // None -> OneHour
        state.cycle_time_filter();
        assert_eq!(state.time_filter, Some(TimeFilter::OneHour));

        // OneHour -> FourHours
        state.cycle_time_filter();
        assert_eq!(state.time_filter, Some(TimeFilter::FourHours));

        // FourHours -> TwelveHours
        state.cycle_time_filter();
        assert_eq!(state.time_filter, Some(TimeFilter::TwelveHours));

        // TwelveHours -> TwentyFourHours
        state.cycle_time_filter();
        assert_eq!(state.time_filter, Some(TimeFilter::TwentyFourHours));

        // TwentyFourHours -> SevenDays
        state.cycle_time_filter();
        assert_eq!(state.time_filter, Some(TimeFilter::SevenDays));

        // SevenDays -> None
        state.cycle_time_filter();
        assert!(state.time_filter.is_none());
    }

    #[test]
    fn test_toggle_archived() {
        let mut state = HomeViewState::new();
        assert!(!state.show_archived);

        state.toggle_archived();
        assert!(state.show_archived);

        state.toggle_archived();
        assert!(!state.show_archived);
    }

    #[test]
    fn test_agent_browser_navigation() {
        let mut state = HomeViewState::new();

        // Initially not in detail view (derived from viewing_agent_id being None)
        assert!(!state.in_agent_detail());
        assert!(state.viewing_agent_id.is_none());

        // Enter detail view using the API method
        state.enter_agent_detail("agent-123".to_string());
        assert!(state.in_agent_detail());
        assert_eq!(state.viewing_agent_id, Some("agent-123".to_string()));

        // Exit detail view using the API method
        state.exit_agent_detail();
        assert!(!state.in_agent_detail());
        assert!(state.viewing_agent_id.is_none());
    }

    #[test]
    fn test_reset_agent_browser() {
        let mut state = HomeViewState::new();

        // Set some state using setters
        state.set_agent_index(5);
        state.set_agent_filter("test".to_string());
        state.enter_agent_detail("agent-456".to_string());

        // Verify state before reset
        assert_eq!(state.agent_browser_index, 5);
        assert_eq!(state.agent_browser_filter, "test");
        assert!(state.in_agent_detail());

        // Reset clears everything
        state.reset_agent_browser();

        assert_eq!(state.agent_browser_index, 0);
        assert!(state.agent_browser_filter.is_empty());
        assert!(!state.in_agent_detail());
        assert!(state.viewing_agent_id.is_none());
    }

    #[test]
    fn test_agent_filter_operations() {
        let mut state = HomeViewState::new();

        // Set filter
        state.set_agent_filter("search term".to_string());
        assert_eq!(state.agent_browser_filter, "search term");

        // Clear filter
        state.clear_agent_filter();
        assert!(state.agent_browser_filter.is_empty());
    }

    #[test]
    fn test_in_agent_detail_is_derived() {
        let mut state = HomeViewState::new();

        // Directly setting viewing_agent_id affects in_agent_detail()
        state.viewing_agent_id = Some("test-agent".to_string());
        assert!(state.in_agent_detail());

        state.viewing_agent_id = None;
        assert!(!state.in_agent_detail());

        // This confirms the boolean is truly derived, not stored separately
    }

    #[test]
    fn test_append_and_backspace_filter() {
        let mut state = HomeViewState::new();
        state.set_agent_index(5); // Set index to non-zero

        // Append character resets index to 0
        state.append_to_filter('a');
        assert_eq!(state.agent_browser_filter, "a");
        assert_eq!(state.agent_browser_index, 0);

        // Append more characters
        state.append_to_filter('b');
        state.append_to_filter('c');
        assert_eq!(state.agent_browser_filter, "abc");

        // Backspace removes last character and resets index
        state.set_agent_index(3);
        state.backspace_filter();
        assert_eq!(state.agent_browser_filter, "ab");
        assert_eq!(state.agent_browser_index, 0);

        // Continue backspacing
        state.backspace_filter();
        state.backspace_filter();
        assert!(state.agent_browser_filter.is_empty());

        // Backspace on empty is safe (no panic)
        state.backspace_filter();
        assert!(state.agent_browser_filter.is_empty());
    }

    #[test]
    fn test_agent_index_navigation() {
        let mut state = HomeViewState::new();

        // Start at 0
        assert_eq!(state.agent_browser_index, 0);

        // Can't go negative (select_prev does nothing at 0)
        state.select_prev_agent();
        assert_eq!(state.agent_browser_index, 0);

        // Navigate down with a count of 5 items
        state.select_next_agent(5);
        assert_eq!(state.agent_browser_index, 1);

        state.select_next_agent(5);
        assert_eq!(state.agent_browser_index, 2);

        // Navigate to last item
        state.select_next_agent(5);
        state.select_next_agent(5);
        assert_eq!(state.agent_browser_index, 4);

        // Can't go past the end
        state.select_next_agent(5);
        assert_eq!(state.agent_browser_index, 4);

        // Navigate back up
        state.select_prev_agent();
        assert_eq!(state.agent_browser_index, 3);
    }

    #[test]
    fn test_complete_agent_browser_workflow() {
        // Integration-style test: simulates a complete user workflow
        let mut state = HomeViewState::new();

        // User types a search filter
        state.append_to_filter('t');
        state.append_to_filter('e');
        state.append_to_filter('s');
        state.append_to_filter('t');
        assert_eq!(state.agent_browser_filter, "test");

        // User navigates through results (assume 3 agents matched)
        state.select_next_agent(3);
        state.select_next_agent(3);
        assert_eq!(state.agent_browser_index, 2);

        // User selects an agent to view details
        state.enter_agent_detail("selected-agent-id".to_string());
        assert!(state.in_agent_detail());
        assert_eq!(state.viewing_agent_id, Some("selected-agent-id".to_string()));

        // User exits back to list
        state.exit_agent_detail();
        assert!(!state.in_agent_detail());
        // Filter and index should still be preserved
        assert_eq!(state.agent_browser_filter, "test");
        assert_eq!(state.agent_browser_index, 2);

        // User clears everything
        state.reset_agent_browser();
        assert!(state.agent_browser_filter.is_empty());
        assert_eq!(state.agent_browser_index, 0);
        assert!(!state.in_agent_detail());
    }
}
