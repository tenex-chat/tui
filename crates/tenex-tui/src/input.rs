use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::nostr;
use crate::nostr::NostrCommand;
use crate::ui;
use crate::ui::selector::{handle_selector_key, SelectorAction};
use crate::ui::views::chat::{group_messages, DisplayItem};
use crate::ui::views::home::get_hierarchical_threads;
use crate::ui::views::login::LoginStep;
use crate::ui::{App, HomeTab, InputMode, ModalState, View};

pub(crate) fn handle_key(
    app: &mut App,
    key: KeyEvent,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
) -> Result<()> {
    let code = key.code;

    // Handle attachment modal when open
    if app.is_attachment_modal_open() {
        handle_attachment_modal_key(app, key);
        return Ok(());
    }

    // Handle ask modal when open
    if matches!(app.modal_state, ModalState::AskModal(_)) {
        handle_ask_modal_key(app, key);
        return Ok(());
    }

    // Handle command palette when open
    if matches!(app.modal_state, ModalState::CommandPalette(_)) {
        handle_command_palette_key(app, key);
        return Ok(());
    }

    // Handle tab modal when open
    if app.showing_tab_modal {
        handle_tab_modal_key(app, key);
        return Ok(());
    }

    // Handle search modal when open
    if app.showing_search_modal {
        handle_search_modal_key(app, key);
        return Ok(());
    }

    // Handle agent selector when open (using ModalState)
    if matches!(app.modal_state, ModalState::AgentSelector { .. }) {
        // Get agents BEFORE mutably borrowing modal_state
        let agents = app.filtered_agents();
        let item_count = agents.len();

        // Handle 's' key to open agent settings
        if let KeyCode::Char('s') = code {
            if let ModalState::AgentSelector { ref selector } = app.modal_state {
                if let Some(agent) = agents.get(selector.index).cloned() {
                    // Get project a_tag for the settings event
                    if let Some(project) = &app.selected_project {
                        // Get all available tools and models from the project status
                        let (all_tools, all_models) = app.data_store.borrow()
                            .get_project_status(&project.a_tag())
                            .map(|status| {
                                let tools = status.tools().iter().map(|s| s.to_string()).collect();
                                let models = status.models().iter().map(|s| s.to_string()).collect();
                                (tools, models)
                            })
                            .unwrap_or_default();

                        let settings_state = crate::ui::modal::AgentSettingsState::new(
                            agent.name.clone(),
                            agent.pubkey.clone(),
                            project.a_tag(),
                            agent.model.clone(),
                            agent.tools.clone(),
                            all_models,
                            all_tools,
                        );
                        app.modal_state = ModalState::AgentSettings(settings_state);
                        return Ok(());
                    }
                }
            }
        }

        if let ModalState::AgentSelector { ref mut selector } = app.modal_state {
            match handle_selector_key(selector, key, item_count, |idx| agents.get(idx).cloned()) {
                SelectorAction::Selected(agent) => {
                    let agent_name = agent.name.clone();
                    app.selected_agent = Some(agent);
                    // Mark that user explicitly selected this agent
                    // This prevents auto-sync from overriding their choice
                    app.user_explicitly_selected_agent = true;
                    // Insert @agent_name into chat editor
                    let mention = format!("@{} ", agent_name);
                    for c in mention.chars() {
                        app.chat_editor.insert_char(c);
                    }
                    app.save_chat_draft();
                    app.modal_state = ModalState::None;
                }
                SelectorAction::Cancelled => {
                    app.modal_state = ModalState::None;
                }
                SelectorAction::Continue => {}
            }
        }
        return Ok(());
    }

    // Handle branch selector when open (using ModalState)
    if matches!(app.modal_state, ModalState::BranchSelector { .. }) {
        // Get branches BEFORE mutably borrowing modal_state
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
        return Ok(());
    }

    // Handle message actions modal when open
    if matches!(app.modal_state, ModalState::MessageActions { .. }) {
        handle_message_actions_modal_key(app, key);
        return Ok(());
    }

    // Handle view raw event modal when open
    if matches!(app.modal_state, ModalState::ViewRawEvent { .. }) {
        handle_view_raw_event_modal_key(app, key);
        return Ok(());
    }

    // Handle hotkey help modal when open
    if matches!(app.modal_state, ModalState::HotkeyHelp) {
        handle_hotkey_help_modal_key(app, key);
        return Ok(());
    }

    // Handle nudge selector modal when open
    if matches!(app.modal_state, ModalState::NudgeSelector(_)) {
        handle_nudge_selector_key(app, key);
        return Ok(());
    }

    // Handle create agent modal when open (global, works in any view)
    if matches!(app.modal_state, ModalState::CreateAgent(_)) {
        handle_create_agent_key(app, key);
        return Ok(());
    }

    // Handle project actions modal when open
    if matches!(app.modal_state, ModalState::ProjectActions(_)) {
        handle_project_actions_modal_key(app, key);
        return Ok(());
    }

    // Handle report viewer modal when open
    if matches!(app.modal_state, ModalState::ReportViewer(_)) {
        handle_report_viewer_modal_key(app, key);
        return Ok(());
    }

    // Handle agent settings modal when open
    if matches!(app.modal_state, ModalState::AgentSettings(_)) {
        handle_agent_settings_modal_key(app, key);
        return Ok(());
    }

    // Handle conversation actions modal when open
    if matches!(app.modal_state, ModalState::ConversationActions(_)) {
        handle_conversation_actions_modal_key(app, key);
        return Ok(());
    }

    // Handle chat actions modal when open (Ctrl+T /)
    if matches!(app.modal_state, ModalState::ChatActions(_)) {
        handle_chat_actions_modal_key(app, key);
        return Ok(());
    }

    // Handle expanded editor modal when open
    if matches!(app.modal_state, ModalState::ExpandedEditor { .. }) {
        handle_expanded_editor_key(app, key);
        return Ok(());
    }

    // Global tab navigation with Alt key (works in all views except Login)
    // These bindings work regardless of input mode
    if app.view != View::Login {
        let modifiers = key.modifiers;
        let has_alt = modifiers.contains(KeyModifiers::ALT);
        let has_shift = modifiers.contains(KeyModifiers::SHIFT);

        // macOS Option+Number produces special characters instead of Alt+Number
        // Handle these characters for tab switching
        match code {
            // Option+1 on macOS produces '¡' - go to dashboard
            KeyCode::Char('¡') => {
                app.save_chat_draft();
                app.view = View::Home;
                return Ok(());
            }
            // Option+2..9 on macOS produces special chars - switch to tab
            // ™=2, £=3, ¢=4, ∞=5, §=6, ¶=7, •=8, ª=9
            KeyCode::Char(c) if matches!(c, '™' | '£' | '¢' | '∞' | '§' | '¶' | '•' | 'ª') => {
                let tab_index = match c {
                    '™' => 0, // Option+2 -> tab 0
                    '£' => 1, // Option+3 -> tab 1
                    '¢' => 2, // Option+4 -> tab 2
                    '∞' => 3, // Option+5 -> tab 3
                    '§' => 4, // Option+6 -> tab 4
                    '¶' => 5, // Option+7 -> tab 5
                    '•' => 6, // Option+8 -> tab 6
                    'ª' => 7, // Option+9 -> tab 7
                    _ => return Ok(()),
                };
                if tab_index < app.open_tabs.len() {
                    app.switch_to_tab(tab_index);
                    app.view = View::Chat;
                }
                return Ok(());
            }
            _ => {}
        }

        if has_alt {
            match code {
                // Alt+1 = go to dashboard (home) - always first tab
                KeyCode::Char('1') => {
                    app.save_chat_draft();
                    app.view = View::Home;
                    return Ok(());
                }
                // Alt+2..9 = jump directly to tab N-1 (since 1 is Home)
                KeyCode::Char(c) if c >= '2' && c <= '9' => {
                    let tab_index = (c as usize) - ('2' as usize); // '2' -> 0, '3' -> 1, etc.
                    if tab_index < app.open_tabs.len() {
                        app.switch_to_tab(tab_index);
                        app.view = View::Chat;
                    }
                    return Ok(());
                }
                // Alt+Tab = cycle forward through recently viewed tabs
                KeyCode::Tab => {
                    if has_shift {
                        app.cycle_tab_history_backward();
                    } else {
                        app.cycle_tab_history_forward();
                    }
                    if !app.open_tabs.is_empty() {
                        app.view = View::Chat;
                    }
                    return Ok(());
                }
                // Alt+/ = open tab modal
                KeyCode::Char('/') => {
                    if !app.open_tabs.is_empty() || app.view == View::Chat || app.view == View::Home {
                        app.open_tab_modal();
                    }
                    return Ok(());
                }
                // Alt+Left = previous tab, Alt+Right = next tab
                KeyCode::Left => {
                    app.prev_tab();
                    return Ok(());
                }
                KeyCode::Right => {
                    app.next_tab();
                    return Ok(());
                }
                _ => {}
            }
        }
    }

    // Handle Home view (projects modal and panel navigation)
    if app.view == View::Home {
        handle_home_view_key(app, key)?;
        return Ok(());
    }

    // Handle Chat view with rich text editor
    if app.view == View::Chat && app.input_mode == InputMode::Editing {
        handle_chat_editor_key(app, key);
        return Ok(());
    }

    // Handle tab navigation in Chat view (Normal mode)
    if app.view == View::Chat && app.input_mode == InputMode::Normal {
        let modifiers = key.modifiers;
        let has_shift = modifiers.contains(KeyModifiers::SHIFT);
        let has_alt = modifiers.contains(KeyModifiers::ALT);

        match code {
            // Alt+B = open branch selector
            KeyCode::Char('b') if has_alt => {
                app.open_branch_selector();
                return Ok(());
            }
            // Number keys 1-9 to navigate (1 = Home, 2-9 = tabs) in Normal mode
            KeyCode::Char('1') => {
                app.save_chat_draft();
                app.view = View::Home;
                return Ok(());
            }
            KeyCode::Char(c) if c >= '2' && c <= '9' => {
                let tab_index = (c as usize) - ('2' as usize); // '2' -> 0, '3' -> 1, etc.
                if tab_index < app.open_tabs.len() {
                    app.switch_to_tab(tab_index);
                }
                return Ok(());
            }
            // Tab key cycles through tabs (Shift+Tab = prev, Tab = next)
            KeyCode::Tab => {
                if has_shift {
                    app.prev_tab();
                } else {
                    app.next_tab();
                }
                return Ok(());
            }
            // x closes current tab
            KeyCode::Char('x') => {
                app.close_current_tab();
                return Ok(());
            }
            // / = open message actions modal
            KeyCode::Char('/') => {
                app.open_message_actions_modal();
                return Ok(());
            }
            _ => {}
        }
    }

    match app.input_mode {
        InputMode::Normal => match code {
            KeyCode::Char('q') => {
                app.quit();
            }
            KeyCode::Char('r') => {
                if let Some(core_handle) = app.core_handle.clone() {
                    app.set_status("Syncing...");
                    if let Err(e) = core_handle.send(NostrCommand::Sync) {
                        app.set_status(&format!("Sync request failed: {}", e));
                    }
                }
            }
            KeyCode::Char(c) => {
                if c == 'a' && app.view == View::Chat && !app.available_agents().is_empty() {
                    // 'a' opens agent selector
                    app.open_agent_selector();
                } else if c == '@' && app.view == View::Chat && !app.available_agents().is_empty() {
                    app.open_agent_selector();
                } else if c == '.' && app.view == View::Chat {
                    // '.' stops agents working on the selected item
                    // Uses delegation thread_id if a DelegationPreview is selected
                    if let Some(stop_thread_id) = app.get_stop_target_thread_id() {
                        let (is_busy, project_a_tag) = {
                            let store = app.data_store.borrow();
                            let is_busy = store.is_event_busy(&stop_thread_id);
                            let project_a_tag = store.find_project_for_thread(&stop_thread_id);
                            (is_busy, project_a_tag)
                        };
                        if is_busy {
                            if let (Some(core_handle), Some(a_tag)) = (app.core_handle.clone(), project_a_tag) {
                                let working_agents = app.data_store.borrow().get_working_agents(&stop_thread_id);
                                if let Err(e) = core_handle.send(NostrCommand::StopOperations {
                                    project_a_tag: a_tag,
                                    event_ids: vec![stop_thread_id.clone()],
                                    agent_pubkeys: working_agents,
                                }) {
                                    app.set_status(&format!("Failed to stop: {}", e));
                                } else {
                                    app.set_status("Stop command sent");
                                }
                            }
                        }
                    }
                } else if c == 't' && app.view == View::Chat {
                    // 't' toggles todo sidebar
                    app.todo_sidebar_visible = !app.todo_sidebar_visible;
                } else if c == 'o' && app.view == View::Chat {
                    app.open_first_image();
                } else if c == 'j' && app.view == View::LessonViewer {
                    app.scroll_down(3);
                } else if c == 'k' && app.view == View::LessonViewer {
                    app.scroll_up(3);
                } else if c == 'j' && app.view == View::AgentBrowser && app.agent_browser_in_detail {
                    app.scroll_down(3);
                } else if c == 'k' && app.view == View::AgentBrowser && app.agent_browser_in_detail {
                    app.scroll_up(3);
                } else if c == 'f' && app.view == View::AgentBrowser && app.agent_browser_in_detail {
                    // Fork the currently viewed agent
                    if let Some(ref agent_id) = app.viewing_agent_id.clone() {
                        if let Some(agent) = app.all_agent_definitions().iter().find(|a| a.id == *agent_id).cloned() {
                            app.modal_state = ui::modal::ModalState::CreateAgent(
                                ui::modal::CreateAgentState::fork_from(&agent)
                            );
                        }
                    }
                } else if c == 'c' && app.view == View::AgentBrowser && app.agent_browser_in_detail {
                    // Clone the currently viewed agent
                    if let Some(ref agent_id) = app.viewing_agent_id.clone() {
                        if let Some(agent) = app.all_agent_definitions().iter().find(|a| a.id == *agent_id).cloned() {
                            app.modal_state = ui::modal::ModalState::CreateAgent(
                                ui::modal::CreateAgentState::clone_from(&agent)
                            );
                        }
                    }
                } else if c == 'n' && app.view == View::AgentBrowser && !app.agent_browser_in_detail {
                    // Create new agent
                    app.modal_state = ui::modal::ModalState::CreateAgent(
                        ui::modal::CreateAgentState::new()
                    );
                } else if app.view == View::AgentBrowser && !app.agent_browser_in_detail && c != 'q' && c != 'n' {
                    // In list mode, add characters to search filter (but not 'q' for quit or 'n' for new)
                    app.agent_browser_filter.push(c);
                    app.agent_browser_index = 0; // Reset selection when filter changes
                } else if c >= '1' && c <= '5' && app.view == View::LessonViewer {
                    // Navigate to section 1-5
                    let section_index = (c as usize) - ('1' as usize);
                    if let Some(ref lesson_id) = app.viewing_lesson_id {
                        if let Some(lesson) = app.data_store.borrow().get_lesson(lesson_id) {
                            if section_index < lesson.sections().len() {
                                app.lesson_viewer_section = section_index;
                                app.scroll_offset = 0; // Reset scroll when changing sections
                            }
                        }
                    }
                }
            }
            KeyCode::Backspace => {
                if app.view == View::AgentBrowser && !app.agent_browser_in_detail {
                    app.agent_browser_filter.pop();
                    app.agent_browser_index = 0;
                }
            }
            KeyCode::Up => match app.view {
                View::Chat => {
                    if app.selected_message_index > 0 {
                        app.selected_message_index -= 1;
                    }
                }
                View::LessonViewer => {
                    app.scroll_up(3);
                }
                View::AgentBrowser => {
                    if app.agent_browser_in_detail {
                        app.scroll_up(3);
                    } else if app.agent_browser_index > 0 {
                        app.agent_browser_index -= 1;
                    }
                }
                _ => {}
            },
            KeyCode::Down => match app.view {
                View::LessonViewer => {
                    app.scroll_down(3);
                }
                View::AgentBrowser => {
                    if app.agent_browser_in_detail {
                        app.scroll_down(3);
                    } else {
                        let count = app.filtered_agent_definitions().len();
                        if app.agent_browser_index < count.saturating_sub(1) {
                            app.agent_browser_index += 1;
                        }
                    }
                }
                View::Chat => {
                    // Get grouped item count for bounds checking (selected_message_index is group index)
                    let messages = app.messages();
                    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());
                    let user_pubkey = app.data_store.borrow().user_pubkey.clone();

                    let display_messages: Vec<&crate::models::Message> = if let Some(ref root_id) = app.subthread_root {
                        messages.iter()
                            .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                            .collect()
                    } else {
                        // Include thread root + direct replies
                        messages.iter()
                            .filter(|m| {
                                Some(m.id.as_str()) == thread_id
                                    || m.reply_to.is_none()
                                    || m.reply_to.as_deref() == thread_id
                            })
                            .collect()
                    };

                    // Group messages to get actual count of selectable items
                    let grouped = group_messages(&display_messages, user_pubkey.as_deref());

                    if app.selected_message_index < grouped.len().saturating_sub(1) {
                        app.selected_message_index += 1;
                    }
                }
                _ => {}
            },
            KeyCode::Home => {
                if app.view == View::Chat {
                    app.scroll_offset = 0;
                }
            }
            KeyCode::End => {
                if app.view == View::Chat {
                    app.scroll_to_bottom();
                }
            }
            KeyCode::PageUp => {
                if app.view == View::Chat {
                    app.scroll_up(20);
                }
            }
            KeyCode::PageDown => {
                if app.view == View::Chat {
                    app.scroll_down(20);
                }
            }
            KeyCode::Enter => match app.view {
                View::Chat => {
                    // Get messages and build grouped display model (same as rendering)
                    let messages = app.messages();
                    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());
                    let user_pubkey = app.data_store.borrow().user_pubkey.clone();

                    // Get display messages based on current view (must match rendering in messages.rs)
                    let display_messages: Vec<&crate::models::Message> = if let Some(ref root_id) = app.subthread_root {
                        messages.iter()
                            .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                            .collect()
                    } else {
                        messages.iter()
                            .filter(|m| {
                                // Include thread root (id == thread_id) + direct replies
                                Some(m.id.as_str()) == thread_id
                                    || m.reply_to.is_none()
                                    || m.reply_to.as_deref() == thread_id
                            })
                            .collect()
                    };

                    // Group messages to match rendering
                    let grouped = group_messages(&display_messages, user_pubkey.as_deref());

                    // Handle based on what's selected
                    if let Some(item) = grouped.get(app.selected_message_index) {
                        match item {
                            DisplayItem::AgentGroup { messages: group_messages, collapsed_count, .. } => {
                                // For groups with collapsed messages, toggle expansion
                                if *collapsed_count > 0 {
                                    if let Some(first_msg) = group_messages.first() {
                                        app.toggle_group_expansion(&first_msg.id);
                                    }
                                }
                            }
                            DisplayItem::SingleMessage { message: msg, .. } => {
                                // For single messages, navigate into subthread if it has replies
                                let has_replies = messages.iter().any(|m| {
                                    m.reply_to.as_deref() == Some(msg.id.as_str()) &&
                                    // Only count as reply if parent is NOT the thread root
                                    Some(msg.id.as_str()) != thread_id
                                });
                                if has_replies {
                                    app.enter_subthread((*msg).clone());
                                }
                            }
                            DisplayItem::DelegationPreview { thread_id, .. } => {
                                // Navigate to the delegated conversation
                                let thread_and_project = {
                                    let store = app.data_store.borrow();
                                    store.get_thread_by_id(thread_id).map(|t| {
                                        let project_a_tag = store.find_project_for_thread(thread_id)
                                            .unwrap_or_default();
                                        (t.clone(), project_a_tag)
                                    })
                                };
                                if let Some((thread, project_a_tag)) = thread_and_project {
                                    app.open_thread_from_home(&thread, &project_a_tag);
                                }
                            }
                        }
                    }
                }
                View::AgentBrowser => {
                    if !app.agent_browser_in_detail {
                        let agents = app.filtered_agent_definitions();
                        if let Some(agent) = agents.get(app.agent_browser_index) {
                            app.viewing_agent_id = Some(agent.id.clone());
                            app.agent_browser_in_detail = true;
                            app.scroll_offset = 0;
                        }
                    }
                }
                _ => {}
            },
            KeyCode::Esc => match app.view {
                View::Chat => {
                    if app.in_subthread() {
                        // Exit subthread view and return to main thread view
                        app.exit_subthread();
                    } else {
                        // Exit chat and go back to home
                        app.save_chat_draft();
                        app.chat_editor.clear();
                        app.view = View::Home;
                    }
                }
                View::LessonViewer => {
                    // Return to home view
                    app.view = View::Home;
                    app.viewing_lesson_id = None;
                    app.lesson_viewer_section = 0;
                    app.scroll_offset = 0;
                }
                View::AgentBrowser => {
                    if app.agent_browser_in_detail {
                        // Exit detail view and return to list
                        app.agent_browser_in_detail = false;
                        app.viewing_agent_id = None;
                        app.scroll_offset = 0;
                    } else {
                        // Exit browser and go back to home
                        app.view = View::Home;
                        app.agent_browser_filter.clear();
                        app.agent_browser_index = 0;
                    }
                }
                _ => {}
            },
            _ => {}
        },
        // Editing mode for non-Chat views (Login, Threads)
        InputMode::Editing => match code {
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
                app.clear_input();
                if app.creating_thread {
                    app.creating_thread = false;
                }
            }
            KeyCode::Char(c) => app.enter_char(c),
            KeyCode::Backspace => app.delete_char(),
            KeyCode::Left => app.move_cursor_left(),
            KeyCode::Right => app.move_cursor_right(),
            KeyCode::Enter => {
                let input = app.submit_input();
                app.input_mode = InputMode::Normal;

                match app.view {
                    View::Login => match login_step {
                        LoginStep::Nsec => {
                            // Check if user wants to use stored credentials
                            if input.is_empty() && nostr::has_stored_credentials(&app.db.credentials_conn()) {
                                *pending_nsec = None;
                                *login_step = LoginStep::Password;
                            } else if input.starts_with("nsec") {
                                *pending_nsec = Some(input);
                                *login_step = LoginStep::Password;
                            } else {
                                app.set_status("Invalid nsec format");
                            }
                        }
                        LoginStep::Password => {
                            let keys_result = if pending_nsec.is_none() {
                                nostr::load_stored_keys(&input, &app.db.credentials_conn())
                            } else if let Some(ref nsec) = pending_nsec {
                                let password = if input.is_empty() { None } else { Some(input.as_str()) };
                                nostr::auth::login_with_nsec(nsec, password, &app.db.credentials_conn())
                            } else {
                                Err(anyhow::anyhow!("No credentials provided"))
                            };

                            match keys_result {
                                Ok(keys) => {
                                    let user_pubkey = nostr::get_current_pubkey(&keys);
                                    app.keys = Some(keys.clone());
                                    app.data_store.borrow_mut().set_user_pubkey(user_pubkey.clone());

                                    if let Some(ref core_handle) = app.core_handle {
                                        if let Err(e) = core_handle.send(NostrCommand::Connect {
                                            keys: keys.clone(),
                                            user_pubkey: user_pubkey.clone(),
                                        }) {
                                            app.set_status(&format!("Failed to connect: {}", e));
                                            *login_step = LoginStep::Nsec;
                                        } else if let Err(e) = core_handle.send(NostrCommand::Sync) {
                                            app.set_status(&format!("Failed to sync: {}", e));
                                        } else {
                                            app.view = View::Home;
                                            app.load_filter_preferences();
                                            app.dismiss_notification();
                                        }
                                    }
                                }
                                Err(e) => {
                                    app.set_status(&format!("Login failed: {}", e));
                                    *login_step = LoginStep::Nsec;
                                }
                            }
                            *pending_nsec = None;
                        }
                        LoginStep::Unlock => {
                            let keys_result = nostr::load_stored_keys(&input, &app.db.credentials_conn());

                            match keys_result {
                                Ok(keys) => {
                                    let user_pubkey = nostr::get_current_pubkey(&keys);
                                    app.keys = Some(keys.clone());
                                    app.data_store.borrow_mut().set_user_pubkey(user_pubkey.clone());

                                    if let Some(ref core_handle) = app.core_handle {
                                        if let Err(e) = core_handle.send(NostrCommand::Connect {
                                            keys: keys.clone(),
                                            user_pubkey: user_pubkey.clone(),
                                        }) {
                                            app.set_status(&format!("Failed to connect: {}", e));
                                            *login_step = LoginStep::Unlock;
                                        } else if let Err(e) = core_handle.send(NostrCommand::Sync) {
                                            app.set_status(&format!("Failed to sync: {}", e));
                                        } else {
                                            app.view = View::Home;
                                            app.load_filter_preferences();
                                            app.dismiss_notification();
                                        }
                                    }
                                }
                                Err(e) => {
                                    app.set_status(&format!(
                                        "Unlock failed: {}. Press Esc to clear input and retry.",
                                        e
                                    ));
                                }
                            }
                        }
                    },
                    _ => {}
                }
            }
            _ => {}
        },
    }

    Ok(())
}

