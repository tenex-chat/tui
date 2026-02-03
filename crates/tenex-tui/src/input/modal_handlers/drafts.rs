use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::{App, ModalState};

pub(super) fn handle_draft_navigator_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;

    // Get mutable access to the draft navigator state
    let state = match &mut app.modal_state {
        ModalState::DraftNavigator(state) => state,
        _ => return,
    };

    match code {
        // Close modal
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
        }

        // Navigation
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_down();
        }

        // Select draft (restore to chat editor)
        KeyCode::Enter => {
            if let Some(draft) = state.selected_draft().cloned() {
                app.modal_state = ModalState::None;
                app.restore_named_draft(&draft);
            }
        }

        // Delete draft
        KeyCode::Char('d') if modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(draft) = state.selected_draft() {
                let draft_id = draft.id.clone();
                app.delete_named_draft(&draft_id);
                // Refresh the drafts list (project-scoped to maintain consistency)
                let drafts = app.get_named_drafts_for_current_project();
                if let ModalState::DraftNavigator(state) = &mut app.modal_state {
                    state.drafts = drafts;
                    // Clamp selection index
                    let max = state.filtered_drafts().len();
                    if state.selected_index >= max && max > 0 {
                        state.selected_index = max - 1;
                    }
                }
            }
        }

        // Filter input
        KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => {
            state.add_filter_char(c);
        }
        KeyCode::Backspace => {
            state.backspace_filter();
        }

        _ => {}
    }
}
