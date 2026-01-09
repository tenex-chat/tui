use crate::models::AskEvent;
use crate::ui::ask_input::AskInputState;
use crate::ui::selector::SelectorState;
use crate::ui::text_editor::TextEditor;

/// State for the ask modal (answering multi-question ask events)
#[derive(Debug, Clone)]
pub struct AskModalState {
    pub message_id: String,
    pub ask_event: AskEvent,
    pub input_state: AskInputState,
}

/// State for project settings modal
#[derive(Debug, Clone)]
pub struct ProjectSettingsState {
    pub project_a_tag: String,
    pub project_name: String,
    pub original_agent_ids: Vec<String>,
    pub pending_agent_ids: Vec<String>,
    pub selector_index: usize,
    pub in_add_mode: bool,
    pub add_filter: String,
    pub add_index: usize,
}

impl ProjectSettingsState {
    pub fn new(project_a_tag: String, project_name: String, agent_ids: Vec<String>) -> Self {
        Self {
            project_a_tag,
            project_name,
            original_agent_ids: agent_ids.clone(),
            pending_agent_ids: agent_ids,
            selector_index: 0,
            in_add_mode: false,
            add_filter: String::new(),
            add_index: 0,
        }
    }

    pub fn has_changes(&self) -> bool {
        self.original_agent_ids != self.pending_agent_ids
    }

    pub fn add_agent(&mut self, event_id: String) {
        if !self.pending_agent_ids.contains(&event_id) {
            self.pending_agent_ids.push(event_id);
        }
    }

    pub fn remove_agent(&mut self, index: usize) {
        if index < self.pending_agent_ids.len() {
            self.pending_agent_ids.remove(index);
            if self.selector_index >= self.pending_agent_ids.len() && self.selector_index > 0 {
                self.selector_index -= 1;
            }
        }
    }

    pub fn set_pm(&mut self, index: usize) {
        if index < self.pending_agent_ids.len() && index > 0 {
            let agent_id = self.pending_agent_ids.remove(index);
            self.pending_agent_ids.insert(0, agent_id);
            self.selector_index = 0;
        }
    }
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
        /// If true, selecting a project navigates to chat view to create a new thread
        for_new_thread: bool,
    },
    AskModal(AskModalState),
    ProjectSettings(ProjectSettingsState),
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