/// Handle key events for Home view (panel navigation and projects modal)
fn handle_home_view_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    // Handle Reports search input mode
    if app.input_mode == InputMode::Editing && app.home_panel_focus == HomeTab::Reports {
        match code {
            KeyCode::Char(c) => {
                app.report_search_filter.push(c);
                app.tab_selection.insert(HomeTab::Reports, 0);
            }
            KeyCode::Backspace => {
                app.report_search_filter.pop();
                app.tab_selection.insert(HomeTab::Reports, 0);
            }
            KeyCode::Esc | KeyCode::Enter => {
                app.input_mode = InputMode::Normal;
            }
            _ => {}
        }
        return Ok(());
    }

    // Handle projects modal when showing (using ModalState)
    if matches!(app.modal_state, ModalState::ProjectsModal { .. }) {
        // Get projects and for_new_thread flag BEFORE mutably borrowing modal_state
        let (online_projects, offline_projects) = app.filtered_projects();
        let all_projects: Vec<_> = online_projects.into_iter().chain(offline_projects).collect();
        let item_count = all_projects.len();
        let for_new_thread = matches!(app.modal_state, ModalState::ProjectsModal { for_new_thread: true, .. });

        if let ModalState::ProjectsModal { ref mut selector, .. } = app.modal_state {
            match handle_selector_key(selector, key, item_count, |idx| all_projects.get(idx).cloned()) {
                SelectorAction::Selected(project) => {
                    let a_tag = project.a_tag();
                    app.selected_project = Some(project);

                    // Auto-select PM agent and default branch from status
                    if let Some(status) = app.data_store.borrow().get_project_status(&a_tag) {
                        // Always select PM agent for new threads
                        if for_new_thread || app.selected_agent.is_none() {
                            if let Some(pm) = status.pm_agent() {
                                app.selected_agent = Some(pm.clone());
                            }
                        }
                        if app.selected_branch.is_none() {
                            app.selected_branch = status.default_branch().map(String::from);
                        }
                    }

                    app.modal_state = ModalState::None;

                    if for_new_thread {
                        // Create draft tab and navigate to chat view
                        let project_name = app.selected_project.as_ref()
                            .map(|p| p.title.clone())
                            .unwrap_or_else(|| "New".to_string());
                        let tab_idx = app.open_draft_tab(&a_tag, &project_name);
                        app.switch_to_tab(tab_idx);
                        app.chat_editor.clear();
                    } else {
                        // Set filter to show only this project (existing behavior)
                        app.visible_projects.clear();
                        app.visible_projects.insert(a_tag);
                    }
                }
                SelectorAction::Cancelled => {
                    app.modal_state = ModalState::None;
                }
                SelectorAction::Continue => {}
            }
        }
        return Ok(());
    }

    // Handle project settings modal when showing
    if matches!(app.modal_state, ModalState::ProjectSettings(_)) {
        handle_project_settings_key(app, key);
        return Ok(());
    }

    // Handle create project modal when showing
    if matches!(app.modal_state, ModalState::CreateProject(_)) {
        handle_create_project_key(app, key);
        return Ok(());
    }

    // Normal Home view navigation
    match code {
        KeyCode::Char('q') => app.quit(),
        KeyCode::Char('/') => {
            if app.home_panel_focus == HomeTab::Reports {
                // Enter search mode for Reports tab
                app.input_mode = InputMode::Editing;
            } else if app.home_panel_focus == HomeTab::Recent {
                // Open conversation actions modal for selected thread
                let hierarchy = get_hierarchical_threads(app);
                if let Some(item) = hierarchy.get(app.current_selection()) {
                    let thread_id = item.thread.id.clone();
                    let thread_title = item.thread.title.clone();
                    let project_a_tag = item.a_tag.clone();
                    let is_archived = app.is_thread_archived(&thread_id);
                    app.modal_state = ModalState::ConversationActions(
                        ui::modal::ConversationActionsState::new(thread_id, thread_title, project_a_tag, is_archived)
                    );
                }
            } else if app.home_panel_focus == HomeTab::Inbox {
                // Open conversation actions modal for selected inbox item
                let items = app.inbox_items();
                if let Some(item) = items.get(app.current_selection()) {
                    if let Some(ref thread_id) = item.thread_id {
                        let project_a_tag = item.project_a_tag.clone();
                        // Find thread to get title
                        let thread = app.data_store.borrow().get_threads(&project_a_tag)
                            .iter()
                            .find(|t| t.id == *thread_id)
                            .cloned();
                        if let Some(thread) = thread {
                            let is_archived = app.is_thread_archived(thread_id);
                            app.modal_state = ModalState::ConversationActions(
                                ui::modal::ConversationActionsState::new(
                                    thread_id.clone(),
                                    thread.title.clone(),
                                    project_a_tag,
                                    is_archived
                                )
                            );
                        }
                    }
                }
            }
        }
        KeyCode::Char('p') => {
            app.open_projects_modal(false);
        }
        KeyCode::Char('n') => {
            // Open projects modal - selecting a project navigates to chat to create new thread
            app.open_projects_modal(true);
        }
        KeyCode::Char('m') => {
            // Toggle "only by me" filter
            app.toggle_only_by_me();
        }
        KeyCode::Char('f') => {
            // Cycle through time filter options
            app.cycle_time_filter();
        }
        KeyCode::Char('A') => {
            // Open agent browser
            app.open_agent_browser();
        }
        KeyCode::Char('N') if has_shift => {
            // Open create project modal
            app.modal_state = ui::modal::ModalState::CreateProject(
                ui::modal::CreateProjectState::new()
            );
        }
        KeyCode::Tab => {
            // Switch between tabs (forward)
            app.home_panel_focus = match app.home_panel_focus {
                HomeTab::Recent => HomeTab::Inbox,
                HomeTab::Inbox => HomeTab::Reports,
                HomeTab::Reports => HomeTab::Status,
                HomeTab::Status => HomeTab::Recent,
            };
        }
        KeyCode::BackTab if has_shift => {
            // Shift+Tab switches tabs (backward)
            app.home_panel_focus = match app.home_panel_focus {
                HomeTab::Recent => HomeTab::Status,
                HomeTab::Inbox => HomeTab::Recent,
                HomeTab::Reports => HomeTab::Inbox,
                HomeTab::Status => HomeTab::Reports,
            };
        }
        KeyCode::Right => {
            // Move focus to sidebar (on the right)
            app.sidebar_focused = true;
        }
        KeyCode::Left => {
            // Move focus to content area (on the left)
            app.sidebar_focused = false;
        }
        KeyCode::Up => {
            if app.sidebar_focused {
                // Navigate sidebar projects
                if app.sidebar_project_index > 0 {
                    app.sidebar_project_index -= 1;
                }
            } else {
                // Navigate content using consolidated state
                let current = app.current_selection();
                if current > 0 {
                    app.set_current_selection(current - 1);
                }
            }
        }
        KeyCode::Down => {
            if app.sidebar_focused {
                // Navigate sidebar projects
                let (online, offline) = app.filtered_projects();
                let max = (online.len() + offline.len()).saturating_sub(1);
                if app.sidebar_project_index < max {
                    app.sidebar_project_index += 1;
                }
            } else {
                // Navigate content using consolidated state
                let current = app.current_selection();
                let max = match app.home_panel_focus {
                    HomeTab::Inbox => app.inbox_items().len().saturating_sub(1),
                    HomeTab::Recent => get_hierarchical_threads(app).len().saturating_sub(1),
                    HomeTab::Reports => app.reports().len().saturating_sub(1),
                    HomeTab::Status => app.status_threads().len().saturating_sub(1),
                };
                if current < max {
                    app.set_current_selection(current + 1);
                }
            }
        }
        KeyCode::Char(' ') if app.sidebar_focused => {
            // Toggle project visibility in sidebar
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                let a_tag = project.a_tag();
                if app.visible_projects.contains(&a_tag) {
                    app.visible_projects.remove(&a_tag);
                } else {
                    app.visible_projects.insert(a_tag);
                }
                app.save_selected_projects();
            }
        }
        KeyCode::Char('s') if app.sidebar_focused => {
            // Open project settings for focused project
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                let a_tag = project.a_tag();
                let project_name = project.name.clone();
                let agent_ids = project.agent_ids.clone();

                app.modal_state = ui::modal::ModalState::ProjectSettings(
                    ui::modal::ProjectSettingsState::new(a_tag, project_name, agent_ids)
                );
            }
        }
        KeyCode::Char('S') if app.sidebar_focused && has_shift => {
            // Stop all agents working on this project (Shift+S)
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                let a_tag = project.a_tag();
                let (is_busy, event_ids, agent_pubkeys) = {
                    let store = app.data_store.borrow();
                    (
                        store.is_project_busy(&a_tag),
                        store.get_active_event_ids(&a_tag),
                        store.get_project_working_agents(&a_tag),
                    )
                };
                if is_busy {
                    if let Some(core_handle) = app.core_handle.clone() {
                        if let Err(e) = core_handle.send(NostrCommand::StopOperations {
                            project_a_tag: a_tag,
                            event_ids,
                            agent_pubkeys,
                        }) {
                            app.set_status(&format!("Failed to stop: {}", e));
                        } else {
                            app.set_status("Stop command sent for all project operations");
                        }
                    }
                }
            }
        }
        KeyCode::Char('b') if app.sidebar_focused => {
            // Boot offline project
            let (online, offline) = app.filtered_projects();
            let online_count = online.len();
            if app.sidebar_project_index >= online_count {
                // This is an offline project
                let offline_index = app.sidebar_project_index - online_count;
                if let Some(project) = offline.get(offline_index) {
                    let a_tag = project.a_tag();
                    let pubkey = project.pubkey.clone();
                    if let Some(core_handle) = app.core_handle.clone() {
                        if let Err(e) = core_handle.send(NostrCommand::BootProject {
                            project_a_tag: a_tag,
                            project_pubkey: Some(pubkey),
                        }) {
                            app.set_status(&format!("Failed to boot: {}", e));
                        } else {
                            app.set_status(&format!("Boot request sent for {}", project.name));
                        }
                    }
                }
            } else {
                app.set_status("Project is already online");
            }
        }
        KeyCode::Enter => {
            if app.sidebar_focused {
                // Open project actions modal
                let (online, offline) = app.filtered_projects();
                let online_count = online.len();
                let is_online = app.sidebar_project_index < online_count;
                let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
                if let Some(project) = all_projects.get(app.sidebar_project_index) {
                    app.modal_state = ui::modal::ModalState::ProjectActions(
                        ui::modal::ProjectActionsState::new(
                            project.a_tag(),
                            project.name.clone(),
                            project.pubkey.clone(),
                            is_online,
                        )
                    );
                }
            } else {
                // Open selected item
                let idx = app.current_selection();
                match app.home_panel_focus {
                    HomeTab::Inbox => {
                        let items = app.inbox_items();
                        if let Some(item) = items.get(idx) {
                            // Mark as read
                            let item_id = item.id.clone();
                            app.data_store.borrow_mut().mark_inbox_read(&item_id);

                            // Navigate to thread if available
                            if let Some(ref thread_id) = item.thread_id {
                                let project_a_tag = item.project_a_tag.clone();

                                // Find the thread
                                let thread = app.data_store.borrow().get_threads(&project_a_tag)
                                    .iter()
                                    .find(|t| t.id == *thread_id)
                                    .cloned();

                                if let Some(thread) = thread {
                                    app.open_thread_from_home(&thread, &project_a_tag);
                                }
                            }
                        }
                    }
                    HomeTab::Recent => {
                        // Use hierarchy for selection (respects collapsed state)
                        let hierarchy = get_hierarchical_threads(app);
                        if let Some(item) = hierarchy.get(idx) {
                            let thread = item.thread.clone();
                            let a_tag = item.a_tag.clone();
                            app.open_thread_from_home(&thread, &a_tag);
                        }
                    }
                    HomeTab::Reports => {
                        let reports = app.reports();
                        if let Some(report) = reports.get(idx) {
                            app.modal_state = ModalState::ReportViewer(
                                ui::modal::ReportViewerState::new(report.clone())
                            );
                        }
                    }
                    HomeTab::Status => {
                        let status_items = app.status_threads();
                        if let Some((thread, a_tag)) = status_items.get(idx) {
                            app.open_thread_from_home(thread, a_tag);
                        }
                    }
                }
            }
        }
        KeyCode::Char('r') if app.home_panel_focus == HomeTab::Inbox => {
            // Mark current inbox item as read
            let items = app.inbox_items();
            if let Some(item) = items.get(app.current_selection()) {
                let item_id = item.id.clone();
                app.data_store.borrow_mut().mark_inbox_read(&item_id);
            }
        }
        KeyCode::Char(' ') if app.home_panel_focus == HomeTab::Recent => {
            // Toggle collapse for threads with children
            let hierarchy = get_hierarchical_threads(app);
            if let Some(item) = hierarchy.get(app.current_selection()) {
                if item.has_children {
                    app.toggle_thread_collapse(&item.thread.id);
                }
            }
        }
        KeyCode::Char('x') if app.home_panel_focus == HomeTab::Recent && !app.sidebar_focused => {
            // Quick archive - press x to archive selected thread
            let hierarchy = get_hierarchical_threads(app);
            if let Some(item) = hierarchy.get(app.current_selection()) {
                let thread_id = item.thread.id.clone();
                let thread_title = item.thread.title.clone();
                let is_now_archived = app.toggle_thread_archived(&thread_id);
                let status = if is_now_archived {
                    format!("Archived: {}", thread_title)
                } else {
                    format!("Unarchived: {}", thread_title)
                };
                app.set_status(&status);
            }
        }
        KeyCode::Char('x') if app.home_panel_focus == HomeTab::Inbox && !app.sidebar_focused => {
            // Quick archive - press x to archive selected inbox item's thread
            let items = app.inbox_items();
            if let Some(item) = items.get(app.current_selection()) {
                if let Some(ref thread_id) = item.thread_id {
                    let thread_id = thread_id.clone();
                    // Find thread title
                    let thread_title = app.data_store.borrow().get_threads(&item.project_a_tag)
                        .iter()
                        .find(|t| t.id == thread_id)
                        .map(|t| t.title.clone())
                        .unwrap_or_else(|| "Conversation".to_string());
                    let is_now_archived = app.toggle_thread_archived(&thread_id);
                    let status = if is_now_archived {
                        format!("Archived: {}", thread_title)
                    } else {
                        format!("Unarchived: {}", thread_title)
                    };
                    app.set_status(&status);
                }
            }
        }
        KeyCode::Char('x') if app.home_panel_focus == HomeTab::Status && !app.sidebar_focused => {
            // Quick archive - press x to archive selected status thread
            let status_items = app.status_threads();
            if let Some((thread, _)) = status_items.get(app.current_selection()) {
                let thread_id = thread.id.clone();
                let thread_title = thread.title.clone();
                let is_now_archived = app.toggle_thread_archived(&thread_id);
                let status = if is_now_archived {
                    format!("Archived: {}", thread_title)
                } else {
                    format!("Unarchived: {}", thread_title)
                };
                app.set_status(&status);
            }
        }
        // Vim-style navigation (j/k)
        KeyCode::Char('k') if !app.sidebar_focused => {
            let current = app.current_selection();
            if current > 0 {
                app.set_current_selection(current - 1);
            }
        }
        KeyCode::Char('j') if !app.sidebar_focused => {
            let current = app.current_selection();
            let max = match app.home_panel_focus {
                HomeTab::Inbox => app.inbox_items().len().saturating_sub(1),
                HomeTab::Recent => get_hierarchical_threads(app).len().saturating_sub(1),
                HomeTab::Reports => app.reports().len().saturating_sub(1),
                HomeTab::Status => app.status_threads().len().saturating_sub(1),
            };
            if current < max {
                app.set_current_selection(current + 1);
            }
        }
        // Esc to clear Reports search filter
        KeyCode::Esc if app.home_panel_focus == HomeTab::Reports => {
            if !app.report_search_filter.is_empty() {
                app.report_search_filter.clear();
                app.tab_selection.insert(HomeTab::Reports, 0);
            }
        }
        // Number keys for tab switching (1 = stay on Home, 2-9 = tabs)
        KeyCode::Char('1') => {
            // Already on Home, do nothing
        }
        KeyCode::Char(c) if c >= '2' && c <= '9' => {
            let tab_index = (c as usize) - ('2' as usize); // '2' -> 0, '3' -> 1, etc.
            if tab_index < app.open_tabs.len() {
                app.switch_to_tab(tab_index);
                app.view = View::Chat;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle key events for the project settings modal
fn handle_project_settings_key(app: &mut App, key: KeyEvent) {
    use ui::views::{available_agent_count, get_agent_id_at_index};

    let code = key.code;

    // Extract state to avoid borrow issues
    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::ProjectSettings(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    if state.in_add_mode {
        // Add agent mode
        match code {
            KeyCode::Esc => {
                state.in_add_mode = false;
                state.add_filter.clear();
                state.add_index = 0;
            }
            KeyCode::Up => {
                if state.add_index > 0 {
                    state.add_index -= 1;
                }
            }
            KeyCode::Down => {
                let count = available_agent_count(app, &state);
                if state.add_index + 1 < count {
                    state.add_index += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(agent_id) = get_agent_id_at_index(app, &state, state.add_index) {
                    state.add_agent(agent_id);
                    state.in_add_mode = false;
                    state.add_filter.clear();
                    state.add_index = 0;
                }
            }
            KeyCode::Char(c) => {
                state.add_filter.push(c);
                state.add_index = 0;
            }
            KeyCode::Backspace => {
                state.add_filter.pop();
                state.add_index = 0;
            }
            _ => {}
        }
    } else {
        // Main settings mode
        match code {
            KeyCode::Esc => {
                // Close modal without saving
                app.modal_state = ModalState::None;
                return;
            }
            KeyCode::Up => {
                if state.selector_index > 0 {
                    state.selector_index -= 1;
                }
            }
            KeyCode::Down => {
                let count = state.pending_agent_ids.len();
                if state.selector_index + 1 < count {
                    state.selector_index += 1;
                }
            }
            KeyCode::Char('a') => {
                state.in_add_mode = true;
                state.add_filter.clear();
                state.add_index = 0;
            }
            KeyCode::Char('d') => {
                if !state.pending_agent_ids.is_empty() {
                    state.remove_agent(state.selector_index);
                    // Adjust index if needed
                    if state.selector_index >= state.pending_agent_ids.len() && state.selector_index > 0 {
                        state.selector_index -= 1;
                    }
                }
            }
            KeyCode::Char('p') => {
                if !state.pending_agent_ids.is_empty() && state.selector_index > 0 {
                    state.set_pm(state.selector_index);
                    state.selector_index = 0; // Move selection to new PM position
                }
            }
            KeyCode::Enter => {
                if state.has_changes() {
                    // Publish the changes
                    let project_a_tag = state.project_a_tag.clone();
                    let agent_ids = state.pending_agent_ids.clone();

                    if let Some(ref core_handle) = app.core_handle {
                        if let Err(e) = core_handle.send(NostrCommand::UpdateProjectAgents {
                            project_a_tag,
                            agent_ids,
                        }) {
                            app.set_status(&format!("Failed to update agents: {}", e));
                        } else {
                            app.set_status("Project agents updated");
                        }
                    }

                    app.modal_state = ModalState::None;
                    return;
                }
            }
            _ => {}
        }
    }

    // Restore the state
    app.modal_state = ModalState::ProjectSettings(state);
}

/// Handle key events for the create project modal
fn handle_create_project_key(app: &mut App, key: KeyEvent) {
    use ui::modal::{CreateProjectFocus, CreateProjectStep};

    let code = key.code;

    // Extract state to avoid borrow issues
    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::CreateProject(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    match state.step {
        CreateProjectStep::Details => {
            match code {
                KeyCode::Esc => {
                    // Cancel - close modal
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Tab => {
                    // Switch focus between fields
                    state.focus = match state.focus {
                        CreateProjectFocus::Name => CreateProjectFocus::Description,
                        CreateProjectFocus::Description => CreateProjectFocus::Name,
                    };
                }
                KeyCode::Enter => {
                    // Proceed to next step if name is valid
                    if state.can_proceed() {
                        state.step = CreateProjectStep::SelectAgents;
                    }
                }
                KeyCode::Char(c) => {
                    match state.focus {
                        CreateProjectFocus::Name => state.name.push(c),
                        CreateProjectFocus::Description => state.description.push(c),
                    }
                }
                KeyCode::Backspace => {
                    match state.focus {
                        CreateProjectFocus::Name => { state.name.pop(); }
                        CreateProjectFocus::Description => { state.description.pop(); }
                    }
                }
                _ => {}
            }
        }
        CreateProjectStep::SelectAgents => {
            // Get filtered agents for index bounds checking
            let filtered_agents = app.agent_definitions_filtered_by(&state.agent_selector.filter);
            let item_count = filtered_agents.len();

            match code {
                KeyCode::Esc => {
                    // Cancel - close modal
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Backspace if state.agent_selector.filter.is_empty() => {
                    // Go back to details step
                    state.step = CreateProjectStep::Details;
                }
                KeyCode::Backspace => {
                    // Remove from filter
                    state.agent_selector.filter.pop();
                    state.agent_selector.index = 0;
                }
                KeyCode::Up => {
                    if state.agent_selector.index > 0 {
                        state.agent_selector.index -= 1;
                    }
                }
                KeyCode::Down => {
                    if item_count > 0 && state.agent_selector.index + 1 < item_count {
                        state.agent_selector.index += 1;
                    }
                }
                KeyCode::Char(' ') => {
                    // Toggle agent selection
                    if let Some(agent) = filtered_agents.get(state.agent_selector.index) {
                        state.toggle_agent(agent.id.clone());
                    }
                }
                KeyCode::Enter => {
                    // Create the project
                    if let Some(ref core_handle) = app.core_handle {
                        if let Err(e) = core_handle.send(NostrCommand::CreateProject {
                            name: state.name.clone(),
                            description: state.description.clone(),
                            agent_ids: state.agent_ids.clone(),
                        }) {
                            app.set_status(&format!("Failed to create project: {}", e));
                        } else {
                            app.set_status("Project created");
                        }
                    }
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Char(c) => {
                    // Add to filter
                    state.agent_selector.filter.push(c);
                    state.agent_selector.index = 0;
                }
                _ => {}
            }
        }
    }

    // Restore the state
    app.modal_state = ModalState::CreateProject(state);
}

/// Handle key events for the create agent modal
fn handle_create_agent_key(app: &mut App, key: KeyEvent) {
    use ui::modal::{AgentCreateStep, AgentFormFocus};

    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);

    // Extract state to avoid borrow issues
    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::CreateAgent(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    match state.step {
        AgentCreateStep::Basics => {
            match code {
                KeyCode::Esc => {
                    // Cancel - close modal
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Tab => {
                    // Cycle through fields
                    state.focus = match state.focus {
                        AgentFormFocus::Name => AgentFormFocus::Description,
                        AgentFormFocus::Description => AgentFormFocus::Role,
                        AgentFormFocus::Role => AgentFormFocus::Name,
                    };
                }
                KeyCode::Enter => {
                    // Proceed to next step if valid
                    if state.can_proceed() {
                        state.step = AgentCreateStep::Instructions;
                    }
                }
                KeyCode::Char(c) => {
                    match state.focus {
                        AgentFormFocus::Name => state.name.push(c),
                        AgentFormFocus::Description => state.description.push(c),
                        AgentFormFocus::Role => state.role.push(c),
                    }
                }
                KeyCode::Backspace => {
                    match state.focus {
                        AgentFormFocus::Name => { state.name.pop(); }
                        AgentFormFocus::Description => { state.description.pop(); }
                        AgentFormFocus::Role => { state.role.pop(); }
                    }
                }
                _ => {}
            }
        }
        AgentCreateStep::Instructions => {
            match code {
                KeyCode::Esc => {
                    // Cancel - close modal
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Enter if has_ctrl => {
                    // Proceed to review step
                    state.step = AgentCreateStep::Review;
                    state.instructions_scroll = 0;
                }
                KeyCode::Enter => {
                    // Add newline to instructions
                    state.instructions.insert(state.instructions_cursor, '\n');
                    state.instructions_cursor += 1;
                }
                KeyCode::Backspace => {
                    if state.instructions_cursor > 0 {
                        state.instructions_cursor -= 1;
                        state.instructions.remove(state.instructions_cursor);
                    } else if state.instructions.is_empty() {
                        // Go back to basics step if empty
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
                    // Move cursor up one line
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
                        // First line, go to start
                        state.instructions_cursor = col.min(current_line_start.saturating_sub(1));
                    }
                }
                KeyCode::Down => {
                    // Move cursor down one line
                    let current_line_start = state.instructions[..state.instructions_cursor]
                        .rfind('\n')
                        .map(|pos| pos + 1)
                        .unwrap_or(0);
                    let col = state.instructions_cursor - current_line_start;

                    if let Some(next_line_start_offset) = state.instructions[state.instructions_cursor..]
                        .find('\n')
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
                    // Move to start of current line
                    state.instructions_cursor = state.instructions[..state.instructions_cursor]
                        .rfind('\n')
                        .map(|pos| pos + 1)
                        .unwrap_or(0);
                }
                KeyCode::End => {
                    // Move to end of current line
                    state.instructions_cursor = state.instructions[state.instructions_cursor..]
                        .find('\n')
                        .map(|pos| state.instructions_cursor + pos)
                        .unwrap_or(state.instructions.len());
                }
                _ => {}
            }
        }
        AgentCreateStep::Review => {
            match code {
                KeyCode::Esc => {
                    // Cancel - close modal
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Backspace => {
                    // Go back to instructions step
                    state.step = AgentCreateStep::Instructions;
                    state.instructions_scroll = 0;
                }
                KeyCode::Up => {
                    // Scroll up
                    if state.instructions_scroll > 0 {
                        state.instructions_scroll -= 1;
                    }
                }
                KeyCode::Down => {
                    // Scroll down
                    let line_count = state.instructions.lines().count();
                    if state.instructions_scroll + 1 < line_count {
                        state.instructions_scroll += 1;
                    }
                }
                KeyCode::Enter => {
                    // Publish the agent definition
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
            }
        }
    }

    // Restore the state
    app.modal_state = ModalState::CreateAgent(state);
}

/// Handle key events for the chat editor (rich text editing)
fn handle_chat_editor_key(app: &mut App, key: KeyEvent) {
    use ui::app::VimMode;

    // If vim mode is enabled, dispatch based on mode
    if app.vim_enabled {
        match app.vim_mode {
            VimMode::Normal => {
                handle_vim_normal_mode(app, key);
                return;
            }
            VimMode::Insert => {
                // Esc exits insert mode
                if key.code == KeyCode::Esc {
                    app.vim_enter_normal();
                    app.save_chat_draft();
                    return;
                }
                // Otherwise fall through to normal editing
            }
        }
    }

    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    match code {
        // Shift+Enter or Alt+Enter = newline
        // Also handle Ctrl+J which is what iTerm2/macOS sends for Shift+Enter (LF = ^J = ASCII 10)
        KeyCode::Enter if has_shift || has_alt => {
            app.chat_editor.insert_newline();
            app.save_chat_draft();
        }
        KeyCode::Char('j') | KeyCode::Char('J') if has_ctrl => {
            // Ctrl+J is Line Feed (ASCII 10), same as Shift+Enter on many terminals
            app.chat_editor.insert_newline();
            app.save_chat_draft();
        }
        // Enter = send message or create new thread
        KeyCode::Enter => {
            let content = app.chat_editor.submit();
            if !content.is_empty() {
                // Save to message history for ↑/↓ navigation
                app.add_to_message_history(content.clone());
                app.exit_history_mode();
                if let (Some(ref core_handle), Some(ref project)) =
                    (&app.core_handle, &app.selected_project)
                {
                    let project_a_tag = project.a_tag();
                    let agent_pubkey = app.selected_agent.as_ref().map(|a| a.pubkey.clone());
                    let branch = app.selected_branch.clone();
                    let nudge_ids = app.selected_nudge_ids.clone();

                    if let Some(ref thread) = app.selected_thread {
                        // Reply to existing thread
                        let thread_id = thread.id.clone();
                        // NIP-22: lowercase "e" tag references the parent message
                        // When in subthread, reply to the subthread root
                        // When in main view, reply to the thread root (or first message)
                        let reply_to = if let Some(ref root_id) = app.subthread_root {
                            Some(root_id.clone())
                        } else {
                            Some(thread_id.clone())
                        };

                        if let Err(e) = core_handle.send(NostrCommand::PublishMessage {
                            thread_id,
                            project_a_tag,
                            content,
                            agent_pubkey,
                            reply_to,
                            branch,
                            nudge_ids,
                            ask_author_pubkey: None,
                        }) {
                            app.set_status(&format!("Failed to publish message: {}", e));
                        } else {
                            app.delete_chat_draft();
                            app.selected_nudge_ids.clear();
                        }
                    } else {
                        // Create new thread (kind:1)
                        let title = content.lines().next().unwrap_or("New Thread").to_string();

                        // Capture the draft_id before sending (if we're in a draft tab)
                        let draft_id = app.find_draft_tab(&project_a_tag)
                            .map(|(_, id)| id.to_string());

                        if let Err(e) = core_handle.send(NostrCommand::PublishThread {
                            project_a_tag: project_a_tag.clone(),
                            title,
                            content,
                            agent_pubkey,
                            branch,
                            nudge_ids,
                        }) {
                            app.set_status(&format!("Failed to create thread: {}", e));
                        } else {
                            // Navigate to it once it arrives via subscription
                            app.pending_new_thread_project = Some(project_a_tag.clone());
                            app.pending_new_thread_draft_id = draft_id;
                            app.selected_nudge_ids.clear();
                        }
                    }
                }
            }
        }
        // Esc = exit input mode (then navigate back via normal mode Esc)
        KeyCode::Esc => {
            app.save_chat_draft();
            app.input_mode = InputMode::Normal;
        }
        // Tab = cycle focus between input and attachments
        KeyCode::Tab if app.chat_editor.has_attachments() => {
            app.chat_editor.cycle_focus();
            // If focused on a paste attachment, open the modal (not for images)
            if app.chat_editor.get_focused_attachment().is_some() {
                app.open_attachment_modal();
            }
        }
        // Up = cycle through message history (when input is empty)
        KeyCode::Up if app.chat_editor.text.is_empty() && !app.chat_editor.has_attachments() => {
            app.history_prev();
        }
        // Down = cycle forward through message history (when browsing)
        KeyCode::Down if app.is_browsing_history() => {
            app.history_next();
        }
        // Up = focus attachments (when there are any)
        KeyCode::Up if app.chat_editor.has_attachments() && app.chat_editor.focused_attachment.is_none() => {
            app.chat_editor.focus_attachments();
        }
        // Down = unfocus attachments (return to input)
        KeyCode::Down if app.chat_editor.focused_attachment.is_some() => {
            app.chat_editor.unfocus_attachments();
        }
        // Left/Right = navigate between attachments when focused
        KeyCode::Left if app.chat_editor.focused_attachment.is_some() => {
            if let Some(idx) = app.chat_editor.focused_attachment {
                if idx > 0 {
                    app.chat_editor.focused_attachment = Some(idx - 1);
                }
            }
        }
        KeyCode::Right if app.chat_editor.focused_attachment.is_some() => {
            if let Some(idx) = app.chat_editor.focused_attachment {
                let total = app.chat_editor.total_attachments();
                if idx + 1 < total {
                    app.chat_editor.focused_attachment = Some(idx + 1);
                }
            }
        }
        // @ = open agent selector
        KeyCode::Char('@') => {
            app.open_agent_selector();
        }
        // % = open branch selector
        KeyCode::Char('%') => {
            app.open_branch_selector();
        }
        // Ctrl+N = open nudge selector
        KeyCode::Char('n') if has_ctrl => {
            app.open_nudge_selector();
        }
        // Ctrl+A = move to beginning of visual line
        KeyCode::Char('a') if has_ctrl => {
            app.chat_editor.move_to_visual_line_start(app.chat_input_wrap_width);
        }
        // Ctrl+E = move to end of visual line
        KeyCode::Char('e') if has_ctrl => {
            app.chat_editor.move_to_visual_line_end(app.chat_input_wrap_width);
        }
        // Ctrl+K = kill to end of line
        KeyCode::Char('k') if has_ctrl => {
            app.chat_editor.kill_to_line_end();
            app.save_chat_draft();
        }
        // Ctrl+U = kill to beginning of line
        KeyCode::Char('u') if has_ctrl => {
            app.chat_editor.kill_to_line_start();
            app.save_chat_draft();
        }
        // Ctrl+W = delete word backward
        KeyCode::Char('w') if has_ctrl => {
            app.chat_editor.delete_word_backward();
            app.save_chat_draft();
        }
        // Ctrl+D = delete character at cursor
        KeyCode::Char('d') if has_ctrl => {
            app.chat_editor.delete_char_at();
            app.save_chat_draft();
        }
        // Ctrl+Shift+Z = redo
        KeyCode::Char('z') if has_ctrl && modifiers.contains(KeyModifiers::SHIFT) => {
            app.chat_editor.redo();
            app.save_chat_draft();
        }
        // Ctrl+Z = undo
        KeyCode::Char('z') if has_ctrl => {
            app.chat_editor.undo();
            app.save_chat_draft();
        }
        // Ctrl+C = copy selection
        KeyCode::Char('c') if has_ctrl => {
            if let Some(selected) = app.chat_editor.selected_text() {
                use arboard::Clipboard;
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(selected);
                }
            }
        }
        // Ctrl+X = cut selection
        KeyCode::Char('x') if has_ctrl => {
            if let Some(selected) = app.chat_editor.selected_text() {
                use arboard::Clipboard;
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(selected);
                }
                app.chat_editor.delete_selection();
                app.save_chat_draft();
            }
        }
        // Shift+Alt+Left = word left extend selection
        KeyCode::Left if has_alt && modifiers.contains(KeyModifiers::SHIFT) => {
            app.chat_editor.move_word_left_extend_selection();
        }
        // Shift+Alt+Right = word right extend selection
        KeyCode::Right if has_alt && modifiers.contains(KeyModifiers::SHIFT) => {
            app.chat_editor.move_word_right_extend_selection();
        }
        // Alt+Left = word left
        KeyCode::Left if has_alt => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_word_left();
        }
        // Alt+Right = word right
        KeyCode::Right if has_alt => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_word_right();
        }
        // Shift+Left = extend selection left
        KeyCode::Left if modifiers.contains(KeyModifiers::SHIFT) => {
            app.chat_editor.move_left_extend_selection();
        }
        // Shift+Right = extend selection right
        KeyCode::Right if modifiers.contains(KeyModifiers::SHIFT) => {
            app.chat_editor.move_right_extend_selection();
        }
        // Basic navigation (clears selection)
        KeyCode::Left => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_left();
        }
        KeyCode::Right => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_right();
        }
        // Home = move to beginning of line
        KeyCode::Home => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_to_line_start();
        }
        // End = move to end of line
        KeyCode::End => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_to_line_end();
        }
        // Alt+Backspace = delete word backward
        KeyCode::Backspace if has_alt => {
            app.chat_editor.delete_word_backward();
            app.save_chat_draft();
        }
        KeyCode::Backspace => {
            // If an attachment is focused, delete it
            if app.chat_editor.focused_attachment.is_some() {
                app.chat_editor.delete_focused_attachment();
            } else {
                app.chat_editor.delete_char_before();
            }
            app.save_chat_draft();
        }
        KeyCode::Delete => {
            // If an attachment is focused, delete it
            if app.chat_editor.focused_attachment.is_some() {
                app.chat_editor.delete_focused_attachment();
            } else {
                app.chat_editor.delete_char_at();
            }
            app.save_chat_draft();
        }
        // Scrolling while editing
        KeyCode::Up if has_ctrl => {
            app.scroll_up(3);
        }
        KeyCode::Down if has_ctrl => {
            app.scroll_down(3);
        }
        // Up/Down = move by visual lines (for wrapped text navigation)
        KeyCode::Up => {
            app.chat_editor.move_up_visual(app.chat_input_wrap_width);
        }
        KeyCode::Down => {
            app.chat_editor.move_down_visual(app.chat_input_wrap_width);
        }
        KeyCode::PageUp => {
            app.scroll_up(20);
        }
        KeyCode::PageDown => {
            app.scroll_down(20);
        }
        // Regular character input
        KeyCode::Char(c) => {
            app.chat_editor.insert_char(c);
            app.save_chat_draft();
        }
        _ => {}
    }
}

/// Handle key events for vim normal mode in chat editor
fn handle_vim_normal_mode(app: &mut App, key: KeyEvent) {
    let code = key.code;

    match code {
        // Ctrl+J is Line Feed (ASCII 10), same as Shift+Enter on iTerm2/macOS
        // MUST come before regular 'j' movement handler
        KeyCode::Char('j') | KeyCode::Char('J')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.chat_editor.insert_newline();
            app.save_chat_draft();
        }

        // Mode switching
        KeyCode::Char('i') => {
            app.vim_enter_insert();
        }
        KeyCode::Char('a') => {
            app.vim_enter_append();
        }
        KeyCode::Char('A') => {
            // Append at end of line
            app.chat_editor.move_to_line_end();
            app.vim_enter_insert();
        }
        KeyCode::Char('I') => {
            // Insert at beginning of line
            app.chat_editor.move_to_line_start();
            app.vim_enter_insert();
        }
        KeyCode::Char('o') => {
            // Open line below
            app.chat_editor.move_to_line_end();
            app.chat_editor.insert_newline();
            app.vim_enter_insert();
            app.save_chat_draft();
        }
        KeyCode::Char('O') => {
            // Open line above
            app.chat_editor.move_to_line_start();
            app.chat_editor.insert_newline();
            app.chat_editor.move_up();
            app.vim_enter_insert();
            app.save_chat_draft();
        }

        // Movement
        KeyCode::Char('h') | KeyCode::Left => {
            app.chat_editor.move_left();
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.chat_editor.move_right();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.chat_editor.move_down_visual(app.chat_input_wrap_width);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.chat_editor.move_up_visual(app.chat_input_wrap_width);
        }
        KeyCode::Char('w') => {
            app.chat_editor.move_word_right();
        }
        KeyCode::Char('b') => {
            app.chat_editor.move_word_left();
        }
        KeyCode::Char('0') => {
            app.chat_editor.move_to_line_start();
        }
        KeyCode::Char('$') => {
            app.chat_editor.move_to_line_end();
        }

        // Editing
        KeyCode::Char('x') => {
            app.chat_editor.delete_char_at();
            app.save_chat_draft();
        }
        KeyCode::Char('X') => {
            app.chat_editor.delete_char_before();
            app.save_chat_draft();
        }
        KeyCode::Char('u') => {
            app.chat_editor.undo();
            app.save_chat_draft();
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.chat_editor.redo();
            app.save_chat_draft();
        }
        KeyCode::Char('D') => {
            app.chat_editor.kill_to_line_end();
            app.save_chat_draft();
        }

        // Esc in normal mode exits editing mode
        KeyCode::Esc => {
            app.save_chat_draft();
            app.input_mode = InputMode::Normal;
        }

        // Shift+Enter or Alt+Enter = newline (even in normal mode)
        KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::SHIFT)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            app.chat_editor.insert_newline();
            app.save_chat_draft();
        }

        _ => {}
    }
}

/// Handle key events for the expanded editor modal (Ctrl+E)
fn handle_expanded_editor_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    match code {
        // Esc = close modal without saving
        KeyCode::Esc => {
            app.cancel_expanded_editor();
        }
        // Ctrl+S = save and close
        KeyCode::Char('s') if has_ctrl => {
            app.save_and_close_expanded_editor();
        }
        // Enter = newline
        KeyCode::Enter => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.insert_newline();
            }
        }
        // Ctrl+Shift+Z = redo
        KeyCode::Char('z') if has_ctrl && has_shift => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.redo();
            }
        }
        // Ctrl+Z = undo
        KeyCode::Char('z') if has_ctrl => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.undo();
            }
        }
        // Ctrl+A = select all
        KeyCode::Char('a') if has_ctrl => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.select_all();
            }
        }
        // Alt+arrows = word navigation with selection
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
        // Shift+arrows = selection
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
        // Basic navigation
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
        // Ctrl+C = copy selection
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
        // Ctrl+X = cut selection
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
        // Regular character input
        KeyCode::Char(c) => {
            if let Some(editor) = app.expanded_editor_mut() {
                editor.insert_char(c);
            }
        }
        _ => {}
    }
}

