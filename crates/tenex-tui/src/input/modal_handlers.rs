//! Modal-specific keyboard event handlers.
//!
//! Each modal type has its own handler function, keeping the logic focused and testable.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::nostr::NostrCommand;
use crate::ui;
use crate::ui::selector::{handle_selector_key, SelectorAction};
use crate::ui::{App, InputMode, ModalState, View};

/// Routes input to the appropriate modal handler.
/// Returns `true` if the input was handled by a modal, `false` otherwise.
pub(super) fn handle_modal_input(app: &mut App, key: KeyEvent) -> Result<bool> {
    // Handle attachment modal when open
    if app.is_attachment_modal_open() {
        handle_attachment_modal_key(app, key);
        return Ok(true);
    }

    // Handle ask modal when open
    if matches!(app.modal_state, ModalState::AskModal(_)) {
        handle_ask_modal_key(app, key);
        return Ok(true);
    }

    // Handle command palette when open
    if matches!(app.modal_state, ModalState::CommandPalette(_)) {
        handle_command_palette_key(app, key);
        return Ok(true);
    }

    // Handle tab modal when open
    if app.showing_tab_modal() {
        handle_tab_modal_key(app, key);
        return Ok(true);
    }

    // Handle search modal when open
    if app.showing_search_modal {
        handle_search_modal_key(app, key);
        return Ok(true);
    }

    // Handle in-conversation search when active (per-tab isolated)
    if app.is_chat_search_active() {
        handle_chat_search_key(app, key);
        return Ok(true);
    }

    // Handle agent selector when open
    if matches!(app.modal_state, ModalState::AgentSelector { .. }) {
        handle_agent_selector_key(app, key)?;
        return Ok(true);
    }

    // Handle branch selector when open
    if matches!(app.modal_state, ModalState::BranchSelector { .. }) {
        handle_branch_selector_key(app, key)?;
        return Ok(true);
    }

    // Handle view raw event modal when open
    if matches!(app.modal_state, ModalState::ViewRawEvent { .. }) {
        handle_view_raw_event_modal_key(app, key);
        return Ok(true);
    }

    // Handle hotkey help modal when open
    if matches!(app.modal_state, ModalState::HotkeyHelp) {
        handle_hotkey_help_modal_key(app, key);
        return Ok(true);
    }

    // Handle nudge selector modal when open
    if matches!(app.modal_state, ModalState::NudgeSelector(_)) {
        handle_nudge_selector_key(app, key);
        return Ok(true);
    }

    // Handle create agent modal when open (global, works in any view)
    if matches!(app.modal_state, ModalState::CreateAgent(_)) {
        handle_create_agent_key(app, key);
        return Ok(true);
    }

    // Handle project actions modal when open
    if matches!(app.modal_state, ModalState::ProjectActions(_)) {
        handle_project_actions_modal_key(app, key);
        return Ok(true);
    }

    // Handle report viewer modal when open
    if matches!(app.modal_state, ModalState::ReportViewer(_)) {
        handle_report_viewer_modal_key(app, key);
        return Ok(true);
    }

    // Handle agent settings modal when open
    if matches!(app.modal_state, ModalState::AgentSettings(_)) {
        handle_agent_settings_modal_key(app, key);
        return Ok(true);
    }

    // Handle conversation actions modal when open
    if matches!(app.modal_state, ModalState::ConversationActions(_)) {
        handle_conversation_actions_modal_key(app, key);
        return Ok(true);
    }

    // Handle chat actions modal when open (via Ctrl+T command palette)
    if matches!(app.modal_state, ModalState::ChatActions(_)) {
        handle_chat_actions_modal_key(app, key);
        return Ok(true);
    }

    // Handle expanded editor modal when open
    if matches!(app.modal_state, ModalState::ExpandedEditor { .. }) {
        handle_expanded_editor_key(app, key);
        return Ok(true);
    }

    // Handle draft navigator modal when open
    if matches!(app.modal_state, ModalState::DraftNavigator(_)) {
        handle_draft_navigator_key(app, key);
        return Ok(true);
    }

    // Handle backend approval modal when open
    if matches!(app.modal_state, ModalState::BackendApproval(_)) {
        handle_backend_approval_modal_key(app, key);
        return Ok(true);
    }

    Ok(false)
}

