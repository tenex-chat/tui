//! View-specific keyboard event handlers.
//!
//! Each main view (Home, Chat, etc.) has its own handler function.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::models::Message;
use crate::nostr::NostrCommand;
use crate::ui;
use crate::ui::selector::{handle_selector_key, SelectorAction};
use crate::ui::views::chat::{group_messages, DisplayItem};
use crate::ui::views::home::get_hierarchical_threads;
use crate::ui::{App, HomeTab, InputMode, ModalState, View};

// =============================================================================
// HOME VIEW
// =============================================================================

/// Get thread ID at a given index for the current home tab
fn get_thread_id_at_index(app: &App, index: usize) -> Option<String> {
    match app.home_panel_focus {
        HomeTab::Conversations => {
            let threads = get_hierarchical_threads(app);
            threads.get(index).map(|h| h.thread.id.clone())
        }
        HomeTab::Inbox => {
            let items = app.inbox_items();
            items.get(index).and_then(|item| item.thread_id.clone())
        }
        HomeTab::Status => {
            let items = app.status_threads();
            items.get(index).map(|(thread, _)| thread.id.clone())
        }
        HomeTab::Search => {
            let items = app.search_results();
            items.get(index).map(|result| result.thread.id.clone())
        }
        HomeTab::Reports => None, // Reports are not threads
    }
}

