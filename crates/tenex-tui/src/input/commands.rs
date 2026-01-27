//! Command definitions for the command palette.
//!
//! Each command is defined once with its key, label, section, availability condition,
//! and execution function. No duplication between display and execution.

use crate::models::Message;
use crate::nostr::NostrCommand;
use crate::store::{get_raw_event_json, get_trace_context};
use crate::ui::views::chat::{group_messages, DisplayItem};
use crate::ui::views::home::get_hierarchical_threads;
use crate::ui::{modal, App, HomeTab, InputMode, ModalState, UndoAction, View};

use super::modal_handlers::export_thread_as_jsonl;

/// A command available in the palette.
pub struct Command {
    pub key: char,
    pub label: &'static str,
    pub section: &'static str,
    pub available: fn(&App) -> bool,
    pub execute: fn(&mut App),
}

/// All commands in the system. Each command is defined exactly once.
pub static COMMANDS: &[Command] = &[
    // =========================================================================
    // GLOBAL - Always available
    // =========================================================================
    Command {
        key: '1',
        label: "Go to Home",
        section: "Navigation",
        available: |_| true,
        execute: |app| {
            app.go_home();
            app.input_mode = InputMode::Normal;
        },
    },
    Command {
        key: '?',
        label: "Help",
        section: "Navigation",
        available: |_| true,
        execute: |app| {
            app.modal_state = ModalState::HotkeyHelp;
        },
    },
    Command {
        key: 'D',
        label: "Debug stats",
        section: "System",
        available: |_| true,
        execute: |app| {
            app.modal_state = ModalState::DebugStats(modal::DebugStatsState::new());
        },
    },
    Command {
        key: 'q',
        label: "Quit",
        section: "System",
        available: |_| true,
        execute: |app| {
            app.quit();
        },
    },
    // =========================================================================
    // HOME VIEW - Conversations/Inbox/Status tabs (main panel, not sidebar)
    // =========================================================================
    // NOTE: 'n' is reserved as a fallback for "next tab" in command palette
    // Use 'N' (Shift+N) for new conversation instead
    Command {
        key: 'o',
        label: "Open selected",
        section: "Conversation",
        available: |app| {
            app.view == View::Home
                && !app.sidebar_focused
                && matches!(app.home_panel_focus, HomeTab::Conversations | HomeTab::Status)
        },
        execute: |app| {
            let threads = app.recent_threads();
            if let Some((thread, project_a_tag)) = threads.get(app.current_selection()) {
                app.open_thread_from_home(thread, project_a_tag);
            }
        },
    },
    Command {
        key: 'o',
        label: "Open selected",
        section: "Inbox",
        available: |app| {
            app.view == View::Home && !app.sidebar_focused && app.home_panel_focus == HomeTab::Inbox
        },
        execute: |app| {
            // Inbox open is handled by the view handler, but we can trigger it here
            let items = app.inbox_items();
            if let Some(item) = items.get(app.current_selection()) {
                if let Some(ref thread_id) = item.thread_id {
                    let store = app.data_store.borrow();
                    if let Some(thread) = store
                        .get_threads(&item.project_a_tag)
                        .iter()
                        .find(|t| t.id == *thread_id)
                    {
                        let thread = thread.clone();
                        let a_tag = item.project_a_tag.clone();
                        drop(store);
                        app.open_thread_from_home(&thread, &a_tag);
                    }
                }
            }
        },
    },
    Command {
        key: 'o',
        label: "View report",
        section: "Reports",
        available: |app| {
            app.view == View::Home
                && !app.sidebar_focused
                && app.home_panel_focus == HomeTab::Reports
        },
        execute: |app| {
            let reports = app.reports();
            if let Some(report) = reports.get(app.current_selection()) {
                app.modal_state = ModalState::ReportViewer(modal::ReportViewerState::new(
                    report.clone(),
                ));
            }
        },
    },
    Command {
        key: 'a',
        label: "Archive/Unarchive",
        section: "Conversation",
        available: |app| {
            (app.view == View::Home
                && !app.sidebar_focused
                && matches!(
                    app.home_panel_focus,
                    HomeTab::Conversations | HomeTab::Inbox | HomeTab::Status
                ))
                || app.view == View::Chat
        },
        execute: archive_toggle,
    },
    Command {
        key: 'e',
        label: "Export JSONL",
        section: "Conversation",
        available: |app| {
            (app.view == View::Home
                && !app.sidebar_focused
                && matches!(app.home_panel_focus, HomeTab::Conversations | HomeTab::Status))
                || app.view == View::Chat
        },
        execute: |app| {
            if app.view == View::Chat {
                if let Some(thread) = &app.selected_thread {
                    export_thread_as_jsonl(app, &thread.id.clone());
                }
            } else if app.view == View::Home {
                let threads = app.recent_threads();
                if let Some((thread, _)) = threads.get(app.current_selection()) {
                    export_thread_as_jsonl(app, &thread.id.clone());
                }
            }
        },
    },
    // NOTE: 'p' is reserved as a fallback for "prev tab" in command palette
    // Switch project is available via direct 'p' key in Home view (not via command palette)
    Command {
        key: 'f',
        label: "Time filter",
        section: "Filter",
        available: |app| {
            app.view == View::Home
                && !app.sidebar_focused
                && app.home_panel_focus == HomeTab::Conversations
        },
        execute: |app| {
            app.cycle_time_filter();
        },
    },
    Command {
        key: 'H',
        label: "Show archived conversations",
        section: "Filter",
        available: |app| {
            app.view == View::Home
                && !app.sidebar_focused
                && matches!(app.home_panel_focus, HomeTab::Conversations | HomeTab::Inbox)
                && !app.show_archived
        },
        execute: |app| {
            app.toggle_show_archived();
        },
    },
    Command {
        key: 'H',
        label: "Hide archived conversations",
        section: "Filter",
        available: |app| {
            app.view == View::Home
                && !app.sidebar_focused
                && matches!(app.home_panel_focus, HomeTab::Conversations | HomeTab::Inbox)
                && app.show_archived
        },
        execute: |app| {
            app.toggle_show_archived();
        },
    },
    Command {
        key: 'A',
        label: "Show archived projects",
        section: "Filter",
        available: |app| {
            app.view == View::Home && !app.sidebar_focused && !app.show_archived_projects
        },
        execute: |app| {
            app.toggle_show_archived_projects();
        },
    },
    Command {
        key: 'A',
        label: "Hide archived projects",
        section: "Filter",
        available: |app| {
            app.view == View::Home && !app.sidebar_focused && app.show_archived_projects
        },
        execute: |app| {
            app.toggle_show_archived_projects();
        },
    },
    Command {
        key: 'R',
        label: "Mark as read",
        section: "Inbox",
        available: |app| {
            app.view == View::Home && !app.sidebar_focused && app.home_panel_focus == HomeTab::Inbox
        },
        execute: |app| {
            let items = app.inbox_items();
            if let Some(item) = items.get(app.current_selection()) {
                let item_id = item.id.clone();
                app.data_store.borrow_mut().mark_inbox_read(&item_id);
            }
        },
    },
    Command {
        key: 'M',
        label: "Mark all read",
        section: "Inbox",
        available: |app| {
            app.view == View::Home && !app.sidebar_focused && app.home_panel_focus == HomeTab::Inbox
        },
        execute: |app| {
            let items = app.inbox_items();
            for item in items {
                app.data_store.borrow_mut().mark_inbox_read(&item.id);
            }
        },
    },
    Command {
        key: '/',
        label: "Search",
        section: "Filter",
        available: |app| {
            app.view == View::Home
                && !app.sidebar_focused
                && matches!(app.home_panel_focus, HomeTab::Conversations | HomeTab::Reports)
        },
        execute: |app| {
            app.toggle_sidebar_search();
        },
    },
    Command {
        key: 'B',
        label: "Agent Browser",
        section: "Other",
        available: |app| app.view == View::Home && !app.sidebar_focused,
        execute: |app| {
            app.open_agent_browser();
        },
    },
    Command {
        key: 'U',
        label: "Nudge Manager",
        section: "Other",
        available: |app| app.view == View::Home && !app.sidebar_focused,
        execute: |app| {
            app.modal_state = ModalState::NudgeList(modal::NudgeListState::new());
        },
    },
    Command {
        key: 'C',
        label: "Create project",
        section: "Other",
        available: |app| app.view == View::Home && !app.sidebar_focused,
        execute: |app| {
            app.modal_state = ModalState::CreateProject(modal::CreateProjectState::new());
        },
    },
    Command {
        key: 'N',
        label: "New conversation (current project)",
        section: "Conversation",
        available: |app| {
            // Available in Home view (Conversations tab, not sidebar focused)
            (app.view == View::Home
                && !app.sidebar_focused
                && app.home_panel_focus == HomeTab::Conversations)
            // Also available in Chat view
            || app.view == View::Chat
        },
        execute: |app| {
            if app.view == View::Chat {
                // Chat view: new conversation with same project/agent/branch
                if let Some(ref project) = app.selected_project {
                    let project_a_tag = project.a_tag();
                    let project_name = project.name.clone();
                    let inherited_agent = app.selected_agent.clone();
                    let inherited_branch = app.selected_branch.clone();

                    app.save_chat_draft();
                    let tab_idx = app.open_draft_tab(&project_a_tag, &project_name);
                    app.switch_to_tab(tab_idx);

                    app.selected_agent = inherited_agent;
                    app.selected_branch = inherited_branch;
                    app.chat_editor_mut().clear();
                    app.set_status("New conversation (same project, agent, and branch)");
                }
            } else {
                // Home view: use the existing new_conversation_current_project function
                new_conversation_current_project(app);
            }
        },
    },
    Command {
        key: 'P',
        label: "New conversation on project...",
        section: "Conversation",
        available: |app| {
            // Available in Home view (Conversations tab, not sidebar focused)
            (app.view == View::Home
                && !app.sidebar_focused
                && app.home_panel_focus == HomeTab::Conversations)
            // Also available in Chat view
            || app.view == View::Chat
        },
        execute: |app| {
            app.open_projects_modal(true);
        },
    },
    // =========================================================================
    // HOME VIEW - Sidebar (projects list)
    // =========================================================================
    Command {
        key: ' ',
        label: "Toggle visibility",
        section: "Project",
        available: |app| app.view == View::Home && app.sidebar_focused,
        execute: toggle_project_visibility,
    },
    // NOTE: 'n' is reserved as a fallback for "next tab" in command palette
    // New conversation in sidebar is available via direct 'n' key (not via command palette)
    Command {
        key: 's',
        label: "Settings",
        section: "Project",
        available: |app| app.view == View::Home && app.sidebar_focused,
        execute: open_project_settings,
    },
    Command {
        key: 'b',
        label: "Boot project",
        section: "Project",
        available: |app| {
            if app.view != View::Home || !app.sidebar_focused {
                return false;
            }
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                // Check if project is offline
                !online.iter().any(|p| p.a_tag() == project.a_tag())
            } else {
                false
            }
        },
        execute: boot_project,
    },
    Command {
        key: '.',
        label: "Stop all agents",
        section: "Project",
        available: |app| {
            if app.view != View::Home || !app.sidebar_focused {
                return false;
            }
            let (online, _) = app.filtered_projects();
            if let Some(project) = online.get(app.sidebar_project_index) {
                app.data_store.borrow().is_project_busy(&project.a_tag())
            } else {
                false
            }
        },
        execute: |app| {
            // Stop agents for selected project
            let (online, _) = app.filtered_projects();
            if let Some(project) = online.get(app.sidebar_project_index) {
                if let Some(core_handle) = app.core_handle.clone() {
                    let _ = core_handle.send(NostrCommand::StopOperations {
                        project_a_tag: project.a_tag(),
                        event_ids: vec![],
                        agent_pubkeys: vec![],
                    });
                }
            }
        },
    },
    Command {
        key: 'a',
        label: "Archive",
        section: "Project",
        available: |app| {
            if app.view != View::Home || !app.sidebar_focused {
                return false;
            }
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                !app.is_project_archived(&project.a_tag())
            } else {
                false
            }
        },
        execute: archive_toggle,
    },
    Command {
        key: 'a',
        label: "Unarchive",
        section: "Project",
        available: |app| {
            if app.view != View::Home || !app.sidebar_focused {
                return false;
            }
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                app.is_project_archived(&project.a_tag())
            } else {
                false
            }
        },
        execute: archive_toggle,
    },
    Command {
        key: 'H',
        label: "Show archived projects",
        section: "Filter",
        available: |app| app.view == View::Home && app.sidebar_focused && !app.show_archived_projects,
        execute: |app| {
            app.toggle_show_archived_projects();
        },
    },
    Command {
        key: 'H',
        label: "Hide archived projects",
        section: "Filter",
        available: |app| app.view == View::Home && app.sidebar_focused && app.show_archived_projects,
        execute: |app| {
            app.toggle_show_archived_projects();
        },
    },
    // =========================================================================
    // CHAT VIEW - Available in both Normal and Editing modes
    // =========================================================================
    Command {
        key: '@',
        label: "Mention agent",
        section: "Input",
        available: |app| app.view == View::Chat && !app.available_agents().is_empty(),
        execute: |app| {
            app.open_agent_selector();
        },
    },
    Command {
        key: '%',
        label: "Select branch",
        section: "Input",
        available: |app| app.view == View::Chat,
        execute: |app| {
            app.open_branch_selector();
        },
    },
    Command {
        key: 'd',
        label: "View drafts",
        section: "Draft",
        available: |app| app.view == View::Chat,
        execute: |app| {
            app.open_draft_navigator();
        },
    },
    // NOTE: 'n' is reserved as a fallback for "next tab" in command palette
    // New conversation in Chat view is available via Shift+N (which is handled by the 'N' command below)
    Command {
        key: 'g',
        label: "Go to parent",
        section: "Conversation",
        available: |app| {
            app.view == View::Chat
                && app
                    .selected_thread
                    .as_ref()
                    .and_then(|t| t.parent_conversation_id.as_ref())
                    .is_some()
        },
        execute: go_to_parent,
    },
    Command {
        key: 'c',
        label: "Copy conversation ID",
        section: "Conversation",
        available: |app| app.view == View::Chat && app.selected_thread.is_some(),
        execute: copy_conversation_id,
    },
    Command {
        key: 'O',
        label: "Open trace",
        section: "Conversation",
        available: |app| app.view == View::Chat && app.selected_thread.is_some(),
        execute: open_conversation_trace,
    },
    Command {
        key: 'r',
        label: "Reference conversation",
        section: "Conversation",
        available: |app| app.view == View::Chat && app.selected_thread.is_some(),
        execute: reference_conversation,
    },
    Command {
        key: 'x',
        label: "Close tab",
        section: "Tab",
        available: |app| app.view == View::Chat,
        execute: |app| {
            app.close_current_tab();
        },
    },
    Command {
        key: 'X',
        label: "Archive + Close",
        section: "Tab",
        available: |app| app.view == View::Chat,
        execute: archive_and_close_tab,
    },
    // =========================================================================
    // CHAT VIEW - Normal mode only
    // =========================================================================
    Command {
        key: 'y',
        label: "Copy content",
        section: "Message",
        available: |app| app.view == View::Chat && app.input_mode == InputMode::Normal,
        execute: copy_selected_message,
    },
    Command {
        key: 'v',
        label: "View raw event",
        section: "Message",
        available: |app| app.view == View::Chat && app.input_mode == InputMode::Normal,
        execute: view_raw_event,
    },
    Command {
        key: 't',
        label: "Open trace",
        section: "Message",
        available: |app| {
            app.view == View::Chat
                && app.input_mode == InputMode::Normal
                && app.selected_message_has_trace()
        },
        execute: open_message_trace,
    },
    Command {
        key: '.',
        label: "Stop agent",
        section: "Agent",
        available: |app| {
            app.view == View::Chat
                && app.input_mode == InputMode::Normal
                && app
                    .selected_thread
                    .as_ref()
                    .map(|t| app.data_store.borrow().is_event_busy(&t.id))
                    .unwrap_or(false)
        },
        execute: stop_agents,
    },
    Command {
        key: 'T',
        label: "Toggle sidebar",
        section: "View",
        available: |app| app.view == View::Chat && app.input_mode == InputMode::Normal,
        execute: |app| {
            app.todo_sidebar_visible = !app.todo_sidebar_visible;
        },
    },
    // =========================================================================
    // CHAT VIEW - Editing mode only
    // =========================================================================
    Command {
        key: 'E',
        label: "Expand editor",
        section: "Input",
        available: |app| app.view == View::Chat && app.input_mode == InputMode::Editing,
        execute: |app| {
            app.open_expanded_editor_modal();
        },
    },
    Command {
        key: 's',
        label: "Save as draft",
        section: "Draft",
        available: |app| app.view == View::Chat && app.input_mode == InputMode::Editing,
        execute: |app| {
            app.save_named_draft();
        },
    },
    // =========================================================================
    // UNDO - Available in Home and Chat
    // =========================================================================
    Command {
        key: 'u',
        label: "Undo",
        section: "Other",
        available: |app| {
            (app.view == View::Home || app.view == View::Chat) && app.last_undo_action.is_some()
        },
        execute: undo_last_action,
    },
    // =========================================================================
    // AGENT BROWSER
    // =========================================================================
    Command {
        key: 'o',
        label: "View agent",
        section: "Agent",
        available: |app| app.view == View::AgentBrowser && !app.agent_browser_in_detail,
        execute: |app| {
            let agents = app.filtered_agent_definitions();
            if let Some(agent) = agents.get(app.agent_browser_index) {
                app.viewing_agent_id = Some(agent.id.clone());
                app.agent_browser_in_detail = true;
                app.scroll_offset = 0;
            }
        },
    },
    // NOTE: 'n' is reserved as a fallback for "next tab" in command palette
    // Create new agent is available via direct 'n' key in Agent Browser (not via command palette)
    Command {
        key: 'f',
        label: "Fork agent",
        section: "Agent",
        available: |app| app.view == View::AgentBrowser && app.agent_browser_in_detail,
        execute: |app| {
            if let Some(agent_id) = &app.viewing_agent_id {
                if let Some(agent) = app.data_store.borrow().get_agent_definition(agent_id) {
                    app.modal_state =
                        ModalState::CreateAgent(modal::CreateAgentState::fork_from(&agent));
                }
            }
        },
    },
    Command {
        key: 'c',
        label: "Clone agent",
        section: "Agent",
        available: |app| app.view == View::AgentBrowser && app.agent_browser_in_detail,
        execute: |app| {
            if let Some(agent_id) = &app.viewing_agent_id {
                if let Some(agent) = app.data_store.borrow().get_agent_definition(agent_id) {
                    app.modal_state =
                        ModalState::CreateAgent(modal::CreateAgentState::clone_from(&agent));
                }
            }
        },
    },
];