// =============================================================================
// ATTACHMENT MODAL
// =============================================================================

fn handle_attachment_modal_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);

    match code {
        KeyCode::Esc => app.cancel_attachment_modal(),
        KeyCode::Char('s') if has_ctrl => app.save_and_close_attachment_modal(),
        KeyCode::Char('d') if has_ctrl => app.delete_attachment_and_close_modal(),
        KeyCode::Enter => app.attachment_modal_editor_mut().insert_newline(),
        KeyCode::Char('a') if has_ctrl => app.attachment_modal_editor_mut().move_to_line_start(),
        KeyCode::Char('e') if has_ctrl => app.attachment_modal_editor_mut().move_to_line_end(),
        KeyCode::Char('k') if has_ctrl => app.attachment_modal_editor_mut().kill_to_line_end(),
        KeyCode::Left if has_alt => app.attachment_modal_editor_mut().move_word_left(),
        KeyCode::Right if has_alt => app.attachment_modal_editor_mut().move_word_right(),
        KeyCode::Left => app.attachment_modal_editor_mut().move_left(),
        KeyCode::Right => app.attachment_modal_editor_mut().move_right(),
        KeyCode::Backspace => app.attachment_modal_editor_mut().delete_char_before(),
        KeyCode::Delete => app.attachment_modal_editor_mut().delete_char_at(),
        KeyCode::Char(c) => app.attachment_modal_editor_mut().insert_char(c),
        _ => {}
    }
}

// =============================================================================
// ASK MODAL
// =============================================================================

fn handle_ask_modal_key(app: &mut App, key: KeyEvent) {
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
            KeyCode::Up | KeyCode::Char('k') => input_state.prev_option(),
            KeyCode::Down | KeyCode::Char('j') => input_state.next_option(),
            KeyCode::Right => {
                input_state.skip_question();
                if input_state.is_complete() {
                    submit_ask_response(app);
                }
            }
            KeyCode::Left => input_state.prev_question(),
            KeyCode::Char(' ') if input_state.is_multi_select() => {
                input_state.toggle_multi_select();
            }
            KeyCode::Enter => {
                input_state.select_current_option();
                if input_state.is_complete() {
                    submit_ask_response(app);
                }
            }
            KeyCode::Esc => app.close_ask_modal(),
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

    if let (Some(ref core_handle), Some(ref thread), Some(ref project)) =
        (&app.core_handle, &app.selected_thread, &app.selected_project)
    {
        let _ = core_handle.send(NostrCommand::PublishMessage {
            thread_id: thread.id.clone(),
            project_a_tag: project.a_tag(),
            content: response_text,
            agent_pubkey: None,
            reply_to: Some(message_id),
            branch: None,
            nudge_ids: vec![],
            ask_author_pubkey: Some(ask_author_pubkey),
        });
    }

    app.input_mode = InputMode::Editing;
}

// =============================================================================
// COMMAND PALETTE
// =============================================================================

fn handle_command_palette_key(app: &mut App, key: KeyEvent) {
    // Get available commands before matching on state (to avoid mutable borrow conflict)
    let commands = super::commands::available_commands(app);
    let cmd_count = commands.len();

    if let ModalState::CommandPalette(ref mut state) = app.modal_state {
        match key.code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
            }
            KeyCode::Left => {
                app.modal_state = ModalState::None;
                app.prev_tab();
            }
            KeyCode::Right => {
                app.modal_state = ModalState::None;
                app.next_tab();
            }
            KeyCode::Up => {
                state.move_up();
            }
            KeyCode::Down => {
                state.move_down(cmd_count);
            }
            KeyCode::Enter => {
                if let Some(cmd) = commands.get(state.selected_index) {
                    let cmd_key = cmd.key;
                    app.modal_state = ModalState::None;
                    super::commands::execute_command(app, cmd_key);
                }
            }
            KeyCode::Char(c) => {
                // Execute command directly if it matches a hotkey
                app.modal_state = ModalState::None;
                super::commands::execute_command(app, c);
            }
            _ => {}
        }
    }
}

// =============================================================================
// TAB MODAL
// =============================================================================

