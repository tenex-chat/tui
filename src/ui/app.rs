use crate::models::{AskEvent, ChatDraft, DraftStorage, Message, PreferencesStorage, Project, ProjectAgent, ProjectDraft, ProjectDraftStorage, ProjectStatus, Thread};
use crate::nostr::{DataChange, NostrCommand};
use crate::store::{AppDataStore, Database};
use crate::ui::ask_input::AskInputState;
use crate::ui::modal::ModalState;
use crate::ui::selector::SelectorState;
use crate::ui::text_editor::TextEditor;
use nostr_sdk::Keys;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Login,
    Home,
    Chat,
    LessonViewer,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
}

#[derive(Debug, Clone, PartialEq)]
pub enum HomeTab {
    Recent,
    Inbox,
}

#[derive(Debug, Clone, PartialEq)]
pub enum NewThreadField {
    Content,
    Project,
    Agent,
}

/// An open tab representing a thread
#[derive(Debug, Clone)]
pub struct OpenTab {
    pub thread_id: String,
    pub thread_title: String,
    pub project_a_tag: String,
    pub has_unread: bool,
}

/// Maximum number of open tabs (matches 1-9 shortcuts)
pub const MAX_TABS: usize = 9;

/// State for the ask modal (answering multi-question ask events)
#[derive(Debug, Clone)]
pub struct AskModalState {
    pub message_id: String,
    pub ask_event: AskEvent,
    pub input_state: AskInputState,
}

/// Buffer for local streaming content (per conversation)
#[derive(Default, Clone)]
pub struct LocalStreamBuffer {
    pub agent_pubkey: String,
    pub text_content: String,
    pub reasoning_content: String,
    pub is_complete: bool,
}

pub struct App {
    pub running: bool,
    pub view: View,
    pub input_mode: InputMode,
    pub input: String,
    pub cursor_position: usize,

    pub db: Arc<Database>,
    pub keys: Option<Keys>,

    pub selected_project: Option<Project>,
    pub selected_thread: Option<Thread>,
    pub selected_agent: Option<ProjectAgent>,

    pub scroll_offset: usize,
    /// Maximum scroll offset (set after rendering to enable proper scroll clamping)
    pub max_scroll_offset: usize,
    pub status_message: Option<String>,

    pub creating_thread: bool,
    pub selected_branch: Option<String>,

    pub command_tx: Option<Sender<NostrCommand>>,
    pub data_rx: Option<Receiver<DataChange>>,

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

    /// When viewing a subthread, this is the root message ID
    pub subthread_root: Option<String>,
    /// The root message when viewing a subthread (for display and reply tagging)
    pub subthread_root_message: Option<Message>,
    /// Index of selected message in chat view (for navigation)
    pub selected_message_index: usize,

    /// Open tabs (max 9, LRU eviction)
    pub open_tabs: Vec<OpenTab>,
    /// Index of the active tab
    pub active_tab_index: usize,
    /// Tab visit history for Alt+Tab cycling (most recent last)
    pub tab_history: Vec<usize>,
    /// Whether the tab modal is showing
    pub showing_tab_modal: bool,
    /// Selected index in tab modal
    pub tab_modal_index: usize,

    // Home view state
    pub home_panel_focus: HomeTab,
    pub selected_inbox_index: usize,
    pub selected_recent_index: usize,
    /// Whether sidebar is focused (vs content area)
    pub sidebar_focused: bool,
    /// Selected index in sidebar project list
    pub sidebar_project_index: usize,
    /// Projects to show in Recent/Inbox (empty = all projects)
    pub visible_projects: HashSet<String>,

    // New thread modal state
    pub showing_new_thread_modal: bool,
    pub new_thread_modal_focus: NewThreadField,
    pub new_thread_project_filter: String,
    pub new_thread_agent_filter: String,
    pub new_thread_selected_project: Option<Project>,
    pub new_thread_selected_agent: Option<ProjectAgent>,
    pub new_thread_editor: TextEditor,
    pub new_thread_project_index: usize,
    pub new_thread_agent_index: usize,
    project_draft_storage: RefCell<ProjectDraftStorage>,
    preferences: RefCell<PreferencesStorage>,

    // Ask modal state
    pub ask_modal_state: Option<AskModalState>,

    /// Unified modal state (will replace individual modal booleans)
    pub modal_state: ModalState,

