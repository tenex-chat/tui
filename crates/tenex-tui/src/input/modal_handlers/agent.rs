use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::nostr::NostrCommand;
use crate::ui::{self, App, ModalState};

pub(super) fn handle_create_agent_key(app: &mut App, key: KeyEvent) {
    use ui::modal::{AgentCreateStep, AgentFormFocus};

    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);

    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::CreateAgent(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    match state.step {
        AgentCreateStep::Basics => match code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
                return;
            }
            KeyCode::Tab => {
                state.focus = match state.focus {
                    AgentFormFocus::Name => AgentFormFocus::Description,
                    AgentFormFocus::Description => AgentFormFocus::Role,
                    AgentFormFocus::Role => AgentFormFocus::Name,
                };
            }
            KeyCode::Enter => {
                if state.can_proceed() {
                    state.step = AgentCreateStep::Instructions;
                }
            }
            KeyCode::Char(c) => match state.focus {
                AgentFormFocus::Name => state.name.push(c),
                AgentFormFocus::Description => state.description.push(c),
                AgentFormFocus::Role => state.role.push(c),
            },
            KeyCode::Backspace => match state.focus {
                AgentFormFocus::Name => {
                    state.name.pop();
                }
                AgentFormFocus::Description => {
                    state.description.pop();
                }
                AgentFormFocus::Role => {
                    state.role.pop();
                }
            },
            _ => {}
        },
        AgentCreateStep::Instructions => match code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
                return;
            }
            KeyCode::Enter if has_ctrl => {
                state.step = AgentCreateStep::Review;
                state.instructions_scroll = 0;
            }
            KeyCode::Enter => {
                state.instructions.insert(state.instructions_cursor, '\n');
                state.instructions_cursor += 1;
            }
            KeyCode::Backspace => {
                if state.instructions_cursor > 0 {
                    state.instructions_cursor -= 1;
                    state.instructions.remove(state.instructions_cursor);
                } else if state.instructions.is_empty() {
                    state.step = AgentCreateStep::Basics;
                }
            }
            KeyCode::Char(c) => {
                state.instructions.insert(state.instructions_cursor, c);
                state.instructions_cursor += 1;
            }
            KeyCode::Left => {
                if state.instructions_cursor > 0 {
                    state.instructions_cursor -= 1;
                }
            }
            KeyCode::Right => {
                if state.instructions_cursor < state.instructions.len() {
                    state.instructions_cursor += 1;
                }
            }
            KeyCode::Up => {
                let current_line_start = state.instructions[..state.instructions_cursor]
                    .rfind('\n')
                    .map(|pos| pos + 1)
                    .unwrap_or(0);
                let col = state.instructions_cursor - current_line_start;

                if let Some(prev_line_end) =
                    state.instructions[..current_line_start.saturating_sub(1)].rfind('\n')
                {
                    let prev_line_start = prev_line_end + 1;
                    let prev_line_len = current_line_start.saturating_sub(1) - prev_line_start;
                    state.instructions_cursor = prev_line_start + col.min(prev_line_len);
                } else if current_line_start > 0 {
                    state.instructions_cursor = col.min(current_line_start.saturating_sub(1));
                }
            }
            KeyCode::Down => {
                let current_line_start = state.instructions[..state.instructions_cursor]
                    .rfind('\n')
                    .map(|pos| pos + 1)
                    .unwrap_or(0);
                let col = state.instructions_cursor - current_line_start;

                if let Some(next_line_start_offset) =
                    state.instructions[state.instructions_cursor..].find('\n')
                {
                    let next_line_start = state.instructions_cursor + next_line_start_offset + 1;
                    let next_line_end = state.instructions[next_line_start..]
                        .find('\n')
                        .map(|pos| next_line_start + pos)
                        .unwrap_or(state.instructions.len());
                    let next_line_len = next_line_end - next_line_start;
                    state.instructions_cursor = next_line_start + col.min(next_line_len);
                }
            }
            KeyCode::Home => {
                state.instructions_cursor = state.instructions[..state.instructions_cursor]
                    .rfind('\n')
                    .map(|pos| pos + 1)
                    .unwrap_or(0);
            }
            KeyCode::End => {
                state.instructions_cursor = state.instructions[state.instructions_cursor..]
                    .find('\n')
                    .map(|pos| state.instructions_cursor + pos)
                    .unwrap_or(state.instructions.len());
            }
            _ => {}
        },
        AgentCreateStep::Review => match code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
                return;
            }
            KeyCode::Backspace => {
                state.step = AgentCreateStep::Instructions;
                state.instructions_scroll = 0;
            }
            KeyCode::Up => {
                if state.instructions_scroll > 0 {
                    state.instructions_scroll -= 1;
                }
            }
            KeyCode::Down => {
                let line_count = state.instructions.lines().count();
                if state.instructions_scroll + 1 < line_count {
                    state.instructions_scroll += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(ref core_handle) = app.core_handle {
                    if let Err(e) = core_handle.send(NostrCommand::CreateAgentDefinition {
                        name: state.name.clone(),
                        description: state.description.clone(),
                        role: state.role.clone(),
                        instructions: state.instructions.clone(),
                        version: state.version.clone(),
                        source_id: state.source_id.clone(),
                        is_fork: matches!(state.mode, ui::modal::AgentCreateMode::Fork),
                    }) {
                        app.set_warning_status(&format!("Failed to create agent: {}", e));
                    } else {
                        app.set_warning_status(&format!("Agent '{}' created", state.name));
                    }
                }
                app.modal_state = ModalState::None;
                return;
            }
            _ => {}
        },
    }

    app.modal_state = ModalState::CreateAgent(state);
}

pub(super) fn handle_agent_settings_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::AgentSettingsFocus;

    if let ModalState::AgentSettings(ref mut state) = app.modal_state {
        match key.code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
            }
            KeyCode::Tab => {
                state.focus = match state.focus {
                    AgentSettingsFocus::Model => AgentSettingsFocus::Tools,
                    AgentSettingsFocus::Tools => AgentSettingsFocus::Model,
                };
            }
            KeyCode::Up => match state.focus {
                AgentSettingsFocus::Model => {
                    if state.model_index > 0 {
                        state.model_index -= 1;
                    }
                }
                AgentSettingsFocus::Tools => {
                    state.move_cursor_up();
                }
            },
            KeyCode::Down => match state.focus {
                AgentSettingsFocus::Model => {
                    if state.model_index < state.available_models.len().saturating_sub(1) {
                        state.model_index += 1;
                    }
                }
                AgentSettingsFocus::Tools => {
                    state.move_cursor_down();
                }
            },
            KeyCode::Char(' ') => {
                if state.focus == AgentSettingsFocus::Tools {
                    state.toggle_at_cursor();
                }
            }
            KeyCode::Char('a') => {
                if state.focus == AgentSettingsFocus::Tools {
                    state.toggle_group_all();
                }
            }
            KeyCode::Enter => {
                let project_a_tag = state.project_a_tag.clone();
                let agent_pubkey = state.agent_pubkey.clone();
                let model = state.selected_model().map(|s| s.to_string());
                let tools = state.selected_tools_vec();

                if let Some(ref core_handle) = app.core_handle {
                    if let Err(e) = core_handle.send(NostrCommand::UpdateAgentConfig {
                        project_a_tag,
                        agent_pubkey,
                        model,
                        tools,
                    }) {
                        app.set_warning_status(&format!("Failed to update agent config: {}", e));
                    } else {
                        app.set_warning_status("Agent config update sent");
                    }
                }
                app.modal_state = ModalState::None;
            }
            _ => {}
        }
    }
}
