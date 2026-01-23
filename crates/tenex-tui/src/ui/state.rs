//! Extracted state types for the App struct.
//!
//! This module contains self-contained state machines that have been extracted
//! from the monolithic App struct to improve encapsulation and testability.

use std::collections::HashSet;

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

/// State for message history navigation (up/down arrow in input).
///
/// Tracks previously sent messages and allows cycling through them
/// while preserving the current draft.
#[derive(Debug, Clone, Default)]
pub struct MessageHistoryState {
    /// Sent message history (most recent last, max 50)
    history: Vec<String>,
    /// Current index in history (None = typing new message)
    index: Option<usize>,
    /// Draft preserved when browsing history
    draft: Option<String>,
}

impl MessageHistoryState {
    /// Maximum number of messages to keep in history
    pub const MAX_HISTORY: usize = 50;

    pub fn new() -> Self {
        Self::default()
    }

    /// Add a message to history
    pub fn add(&mut self, message: String) {
        // Don't add duplicates consecutively
        if self.history.last() != Some(&message) {
            self.history.push(message);
            // Trim to max size
            if self.history.len() > Self::MAX_HISTORY {
                self.history.remove(0);
            }
        }
        // Exit browsing mode after sending
        self.index = None;
        self.draft = None;
    }

    /// Navigate to previous (older) message
    /// Returns the message to display, or None if at start
    pub fn prev(&mut self, current_input: &str) -> Option<&str> {
        if self.history.is_empty() {
            return None;
        }

        match self.index {
            None => {
                // Start browsing - save current as draft
                self.draft = Some(current_input.to_string());
                self.index = Some(self.history.len() - 1);
            }
            Some(idx) if idx > 0 => {
                self.index = Some(idx - 1);
            }
            _ => return None, // Already at oldest
        }

        self.index.and_then(|i| self.history.get(i).map(|s| s.as_str()))
    }

    /// Navigate to next (newer) message
    /// Returns the message to display, or the draft if reaching end
    pub fn next(&mut self) -> Option<&str> {
        match self.index {
            Some(idx) if idx + 1 < self.history.len() => {
                self.index = Some(idx + 1);
                self.history.get(idx + 1).map(|s| s.as_str())
            }
            Some(_) => {
                // At end of history, return to draft
                self.index = None;
                self.draft.as_deref()
            }
            None => None,
        }
    }

    /// Check if currently browsing history
    pub fn is_browsing(&self) -> bool {
        self.index.is_some()
    }

    /// Exit browsing mode without keeping selection
    pub fn exit(&mut self) {
        self.index = None;
        self.draft = None;
    }

    /// Get current draft (for restoration after exiting history)
    pub fn get_draft(&self) -> Option<&str> {
        self.draft.as_deref()
    }
}

/// State for nudge management.
#[derive(Debug, Clone, Default)]
pub struct NudgeState {
    /// Selected nudge IDs for the current conversation
    pub selected_ids: Vec<String>,
}

impl NudgeState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a nudge ID to selection
    pub fn select(&mut self, id: String) {
        if !self.selected_ids.contains(&id) {
            self.selected_ids.push(id);
        }
    }

    /// Remove a nudge ID from selection
    pub fn deselect(&mut self, id: &str) {
        self.selected_ids.retain(|i| i != id);
    }

    /// Toggle nudge selection
    pub fn toggle(&mut self, id: String) {
        if self.selected_ids.contains(&id) {
            self.deselect(&id);
        } else {
            self.select(id);
        }
    }

    /// Check if a nudge is selected
    pub fn is_selected(&self, id: &str) -> bool {
        self.selected_ids.contains(&id.to_string())
    }

    /// Clear all selected nudges
    pub fn clear(&mut self) {
        self.selected_ids.clear();
    }
}

/// State for agent browser view.
#[derive(Debug, Clone, Default)]
pub struct AgentBrowserState {
    /// Selected index in agent list
    pub index: usize,
    /// Filter string for fuzzy search
    pub filter: String,
    /// Whether viewing agent detail
    pub in_detail: bool,
    /// ID of agent being viewed (when in_detail)
    pub viewing_id: Option<String>,
}

