//! Extracted state types for the App struct.
//!
//! This module contains self-contained state machines that have been extracted
//! from the monolithic App struct to improve encapsulation and testability.

use std::collections::HashSet;

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

/// State for nudge and ask event management.
#[derive(Debug, Clone, Default)]
pub struct NudgeState {
    /// Selected nudge IDs for the current conversation
    pub selected_ids: Vec<String>,
    /// Ask event IDs that user dismissed (ESC) without answering
    pub dismissed_ask_ids: HashSet<String>,
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

    /// Dismiss an ask event
    pub fn dismiss_ask(&mut self, id: String) {
        self.dismissed_ask_ids.insert(id);
    }

    /// Check if an ask event was dismissed
    pub fn is_ask_dismissed(&self, id: &str) -> bool {
        self.dismissed_ask_ids.contains(id)
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

        state.dismiss_ask("ask1".to_string());
        assert!(state.is_ask_dismissed("ask1"));
        assert!(!state.is_ask_dismissed("ask2"));
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
}