fn handle_tab_modal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.close_tab_modal(),
        KeyCode::Up => {
            if app.tab_modal_index() > 0 {
                app.set_tab_modal_index(app.tab_modal_index() - 1);
            }
        }
        KeyCode::Down => {
            if app.tab_modal_index() + 1 < app.open_tabs().len() {
                app.set_tab_modal_index(app.tab_modal_index() + 1);
            }
        }
        KeyCode::Enter => {
            let idx = app.tab_modal_index();
            app.close_tab_modal();
            if idx < app.open_tabs().len() {
                app.switch_to_tab(idx);
                app.view = View::Chat;
            }
        }
        KeyCode::Char('x') => {
            if !app.open_tabs().is_empty() {
                let idx = app.tab_modal_index();
                app.close_tab_at(idx);
                if app.open_tabs().is_empty() {
                    app.close_tab_modal();
                }
            }
        }
        KeyCode::Char('1') => {
            app.close_tab_modal();
            app.save_chat_draft();
            app.view = View::Home;
        }
        KeyCode::Char(c) if c >= '2' && c <= '9' => {
            let tab_index = (c as usize) - ('2' as usize);
            app.close_tab_modal();
            if tab_index < app.open_tabs().len() {
                app.switch_to_tab(tab_index);
                app.view = View::Chat;
            }
        }
        _ => {}
    }
}

// =============================================================================
// SEARCH MODAL
// =============================================================================

fn handle_search_modal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.showing_search_modal = false;
            app.search_filter.clear();
            app.search_index = 0;
        }
        KeyCode::Up => {
            if app.search_index > 0 {
                app.search_index -= 1;
            }
        }
        KeyCode::Down => {
            let count = app.search_results().len();
            if app.search_index + 1 < count {
                app.search_index += 1;
            }
        }
        KeyCode::Enter => {
            let results = app.search_results();
            if let Some(result) = results.get(app.search_index).cloned() {
                app.showing_search_modal = false;
                app.search_filter.clear();
                app.search_index = 0;
                app.open_thread_from_home(&result.thread, &result.project_a_tag);
            }
        }
        KeyCode::Char(c) => {
            app.search_filter.push(c);
            app.search_index = 0;
        }
        KeyCode::Backspace => {
            app.search_filter.pop();
            app.search_index = 0;
        }
        _ => {}
    }
}

// =============================================================================
// CHAT SEARCH (in-conversation search)
// =============================================================================

fn handle_chat_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.exit_chat_search(),
        KeyCode::Enter | KeyCode::Down => app.chat_search_next(),
        KeyCode::Up => app.chat_search_prev(),
        KeyCode::Char(c) => {
            // Per-tab isolated: update query on active tab
            if let Some(tab) = app.tabs.active_tab_mut() {
                tab.chat_search.query.push(c);
            }
            app.update_chat_search();
        }
        KeyCode::Backspace => {
            // Per-tab isolated: update query on active tab
            if let Some(tab) = app.tabs.active_tab_mut() {
                tab.chat_search.query.pop();
            }
            app.update_chat_search();
        }
        _ => {}
    }
}

// =============================================================================
// AGENT SELECTOR
// =============================================================================

fn handle_agent_selector_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let agents = app.filtered_agents();
    let item_count = agents.len();

    if let ModalState::AgentSelector { ref mut selector } = app.modal_state {
        match handle_selector_key(selector, key, item_count, |idx| agents.get(idx).cloned()) {
            SelectorAction::Selected(agent) => {
                // Set agent as recipient - never insert text into input
                app.selected_agent = Some(agent);
                app.user_explicitly_selected_agent = true;
                app.modal_state = ModalState::None;
            }
            SelectorAction::Cancelled => {
                app.modal_state = ModalState::None;
            }
            SelectorAction::Continue => {}
        }
    }
    Ok(())
}

// =============================================================================
// BRANCH SELECTOR
// =============================================================================

fn handle_branch_selector_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let branches = app.filtered_branches();
    let item_count = branches.len();

    if let ModalState::BranchSelector { ref mut selector } = app.modal_state {
        match handle_selector_key(selector, key, item_count, |idx| branches.get(idx).cloned()) {
            SelectorAction::Selected(branch) => {
                app.selected_branch = Some(branch);
                app.modal_state = ModalState::None;
            }
            SelectorAction::Cancelled => {
                app.modal_state = ModalState::None;
            }
            SelectorAction::Continue => {}
        }
    }
    Ok(())
}

// =============================================================================
// VIEW RAW EVENT MODAL
// =============================================================================