/// Get commands available for the current app state.
/// Returns commands in display order (sorted by section name, then by definition order within section).
pub fn available_commands(app: &App) -> Vec<&'static Command> {
    use std::collections::BTreeMap;

    let available: Vec<&'static Command> = COMMANDS.iter().filter(|c| (c.available)(app)).collect();

    // Group by section (BTreeMap sorts alphabetically by key)
    let mut sections_map: BTreeMap<&str, Vec<&'static Command>> = BTreeMap::new();
    for cmd in available {
        sections_map.entry(cmd.section).or_default().push(cmd);
    }

    // Flatten back to a single vec in section-sorted order
    sections_map.into_values().flatten().collect()
}

/// Execute a command by its key. Returns true if a command was executed.
pub fn execute_command(app: &mut App, key: char) -> bool {
    // Find and execute the first available command with this key
    if let Some(cmd) = COMMANDS
        .iter()
        .find(|c| c.key == key && (c.available)(app))
    {
        (cmd.execute)(app);
        true
    } else {
        false
    }
}

// =============================================================================
// Helper functions (moved from palette.rs)
// =============================================================================

fn filter_and_group_messages<'a>(
    messages: &'a [Message],
    thread_id: Option<&str>,
    subthread_root: Option<&str>,
) -> Vec<DisplayItem<'a>> {
    let display_messages: Vec<&Message> = if let Some(root_id) = subthread_root {
        messages
            .iter()
            .filter(|m| m.reply_to.as_deref() == Some(root_id))
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

    group_messages(&display_messages)
}

