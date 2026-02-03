use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::{self, App, ModalState};

pub(super) fn handle_backend_approval_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::BackendApprovalAction;

    let state = match &app.modal_state {
        ModalState::BackendApproval(s) => s.clone(),
        _ => return,
    };

    let action_count = BackendApprovalAction::ALL.len();

    match key.code {
        KeyCode::Esc => {
            // Dismiss - same as reject (will ask again later)
            app.modal_state = ModalState::None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.selected_index > 0 {
                if let ModalState::BackendApproval(ref mut s) = app.modal_state {
                    s.selected_index -= 1;
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.selected_index + 1 < action_count {
                if let ModalState::BackendApproval(ref mut s) = app.modal_state {
                    s.selected_index += 1;
                }
            }
        }
        KeyCode::Enter => {
            execute_backend_approval_action(app, &state, state.selected_action());
        }
        KeyCode::Char('a') => {
            execute_backend_approval_action(app, &state, BackendApprovalAction::Approve);
        }
        KeyCode::Char('r') => {
            execute_backend_approval_action(app, &state, BackendApprovalAction::Reject);
        }
        KeyCode::Char('b') => {
            execute_backend_approval_action(app, &state, BackendApprovalAction::Block);
        }
        _ => {}
    }
}

fn execute_backend_approval_action(
    app: &mut App,
    state: &ui::modal::BackendApprovalState,
    action: ui::modal::BackendApprovalAction,
) {
    use ui::modal::BackendApprovalAction;

    match action {
        BackendApprovalAction::Approve => {
            app.approve_backend(&state.backend_pubkey);
            app.set_warning_status(&format!(
                "Approved backend {}...",
                &state.backend_pubkey[..8.min(state.backend_pubkey.len())]
            ));
            app.modal_state = ModalState::None;
        }
        BackendApprovalAction::Reject => {
            // Just dismiss - will ask again later
            app.modal_state = ModalState::None;
        }
        BackendApprovalAction::Block => {
            app.block_backend(&state.backend_pubkey);
            app.set_warning_status(&format!(
                "Blocked backend {}...",
                &state.backend_pubkey[..8.min(state.backend_pubkey.len())]
            ));
            app.modal_state = ModalState::None;
        }
    }
}