/// Handle key events for the ask modal
fn handle_ask_modal_key(app: &mut App, key: KeyEvent) {
    use crate::ui::ask_input::InputMode as AskInputMode;

    let code = key.code;
    let modifiers = key.modifiers;

    // Extract modal_state to avoid borrow issues
    let modal_state = match app.ask_modal_state_mut() {
        Some(state) => state,
        None => return,
    };

    let input_state = &mut modal_state.input_state;

    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    match input_state.mode {
        AskInputMode::Selection => {
            match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    input_state.prev_option();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    input_state.next_option();
                }
                KeyCode::Right => {
                    // Skip this question
                    input_state.skip_question();
                    if input_state.is_complete() {
                        submit_ask_response(app);
                    }
                }
                KeyCode::Left => {
                    // Go back to previous question
                    input_state.prev_question();
                }
                KeyCode::Char(' ') if input_state.is_multi_select() => {
                    input_state.toggle_multi_select();
                }
                KeyCode::Enter => {
                    input_state.select_current_option();
                    if input_state.is_complete() {
                        submit_ask_response(app);
                    }
                }
                KeyCode::Esc => {
                    app.close_ask_modal();
                }
                _ => {}
            }
        }
        AskInputMode::CustomInput => {
            match code {
                KeyCode::Enter if has_shift => {
                    // Shift+Enter adds newline
                    input_state.insert_char('\n');
                }
                KeyCode::Enter => {
                    // Enter submits custom input
                    input_state.submit_custom_answer();
                    if input_state.is_complete() {
                        submit_ask_response(app);
                    }
                }
                KeyCode::Esc => {
                    input_state.cancel_custom_mode();
                }
                KeyCode::Left => {
                    input_state.move_cursor_left();
                }
                KeyCode::Right => {
                    input_state.move_cursor_right();
                }
                KeyCode::Backspace => {
                    input_state.delete_char();
                }
                KeyCode::Char(c) => {
                    input_state.insert_char(c);
                }
                _ => {}
            }
        }
    }
}

