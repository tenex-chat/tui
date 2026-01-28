//! View-specific keyboard event handlers.
//!
//! Each main view (Home, Chat, etc.) has its own handler function.

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use tenex_core::models::OperationsStatus;

use crate::models::Message;
use crate::nostr::NostrCommand;
use crate::ui;
use crate::ui::hotkeys::{resolve_hotkey, HotkeyContext, HotkeyId};
use crate::ui::selector::{handle_selector_key, SelectorAction};
use crate::ui::views::chat::{group_messages, DisplayItem};
use crate::ui::views::home::get_hierarchical_threads;
use crate::ui::{App, HomeTab, InputMode, ModalState, View};

/// Cached ActiveWork operations to avoid snapshot drift during key event handling.
/// This ensures consistent state when navigating, selecting, or opening items.
struct ActiveWorkCache {
    operations: Vec<OperationsStatus>,
}

impl ActiveWorkCache {
    fn new(app: &App) -> Self {
        let operations = app.data_store.borrow()
            .get_all_active_operations()
            .into_iter()
            .cloned()
            .collect();
        Self { operations }
    }

    fn len(&self) -> usize {
        self.operations.len()
    }

    fn get(&self, index: usize) -> Option<&OperationsStatus> {
        self.operations.get(index)
    }
}

// =============================================================================
// HOME VIEW
// =============================================================================

/// Get thread ID at a given index for the current home tab.
/// For ActiveWork tab, uses the provided cache to avoid snapshot drift.
fn get_thread_id_at_index(app: &App, index: usize, active_work_cache: Option<&ActiveWorkCache>) -> Option<String> {
    match app.home_panel_focus {
        HomeTab::Conversations => {
            let threads = get_hierarchical_threads(app);
            threads.get(index).map(|h| h.thread.id.clone())
        }
        HomeTab::Inbox => {
            let items = app.inbox_items();
            items.get(index).and_then(|item| item.thread_id.clone())
        }
        HomeTab::Feed => {
            let items = app.feed_items();
            items.get(index).map(|item| item.thread_id.clone())
        }
        HomeTab::ActiveWork => {
            // Use provided cache to avoid snapshot drift, or fetch fresh if not provided
            if let Some(cache) = active_work_cache {
                if let Some(op) = cache.get(index) {
                    // First try thread_id if present
                    if let Some(ref thread_id) = op.thread_id {
                        return Some(thread_id.clone());
                    }
                    // Fall back to looking up thread from event_id (like renderer does)
                    let data_store = app.data_store.borrow();
                    if let Some((thread_id, _title)) = data_store.get_thread_info_for_event(&op.event_id) {
                        return Some(thread_id);
                    }
                }
            } else {
                // Fallback: fetch fresh (should not happen if caller provides cache)
                let data_store = app.data_store.borrow();
                let operations = data_store.get_all_active_operations();
                if let Some(op) = operations.get(index) {
                    if let Some(ref thread_id) = op.thread_id {
                        return Some(thread_id.clone());
                    }
                    if let Some((thread_id, _title)) = data_store.get_thread_info_for_event(&op.event_id) {
                        return Some(thread_id);
                    }
                }
            }
            None
        }
        HomeTab::Reports => None,      // Reports are not threads
        HomeTab::Stats => None,        // Stats are not threads
    }
}