pub(super) fn handle_home_view_key(app: &mut App, key: KeyEvent) -> Result<()> {
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

    // Handle Search tab input mode
    if app.input_mode == InputMode::Editing && app.home_panel_focus == HomeTab::Search {
        match code {
            KeyCode::Char(c) => {
                app.search_filter.push(c);
                app.tab_selection.insert(HomeTab::Search, 0);
            }
            KeyCode::Backspace => {
                app.search_filter.pop();
                app.tab_selection.insert(HomeTab::Search, 0);
            }
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
            }
            KeyCode::Enter => {
                app.input_mode = InputMode::Normal;
                let results = app.search_results();
                let idx = app.current_selection();
                if let Some(result) = results.get(idx).cloned() {
                    app.open_thread_from_home(&result.thread, &result.project_a_tag);
                }
            }
            KeyCode::Up => {
                let current = app.current_selection();
                if current > 0 {
                    app.set_current_selection(current - 1);
                }
            }
            KeyCode::Down => {
                let current = app.current_selection();
                let max = app.search_results().len().saturating_sub(1);
                if current < max {
                    app.set_current_selection(current + 1);
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // Handle projects modal when showing (using ModalState)
    if matches!(app.modal_state, ModalState::ProjectsModal { .. }) {
        handle_projects_modal_key(app, key)?;
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
            if app.home_panel_focus == HomeTab::Reports || app.home_panel_focus == HomeTab::Search {
                app.input_mode = InputMode::Editing;
            }
        }
        KeyCode::Char('p') => {
            app.open_projects_modal(false);
        }
        KeyCode::Char('f') => {
            app.cycle_time_filter();
        }
        KeyCode::Char('A') => {
            app.open_agent_browser();
        }
        KeyCode::Char('N') if has_shift => {
            app.modal_state =
                ui::modal::ModalState::CreateProject(ui::modal::CreateProjectState::new());
        }
        KeyCode::Char('H') if has_shift => {
            // Show/hide archived items based on focus
            if app.sidebar_focused {
                app.toggle_show_archived_projects();
            } else {
                app.toggle_show_archived();
            }
        }
        KeyCode::Char('P') if has_shift => {
            // Show/hide archived projects from main panel
            if !app.sidebar_focused {
                app.toggle_show_archived_projects();
            }
        }
        KeyCode::Tab => {
            app.home_panel_focus = match app.home_panel_focus {
                HomeTab::Conversations => HomeTab::Inbox,
                HomeTab::Inbox => HomeTab::Reports,
                HomeTab::Reports => HomeTab::Status,
                HomeTab::Status => HomeTab::Search,
                HomeTab::Search => HomeTab::Conversations,
            };
        }
        KeyCode::BackTab if has_shift => {
            app.home_panel_focus = match app.home_panel_focus {
                HomeTab::Conversations => HomeTab::Search,
                HomeTab::Inbox => HomeTab::Conversations,
                HomeTab::Reports => HomeTab::Inbox,
                HomeTab::Status => HomeTab::Reports,
                HomeTab::Search => HomeTab::Status,
            };
        }
        KeyCode::Right => {
            app.sidebar_focused = true;
        }
        KeyCode::Left => {
            app.sidebar_focused = false;
        }
        KeyCode::Up => {
            if app.sidebar_focused {
                if app.sidebar_project_index > 0 {
                    app.sidebar_project_index -= 1;
                }
            } else {
                let current = app.current_selection();
                // If Shift is held, add current item to multi-selection before moving
                if has_shift {
                    if let Some(thread_id) = get_thread_id_at_index(app, current) {
                        app.add_thread_to_multi_select(&thread_id);
                    }
                } else {
                    // Clear multi-selection when navigating without Shift
                    app.clear_multi_selection();
                }
                if current > 0 {
                    app.set_current_selection(current - 1);
                    // Also add the new position to selection when Shift is held
                    if has_shift {
                        if let Some(thread_id) = get_thread_id_at_index(app, current - 1) {
                            app.add_thread_to_multi_select(&thread_id);
                        }
                    }
                }
            }
        }
        KeyCode::Down => {
            if app.sidebar_focused {
                let (online, offline) = app.filtered_projects();
                let max = (online.len() + offline.len()).saturating_sub(1);
                if app.sidebar_project_index < max {
                    app.sidebar_project_index += 1;
                }
            } else {
                let current = app.current_selection();
                let max = match app.home_panel_focus {
                    HomeTab::Inbox => app.inbox_items().len().saturating_sub(1),
                    HomeTab::Conversations => get_hierarchical_threads(app).len().saturating_sub(1),
                    HomeTab::Reports => app.reports().len().saturating_sub(1),
                    HomeTab::Status => app.status_threads().len().saturating_sub(1),
                    HomeTab::Search => app.search_results().len().saturating_sub(1),
                };
                // If Shift is held, add current item to multi-selection before moving
                if has_shift {
                    if let Some(thread_id) = get_thread_id_at_index(app, current) {
                        app.add_thread_to_multi_select(&thread_id);
                    }
                } else {
                    // Clear multi-selection when navigating without Shift
                    app.clear_multi_selection();
                }
                if current < max {
                    app.set_current_selection(current + 1);
                    // Also add the new position to selection when Shift is held
                    if has_shift {
                        if let Some(thread_id) = get_thread_id_at_index(app, current + 1) {
                            app.add_thread_to_multi_select(&thread_id);
                        }
                    }
                }
            }
        }
        KeyCode::Char(' ') if app.sidebar_focused => {
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

                // Clamp selection to valid range after filtering change
                let current = app.current_selection();
                let max = match app.home_panel_focus {
                    HomeTab::Inbox => app.inbox_items().len().saturating_sub(1),
                    HomeTab::Conversations => get_hierarchical_threads(app).len().saturating_sub(1),
                    HomeTab::Reports => app.reports().len().saturating_sub(1),
                    HomeTab::Status => app.status_threads().len().saturating_sub(1),
                    HomeTab::Search => app.search_results().len().saturating_sub(1),
                };
                if current > max {
                    app.set_current_selection(max);
                }
            }
        }
        KeyCode::Char('s') if app.sidebar_focused => {
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                let a_tag = project.a_tag();
                let project_name = project.name.clone();
                let agent_ids = project.agent_ids.clone();

                app.modal_state = ui::modal::ModalState::ProjectSettings(
                    ui::modal::ProjectSettingsState::new(a_tag, project_name, agent_ids),
                );
            }
        }
        KeyCode::Char('S') if app.sidebar_focused && has_shift => {
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
            let (online, offline) = app.filtered_projects();
            let online_count = online.len();
            if app.sidebar_project_index >= online_count {
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
                let (online, offline) = app.filtered_projects();
                let online_count = online.len();
                let is_online = app.sidebar_project_index < online_count;
                let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
                if let Some(project) = all_projects.get(app.sidebar_project_index) {
                    let a_tag = project.a_tag();
                    let is_archived = app.is_project_archived(&a_tag);
                    app.modal_state = ui::modal::ModalState::ProjectActions(
                        ui::modal::ProjectActionsState::new(
                            a_tag,
                            project.name.clone(),
                            project.pubkey.clone(),
                            is_online,
                            is_archived,
                        ),
                    );
                }
            } else {
                let idx = app.current_selection();
                match app.home_panel_focus {
                    HomeTab::Inbox => {
                        let items = app.inbox_items();
                        if let Some(item) = items.get(idx) {
                            let item_id = item.id.clone();
                            app.data_store.borrow_mut().mark_inbox_read(&item_id);

                            if let Some(ref thread_id) = item.thread_id {
                                let project_a_tag = item.project_a_tag.clone();
                                let thread = app
                                    .data_store
                                    .borrow()
                                    .get_threads(&project_a_tag)
                                    .iter()
                                    .find(|t| t.id == *thread_id)
                                    .cloned();

                                if let Some(thread) = thread {
                                    app.open_thread_from_home(&thread, &project_a_tag);
                                }
                            }
                        }
                    }
                    HomeTab::Conversations => {
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
                                ui::modal::ReportViewerState::new(report.clone()),
                            );
                        }
                    }
                    HomeTab::Status => {
                        let status_items = app.status_threads();
                        if let Some((thread, a_tag)) = status_items.get(idx) {
                            app.open_thread_from_home(thread, a_tag);
                        }
                    }
                    HomeTab::Search => {
                        let results = app.search_results();
                        if let Some(result) = results.get(idx).cloned() {
                            app.open_thread_from_home(&result.thread, &result.project_a_tag);
                        }
                    }
                }
            }
        }
        KeyCode::Char('r') if app.home_panel_focus == HomeTab::Inbox => {
            let items = app.inbox_items();
            if let Some(item) = items.get(app.current_selection()) {
                let item_id = item.id.clone();
                app.data_store.borrow_mut().mark_inbox_read(&item_id);
            }
        }
        KeyCode::Char(' ') if app.home_panel_focus == HomeTab::Conversations => {
            let hierarchy = get_hierarchical_threads(app);
            if let Some(item) = hierarchy.get(app.current_selection()) {
                if item.has_children {
                    app.toggle_thread_collapse(&item.thread.id);
                }
            }
        }
        KeyCode::Char('c') if app.home_panel_focus == HomeTab::Conversations => {
            // Toggle collapse/expand all threads
            let now_collapsed = app.toggle_collapse_all_threads();
            if now_collapsed {
                app.set_status("All threads collapsed");
            } else {
                app.set_status("All threads expanded");
            }
        }
        // Vim-style navigation (j/k) with Shift support for multi-select
        KeyCode::Char('k') | KeyCode::Char('K') if !app.sidebar_focused => {
            let current = app.current_selection();
            if has_shift {
                if let Some(thread_id) = get_thread_id_at_index(app, current) {
                    app.add_thread_to_multi_select(&thread_id);
                }
            } else {
                app.clear_multi_selection();
            }
            if current > 0 {
                app.set_current_selection(current - 1);
                if has_shift {
                    if let Some(thread_id) = get_thread_id_at_index(app, current - 1) {
                        app.add_thread_to_multi_select(&thread_id);
                    }
                }
            }
        }
        KeyCode::Char('j') | KeyCode::Char('J') if !app.sidebar_focused => {
            let current = app.current_selection();
            let max = match app.home_panel_focus {
                HomeTab::Inbox => app.inbox_items().len().saturating_sub(1),
                HomeTab::Conversations => get_hierarchical_threads(app).len().saturating_sub(1),
                HomeTab::Reports => app.reports().len().saturating_sub(1),
                HomeTab::Status => app.status_threads().len().saturating_sub(1),
                HomeTab::Search => app.search_results().len().saturating_sub(1),
            };
            if has_shift {
                if let Some(thread_id) = get_thread_id_at_index(app, current) {
                    app.add_thread_to_multi_select(&thread_id);
                }
            } else {
                app.clear_multi_selection();
            }
            if current < max {
                app.set_current_selection(current + 1);
                if has_shift {
                    if let Some(thread_id) = get_thread_id_at_index(app, current + 1) {
                        app.add_thread_to_multi_select(&thread_id);
                    }
                }
            }
        }
        // Archive selected conversations ('a')
        KeyCode::Char('a') if !app.sidebar_focused && app.home_panel_focus != HomeTab::Reports => {
            if !app.multi_selected_threads.is_empty() {
                // Archive all multi-selected
                app.archive_multi_selected();
            } else {
                // Archive just the current selection
                let current = app.current_selection();
                if let Some(thread_id) = get_thread_id_at_index(app, current) {
                    let is_archived = app.toggle_thread_archived(&thread_id);
                    if is_archived {
                        app.set_status("Archived conversation");
                    } else {
                        app.set_status("Unarchived conversation");
                    }
                }
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
            let tab_index = (c as usize) - ('2' as usize);
            if tab_index < app.open_tabs().len() {
                app.switch_to_tab(tab_index);
                app.view = View::Chat;
            }
        }
        _ => {}
    }
    Ok(())
}