fn get_message_id(item: &DisplayItem<'_>) -> Option<String> {
    match item {
        DisplayItem::SingleMessage { message, .. } => Some(message.id.clone()),
        DisplayItem::DelegationPreview { .. } => None,
    }
}

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
        if let Some(status) = app.data_store.borrow().get_project_status(&a_tag) {
            if let Some(pm) = status.pm_agent() {
                app.selected_agent = Some(pm.clone());
            }
            if app.selected_branch.is_none() {
                app.selected_branch = status.default_branch().map(String::from);
            }
        }

        let tab_idx = app.open_draft_tab(&a_tag, &name);
        app.switch_to_tab(tab_idx);
        app.view = View::Chat;
        app.chat_editor_mut().clear();
    } else {
        app.set_status("No project available for new conversation");
    }
}

fn toggle_project_visibility(app: &mut App) {
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
        let agent_ids = project.agent_ids.clone();
        app.modal_state = ModalState::ProjectSettings(modal::ProjectSettingsState::new(
            a_tag,
            project_name,
            agent_ids,
        ));
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
    let messages = app.messages();
    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());
    let subthread_root = app.subthread_root.as_deref();

    let grouped = filter_and_group_messages(&messages, thread_id, subthread_root);

    if let Some(item) = grouped.get(app.selected_message_index) {
        let content = match item {
            DisplayItem::SingleMessage { message, .. } => message.content.as_str(),
            DisplayItem::DelegationPreview { thread_id, .. } => thread_id.as_str(),
        };

        if let Err(e) = arboard::Clipboard::new().and_then(|mut c| c.set_text(content)) {
            app.set_status(&format!("Failed to copy: {}", e));
        } else {
            app.set_status("Content copied to clipboard");
        }
    }
}