fn handle_view_raw_event_modal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
        }
        KeyCode::Char('y') => {
            if let ModalState::ViewRawEvent { ref json, .. } = app.modal_state {
                use arboard::Clipboard;
                match Clipboard::new() {
                    Ok(mut clipboard) => {
                        if clipboard.set_text(json).is_ok() {
                            app.set_status("Raw event copied to clipboard");
                        } else {
                            app.set_status("Failed to copy to clipboard");
                        }
                    }
                    Err(_) => {
                        app.set_status("Failed to access clipboard");
                    }
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let ModalState::ViewRawEvent {
                scroll_offset: ref mut offset,
                ..
            } = app.modal_state
            {
                *offset = offset.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let ModalState::ViewRawEvent {
                scroll_offset: ref mut offset,
                ..
            } = app.modal_state
            {
                *offset += 1;
            }
        }
        KeyCode::PageUp => {
            if let ModalState::ViewRawEvent {
                scroll_offset: ref mut offset,
                ..
            } = app.modal_state
            {
                *offset = offset.saturating_sub(20);
            }
        }
        KeyCode::PageDown => {
            if let ModalState::ViewRawEvent {
                scroll_offset: ref mut offset,
                ..
            } = app.modal_state
            {
                *offset += 20;
            }
        }
        _ => {}
    }
}

// =============================================================================
// HOTKEY HELP MODAL
// =============================================================================

fn handle_hotkey_help_modal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::Char('q') => {
            app.modal_state = ModalState::None;
        }
        _ => {
            app.modal_state = ModalState::None;
        }
    }
}

// =============================================================================
// NUDGE SELECTOR
// =============================================================================

fn handle_nudge_selector_key(app: &mut App, key: KeyEvent) {
    let nudges = app.filtered_nudges();
    let item_count = nudges.len();

    if let ModalState::NudgeSelector(ref mut state) = app.modal_state {
        match key.code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
            }
            KeyCode::Enter => {
                // Apply to current tab (per-tab isolated)
                let selected_ids = state.selected_nudge_ids.clone();
                if let Some(tab) = app.tabs.active_tab_mut() {
                    tab.selected_nudge_ids = selected_ids;
                }
                app.modal_state = ModalState::None;
            }
            KeyCode::Up => {
                if state.selector.index > 0 {
                    state.selector.index -= 1;
                }
            }
            KeyCode::Down => {
                if item_count > 0 && state.selector.index < item_count - 1 {
                    state.selector.index += 1;
                }
            }
            KeyCode::Char(' ') => {
                if let Some(nudge) = nudges.get(state.selector.index) {
                    let nudge_id = nudge.id.clone();
                    if let Some(pos) = state.selected_nudge_ids.iter().position(|id| id == &nudge_id)
                    {
                        state.selected_nudge_ids.remove(pos);
                    } else {
                        state.selected_nudge_ids.push(nudge_id);
                    }
                }
            }
            KeyCode::Char(c) => {
                state.selector.filter.push(c);
                state.selector.index = 0;
            }
            KeyCode::Backspace => {
                state.selector.filter.pop();
                state.selector.index = 0;
            }
            _ => {}
        }
    }
}

// =============================================================================
// CREATE AGENT MODAL
// =============================================================================