fn handle_projects_modal_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let (online_projects, offline_projects) = app.filtered_projects();
    let all_projects: Vec<_> = online_projects
        .into_iter()
        .chain(offline_projects)
        .collect();
    let item_count = all_projects.len();
    let for_new_thread = matches!(
        app.modal_state,
        ModalState::ProjectsModal {
            for_new_thread: true,
            ..
        }
    );

    if let ModalState::ProjectsModal {
        ref mut selector, ..
    } = app.modal_state
    {
        match handle_selector_key(selector, key, item_count, |idx| all_projects.get(idx).cloned()) {
            SelectorAction::Selected(project) => {
                let a_tag = project.a_tag();
                app.selected_project = Some(project);

                // Auto-select PM agent and default branch from status
                if let Some(status) = app.data_store.borrow().get_project_status(&a_tag) {
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
                    let project_name = app
                        .selected_project
                        .as_ref()
                        .map(|p| p.name.clone())
                        .unwrap_or_else(|| "New".to_string());
                    let tab_idx = app.open_draft_tab(&a_tag, &project_name);
                    app.switch_to_tab(tab_idx);
                    app.chat_editor_mut().clear();
                } else {
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
    Ok(())
}

fn handle_project_settings_key(app: &mut App, key: KeyEvent) {
    use ui::views::{available_agent_count, get_agent_id_at_index};

    let code = key.code;

    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::ProjectSettings(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    if state.in_add_mode {
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
        match code {
            KeyCode::Esc => {
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
                    if state.selector_index >= state.pending_agent_ids.len()
                        && state.selector_index > 0
                    {
                        state.selector_index -= 1;
                    }
                }
            }
            KeyCode::Char('p') => {
                if !state.pending_agent_ids.is_empty() && state.selector_index > 0 {
                    state.set_pm(state.selector_index);
                    state.selector_index = 0;
                }
            }
            KeyCode::Enter => {
                if state.has_changes() {
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

    app.modal_state = ModalState::ProjectSettings(state);
}

fn handle_create_project_key(app: &mut App, key: KeyEvent) {
    use ui::modal::{CreateProjectFocus, CreateProjectStep};

    let code = key.code;

    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::CreateProject(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    match state.step {
        CreateProjectStep::Details => match code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
                return;
            }
            KeyCode::Tab => {
                state.focus = match state.focus {
                    CreateProjectFocus::Name => CreateProjectFocus::Description,
                    CreateProjectFocus::Description => CreateProjectFocus::Name,
                };
            }
            KeyCode::Enter => {
                if state.can_proceed() {
                    state.step = CreateProjectStep::SelectAgents;
                }
            }
            KeyCode::Char(c) => match state.focus {
                CreateProjectFocus::Name => state.name.push(c),
                CreateProjectFocus::Description => state.description.push(c),
            },
            KeyCode::Backspace => match state.focus {
                CreateProjectFocus::Name => {
                    state.name.pop();
                }
                CreateProjectFocus::Description => {
                    state.description.pop();
                }
            },
            _ => {}
        },
        CreateProjectStep::SelectAgents => {
            let filtered_agents = app.agent_definitions_filtered_by(&state.agent_selector.filter);
            let item_count = filtered_agents.len();

            match code {
                KeyCode::Esc => {
                    app.modal_state = ModalState::None;
                    return;
                }
                KeyCode::Backspace if state.agent_selector.filter.is_empty() => {
                    state.step = CreateProjectStep::Details;
                }
                KeyCode::Backspace => {
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
                    if let Some(agent) = filtered_agents.get(state.agent_selector.index) {
                        state.toggle_agent(agent.id.clone());
                    }
                }
                KeyCode::Enter => {
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
                    state.agent_selector.filter.push(c);
                    state.agent_selector.index = 0;
                }
                _ => {}
            }
        }
    }

    app.modal_state = ModalState::CreateProject(state);
}

// =============================================================================
// CHAT VIEW (Normal Mode)
// =============================================================================

pub(super) fn handle_chat_normal_mode(app: &mut App, key: KeyEvent) -> Result<bool> {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);
    let has_alt = modifiers.contains(KeyModifiers::ALT);

    match code {
        // Alt+B = open branch selector
        KeyCode::Char('b') if has_alt => {
            app.open_branch_selector();
            return Ok(true);
        }
        // Number keys 1-9 to navigate (1 = Home, 2-9 = tabs) in Normal mode
        KeyCode::Char('1') => {
            app.save_chat_draft();
            app.view = View::Home;
            return Ok(true);
        }
        KeyCode::Char(c) if c >= '2' && c <= '9' => {
            let tab_index = (c as usize) - ('2' as usize);
            if tab_index < app.open_tabs().len() {
                app.switch_to_tab(tab_index);
            }
            return Ok(true);
        }
        // Tab key cycles through tabs (Shift+Tab = prev, Tab = next)
        KeyCode::Tab => {
            if has_shift {
                app.prev_tab();
            } else {
                app.next_tab();
            }
            return Ok(true);
        }
        // x closes current tab
        KeyCode::Char('x') => {
            app.close_current_tab();
            return Ok(true);
        }
        // Ctrl+F = enter in-conversation search
        KeyCode::Char('f') if modifiers.contains(KeyModifiers::CONTROL) => {
            app.enter_chat_search();
            return Ok(true);
        }
        _ => {}
    }

    Ok(false)
}

// =============================================================================
// NORMAL MODE (non-Chat views)
// =============================================================================

pub(super) fn handle_normal_mode(
    app: &mut App,
    key: KeyEvent,
    _login_step: &mut crate::ui::views::login::LoginStep,
    _pending_nsec: &mut Option<String>,
) -> Result<()> {
    let code = key.code;

    match code {
        KeyCode::Char('q') => {
            app.quit();
        }
        KeyCode::Char(c) => {
            handle_normal_mode_char(app, c)?;
        }
        KeyCode::Backspace => {
            if app.view == View::AgentBrowser && !app.agent_browser_in_detail {
                app.agent_browser_filter.pop();
                app.agent_browser_index = 0;
            }
        }
        KeyCode::Up => match app.view {
            View::Chat => {
                // Simple navigation - expanded groups are flattened so each item is selectable
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
                // Simple navigation - expanded groups are flattened so each item is selectable
                let count = app.display_item_count();
                if app.selected_message_index < count.saturating_sub(1) {
                    app.selected_message_index += 1;
                } else {
                    // At last message, focus the input
                    app.input_mode = InputMode::Editing;
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
                handle_chat_enter(app)?;
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
                    // First priority: exit subthread view
                    app.exit_subthread();
                } else if app.has_navigation_stack() {
                    // Second priority: pop navigation stack (return to parent delegation)
                    app.pop_navigation_stack();
                } else {
                    // Third priority: close tab and go to Home
                    app.close_current_tab();
                }
            }
            View::LessonViewer => {
                app.view = View::Home;
                app.viewing_lesson_id = None;
                app.lesson_viewer_section = 0;
                app.scroll_offset = 0;
            }
            View::AgentBrowser => {
                if app.agent_browser_in_detail {
                    app.agent_browser_in_detail = false;
                    app.viewing_agent_id = None;
                    app.scroll_offset = 0;
                } else {
                    app.view = View::Home;
                    app.agent_browser_filter.clear();
                    app.agent_browser_index = 0;
                }
            }
            _ => {}
        },
        _ => {}
    }

    Ok(())
}

fn handle_normal_mode_char(app: &mut App, c: char) -> Result<()> {
    if c == 'a' && app.view == View::Chat && !app.available_agents().is_empty() {
        app.open_agent_selector();
    } else if c == '@' && app.view == View::Chat && !app.available_agents().is_empty() {
        app.open_agent_selector();
    } else if c == '.' && app.view == View::Chat {
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
        if let Some(ref agent_id) = app.viewing_agent_id.clone() {
            if let Some(agent) = app
                .all_agent_definitions()
                .iter()
                .find(|a| a.id == *agent_id)
                .cloned()
            {
                app.modal_state = ui::modal::ModalState::CreateAgent(
                    ui::modal::CreateAgentState::fork_from(&agent),
                );
            }
        }
    } else if c == 'c' && app.view == View::AgentBrowser && app.agent_browser_in_detail {
        if let Some(ref agent_id) = app.viewing_agent_id.clone() {
            if let Some(agent) = app
                .all_agent_definitions()
                .iter()
                .find(|a| a.id == *agent_id)
                .cloned()
            {
                app.modal_state = ui::modal::ModalState::CreateAgent(
                    ui::modal::CreateAgentState::clone_from(&agent),
                );
            }
        }
    } else if c == 'n' && app.view == View::AgentBrowser && !app.agent_browser_in_detail {
        app.modal_state =
            ui::modal::ModalState::CreateAgent(ui::modal::CreateAgentState::new());
    } else if app.view == View::AgentBrowser && !app.agent_browser_in_detail && c != 'q' && c != 'n'
    {
        app.agent_browser_filter.push(c);
        app.agent_browser_index = 0;
    } else if c >= '1' && c <= '5' && app.view == View::LessonViewer {
        let section_index = (c as usize) - ('1' as usize);
        if let Some(ref lesson_id) = app.viewing_lesson_id {
            if let Some(lesson) = app.data_store.borrow().get_lesson(lesson_id) {
                if section_index < lesson.sections().len() {
                    app.lesson_viewer_section = section_index;
                    app.scroll_offset = 0;
                }
            }
        }
    }

    Ok(())
}

fn handle_chat_enter(app: &mut App) -> Result<()> {
    let messages = app.messages();
    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());

    let display_messages: Vec<&Message> = if let Some(ref root_id) = app.subthread_root {
        messages
            .iter()
            .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
            .collect()
    } else {
        messages
            .iter()
            .filter(|m| {
                Some(m.id.as_str()) == thread_id
                    || m.reply_to.is_none()
                    || m.reply_to.as_deref() == thread_id
            })
            .collect()
    };

    let grouped = group_messages(&display_messages);

    if let Some(item) = grouped.get(app.selected_message_index) {
        match item {
            DisplayItem::SingleMessage { message: msg, .. } => {
                let has_replies = messages.iter().any(|m| {
                    m.reply_to.as_deref() == Some(msg.id.as_str())
                        && Some(msg.id.as_str()) != thread_id
                });
                if has_replies {
                    app.enter_subthread((*msg).clone());
                }
            }
            DisplayItem::DelegationPreview { thread_id, .. } => {
                // Push onto navigation stack instead of opening a new tab
                app.push_delegation(thread_id);
            }
        }
    }

    Ok(())
}

// =============================================================================
// EDITING MODE (non-Chat views, e.g., Login)
// =============================================================================

pub(super) fn handle_editing_mode(
    app: &mut App,
    key: KeyEvent,
    login_step: &mut crate::ui::views::login::LoginStep,
    pending_nsec: &mut Option<String>,
) -> Result<()> {
    use crate::nostr;
    use crate::ui::views::login::LoginStep;

    let code = key.code;

    match code {
        KeyCode::Esc => {
            // Login view always stays in editing mode
            if app.view != View::Login {
                app.input_mode = InputMode::Normal;
            }
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

            if app.view == View::Login {
                // Keep input focused on login screen
                match login_step {
                    LoginStep::Nsec => {
                        if input.is_empty()
                            && nostr::has_stored_credentials(&app.preferences.borrow())
                        {
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
                            nostr::load_stored_keys(&input, &app.preferences.borrow())
                        } else if let Some(ref nsec) = pending_nsec {
                            let password = if input.is_empty() {
                                None
                            } else {
                                Some(input.as_str())
                            };
                            nostr::auth::login_with_nsec(nsec, password, &mut app.preferences.borrow_mut())
                        } else {
                            Err(anyhow::anyhow!("No credentials provided"))
                        };

                        match keys_result {
                            Ok(keys) => {
                                let user_pubkey = nostr::get_current_pubkey(&keys);
                                app.keys = Some(keys.clone());
                                app.data_store
                                    .borrow_mut()
                                    .set_user_pubkey(user_pubkey.clone());

                                if let Some(ref core_handle) = app.core_handle {
                                    if let Err(e) = core_handle.send(NostrCommand::Connect {
                                        keys: keys.clone(),
                                        user_pubkey: user_pubkey.clone(),
                                    }) {
                                        app.set_status(&format!("Failed to connect: {}", e));
                                        *login_step = LoginStep::Nsec;
                                    } else {
                                        app.view = View::Home;
                                        app.load_filter_preferences();
                                        app.init_trusted_backends();
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
                        let keys_result =
                            nostr::load_stored_keys(&input, &app.preferences.borrow());

                        match keys_result {
                            Ok(keys) => {
                                let user_pubkey = nostr::get_current_pubkey(&keys);
                                app.keys = Some(keys.clone());
                                app.data_store
                                    .borrow_mut()
                                    .set_user_pubkey(user_pubkey.clone());

                                if let Some(ref core_handle) = app.core_handle {
                                    if let Err(e) = core_handle.send(NostrCommand::Connect {
                                        keys: keys.clone(),
                                        user_pubkey: user_pubkey.clone(),
                                    }) {
                                        app.set_status(&format!("Failed to connect: {}", e));
                                        *login_step = LoginStep::Unlock;
                                    } else {
                                        app.view = View::Home;
                                        app.load_filter_preferences();
                                        app.init_trusted_backends();
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
                }
            }
        }
        _ => {}
    }

    Ok(())
}