fn view_raw_event(app: &mut App) {
    let messages = app.messages();
    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());
    let subthread_root = app.subthread_root.as_deref();

    let grouped = filter_and_group_messages(&messages, thread_id, subthread_root);

    if let Some(item) = grouped.get(app.selected_message_index) {
        if let Some(id) = get_message_id(item) {
            if let Some(json) = get_raw_event_json(&app.db.ndb, &id) {
                let pretty_json =
                    if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
                        serde_json::to_string_pretty(&value).unwrap_or(json)
                    } else {
                        json
                    };
                app.modal_state = ModalState::ViewRawEvent {
                    message_id: id,
                    json: pretty_json,
                    scroll_offset: 0,
                };
            }
        }
    }
}

fn open_message_trace(app: &mut App) {
    let messages = app.messages();
    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());
    let subthread_root = app.subthread_root.as_deref();

    let grouped = filter_and_group_messages(&messages, thread_id, subthread_root);

    if let Some(item) = grouped.get(app.selected_message_index) {
        if let Some(id) = get_message_id(item) {
            if let Some(trace_ctx) = get_trace_context(&app.db.ndb, &id) {
                let url = format!(
                    "http://localhost:16686/trace/{}?uiFind={}",
                    trace_ctx.trace_id, trace_ctx.span_id
                );
                #[cfg(target_os = "macos")]
                let _ = std::process::Command::new("open").arg(&url).spawn();
                #[cfg(target_os = "linux")]
                let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
            }
        }
    }
}

