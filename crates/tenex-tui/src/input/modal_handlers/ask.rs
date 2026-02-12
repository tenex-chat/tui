use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::nostr::NostrCommand;
use crate::ui::{App, InputMode, ModalState};

pub(super) fn handle_ask_modal_key(app: &mut App, key: KeyEvent) {
    use crate::ui::ask_input::InputMode as AskInputMode;

    let code = key.code;
    let modifiers = key.modifiers;
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    // Extract modal_state to avoid borrow issues
    let modal_state = match app.ask_modal_state_mut() {
        Some(state) => state,
        None => return,
    };

    let input_state = &mut modal_state.input_state;

    match input_state.mode {
        AskInputMode::Selection => match code {
            KeyCode::Up | KeyCode::Char('k') if !input_state.is_custom_option_selected() => input_state.prev_option(),
            KeyCode::Up if input_state.is_custom_option_selected() && input_state.custom_input.is_empty() => input_state.prev_option(),
            KeyCode::Down | KeyCode::Char('j') if !input_state.is_custom_option_selected() => input_state.next_option(),
            KeyCode::Right if !input_state.is_custom_option_selected() => {
                input_state.skip_question();
                if input_state.is_complete() {
                    submit_ask_response(app);
                }
            }
            KeyCode::Left if !input_state.is_custom_option_selected() => input_state.prev_question(),
            // When on custom option with text, Left/Right move cursor
            KeyCode::Left if input_state.is_custom_option_selected() => {
                if input_state.custom_input.is_empty() {
                    input_state.prev_question();
                } else {
                    input_state.move_cursor_left();
                }
            }
            KeyCode::Right if input_state.is_custom_option_selected() => {
                if input_state.custom_input.is_empty() {
                    input_state.skip_question();
                    if input_state.is_complete() {
                        submit_ask_response(app);
                    }
                } else {
                    input_state.move_cursor_right();
                }
            }
            KeyCode::Char(' ') if input_state.is_multi_select() && !input_state.is_custom_option_selected() => {
                input_state.toggle_multi_select();
            }
            KeyCode::Enter => {
                // If on custom option with text, submit the custom answer
                if input_state.is_custom_option_selected() && !input_state.custom_input.trim().is_empty() {
                    input_state.submit_custom_answer();
                    if input_state.is_complete() {
                        submit_ask_response(app);
                    }
                } else if !input_state.is_custom_option_selected() {
                    input_state.select_current_option();
                    if input_state.is_complete() {
                        submit_ask_response(app);
                    }
                }
                // If on custom option with no text, Enter does nothing (need to type something)
            }
            KeyCode::Esc => {
                // If on custom option with text, clear the text first
                if input_state.is_custom_option_selected() && !input_state.custom_input.is_empty() {
                    input_state.custom_input.clear();
                    input_state.custom_cursor = 0;
                } else {
                    app.close_ask_modal();
                }
            }
            // Backspace on custom option deletes characters
            KeyCode::Backspace if input_state.is_custom_option_selected() => {
                input_state.delete_char();
            }
            // Any character typed on custom option starts inline input
            KeyCode::Char(c) if input_state.is_custom_option_selected() => {
                input_state.insert_char(c);
            }
            _ => {}
        },
        AskInputMode::CustomInput => match code {
            KeyCode::Enter if has_shift => input_state.insert_char('\n'),
            KeyCode::Enter => {
                input_state.submit_custom_answer();
                if input_state.is_complete() {
                    submit_ask_response(app);
                }
            }
            KeyCode::Esc => input_state.cancel_custom_mode(),
            KeyCode::Left => input_state.move_cursor_left(),
            KeyCode::Right => input_state.move_cursor_right(),
            KeyCode::Backspace => input_state.delete_char(),
            KeyCode::Char(c) => input_state.insert_char(c),
            _ => {}
        },
    }
}

fn submit_ask_response(app: &mut App) {
    let modal_state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::AskModal(state) => state,
        other => {
            app.modal_state = other;
            return;
        }
    };

    let response_text = modal_state.input_state.format_response();
    let message_id = modal_state.message_id;
    let ask_author_pubkey = modal_state.ask_author_pubkey;

    if let (Some(ref core_handle), Some(thread), Some(ref project)) =
        (&app.core_handle, app.selected_thread(), &app.selected_project)
    {
        let _ = core_handle.send(NostrCommand::PublishMessage {
            thread_id: thread.id.clone(),
            project_a_tag: project.a_tag(),
            content: response_text,
            agent_pubkey: None,
            reply_to: Some(message_id),
            nudge_ids: vec![],
            ask_author_pubkey: Some(ask_author_pubkey),
            response_tx: None,
        });
    }

    app.input_mode = InputMode::Editing;
}
