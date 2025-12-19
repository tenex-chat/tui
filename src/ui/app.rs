use crate::models::{ChatDraft, DraftStorage, Project, ProjectAgent, ProjectStatus, StreamingAccumulator, Thread};
use crate::nostr::{DataChange, NostrCommand};
use crate::store::{AppDataStore, Database};
use crate::ui::text_editor::TextEditor;
use nostr_sdk::Keys;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Login,
    Projects,
    Threads,
    Chat,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
}

pub struct App {
    pub running: bool,
    pub view: View,
    pub input_mode: InputMode,
    pub input: String,
    pub cursor_position: usize,

    pub db: Arc<Database>,
    pub keys: Option<Keys>,

    pub selected_project_index: usize,
    pub selected_thread_index: usize,
    pub selected_project: Option<Project>,
    pub selected_thread: Option<Thread>,
    pub selected_agent: Option<ProjectAgent>,

    pub scroll_offset: usize,
    pub status_message: Option<String>,

    pub creating_thread: bool,
    pub showing_agent_selector: bool,
    pub agent_selector_index: usize,
    pub showing_branch_selector: bool,
    pub branch_selector_index: usize,
    pub selected_branch: Option<String>,
    pub selector_filter: String,

    pub streaming_accumulator: StreamingAccumulator,

    pub command_tx: Option<Sender<NostrCommand>>,
    pub data_rx: Option<Receiver<DataChange>>,

    /// Filter text for projects view (type to filter)
    pub project_filter: String,

    /// Whether offline projects section is expanded
    pub offline_projects_expanded: bool,

    /// Whether user pressed Ctrl+C once (pending quit confirmation)
    pub pending_quit: bool,

    /// Draft storage for persisting message drafts
    draft_storage: RefCell<DraftStorage>,

    /// Rich text editor for chat input (multiline, attachments)
    pub chat_editor: TextEditor,

    /// Whether attachment modal is open
    pub showing_attachment_modal: bool,

    /// Editor for the attachment modal content
    pub attachment_modal_editor: TextEditor,

    /// Single source of truth for app data
    pub data_store: Rc<RefCell<AppDataStore>>,
}

impl App {
    pub fn new(db: Database, data_store: Rc<RefCell<AppDataStore>>) -> Self {
        Self {
            running: true,
            view: View::Login,
            input_mode: InputMode::Normal,
            input: String::new(),
            cursor_position: 0,

            db: Arc::new(db),
            keys: None,

            selected_project_index: 0,
            selected_thread_index: 0,
            selected_project: None,
            selected_thread: None,
            selected_agent: None,

            scroll_offset: 0,
            status_message: None,

            creating_thread: false,
            showing_agent_selector: false,
            agent_selector_index: 0,
            showing_branch_selector: false,
            branch_selector_index: 0,
            selected_branch: None,
            selector_filter: String::new(),

            streaming_accumulator: StreamingAccumulator::new(),

            command_tx: None,
            data_rx: None,

            project_filter: String::new(),
            offline_projects_expanded: false,
            pending_quit: false,
            draft_storage: RefCell::new(DraftStorage::new("tenex_data")),
            chat_editor: TextEditor::new(),
            showing_attachment_modal: false,
            attachment_modal_editor: TextEditor::new(),
            data_store,
        }
    }

    /// Get project status for a project - delegates to data store
    pub fn get_project_status(&self, project: &Project) -> Option<ProjectStatus> {
        self.data_store.borrow().get_project_status(&project.a_tag()).cloned()
    }

    /// Get project status for selected project
    pub fn get_selected_project_status(&self) -> Option<ProjectStatus> {
        self.selected_project.as_ref().and_then(|p| self.get_project_status(p))
    }

    /// Check if a project is online (has recent 24010 status)
    pub fn is_project_online(&self, project: &Project) -> bool {
        self.data_store.borrow().is_project_online(&project.a_tag())
    }

    /// Save current chat editor content as draft for the selected thread
    pub fn save_chat_draft(&self) {
        if let Some(ref thread) = self.selected_thread {
            let draft = ChatDraft {
                conversation_id: thread.id.clone(),
                text: self.chat_editor.build_full_content(),
                selected_agent_pubkey: self.selected_agent.as_ref().map(|a| a.pubkey.clone()),
                selected_branch: self.selected_branch.clone(),
                last_modified: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            };
            self.draft_storage.borrow_mut().save(draft);
        }
    }

    /// Restore draft for the selected thread into chat_editor
    pub fn restore_chat_draft(&mut self) {
        if let Some(ref thread) = self.selected_thread {
            if let Some(draft) = self.draft_storage.borrow().load(&thread.id) {
                // For now, put all content in the text field
                // (attachments will be re-created on paste if needed)
                self.chat_editor.text = draft.text;
                self.chat_editor.cursor = self.chat_editor.text.len();
                self.selected_branch = draft.selected_branch;
                if let Some(agent_pubkey) = draft.selected_agent_pubkey {
                    if let Some(status) = self.get_selected_project_status() {
                        self.selected_agent = status
                            .agents
                            .iter()
                            .find(|a| a.pubkey == agent_pubkey)
                            .cloned();
                    }
                }
            }
        }
    }