fn handle_create_agent_key(app: &mut App, key: KeyEvent) {
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

                if let Some(prev_line_end) = state.instructions[..current_line_start.saturating_sub(1)]
                    .rfind('\n')
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
                        app.set_status(&format!("Failed to create agent: {}", e));
                    } else {
                        app.set_status(&format!("Agent '{}' created", state.name));
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

// =============================================================================
// PROJECT ACTIONS MODAL
// =============================================================================

fn handle_project_actions_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::ProjectAction;

    let state = match &app.modal_state {
        ModalState::ProjectActions(s) => s.clone(),
        _ => return,
    };

    let actions = state.available_actions();
    let action_count = actions.len();

    match key.code {
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.selected_index > 0 {
                if let ModalState::ProjectActions(ref mut s) = app.modal_state {
                    s.selected_index -= 1;
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.selected_index + 1 < action_count {
                if let ModalState::ProjectActions(ref mut s) = app.modal_state {
                    s.selected_index += 1;
                }
            }
        }
        KeyCode::Enter => {
            if let Some(action) = state.selected_action() {
                execute_project_action(app, &state, action);
            }
        }
        KeyCode::Char('n') if state.is_online => {
            execute_project_action(app, &state, ProjectAction::NewConversation);
        }
        KeyCode::Char('b') if !state.is_online => {
            execute_project_action(app, &state, ProjectAction::Boot);
        }
        KeyCode::Char('s') => {
            execute_project_action(app, &state, ProjectAction::Settings);
        }
        KeyCode::Char('a') => {
            execute_project_action(app, &state, ProjectAction::ToggleArchive);
        }
        _ => {}
    }
}

fn execute_project_action(
    app: &mut App,
    state: &ui::modal::ProjectActionsState,
    action: ui::modal::ProjectAction,
) {
    use crate::ui::notifications::Notification;
    use ui::modal::ProjectAction;

    match action {
        ProjectAction::Boot => {
            if let Some(core_handle) = app.core_handle.clone() {
                if let Err(e) = core_handle.send(NostrCommand::BootProject {
                    project_a_tag: state.project_a_tag.clone(),
                    project_pubkey: Some(state.project_pubkey.clone()),
                }) {
                    app.set_status(&format!("Failed to boot: {}", e));
                } else {
                    app.set_status(&format!("Boot request sent for {}", state.project_name));
                }
            }
            app.modal_state = ModalState::None;
        }
        ProjectAction::Settings => {
            let agent_ids = {
                let store = app.data_store.borrow();
                store
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == state.project_a_tag)
                    .map(|p| p.agent_ids.clone())
                    .unwrap_or_default()
            };
            app.modal_state = ModalState::ProjectSettings(ui::modal::ProjectSettingsState::new(
                state.project_a_tag.clone(),
                state.project_name.clone(),
                agent_ids,
            ));
        }
        ProjectAction::NewConversation => {
            let project = {
                let store = app.data_store.borrow();
                store
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == state.project_a_tag)
                    .cloned()
            };

            if let Some(project) = project {
                let a_tag = project.a_tag();
                let project_name = state.project_name.clone();
                app.selected_project = Some(project);

                if let Some(status) = app.data_store.borrow().get_project_status(&a_tag) {
                    if let Some(pm) = status.pm_agent() {
                        app.selected_agent = Some(pm.clone());
                    }
                    if app.selected_branch.is_none() {
                        app.selected_branch = status.default_branch().map(String::from);
                    }
                }

                app.modal_state = ModalState::None;
                let tab_idx = app.open_draft_tab(&a_tag, &project_name);
                app.switch_to_tab(tab_idx);
                app.chat_editor.clear();
            } else {
                app.modal_state = ModalState::None;
                app.set_status("Project not found");
            }
        }
        ProjectAction::ToggleArchive => {
            let is_now_archived = app.toggle_project_archived(&state.project_a_tag);
            let status = if is_now_archived {
                format!("Archived: {}", state.project_name)
            } else {
                format!("Unarchived: {}", state.project_name)
            };
            app.notify(Notification::info(&status));
            app.modal_state = ModalState::None;
        }
    }
}

// =============================================================================
// REPORT VIEWER MODAL
// =============================================================================