    // Lesson viewer state
    pub viewing_lesson_id: Option<String>,
    pub lesson_viewer_section: usize,

    /// Local streaming buffers by conversation_id
    pub local_stream_buffers: HashMap<String, LocalStreamBuffer>,

    /// Toggle for showing/hiding LLM metadata on messages (model, tokens, cost)
    pub show_llm_metadata: bool,

    /// Toggle for showing/hiding the todo sidebar
    pub todo_sidebar_visible: bool,
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

            selected_project: None,
            selected_thread: None,
            selected_agent: None,

            scroll_offset: 0,
            max_scroll_offset: 0,
            status_message: None,

            creating_thread: false,
            selected_branch: None,

            command_tx: None,
            data_rx: None,

            project_filter: String::new(),
            pending_quit: false,
            draft_storage: RefCell::new(DraftStorage::new("tenex_data")),
            chat_editor: TextEditor::new(),
            showing_attachment_modal: false,
            attachment_modal_editor: TextEditor::new(),
            data_store,
            subthread_root: None,
            subthread_root_message: None,
            selected_message_index: 0,
            open_tabs: Vec::new(),
            active_tab_index: 0,
            tab_history: Vec::new(),
            showing_tab_modal: false,
            tab_modal_index: 0,
            home_panel_focus: HomeTab::Recent,
            selected_inbox_index: 0,
            selected_recent_index: 0,
            showing_projects_modal: false,
            sidebar_focused: false,
            sidebar_project_index: 0,
            visible_projects: HashSet::new(),
            showing_new_thread_modal: false,
            new_thread_modal_focus: NewThreadField::Content,
            new_thread_project_filter: String::new(),
            new_thread_agent_filter: String::new(),
            new_thread_selected_project: None,
            new_thread_selected_agent: None,
            new_thread_editor: TextEditor::new(),
            new_thread_project_index: 0,
            new_thread_agent_index: 0,
            project_draft_storage: RefCell::new(ProjectDraftStorage::new("tenex_data")),
            preferences: RefCell::new(PreferencesStorage::new("tenex_data")),
            ask_modal_state: None,
            modal_state: ModalState::None,
            viewing_lesson_id: None,
            lesson_viewer_section: 0,
            local_stream_buffers: HashMap::new(),
            show_llm_metadata: false,
            todo_sidebar_visible: true,
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

    /// Get messages for the currently selected thread
    pub fn messages(&self) -> Vec<Message> {
        self.selected_thread.as_ref()
            .map(|t| self.data_store.borrow().get_messages(&t.id).to_vec())
            .unwrap_or_default()
    }

    /// Enter a subthread view rooted at the given message
    pub fn enter_subthread(&mut self, message: Message) {
        self.subthread_root = Some(message.id.clone());
        self.subthread_root_message = Some(message);
        self.selected_message_index = 0;
        self.scroll_offset = 0;
    }

    /// Exit the current subthread view and return to parent
    pub fn exit_subthread(&mut self) {
        self.subthread_root = None;
        self.subthread_root_message = None;
        self.selected_message_index = 0;
    }

