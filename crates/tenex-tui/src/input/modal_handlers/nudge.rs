use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::nostr::NostrCommand;
use crate::ui::{self, App, ModalState};

pub(super) fn handle_nudge_list_key(app: &mut App, key: KeyEvent) {
    use ui::modal::{NudgeDetailState, NudgeDeleteConfirmState};
    use ui::nudge::NudgeFormState;

    let state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::NudgeList(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    // Get nudge count for navigation
    let nudge_count = app.data_store.borrow().content.nudges.len();

    match key.code {
        KeyCode::Esc => {
            // Close modal
            app.modal_state = ModalState::None;
            return;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let mut new_state = state;
            new_state.move_up();
            app.modal_state = ModalState::NudgeList(new_state);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            let mut new_state = state;
            new_state.move_down(nudge_count);
            app.modal_state = ModalState::NudgeList(new_state);
        }
        KeyCode::Char('n') => {
            // Create new nudge
            app.modal_state = ModalState::NudgeCreate(NudgeFormState::new());
        }
        KeyCode::Char('c') => {
            // Copy selected nudge (pre-populate wizard with nudge data to create a new one)
            let nudge_id = get_selected_nudge_id(app, &state);
            if let Some(id) = nudge_id {
                let nudge = app.data_store.borrow().content.nudges.get(&id).cloned();
                if let Some(nudge) = nudge {
                    app.modal_state = ModalState::NudgeCreate(NudgeFormState::copy_from_nudge(&nudge));
                } else {
                    app.modal_state = ModalState::NudgeList(state);
                }
            } else {
                app.modal_state = ModalState::NudgeList(state);
            }
        }
        KeyCode::Char('d') => {
            // Delete selected nudge
            let nudge_id = get_selected_nudge_id(app, &state);
            if let Some(id) = nudge_id {
                app.modal_state = ModalState::NudgeDeleteConfirm(NudgeDeleteConfirmState::new(id));
            } else {
                app.modal_state = ModalState::NudgeList(state);
            }
        }
        KeyCode::Enter => {
            // View selected nudge
            let nudge_id = get_selected_nudge_id(app, &state);
            if let Some(id) = nudge_id {
                app.modal_state = ModalState::NudgeDetail(NudgeDetailState::new(id));
            } else {
                app.modal_state = ModalState::NudgeList(state);
            }
        }
        KeyCode::Char(c) if !key.modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) => {
            // Filter input
            let mut new_state = state;
            new_state.add_filter_char(c);
            app.modal_state = ModalState::NudgeList(new_state);
        }
        KeyCode::Backspace => {
            let mut new_state = state;
            new_state.backspace_filter();
            app.modal_state = ModalState::NudgeList(new_state);
        }
        _ => {
            app.modal_state = ModalState::NudgeList(state);
        }
    }
}
fn get_selected_nudge_id(app: &App, state: &ui::modal::NudgeListState) -> Option<String> {
    let data_store = app.data_store.borrow();
    let filter_lower = state.filter.to_lowercase();

    let mut filtered: Vec<&tenex_core::models::Nudge> = data_store
        .content
        .nudges
        .values()
        .filter(|n| {
            if state.filter.is_empty() {
                return true;
            }
            n.title.to_lowercase().contains(&filter_lower)
                || n.description.to_lowercase().contains(&filter_lower)
                || n.content.to_lowercase().contains(&filter_lower)
                || n.hashtags.iter().any(|h| h.to_lowercase().contains(&filter_lower))
        })
        .collect();

    filtered.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    filtered.get(state.selected_index).map(|n| n.id.clone())
}