fn submit_ask_response(app: &mut App) {
    // Extract the ask modal state
    let modal_state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::AskModal(state) => state,
        other => {
            // Restore the state if it wasn't an ask modal
            app.modal_state = other;
            return;
        }
    };

    let response_text = modal_state.input_state.format_response();
    let message_id = modal_state.message_id;
    let ask_author_pubkey = modal_state.ask_author_pubkey;

    // Send reply to the ask event
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

fn handle_attachment_modal_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);

    match code {
        // Esc = close modal without saving
        KeyCode::Esc => {
            app.cancel_attachment_modal();
        }
        // Ctrl+S = save and close
        KeyCode::Char('s') if has_ctrl => {
            app.save_and_close_attachment_modal();
        }
        // Ctrl+D = delete attachment
        KeyCode::Char('d') if has_ctrl => {
            app.delete_attachment_and_close_modal();
        }
        // Enter = newline in modal
        KeyCode::Enter => {
            app.attachment_modal_editor_mut().insert_newline();
        }
        // Ctrl+A = move to beginning of line
        KeyCode::Char('a') if has_ctrl => {
            app.attachment_modal_editor_mut().move_to_line_start();
        }
        // Ctrl+E = move to end of line
        KeyCode::Char('e') if has_ctrl => {
            app.attachment_modal_editor_mut().move_to_line_end();
        }
        // Ctrl+K = kill to end of line
        KeyCode::Char('k') if has_ctrl => {
            app.attachment_modal_editor_mut().kill_to_line_end();
        }
        // Alt+Left = word left
        KeyCode::Left if has_alt => {
            app.attachment_modal_editor_mut().move_word_left();
        }
        // Alt+Right = word right
        KeyCode::Right if has_alt => {
            app.attachment_modal_editor_mut().move_word_right();
        }
        // Basic navigation
        KeyCode::Left => {
            app.attachment_modal_editor_mut().move_left();
        }
        KeyCode::Right => {
            app.attachment_modal_editor_mut().move_right();
        }
        KeyCode::Backspace => {
            app.attachment_modal_editor_mut().delete_char_before();
        }
        KeyCode::Delete => {
            app.attachment_modal_editor_mut().delete_char_at();
        }
        // Regular character input
        KeyCode::Char(c) => {
            app.attachment_modal_editor_mut().insert_char(c);
        }
        _ => {}
    }
}