fn handle_report_viewer_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::{ReportCopyOption, ReportViewMode, ReportViewerFocus};

    if let ModalState::ReportViewer(ref mut state) = app.modal_state {
        match key.code {
            KeyCode::Esc => {
                if state.show_copy_menu {
                    state.show_copy_menu = false;
                } else {
                    app.modal_state = ModalState::None;
                }
            }
            KeyCode::Tab => {
                state.view_mode = match state.view_mode {
                    ReportViewMode::Current => ReportViewMode::Changes,
                    ReportViewMode::Changes => ReportViewMode::Current,
                };
            }
            KeyCode::Char('t') => {
                state.show_threads = !state.show_threads;
            }
            KeyCode::Char('h') | KeyCode::Left => {
                state.focus = ReportViewerFocus::Content;
            }
            KeyCode::Char('l') | KeyCode::Right => {
                if state.show_threads {
                    state.focus = ReportViewerFocus::Threads;
                }
            }
            KeyCode::Char('y') => {
                state.show_copy_menu = !state.show_copy_menu;
            }
            KeyCode::Char('[') => {
                let slug = state.report.slug.clone();
                let versions = app
                    .data_store
                    .borrow()
                    .get_report_versions(&slug)
                    .into_iter()
                    .cloned()
                    .collect::<Vec<_>>();
                if state.version_index + 1 < versions.len() {
                    state.version_index += 1;
                    if let Some(v) = versions.get(state.version_index) {
                        state.report = v.clone();
                        state.content_scroll = 0;
                    }
                }
            }
            KeyCode::Char(']') => {
                if state.version_index > 0 {
                    state.version_index -= 1;
                    let slug = state.report.slug.clone();
                    let versions = app
                        .data_store
                        .borrow()
                        .get_report_versions(&slug)
                        .into_iter()
                        .cloned()
                        .collect::<Vec<_>>();
                    if let Some(v) = versions.get(state.version_index) {
                        state.report = v.clone();
                        state.content_scroll = 0;
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => match state.focus {
                ReportViewerFocus::Content => {
                    state.content_scroll = state.content_scroll.saturating_sub(1);
                }
                ReportViewerFocus::Threads => {
                    if state.selected_thread_index > 0 {
                        state.selected_thread_index -= 1;
                    }
                }
            },
            KeyCode::Down | KeyCode::Char('j') => match state.focus {
                ReportViewerFocus::Content => {
                    state.content_scroll += 1;
                }
                ReportViewerFocus::Threads => {
                    state.selected_thread_index += 1;
                }
            },
            KeyCode::Enter => {
                if state.show_copy_menu {
                    use crate::store::get_raw_event_json;
                    use nostr_sdk::prelude::{EventId, ToBech32};

                    let option = ReportCopyOption::ALL[state.copy_menu_index];
                    let text = match option {
                        ReportCopyOption::Bech32Id => EventId::from_hex(&state.report.id)
                            .ok()
                            .and_then(|id| id.to_bech32().ok())
                            .unwrap_or_else(|| state.report.id.clone()),
                        ReportCopyOption::RawEvent => {
                            get_raw_event_json(&app.db.ndb, &state.report.id)
                                .map(|json| {
                                    serde_json::from_str::<serde_json::Value>(&json)
                                        .ok()
                                        .and_then(|v| serde_json::to_string_pretty(&v).ok())
                                        .unwrap_or(json)
                                })
                                .unwrap_or_else(|| "Failed to get raw event".to_string())
                        }
                        ReportCopyOption::Markdown => state.report.content.clone(),
                    };
                    state.show_copy_menu = false;
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(&text);
                    }
                } else if state.focus == ReportViewerFocus::Threads {
                    let a_tag = state.report.a_tag();
                    let project_a_tag = state.report.project_a_tag.clone();
                    let threads = app
                        .data_store
                        .borrow()
                        .get_document_threads(&a_tag)
                        .to_vec();
                    if let Some(thread) = threads.get(state.selected_thread_index) {
                        app.open_thread_from_home(thread, &project_a_tag);
                        app.modal_state = ModalState::None;
                    }
                }
            }
            KeyCode::Char('n') => {
                if state.focus == ReportViewerFocus::Threads || state.show_threads {
                    app.set_status("Thread creation not yet implemented");
                }
            }
            _ => {}
        }
    }
}

// =============================================================================
// AGENT SETTINGS MODAL
// =============================================================================

fn handle_agent_settings_modal_key(app: &mut App, key: KeyEvent) {
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
                        app.set_status(&format!("Failed to update agent config: {}", e));
                    } else {
                        app.set_status("Agent config update sent");
                    }
                }
                app.modal_state = ModalState::None;
            }
            _ => {}
        }
    }
}

// =============================================================================
// CONVERSATION ACTIONS MODAL
// =============================================================================

fn handle_conversation_actions_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::ConversationAction;

    let state = match &app.modal_state {
        ModalState::ConversationActions(s) => s.clone(),
        _ => return,
    };

    let action_count = ConversationAction::ALL.len();

    match key.code {
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.selected_index > 0 {
                if let ModalState::ConversationActions(ref mut s) = app.modal_state {
                    s.selected_index -= 1;
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.selected_index + 1 < action_count {
                if let ModalState::ConversationActions(ref mut s) = app.modal_state {
                    s.selected_index += 1;
                }
            }
        }
        KeyCode::Enter => {
            execute_conversation_action(app, &state, state.selected_action());
        }
        KeyCode::Char('o') => {
            execute_conversation_action(app, &state, ConversationAction::Open);
        }
        KeyCode::Char('e') => {
            execute_conversation_action(app, &state, ConversationAction::ExportJsonl);
        }
        KeyCode::Char('a') => {
            execute_conversation_action(app, &state, ConversationAction::ToggleArchive);
        }
        _ => {}
    }
}