pub(super) fn handle_nudge_form_key(app: &mut App, key: KeyEvent) {
    use ui::nudge::{NudgeFormFocus, NudgeFormStep, PermissionMode};

    let state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::NudgeCreate(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    let mut state = state;

    match state.step {
        NudgeFormStep::Basics => {
            match key.code {
                KeyCode::Esc => {
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Tab => {
                    state.focus = state.focus.next();
                }
                KeyCode::BackTab => {
                    state.focus = state.focus.prev();
                }
                KeyCode::Enter => {
                    if state.focus == NudgeFormFocus::Hashtags {
                        // Add hashtag
                        state.add_hashtag();
                    } else if state.can_proceed() {
                        state.next_step();
                    }
                }
                KeyCode::Char(' ') if state.focus == NudgeFormFocus::Hashtags => {
                    // Space adds hashtag in hashtag field
                    state.add_hashtag();
                }
                KeyCode::Char(c) => {
                    match state.focus {
                        NudgeFormFocus::Title => state.title.push(c),
                        NudgeFormFocus::Description => state.description.push(c),
                        NudgeFormFocus::Hashtags => state.hashtag_input.push(c),
                    }
                }
                KeyCode::Backspace => {
                    match state.focus {
                        NudgeFormFocus::Title => { state.title.pop(); }
                        NudgeFormFocus::Description => { state.description.pop(); }
                        NudgeFormFocus::Hashtags => {
                            if state.hashtag_input.is_empty() {
                                state.hashtags.pop();
                            } else {
                                state.hashtag_input.pop();
                            }
                        }
                    }
                }
                _ => {}
            }
        }
        NudgeFormStep::Content => {
            match key.code {
                KeyCode::Esc => {
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Tab if key.modifiers.contains(KeyModifiers::SHIFT) => {
                    // Shift+Tab to go back
                    state.prev_step();
                }
                KeyCode::Tab => {
                    // Tab to proceed to next step
                    if state.can_proceed() {
                        state.next_step();
                    }
                }
                KeyCode::Enter => {
                    state.insert_content_char('\n');
                }
                KeyCode::Backspace => {
                    state.backspace_content();
                }
                KeyCode::Up => {
                    state.move_content_up();
                }
                KeyCode::Down => {
                    state.move_content_down();
                }
                KeyCode::Left => {
                    state.move_content_left();
                }
                KeyCode::Right => {
                    state.move_content_right();
                }
                KeyCode::Char(c) => {
                    state.insert_content_char(c);
                }
                _ => {}
            }
        }
        NudgeFormStep::Permissions => {
            match state.permission_mode {
                PermissionMode::Browse => {
                    use crate::ui::nudge::ToolMode;

                    // Handle selection mode separately
                    if state.selecting_configured {
                        match key.code {
                            KeyCode::Esc => {
                                // Exit selection mode
                                state.selecting_configured = false;
                            }
                            KeyCode::Up => {
                                state.configured_tool_up();
                            }
                            KeyCode::Down => {
                                state.configured_tool_down();
                            }
                            KeyCode::Char('x') | KeyCode::Delete | KeyCode::Enter => {
                                // Remove selected tool
                                state.remove_selected_configured_tool();
                                // If no more tools, exit selection mode
                                if state.get_configured_tools().is_empty() {
                                    state.selecting_configured = false;
                                }
                            }
                            _ => {}
                        }
                    } else {
                        // Normal browse mode
                        match key.code {
                            KeyCode::Esc => {
                                app.modal_state = ModalState::None;
                                return;
                            }
                            KeyCode::Enter => {
                                state.next_step();
                            }
                            KeyCode::Backspace => {
                                state.prev_step();
                            }
                            // Mode switching keys (XOR between Additive and Exclusive)
                            KeyCode::Char('1') => {
                                state.permissions.set_mode(ToolMode::Additive);
                                state.selecting_configured = false;
                                state.configured_tool_index = 0;
                            }
                            KeyCode::Char('2') => {
                                state.permissions.set_mode(ToolMode::Exclusive);
                                state.selecting_configured = false;
                                state.configured_tool_index = 0;
                            }
                            // Additive mode: 'a' for allow, 'd' for deny
                            KeyCode::Char('a') if state.permissions.is_additive_mode() => {
                                state.permission_mode = PermissionMode::AddAllow;
                                state.tool_filter.clear();
                                state.tool_index = 0;
                                state.tool_scroll = 0;
                            }
                            KeyCode::Char('d') if state.permissions.is_additive_mode() => {
                                state.permission_mode = PermissionMode::AddDeny;
                                state.tool_filter.clear();
                                state.tool_index = 0;
                                state.tool_scroll = 0;
                            }
                            // Exclusive mode: 'o' for only-tool
                            KeyCode::Char('o') if state.permissions.is_exclusive_mode() => {
                                state.permission_mode = PermissionMode::AddOnly;
                                state.tool_filter.clear();
                                state.tool_index = 0;
                                state.tool_scroll = 0;
                            }
                            // Enter selection mode for removal (only if there are configured tools)
                            KeyCode::Char('x') => {
                                if !state.get_configured_tools().is_empty() {
                                    state.selecting_configured = true;
                                    state.configured_tool_index = 0;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                PermissionMode::AddAllow | PermissionMode::AddDeny | PermissionMode::AddOnly => {
                    use crate::ui::nudge::get_available_tools_from_statuses;

                    match key.code {
                        KeyCode::Esc => {
                            state.permission_mode = PermissionMode::Browse;
                            state.tool_filter.clear();
                            state.tool_index = 0;
                            state.tool_scroll = 0;
                        }
                        KeyCode::Enter => {
                            // Add selected tool based on current mode
                            let tools = {
                                let data_store = app.data_store.borrow();
                                get_available_tools_from_statuses(&data_store.project_statuses)
                            };
                            let filtered = state.filter_tools(&tools);
                            if let Some(tool) = filtered.get(state.tool_index) {
                                match state.permission_mode {
                                    PermissionMode::AddAllow => {
                                        state.permissions.add_allow_tool((*tool).to_string());
                                    }
                                    PermissionMode::AddDeny => {
                                        state.permissions.add_deny_tool((*tool).to_string());
                                    }
                                    PermissionMode::AddOnly => {
                                        state.permissions.add_only_tool((*tool).to_string());
                                    }
                                    PermissionMode::Browse => {} // Shouldn't happen
                                }
                            }
                            state.permission_mode = PermissionMode::Browse;
                            state.tool_filter.clear();
                            state.tool_index = 0;
                            state.tool_scroll = 0;
                        }
                        KeyCode::Up => {
                            if state.tool_index > 0 {
                                state.tool_index -= 1;
                            }
                        }
                        KeyCode::Down => {
                            let tools = {
                                let data_store = app.data_store.borrow();
                                get_available_tools_from_statuses(&data_store.project_statuses)
                            };
                            let filtered_count = state.filter_tools(&tools).len();
                            if filtered_count > 0 && state.tool_index + 1 < filtered_count {
                                state.tool_index += 1;
                            }
                        }
                        KeyCode::Char(c) => {
                            state.tool_filter.push(c);
                            // Reset index when filter changes and clamp to new bounds
                            state.tool_index = 0;
                            state.tool_scroll = 0;
                        }
                        KeyCode::Backspace => {
                            state.tool_filter.pop();
                            // Reset index when filter changes
                            state.tool_index = 0;
                            state.tool_scroll = 0;
                        }
                        _ => {}
                    }
                }
            }
        }
        NudgeFormStep::Review => {
            match key.code {
                KeyCode::Esc => {
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Backspace => {
                    state.prev_step();
                }
                KeyCode::Enter => {
                    // Check for validation errors first
                    let errors = state.get_submission_errors();
                    if !errors.is_empty() {
                        // Show first error to user
                        app.set_warning_status(&format!("Cannot submit: {}", errors[0]));
                        app.modal_state = ModalState::NudgeCreate(state);
                        return;
                    }

                    if state.can_submit() {
                        // Submit the nudge (always creates a new one - Nostr events are immutable)
                        if let Some(ref core_handle) = app.core_handle {
                            // Mode-aware tool permissions:
                            // - Exclusive mode: only send only_tools, ignore allow/deny
                            // - Additive mode: send allow/deny (with conflict resolution), ignore only_tools
                            let (sanitized_allow_tools, sanitized_deny_tools, only_tools) = if state.permissions.is_exclusive_mode() {
                                // Exclusive mode: only send only_tools
                                (Vec::new(), Vec::new(), state.permissions.only_tools.clone())
                            } else {
                                // Additive mode: sanitize allow list - "Deny wins" conflict resolution
                                let deny_set: std::collections::HashSet<_> = state.permissions.deny_tools.iter().collect();
                                let sanitized_allow: Vec<String> = state.permissions.allow_tools
                                    .iter()
                                    .filter(|tool| !deny_set.contains(tool))
                                    .cloned()
                                    .collect();
                                (sanitized_allow, state.permissions.deny_tools.clone(), Vec::new())
                            };

                            let result = core_handle.send(NostrCommand::CreateNudge {
                                title: state.title.clone(),
                                description: state.description.clone(),
                                content: state.content.clone(),
                                hashtags: state.hashtags.clone(),
                                allow_tools: sanitized_allow_tools,
                                deny_tools: sanitized_deny_tools,
                                only_tools,
                            });

                            match result {
                                Ok(_) => {
                                    app.set_warning_status(&format!("Nudge '{}' created", state.title));
                                }
                                Err(e) => {
                                    app.set_warning_status(&format!("Failed to create nudge: {}", e));
                                }
                            }
                        } else {
                            app.set_warning_status("Error: Not connected to backend");
                        }
                        app.modal_state = ModalState::None;
                        return;
                    }
                }
                KeyCode::Up => {
                    if state.review_scroll > 0 {
                        state.review_scroll -= 1;
                    }
                }
                KeyCode::Down => {
                    state.review_scroll += 1;
                }
                _ => {}
            }
        }
    }

    // Restore state
    app.modal_state = ModalState::NudgeCreate(state);
}

pub(super) fn handle_nudge_detail_key(app: &mut App, key: KeyEvent) {
    use ui::modal::{NudgeDeleteConfirmState, NudgeListState};
    use ui::nudge::NudgeFormState;

    let state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::NudgeDetail(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    match key.code {
        KeyCode::Esc => {
            // Go back to list
            app.modal_state = ModalState::NudgeList(NudgeListState::new());
        }
        KeyCode::Up | KeyCode::Char('k') => {
            let mut new_state = state;
            new_state.scroll_up();
            app.modal_state = ModalState::NudgeDetail(new_state);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            // Calculate max scroll to prevent scrolling past content
            // The visible height in detail view is approximately 20 lines
            // (80% modal height minus ~10 lines for header, metadata, borders, hints)
            let visible_height = 20;
            let nudge = app.data_store.borrow().content.nudges.get(&state.nudge_id).cloned();
            let max_scroll = nudge
                .map(|n| n.content.lines().count().saturating_sub(visible_height))
                .unwrap_or(0);
            let mut new_state = state;
            new_state.scroll_down(max_scroll);
            app.modal_state = ModalState::NudgeDetail(new_state);
        }
        KeyCode::Char('e') => {
            // Copy this nudge (pre-populate wizard with nudge data to create a new one)
            // Note: Nostr events are immutable - we can't edit, only copy and create new
            let nudge = app.data_store.borrow().content.nudges.get(&state.nudge_id).cloned();
            if let Some(nudge) = nudge {
                app.modal_state = ModalState::NudgeCreate(NudgeFormState::copy_from_nudge(&nudge));
            } else {
                app.modal_state = ModalState::NudgeDetail(state);
            }
        }
        KeyCode::Char('d') => {
            // Delete this nudge
            app.modal_state = ModalState::NudgeDeleteConfirm(NudgeDeleteConfirmState::new(state.nudge_id));
        }
        KeyCode::Char('c') => {
            // Copy nudge ID to clipboard
            if let Ok(mut clipboard) = arboard::Clipboard::new() {
                if clipboard.set_text(&state.nudge_id).is_ok() {
                    app.set_warning_status("Nudge ID copied to clipboard");
                }
            }
            app.modal_state = ModalState::NudgeDetail(state);
        }
        _ => {
            app.modal_state = ModalState::NudgeDetail(state);
        }
    }
}

pub(super) fn handle_nudge_delete_confirm_key(app: &mut App, key: KeyEvent) {
    use ui::modal::NudgeListState;

    let state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::NudgeDeleteConfirm(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    match key.code {
        KeyCode::Esc => {
            // Cancel - go back to list
            app.modal_state = ModalState::NudgeList(NudgeListState::new());
        }
        KeyCode::Up | KeyCode::Down => {
            let mut new_state = state;
            new_state.toggle();
            app.modal_state = ModalState::NudgeDeleteConfirm(new_state);
        }
        KeyCode::Enter => {
            if state.selected_index == 1 {
                // Delete confirmed
                if let Some(ref core_handle) = app.core_handle {
                    if let Err(e) = core_handle.send(NostrCommand::DeleteNudge {
                        nudge_id: state.nudge_id.clone(),
                    }) {
                        app.set_warning_status(&format!("Failed to delete nudge: {}", e));
                    } else {
                        // Remove from selection if this nudge was selected (prevents stale references)
                        app.remove_selected_nudge(&state.nudge_id);
                        app.set_warning_status("Nudge deleted");
                    }
                }
            }
            // Go back to list
            app.modal_state = ModalState::NudgeList(NudgeListState::new());
        }
        KeyCode::Char('d') => {
            // Quick delete
            if let Some(ref core_handle) = app.core_handle {
                if let Err(e) = core_handle.send(NostrCommand::DeleteNudge {
                    nudge_id: state.nudge_id.clone(),
                }) {
                    app.set_warning_status(&format!("Failed to delete nudge: {}", e));
                } else {
                    // Remove from selection if this nudge was selected (prevents stale references)
                    app.remove_selected_nudge(&state.nudge_id);
                    app.set_warning_status("Nudge deleted");
                }
            }
            app.modal_state = ModalState::NudgeList(NudgeListState::new());
        }
        _ => {
            app.modal_state = ModalState::NudgeDeleteConfirm(state);
        }
    }
}