impl AgentBrowserState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Enter agent browser
    pub fn enter(&mut self) {
        self.index = 0;
        self.filter.clear();
        self.in_detail = false;
        self.viewing_id = None;
    }

    /// Exit agent browser
    pub fn exit(&mut self) {
        self.filter.clear();
        self.index = 0;
        self.in_detail = false;
        self.viewing_id = None;
    }

    /// Enter detail view for an agent
    pub fn view_detail(&mut self, agent_id: String) {
        self.viewing_id = Some(agent_id);
        self.in_detail = true;
    }

    /// Exit detail view
    pub fn exit_detail(&mut self) {
        self.in_detail = false;
        self.viewing_id = None;
    }

    /// Update filter and reset index
    pub fn set_filter(&mut self, filter: String) {
        self.filter = filter;
        self.index = 0;
    }

    /// Add character to filter
    pub fn push_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.index = 0;
    }

    /// Remove last character from filter
    pub fn pop_filter_char(&mut self) {
        self.filter.pop();
        self.index = 0;
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
}

impl OpenTab {
    /// Check if this is a draft tab (no thread created yet)
    pub fn is_draft(&self) -> bool {
        self.draft_id.is_some()
    }

    /// Create a new tab for an existing thread
    pub fn for_thread(thread_id: String, thread_title: String, project_a_tag: String) -> Self {
        Self {
            thread_id,
            thread_title,
            project_a_tag,
            has_unread: false,
            draft_id: None,
            navigation_stack: Vec::new(),
            message_history: TabMessageHistory::default(),
            chat_search: ChatSearchState::default(),
            selected_nudge_ids: Vec::new(),
            editor: TextEditor::new(),
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
            draft_id: Some(draft_id),
            navigation_stack: Vec::new(),
            message_history: TabMessageHistory::default(),
            chat_search: ChatSearchState::default(),
            selected_nudge_ids: Vec::new(),
            editor: TextEditor::new(),
        }
    }
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
            self.tabs[idx].has_unread = false;
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
        self.active_index = index;
        self.tabs[index].has_unread = false;
        true
    }

    /// Close the current tab.
    /// Returns a tuple of (removed_tab, new_active_index).
    /// new_active_index is None if no tabs remain.
    pub fn close_current(&mut self) -> (Option<OpenTab>, Option<usize>) {
        if self.tabs.is_empty() {
            return (None, None);
        }

        let removed_index = self.active_index;
        let removed_tab = self.tabs.remove(removed_index);
        self.cleanup_history(removed_index);

        if self.tabs.is_empty() {
            self.active_index = 0;
            return (Some(removed_tab), None);
        }

        // Move to next tab (or previous if we were at the end)
        if self.active_index >= self.tabs.len() {
            self.active_index = self.tabs.len() - 1;
        }
        (Some(removed_tab), Some(self.active_index))
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

// =============================================================================
// HOME VIEW STATE
// =============================================================================

/// Which tab is focused in the home view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum HomeTab {
    #[default]
    Recent,
    Inbox,
    Reports,
    Status,
    Search,
}

/// State for home view navigation and filtering.
#[derive(Debug, Clone, Default)]
pub struct HomeViewState {
    /// Current tab focus
    pub panel_focus: HomeTab,
    /// Per-tab selection index (preserves position when switching tabs)
    pub tab_selections: std::collections::HashMap<HomeTab, usize>,
    /// Report search filter
    pub report_filter: String,
    /// Whether sidebar is focused (vs content area)
    pub sidebar_focused: bool,
    /// Selected index in sidebar project list
    pub sidebar_index: usize,
    /// Projects to show in Recent/Inbox (empty = none)
    pub visible_projects: HashSet<String>,
    /// Whether to show archived conversations
    pub show_archived: bool,
    /// Whether to show archived projects in sidebar
    pub show_archived_projects: bool,
}

