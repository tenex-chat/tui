use crossterm::event::{KeyCode, KeyEvent};

use crate::jaeger;
use crate::ui::modal::GeneralSetting;
use crate::ui::{self, App, ModalState};

pub(super) fn handle_workspace_manager_key(app: &mut App, key: KeyEvent) {
    use ui::modal::{WorkspaceFocus, WorkspaceMode};

    let state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::WorkspaceManager(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    let mut state = state;

    match state.mode {
        WorkspaceMode::List => {
            // Sort workspaces same as renderer: pinned first, then by name
            let mut workspaces = app.preferences.borrow().workspaces().to_vec();
            workspaces.sort_by(|a, b| {
                match (a.pinned, b.pinned) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => a.name.cmp(&b.name),
                }
            });
            let workspace_count = workspaces.len();

            match key.code {
                KeyCode::Esc | KeyCode::Char('q') => {
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Up | KeyCode::Char('k') => {
                    state.move_up();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    state.move_down(workspace_count);
                }
                KeyCode::Enter => {
                    // Switch to selected workspace
                    if let Some(workspace) = workspaces.get(state.selected_index) {
                        let workspace_id = workspace.id.clone();
                        let project_ids = workspace.project_ids.clone();
                        app.apply_workspace(Some(&workspace_id), &project_ids);
                        app.set_warning_status(&format!("Switched to workspace: {}", workspace.name));
                        app.modal_state = ModalState::None;
                        return;
                    }
                }
                KeyCode::Char('n') => {
                    // Create new workspace
                    state.enter_create_mode();
                }
                KeyCode::Char('e') => {
                    // Edit selected workspace
                    if let Some(workspace) = workspaces.get(state.selected_index) {
                        state.enter_edit_mode(workspace);
                    }
                }
                KeyCode::Char('d') => {
                    // Delete selected workspace
                    if !workspaces.is_empty() {
                        state.enter_delete_mode();
                    }
                }
                KeyCode::Char('p') => {
                    // Toggle pin on selected workspace
                    if let Some(workspace) = workspaces.get(state.selected_index) {
                        let is_pinned = app.preferences.borrow_mut().toggle_workspace_pinned(&workspace.id);
                        let msg = if is_pinned { "pinned" } else { "unpinned" };
                        app.set_warning_status(&format!("Workspace {}", msg));
                    }
                }
                KeyCode::Backspace => {
                    // Clear active workspace (show all projects)
                    app.apply_workspace(None, &[]);
                    app.set_warning_status("Showing all projects");
                    app.modal_state = ModalState::None;
                    return;
                }
                _ => {}
            }
        }
        WorkspaceMode::Create | WorkspaceMode::Edit => {
            // Get projects for the selector
            let projects: Vec<_> = {
                let store = app.data_store.borrow();
                store.get_projects().to_vec()
            };
            let project_count = projects.len();

            match key.code {
                KeyCode::Esc => {
                    state.back_to_list();
                }
                KeyCode::Tab => {
                    // Switch focus between Name and Projects
                    state.focus = match state.focus {
                        WorkspaceFocus::Name => WorkspaceFocus::Projects,
                        WorkspaceFocus::Projects => WorkspaceFocus::Name,
                    };
                }
                KeyCode::Up | KeyCode::Char('k') if state.focus == WorkspaceFocus::Projects => {
                    if state.project_selector_index > 0 {
                        state.project_selector_index -= 1;
                    }
                }
                KeyCode::Down | KeyCode::Char('j') if state.focus == WorkspaceFocus::Projects => {
                    if state.project_selector_index + 1 < project_count {
                        state.project_selector_index += 1;
                    }
                }
                KeyCode::Char(' ') if state.focus == WorkspaceFocus::Projects => {
                    // Toggle project selection
                    if let Some(project) = projects.get(state.project_selector_index) {
                        state.toggle_project(&project.a_tag());
                    }
                }
                KeyCode::Enter => {
                    if state.can_save() {
                        // Save workspace
                        let name = state.editing_name.clone();
                        let project_ids: Vec<String> = state.editing_project_ids.iter().cloned().collect();

                        if state.mode == WorkspaceMode::Create {
                            let ws = app.preferences.borrow_mut().add_workspace(name.clone(), project_ids);
                            app.set_warning_status(&format!("Created workspace: {}", name));
                            // Auto-activate the new workspace
                            let ws_project_ids = ws.project_ids.clone();
                            app.apply_workspace(Some(&ws.id), &ws_project_ids);
                        } else if let Some(ref id) = state.editing_workspace_id {
                            app.preferences.borrow_mut().update_workspace(id, name.clone(), project_ids.clone());
                            app.set_warning_status(&format!("Updated workspace: {}", name));
                            // If editing the active workspace, re-apply it
                            if app.preferences.borrow().active_workspace_id() == Some(id.as_str()) {
                                app.apply_workspace(Some(id), &project_ids);
                            }
                        }
                        app.modal_state = ModalState::None;
                        return;
                    }
                }
                KeyCode::Char(c) if state.focus == WorkspaceFocus::Name => {
                    state.editing_name.push(c);
                }
                KeyCode::Backspace if state.focus == WorkspaceFocus::Name => {
                    state.editing_name.pop();
                }
                _ => {}
            }
        }
        WorkspaceMode::Delete => {
            let workspaces = app.preferences.borrow().workspaces().to_vec();

            match key.code {
                KeyCode::Esc => {
                    state.back_to_list();
                }
                KeyCode::Enter | KeyCode::Char('d') => {
                    // Confirm delete
                    if let Some(workspace) = workspaces.get(state.selected_index) {
                        let name = workspace.name.clone();
                        app.preferences.borrow_mut().delete_workspace(&workspace.id);
                        app.set_warning_status(&format!("Deleted workspace: {}", name));
                        state.selected_index = state.selected_index.saturating_sub(1);
                        state.back_to_list();
                    }
                }
                _ => {}
            }
        }
    }

    app.modal_state = ModalState::WorkspaceManager(state);
}

pub(super) fn handle_app_settings_key(app: &mut App, key: KeyEvent) {
    let state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::AppSettings(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    let mut state = state;

    match key.code {
        KeyCode::Tab if !state.editing => {
            // Switch tabs when not editing
            state.next_tab();
        }
        KeyCode::BackTab if !state.editing => {
            // Switch tabs backwards when not editing
            state.prev_tab();
        }
        KeyCode::Esc => {
            if state.editing {
                // Cancel editing, restore original value based on which setting was selected
                state.stop_editing();
                match state.current_tab {
                    ui::modal::SettingsTab::General => {
                        match state.selected_general_setting() {
                            Some(GeneralSetting::JaegerEndpoint) => {
                                state.jaeger_endpoint_input =
                                    app.preferences.borrow().jaeger_endpoint().to_string();
                            }
                            None => {}
                        }
                    }
                    ui::modal::SettingsTab::AI => {
                        // Restore AI settings inputs if needed
                        state.ai.elevenlabs_key_input.clear();
                        state.ai.openrouter_key_input.clear();
                    }
                }
                app.modal_state = ModalState::AppSettings(state);
            } else {
                // Close the modal
                app.modal_state = ModalState::None;
            }
            return;
        }
        KeyCode::Enter => {
            if state.editing {
                // Save the value based on which tab and setting are selected
                match state.current_tab {
                    ui::modal::SettingsTab::General => {
                        match state.selected_general_setting() {
                            Some(GeneralSetting::JaegerEndpoint) => {
                                let new_endpoint = state.jaeger_endpoint_input.clone();

                                // Validate the endpoint before saving
                                match jaeger::validate_and_normalize_endpoint(&new_endpoint) {
                                    Ok(normalized) => {
                                        // Save the normalized endpoint
                                        let save_result =
                                            app.preferences.borrow_mut().set_jaeger_endpoint(normalized);
                                        match save_result {
                                            Ok(()) => {
                                                app.set_warning_status("Jaeger endpoint saved");
                                                state.stop_editing();
                                            }
                                            Err(e) => {
                                                app.set_warning_status(&format!("Failed to save: {}", e));
                                                // Don't stop editing - let user fix the issue
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        // Validation failed - show error and keep editing
                                        app.set_warning_status(&format!("Invalid endpoint: {}", e));
                                        // Don't stop editing - let user fix the issue
                                    }
                                }
                            }
                            None => {
                                state.stop_editing();
                            }
                        }
                    }
                    ui::modal::SettingsTab::AI => {
                        match state.selected_ai_setting() {
                            Some(ui::modal::AiSetting::Enabled) => {
                                // TODO: Toggle enabled state
                                app.set_warning_status("Audio notifications toggle not yet implemented");
                            }
                            Some(ui::modal::AiSetting::ElevenLabsApiKey) => {
                                let key = state.ai.elevenlabs_key_input.clone();
                                if !key.is_empty() {
                                    // Save the API key to secure storage
                                    match tenex_core::SecureStorage::set(
                                        tenex_core::SecureKey::ElevenLabsApiKey,
                                        &key
                                    ) {
                                        Ok(()) => {
                                            app.set_warning_status("ElevenLabs API key saved");
                                            state.ai.elevenlabs_key_input.clear();
                                            state.stop_editing();
                                        }
                                        Err(e) => {
                                            app.set_warning_status(&format!("Failed to save: {}", e));
                                        }
                                    }
                                }
                            }
                            Some(ui::modal::AiSetting::OpenRouterApiKey) => {
                                let key = state.ai.openrouter_key_input.clone();
                                if !key.is_empty() {
                                    // Save the API key to secure storage
                                    match tenex_core::SecureStorage::set(
                                        tenex_core::SecureKey::OpenRouterApiKey,
                                        &key
                                    ) {
                                        Ok(()) => {
                                            app.set_warning_status("OpenRouter API key saved");
                                            state.ai.openrouter_key_input.clear();
                                            state.stop_editing();
                                        }
                                        Err(e) => {
                                            app.set_warning_status(&format!("Failed to save: {}", e));
                                        }
                                    }
                                }
                            }
                            Some(ui::modal::AiSetting::SelectedVoices) => {
                                // TODO: Open voice picker modal
                                app.set_warning_status("Voice selection not yet implemented");
                            }
                            Some(ui::modal::AiSetting::OpenRouterModel) => {
                                // TODO: Open model picker modal
                                app.set_warning_status("Model selection not yet implemented");
                            }
                            Some(ui::modal::AiSetting::AudioPrompt) => {
                                // TODO: Open text editor modal
                                app.set_warning_status("Prompt editing not yet implemented");
                            }
                            None => {
                                state.stop_editing();
                            }
                        }
                    }
                }
            } else {
                // Start editing the currently selected setting
                state.start_editing();
            }
        }
        KeyCode::Char(c) if state.editing => {
            // Handle character input based on which tab and setting are being edited
            match state.current_tab {
                ui::modal::SettingsTab::General => {
                    match state.selected_general_setting() {
                        Some(GeneralSetting::JaegerEndpoint) => {
                            state.jaeger_endpoint_input.push(c);
                        }
                        None => {}
                    }
                }
                ui::modal::SettingsTab::AI => {
                    match state.selected_ai_setting() {
                        Some(ui::modal::AiSetting::ElevenLabsApiKey) => {
                            state.ai.elevenlabs_key_input.push(c);
                        }
                        Some(ui::modal::AiSetting::OpenRouterApiKey) => {
                            state.ai.openrouter_key_input.push(c);
                        }
                        // Other AI settings don't support character input
                        Some(ui::modal::AiSetting::Enabled) |
                        Some(ui::modal::AiSetting::SelectedVoices) |
                        Some(ui::modal::AiSetting::OpenRouterModel) |
                        Some(ui::modal::AiSetting::AudioPrompt) |
                        None => {}
                    }
                }
            }
        }
        KeyCode::Backspace if state.editing => {
            // Handle backspace based on which tab and setting are being edited
            match state.current_tab {
                ui::modal::SettingsTab::General => {
                    match state.selected_general_setting() {
                        Some(GeneralSetting::JaegerEndpoint) => {
                            state.jaeger_endpoint_input.pop();
                        }
                        None => {}
                    }
                }
                ui::modal::SettingsTab::AI => {
                    match state.selected_ai_setting() {
                        Some(ui::modal::AiSetting::ElevenLabsApiKey) => {
                            state.ai.elevenlabs_key_input.pop();
                        }
                        Some(ui::modal::AiSetting::OpenRouterApiKey) => {
                            state.ai.openrouter_key_input.pop();
                        }
                        // Other AI settings don't support backspace
                        Some(ui::modal::AiSetting::Enabled) |
                        Some(ui::modal::AiSetting::SelectedVoices) |
                        Some(ui::modal::AiSetting::OpenRouterModel) |
                        Some(ui::modal::AiSetting::AudioPrompt) |
                        None => {}
                    }
                }
            }
        }
        KeyCode::Up if !state.editing => {
            state.move_up();
        }
        KeyCode::Down if !state.editing => {
            state.move_down();
        }
        _ => {}
    }

    app.modal_state = ModalState::AppSettings(state);
}
