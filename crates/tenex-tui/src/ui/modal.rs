use crate::models::{AskEvent, Project, ProjectAgent};
use crate::ui::ask_input::AskInputState;
use crate::ui::selector::SelectorState;
use crate::ui::text_editor::TextEditor;

use super::app::NewThreadField;

/// State for the ask modal (answering multi-question ask events)
#[derive(Debug, Clone)]
pub struct AskModalState {
    pub message_id: String,
    pub ask_event: AskEvent,
    pub input_state: AskInputState,
}

/// Unified modal state - only one modal can be open at a time
#[derive(Debug, Clone)]
pub enum ModalState {
    None,
    AttachmentEditor {
        editor: TextEditor,
    },
    AgentSelector {
        selector: SelectorState,
    },
    BranchSelector {
        selector: SelectorState,
    },
    ProjectsModal {
        selector: SelectorState,
    },
    NewThread {
        focus: NewThreadField,
        project_selector: SelectorState,
        agent_selector: SelectorState,
        selected_project: Option<Project>,
        selected_agent: Option<ProjectAgent>,
        editor: TextEditor,
    },
    AskModal(AskModalState),
}

impl Default for ModalState {
    fn default() -> Self {
        Self::None
    }
}

impl ModalState {
    pub fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }

    pub fn close(&mut self) {
        *self = Self::None;
    }
}