impl HomeViewState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the selection index for the current tab
    pub fn current_selection(&self) -> usize {
        *self.tab_selections.get(&self.panel_focus).unwrap_or(&0)
    }

    /// Set the selection index for the current tab
    pub fn set_current_selection(&mut self, index: usize) {
        self.tab_selections.insert(self.panel_focus, index);
    }

    /// Cycle to next tab
    pub fn next_tab(&mut self) {
        self.panel_focus = match self.panel_focus {
            HomeTab::Recent => HomeTab::Inbox,
            HomeTab::Inbox => HomeTab::Reports,
            HomeTab::Reports => HomeTab::Status,
            HomeTab::Status => HomeTab::Search,
            HomeTab::Search => HomeTab::Recent,
        };
    }

    /// Cycle to previous tab
    pub fn prev_tab(&mut self) {
        self.panel_focus = match self.panel_focus {
            HomeTab::Recent => HomeTab::Search,
            HomeTab::Inbox => HomeTab::Recent,
            HomeTab::Reports => HomeTab::Inbox,
            HomeTab::Status => HomeTab::Reports,
            HomeTab::Search => HomeTab::Status,
        };
    }

    /// Toggle project visibility
    pub fn toggle_project_visibility(&mut self, project_a_tag: &str) {
        if self.visible_projects.contains(project_a_tag) {
            self.visible_projects.remove(project_a_tag);
        } else {
            self.visible_projects.insert(project_a_tag.to_string());
        }
    }

    /// Check if a project is visible
    pub fn is_project_visible(&self, project_a_tag: &str) -> bool {
        self.visible_projects.contains(project_a_tag)
    }
}

// =============================================================================
// CHAT VIEW STATE
// =============================================================================

/// State for chat view navigation and display.
#[derive(Debug, Clone, Default)]
pub struct ChatViewState {
    /// Current scroll position
    pub scroll_offset: usize,
    /// Maximum scroll offset (set during rendering)
    pub max_scroll_offset: usize,
    /// Index of selected message for navigation
    pub selected_message_index: usize,
    /// Root message ID when viewing a subthread
    pub subthread_root: Option<String>,
    /// Toggle for showing LLM metadata (model, tokens, cost)
    pub show_llm_metadata: bool,
    /// Toggle for showing the todo sidebar
    pub todo_sidebar_visible: bool,
    /// Collapsed thread IDs (parent threads whose children are hidden)
    pub collapsed_threads: HashSet<String>,
}

impl ChatViewState {
    pub fn new() -> Self {
        Self {
            todo_sidebar_visible: true,
            ..Default::default()
        }
    }

    /// Reset state for a new conversation
    pub fn reset_for_conversation(&mut self) {
        self.scroll_offset = 0;
        self.selected_message_index = 0;
        self.subthread_root = None;
    }

    /// Enter a subthread
    pub fn enter_subthread(&mut self, root_id: String) {
        self.subthread_root = Some(root_id);
        self.scroll_offset = 0;
        self.selected_message_index = 0;
    }

    /// Exit subthread view
    pub fn exit_subthread(&mut self) {
        self.subthread_root = None;
        self.scroll_offset = 0;
        self.selected_message_index = 0;
    }

    /// Check if in subthread view
    pub fn in_subthread(&self) -> bool {
        self.subthread_root.is_some()
    }

    /// Toggle thread collapse state
    pub fn toggle_thread_collapse(&mut self, thread_id: &str) {
        if self.collapsed_threads.contains(thread_id) {
            self.collapsed_threads.remove(thread_id);
        } else {
            self.collapsed_threads.insert(thread_id.to_string());
        }
    }

    /// Check if a thread is collapsed
    pub fn is_thread_collapsed(&self, thread_id: &str) -> bool {
        self.collapsed_threads.contains(thread_id)
    }

    /// Scroll up by a number of lines
    pub fn scroll_up(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(lines);
    }

    /// Scroll down by a number of lines
    pub fn scroll_down(&mut self, lines: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(lines);
        // Clamp is done externally since we don't know max here
    }