    /// Delete draft for the selected thread (call after sending message)
    pub fn delete_chat_draft(&self) {
        if let Some(ref thread) = self.selected_thread {
            self.draft_storage.borrow_mut().delete(&thread.id);
        }
    }

    /// Open attachment modal with focused attachment's content
    pub fn open_attachment_modal(&mut self) {
        if let Some(attachment) = self.chat_editor.get_focused_attachment() {
            self.attachment_modal_editor.text = attachment.content.clone();
            self.attachment_modal_editor.cursor = 0;
            self.showing_attachment_modal = true;
        }
    }

    /// Save attachment modal changes and close
    pub fn save_and_close_attachment_modal(&mut self) {
        let new_content = self.attachment_modal_editor.text.clone();
        self.chat_editor.update_focused_attachment(new_content);
        self.attachment_modal_editor.clear();
        self.showing_attachment_modal = false;
    }

    /// Close attachment modal without saving
    pub fn cancel_attachment_modal(&mut self) {
        self.attachment_modal_editor.clear();
        self.showing_attachment_modal = false;
    }

    /// Delete focused attachment and close modal
    pub fn delete_attachment_and_close_modal(&mut self) {
        self.chat_editor.delete_focused_attachment();
        self.attachment_modal_editor.clear();
        self.showing_attachment_modal = false;
    }

    /// Get filtered projects based on current filter
    pub fn filtered_projects(&self) -> (Vec<Project>, Vec<Project>) {
        let filter = self.project_filter.to_lowercase();
        let store = self.data_store.borrow();
        let projects = store.get_projects();

        let matching: Vec<&Project> = projects
            .iter()
            .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter))
            .collect();

        // Separate into online and offline
        let (online, offline): (Vec<_>, Vec<_>) = matching
            .into_iter()
            .partition(|p| store.is_project_online(&p.a_tag()));

        (online.into_iter().cloned().collect(), offline.into_iter().cloned().collect())
    }

    pub fn set_channels(&mut self, command_tx: Sender<NostrCommand>, data_rx: Receiver<DataChange>) {
        self.command_tx = Some(command_tx);
        self.data_rx = Some(data_rx);
    }

    /// Process streaming deltas from the worker channel.
    /// All other updates are handled via nostrdb SubscriptionStream in main.rs.
    pub fn check_for_data_updates(&mut self) -> anyhow::Result<()> {
        if let Some(ref data_rx) = self.data_rx {
            // Process streaming deltas (need ordered delivery)
            while let Ok(DataChange::StreamingDelta { message_id, delta }) = data_rx.try_recv() {
                let streaming_delta = crate::models::StreamingDelta {
                    message_id,
                    delta,
                    sequence: None,
                    created_at: 0,
                };
                self.streaming_accumulator.add_delta(streaming_delta);
            }
        }
        Ok(())
    }

    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some(msg.to_string());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position += 1;
        }
    }

    pub fn enter_char(&mut self, c: char) {
        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 && !self.input.is_empty() {
            self.cursor_position -= 1;
            self.input.remove(self.cursor_position);
        }
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
    }

    /// Get available agents from project status (from data store)
    pub fn available_agents(&self) -> Vec<crate::models::ProjectAgent> {
        self.selected_project.as_ref()
            .and_then(|p| self.data_store.borrow().get_project_status(&p.a_tag()))
            .map(|s| s.agents.clone())
            .unwrap_or_default()
    }

    /// Select agent by index from available agents
    pub fn select_agent_by_index(&mut self, index: usize) {
        if let Some(status) = self.get_selected_project_status() {
            if let Some(agent) = status.agents.get(index) {
                self.selected_agent = Some(agent.clone());
            }
        }
    }

    /// Get available branches from project status (from data store)
    pub fn available_branches(&self) -> Vec<String> {
        self.selected_project.as_ref()
            .and_then(|p| self.data_store.borrow().get_project_status(&p.a_tag()))
            .map(|s| s.branches.clone())
            .unwrap_or_default()
    }

    /// Get agents filtered by selector_filter
    pub fn filtered_agents(&self) -> Vec<crate::models::ProjectAgent> {
        let filter = self.selector_filter.to_lowercase();
        self.available_agents()
            .into_iter()
            .filter(|a| filter.is_empty() || a.name.to_lowercase().contains(&filter))
            .collect()
    }

    /// Get branches filtered by selector_filter
    pub fn filtered_branches(&self) -> Vec<String> {
        let filter = self.selector_filter.to_lowercase();
        self.available_branches()
            .into_iter()
            .filter(|b| filter.is_empty() || b.to_lowercase().contains(&filter))
            .collect()
    }

    /// Select branch by index from filtered branches
    pub fn select_branch_by_index(&mut self, index: usize) {
        let filtered = self.filtered_branches();
        if let Some(branch) = filtered.get(index) {
            self.selected_branch = Some(branch.clone());
        }
    }

    /// Select agent by index from filtered agents
    pub fn select_filtered_agent_by_index(&mut self, index: usize) {
        let filtered = self.filtered_agents();
        if let Some(project_agent) = filtered.get(index) {
            self.selected_agent = Some(project_agent.clone());
        }
    }

    pub fn submit_input(&mut self) -> String {
        let input = self.input.clone();
        self.clear_input();
        input
    }
}