    /// Check if we're currently viewing a subthread
    pub fn in_subthread(&self) -> bool {
        self.subthread_root.is_some()
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

    /// Get filtered projects based on current filter (from ModalState)
    pub fn filtered_projects(&self) -> (Vec<Project>, Vec<Project>) {
        let filter = self.projects_modal_filter().to_lowercase();
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

    /// Open the projects modal
    pub fn open_projects_modal(&mut self) {
        self.modal_state = ModalState::ProjectsModal {
            selector: SelectorState::new(),
        };
    }

    /// Get projects modal index (from ModalState)
    pub fn projects_modal_index(&self) -> usize {
        match &self.modal_state {
            ModalState::ProjectsModal { selector } => selector.index,
            _ => 0,
        }
    }

    /// Get projects modal filter (from ModalState)
    pub fn projects_modal_filter(&self) -> &str {
        match &self.modal_state {
            ModalState::ProjectsModal { selector } => &selector.filter,
            _ => "",
        }
    }

    pub fn set_channels(&mut self, command_tx: Sender<NostrCommand>, data_rx: Receiver<DataChange>) {
        self.command_tx = Some(command_tx);
        self.data_rx = Some(data_rx);
    }

    /// Process local streaming chunks from the worker channel.
    /// All other updates are handled via nostrdb SubscriptionStream in main.rs.
    pub fn check_for_data_updates(&mut self) -> anyhow::Result<()> {
        // Collect all pending changes first to avoid borrow conflicts
        let changes: Vec<DataChange> = self.data_rx
            .as_ref()
            .map(|rx| std::iter::from_fn(|| rx.try_recv().ok()).collect())
            .unwrap_or_default();

        for change in changes {
            match change {
                DataChange::LocalStreamChunk {
                    agent_pubkey,
                    conversation_id,
                    text_delta,
                    reasoning_delta,
                    is_finish,
                } => {
                    self.handle_local_stream_chunk(
                        agent_pubkey,
                        conversation_id,
                        text_delta,
                        reasoning_delta,
                        is_finish,
                    );
                }
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

    /// Scroll up by the given amount, clamping to valid range
    pub fn scroll_up(&mut self, amount: usize) {
        // First clamp scroll_offset to max if it's above (handles usize::MAX sentinel)
        if self.scroll_offset > self.max_scroll_offset {
            self.scroll_offset = self.max_scroll_offset;
        }
        self.scroll_offset = self.scroll_offset.saturating_sub(amount);
    }

    /// Scroll down by the given amount, clamping to valid range
    pub fn scroll_down(&mut self, amount: usize) {
        // First clamp scroll_offset to max if it's above (handles usize::MAX sentinel)
        if self.scroll_offset > self.max_scroll_offset {
            self.scroll_offset = self.max_scroll_offset;
        }
        self.scroll_offset = self.scroll_offset.saturating_add(amount).min(self.max_scroll_offset);
    }

    /// Scroll to bottom
    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.max_scroll_offset;
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
            .and_then(|p| {
                self.data_store.borrow()
                    .get_project_status(&p.a_tag())
                    .map(|s| s.agents.clone())
            })
            .unwrap_or_default()
    }

    /// Get available branches from project status (from data store)
    pub fn available_branches(&self) -> Vec<String> {
        self.selected_project.as_ref()
            .and_then(|p| {
                self.data_store.borrow()
                    .get_project_status(&p.a_tag())
                    .map(|s| s.branches.clone())
            })
            .unwrap_or_default()
    }

    /// Get agents filtered by current filter (from ModalState or empty)
    pub fn filtered_agents(&self) -> Vec<crate::models::ProjectAgent> {
        let filter = match &self.modal_state {
            ModalState::AgentSelector { selector } => selector.filter.to_lowercase(),
            _ => String::new(),
        };
        self.available_agents()
            .into_iter()
            .filter(|a| filter.is_empty() || a.name.to_lowercase().contains(&filter))
            .collect()
    }

    /// Get agent selector index (from ModalState)
    pub fn agent_selector_index(&self) -> usize {
        match &self.modal_state {
            ModalState::AgentSelector { selector } => selector.index,
            _ => 0,
        }
    }

    /// Get agent selector filter (from ModalState)
    pub fn agent_selector_filter(&self) -> &str {
        match &self.modal_state {
            ModalState::AgentSelector { selector } => &selector.filter,
            _ => "",
        }
    }

    /// Open the agent selector modal
    pub fn open_agent_selector(&mut self) {
        self.modal_state = ModalState::AgentSelector {
            selector: SelectorState::new(),
        };
    }

    /// Close the agent selector modal
    pub fn close_agent_selector(&mut self) {
        if matches!(self.modal_state, ModalState::AgentSelector { .. }) {
            self.modal_state = ModalState::None;
        }
    }

    /// Get branches filtered by current filter (from ModalState)
    pub fn filtered_branches(&self) -> Vec<String> {
        let filter = match &self.modal_state {
            ModalState::BranchSelector { selector } => selector.filter.to_lowercase(),
            _ => String::new(),
        };
        self.available_branches()
            .into_iter()
            .filter(|b| filter.is_empty() || b.to_lowercase().contains(&filter))
            .collect()
    }

    /// Get branch selector index (from ModalState or legacy)
    pub fn branch_selector_index(&self) -> usize {
        match &self.modal_state {
            ModalState::BranchSelector { selector } => selector.index,
            _ => 0,
        }
    }

    /// Get branch selector filter (from ModalState)
    pub fn branch_selector_filter(&self) -> &str {
        match &self.modal_state {
            ModalState::BranchSelector { selector } => &selector.filter,
            _ => "",
        }
    }

    /// Open the branch selector modal
    pub fn open_branch_selector(&mut self) {
        self.modal_state = ModalState::BranchSelector {
            selector: SelectorState::new(),
        };
    }

    /// Close the branch selector modal
    pub fn close_branch_selector(&mut self) {
        if matches!(self.modal_state, ModalState::BranchSelector { .. }) {
            self.modal_state = ModalState::None;
        }
    }

    /// Select branch by index from filtered branches and close modal
    pub fn select_branch_by_index(&mut self, index: usize) {
        let filtered = self.filtered_branches();
        if let Some(branch) = filtered.get(index) {
            self.selected_branch = Some(branch.clone());
        }
        self.close_branch_selector();
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

    /// Open a thread in a tab (or switch to it if already open)
    /// Returns the tab index
    pub fn open_tab(&mut self, thread: &Thread, project_a_tag: &str) -> usize {
        // Check if already open
        if let Some(idx) = self.open_tabs.iter().position(|t| t.thread_id == thread.id) {
            // Clear unread since we're switching to it
            self.open_tabs[idx].has_unread = false;
            self.active_tab_index = idx;
            return idx;
        }

        // Create new tab
        let tab = OpenTab {
            thread_id: thread.id.clone(),
            thread_title: thread.title.clone(),
            project_a_tag: project_a_tag.to_string(),
            has_unread: false,
        };

        // If at max capacity, remove the oldest (leftmost) tab
        if self.open_tabs.len() >= MAX_TABS {
            self.open_tabs.remove(0);
            // Adjust active index if needed
            if self.active_tab_index > 0 {
                self.active_tab_index -= 1;
            }
        }

        self.open_tabs.push(tab);
        self.active_tab_index = self.open_tabs.len() - 1;
        self.active_tab_index
    }

    /// Close the current tab
    pub fn close_current_tab(&mut self) {
        if self.open_tabs.is_empty() {
            return;
        }

        let removed_index = self.active_tab_index;
        self.open_tabs.remove(removed_index);
        self.cleanup_tab_history(removed_index);

        if self.open_tabs.is_empty() {
            // No more tabs - go back to home view
            self.save_chat_draft();
            self.chat_editor.clear();
            self.selected_thread = None;
            self.view = View::Home;
            self.active_tab_index = 0;
        } else {
            // Move to next tab (or previous if we were at the end)
            if self.active_tab_index >= self.open_tabs.len() {
                self.active_tab_index = self.open_tabs.len() - 1;
            }
            // Switch to the new active tab
            self.switch_to_tab(self.active_tab_index);
        }
    }

    /// Switch to a specific tab by index
    pub fn switch_to_tab(&mut self, index: usize) {
        if index >= self.open_tabs.len() {
            return;
        }

        // Save current draft before switching
        self.save_chat_draft();

        // Track history for Alt+Tab cycling
        self.push_tab_history(index);

        self.active_tab_index = index;

        // Extract data we need before mutating
        let thread_id = self.open_tabs[index].thread_id.clone();
        let project_a_tag = self.open_tabs[index].project_a_tag.clone();

        // Clear unread for this tab
        self.open_tabs[index].has_unread = false;

        // Find the thread in data store
        let thread = self.data_store.borrow().get_threads(&project_a_tag)
            .iter()
            .find(|t| t.id == thread_id)
            .cloned();

        if let Some(thread) = thread {
            self.selected_thread = Some(thread);
            self.restore_chat_draft();
            self.scroll_offset = usize::MAX; // Scroll to bottom
            self.selected_message_index = 0;
            self.subthread_root = None;
            self.subthread_root_message = None;
        }
    }

    /// Push a tab index to history, removing any existing entry for that index
    fn push_tab_history(&mut self, index: usize) {
        // Remove existing entry if present
        self.tab_history.retain(|&i| i != index);
        // Add to end (most recent)
        self.tab_history.push(index);
        // Keep history bounded (max 20 entries)
        if self.tab_history.len() > 20 {
            self.tab_history.remove(0);
        }
    }

    /// Cycle to next tab in history (Alt+Tab behavior)
    pub fn cycle_tab_history_forward(&mut self) {
        if self.tab_history.len() < 2 {
            // Not enough history, just cycle to next tab
            self.next_tab();
            return;
        }

        // Get the second-to-last entry (the previously viewed tab)
        let history_len = self.tab_history.len();
        if history_len >= 2 {
            let prev_index = self.tab_history[history_len - 2];
            if prev_index < self.open_tabs.len() {
                self.switch_to_tab(prev_index);
            }
        }
    }

    /// Cycle to previous tab in history (Alt+Shift+Tab behavior)
    pub fn cycle_tab_history_backward(&mut self) {
        if self.tab_history.len() < 2 {
            // Not enough history, just cycle to prev tab
            self.prev_tab();
            return;
        }

        // Move the current tab to the front of history and switch to what was second-to-last
        // This rotates through history in reverse order
        if let Some(current) = self.tab_history.pop() {
            self.tab_history.insert(0, current);
            if let Some(&next) = self.tab_history.last() {
                if next < self.open_tabs.len() {
                    self.active_tab_index = next;
                    // Re-push to mark as most recent
                    if let Some(idx) = self.tab_history.pop() {
                        self.push_tab_history(idx);
                    }
                }
            }
        }
    }

    /// Clean up tab history when a tab is closed (adjust indices)
    fn cleanup_tab_history(&mut self, removed_index: usize) {
        // Remove the closed tab from history
        self.tab_history.retain(|&i| i != removed_index);
        // Adjust indices for tabs that shifted down
        for idx in self.tab_history.iter_mut() {
            if *idx > removed_index {
                *idx -= 1;
            }
        }
    }

    /// Open tab modal
    pub fn open_tab_modal(&mut self) {
        self.showing_tab_modal = true;
        self.tab_modal_index = self.active_tab_index;
    }

    /// Close tab modal
    pub fn close_tab_modal(&mut self) {
        self.showing_tab_modal = false;
    }

    /// Close tab at specific index (for tab modal)
    pub fn close_tab_at(&mut self, index: usize) {
        if index >= self.open_tabs.len() {
            return;
        }

        self.open_tabs.remove(index);
        self.cleanup_tab_history(index);

        if self.open_tabs.is_empty() {
            // No more tabs - go back to home view
            self.save_chat_draft();
            self.chat_editor.clear();
            self.selected_thread = None;
            self.view = View::Home;
            self.active_tab_index = 0;
        } else {
            // Adjust active tab index if needed
            if self.active_tab_index >= self.open_tabs.len() {
                self.active_tab_index = self.open_tabs.len() - 1;
            } else if self.active_tab_index > index {
                self.active_tab_index -= 1;
            }
            // Adjust modal index if needed
            if self.tab_modal_index >= self.open_tabs.len() {
                self.tab_modal_index = self.open_tabs.len() - 1;
            }
            // If the closed tab was the active one, switch to the new active tab
            if index == self.active_tab_index {
                self.switch_to_tab(self.active_tab_index);
            }
        }
    }

    /// Switch to next tab (Ctrl+Tab)
    pub fn next_tab(&mut self) {
        if self.open_tabs.len() <= 1 {
            return;
        }
        let next = (self.active_tab_index + 1) % self.open_tabs.len();
        self.switch_to_tab(next);
    }

    /// Switch to previous tab (Ctrl+Shift+Tab)
    pub fn prev_tab(&mut self) {
        if self.open_tabs.len() <= 1 {
            return;
        }
        let prev = if self.active_tab_index == 0 {
            self.open_tabs.len() - 1
        } else {
            self.active_tab_index - 1
        };
        self.switch_to_tab(prev);
    }

    /// Mark a thread as having unread messages (if it's open in a tab but not active)
    pub fn mark_tab_unread(&mut self, thread_id: &str) {
        for (idx, tab) in self.open_tabs.iter_mut().enumerate() {
            if tab.thread_id == thread_id && idx != self.active_tab_index {
                tab.has_unread = true;
            }
        }
    }

    // ===== Home View Methods =====

    /// Get recent threads across all projects for Home view (filtered by visible_projects)
    pub fn recent_threads(&self) -> Vec<(Thread, String)> {
        let threads = self.data_store.borrow().get_all_recent_threads(50);
        if self.visible_projects.is_empty() {
            threads
        } else {
            threads.into_iter()
                .filter(|(_, a_tag)| self.visible_projects.contains(a_tag))
                .collect()
        }
    }

    /// Get inbox items for Home view (filtered by visible_projects)
    pub fn inbox_items(&self) -> Vec<crate::models::InboxItem> {
        let items = self.data_store.borrow().get_inbox_items().to_vec();
        if self.visible_projects.is_empty() {
            items
        } else {
            items.into_iter()
                .filter(|item| self.visible_projects.contains(&item.project_a_tag))
                .collect()
        }
    }

    /// Open thread from Home view (recent conversations or inbox)
    pub fn open_thread_from_home(&mut self, thread: &Thread, project_a_tag: &str) {
        // Find and set selected project
        let project = self.data_store.borrow().get_projects()
            .iter()
            .find(|p| p.a_tag() == project_a_tag)
            .cloned();

        if let Some(project) = project {
            self.selected_project = Some(project);

            // Open tab and switch to chat
            self.open_tab(thread, project_a_tag);
            self.selected_thread = Some(thread.clone());
            self.restore_chat_draft();
            self.view = View::Chat;
            self.input_mode = InputMode::Editing;
            self.scroll_offset = usize::MAX;
        }
    }

    // ===== New Thread Modal Methods =====

    /// Open the new thread modal
    pub fn open_new_thread_modal(&mut self) {
        self.showing_new_thread_modal = true;
        self.new_thread_modal_focus = NewThreadField::Content;
        self.new_thread_project_filter.clear();
        self.new_thread_agent_filter.clear();
        self.new_thread_project_index = 0;
        self.new_thread_agent_index = 0;

        // Try to load last used project
        let last_project_a_tag = self.preferences.borrow().last_project().map(|s| s.to_string());

        if let Some(ref a_tag) = last_project_a_tag {
            let project = self.data_store.borrow().get_projects()
                .iter()
                .find(|p| p.a_tag() == *a_tag)
                .cloned();

            if let Some(project) = project {
                self.new_thread_selected_project = Some(project.clone());
                // Auto-select first agent if available
                if let Some(status) = self.data_store.borrow().get_project_status(&project.a_tag()) {
                    self.new_thread_selected_agent = status.agents.first().cloned();
                }
                // Load project draft
                self.restore_project_draft(&project.a_tag());
            }
        }

        // If no last project, try to select first online project
        if self.new_thread_selected_project.is_none() {
            let (online, _) = self.filtered_projects();
            if let Some(project) = online.first() {
                self.new_thread_selected_project = Some(project.clone());
                if let Some(status) = self.data_store.borrow().get_project_status(&project.a_tag()) {
                    self.new_thread_selected_agent = status.agents.first().cloned();
                }
            }
        }

        self.input_mode = InputMode::Editing;
    }

    /// Close the new thread modal, saving draft
    pub fn close_new_thread_modal(&mut self) {
        self.save_project_draft();
        self.showing_new_thread_modal = false;
        self.new_thread_editor.clear();
        self.new_thread_selected_project = None;
        self.new_thread_selected_agent = None;
        self.input_mode = InputMode::Normal;
    }

    /// Save project draft for the current modal state
    fn save_project_draft(&self) {
        if let Some(ref project) = self.new_thread_selected_project {
            let draft = ProjectDraft {
                project_a_tag: project.a_tag(),
                text: self.new_thread_editor.build_full_content(),
                selected_agent_pubkey: self.new_thread_selected_agent.as_ref().map(|a| a.pubkey.clone()),
                last_modified: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
            };
            self.project_draft_storage.borrow_mut().save(draft);
        }
    }

    /// Restore project draft into the modal editor
    fn restore_project_draft(&mut self, project_a_tag: &str) {
        if let Some(draft) = self.project_draft_storage.borrow().load(project_a_tag) {
            self.new_thread_editor.text = draft.text;
            self.new_thread_editor.cursor = self.new_thread_editor.text.len();
            if let Some(agent_pubkey) = draft.selected_agent_pubkey {
                if let Some(status) = self.data_store.borrow().get_project_status(project_a_tag) {
                    self.new_thread_selected_agent = status
                        .agents
                        .iter()
                        .find(|a| a.pubkey == agent_pubkey)
                        .cloned();
                }
            }
        }
    }

    /// Delete project draft (after sending)
    pub fn delete_project_draft(&self, project_a_tag: &str) {
        self.project_draft_storage.borrow_mut().delete(project_a_tag);
    }

    /// Set last used project preference
    pub fn set_last_project(&self, project_a_tag: &str) {
        self.preferences.borrow_mut().set_last_project(project_a_tag);
    }

    /// Cycle to next field in new thread modal
    pub fn new_thread_modal_next_field(&mut self) {
        // Save draft when switching away from project
        if self.new_thread_modal_focus == NewThreadField::Project {
            self.save_project_draft();
        }

        self.new_thread_modal_focus = match self.new_thread_modal_focus {
            NewThreadField::Content => NewThreadField::Project,
            NewThreadField::Project => NewThreadField::Agent,
            NewThreadField::Agent => NewThreadField::Content,
        };

        // Clear filter when entering a selector
        match self.new_thread_modal_focus {
            NewThreadField::Project => self.new_thread_project_filter.clear(),
            NewThreadField::Agent => self.new_thread_agent_filter.clear(),
            NewThreadField::Content => {}
        }
    }

    /// Get filtered projects for the new thread modal
    pub fn new_thread_filtered_projects(&self) -> Vec<Project> {
        let filter = self.new_thread_project_filter.to_lowercase();
        let store = self.data_store.borrow();
        let projects = store.get_projects();

        projects
            .iter()
            .filter(|p| filter.is_empty() || p.name.to_lowercase().contains(&filter))
            .filter(|p| store.is_project_online(&p.a_tag()))
            .cloned()
            .collect()
    }

    /// Get filtered agents for the new thread modal
    pub fn new_thread_filtered_agents(&self) -> Vec<ProjectAgent> {
        let filter = self.new_thread_agent_filter.to_lowercase();
        self.new_thread_selected_project
            .as_ref()
            .and_then(|p| {
                self.data_store
                    .borrow()
                    .get_project_status(&p.a_tag())
                    .map(|s| {
                        s.agents
                            .iter()
                            .filter(|a| filter.is_empty() || a.name.to_lowercase().contains(&filter))
                            .cloned()
                            .collect()
                    })
            })
            .unwrap_or_default()
    }

    /// Select a project in the new thread modal
    pub fn new_thread_select_project(&mut self, project: Project) {
        // Save draft for old project
        self.save_project_draft();

        let a_tag = project.a_tag();
        self.new_thread_selected_project = Some(project);
        self.new_thread_selected_agent = None;

        // Auto-select first agent
        if let Some(status) = self.data_store.borrow().get_project_status(&a_tag) {
            self.new_thread_selected_agent = status.agents.first().cloned();
        }

        // Load draft for new project
        self.restore_project_draft(&a_tag);

        // Move to next field
        self.new_thread_modal_focus = NewThreadField::Agent;
        self.new_thread_agent_filter.clear();
    }

    /// Select an agent in the new thread modal
    pub fn new_thread_select_agent(&mut self, agent: ProjectAgent) {
        self.new_thread_selected_agent = Some(agent);
        self.new_thread_modal_focus = NewThreadField::Content;
    }

    /// Check if the new thread modal can submit
    pub fn can_submit_new_thread(&self) -> bool {
        self.new_thread_selected_project.is_some()
            && self.new_thread_selected_agent.is_some()
            && !self.new_thread_editor.text.trim().is_empty()
    }

    /// Get all image URLs from messages in the current thread
    pub fn get_image_urls_from_thread(&self) -> Vec<String> {
        let messages = self.messages();
        let mut urls = Vec::new();

        for msg in &messages {
            if msg.has_images() {
                urls.extend(msg.extract_image_urls());
            }
        }

        urls
    }

    /// Open an image URL in the system default viewer
    pub fn open_image_in_viewer(&self, url: &str) -> Result<(), String> {
        use std::process::Command;

        #[cfg(target_os = "macos")]
        let cmd = "open";
        #[cfg(target_os = "linux")]
        let cmd = "xdg-open";
        #[cfg(target_os = "windows")]
        let cmd = "start";

        Command::new(cmd)
            .arg(url)
            .spawn()
            .map_err(|e| format!("Failed to open image: {}", e))?;

        Ok(())
    }

    /// Open the first image from the current thread in the system viewer
    pub fn open_first_image(&mut self) {
        let urls = self.get_image_urls_from_thread();

        if urls.is_empty() {
            self.set_status("No images in current conversation");
            return;
        }

        match self.open_image_in_viewer(&urls[0]) {
            Ok(_) => {
                self.set_status(&format!("Opening image in viewer..."));
            }
            Err(e) => {
                self.set_status(&e);
            }
        }
    }

    /// Open ask UI inline (replacing input box)
    pub fn open_ask_modal(&mut self, message_id: String, ask_event: AskEvent) {
        let input_state = AskInputState::new(ask_event.questions.clone());
        self.ask_modal_state = Some(AskModalState {
            message_id,
            ask_event,
            input_state,
        });
        self.input_mode = InputMode::Normal;
    }

    /// Close ask UI and return to normal input
    pub fn close_ask_modal(&mut self) {
        self.ask_modal_state = None;
        self.input_mode = InputMode::Editing;
    }

    /// Check for unanswered ask events in current thread
    /// Returns the first unanswered ask event found (not answered by current user)
    pub fn has_unanswered_ask_event(&self) -> Option<(String, AskEvent)> {
        let messages = self.messages();
        let thread_id = self.selected_thread.as_ref().map(|t| t.id.as_str())?;

        // Get current user's pubkey - if no user, can't answer questions
        let user_pubkey = self.data_store.borrow().user_pubkey.clone()?;

        // Get all message IDs that have been replied to BY THE CURRENT USER
        let mut replied_to_by_user: std::collections::HashSet<&str> = std::collections::HashSet::new();
        for msg in &messages {
            // Only count replies from the current user
            if msg.pubkey == user_pubkey {
                if let Some(ref reply_to) = msg.reply_to {
                    replied_to_by_user.insert(reply_to.as_str());
                }
            }
        }

        // Find first ask event that hasn't been replied to by current user
        let display_messages: Vec<&Message> = if let Some(ref root_id) = self.subthread_root {
            messages.iter()
                .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                .collect()
        } else {
            messages.iter()
                .filter(|m| m.reply_to.is_none() || m.reply_to.as_deref() == Some(thread_id))
                .collect()
        };

        for msg in display_messages {
            if let Some(ref ask_event) = msg.ask_event {
                // Check if this message has been replied to by current user
                if !replied_to_by_user.contains(msg.id.as_str()) {
                    return Some((msg.id.clone(), ask_event.clone()));
                }
            }
        }

        None
    }

    /// Check if a specific message's ask event has been answered by the current user
    pub fn is_ask_answered_by_user(&self, message_id: &str) -> bool {
        let messages = self.messages();

        // Get current user's pubkey
        let user_pubkey = match self.data_store.borrow().user_pubkey.clone() {
            Some(pk) => pk,
            None => return false,
        };

        // Check if there's a reply from current user to this message
        for msg in &messages {
            if msg.pubkey == user_pubkey {
                if let Some(ref reply_to) = msg.reply_to {
                    if reply_to == message_id {
                        return true;
                    }
                }
            }
        }

        false
    }

    // ===== Local Streaming Methods =====

    /// Get streaming content for current conversation
    #[allow(dead_code)]
    pub fn local_streaming_content(&self) -> Option<&LocalStreamBuffer> {
        let conv_id = self.current_conversation_id()?;
        self.local_stream_buffers.get(&conv_id)
    }

    /// Update streaming buffer from local chunk
    pub fn handle_local_stream_chunk(
        &mut self,
        agent_pubkey: String,
        conversation_id: String,
        text_delta: Option<String>,
        reasoning_delta: Option<String>,
        is_finish: bool,
    ) {
        let buffer = self.local_stream_buffers
            .entry(conversation_id)
            .or_default();

        buffer.agent_pubkey = agent_pubkey;
        if let Some(delta) = text_delta {
            buffer.text_content.push_str(&delta);
        }
        if let Some(delta) = reasoning_delta {
            buffer.reasoning_content.push_str(&delta);
        }
        if is_finish {
            buffer.is_complete = true;
        }
    }

    /// Clear the local stream buffer for a conversation
    pub fn clear_local_stream_buffer(&mut self, conversation_id: &str) {
        self.local_stream_buffers.remove(conversation_id);
    }

    /// Get current conversation ID (thread ID)
    pub fn current_conversation_id(&self) -> Option<String> {
        self.selected_thread.as_ref().map(|t| t.id.clone())
    }
}