/// Handle key events for the tab modal (Alt+/)
fn handle_tab_modal_key(app: &mut App, key: KeyEvent) {
    let code = key.code;

    match code {
        // Escape closes the modal
        KeyCode::Esc => {
            app.close_tab_modal();
        }
        // Up arrow moves selection up
        KeyCode::Up => {
            if app.tab_modal_index > 0 {
                app.tab_modal_index -= 1;
            }
        }
        // Down arrow moves selection down
        KeyCode::Down => {
            if app.tab_modal_index + 1 < app.open_tabs.len() {
                app.tab_modal_index += 1;
            }
        }
        // Enter switches to selected tab
        KeyCode::Enter => {
            let idx = app.tab_modal_index;
            app.close_tab_modal();
            if idx < app.open_tabs.len() {
                app.switch_to_tab(idx);
                app.view = View::Chat;
            }
        }
        // 'x' closes the selected tab
        KeyCode::Char('x') => {
            if !app.open_tabs.is_empty() {
                let idx = app.tab_modal_index;
                app.close_tab_at(idx);
                // If no more tabs, close the modal
                if app.open_tabs.is_empty() {
                    app.close_tab_modal();
                }
            }
        }
        // '1' goes to dashboard (home) - always first "tab"
        KeyCode::Char('1') => {
            app.close_tab_modal();
            app.save_chat_draft();
            app.view = View::Home;
        }
        // Number keys 2-9 switch directly to that tab (2 -> tab 0, 3 -> tab 1, etc.)
        KeyCode::Char(c) if c >= '2' && c <= '9' => {
            let tab_index = (c as usize) - ('2' as usize);
            app.close_tab_modal();
            if tab_index < app.open_tabs.len() {
                app.switch_to_tab(tab_index);
                app.view = View::Chat;
            }
        }
        _ => {}
    }
}