fn execute_conversation_action(
    app: &mut App,
    state: &ui::modal::ConversationActionsState,
    action: ui::modal::ConversationAction,
) {
    use ui::modal::ConversationAction;

    match action {
        ConversationAction::Open => {
            let thread = app
                .data_store
                .borrow()
                .get_threads(&state.project_a_tag)
                .iter()
                .find(|t| t.id == state.thread_id)
                .cloned();
            if let Some(thread) = thread {
                let a_tag = state.project_a_tag.clone();
                app.modal_state = ModalState::None;
                app.open_thread_from_home(&thread, &a_tag);
            } else {
                app.modal_state = ModalState::None;
            }
        }
        ConversationAction::ExportJsonl => {
            export_thread_as_jsonl(app, &state.thread_id);
            app.modal_state = ModalState::None;
        }
        ConversationAction::ToggleArchive => {
            let is_now_archived = app.toggle_thread_archived(&state.thread_id);
            let status = if is_now_archived {
                format!("Archived: {}", state.thread_title)
            } else {
                format!("Unarchived: {}", state.thread_title)
            };
            app.set_status(&status);
            app.modal_state = ModalState::None;
        }
    }
}

// =============================================================================
// CHAT ACTIONS MODAL
// =============================================================================

fn handle_chat_actions_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::ChatAction;

    let state = match &app.modal_state {
        ModalState::ChatActions(s) => s.clone(),
        _ => return,
    };

    let actions = state.available_actions();
    let action_count = actions.len();

    match key.code {
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if state.selected_index > 0 {
                if let ModalState::ChatActions(ref mut s) = app.modal_state {
                    s.selected_index -= 1;
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if state.selected_index + 1 < action_count {
                if let ModalState::ChatActions(ref mut s) = app.modal_state {
                    s.selected_index += 1;
                }
            }
        }
        KeyCode::Enter => {
            if let Some(action) = state.selected_action() {
                execute_chat_action(app, &state, action);
            }
        }
        KeyCode::Char('n') => {
            execute_chat_action(app, &state, ChatAction::NewConversation);
        }
        KeyCode::Char('p') => {
            if state.has_parent() {
                execute_chat_action(app, &state, ChatAction::GoToParent);
            }
        }
        KeyCode::Char('e') => {
            execute_chat_action(app, &state, ChatAction::ExportJsonl);
        }
        _ => {}
    }
}

fn execute_chat_action(
    app: &mut App,
    state: &ui::modal::ChatActionsState,
    action: ui::modal::ChatAction,
) {
    use ui::modal::ChatAction;

    match action {
        ChatAction::NewConversation => {
            let project_a_tag = state.project_a_tag.clone();
            let project_name = app
                .data_store
                .borrow()
                .get_projects()
                .iter()
                .find(|p| p.a_tag() == project_a_tag)
                .map(|p| p.name.clone())
                .unwrap_or_else(|| "New".to_string());

            app.modal_state = ModalState::None;
            app.save_chat_draft();
            let tab_idx = app.open_draft_tab(&project_a_tag, &project_name);
            app.switch_to_tab(tab_idx);
            app.chat_editor.clear();
            app.set_status("New conversation (same project, agent, and branch)");
        }
        ChatAction::GoToParent => {
            if let Some(ref parent_id) = state.parent_conversation_id {
                let parent_thread = app
                    .data_store
                    .borrow()
                    .get_threads(&state.project_a_tag)
                    .iter()
                    .find(|t| t.id == *parent_id)
                    .cloned();

                if let Some(thread) = parent_thread {
                    let a_tag = state.project_a_tag.clone();
                    app.modal_state = ModalState::None;
                    app.open_thread_from_home(&thread, &a_tag);
                    app.set_status(&format!("Navigated to parent: {}", thread.title));
                } else {
                    app.set_status("Parent conversation not found");
                    app.modal_state = ModalState::None;
                }
            }
        }
        ChatAction::ExportJsonl => {
            export_thread_as_jsonl(app, &state.thread_id);
            app.modal_state = ModalState::None;
        }
    }
}