fn open_conversation_trace(app: &mut App) {
    if let Some(thread) = &app.selected_thread {
        let trace_id = &thread.id[..32.min(thread.id.len())];
        let url = format!("http://localhost:16686/trace/{}", trace_id);
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(&url).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(&url).spawn();
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
            let parent_data = {
                let store = app.data_store.borrow();
                store.get_thread_by_id(parent_id).map(|t| {
                    let a_tag = store
                        .find_project_for_thread(parent_id)
                        .unwrap_or_default();
                    (t.clone(), a_tag)
                })
            };
            if let Some((parent, a_tag)) = parent_data {
                app.open_thread_from_home(&parent, &a_tag);
            } else {
                app.set_status(&format!(
                    "Parent conversation not found: {}",
                    &parent_id[..8]
                ));
            }
        }
    }
}

fn archive_toggle(app: &mut App) {
    if app.view == View::Home {
        if app.sidebar_focused {
            // Archive/unarchive project
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                let a_tag = project.a_tag();
                let project_name = project.name.clone();
                let is_now_archived = app.toggle_project_archived(&a_tag);

                app.last_undo_action = Some(if is_now_archived {
                    UndoAction::ProjectArchived {
                        project_a_tag: a_tag,
                        project_name: project_name.clone(),
                    }
                } else {
                    UndoAction::ProjectUnarchived {
                        project_a_tag: a_tag,
                        project_name: project_name.clone(),
                    }
                });

                let status = if is_now_archived {
                    format!("Archived: {} (Ctrl+T u to undo)", project_name)
                } else {
                    format!("Unarchived: {} (Ctrl+T u to undo)", project_name)
                };
                app.set_status(&status);
            }
        } else {
            // Archive/unarchive thread based on current home tab
            match app.home_panel_focus {
                HomeTab::Conversations => {
                    let hierarchy = get_hierarchical_threads(app);
                    if let Some(item) = hierarchy.get(app.current_selection()) {
                        let thread_id = item.thread.id.clone();
                        let thread_title = item.thread.title.clone();
                        let is_now_archived = app.toggle_thread_archived(&thread_id);

                        app.last_undo_action = Some(if is_now_archived {
                            UndoAction::ThreadArchived {
                                thread_id,
                                thread_title: thread_title.clone(),
                            }
                        } else {
                            UndoAction::ThreadUnarchived {
                                thread_id,
                                thread_title: thread_title.clone(),
                            }
                        });

                        let status = if is_now_archived {
                            format!("Archived: {} (Ctrl+T u to undo)", thread_title)
                        } else {
                            format!("Unarchived: {} (Ctrl+T u to undo)", thread_title)
                        };
                        app.set_status(&status);
                    }
                }
                HomeTab::Inbox => {
                    let items = app.inbox_items();
                    if let Some(item) = items.get(app.current_selection()) {
                        if let Some(ref thread_id) = item.thread_id {
                            let thread_id = thread_id.clone();
                            let thread_title = app
                                .data_store
                                .borrow()
                                .get_threads(&item.project_a_tag)
                                .iter()
                                .find(|t| t.id == thread_id)
                                .map(|t| t.title.clone())
                                .unwrap_or_else(|| "Conversation".to_string());
                            let is_now_archived = app.toggle_thread_archived(&thread_id);

                            app.last_undo_action = Some(if is_now_archived {
                                UndoAction::ThreadArchived {
                                    thread_id,
                                    thread_title: thread_title.clone(),
                                }
                            } else {
                                UndoAction::ThreadUnarchived {
                                    thread_id,
                                    thread_title: thread_title.clone(),
                                }
                            });

                            let status = if is_now_archived {
                                format!("Archived: {} (Ctrl+T u to undo)", thread_title)
                            } else {
                                format!("Unarchived: {} (Ctrl+T u to undo)", thread_title)
                            };
                            app.set_status(&status);
                        }
                    }
                }
                HomeTab::Status => {
                    let status_items = app.status_threads();
                    if let Some((thread, _)) = status_items.get(app.current_selection()) {
                        let thread_id = thread.id.clone();
                        let thread_title = thread.title.clone();
                        let is_now_archived = app.toggle_thread_archived(&thread_id);

                        app.last_undo_action = Some(if is_now_archived {
                            UndoAction::ThreadArchived {
                                thread_id,
                                thread_title: thread_title.clone(),
                            }
                        } else {
                            UndoAction::ThreadUnarchived {
                                thread_id,
                                thread_title: thread_title.clone(),
                            }
                        });

                        let status = if is_now_archived {
                            format!("Archived: {} (Ctrl+T u to undo)", thread_title)
                        } else {
                            format!("Unarchived: {} (Ctrl+T u to undo)", thread_title)
                        };
                        app.set_status(&status);
                    }
                }
                _ => {
                    app.set_status("Archive not available in this tab");
                }
            }
        }
    } else if app.view == View::Chat {
        if let Some(ref thread) = app.selected_thread {
            let thread_id = thread.id.clone();
            let thread_title = thread.title.clone();
            let is_now_archived = app.toggle_thread_archived(&thread_id);

            app.last_undo_action = Some(if is_now_archived {
                UndoAction::ThreadArchived {
                    thread_id,
                    thread_title: thread_title.clone(),
                }
            } else {
                UndoAction::ThreadUnarchived {
                    thread_id,
                    thread_title: thread_title.clone(),
                }
            });

            let status = if is_now_archived {
                format!("Archived: {} (Ctrl+T u to undo)", thread_title)
            } else {
                format!("Unarchived: {} (Ctrl+T u to undo)", thread_title)
            };
            app.set_status(&status);
        }
    }
}

