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

/// Select the currently highlighted agent and mark it as an explicit user choice.
/// Shared by both the Enter and Shift+Enter handlers in the agent config modal.
fn select_active_agent(app: &mut App, state: &ui::modal::AgentConfigState) {
    if let Some(active_pubkey) = state.active_agent_pubkey.as_ref() {
        let selected_agent = app
            .available_agents()
            .into_iter()
            .find(|a| a.pubkey == *active_pubkey);
        if let Some(agent) = selected_agent {
            app.select_agent_explicit(agent);
        }
    }
}

pub(super) fn handle_agent_config_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::AgentConfigFocus;

    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::AgentConfig(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    let has_shift = key.modifiers.contains(KeyModifiers::SHIFT);
    let mut should_close = false;

    match key.code {
        KeyCode::Esc => {
            should_close = true;
        }
        KeyCode::Left => {
            state.focus = state.focus.prev();
        }
        KeyCode::Right => {
            state.focus = state.focus.next();
        }
        KeyCode::BackTab => {
            state.focus = state.focus.prev();
        }
        KeyCode::Tab if has_shift => {
            state.focus = state.focus.prev();
        }
        KeyCode::Tab => {
            state.focus = state.focus.next();
        }
        KeyCode::Up => match state.focus {
            AgentConfigFocus::Agents => {
                state.selector.index = state.selector.index.saturating_sub(1);
                app.refresh_agent_config_modal_state(&mut state);
            }
            AgentConfigFocus::Model => {
                if let Some(settings) = state.settings.as_mut() {
                    if settings.model_index > 0 {
                        settings.model_index -= 1;
                    }
                }
            }
            AgentConfigFocus::Tools => {
                if let Some(settings) = state.settings.as_mut() {
                    settings.move_cursor_up();
                }
            }
        },
        KeyCode::Down => match state.focus {
            AgentConfigFocus::Agents => {
                state.selector.index = state.selector.index.saturating_add(1);
                app.refresh_agent_config_modal_state(&mut state);
            }
            AgentConfigFocus::Model => {
                if let Some(settings) = state.settings.as_mut() {
                    if settings.model_index < settings.available_models.len().saturating_sub(1) {
                        settings.model_index += 1;
                    }
                }
            }
            AgentConfigFocus::Tools => {
                if let Some(settings) = state.settings.as_mut() {
                    settings.move_cursor_down();
                }
            }
        },
        KeyCode::Char(' ') => {
            if state.focus == AgentConfigFocus::Tools {
                if let Some(settings) = state.settings.as_mut() {
                    settings.toggle_at_cursor();
                }
            }
        }
        KeyCode::Char('a') if state.focus == AgentConfigFocus::Tools => {
            if let Some(settings) = state.settings.as_mut() {
                settings.toggle_group_all();
            }
        }
        // Ctrl+G: toggle save scope (project vs global)
        KeyCode::Char('g') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            state.save_globally = !state.save_globally;
        }
        // Ctrl+M: toggle PM marker
        KeyCode::Char('m') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            if let Some(settings) = state.settings.as_mut() {
                settings.is_pm = !settings.is_pm;
            }
        }
        KeyCode::Char(c)
            if state.focus == AgentConfigFocus::Agents
                && !key
                    .modifiers
                    .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT)
                && !has_shift =>
        {
            state.selector.add_filter_char(c);
            app.refresh_agent_config_modal_state(&mut state);
        }
        KeyCode::Backspace if state.focus == AgentConfigFocus::Agents => {
            state.selector.backspace_filter();
            app.refresh_agent_config_modal_state(&mut state);
        }
        KeyCode::Enter => {
            select_active_agent(app, &state);

            if state.has_config_changes() {
                if let Some(settings) = state.settings.as_ref() {
                    let agent_pubkey = settings.agent_pubkey.clone();
                    let model = settings.selected_model().map(str::to_string);
                    let tools = settings.selected_tools_vec();
                    let tags = if settings.is_pm {
                        vec!["pm".to_string()]
                    } else {
                        Vec::new()
                    };

                    if state.save_globally {
                        if let Some(ref core_handle) = app.core_handle {
                            if let Err(e) =
                                core_handle.send(NostrCommand::UpdateGlobalAgentConfig {
                                    agent_pubkey,
                                    model,
                                    tools,
                                    tags,
                                })
                            {
                                app.set_warning_status(&format!(
                                    "Failed to save global config: {}",
                                    e
                                ));
                            } else {
                                app.set_warning_status("Agent config saved globally");
                            }
                        }
                    } else {
                        match app.selected_project.as_ref().map(|p| p.a_tag()) {
                            Some(project_a_tag) => {
                                if let Some(ref core_handle) = app.core_handle {
                                    if let Err(e) =
                                        core_handle.send(NostrCommand::UpdateAgentConfig {
                                            project_a_tag,
                                            agent_pubkey,
                                            model,
                                            tools,
                                            tags,
                                        })
                                    {
                                        app.set_warning_status(&format!(
                                            "Failed to save config: {}",
                                            e
                                        ));
                                    } else {
                                        app.set_warning_status("Agent config saved for project");
                                    }
                                }
                            }
                            None => {
                                app.set_warning_status("No active project — toggle 'g' for global");
                            }
                        }
                    }
                }
            }

            should_close = true;
        }
        _ => {}
    }

    if should_close {
        app.modal_state = ModalState::None;
    } else {
        app.modal_state = ModalState::AgentConfig(state);
    }
}

/// Handle key events for the agent deletion confirmation dialog (kind:24030)
pub(super) fn handle_agent_deletion_key(app: &mut App, key: KeyEvent) {
    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::AgentDeletion(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    match key.code {
        KeyCode::Esc => {
            // Cancel — return to project settings
            app.modal_state = ModalState::None;
        }
        KeyCode::Up | KeyCode::Down => {
            state.toggle_action();
            app.modal_state = ModalState::AgentDeletion(state);
        }
        KeyCode::Tab => {
            // Tab toggles scope between Project and Global
            state.toggle_scope();
            app.modal_state = ModalState::AgentDeletion(state);
        }
        KeyCode::Enter => {
            if state.selected_index == 1 {
                // Deletion confirmed — publish the kind:24030 event
                let project_a_tag = match state.scope {
                    ui::modal::AgentDeletionScope::Project => Some(state.project_a_tag.clone()),
                    ui::modal::AgentDeletionScope::Global => None,
                };

                if let Some(ref core_handle) = app.core_handle {
                    if let Err(e) = core_handle.send(NostrCommand::DeleteAgent {
                        agent_pubkey: state.agent_pubkey.clone(),
                        project_a_tag,
                        reason: None,
                        client: Some("tenex-tui".to_string()),
                    }) {
                        app.set_warning_status(&format!("Failed to publish agent deletion: {}", e));
                    } else {
                        let scope_str = match state.scope {
                            ui::modal::AgentDeletionScope::Project => "project",
                            ui::modal::AgentDeletionScope::Global => "global",
                        };
                        app.set_warning_status(&format!(
                            "Agent '{}' deletion event published ({})",
                            state.agent_name, scope_str
                        ));
                    }
                } else {
                    app.set_warning_status("Error: Not connected to relays");
                }
            }
            app.modal_state = ModalState::None;
        }
        _ => {
            app.modal_state = ModalState::AgentDeletion(state);
        }
    }
}