// =============================================================================
// EXPANDED EDITOR MODAL
// =============================================================================

fn handle_expanded_editor_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    match code {
        KeyCode::Esc => app.cancel_expanded_editor(),
        KeyCode::Char('s') if has_ctrl => app.save_and_close_expanded_editor(),
        KeyCode::Enter => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.insert_newline();
            }
        }
        KeyCode::Char('z') if has_ctrl && has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.redo();
            }
        }
        KeyCode::Char('z') if has_ctrl => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.undo();
            }
        }
        KeyCode::Char('a') if has_ctrl => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.select_all();
            }
        }
        KeyCode::Left if has_alt && has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_word_left_extend_selection();
            }
        }
        KeyCode::Right if has_alt && has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_word_right_extend_selection();
            }
        }
        KeyCode::Left if has_alt => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_word_left();
            }
        }
        KeyCode::Right if has_alt => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_word_right();
            }
        }
        KeyCode::Left if has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_left_extend_selection();
            }
        }
        KeyCode::Right if has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_right_extend_selection();
            }
        }
        KeyCode::Left => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_left();
            }
        }
        KeyCode::Right => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_right();
            }
        }
        KeyCode::Up => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_up();
            }
        }
        KeyCode::Down => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.move_down();
            }
        }
        KeyCode::Backspace => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.delete_char_before();
            }
        }
        KeyCode::Delete => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.delete_char_at();
            }
        }
        KeyCode::Char('c') if has_ctrl => {
            if let Some(editor) = app.expanded_editor_mut() {
                if let Some(selected) = editor.selected_text() {
                    use arboard::Clipboard;
                    if let Ok(mut clipboard) = Clipboard::new() {
                        let _ = clipboard.set_text(selected);
                    }
                }
            }
        }
        KeyCode::Char('x') if has_ctrl => {
            if let Some(editor) = app.expanded_editor_mut() {
                if let Some(selected) = editor.selected_text() {
                    use arboard::Clipboard;
                    if let Ok(mut clipboard) = Clipboard::new() {
                        let _ = clipboard.set_text(selected);
                    }
                    editor.delete_selection();
                }
            }
        }
        KeyCode::Char(c) => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.insert_char(c);
            }
        }
        _ => {}
    }
}

// =============================================================================
// HELPER FUNCTIONS
// =============================================================================

/// Export a thread as JSONL (one raw event per line)
pub(super) fn export_thread_as_jsonl(app: &mut App, thread_id: &str) {
    use crate::store::get_raw_event_json;

    let messages = app.data_store.borrow().get_messages(thread_id).to_vec();

    if messages.is_empty() {
        app.set_status("No messages to export");
        return;
    }

    let mut lines = Vec::new();

    // First, add the thread root event if available
    if let Some(json) = get_raw_event_json(&app.db.ndb, thread_id) {
        lines.push(json);
    }

    // Then add all message events
    for msg in &messages {
        if msg.id != thread_id {
            if let Some(json) = get_raw_event_json(&app.db.ndb, &msg.id) {
                lines.push(json);
            }
        }
    }

    let content = lines.join("\n");

    use arboard::Clipboard;
    match Clipboard::new() {
        Ok(mut clipboard) => {
            if clipboard.set_text(&content).is_ok() {
                app.set_status(&format!(
                    "Exported {} events to clipboard as JSONL",
                    lines.len()
                ));
            } else {
                app.set_status("Failed to copy to clipboard");
            }
        }
        Err(_) => {
            app.set_status("Failed to access clipboard");
        }
    }
}

// =============================================================================
// DRAFT NAVIGATOR MODAL
// =============================================================================

fn handle_draft_navigator_key(app: &mut App, key: KeyEvent) {
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

// =============================================================================
// BACKEND APPROVAL MODAL
// =============================================================================

fn handle_backend_approval_modal_key(app: &mut App, key: KeyEvent) {
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
            app.set_status(&format!(
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
            app.set_status(&format!(
                "Blocked backend {}...",
                &state.backend_pubkey[..8.min(state.backend_pubkey.len())]
            ));
            app.modal_state = ModalState::None;
        }
    }
}