pub(super) fn handle_home_view_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    // Cache ActiveWork operations once per key event to avoid snapshot drift
    // during navigation, selection, and opening operations
    let active_work_cache = if app.home_panel_focus == HomeTab::ActiveWork {
        Some(ActiveWorkCache::new(app))
    } else {
        None
    };

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

    // Resolve hotkey using centralized registry
    let context = HotkeyContext::from_app_state(
        &app.view,
        &app.input_mode,
        &app.modal_state,
        &app.home_panel_focus,
        app.sidebar_focused,
    );

    // Try hotkey resolution first for standard actions
    if let Some(hotkey_id) = resolve_hotkey(code, modifiers, context) {
        match hotkey_id {
            HotkeyId::Quit => {
                app.quit();
                return Ok(());
            }
            HotkeyId::SearchReports => {
                app.input_mode = InputMode::Editing;
                return Ok(());
            }
            HotkeyId::SwitchProject => {
                app.open_projects_modal(false);
                return Ok(());
            }
            HotkeyId::TimeFilter => {
                app.cycle_time_filter();
                return Ok(());
            }
            HotkeyId::AgentBrowser => {
                app.open_agent_browser();
                return Ok(());
            }
            HotkeyId::CreateProject => {
                app.modal_state =
                    ui::modal::ModalState::CreateProject(ui::modal::CreateProjectState::new());
                return Ok(());
            }
            HotkeyId::NewConversation => {
                new_conversation_current_project(app);
                return Ok(());
            }
            HotkeyId::NewConversationWithPicker => {
                app.open_projects_modal(true);
                return Ok(());
            }
            HotkeyId::ShowHideArchived => {
                app.toggle_show_archived();
                return Ok(());
            }
            HotkeyId::ToggleHideScheduled if !app.sidebar_focused => {
                app.toggle_hide_scheduled();
                return Ok(());
            }
            HotkeyId::NextHomeTab => {
                app.home_panel_focus = match app.home_panel_focus {
                    HomeTab::Conversations => HomeTab::Inbox,
                    HomeTab::Inbox => HomeTab::Reports,
                    HomeTab::Reports => HomeTab::Feed,
                    HomeTab::Feed => HomeTab::ActiveWork,
                    HomeTab::ActiveWork => HomeTab::Stats,
                    HomeTab::Stats => HomeTab::Conversations,
                };
                return Ok(());
            }
            HotkeyId::PrevHomeTab => {
                app.home_panel_focus = match app.home_panel_focus {
                    HomeTab::Conversations => HomeTab::Stats,
                    HomeTab::Inbox => HomeTab::Conversations,
                    HomeTab::Reports => HomeTab::Inbox,
                    HomeTab::Feed => HomeTab::Reports,
                    HomeTab::ActiveWork => HomeTab::Feed,
                    HomeTab::Stats => HomeTab::ActiveWork,
                };
                return Ok(());
            }
            HotkeyId::FocusSidebar => {
                // On Stats tab, Right switches subtabs; otherwise focuses sidebar
                if app.home_panel_focus == HomeTab::Stats {
                    app.stats_subtab = app.stats_subtab.next();
                } else {
                    app.sidebar_focused = true;
                }
                return Ok(());
            }
            HotkeyId::UnfocusSidebar => {
                app.sidebar_focused = false;
                return Ok(());
            }
            // Other hotkeys handled below or not applicable to Home view
            _ => {}
        }
    }

    // Handle special cases not covered by hotkey registry
    match code {
        // Vim-style h/l navigation for Stats subtabs
        KeyCode::Char('h') if app.home_panel_focus == HomeTab::Stats => {
            app.stats_subtab = app.stats_subtab.prev();
        }
        KeyCode::Char('l') if app.home_panel_focus == HomeTab::Stats => {
            app.stats_subtab = app.stats_subtab.next();
        }
        // Left arrow on Stats tab switches subtabs (not sidebar focus)
        KeyCode::Left if app.home_panel_focus == HomeTab::Stats => {
            app.stats_subtab = app.stats_subtab.prev();
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
                    if let Some(thread_id) = get_thread_id_at_index(app, current, active_work_cache.as_ref()) {
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
                        if let Some(thread_id) = get_thread_id_at_index(app, current - 1, active_work_cache.as_ref()) {
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
                    HomeTab::Feed => app.feed_items().len().saturating_sub(1),
                    HomeTab::ActiveWork => active_work_cache.as_ref().map_or(0, |c| c.len().saturating_sub(1)),
                    HomeTab::Stats => 0, // Stats tab has no list selection
                };
                // If Shift is held, add current item to multi-selection before moving
                if has_shift {
                    if let Some(thread_id) = get_thread_id_at_index(app, current, active_work_cache.as_ref()) {
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
                        if let Some(thread_id) = get_thread_id_at_index(app, current + 1, active_work_cache.as_ref()) {
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
                app.toggle_project_visibility(&a_tag);

                // Clamp selection to valid range after filtering change
                let current = app.current_selection();
                let max = match app.home_panel_focus {
                    HomeTab::Inbox => app.inbox_items().len().saturating_sub(1),
                    HomeTab::Conversations => get_hierarchical_threads(app).len().saturating_sub(1),
                    HomeTab::Reports => app.reports().len().saturating_sub(1),
                    HomeTab::Feed => app.feed_items().len().saturating_sub(1),
                    HomeTab::ActiveWork => active_work_cache.as_ref().map_or(0, |c| c.len().saturating_sub(1)),
                    HomeTab::Stats => 0, // Stats tab has no list selection
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
                            app.set_warning_status(&format!("Failed to stop: {}", e));
                        } else {
                            app.set_warning_status("Stop command sent for all project operations");
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
                            app.set_warning_status(&format!("Failed to boot: {}", e));
                        } else {
                            app.set_warning_status(&format!("Boot request sent for {}", project.name));
                        }
                    }
                }
            } else {
                app.set_warning_status("Project is already online");
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
                    HomeTab::Feed => {
                        let items = app.feed_items();
                        if let Some(item) = items.get(idx) {
                            // Find the thread and open it
                            let thread = app.data_store.borrow()
                                .get_threads(&item.project_a_tag)
                                .iter()
                                .find(|t| t.id == item.thread_id)
                                .cloned();

                            if let Some(thread) = thread {
                                let project_a_tag = item.project_a_tag.clone();
                                app.open_thread_from_home(&thread, &project_a_tag);
                            }
                        }
                    }
                    HomeTab::ActiveWork => {
                        // Open conversation from Active Work tab using cached operations
                        let (event_id, thread_id_opt, project_a_tag): (String, Option<String>, String) = active_work_cache.as_ref()
                            .and_then(|cache| cache.get(idx))
                            .map(|op| (op.event_id.clone(), op.thread_id.clone(), op.project_coordinate.clone()))
                            .unwrap_or_default();

                        if project_a_tag.is_empty() {
                            return Ok(());
                        }

                        // Try thread_id first, then fall back to event lookup (like renderer does)
                        let resolved_thread_id: Option<String> = thread_id_opt.or_else(|| {
                            app.data_store.borrow()
                                .get_thread_info_for_event(&event_id)
                                .map(|(thread_id, _)| thread_id)
                        });

                        if let Some(thread_id) = resolved_thread_id {
                            let thread = app.data_store.borrow()
                                .get_threads(&project_a_tag)
                                .iter()
                                .find(|t| t.id == thread_id)
                                .cloned();

                            if let Some(thread) = thread {
                                app.open_thread_from_home(&thread, &project_a_tag);
                            } else {
                                app.set_warning_status("Could not find conversation thread");
                            }
                        } else {
                            app.set_warning_status("No conversation linked to this operation");
                        }
                    }
                    HomeTab::Stats => {
                        // Stats tab has no selectable items to open
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
                app.set_warning_status("All threads collapsed");
            } else {
                app.set_warning_status("All threads expanded");
            }
        }
        // Vim-style navigation (j/k) with Shift support for multi-select
        KeyCode::Char('k') | KeyCode::Char('K') if !app.sidebar_focused => {
            let current = app.current_selection();
            if has_shift {
                if let Some(thread_id) = get_thread_id_at_index(app, current, active_work_cache.as_ref()) {
                    app.add_thread_to_multi_select(&thread_id);
                }
            } else {
                app.clear_multi_selection();
            }
            if current > 0 {
                app.set_current_selection(current - 1);
                if has_shift {
                    if let Some(thread_id) = get_thread_id_at_index(app, current - 1, active_work_cache.as_ref()) {
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
                HomeTab::Feed => app.feed_items().len().saturating_sub(1),
                HomeTab::ActiveWork => active_work_cache.as_ref().map_or(0, |c| c.len().saturating_sub(1)),
                HomeTab::Stats => 0, // Stats tab has no list selection
            };
            if has_shift {
                if let Some(thread_id) = get_thread_id_at_index(app, current, active_work_cache.as_ref()) {
                    app.add_thread_to_multi_select(&thread_id);
                }
            } else {
                app.clear_multi_selection();
            }
            if current < max {
                app.set_current_selection(current + 1);
                if has_shift {
                    if let Some(thread_id) = get_thread_id_at_index(app, current + 1, active_work_cache.as_ref()) {
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
                if let Some(thread_id) = get_thread_id_at_index(app, current, active_work_cache.as_ref()) {
                    let is_archived = app.toggle_thread_archived(&thread_id);
                    if is_archived {
                        app.set_warning_status("Archived conversation");
                    } else {
                        app.set_warning_status("Unarchived conversation");
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

/// Helper function to start a new conversation in the current project
fn new_conversation_current_project(app: &mut App) {
    // Get the first visible project's info
    let hierarchy = get_hierarchical_threads(app);
    let project_info = if let Some(first_item) = hierarchy.first() {
        let store = app.data_store.borrow();
        store
            .get_projects()
            .iter()
            .find(|p| p.a_tag() == first_item.a_tag)
            .map(|p| (p.a_tag(), p.name.clone(), p.clone()))
    } else {
        None
    };

    if let Some((a_tag, name, project)) = project_info {
        app.selected_project = Some(project);

        // Auto-select PM agent and default branch from status
        // Extract values before making mutable calls to avoid borrow issues
        let (pm_agent, default_branch) = {
            let store = app.data_store.borrow();
            if let Some(status) = store.get_project_status(&a_tag) {
                (status.pm_agent().cloned(), status.default_branch().map(String::from))
            } else {
                (None, None)
            }
        };
        if let Some(pm) = pm_agent {
            app.set_selected_agent(Some(pm));
        }
        if app.selected_branch.is_none() {
            app.selected_branch = default_branch;
        }

        let tab_idx = app.open_draft_tab(&a_tag, &name);
        app.switch_to_tab(tab_idx);
        app.view = View::Chat;
        app.chat_editor_mut().clear();
    } else {
        app.set_warning_status("No project available for new conversation");
    }
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
                let needs_agent = for_new_thread || app.selected_agent().is_none();
                let needs_branch = app.selected_branch.is_none();
                app.selected_project = Some(project);

                // Auto-select PM agent and default branch from status
                // Extract values before making mutable calls to avoid borrow issues
                let (pm_agent, default_branch) = {
                    let store = app.data_store.borrow();
                    if let Some(status) = store.get_project_status(&a_tag) {
                        let pm = if needs_agent { status.pm_agent().cloned() } else { None };
                        let branch = if needs_branch { status.default_branch().map(String::from) } else { None };
                        (pm, branch)
                    } else {
                        (None, None)
                    }
                };
                if let Some(pm) = pm_agent {
                    app.set_selected_agent(Some(pm));
                }
                if let Some(branch) = default_branch {
                    app.selected_branch = Some(branch);
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
                    // Clear workspace - we're now in manual mode showing only this project
                    if app.preferences.borrow().active_workspace_id().is_some() {
                        app.preferences.borrow_mut().set_active_workspace(None);
                    }
                    app.visible_projects.clear();
                    app.visible_projects.insert(a_tag);
                    app.save_selected_projects();
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
                            app.set_warning_status(&format!("Failed to update agents: {}", e));
                        } else {
                            app.set_warning_status("Project agents updated");
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
                            app.set_warning_status(&format!("Failed to create project: {}", e));
                        } else {
                            app.set_warning_status("Project created");
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

    // Handle sidebar-focused state first
    if app.is_sidebar_focused() {
        match code {
            // Escape or 'h' unfocuses sidebar
            KeyCode::Esc | KeyCode::Char('h') => {
                app.set_sidebar_focused(false);
                return Ok(true);
            }
            // Up/k = move selection up
            KeyCode::Up | KeyCode::Char('k') => {
                app.sidebar_move_up();
                return Ok(true);
            }
            // Down/j = move selection down
            KeyCode::Down | KeyCode::Char('j') => {
                app.sidebar_move_down();
                return Ok(true);
            }
            // Enter = activate selected item
            KeyCode::Enter => {
                if let Some(selection) = app.sidebar_activate() {
                    use crate::ui::components::SidebarSelection;
                    match selection {
                        SidebarSelection::Delegation(thread_id) => {
                            // Navigate to the delegated conversation
                            app.push_delegation(&thread_id);
                        }
                        SidebarSelection::Report(a_tag) => {
                            // Open the report viewer using shared coordinate parser
                            use crate::ui::components::ReportCoordinate;
                            if let Some(coord) = ReportCoordinate::parse(&a_tag) {
                                let report = app.data_store.borrow().get_report(&coord.slug).cloned();
                                if let Some(report) = report {
                                    use crate::ui::modal::{ModalState, ReportViewerState};
                                    app.modal_state = ModalState::ReportViewer(ReportViewerState::new(report));
                                }
                            }
                        }
                    }
                    app.set_sidebar_focused(false);
                }
                return Ok(true);
            }
            // Tab = unfocus sidebar (go back to message panel)
            KeyCode::Tab => {
                app.set_sidebar_focused(false);
                return Ok(true);
            }
            _ => {}
        }
    }

    // Resolve hotkey using centralized registry
    let context = HotkeyContext::from_app_state(
        &app.view,
        &app.input_mode,
        &app.modal_state,
        &app.home_panel_focus,
        app.is_sidebar_focused(),
    );

    // Try hotkey resolution for standard actions
    if let Some(hotkey_id) = resolve_hotkey(code, modifiers, context) {
        match hotkey_id {
            HotkeyId::AgentBrowser => {
                app.open_agent_browser();
                return Ok(true);
            }
            HotkeyId::CreateProject => {
                app.modal_state =
                    ui::modal::ModalState::CreateProject(ui::modal::CreateProjectState::new());
                return Ok(true);
            }
            HotkeyId::NewConversation => {
                // New conversation with same project/agent/branch
                if let Some(ref project) = app.selected_project {
                    let project_a_tag = project.a_tag();
                    let project_name = project.name.clone();
                    let inherited_agent = app.selected_agent().cloned();
                    let inherited_branch = app.selected_branch.clone();

                    app.save_chat_draft();
                    let tab_idx = app.open_draft_tab(&project_a_tag, &project_name);
                    app.switch_to_tab(tab_idx);

                    app.set_selected_agent(inherited_agent);
                    app.selected_branch = inherited_branch;
                    app.chat_editor_mut().clear();
                    app.set_warning_status("New conversation (same project, agent, and branch)");
                }
                return Ok(true);
            }
            HotkeyId::NewConversationWithPicker => {
                app.open_projects_modal(true);
                return Ok(true);
            }
            HotkeyId::ShowHideArchived => {
                app.toggle_show_archived();
                return Ok(true);
            }
            HotkeyId::SelectBranchAlt => {
                app.open_branch_selector();
                return Ok(true);
            }
            HotkeyId::GoToHome => {
                app.go_home();
                return Ok(true);
            }
            HotkeyId::CloseTab => {
                app.close_current_tab();
                return Ok(true);
            }
            HotkeyId::InConversationSearch => {
                app.enter_chat_search();
                return Ok(true);
            }
            // Other hotkeys not handled here
            _ => {}
        }
    }

    // Handle special cases not covered by hotkey registry
    match code {
        // Number keys 2-9 for tab navigation
        KeyCode::Char(c) if c >= '2' && c <= '9' => {
            let tab_index = (c as usize) - ('2' as usize);
            if tab_index < app.open_tabs().len() {
                app.switch_to_tab(tab_index);
            }
            return Ok(true);
        }
        // Tab key: if sidebar has items, toggle focus; otherwise cycle tabs
        KeyCode::Tab => {
            if app.sidebar_state.has_items() && app.todo_sidebar_visible {
                app.toggle_sidebar_focus();
            } else if has_shift {
                app.prev_tab();
            } else {
                app.next_tab();
            }
            return Ok(true);
        }
        // 'l' = focus sidebar (vim-style right motion)
        KeyCode::Char('l') if app.sidebar_state.has_items() && app.todo_sidebar_visible => {
            app.set_sidebar_focused(true);
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
            if app.view == View::AgentBrowser && !app.home.in_agent_detail() {
                app.home.backspace_filter();
            }
        }
        KeyCode::Up => match app.view {
            View::Chat => {
                // Simple navigation - expanded groups are flattened so each item is selectable
                if app.selected_message_index() > 0 {
                    app.set_selected_message_index(app.selected_message_index() - 1);
                }
            }
            View::LessonViewer => {
                app.scroll_up(3);
            }
            View::AgentBrowser => {
                if app.home.in_agent_detail() {
                    app.scroll_up(3);
                } else {
                    app.home.select_prev_agent();
                }
            }
            _ => {}
        },
        KeyCode::Down => match app.view {
            View::LessonViewer => {
                app.scroll_down(3);
            }
            View::AgentBrowser => {
                if app.home.in_agent_detail() {
                    app.scroll_down(3);
                } else {
                    let count = app.filtered_agent_definitions().len();
                    app.home.select_next_agent(count);
                }
            }
            View::Chat => {
                // Simple navigation - expanded groups are flattened so each item is selectable
                let count = app.display_item_count();
                if app.selected_message_index() < count.saturating_sub(1) {
                    app.set_selected_message_index(app.selected_message_index() + 1);
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
                if !app.home.in_agent_detail() {
                    let agents = app.filtered_agent_definitions();
                    if let Some(agent) = agents.get(app.home.agent_browser_index) {
                        app.home.enter_agent_detail(agent.id.clone());
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
                app.go_home();
                app.viewing_lesson_id = None;
                app.lesson_viewer_section = 0;
                app.scroll_offset = 0;
            }
            View::AgentBrowser => {
                if app.home.in_agent_detail() {
                    app.home.exit_agent_detail();
                    app.scroll_offset = 0;
                } else {
                    app.go_home();
                    app.home.clear_agent_filter();
                    app.home.set_agent_index(0);
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
                        app.set_warning_status(&format!("Failed to stop: {}", e));
                    } else {
                        app.set_warning_status("Stop command sent");
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
    } else if c == 'j' && app.view == View::AgentBrowser && app.home.in_agent_detail() {
        app.scroll_down(3);
    } else if c == 'k' && app.view == View::AgentBrowser && app.home.in_agent_detail() {
        app.scroll_up(3);
    } else if c == 'f' && app.view == View::AgentBrowser && app.home.in_agent_detail() {
        if let Some(ref agent_id) = app.home.viewing_agent_id.clone() {
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
    } else if c == 'c' && app.view == View::AgentBrowser && app.home.in_agent_detail() {
        if let Some(ref agent_id) = app.home.viewing_agent_id.clone() {
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
    } else if c == 'n' && app.view == View::AgentBrowser && !app.home.in_agent_detail() {
        app.modal_state =
            ui::modal::ModalState::CreateAgent(ui::modal::CreateAgentState::new());
    } else if app.view == View::AgentBrowser && !app.home.in_agent_detail() && c != 'q' && c != 'n'
    {
        app.home.append_to_filter(c);
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
    let thread_id = app.selected_thread().map(|t| t.id.as_str());
    let subthread_root = app.subthread_root().cloned();

    let display_messages: Vec<&Message> = if let Some(ref root_id) = subthread_root {
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

    if let Some(item) = grouped.get(app.selected_message_index()) {
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
                            app.set_warning_status("Invalid nsec format");
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
                                        response_tx: None,
                                    }) {
                                        app.set_warning_status(&format!("Failed to connect: {}", e));
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
                                app.set_warning_status(&format!("Login failed: {}", e));
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
                                        response_tx: None,
                                    }) {
                                        app.set_warning_status(&format!("Failed to connect: {}", e));
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
                                app.set_warning_status(&format!(
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