fn undo_last_action(app: &mut App) {
    let action = match app.last_undo_action.take() {
        Some(a) => a,
        None => {
            app.set_status("Nothing to undo");
            return;
        }
    };

    match action {
        UndoAction::ThreadArchived {
            thread_id,
            thread_title,
        } => {
            app.toggle_thread_archived(&thread_id);
            app.set_status(&format!("Undone: unarchived {}", thread_title));
        }
        UndoAction::ThreadUnarchived {
            thread_id,
            thread_title,
        } => {
            app.toggle_thread_archived(&thread_id);
            app.set_status(&format!("Undone: archived {}", thread_title));
        }
        UndoAction::ProjectArchived {
            project_a_tag,
            project_name,
        } => {
            app.toggle_project_archived(&project_a_tag);
            app.set_status(&format!("Undone: unarchived {}", project_name));
        }
        UndoAction::ProjectUnarchived {
            project_a_tag,
            project_name,
        } => {
            app.toggle_project_archived(&project_a_tag);
            app.set_status(&format!("Undone: archived {}", project_name));
        }
    }
}

fn archive_and_close_tab(app: &mut App) {
    if app.view != View::Chat {
        app.set_status("Archive+close only available in chat view");
        return;
    }

    if let Some(ref thread) = app.selected_thread {
        let thread_id = thread.id.clone();
        let thread_title = thread.title.clone();

        let is_now_archived = app.toggle_thread_archived(&thread_id);

        app.last_undo_action = Some(if is_now_archived {
            UndoAction::ThreadArchived {
                thread_id,
                thread_title: thread_title.clone(),
            }
        } else {
            UndoAction::ThreadUnarchived {
                thread_id,
                thread_title: thread_title.clone(),
            }
        });

        let status = if is_now_archived {
            format!("Archived and closed: {} (Ctrl+T u to undo)", thread_title)
        } else {
            format!("Unarchived and closed: {} (Ctrl+T u to undo)", thread_title)
        };
        app.set_status(&status);

        app.close_current_tab();
    } else {
        app.close_current_tab();
    }
}

