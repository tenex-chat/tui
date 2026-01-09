use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

/// Generic selector state for filterable lists
#[derive(Debug, Clone, Default)]
pub struct SelectorState {
    pub index: usize,
    pub filter: String,
}

impl SelectorState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn move_up(&mut self) {
        self.index = self.index.saturating_sub(1);
    }

    pub fn move_down(&mut self, max_index: usize) {
        if self.index < max_index {
            self.index += 1;
        }
    }

    /// Clamp index to valid range when list shrinks
    pub fn clamp_index(&mut self, item_count: usize) {
        if item_count == 0 {
            self.index = 0;
        } else {
            self.index = self.index.min(item_count - 1);
        }
    }

    pub fn add_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.index = 0;
    }

    pub fn backspace_filter(&mut self) {
        self.filter.pop();
        self.index = 0;
    }

    pub fn clear(&mut self) {
        self.filter.clear();
        self.index = 0;
    }

    pub fn filter_lowercase(&self) -> String {
        self.filter.to_lowercase()
    }
}

/// Result of handling a key in selector mode
pub enum SelectorAction<T> {
    /// Continue showing selector
    Continue,
    /// Selection was made
    Selected(T),
    /// Selector was cancelled
    Cancelled,
}

/// Handle common selector key events.
/// Takes KeyEvent (not KeyCode) to ignore Ctrl/Alt combos.
/// Returns Continue with no mutation when item_count == 0.
pub fn handle_selector_key<T, F>(
    state: &mut SelectorState,
    key: KeyEvent,
    item_count: usize,
    on_select: F,
) -> SelectorAction<T>
where
    F: FnOnce(usize) -> Option<T>,
{
    // Ignore keys with Ctrl/Alt modifiers - they shouldn't mutate selector state
    if key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) {
        return SelectorAction::Continue;
    }

    // Early return on empty list - avoid selecting index 0
    if item_count == 0 {
        return match key.code {
            KeyCode::Esc => {
                state.clear();
                SelectorAction::Cancelled
            }
            KeyCode::Backspace => {
                state.backspace_filter();
                SelectorAction::Continue
            }
            KeyCode::Char(c) => {
                state.add_filter_char(c);
                SelectorAction::Continue
            }
            _ => SelectorAction::Continue,
        };
    }

    match key.code {
        KeyCode::Up => {
            state.move_up();
            SelectorAction::Continue
        }
        KeyCode::Down => {
            state.move_down(item_count.saturating_sub(1));
            SelectorAction::Continue
        }
        KeyCode::Enter => {
            on_select(state.index).map_or(SelectorAction::Continue, SelectorAction::Selected)
        }
        KeyCode::Esc => {
            state.clear();
            SelectorAction::Cancelled
        }
        KeyCode::Backspace => {
            state.backspace_filter();
            SelectorAction::Continue
        }
        KeyCode::Char(c) => {
            state.add_filter_char(c);
            SelectorAction::Continue
        }
        _ => SelectorAction::Continue,
    }
}