    /// Scroll to bottom
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = usize::MAX; // Will be clamped during rendering
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
    fn test_message_history() {
        let mut state = MessageHistoryState::new();

        // Add some messages
        state.add("first".to_string());
        state.add("second".to_string());
        state.add("third".to_string());

        // Browse backward
        assert_eq!(state.prev("current draft"), Some("third"));
        assert!(state.is_browsing());
        assert_eq!(state.prev("current draft"), Some("second"));
        assert_eq!(state.prev("current draft"), Some("first"));
        assert_eq!(state.prev("current draft"), None); // At start

        // Browse forward
        assert_eq!(state.next(), Some("second"));
        assert_eq!(state.next(), Some("third"));
        assert_eq!(state.next(), Some("current draft")); // Back to draft
        assert!(!state.is_browsing());
    }

    #[test]
    fn test_nudge_state() {
        let mut state = NudgeState::new();

        state.select("nudge1".to_string());
        state.select("nudge2".to_string());
        assert!(state.is_selected("nudge1"));
        assert!(state.is_selected("nudge2"));

        state.toggle("nudge1".to_string());
        assert!(!state.is_selected("nudge1"));
    }

    #[test]
    fn test_agent_browser_state() {
        let mut state = AgentBrowserState::new();
        state.enter();

        state.push_filter_char('t');
        state.push_filter_char('e');
        assert_eq!(state.filter, "te");
        assert_eq!(state.index, 0);

        state.view_detail("agent123".to_string());
        assert!(state.in_detail);
        assert_eq!(state.viewing_id, Some("agent123".to_string()));

        state.exit_detail();
        assert!(!state.in_detail);
        assert_eq!(state.viewing_id, None);
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

        // Opening same project draft should reuse
        let idx = tabs.open_draft("project1".to_string(), "Project 1".to_string());
        assert_eq!(idx, 0);
        assert_eq!(tabs.len(), 1);

        // Different project should create new draft
        let idx = tabs.open_draft("project2".to_string(), "Project 2".to_string());
        assert_eq!(idx, 1);
        assert_eq!(tabs.len(), 2);

        // Convert draft to real tab
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

        // Close middle tab
        tabs.switch_to(1);
        let (removed_tab, new_idx) = tabs.close_current();
        assert!(removed_tab.is_some());
        assert_eq!(removed_tab.unwrap().thread_id, "t2");
        assert_eq!(new_idx, Some(1)); // Now t3 is at index 1
        assert_eq!(tabs.len(), 2);

        // Close all
        tabs.close_current();
        tabs.close_current();
        assert!(tabs.is_empty());
        let (removed_tab, new_idx) = tabs.close_current();
        assert!(removed_tab.is_none());
        assert_eq!(new_idx, None);
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

    #[test]
    fn test_home_view_state() {
        let mut state = HomeViewState::new();

        assert_eq!(state.panel_focus, HomeTab::Recent);
        assert_eq!(state.current_selection(), 0);

        state.set_current_selection(5);
        assert_eq!(state.current_selection(), 5);

        state.next_tab();
        assert_eq!(state.panel_focus, HomeTab::Inbox);
        assert_eq!(state.current_selection(), 0); // Different tab, different selection

        state.set_current_selection(3);
        state.next_tab();
        state.prev_tab();
        assert_eq!(state.current_selection(), 3); // Preserved
    }

    #[test]
    fn test_chat_view_state() {
        let mut state = ChatViewState::new();

        assert!(state.todo_sidebar_visible);
        assert!(!state.in_subthread());

        state.enter_subthread("msg123".to_string());
        assert!(state.in_subthread());
        assert_eq!(state.subthread_root, Some("msg123".to_string()));

        state.exit_subthread();
        assert!(!state.in_subthread());

        state.toggle_thread_collapse("t1");
        assert!(state.is_thread_collapsed("t1"));
        state.toggle_thread_collapse("t1");
        assert!(!state.is_thread_collapsed("t1"));
    }
}
