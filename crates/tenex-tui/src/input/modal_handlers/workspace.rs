use crossterm::event::{KeyCode, KeyEvent};

use crate::jaeger;
use crate::runtime::BrowseResult;
use crate::ui::modal::{
    AppearanceSetting, BunkerAuditState, BunkerRulesState, BunkerSetting, GeneralSetting,
    ModelBrowserState, VoiceBrowserState,
};
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
            workspaces.sort_by(|a, b| match (a.pinned, b.pinned) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
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
                        app.set_warning_status(&format!(
                            "Switched to workspace: {}",
                            workspace.name
                        ));
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
                        let is_pinned = app
                            .preferences
                            .borrow_mut()
                            .toggle_workspace_pinned(&workspace.id);
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
                        let project_ids: Vec<String> =
                            state.editing_project_ids.iter().cloned().collect();

                        if state.mode == WorkspaceMode::Create {
                            let ws = app
                                .preferences
                                .borrow_mut()
                                .add_workspace(name.clone(), project_ids);
                            app.set_warning_status(&format!("Created workspace: {}", name));
                            // Auto-activate the new workspace
                            let ws_project_ids = ws.project_ids.clone();
                            app.apply_workspace(Some(&ws.id), &ws_project_ids);
                        } else if let Some(ref id) = state.editing_workspace_id {
                            app.preferences.borrow_mut().update_workspace(
                                id,
                                name.clone(),
                                project_ids.clone(),
                            );
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

    // Delegate to voice browser if active
    if state.voice_browser.is_some() {
        handle_voice_browser_key(app, &mut state, key);
        app.modal_state = ModalState::AppSettings(state);
        return;
    }

    // Delegate to model browser if active
    if state.model_browser.is_some() {
        handle_model_browser_key(app, &mut state, key);
        app.modal_state = ModalState::AppSettings(state);
        return;
    }

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
                    ui::modal::SettingsTab::General => match state.selected_general_setting() {
                        Some(GeneralSetting::JaegerEndpoint) => {
                            state.jaeger_endpoint_input =
                                app.preferences.borrow().jaeger_endpoint().to_string();
                        }
                        None => {}
                    },
                    ui::modal::SettingsTab::AI => {
                        // Restore AI settings inputs from preferences
                        state.ai.elevenlabs_key_input.clear();
                        state.ai.openrouter_key_input.clear();
                        let prefs = app.preferences.borrow();
                        let ai = prefs.ai_audio_settings();
                        state.ai.voice_ids_input = ai.selected_voice_ids.join(", ");
                        state.ai.openrouter_model_input =
                            ai.openrouter_model.clone().unwrap_or_default();
                        state.ai.audio_prompt_input = ai.audio_prompt.clone();
                        state.ai.tts_inactivity_threshold_input =
                            ai.tts_inactivity_threshold_secs.to_string();
                    }
                    ui::modal::SettingsTab::Appearance => {
                        // Appearance settings are toggles/selects, no editing to restore
                    }
                    ui::modal::SettingsTab::Bunker => {
                        // Bunker tab uses action rows (no edit mode)
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
                                        let save_result = app
                                            .preferences
                                            .borrow_mut()
                                            .set_jaeger_endpoint(normalized);
                                        match save_result {
                                            Ok(()) => {
                                                app.set_warning_status("Jaeger endpoint saved");
                                                state.stop_editing();
                                            }
                                            Err(e) => {
                                                app.set_warning_status(&format!(
                                                    "Failed to save: {}",
                                                    e
                                                ));
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
                    ui::modal::SettingsTab::AI => match state.selected_ai_setting() {
                        Some(ui::modal::AiSetting::ElevenLabsApiKey) => {
                            let key = state.ai.elevenlabs_key_input.clone();
                            if !key.is_empty() {
                                match tenex_core::SecureStorage::set(
                                    tenex_core::SecureKey::ElevenLabsApiKey,
                                    &key,
                                ) {
                                    Ok(()) => {
                                        app.set_warning_status("ElevenLabs API key saved");
                                        state.ai.elevenlabs_key_input.clear();
                                        state.ai.elevenlabs_key_exists = true;
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
                                match tenex_core::SecureStorage::set(
                                    tenex_core::SecureKey::OpenRouterApiKey,
                                    &key,
                                ) {
                                    Ok(()) => {
                                        app.set_warning_status("OpenRouter API key saved");
                                        state.ai.openrouter_key_input.clear();
                                        state.ai.openrouter_key_exists = true;
                                        state.stop_editing();
                                    }
                                    Err(e) => {
                                        app.set_warning_status(&format!("Failed to save: {}", e));
                                    }
                                }
                            }
                        }
                        Some(ui::modal::AiSetting::SelectedVoiceIds) => {
                            let ids: Vec<String> = state
                                .ai
                                .voice_ids_input
                                .split(',')
                                .map(|s| s.trim().to_string())
                                .filter(|s| !s.is_empty())
                                .collect();
                            let result = app.preferences.borrow_mut().set_selected_voice_ids(ids);
                            match result {
                                Ok(()) => {
                                    app.set_warning_status("Voice IDs saved");
                                    state.stop_editing();
                                }
                                Err(e) => {
                                    app.set_warning_status(&format!("Failed to save: {}", e));
                                }
                            }
                        }
                        Some(ui::modal::AiSetting::OpenRouterModel) => {
                            let model = state.ai.openrouter_model_input.trim().to_string();
                            let model_opt = if model.is_empty() { None } else { Some(model) };
                            let result =
                                app.preferences.borrow_mut().set_openrouter_model(model_opt);
                            match result {
                                Ok(()) => {
                                    app.set_warning_status("OpenRouter model saved");
                                    state.stop_editing();
                                }
                                Err(e) => {
                                    app.set_warning_status(&format!("Failed to save: {}", e));
                                }
                            }
                        }
                        Some(ui::modal::AiSetting::AudioPrompt) => {
                            let prompt = state.ai.audio_prompt_input.clone();
                            let result = app.preferences.borrow_mut().set_audio_prompt(prompt);
                            match result {
                                Ok(()) => {
                                    app.set_warning_status("Audio prompt saved");
                                    state.stop_editing();
                                }
                                Err(e) => {
                                    app.set_warning_status(&format!("Failed to save: {}", e));
                                }
                            }
                        }
                        Some(ui::modal::AiSetting::TtsInactivityThreshold) => {
                            match state
                                .ai
                                .tts_inactivity_threshold_input
                                .trim()
                                .parse::<u64>()
                            {
                                Ok(secs) => {
                                    let result = app
                                        .preferences
                                        .borrow_mut()
                                        .set_tts_inactivity_threshold(secs);
                                    match result {
                                        Ok(()) => {
                                            app.set_warning_status(&format!(
                                                "TTS inactivity threshold set to {}s",
                                                secs
                                            ));
                                            state.stop_editing();
                                        }
                                        Err(e) => {
                                            app.set_warning_status(&format!(
                                                "Failed to save: {}",
                                                e
                                            ));
                                        }
                                    }
                                }
                                Err(_) => {
                                    app.set_warning_status(
                                        "Invalid value: enter a number of seconds",
                                    );
                                }
                            }
                        }
                        Some(ui::modal::AiSetting::AudioEnabled) | None => {
                            state.stop_editing();
                        }
                    },
                    ui::modal::SettingsTab::Appearance => {
                        // Appearance settings don't use text editing - stop editing mode
                        state.stop_editing();
                    }
                    ui::modal::SettingsTab::Bunker => {
                        state.stop_editing();
                    }
                }
            } else {
                // Handle toggle/cycle settings that don't require edit mode
                if state.current_tab == ui::modal::SettingsTab::AI
                    && state.selected_ai_setting() == Some(ui::modal::AiSetting::AudioEnabled)
                {
                    // AudioEnabled: toggle immediately instead of entering edit mode
                    let result = app.preferences.borrow_mut().toggle_audio_notifications();
                    match result {
                        Ok(new_state) => {
                            state.ai.audio_enabled = new_state;
                            let label = if new_state { "enabled" } else { "disabled" };
                            app.set_warning_status(&format!("Audio notifications {}", label));
                        }
                        Err(e) => {
                            app.set_warning_status(&format!("Failed to toggle: {}", e));
                        }
                    }
                } else if state.current_tab == ui::modal::SettingsTab::Appearance {
                    // Appearance tab settings are toggles/cycles - handle directly
                    match state.selected_appearance_setting() {
                        Some(AppearanceSetting::TimeFilter) => {
                            app.cycle_time_filter();
                            let label = app.home.time_filter.map(|tf| tf.label()).unwrap_or("All");
                            app.set_warning_status(&format!("Time filter: {}", label));
                        }
                        Some(AppearanceSetting::HideScheduled) => {
                            // cycle_scheduled_filter() sends its own notification
                            app.cycle_scheduled_filter();
                        }
                        None => {}
                    }
                } else if state.current_tab == ui::modal::SettingsTab::AI
                    && state.selected_ai_setting() == Some(ui::modal::AiSetting::SelectedVoiceIds)
                {
                    // Open voice browser if API key exists
                    let key = app.preferences.borrow().get_elevenlabs_api_key();
                    if let Some(api_key) = key {
                        let current_ids: Vec<String> = state
                            .ai
                            .voice_ids_input
                            .split(',')
                            .map(|s| s.trim().to_string())
                            .filter(|s| !s.is_empty())
                            .collect();
                        state.voice_browser = Some(VoiceBrowserState::new(current_ids));
                        // Spawn background fetch
                        if let Some(tx) = app.browse_tx.clone() {
                            tokio::spawn(async move {
                                let client = tenex_core::ai::ElevenLabsClient::new(api_key);
                                let result = client.get_voices().await.map_err(|e| e.to_string());
                                let _ = tx.send(BrowseResult::Voices(result)).await;
                            });
                        }
                    } else {
                        app.set_warning_status("Set ElevenLabs API key first");
                    }
                } else if state.current_tab == ui::modal::SettingsTab::AI
                    && state.selected_ai_setting() == Some(ui::modal::AiSetting::OpenRouterModel)
                {
                    // Open model browser if API key exists
                    let key = app.preferences.borrow().get_openrouter_api_key();
                    if let Some(api_key) = key {
                        let current_model = if state.ai.openrouter_model_input.trim().is_empty() {
                            None
                        } else {
                            Some(state.ai.openrouter_model_input.trim().to_string())
                        };
                        state.model_browser = Some(ModelBrowserState::new(current_model));
                        // Spawn background fetch
                        if let Some(tx) = app.browse_tx.clone() {
                            tokio::spawn(async move {
                                let client = tenex_core::ai::OpenRouterClient::new(api_key);
                                let result = client.get_models().await.map_err(|e| e.to_string());
                                let _ = tx.send(BrowseResult::Models(result)).await;
                            });
                        }
                    } else {
                        app.set_warning_status("Set OpenRouter API key first");
                    }
                } else if state.current_tab == ui::modal::SettingsTab::Bunker {
                    match state.selected_bunker_setting() {
                        Some(BunkerSetting::Enabled) => {
                            let target_enabled = !app.bunker_enabled();
                            match app.set_bunker_enabled(target_enabled) {
                                Ok(()) => {
                                    if target_enabled && app.keys.is_none() {
                                        app.set_warning_status(
                                            "Bunker enabled. It will auto-start after login.",
                                        );
                                    } else if target_enabled {
                                        app.set_warning_status("Bunker started");
                                    } else {
                                        app.set_warning_status("Bunker stopped");
                                    }
                                }
                                Err(e) => {
                                    app.set_warning_status(&format!(
                                        "Failed to update bunker setting: {}",
                                        e
                                    ));
                                }
                            }
                        }
                        Some(BunkerSetting::Uri) => {
                            if let Some(uri) = app.bunker_uri.clone() {
                                match arboard::Clipboard::new() {
                                    Ok(mut clipboard) => {
                                        if clipboard.set_text(uri).is_ok() {
                                            app.set_warning_status(
                                                "Bunker URI copied to clipboard",
                                            );
                                        } else {
                                            app.set_warning_status("Failed to copy bunker URI");
                                        }
                                    }
                                    Err(_) => app.set_warning_status("Failed to access clipboard"),
                                }
                            } else {
                                app.set_warning_status("Bunker is not running");
                            }
                        }
                        Some(BunkerSetting::Rules) => {
                            app.load_bunker_rules_from_preferences();
                            app.modal_state =
                                ModalState::BunkerRules(BunkerRulesState::new(Some(state)));
                            return;
                        }
                        Some(BunkerSetting::Audit) => {
                            if let Err(e) = app.refresh_bunker_audit_entries() {
                                app.set_warning_status(&format!(
                                    "Failed to refresh bunker audit: {}",
                                    e
                                ));
                            }
                            app.modal_state =
                                ModalState::BunkerAudit(BunkerAuditState::new(Some(state)));
                            return;
                        }
                        None => {}
                    }
                } else {
                    state.start_editing();
                }
            }
        }
        KeyCode::Char(c) if state.editing => {
            // Handle character input based on which tab and setting are being edited
            match state.current_tab {
                ui::modal::SettingsTab::General => match state.selected_general_setting() {
                    Some(GeneralSetting::JaegerEndpoint) => {
                        state.jaeger_endpoint_input.push(c);
                    }
                    None => {}
                },
                ui::modal::SettingsTab::AI => match state.selected_ai_setting() {
                    Some(ui::modal::AiSetting::ElevenLabsApiKey) => {
                        state.ai.elevenlabs_key_input.push(c);
                    }
                    Some(ui::modal::AiSetting::OpenRouterApiKey) => {
                        state.ai.openrouter_key_input.push(c);
                    }
                    Some(ui::modal::AiSetting::SelectedVoiceIds) => {
                        state.ai.voice_ids_input.push(c);
                    }
                    Some(ui::modal::AiSetting::OpenRouterModel) => {
                        state.ai.openrouter_model_input.push(c);
                    }
                    Some(ui::modal::AiSetting::AudioPrompt) => {
                        state.ai.audio_prompt_input.push(c);
                    }
                    Some(ui::modal::AiSetting::TtsInactivityThreshold) => {
                        state.ai.tts_inactivity_threshold_input.push(c);
                    }
                    _ => {}
                },
                ui::modal::SettingsTab::Appearance => {
                    // Appearance settings don't have text editing
                }
                ui::modal::SettingsTab::Bunker => {
                    // Bunker settings don't have text editing
                }
            }
        }
        KeyCode::Backspace if state.editing => {
            // Handle backspace based on which tab and setting are being edited
            match state.current_tab {
                ui::modal::SettingsTab::General => match state.selected_general_setting() {
                    Some(GeneralSetting::JaegerEndpoint) => {
                        state.jaeger_endpoint_input.pop();
                    }
                    None => {}
                },
                ui::modal::SettingsTab::AI => match state.selected_ai_setting() {
                    Some(ui::modal::AiSetting::ElevenLabsApiKey) => {
                        state.ai.elevenlabs_key_input.pop();
                    }
                    Some(ui::modal::AiSetting::OpenRouterApiKey) => {
                        state.ai.openrouter_key_input.pop();
                    }
                    Some(ui::modal::AiSetting::SelectedVoiceIds) => {
                        state.ai.voice_ids_input.pop();
                    }
                    Some(ui::modal::AiSetting::OpenRouterModel) => {
                        state.ai.openrouter_model_input.pop();
                    }
                    Some(ui::modal::AiSetting::AudioPrompt) => {
                        state.ai.audio_prompt_input.pop();
                    }
                    Some(ui::modal::AiSetting::TtsInactivityThreshold) => {
                        state.ai.tts_inactivity_threshold_input.pop();
                    }
                    _ => {}
                },
                ui::modal::SettingsTab::Appearance => {
                    // Appearance settings don't have text editing
                }
                ui::modal::SettingsTab::Bunker => {
                    // Bunker settings don't have text editing
                }
            }
        }
        KeyCode::Up if !state.editing => {
            state.move_up();
        }
        KeyCode::Down if !state.editing => {
            state.move_down();
        }
        KeyCode::Delete if !state.editing => {
            // Delete/clear settings when not editing
            if state.current_tab == ui::modal::SettingsTab::AI {
                match state.selected_ai_setting() {
                    Some(ui::modal::AiSetting::ElevenLabsApiKey) => {
                        if state.ai.elevenlabs_key_exists {
                            match tenex_core::SecureStorage::delete(
                                tenex_core::SecureKey::ElevenLabsApiKey,
                            ) {
                                Ok(()) => {
                                    app.set_warning_status("ElevenLabs API key deleted");
                                    state.ai.elevenlabs_key_exists = false;
                                }
                                Err(e) => {
                                    app.set_warning_status(&format!("Failed to delete: {}", e));
                                }
                            }
                        } else {
                            app.set_warning_status("No ElevenLabs API key to delete");
                        }
                    }
                    Some(ui::modal::AiSetting::OpenRouterApiKey) => {
                        if state.ai.openrouter_key_exists {
                            match tenex_core::SecureStorage::delete(
                                tenex_core::SecureKey::OpenRouterApiKey,
                            ) {
                                Ok(()) => {
                                    app.set_warning_status("OpenRouter API key deleted");
                                    state.ai.openrouter_key_exists = false;
                                }
                                Err(e) => {
                                    app.set_warning_status(&format!("Failed to delete: {}", e));
                                }
                            }
                        } else {
                            app.set_warning_status("No OpenRouter API key to delete");
                        }
                    }
                    Some(ui::modal::AiSetting::SelectedVoiceIds) => {
                        let result = app.preferences.borrow_mut().set_selected_voice_ids(vec![]);
                        match result {
                            Ok(()) => {
                                state.ai.voice_ids_input.clear();
                                app.set_warning_status("Voice IDs cleared");
                            }
                            Err(e) => {
                                app.set_warning_status(&format!("Failed to clear: {}", e));
                            }
                        }
                    }
                    Some(ui::modal::AiSetting::OpenRouterModel) => {
                        let result = app.preferences.borrow_mut().set_openrouter_model(None);
                        match result {
                            Ok(()) => {
                                state.ai.openrouter_model_input.clear();
                                app.set_warning_status("OpenRouter model cleared");
                            }
                            Err(e) => {
                                app.set_warning_status(&format!("Failed to clear: {}", e));
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        _ => {}
    }

    app.modal_state = ModalState::AppSettings(state);
}

fn handle_voice_browser_key(app: &mut App, state: &mut ui::modal::AppSettingsState, key: KeyEvent) {
    let browser = match state.voice_browser.as_mut() {
        Some(b) => b,
        None => return,
    };

    if browser.loading {
        // Only allow Esc while loading
        if key.code == KeyCode::Esc {
            state.voice_browser = None;
        }
        return;
    }

    let filtered_count = browser.filtered_items().len();

    match key.code {
        KeyCode::Esc => {
            state.voice_browser = None;
        }
        KeyCode::Up => {
            browser.move_up();
        }
        KeyCode::Down => {
            browser.move_down(filtered_count);
        }
        KeyCode::Char(' ') => {
            // Toggle selection on current item
            let filtered = browser.filtered_items();
            if let Some(item) = filtered.get(browser.selected_index) {
                let voice_id = item.voice_id.clone();
                browser.toggle_voice(&voice_id);
            }
        }
        KeyCode::Enter => {
            // Confirm selection — save voice IDs
            let ids = browser.selected_voice_ids.clone();
            let result = app
                .preferences
                .borrow_mut()
                .set_selected_voice_ids(ids.clone());
            match result {
                Ok(()) => {
                    state.ai.voice_ids_input = ids.join(", ");
                    app.set_warning_status(&format!("{} voice(s) saved", ids.len()));
                }
                Err(e) => {
                    app.set_warning_status(&format!("Failed to save: {}", e));
                }
            }
            state.voice_browser = None;
        }
        KeyCode::Char(c) => {
            browser.add_filter_char(c);
        }
        KeyCode::Backspace => {
            browser.backspace_filter();
        }
        _ => {}
    }
}

fn handle_model_browser_key(app: &mut App, state: &mut ui::modal::AppSettingsState, key: KeyEvent) {
    let browser = match state.model_browser.as_mut() {
        Some(b) => b,
        None => return,
    };

    if browser.loading {
        if key.code == KeyCode::Esc {
            state.model_browser = None;
        }
        return;
    }

    let filtered_count = browser.filtered_items().len();

    match key.code {
        KeyCode::Esc => {
            state.model_browser = None;
        }
        KeyCode::Up => {
            browser.move_up();
        }
        KeyCode::Down => {
            browser.move_down(filtered_count);
        }
        KeyCode::Enter => {
            // Select current model
            let filtered = browser.filtered_items();
            if let Some(item) = filtered.get(browser.selected_index) {
                let model_id = item.id.clone();
                let result = app
                    .preferences
                    .borrow_mut()
                    .set_openrouter_model(Some(model_id.clone()));
                match result {
                    Ok(()) => {
                        state.ai.openrouter_model_input = model_id.clone();
                        let display = item.name.as_deref().unwrap_or(&model_id);
                        app.set_warning_status(&format!("Model set: {}", display));
                    }
                    Err(e) => {
                        app.set_warning_status(&format!("Failed to save: {}", e));
                    }
                }
            }
            state.model_browser = None;
        }
        KeyCode::Char(c) => {
            browser.add_filter_char(c);
        }
        KeyCode::Backspace => {
            browser.backspace_filter();
        }
        _ => {}
    }
}
