//! Command palette execution logic.
//!
//! Handles execution of commands triggered via the command palette (Ctrl+T).

use crate::models::Message;
use crate::nostr::NostrCommand;
use crate::store::get_raw_event_json;
use crate::ui;
use crate::ui::views::chat::{group_messages, DisplayItem};
use crate::ui::{App, InputMode, ModalState, UndoAction, View};

use super::modal_handlers::export_thread_as_jsonl;

/// Filter and group messages based on current view (subthread vs main thread).
/// Returns display items for the current selection context.
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

/// Get the message ID from a display item (for actions like "view raw event").
fn get_message_id(item: &DisplayItem<'_>) -> Option<String> {
    match item {
        DisplayItem::SingleMessage { message, .. } => Some(message.id.clone()),
        DisplayItem::DelegationPreview { .. } => None,
    }
}

/// Execute a command from the palette by its key
pub(super) fn execute_palette_command(app: &mut App, key: char) {
    // Close palette first
    app.modal_state = ModalState::None;

    match key {
        // Global commands
        '1' => {
            app.view = View::Home;
            app.input_mode = InputMode::Normal;
        }
        '?' => {
            app.modal_state = ModalState::HotkeyHelp;
        }
        'q' => {
            app.quit();
        }

        // New conversation (context-dependent)
        'n' => {
            if app.view == View::Chat {
                if let Some(ref project) = app.selected_project {
                    let project_a_tag = project.a_tag();
                    let project_name = project.name.clone();

                    // Capture current agent and branch to inherit into new conversation
                    let inherited_agent = app.selected_agent.clone();
                    let inherited_branch = app.selected_branch.clone();

                    app.save_chat_draft();
                    let tab_idx = app.open_draft_tab(&project_a_tag, &project_name);
                    app.switch_to_tab(tab_idx);

                    // Restore inherited agent and branch (switch_to_tab defaults to PM)
                    app.selected_agent = inherited_agent;
                    app.selected_branch = inherited_branch;

                    app.chat_editor.clear();
                    app.set_status("New conversation (same project, agent, and branch)");
                }
            } else {
                app.open_projects_modal(true);
            }
        }
        'o' => {
            if app.view == View::Home {
                let threads = app.recent_threads();
                if let Some((thread, project_a_tag)) = threads.get(app.current_selection()) {
                    app.open_thread_from_home(thread, project_a_tag);
                }
            }
        }
        'a' => {
            // Archive toggle works in both Home and Chat views
            archive_toggle(app);
        }
        'e' => {
            if app.view == View::Chat {
                if let Some(thread) = &app.selected_thread {
                    export_thread_as_jsonl(app, &thread.id.clone());
                }
            }
        }
        'p' => {
            app.open_projects_modal(false);
        }
        'f' => {
            app.cycle_time_filter();
        }
        'A' => {
            app.open_agent_browser();
        }
        'N' => {
            app.modal_state = ModalState::CreateProject(ui::modal::CreateProjectState::new());
        }

        // Sidebar commands
        ' ' => {
            toggle_project_visibility_palette(app);
        }
        's' => {
            open_project_settings(app);
        }
        'b' => {
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
            copy_selected_message(app);
        }
        'v' => {
            view_raw_event(app);
        }
        't' => {
            open_trace(app);
        }
        'O' => {
            open_conversation_trace(app);
        }
        '.' => {
            stop_agents(app);
        }
        'g' => {
            go_to_parent(app);
        }
        'x' => {
            // Close current tab (matches tab bar hint "x:close")
            app.close_current_tab();
        }
        'X' => {
            // Archive conversation AND close tab
            archive_and_close_tab(app);
        }
        'T' => {
            app.todo_sidebar_visible = !app.todo_sidebar_visible;
        }
        'S' => {
            open_agent_settings(app);
        }
        'E' => {
            app.open_expanded_editor_modal();
        }

        'u' => {
            undo_last_action(app);
        }

        // Agent browser commands
        'c' => {
            if app.view == View::Chat {
                // Copy conversation ID (hex) to clipboard
                copy_conversation_id(app);
            } else if app.view == View::AgentBrowser && app.agent_browser_in_detail {
                if let Some(agent_id) = &app.viewing_agent_id {
                    if let Some(agent) = app.data_store.borrow().get_agent_definition(agent_id) {
                        app.modal_state = ModalState::CreateAgent(
                            ui::modal::CreateAgentState::clone_from(&agent),
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
        let agent_ids = project.agent_ids.clone();
        app.modal_state = ModalState::ProjectSettings(ui::modal::ProjectSettingsState::new(
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
        // Get the most relevant content from the display item
        let content = match item {
            DisplayItem::SingleMessage { message, .. } => message.content.as_str(),
            DisplayItem::DelegationPreview { thread_id, .. } => {
                // Copy the thread ID for delegation previews
                thread_id.as_str()
            }
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
                let pretty_json = if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
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

fn open_trace(app: &mut App) {
    use crate::store::get_trace_context;

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
    // Use the conversation root event ID (first 32 chars) as the trace ID
    // This matches the Svelte implementation
    if let Some(thread) = &app.selected_thread {
        // Thread ID is the root event ID in hex format
        // Take first 32 chars as the trace ID
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
            let project_a_tag = app
                .data_store
                .borrow()
                .find_project_for_thread(&thread.id);
            if let Some(a_tag) = project_a_tag {
                let parent_thread = app
                    .data_store
                    .borrow()
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

    let (all_tools, all_models) = app
        .data_store
        .borrow()
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

fn archive_toggle(app: &mut App) {
    use crate::ui::views::home::get_hierarchical_threads;
    use crate::ui::HomeTab;

    if app.view == View::Home {
        if app.sidebar_focused {
            // Archive/unarchive project
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                let a_tag = project.a_tag();
                let project_name = project.name.clone();
                let is_now_archived = app.toggle_project_archived(&a_tag);

                // Store undo action
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
                HomeTab::Recent => {
                    let hierarchy = get_hierarchical_threads(app);
                    if let Some(item) = hierarchy.get(app.current_selection()) {
                        let thread_id = item.thread.id.clone();
                        let thread_title = item.thread.title.clone();
                        let is_now_archived = app.toggle_thread_archived(&thread_id);

                        // Store undo action
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

                            // Store undo action
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

                        // Store undo action
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
        // In chat view, archive the current conversation
        if let Some(ref thread) = app.selected_thread {
            let thread_id = thread.id.clone();
            let thread_title = thread.title.clone();
            let is_now_archived = app.toggle_thread_archived(&thread_id);

            // Store undo action
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
        UndoAction::ThreadArchived { thread_id, thread_title } => {
            // Undo archive = unarchive
            app.toggle_thread_archived(&thread_id);
            app.set_status(&format!("Undone: unarchived {}", thread_title));
        }
        UndoAction::ThreadUnarchived { thread_id, thread_title } => {
            // Undo unarchive = archive
            app.toggle_thread_archived(&thread_id);
            app.set_status(&format!("Undone: archived {}", thread_title));
        }
        UndoAction::ProjectArchived { project_a_tag, project_name } => {
            // Undo archive = unarchive
            app.toggle_project_archived(&project_a_tag);
            app.set_status(&format!("Undone: unarchived {}", project_name));
        }
        UndoAction::ProjectUnarchived { project_a_tag, project_name } => {
            // Undo unarchive = archive
            app.toggle_project_archived(&project_a_tag);
            app.set_status(&format!("Undone: archived {}", project_name));
        }
    }
}

fn archive_and_close_tab(app: &mut App) {
    // Only works in chat view with an existing conversation
    if app.view != View::Chat {
        app.set_status("Archive+close only available in chat view");
        return;
    }

    if let Some(ref thread) = app.selected_thread {
        let thread_id = thread.id.clone();
        let thread_title = thread.title.clone();

        // Archive the conversation
        let is_now_archived = app.toggle_thread_archived(&thread_id);

        // Store undo action
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

        // Close the tab
        app.close_current_tab();
    } else {
        // Draft tab - just close it
        app.close_current_tab();
    }
}

fn copy_conversation_id(app: &mut App) {
    if let Some(ref thread) = app.selected_thread {
        // The thread.id is the conversation's root event ID (hex format)
        let conversation_id = &thread.id;

        use arboard::Clipboard;
        match Clipboard::new() {
            Ok(mut clipboard) => {
                if clipboard.set_text(conversation_id).is_ok() {
                    app.set_status(&format!("Copied conversation ID: {}", conversation_id));
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
