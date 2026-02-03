use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent};

use crate::ui::selector::{handle_selector_key, SelectorAction};
use crate::ui::{App, ModalState};

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

pub(super) fn handle_branch_selector_key(app: &mut App, key: KeyEvent) -> Result<()> {
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