/// Handle key events for the search modal (/)
fn handle_search_modal_key(app: &mut App, key: KeyEvent) {
    let code = key.code;

    match code {
        // Escape closes the modal
        KeyCode::Esc => {
            app.showing_search_modal = false;
            app.search_filter.clear();
            app.search_index = 0;
        }
        // Up arrow moves selection up
        KeyCode::Up => {
            if app.search_index > 0 {
                app.search_index -= 1;
            }
        }
        // Down arrow moves selection down
        KeyCode::Down => {
            let count = app.search_results().len();
            if app.search_index + 1 < count {
                app.search_index += 1;
            }
        }
        // Enter opens the selected thread
        KeyCode::Enter => {
            let results = app.search_results();
            if let Some(result) = results.get(app.search_index).cloned() {
                app.showing_search_modal = false;
                app.search_filter.clear();
                app.search_index = 0;
                app.open_thread_from_home(&result.thread, &result.project_a_tag);
            }
        }
        // Character input appends to filter
        KeyCode::Char(c) => {
            app.search_filter.push(c);
            app.search_index = 0;
        }
        // Backspace removes last character from filter
        KeyCode::Backspace => {
            app.search_filter.pop();
            app.search_index = 0;
        }
        _ => {}
    }
}

/// Handle key events for the message actions modal
fn handle_message_actions_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::MessageAction;

    let code = key.code;

    // Get current state
    let (message_id, selected_index, has_trace) = match &app.modal_state {
        ModalState::MessageActions {
            message_id,
            selected_index,
            has_trace,
        } => (message_id.clone(), *selected_index, *has_trace),
        _ => return,
    };

    // Count available actions
    let action_count = if has_trace { 4 } else { 3 };

    match code {
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if selected_index > 0 {
                if let ModalState::MessageActions {
                    selected_index: ref mut idx,
                    ..
                } = app.modal_state
                {
                    *idx -= 1;
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if selected_index + 1 < action_count {
                if let ModalState::MessageActions {
                    selected_index: ref mut idx,
                    ..
                } = app.modal_state
                {
                    *idx += 1;
                }
            }
        }
        KeyCode::Enter => {
            // Execute selected action
            let actions: Vec<MessageAction> = MessageAction::ALL
                .iter()
                .filter(|a| has_trace || !matches!(a, MessageAction::OpenTrace))
                .copied()
                .collect();

            if let Some(action) = actions.get(selected_index) {
                app.execute_message_action(&message_id, *action);
            }
        }
        // Direct hotkeys
        KeyCode::Char('c') => {
            app.execute_message_action(&message_id, MessageAction::CopyRawEvent);
        }
        KeyCode::Char('s') => {
            app.execute_message_action(&message_id, MessageAction::SendAgain);
        }
        KeyCode::Char('v') => {
            app.execute_message_action(&message_id, MessageAction::ViewRawEvent);
        }
        KeyCode::Char('t') if has_trace => {
            app.execute_message_action(&message_id, MessageAction::OpenTrace);
        }
        _ => {}
    }
}