fn copy_conversation_id(app: &mut App) {
    if let Some(ref thread) = app.selected_thread {
        let conversation_id = &thread.id;
        // Truncate to first 12 characters (short ID format)
        let short_id: String = conversation_id.chars().take(12).collect();

        use arboard::Clipboard;
        match Clipboard::new() {
            Ok(mut clipboard) => {
                if clipboard.set_text(&short_id).is_ok() {
                    app.set_status(&format!("Copied short ID: {}", short_id));
                } else {
                    app.set_status("Failed to copy to clipboard");
                }
            }
            Err(e) => {
                app.set_status(&format!("Clipboard error: {}", e));
            }
        }
    } else {
        app.set_status("No conversation selected");
    }
}

/// Create a new conversation referencing the current one with a "context" tag.
/// The new conversation:
/// 1. Has the same agent, branch, and project as the current one
/// 2. Is pre-filled with a message instructing the agent to inspect the source conversation
/// 3. Includes a "context" tag pointing to the source conversation's event ID
///    (NOTE: "context" is used instead of "q" because "q" is reserved for delegation/child links)
fn reference_conversation(app: &mut App) {
    // Get required context from current state
    let (source_thread_id, project_a_tag, project_name, agent, branch) = {
        let thread = match &app.selected_thread {
            Some(t) => t,
            None => {
                app.set_status("No conversation to reference");
                return;
            }
        };
        let project = match &app.selected_project {
            Some(p) => p,
            None => {
                app.set_status("No project selected");
                return;
            }
        };
        (
            thread.id.clone(),
            project.a_tag(),
            project.name.clone(),
            app.selected_agent.clone(),
            app.selected_branch.clone(),
        )
    };

    // Calculate approximate token count from current conversation history
    // Using chars / 4 as a rough approximation
    let messages = app.messages();
    let total_chars: usize = messages.iter().map(|m| m.content.len()).sum();
    let approx_tokens = total_chars / 4;

    // Create new draft tab with same project/agent/branch
    app.save_chat_draft();
    let tab_idx = app.open_draft_tab(&project_a_tag, &project_name);
    app.switch_to_tab(tab_idx);

    // Restore agent and branch from source conversation
    app.selected_agent = agent.clone();
    app.selected_branch = branch;

    // Pre-fill the editor with the context message as a text attachment
    // This keeps the input clean while providing full context
    let context_message = format!(
        "This message is in the context of conversation id {}. Your first task is to inspect that conversation with conversation_get to understand the context we're working from. The conversation is approximately {} tokens.",
        source_thread_id,
        approx_tokens
    );

    // Add context as a text attachment (like large pastes) instead of inline text
    app.chat_editor_mut().add_text_attachment(&context_message);

    // Store the reference conversation ID in the active tab for when the message is sent
    if let Some(tab) = app.tabs.active_tab_mut() {
        tab.reference_conversation_id = Some(source_thread_id.clone());
    }

    app.view = View::Chat;
    app.input_mode = InputMode::Editing;
    app.set_status(&format!(
        "New conversation referencing {} (~{} tokens)",
        &source_thread_id[..8.min(source_thread_id.len())],
        approx_tokens
    ));
}
