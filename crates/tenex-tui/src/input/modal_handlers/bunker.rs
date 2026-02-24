#[cfg(test)]
use std::collections::{HashSet, VecDeque};

use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::modal::BunkerApprovalAction;
use crate::ui::{App, ModalState};

pub(super) fn handle_bunker_approval_modal_key(app: &mut App, key: KeyEvent) {
    let state = match &app.modal_state {
        ModalState::BunkerApproval(s) => s.clone(),
        _ => return,
    };

    match key.code {
        KeyCode::Esc | KeyCode::Char('r') => {
            app.resolve_bunker_sign_request(state.request, false, false);
        }
        KeyCode::Char('a') => {
            app.resolve_bunker_sign_request(state.request, true, state.always_approve);
        }
        KeyCode::Char('A') => {
            app.resolve_bunker_sign_request(state.request, true, true);
        }
        KeyCode::Char(' ') | KeyCode::Char('t') => {
            if let ModalState::BunkerApproval(ref mut s) = app.modal_state {
                s.toggle_always_approve();
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let ModalState::BunkerApproval(ref mut s) = app.modal_state {
                s.move_up();
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let ModalState::BunkerApproval(ref mut s) = app.modal_state {
                s.move_down();
            }
        }
        KeyCode::Enter => match state.selected_action() {
            BunkerApprovalAction::Approve => {
                app.resolve_bunker_sign_request(state.request, true, state.always_approve);
            }
            BunkerApprovalAction::Reject => {
                app.resolve_bunker_sign_request(state.request, false, false);
            }
        },
        _ => {}
    }
}

pub(super) fn handle_bunker_rules_modal_key(app: &mut App, key: KeyEvent) {
    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::BunkerRules(state) => state,
        other => {
            app.modal_state = other;
            return;
        }
    };

    let close_to = |state: crate::ui::modal::BunkerRulesState| {
        state
            .return_to_settings
            .map(ModalState::AppSettings)
            .unwrap_or(ModalState::None)
    };

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.modal_state = close_to(state);
            return;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_down(app.bunker_auto_approve_rules.len());
        }
        KeyCode::Delete | KeyCode::Char('d') => {
            if let Some(rule) = app
                .bunker_auto_approve_rules
                .get(state.selected_index)
                .cloned()
            {
                if let Err(e) =
                    app.remove_bunker_auto_approve_rule(&rule.requester_pubkey, rule.event_kind)
                {
                    app.set_warning_status(&format!("Failed to remove bunker rule: {}", e));
                } else {
                    app.set_warning_status("Removed bunker auto-approve rule");
                    if state.selected_index >= app.bunker_auto_approve_rules.len()
                        && state.selected_index > 0
                    {
                        state.selected_index -= 1;
                    }
                }
            }
        }
        _ => {}
    }

    app.modal_state = ModalState::BunkerRules(state);
}

pub(super) fn handle_bunker_audit_modal_key(app: &mut App, key: KeyEvent) {
    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::BunkerAudit(state) => state,
        other => {
            app.modal_state = other;
            return;
        }
    };

    let close_to = |state: crate::ui::modal::BunkerAuditState| {
        state
            .return_to_settings
            .map(ModalState::AppSettings)
            .unwrap_or(ModalState::None)
    };

    let total = app.bunker_audit_entries.len();
    const APPROX_VISIBLE_ROWS: usize = 12;

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.modal_state = close_to(state);
            return;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            state.move_up();
        }
        KeyCode::Down | KeyCode::Char('j') => {
            state.move_down(total);
            if state.selected_index >= state.scroll_offset + APPROX_VISIBLE_ROWS {
                state.scroll_offset = state.selected_index + 1 - APPROX_VISIBLE_ROWS;
            }
        }
        KeyCode::Char('r') => {
            if let Err(e) = app.refresh_bunker_audit_entries() {
                app.set_warning_status(&format!("Failed to refresh bunker audit: {}", e));
            } else {
                app.set_warning_status("Refreshed bunker audit entries");
            }
        }
        _ => {}
    }

    app.modal_state = ModalState::BunkerAudit(state);
}

#[cfg(test)]
fn pop_next_bunker_request_id(
    queue: &mut VecDeque<tenex_core::nostr::bunker::BunkerSignRequest>,
    ids: &mut HashSet<String>,
) -> Option<String> {
    queue.pop_front().map(|request| {
        ids.remove(&request.request_id);
        request.request_id
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn approval_action_resolution_from_selection() {
        let request = tenex_core::nostr::bunker::BunkerSignRequest {
            request_id: "req-1".to_string(),
            requester_pubkey: "pubkey-1".to_string(),
            event_kind: Some(1),
            event_json: None,
            event_content: None,
            event_tags_json: None,
        };
        let mut state = crate::ui::modal::BunkerApprovalState::new(request);
        assert_eq!(state.selected_action(), BunkerApprovalAction::Approve);

        state.move_down();
        assert_eq!(state.selected_action(), BunkerApprovalAction::Reject);

        state.toggle_always_approve();
        assert!(state.always_approve);
    }

    #[test]
    fn queue_progression_pops_in_order_and_clears_ids() {
        let mut queue = VecDeque::new();
        let mut ids = HashSet::new();

        for idx in 1..=2 {
            let request_id = format!("req-{}", idx);
            ids.insert(request_id.clone());
            queue.push_back(tenex_core::nostr::bunker::BunkerSignRequest {
                request_id,
                requester_pubkey: "pubkey".to_string(),
                event_kind: Some(1),
                event_json: None,
                event_content: None,
                event_tags_json: None,
            });
        }

        let first = pop_next_bunker_request_id(&mut queue, &mut ids);
        assert_eq!(first.as_deref(), Some("req-1"));
        assert!(!ids.contains("req-1"));

        let second = pop_next_bunker_request_id(&mut queue, &mut ids);
        assert_eq!(second.as_deref(), Some("req-2"));
        assert!(!ids.contains("req-2"));
        assert!(queue.is_empty());
    }
}