/// Handle key events for the conversation actions modal (Home view)
fn handle_conversation_actions_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::ConversationAction;

    let code = key.code;

    // Get current state
    let state = match &app.modal_state {
        ModalState::ConversationActions(s) => s.clone(),
        _ => return,
    };

    let action_count = ConversationAction::ALL.len();

    match code {
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
        // Direct hotkeys
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

/// Execute a conversation action
fn execute_conversation_action(
    app: &mut App,
    state: &ui::modal::ConversationActionsState,
    action: ui::modal::ConversationAction,
) {
    use ui::modal::ConversationAction;

    match action {
        ConversationAction::Open => {
            // Find and open the thread
            let thread = app.data_store.borrow().get_threads(&state.project_a_tag)
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

/// Export a thread as JSONL (one raw event per line)
fn export_thread_as_jsonl(app: &mut App, thread_id: &str) {
    use crate::store::get_raw_event_json;

    // Get all messages in the thread
    let messages = app.data_store.borrow().get_messages(thread_id).to_vec();

    if messages.is_empty() {
        app.set_status("No messages to export");
        return;
    }

    // Build JSONL content - one raw event per line
    let mut lines = Vec::new();

    // First, add the thread root event if available
    if let Some(json) = get_raw_event_json(&app.db.ndb, thread_id) {
        lines.push(json);
    }

    // Then add all message events
    for msg in &messages {
        if msg.id != thread_id {  // Skip if already added as root
            if let Some(json) = get_raw_event_json(&app.db.ndb, &msg.id) {
                lines.push(json);
            }
        }
    }

    let content = lines.join("\n");

    // Copy to clipboard
    use arboard::Clipboard;
    match Clipboard::new() {
        Ok(mut clipboard) => {
            if clipboard.set_text(&content).is_ok() {
                app.set_status(&format!("Exported {} events to clipboard as JSONL", lines.len()));
            } else {
                app.set_status("Failed to copy to clipboard");
            }
        }
        Err(_) => {
            app.set_status("Failed to access clipboard");
        }
    }
}

/// Handle key events for the chat actions modal (Ctrl+T /)
fn handle_chat_actions_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::ChatAction;

    let code = key.code;

    // Get current state
    let state = match &app.modal_state {
        ModalState::ChatActions(s) => s.clone(),
        _ => return,
    };

    let actions = state.available_actions();
    let action_count = actions.len();

    match code {
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
        // Direct hotkeys
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

/// Execute a chat action
fn execute_chat_action(
    app: &mut App,
    state: &ui::modal::ChatActionsState,
    action: ui::modal::ChatAction,
) {
    use ui::modal::ChatAction;

    match action {
        ChatAction::NewConversation => {
            // Start a new conversation keeping the same project, agent, and branch context
            // Create a draft tab so it persists in the tab bar
            let project_name = state.project_name.clone();
            let project_a_tag = state.project_a_tag.clone();

            app.modal_state = ModalState::None;
            app.save_chat_draft(); // Save current draft before switching

            // Create draft tab and switch to it
            let tab_idx = app.open_draft_tab(&project_a_tag, &project_name);
            app.switch_to_tab(tab_idx);

            app.chat_editor.clear();
            app.set_status("New conversation (same project, agent, and branch)");
        }
        ChatAction::GoToParent => {
            if let Some(ref parent_id) = state.parent_conversation_id {
                // Find the parent thread and navigate to it
                let parent_thread = app.data_store.borrow()
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

/// Handle key events for the project actions modal
fn handle_project_actions_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::ProjectAction;

    let code = key.code;

    // Get current state
    let state = match &app.modal_state {
        ModalState::ProjectActions(s) => s.clone(),
        _ => return,
    };

    let actions = state.available_actions();
    let action_count = actions.len();

    match code {
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
        // Direct hotkeys
        KeyCode::Char('n') if state.is_online => {
            execute_project_action(app, &state, ProjectAction::NewConversation);
        }
        KeyCode::Char('b') if !state.is_online => {
            execute_project_action(app, &state, ProjectAction::Boot);
        }
        KeyCode::Char('s') => {
            execute_project_action(app, &state, ProjectAction::Settings);
        }
        _ => {}
    }
}

/// Execute a project action
fn execute_project_action(
    app: &mut App,
    state: &ui::modal::ProjectActionsState,
    action: ui::modal::ProjectAction,
) {
    use ui::modal::ProjectAction;

    match action {
        ProjectAction::Boot => {
            // Send boot command
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
            // Open project settings
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
            // Find the project and set it as selected
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

                // Auto-select PM agent and default branch from status
                if let Some(status) = app.data_store.borrow().get_project_status(&a_tag) {
                    if let Some(pm) = status.pm_agent() {
                        app.selected_agent = Some(pm.clone());
                    }
                    if app.selected_branch.is_none() {
                        app.selected_branch = status.default_branch().map(String::from);
                    }
                }

                // Create draft tab and switch to it
                app.modal_state = ModalState::None;
                let tab_idx = app.open_draft_tab(&a_tag, &project_name);
                app.switch_to_tab(tab_idx);
                app.chat_editor.clear();
            } else {
                app.modal_state = ModalState::None;
                app.set_status("Project not found");
            }
        }
    }
}

/// Handle key events for the view raw event modal
fn handle_view_raw_event_modal_key(app: &mut App, key: KeyEvent) {
    let code = key.code;

    match code {
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
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

/// Handle key events for the command palette modal (Ctrl+T)
fn handle_command_palette_key(app: &mut App, key: KeyEvent) {
    if let ModalState::CommandPalette(ref mut state) = app.modal_state {
        let commands = state.available_commands();
        let cmd_count = commands.len();

        match key.code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                state.move_up();
            }
            KeyCode::Down | KeyCode::Char('j') => {
                state.move_down(cmd_count);
            }
            KeyCode::Enter => {
                // Execute selected command
                if let Some(cmd) = commands.get(state.selected_index) {
                    execute_palette_command(app, cmd.key);
                }
            }
            KeyCode::Backspace => {
                state.filter.pop();
                state.selected_index = 0;
            }
            KeyCode::Char(c) => {
                // Check if it's a direct shortcut key
                if state.filter.is_empty() {
                    // Try to find a command with this key
                    if commands.iter().any(|cmd| cmd.key == c) {
                        execute_palette_command(app, c);
                        return;
                    }
                }
                // Otherwise add to filter
                state.filter.push(c);
                state.selected_index = 0;
            }
            _ => {}
        }
    }
}

/// Execute a command from the palette by its key
fn execute_palette_command(app: &mut App, key: char) {
    // Close palette first
    app.modal_state = ModalState::None;

    match key {
        // Global commands
        '1' => {
            app.view = View::Home;
            app.input_mode = InputMode::Normal;
        }
        '/' => {
            app.showing_search_modal = true;
            app.search_filter.clear();
            app.search_index = 0;
        }
        '?' => {
            app.modal_state = ModalState::HotkeyHelp;
        }
        'q' => {
            app.quit();
        }
        'r' => {
            if let Some(core_handle) = app.core_handle.clone() {
                app.set_status("Syncing...");
                if let Err(e) = core_handle.send(NostrCommand::Sync) {
                    app.set_status(&format!("Sync request failed: {}", e));
                }
            }
        }

        // New conversation (context-dependent)
        'n' => {
            if app.view == View::Chat {
                // In Chat view: start new conversation keeping same project, agent, and branch
                if let Some(ref project) = app.selected_project {
                    let project_a_tag = project.a_tag();
                    let project_name = project.title.clone();

                    app.save_chat_draft(); // Save current draft before switching
                    let tab_idx = app.open_draft_tab(&project_a_tag, &project_name);
                    app.switch_to_tab(tab_idx);
                    app.chat_editor.clear();
                    app.set_status("New conversation (same project, agent, and branch)");
                }
            } else {
                // In Home/other views: open project selector
                app.open_projects_modal(true);
            }
        }
        'o' => {
            // Open selected (context-dependent)
            match app.view {
                View::Home => {
                    // Execute open for current home tab (recent threads)
                    let threads = app.recent_threads();
                    if let Some((thread, project_a_tag)) = threads.get(app.current_selection()) {
                        app.open_thread_from_home(thread, project_a_tag);
                    }
                }
                _ => {}
            }
        }
        'a' => {
            // Archive toggle (Home view - recent threads)
            if app.view == View::Home {
                let threads = app.recent_threads();
                if let Some((thread, _)) = threads.get(app.current_selection()) {
                    let thread_id = thread.id.clone();
                    let thread_title = thread.title.clone();
                    let is_now_archived = app.toggle_thread_archived(&thread_id);
                    let status = if is_now_archived {
                        format!("Archived: {}", thread_title)
                    } else {
                        format!("Unarchived: {}", thread_title)
                    };
                    app.set_status(&status);
                }
            }
        }
        'e' => {
            // Export JSONL
            if app.view == View::Chat {
                if let Some(thread) = &app.selected_thread {
                    export_thread_as_jsonl(app, &thread.id.clone());
                }
            }
        }
        'p' => {
            // Switch project
            app.open_projects_modal(false);
        }
        'm' => {
            // Toggle "by me" filter
            app.toggle_only_by_me();
        }
        'f' => {
            // Cycle time filter
            app.cycle_time_filter();
        }
        'A' => {
            // Agent browser
            app.open_agent_browser();
        }
        'N' => {
            // Create project
            app.modal_state = ModalState::CreateProject(ui::modal::CreateProjectState::new());
        }

        // Sidebar commands
        ' ' => {
            // Toggle project visibility
            toggle_project_visibility_palette(app);
        }
        's' => {
            // Settings
            open_project_settings(app);
        }
        'b' => {
            // Boot project
            boot_project(app);
        }

        // Chat commands
        '@' => {
            if !app.available_agents().is_empty() {
                app.open_agent_selector();
            }
        }
        '%' => {
            app.open_branch_selector();
        }
        'y' => {
            // Copy selected message
            copy_selected_message(app);
        }
        'v' => {
            // View raw event
            view_raw_event(app);
        }
        't' => {
            // Open trace
            open_trace(app);
        }
        '.' => {
            // Stop agent
            stop_agents(app);
        }
        'g' => {
            // Go to parent
            go_to_parent(app);
        }
        'x' => {
            // Close tab
            app.close_current_tab();
        }
        'T' => {
            // Toggle sidebar
            app.todo_sidebar_visible = !app.todo_sidebar_visible;
        }
        'S' => {
            // Agent settings - open settings modal for currently selected agent
            open_agent_settings(app);
        }
        'E' => {
            // Expand editor
            app.open_expanded_editor_modal();
        }

        // Agent browser commands
        'c' => {
            // Clone agent
            if app.view == View::AgentBrowser && app.agent_browser_in_detail {
                if let Some(agent_id) = &app.viewing_agent_id {
                    if let Some(agent) = app.data_store.borrow().get_agent_definition(agent_id) {
                        app.modal_state = ModalState::CreateAgent(
                            ui::modal::CreateAgentState::clone_from(&agent)
                        );
                    }
                }
            }
        }

        _ => {}
    }
}

// Helper functions for palette commands

fn toggle_project_visibility_palette(app: &mut App) {
    let (online, offline) = app.filtered_projects();
    let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
    if let Some(project) = all_projects.get(app.sidebar_project_index) {
        let a_tag = project.a_tag();
        if app.visible_projects.contains(&a_tag) {
            app.visible_projects.remove(&a_tag);
        } else {
            app.visible_projects.insert(a_tag);
        }
    }
}

fn open_project_settings(app: &mut App) {
    let (online, offline) = app.filtered_projects();
    let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
    if let Some(project) = all_projects.get(app.sidebar_project_index) {
        let a_tag = project.a_tag();
        let project_name = project.name.clone();
        // Get agent_ids from the Project struct
        let agent_ids = project.agent_ids.clone();
        app.modal_state = ModalState::ProjectSettings(
            ui::modal::ProjectSettingsState::new(a_tag, project_name, agent_ids)
        );
    }
}

fn boot_project(app: &mut App) {
    let (online, offline) = app.filtered_projects();
    let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
    if let Some(project) = all_projects.get(app.sidebar_project_index) {
        if let Some(core_handle) = app.core_handle.clone() {
            let _ = core_handle.send(NostrCommand::BootProject {
                project_a_tag: project.a_tag(),
                project_pubkey: Some(project.pubkey.clone()),
            });
        }
    }
}

fn copy_selected_message(app: &mut App) {
    use crate::store::get_raw_event_json;
    let messages = app.messages();
    if let Some(msg) = messages.get(app.selected_message_index) {
        if let Some(json) = get_raw_event_json(&app.db.ndb, &msg.id) {
            if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(&json)) {
                app.set_status(&format!("Failed to copy: {}", e));
            } else {
                app.set_status("Raw event copied to clipboard");
            }
        }
    }
}

fn view_raw_event(app: &mut App) {
    use crate::store::get_raw_event_json;
    let messages = app.messages();
    if let Some(msg) = messages.get(app.selected_message_index) {
        if let Some(json) = get_raw_event_json(&app.db.ndb, &msg.id) {
            // Pretty print the JSON
            let pretty_json = if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
                serde_json::to_string_pretty(&value).unwrap_or(json)
            } else {
                json
            };
            app.modal_state = ModalState::ViewRawEvent {
                message_id: msg.id.clone(),
                json: pretty_json,
                scroll_offset: 0,
            };
        }
    }
}

fn open_trace(app: &mut App) {
    use crate::store::get_trace_context;
    let messages = app.messages();
    if let Some(msg) = messages.get(app.selected_message_index) {
        if let Some(trace_ctx) = get_trace_context(&app.db.ndb, &msg.id) {
            let url = format!("http://localhost:16686/trace/{}", trace_ctx.trace_id);
            #[cfg(target_os = "macos")]
            let _ = std::process::Command::new("open").arg(&url).spawn();
            #[cfg(target_os = "linux")]
            let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
        }
    }
}

fn stop_agents(app: &mut App) {
    if let Some(stop_thread_id) = app.get_stop_target_thread_id() {
        let (is_busy, project_a_tag) = {
            let store = app.data_store.borrow();
            let is_busy = store.is_event_busy(&stop_thread_id);
            let project_a_tag = store.find_project_for_thread(&stop_thread_id);
            (is_busy, project_a_tag)
        };
        if is_busy {
            if let (Some(core_handle), Some(a_tag)) = (app.core_handle.clone(), project_a_tag) {
                let working_agents = app.data_store.borrow().get_working_agents(&stop_thread_id);
                if let Err(e) = core_handle.send(NostrCommand::StopOperations {
                    project_a_tag: a_tag,
                    event_ids: vec![stop_thread_id.clone()],
                    agent_pubkeys: working_agents,
                }) {
                    app.set_status(&format!("Failed to stop: {}", e));
                } else {
                    app.set_status("Stop command sent");
                }
            }
        }
    }
}

fn go_to_parent(app: &mut App) {
    if let Some(thread) = &app.selected_thread {
        if let Some(parent_id) = &thread.parent_conversation_id {
            // Get project for current thread first
            let project_a_tag = app.data_store.borrow().find_project_for_thread(&thread.id);
            if let Some(a_tag) = project_a_tag {
                // Find parent thread in same project
                let parent_thread = app.data_store.borrow()
                    .get_threads(&a_tag)
                    .iter()
                    .find(|t| t.id == *parent_id)
                    .cloned();
                if let Some(parent) = parent_thread {
                    app.open_thread_from_home(&parent, &a_tag);
                }
            }
        }
    }
}

fn open_agent_settings(app: &mut App) {
    // Need selected agent and selected project to open settings
    let agent = match &app.selected_agent {
        Some(a) => a.clone(),
        None => {
            app.set_status("No agent selected");
            return;
        }
    };

    let project = match &app.selected_project {
        Some(p) => p.clone(),
        None => {
            app.set_status("No project selected");
            return;
        }
    };

    // Get all available tools and models from the project status
    let (all_tools, all_models) = app.data_store.borrow()
        .get_project_status(&project.a_tag())
        .map(|status| {
            let tools = status.tools().iter().map(|s| s.to_string()).collect();
            let models = status.models().iter().map(|s| s.to_string()).collect();
            (tools, models)
        })
        .unwrap_or_default();

    let settings_state = ui::modal::AgentSettingsState::new(
        agent.name.clone(),
        agent.pubkey.clone(),
        project.a_tag(),
        agent.model.clone(),
        agent.tools.clone(),
        all_models,
        all_tools,
    );
    app.modal_state = ModalState::AgentSettings(settings_state);
}

/// Handle key events for the hotkey help modal
fn handle_hotkey_help_modal_key(app: &mut App, key: KeyEvent) {
    // Any key closes the modal
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::Char('q') => {
            app.modal_state = ModalState::None;
        }
        _ => {
            app.modal_state = ModalState::None;
        }
    }
}

/// Handle key events for the nudge selector modal
fn handle_nudge_selector_key(app: &mut App, key: KeyEvent) {
    let nudges = app.filtered_nudges();
    let item_count = nudges.len();

    if let ModalState::NudgeSelector(ref mut state) = app.modal_state {
        match key.code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
            }
            KeyCode::Enter => {
                // Confirm selection - copy selected nudge ids to app
                let selected_ids = state.selected_nudge_ids.clone();
                app.selected_nudge_ids = selected_ids;
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
                // Toggle selection of current item
                if let Some(nudge) = nudges.get(state.selector.index) {
                    let nudge_id = nudge.id.clone();
                    if let Some(pos) = state.selected_nudge_ids.iter().position(|id| id == &nudge_id) {
                        state.selected_nudge_ids.remove(pos);
                    } else {
                        state.selected_nudge_ids.push(nudge_id);
                    }
                }
            }
            KeyCode::Char(c) => {
                // Add to filter
                state.selector.filter.push(c);
                state.selector.index = 0;
            }
            KeyCode::Backspace => {
                // Remove from filter
                state.selector.filter.pop();
                state.selector.index = 0;
            }
            _ => {}
        }
    }
}

/// Handle key events for the report viewer modal
fn handle_report_viewer_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::{ReportViewerFocus, ReportViewMode, ReportCopyOption};

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
                let versions = app.data_store.borrow().get_report_versions(&slug).into_iter().cloned().collect::<Vec<_>>();
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
                    let versions = app.data_store.borrow().get_report_versions(&slug).into_iter().cloned().collect::<Vec<_>>();
                    if let Some(v) = versions.get(state.version_index) {
                        state.report = v.clone();
                        state.content_scroll = 0;
                    }
                }
            }
            KeyCode::Up | KeyCode::Char('k') => {
                match state.focus {
                    ReportViewerFocus::Content => {
                        state.content_scroll = state.content_scroll.saturating_sub(1);
                    }
                    ReportViewerFocus::Threads => {
                        if state.selected_thread_index > 0 {
                            state.selected_thread_index -= 1;
                        }
                    }
                }
            }
            KeyCode::Down | KeyCode::Char('j') => {
                match state.focus {
                    ReportViewerFocus::Content => {
                        state.content_scroll += 1;
                    }
                    ReportViewerFocus::Threads => {
                        state.selected_thread_index += 1;
                    }
                }
            }
            KeyCode::Enter => {
                if state.show_copy_menu {
                    use nostr_sdk::prelude::{EventId, ToBech32};
                    use crate::store::get_raw_event_json;

                    let option = ReportCopyOption::ALL[state.copy_menu_index];
                    let text = match option {
                        ReportCopyOption::Bech32Id => {
                            EventId::from_hex(&state.report.id)
                                .ok()
                                .and_then(|id| id.to_bech32().ok())
                                .unwrap_or_else(|| state.report.id.clone())
                        }
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
                        ReportCopyOption::Markdown => {
                            state.report.content.clone()
                        }
                    };
                    state.show_copy_menu = false;
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        let _ = clipboard.set_text(&text);
                    }
                } else if state.focus == ReportViewerFocus::Threads {
                    // Open selected thread from document threads
                    let a_tag = state.report.a_tag();
                    let project_a_tag = state.report.project_a_tag.clone();
                    let threads = app.data_store.borrow().get_document_threads(&a_tag).to_vec();
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

/// Handle key events for the agent settings modal
fn handle_agent_settings_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::AgentSettingsFocus;

    if let ModalState::AgentSettings(ref mut state) = app.modal_state {
        match key.code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
            }
            KeyCode::Tab => {
                // Toggle focus between model and tools
                state.focus = match state.focus {
                    AgentSettingsFocus::Model => AgentSettingsFocus::Tools,
                    AgentSettingsFocus::Tools => AgentSettingsFocus::Model,
                };
            }
            KeyCode::Up => {
                match state.focus {
                    AgentSettingsFocus::Model => {
                        if state.model_index > 0 {
                            state.model_index -= 1;
                        }
                    }
                    AgentSettingsFocus::Tools => {
                        state.move_cursor_up();
                    }
                }
            }
            KeyCode::Down => {
                match state.focus {
                    AgentSettingsFocus::Model => {
                        if state.model_index < state.available_models.len().saturating_sub(1) {
                            state.model_index += 1;
                        }
                    }
                    AgentSettingsFocus::Tools => {
                        state.move_cursor_down();
                    }
                }
            }
            KeyCode::Char(' ') => {
                // Toggle tool/group at cursor when in tools focus
                if state.focus == AgentSettingsFocus::Tools {
                    state.toggle_at_cursor();
                }
            }
            KeyCode::Char('a') => {
                // Bulk toggle all tools in the current group
                if state.focus == AgentSettingsFocus::Tools {
                    state.toggle_group_all();
                }
            }
            KeyCode::Enter => {
                // Publish the config update
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
