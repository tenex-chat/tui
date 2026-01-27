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
