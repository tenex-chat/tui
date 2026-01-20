//! Input prefix trigger handling module.
//!
//! This module provides a modular system for handling special prefix characters
//! when typed at the beginning of an empty input field. Each prefix can trigger
//! a different action (e.g., `@` opens agent selector, `/` opens nudges, etc.)
//!
//! ## Adding New Prefix Triggers
//!
//! To add a new prefix trigger:
//! 1. Add a new variant to `PrefixTrigger` enum
//! 2. Add the character mapping in `PrefixTrigger::from_char()`
//! 3. Handle the trigger in `execute_prefix_trigger()`

use crate::ui::App;

/// Represents a prefix trigger action that should be executed
/// when a specific character is typed at the start of an empty input.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrefixTrigger {
    /// `@` - Open agent selector modal
    AgentSelector,
    /// `/` - Open nudges selector modal
    NudgeSelector,
    // Future prefix triggers can be added here:
    // HashtagSearch,   // `#` - Open hashtag/topic search
    // BranchSelector,  // `%` - Open branch selector
}

impl PrefixTrigger {
    /// Maps a character to its corresponding prefix trigger, if any.
    ///
    /// Returns `Some(PrefixTrigger)` if the character is a recognized prefix,
    /// or `None` if it should be handled as normal input.
    pub fn from_char(c: char) -> Option<Self> {
        match c {
            '@' => Some(PrefixTrigger::AgentSelector),
            '/' => Some(PrefixTrigger::NudgeSelector),
            // Add new prefix mappings here:
            // '#' => Some(PrefixTrigger::HashtagSearch),
            // '%' => Some(PrefixTrigger::BranchSelector),
            _ => None,
        }
    }

    /// Returns the character that triggers this prefix action.
    #[allow(dead_code)]
    pub fn trigger_char(&self) -> char {
        match self {
            PrefixTrigger::AgentSelector => '@',
            PrefixTrigger::NudgeSelector => '/',
            // PrefixTrigger::HashtagSearch => '#',
            // PrefixTrigger::BranchSelector => '%',
        }
    }
}

/// Checks if the input is in a state where prefix triggers should be evaluated.
///
/// Returns `true` if the input text is completely empty (no text, no attachments).
pub fn should_check_prefix(app: &App) -> bool {
    app.chat_editor.text.is_empty() && !app.chat_editor.has_attachments()
}

/// Attempts to handle a character as a prefix trigger.
///
/// Returns `true` if the character was handled as a prefix trigger,
/// `false` if it should be processed as normal input.
pub fn try_handle_prefix(app: &mut App, c: char) -> bool {
    // Only check prefixes when input is completely empty
    if !should_check_prefix(app) {
        return false;
    }

    // Check if this character is a prefix trigger
    if let Some(trigger) = PrefixTrigger::from_char(c) {
        execute_prefix_trigger(app, trigger);
        return true;
    }

    false
}

/// Executes the action associated with a prefix trigger.
fn execute_prefix_trigger(app: &mut App, trigger: PrefixTrigger) {
    match trigger {
        PrefixTrigger::AgentSelector => {
            app.open_agent_selector();
        }
        PrefixTrigger::NudgeSelector => {
            app.open_nudge_selector();
        }
        // Handle future triggers here:
        // PrefixTrigger::HashtagSearch => app.open_hashtag_search(),
        // PrefixTrigger::BranchSelector => app.open_branch_selector(),
    }
}
