use crossterm::event::{KeyCode, KeyEvent};

use crate::nostr::NostrCommand;
use crate::ui::{self, App, ModalState};

use super::helpers::export_thread_as_jsonl;

pub(super) fn handle_project_actions_modal_key(app: &mut App, key: KeyEvent) {
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
                    app.set_warning_status(&format!("Failed to boot: {}", e));
                } else {
                    app.set_warning_status(&format!(
                        "Boot request sent for {}",
                        state.project_name
                    ));
                }
            }
            app.modal_state = ModalState::None;
        }
        ProjectAction::Settings => {
            let (agent_definition_ids, mcp_tool_ids) = {
                let store = app.data_store.borrow();
                store
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == state.project_a_tag)
                    .map(|p| (p.agent_definition_ids.clone(), p.mcp_tool_ids.clone()))
                    .unwrap_or_default()
            };
            app.modal_state = ModalState::ProjectSettings(ui::modal::ProjectSettingsState::new(
                state.project_a_tag.clone(),
                state.project_name.clone(),
                agent_definition_ids,
                mcp_tool_ids,
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

                // Auto-select PM agent from status
                let pm_agent = {
                    let store = app.data_store.borrow();
                    store
                        .get_project_status(&a_tag)
                        .and_then(|status| status.pm_agent().cloned())
                };
                if let Some(pm) = pm_agent {
                    app.set_selected_agent(Some(pm));
                }

                app.modal_state = ModalState::None;
                let tab_idx = app.open_draft_tab(&a_tag, &project_name);
                app.switch_to_tab(tab_idx);
                app.chat_editor_mut().clear();
            } else {
                app.modal_state = ModalState::None;
                app.set_warning_status("Project not found");
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

pub(super) fn handle_conversation_actions_modal_key(app: &mut App, key: KeyEvent) {
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
            app.set_warning_status(&status);
            app.modal_state = ModalState::None;
        }
    }
}

pub(super) fn handle_chat_actions_modal_key(app: &mut App, key: KeyEvent) {
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
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "New".to_string());

            app.modal_state = ModalState::None;
            app.save_chat_draft();
            let tab_idx = app.open_draft_tab(&project_a_tag, &project_name);
            app.switch_to_tab(tab_idx);
            app.chat_editor_mut().clear();
            app.set_warning_status("New conversation (same project, agent, and branch)");
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
                    app.set_warning_status(&format!("Navigated to parent: {}", thread.title));
                } else {
                    app.set_warning_status("Parent conversation not found");
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
