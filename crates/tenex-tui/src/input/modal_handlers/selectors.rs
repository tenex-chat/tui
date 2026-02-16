use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::selector::{handle_selector_key, SelectorAction};
use crate::ui::{App, ModalState, View};

pub(super) fn handle_agent_selector_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let agents = app.filtered_agents();
    let item_count = agents.len();

    if let ModalState::AgentSelector { ref mut selector } = app.modal_state {
        match handle_selector_key(selector, key, item_count, |idx| agents.get(idx).cloned()) {
            SelectorAction::Selected(agent) => {
                // Set agent as recipient - never insert text into input
                app.set_selected_agent(Some(agent));
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

pub(super) fn handle_projects_modal_key(app: &mut App, key: KeyEvent) -> Result<()> {
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
                app.selected_project = Some(project);

                // Auto-select PM agent from status
                // Extract values before making mutable calls to avoid borrow issues
                let pm_agent = {
                    let store = app.data_store.borrow();
                    if let Some(status) = store.get_project_status(&a_tag) {
                        if needs_agent { status.pm_agent().cloned() } else { None }
                    } else {
                        None
                    }
                };
                if let Some(pm) = pm_agent {
                    app.set_selected_agent(Some(pm));
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
                    // When switching projects from Chat view (#), update ONLY draft tabs
                    // to the new project. NEVER mutate real thread tabs - their project_a_tag
                    // must always match their thread_id's project to maintain state invariants.
                    if let Some(tab) = app.tabs.active_tab_mut() {
                        if tab.is_draft() && app.view == View::Chat {
                            // Update draft tab to new project - this requires updating:
                            // 1. project_a_tag (where replies will be published)
                            // 2. draft_id (storage key for persisting draft content)
                            // 3. thread_title (UI label showing project name)
                            let project_name = app
                                .selected_project
                                .as_ref()
                                .map(|p| p.name.clone())
                                .unwrap_or_else(|| "New".to_string());

                            tab.project_a_tag = a_tag.clone();
                            tab.draft_id = Some(format!("{}:new", a_tag));
                            tab.thread_title = format!("New: {}", project_name);

                            // Save the draft content under the new project key
                            app.save_chat_draft();
                        }
                        // For real thread tabs: do nothing. Tab stays on its original project,
                        // but Home view filter changes to show only the selected project.
                    }

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

pub(super) fn handle_nudge_selector_key(app: &mut App, key: KeyEvent) {
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

pub(super) fn handle_skill_selector_key(app: &mut App, key: KeyEvent) {
    let skills = app.filtered_skills();
    let item_count = skills.len();

    if let ModalState::SkillSelector(ref mut state) = app.modal_state {
        match key.code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
            }
            KeyCode::Enter => {
                // Apply to current tab (per-tab isolated)
                let selected_ids = state.selected_skill_ids.clone();
                if let Some(tab) = app.tabs.active_tab_mut() {
                    tab.selected_skill_ids = selected_ids;
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
                if let Some(skill) = skills.get(state.selector.index) {
                    let skill_id = skill.id.clone();
                    if let Some(pos) = state.selected_skill_ids.iter().position(|id| id == &skill_id)
                    {
                        state.selected_skill_ids.remove(pos);
                    } else {
                        state.selected_skill_ids.push(skill_id);
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
