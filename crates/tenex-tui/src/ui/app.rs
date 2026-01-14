use crate::models::{AskEvent, ChatDraft, DraftStorage, Message, PreferencesStorage, Project, ProjectAgent, ProjectStatus, Thread, TimeFilter};
use crate::nostr::DataChange;
use crate::store::{AppDataStore, Database};
use crate::ui::ask_input::AskInputState;
use crate::ui::modal::{CommandPaletteState, ModalState, PaletteContext};
use crate::ui::notifications::{Notification, NotificationQueue};
use crate::ui::selector::SelectorState;
use crate::ui::text_editor::TextEditor;
use nostr_sdk::Keys;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use tenex_core::runtime::CoreHandle;

/// Fuzzy match: all chars in pattern must appear in target in order (case-insensitive)
pub fn fuzzy_matches(target: &str, pattern: &str) -> bool {
    if pattern.is_empty() {
        return true;
    }
    let target_lower = target.to_lowercase();
    let pattern_lower = pattern.to_lowercase();
    let mut pattern_chars = pattern_lower.chars().peekable();

    for c in target_lower.chars() {
        if pattern_chars.peek() == Some(&c) {
            pattern_chars.next();
            if pattern_chars.peek().is_none() {
                return true;
            }
        }
    }
    false
}

/// Fuzzy match score: lower is better (0 = exact prefix match)
/// Returns None if no match, Some(score) if match
/// Scoring: prefix matches get 0, then +1 per position after start, +1 per gap
pub fn fuzzy_score(target: &str, pattern: &str) -> Option<usize> {
    if pattern.is_empty() {
        return Some(0);
    }
    let target_lower = target.to_lowercase();
    let pattern_lower = pattern.to_lowercase();
    let target_chars: Vec<char> = target_lower.chars().collect();
    let pattern_chars: Vec<char> = pattern_lower.chars().collect();

    let mut pattern_idx = 0;
    let mut first_match_pos = None;
    let mut total_gaps = 0;
    let mut last_match_pos: Option<usize> = None;

    for (i, &c) in target_chars.iter().enumerate() {
        if pattern_idx < pattern_chars.len() && c == pattern_chars[pattern_idx] {
            if first_match_pos.is_none() {
                first_match_pos = Some(i);
            }
            if let Some(last) = last_match_pos {
                total_gaps += i - last - 1;
            }
            last_match_pos = Some(i);
            pattern_idx += 1;
        }
    }

    if pattern_idx == pattern_chars.len() {
        // Score: position of first match + total gaps between matches
        Some(first_match_pos.unwrap_or(0) + total_gaps)
    } else {
        None
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Login,
    Home,
    Chat,
    LessonViewer,
    AgentBrowser,
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
    Reports,
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

/// Buffer for local streaming content (per conversation)
#[derive(Default, Clone)]
pub struct LocalStreamBuffer {
    pub agent_pubkey: String,
    pub text_content: String,
    pub reasoning_content: String,
    pub is_complete: bool,
}

/// Vim mode states
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum VimMode {
    #[default]
    Normal,
    Insert,
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
    /// Notification queue for toast/status messages
    pub notifications: NotificationQueue,

    pub creating_thread: bool,
    pub selected_branch: Option<String>,

    pub core_handle: Option<CoreHandle>,
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

    /// Current wrap width for chat input (updated during rendering for visual line navigation)
    pub chat_input_wrap_width: usize,

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
    pub selected_report_index: usize,
    pub report_search_filter: String,
    pub selected_recent_index: usize,
    /// Whether sidebar is focused (vs content area)
    pub sidebar_focused: bool,
    /// Selected index in sidebar project list
    pub sidebar_project_index: usize,
    /// Projects to show in Recent/Inbox (empty = none)
    pub visible_projects: HashSet<String>,
    /// Filter to show only threads created by or p-tagging current user
    pub only_by_me: bool,
    /// Filter by time since last activity
    pub time_filter: Option<TimeFilter>,

    preferences: RefCell<PreferencesStorage>,

    /// Unified modal state
    pub modal_state: ModalState,

    // Lesson viewer state
    pub viewing_lesson_id: Option<String>,
    pub lesson_viewer_section: usize,

    // Agent browser state
    pub agent_browser_index: usize,
    pub agent_browser_filter: String,
    pub agent_browser_in_detail: bool,
    pub viewing_agent_id: Option<String>,

    // Search modal state
    pub showing_search_modal: bool,
    pub search_filter: String,
    pub search_index: usize,

    /// Local streaming buffers by conversation_id
    pub local_stream_buffers: HashMap<String, LocalStreamBuffer>,

    /// Toggle for showing/hiding LLM metadata on messages (model, tokens, cost)
    pub show_llm_metadata: bool,

    /// Toggle for showing/hiding the todo sidebar
    pub todo_sidebar_visible: bool,

    /// Collapsed thread IDs (parent threads whose children are hidden)
    pub collapsed_threads: HashSet<String>,

    /// Expanded message groups (group key = first message ID)
    /// When a group is in this set, all collapsed messages are shown
    pub expanded_groups: HashSet<String>,

    /// Project a_tag when waiting for a newly created thread to appear
    pub pending_new_thread_project: Option<String>,

    /// Selected nudge IDs for the current conversation
    pub selected_nudge_ids: Vec<String>,

    /// Frame counter for animations (incremented on each tick)
    pub frame_counter: u64,

    /// Sent message history for ↑/↓ navigation (max 50)
    pub message_history: Vec<String>,
    /// Current index in message history (None = typing new)
    pub history_index: Option<usize>,
    /// Draft preserved when browsing history
    pub history_draft: Option<String>,

    /// Whether vim mode is enabled
    pub vim_enabled: bool,
    /// Current vim mode (Normal or Insert)
    pub vim_mode: VimMode,
    /// Whether to show archived conversations in Recent/Inbox
    pub show_archived: bool,
    /// Whether user explicitly selected an agent in the current conversation
    /// When true, don't auto-sync agent from conversation messages
    pub user_explicitly_selected_agent: bool,
}

impl App {
    pub fn new(
        db: Arc<Database>,
        data_store: Rc<RefCell<AppDataStore>>,
    ) -> Self {
        Self {
            running: true,
            view: View::Login,
            input_mode: InputMode::Normal,
            input: String::new(),
            cursor_position: 0,

            db,
            keys: None,

            selected_project: None,
            selected_thread: None,
            selected_agent: None,

            scroll_offset: 0,
            max_scroll_offset: 0,
            notifications: NotificationQueue::new(),

            creating_thread: false,
            selected_branch: None,

            core_handle: None,
            data_rx: None,

            pending_quit: false,
            draft_storage: RefCell::new(DraftStorage::new("tenex_data")),
            chat_editor: TextEditor::new(),
            showing_attachment_modal: false,
            attachment_modal_editor: TextEditor::new(),
            chat_input_wrap_width: 80, // Default, updated during rendering
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
            selected_report_index: 0,
            report_search_filter: String::new(),
            selected_recent_index: 0,
            sidebar_focused: false,
            sidebar_project_index: 0,
            visible_projects: HashSet::new(),
            only_by_me: false,
            time_filter: None,
            preferences: RefCell::new(PreferencesStorage::new("tenex_data")),
            modal_state: ModalState::None,
            viewing_lesson_id: None,
            lesson_viewer_section: 0,
            agent_browser_index: 0,
            agent_browser_filter: String::new(),
            agent_browser_in_detail: false,
            viewing_agent_id: None,
            showing_search_modal: false,
            search_filter: String::new(),
            search_index: 0,
            local_stream_buffers: HashMap::new(),
            show_llm_metadata: false,
            todo_sidebar_visible: true,
            collapsed_threads: HashSet::new(),
            expanded_groups: HashSet::new(),
            pending_new_thread_project: None,
            selected_nudge_ids: Vec::new(),
            frame_counter: 0,
            message_history: Vec::new(),
            history_index: None,
            history_draft: None,
            vim_enabled: false,
            vim_mode: VimMode::Normal,
            show_archived: false,
            user_explicitly_selected_agent: false,
        }
    }

    /// Increment frame counter and update notifications (call on each tick)
    pub fn tick(&mut self) {
        self.frame_counter = self.frame_counter.wrapping_add(1);
        self.notifications.tick();
    }

    /// Get spinner character based on frame counter
    pub fn spinner_char(&self) -> char {
        const SPINNERS: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
        // Divide by 2 to slow down the animation (every 2 frames = ~200ms at 10fps)
        SPINNERS[(self.frame_counter / 2) as usize % SPINNERS.len()]
    }

    /// Toggle collapse state for a thread (for hierarchical folding)
    pub fn toggle_thread_collapse(&mut self, thread_id: &str) {
        if self.collapsed_threads.contains(thread_id) {
            self.collapsed_threads.remove(thread_id);
        } else {
            self.collapsed_threads.insert(thread_id.to_string());
        }
    }

    /// Toggle expansion for a message group (group key = first message ID)
    pub fn toggle_group_expansion(&mut self, group_key: &str) {
        if self.expanded_groups.contains(group_key) {
            self.expanded_groups.remove(group_key);
        } else {
            self.expanded_groups.insert(group_key.to_string());
        }
    }

    /// Check if a message group is expanded
    pub fn is_group_expanded(&self, group_key: &str) -> bool {
        self.expanded_groups.contains(group_key)
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
    /// Also syncs agent/branch selection with the conversation's most recent activity
    pub fn restore_chat_draft(&mut self) {
        // Reset explicit selection flag when switching conversations
        self.user_explicitly_selected_agent = false;

        if let Some(ref thread) = self.selected_thread {
            // Load draft for text content and branch
            if let Some(draft) = self.draft_storage.borrow().load(&thread.id) {
                self.chat_editor.text = draft.text;
                self.chat_editor.cursor = self.chat_editor.text.len();
                self.selected_branch = draft.selected_branch;
            }

            // Always sync agent selection with the conversation's most recent agent
            // This ensures the input always reflects who you're talking to
            self.sync_agent_with_conversation();
        }
    }

    /// Sync selected_agent with the most recent agent in the conversation
    /// Falls back to PM agent if no agent has responded yet
    pub fn sync_agent_with_conversation(&mut self) {
        // First try to get the most recent agent from the conversation
        if let Some(recent_agent) = self.get_most_recent_agent_from_conversation() {
            self.selected_agent = Some(recent_agent);
            return;
        }

        // Fall back to PM agent if no agent has responded yet
        if let Some(status) = self.get_selected_project_status() {
            if let Some(pm) = status.pm_agent() {
                self.selected_agent = Some(pm.clone());
            }
        }
    }

    /// Delete draft for the selected thread (call after sending message)
    pub fn delete_chat_draft(&self) {
        if let Some(ref thread) = self.selected_thread {
            self.draft_storage.borrow_mut().delete(&thread.id);
        }
    }

    /// Check if attachment modal is open
    pub fn is_attachment_modal_open(&self) -> bool {
        self.showing_attachment_modal
    }

    /// Get reference to the attachment modal editor
    pub fn attachment_modal_editor(&self) -> &TextEditor {
        &self.attachment_modal_editor
    }

    /// Get mutable reference to the attachment modal editor
    pub fn attachment_modal_editor_mut(&mut self) -> &mut TextEditor {
        &mut self.attachment_modal_editor
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

    /// Open expanded editor modal (Ctrl+E) for full-screen editing
    pub fn open_expanded_editor_modal(&mut self) {
        let mut editor = TextEditor::new();
        editor.text = self.chat_editor.text.clone();
        editor.cursor = self.chat_editor.cursor;
        self.modal_state = ModalState::ExpandedEditor { editor };
    }

    /// Save expanded editor changes and close
    pub fn save_and_close_expanded_editor(&mut self) {
        if let ModalState::ExpandedEditor { editor } = &self.modal_state {
            self.chat_editor.text = editor.text.clone();
            self.chat_editor.cursor = editor.cursor;
            self.save_chat_draft();
        }
        self.modal_state = ModalState::None;
    }

    /// Cancel expanded editor without saving
    pub fn cancel_expanded_editor(&mut self) {
        self.modal_state = ModalState::None;
    }

    /// Get mutable reference to expanded editor (if open)
    pub fn expanded_editor_mut(&mut self) -> Option<&mut TextEditor> {
        if let ModalState::ExpandedEditor { editor } = &mut self.modal_state {
            Some(editor)
        } else {
            None
        }
    }

    /// Get filtered projects based on current filter (from ModalState)
    /// Results are sorted by match quality (prefix matches first, then by gap count)
    pub fn filtered_projects(&self) -> (Vec<Project>, Vec<Project>) {
        let filter = self.projects_modal_filter();
        let store = self.data_store.borrow();
        let projects = store.get_projects();

        let mut matching: Vec<_> = projects
            .iter()
            .filter_map(|p| fuzzy_score(&p.name, filter).map(|score| (p, score)))
            .collect();

        // Sort by score (lower = better match), then alphabetically for ties
        matching.sort_by(|(a, score_a), (b, score_b)| {
            score_a.cmp(score_b).then_with(|| a.name.cmp(&b.name))
        });

        // Separate into online and offline, preserving sort order
        let (online, offline): (Vec<_>, Vec<_>) = matching
            .into_iter()
            .map(|(p, _)| p)
            .partition(|p| store.is_project_online(&p.a_tag()));

        (online.into_iter().cloned().collect(), offline.into_iter().cloned().collect())
    }

    /// Open the projects modal
    /// If `for_new_thread` is true, selecting a project navigates to chat view
    pub fn open_projects_modal(&mut self, for_new_thread: bool) {
        self.modal_state = ModalState::ProjectsModal {
            selector: SelectorState::new(),
            for_new_thread,
        };
    }

    /// Get projects modal index (from ModalState)
    pub fn projects_modal_index(&self) -> usize {
        match &self.modal_state {
            ModalState::ProjectsModal { selector, .. } => selector.index,
            _ => 0,
        }
    }

    /// Get projects modal filter (from ModalState)
    pub fn projects_modal_filter(&self) -> &str {
        match &self.modal_state {
            ModalState::ProjectsModal { selector, .. } => &selector.filter,
            _ => "",
        }
    }

    pub fn set_core_handle(&mut self, core_handle: CoreHandle, data_rx: Receiver<DataChange>) {
        self.core_handle = Some(core_handle);
        self.data_rx = Some(data_rx);
    }

    /// Process local streaming chunks from the worker channel.
    /// All other updates are handled via the core runtime's nostrdb subscription.
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

    /// Add a notification to the queue
    pub fn notify(&mut self, notification: Notification) {
        self.notifications.push(notification);
    }

    /// Convenience: set a warning status message (legacy compatibility)
    /// Prefer using notify() with specific notification types for new code
    pub fn set_status(&mut self, msg: &str) {
        self.notifications.push(Notification::warning(msg));
    }

    /// Dismiss the current notification
    pub fn dismiss_notification(&mut self) {
        self.notifications.dismiss();
    }

    /// Get the current notification message (for display)
    pub fn current_notification(&self) -> Option<&Notification> {
        self.notifications.current()
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

    /// Get the most recent agent that published a message in the current conversation.
    /// Returns the agent from available_agents whose pubkey matches the most recent
    /// non-user message in the conversation.
    pub fn get_most_recent_agent_from_conversation(&self) -> Option<crate::models::ProjectAgent> {
        let thread = self.selected_thread.as_ref()?;
        let messages = self.messages();
        let available_agents = self.available_agents();
        let user_pubkey = self.data_store.borrow().user_pubkey.clone();

        // Create a set of agent pubkeys for quick lookup
        let agent_pubkeys: std::collections::HashSet<&str> = available_agents
            .iter()
            .map(|a| a.pubkey.as_str())
            .collect();

        // Find the most recent message from an agent (not the user)
        // Messages are typically sorted by created_at, but we'll iterate and track the latest
        let mut latest_agent_pubkey: Option<&str> = None;
        let mut latest_timestamp: u64 = 0;

        for msg in &messages {
            // Skip messages from the user
            if user_pubkey.as_ref().map(|pk| pk == &msg.pubkey).unwrap_or(false) {
                continue;
            }

            // Check if this message is from a known agent
            if agent_pubkeys.contains(msg.pubkey.as_str()) && msg.created_at >= latest_timestamp {
                latest_timestamp = msg.created_at;
                latest_agent_pubkey = Some(msg.pubkey.as_str());
            }
        }

        // Also check the thread itself (the original message that started the thread)
        // The thread author might be an agent - use last_activity as timestamp proxy
        // Note: for the thread root, we only consider it if no messages from agents exist yet
        if latest_agent_pubkey.is_none() && agent_pubkeys.contains(thread.pubkey.as_str()) {
            if user_pubkey.as_ref().map(|pk| pk != &thread.pubkey).unwrap_or(true) {
                latest_agent_pubkey = Some(thread.pubkey.as_str());
            }
        }

        // Find and return the matching agent
        latest_agent_pubkey.and_then(|pubkey| {
            available_agents.into_iter().find(|a| a.pubkey == pubkey)
        })
    }

    /// Get agents filtered by current filter (from ModalState or empty)
    /// Results are sorted by match quality (prefix matches first, then by gap count)
    pub fn filtered_agents(&self) -> Vec<crate::models::ProjectAgent> {
        let filter = match &self.modal_state {
            ModalState::AgentSelector { selector } => &selector.filter,
            _ => "",
        };
        let mut agents_with_scores: Vec<_> = self.available_agents()
            .into_iter()
            .filter_map(|a| fuzzy_score(&a.name, filter).map(|score| (a, score)))
            .collect();
        // Sort by score (lower = better match), then alphabetically for ties
        agents_with_scores.sort_by(|(a, score_a), (b, score_b)| {
            score_a.cmp(score_b).then_with(|| a.name.cmp(&b.name))
        });
        agents_with_scores.into_iter().map(|(a, _)| a).collect()
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
    /// Results are sorted by match quality (prefix matches first, then by gap count)
    pub fn filtered_branches(&self) -> Vec<String> {
        let filter = match &self.modal_state {
            ModalState::BranchSelector { selector } => &selector.filter,
            _ => "",
        };
        let mut branches_with_scores: Vec<_> = self.available_branches()
            .into_iter()
            .filter_map(|b| fuzzy_score(&b, filter).map(|score| (b, score)))
            .collect();
        // Sort by score (lower = better match), then alphabetically for ties
        branches_with_scores.sort_by(|(a, score_a), (b, score_b)| {
            score_a.cmp(score_b).then_with(|| a.cmp(b))
        });
        branches_with_scores.into_iter().map(|(b, _)| b).collect()
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

    /// Open the command palette modal (Ctrl+T)
    pub fn open_command_palette(&mut self) {
        let context = self.get_palette_context();
        self.modal_state = ModalState::CommandPalette(CommandPaletteState::new(context));
    }

    /// Get the current context for the command palette
    fn get_palette_context(&self) -> PaletteContext {
        match self.view {
            View::Home => {
                if self.sidebar_focused {
                    // Check if selected project is online/busy using filtered_projects
                    let (online, offline) = self.filtered_projects();
                    let online_count = online.len();
                    let is_online = self.sidebar_project_index < online_count;

                    // Get the actual project to check busy status
                    let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
                    let is_busy = all_projects.get(self.sidebar_project_index)
                        .map(|p| self.data_store.borrow().is_project_busy(&p.a_tag()))
                        .unwrap_or(false);

                    PaletteContext::HomeSidebar { is_online, is_busy }
                } else {
                    match self.home_panel_focus {
                        HomeTab::Recent => PaletteContext::HomeRecent,
                        HomeTab::Inbox => PaletteContext::HomeInbox,
                        HomeTab::Reports => PaletteContext::HomeReports,
                    }
                }
            }
            View::Chat => {
                if self.input_mode == InputMode::Editing {
                    PaletteContext::ChatEditing
                } else {
                    // Check context for normal mode
                    let has_parent = self.selected_thread.as_ref()
                        .and_then(|t| t.parent_conversation_id.as_ref())
                        .is_some();

                    // Check if selected message has trace
                    let has_trace = false; // Simplified for now

                    // Check if any agent is working on this thread
                    let agent_working = self.selected_thread.as_ref()
                        .map(|t| self.data_store.borrow().is_event_busy(&t.id))
                        .unwrap_or(false);

                    PaletteContext::ChatNormal { has_parent, has_trace, agent_working }
                }
            }
            View::AgentBrowser => {
                if self.agent_browser_in_detail {
                    PaletteContext::AgentBrowserDetail
                } else {
                    PaletteContext::AgentBrowserList
                }
            }
            _ => PaletteContext::HomeRecent, // Default fallback
        }
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

    /// Open the message actions modal for the currently selected message
    pub fn open_message_actions_modal(&mut self) {
        use crate::store::get_trace_context;

        let messages = self.messages();
        let thread_id = self.selected_thread.as_ref().map(|t| t.id.as_str());

        // Get display messages based on current view (subthread or main)
        let display_messages: Vec<&Message> = if let Some(ref root_id) = self.subthread_root {
            messages
                .iter()
                .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                .collect()
        } else {
            // Include thread root + direct replies
            messages
                .iter()
                .filter(|m| {
                    Some(m.id.as_str()) == thread_id
                        || m.reply_to.is_none()
                        || m.reply_to.as_deref() == thread_id
                })
                .collect()
        };

        if let Some(msg) = display_messages.get(self.selected_message_index) {
            let message_id = msg.id.clone();
            // Check if trace context exists for this message
            let has_trace = get_trace_context(&self.db.ndb, &message_id).is_some();

            self.modal_state = ModalState::MessageActions {
                message_id,
                selected_index: 0,
                has_trace,
            };
        }
    }

    /// Execute a message action
    pub fn execute_message_action(
        &mut self,
        message_id: &str,
        action: crate::ui::modal::MessageAction,
    ) {
        use crate::store::{get_raw_event_json, get_trace_context};
        use crate::ui::modal::MessageAction;

        match action {
            MessageAction::CopyRawEvent => {
                if let Some(json) = get_raw_event_json(&self.db.ndb, message_id) {
                    self.copy_to_clipboard(&json);
                    self.notify(Notification::success("Raw event copied to clipboard"));
                } else {
                    self.notify(Notification::error("Failed to get raw event"));
                }
                self.modal_state = ModalState::None;
            }
            MessageAction::ViewRawEvent => {
                if let Some(json) = get_raw_event_json(&self.db.ndb, message_id) {
                    // Pretty print the JSON
                    let pretty_json = if let Ok(value) = serde_json::from_str::<serde_json::Value>(&json) {
                        serde_json::to_string_pretty(&value).unwrap_or(json)
                    } else {
                        json
                    };

                    self.modal_state = ModalState::ViewRawEvent {
                        message_id: message_id.to_string(),
                        json: pretty_json,
                        scroll_offset: 0,
                    };
                } else {
                    self.notify(Notification::error("Failed to get raw event"));
                    self.modal_state = ModalState::None;
                }
            }
            MessageAction::OpenTrace => {
                if let Some(trace_info) = get_trace_context(&self.db.ndb, message_id) {
                    let url = format!(
                        "http://localhost:16686/trace/{}?uiFind={}",
                        trace_info.trace_id, trace_info.span_id
                    );
                    self.open_url(&url);
                    self.notify(Notification::info("Opening trace in browser..."));
                } else {
                    self.notify(Notification::warning("No trace context found for this message"));
                }
                self.modal_state = ModalState::None;
            }
            MessageAction::SendAgain => {
                // Get the original message content
                let messages = self.messages();
                if let Some(msg) = messages.iter().find(|m| m.id == message_id) {
                    let content = msg.content.clone();

                    // Create a new thread with the same content
                    if let (Some(ref core_handle), Some(ref project)) =
                        (&self.core_handle, &self.selected_project)
                    {
                        use crate::nostr::NostrCommand;

                        let title = content.lines().next().unwrap_or("New Thread").to_string();
                        let project_a_tag = project.a_tag();
                        let agent_pubkey = self.selected_agent.as_ref().map(|a| a.pubkey.clone());
                        let branch = self.selected_branch.clone();

                        if let Err(e) = core_handle.send(NostrCommand::PublishThread {
                            project_a_tag,
                            title,
                            content,
                            agent_pubkey,
                            branch,
                            nudge_ids: vec![],
                        }) {
                            self.notify(Notification::error(format!("Failed to create thread: {}", e)));
                        } else {
                            self.notify(Notification::info("Creating new conversation..."));
                        }
                    }
                }
                self.modal_state = ModalState::None;
            }
        }
    }

    /// Copy text to clipboard
    fn copy_to_clipboard(&self, text: &str) {
        use arboard::Clipboard;
        if let Ok(mut clipboard) = Clipboard::new() {
            let _ = clipboard.set_text(text);
        }
    }

    /// Open a URL in the default browser
    fn open_url(&self, url: &str) {
        #[cfg(target_os = "macos")]
        {
            let _ = std::process::Command::new("open").arg(url).spawn();
        }
        #[cfg(target_os = "linux")]
        {
            let _ = std::process::Command::new("xdg-open").arg(url).spawn();
        }
        #[cfg(target_os = "windows")]
        {
            let _ = std::process::Command::new("cmd")
                .args(["/c", "start", url])
                .spawn();
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
            self.input_mode = InputMode::Editing; // Auto-focus input
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

    /// Get recent threads across all projects for Home view (filtered by visible_projects, only_by_me, time_filter, archived)
    pub fn recent_threads(&self) -> Vec<(Thread, String)> {
        // Empty visible_projects = show nothing (inverted default)
        if self.visible_projects.is_empty() {
            return vec![];
        }

        let threads = self.data_store.borrow().get_all_recent_threads(50);
        let user_pubkey = self.data_store.borrow().user_pubkey.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let prefs = self.preferences.borrow();

        threads.into_iter()
            // Project filter
            .filter(|(_, a_tag)| self.visible_projects.contains(a_tag))
            // Archive filter - hide archived unless show_archived is true
            .filter(|(thread, _)| {
                self.show_archived || !prefs.is_thread_archived(&thread.id)
            })
            // "Only by me" filter
            .filter(|(thread, _)| {
                if !self.only_by_me {
                    return true;
                }
                user_pubkey.as_ref().map_or(false, |pk| thread.involves_user(pk))
            })
            // Time filter
            .filter(|(thread, _)| {
                if let Some(ref tf) = self.time_filter {
                    let cutoff = now.saturating_sub(tf.seconds());
                    thread.last_activity >= cutoff
                } else {
                    true
                }
            })
            .collect()
    }

    /// Get inbox items for Home view (filtered by visible_projects, only_by_me, time_filter, archived)
    pub fn inbox_items(&self) -> Vec<crate::models::InboxItem> {
        // Empty visible_projects = show nothing (inverted default)
        if self.visible_projects.is_empty() {
            return vec![];
        }

        let items = self.data_store.borrow().get_inbox_items().to_vec();
        let user_pubkey = self.data_store.borrow().user_pubkey.clone();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let prefs = self.preferences.borrow();

        items.into_iter()
            // Project filter
            .filter(|item| self.visible_projects.contains(&item.project_a_tag))
            // Archive filter - hide items from archived threads unless show_archived is true
            .filter(|item| {
                if let Some(ref thread_id) = item.thread_id {
                    self.show_archived || !prefs.is_thread_archived(thread_id)
                } else {
                    true  // Keep items without thread_id
                }
            })
            // "Only by me" filter - based on author_pubkey
            .filter(|item| {
                if !self.only_by_me {
                    return true;
                }
                user_pubkey.as_ref().map_or(false, |pk| &item.author_pubkey == pk)
            })
            // Time filter
            .filter(|item| {
                if let Some(ref tf) = self.time_filter {
                    let cutoff = now.saturating_sub(tf.seconds());
                    item.created_at >= cutoff
                } else {
                    true
                }
            })
            .collect()
    }

    /// Get reports for Home view (filtered by visible_projects and search filter)
    pub fn reports(&self) -> Vec<tenex_core::models::Report> {
        // Empty visible_projects = show nothing
        if self.visible_projects.is_empty() {
            return vec![];
        }

        let store = self.data_store.borrow();
        let filter = self.report_search_filter.to_lowercase();

        store.get_reports()
            .into_iter()
            .filter(|r| self.visible_projects.contains(&r.project_a_tag))
            .filter(|r| {
                if filter.is_empty() {
                    return true;
                }
                r.title.to_lowercase().contains(&filter)
                    || r.summary.to_lowercase().contains(&filter)
                    || r.content.to_lowercase().contains(&filter)
                    || r.hashtags.iter().any(|h| h.to_lowercase().contains(&filter))
            })
            .cloned()
            .collect()
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

    /// Start a new thread for a specific project (navigates to chat without a thread selected)
    pub fn start_new_thread_for_project(&mut self, project_a_tag: &str) {
        // Find and set selected project
        let project = self.data_store.borrow().get_projects()
            .iter()
            .find(|p| p.a_tag() == project_a_tag)
            .cloned();

        if let Some(project) = project {
            self.selected_project = Some(project);
            self.selected_thread = None;
            self.creating_thread = true;
            self.view = View::Chat;
            self.input_mode = InputMode::Editing;
            self.chat_editor.clear();
        }
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
            self.notify(Notification::warning("No images in current conversation"));
            return;
        }

        match self.open_image_in_viewer(&urls[0]) {
            Ok(_) => {
                self.notify(Notification::info("Opening image in viewer..."));
            }
            Err(e) => {
                self.notify(Notification::error(e));
            }
        }
    }

    /// Open ask UI inline (replacing input box)
    pub fn open_ask_modal(&mut self, message_id: String, ask_event: AskEvent, ask_author_pubkey: String) {
        use crate::ui::modal::AskModalState;
        let input_state = AskInputState::new(ask_event.questions.clone());
        self.modal_state = ModalState::AskModal(AskModalState {
            message_id,
            ask_event,
            input_state,
            ask_author_pubkey,
        });
        self.input_mode = InputMode::Normal;
    }

    /// Close ask UI and return to normal input
    pub fn close_ask_modal(&mut self) {
        if matches!(self.modal_state, ModalState::AskModal(_)) {
            self.modal_state = ModalState::None;
        }
        self.input_mode = InputMode::Editing;
    }

    /// Get reference to ask modal state if it's open
    pub fn ask_modal_state(&self) -> Option<&crate::ui::modal::AskModalState> {
        match &self.modal_state {
            ModalState::AskModal(state) => Some(state),
            _ => None,
        }
    }

    /// Get mutable reference to ask modal state if it's open
    pub fn ask_modal_state_mut(&mut self) -> Option<&mut crate::ui::modal::AskModalState> {
        match &mut self.modal_state {
            ModalState::AskModal(state) => Some(state),
            _ => None,
        }
    }

    /// Check for unanswered ask events in current thread
    /// Returns the first unanswered ask event found (not answered by current user)
    /// Returns: (message_id, ask_event, author_pubkey)
    pub fn has_unanswered_ask_event(&self) -> Option<(String, AskEvent, String)> {
        let messages = self.messages();
        let thread = self.selected_thread.as_ref()?;
        let thread_id = thread.id.as_str();

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

        // First check the thread root itself (if not in subthread view)
        if self.subthread_root.is_none() {
            if let Some(ref ask_event) = thread.ask_event {
                // Check if the thread has been replied to by current user
                if !replied_to_by_user.contains(thread_id) {
                    return Some((thread.id.clone(), ask_event.clone(), thread.pubkey.clone()));
                }
            }
        }

        // Then check messages
        let display_messages: Vec<&Message> = if let Some(ref root_id) = self.subthread_root {
            messages.iter()
                .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                .collect()
        } else {
            messages.iter()
                .filter(|m| m.reply_to.is_none() || m.reply_to.as_deref() == Some(thread_id))
                .collect()
        };

        for msg in &display_messages {
            if let Some(ref ask_event) = msg.ask_event {
                // Check if this message has been replied to by current user
                if !replied_to_by_user.contains(msg.id.as_str()) {
                    return Some((msg.id.clone(), ask_event.clone(), msg.pubkey.clone()));
                }
            }
        }

        // Also check q-tags for ask events (like Svelte's DelegationPreview)
        // q-tags may point directly to ask events instead of threads
        for msg in display_messages {
            for q_tag in &msg.q_tags {
                // Check if this q-tag points to an ask event
                if let Some((ask_event, pubkey)) = self.data_store.borrow().get_ask_event_by_id(q_tag) {
                    // Check if this ask event has been replied to by current user
                    if !replied_to_by_user.contains(q_tag.as_str()) {
                        return Some((q_tag.clone(), ask_event, pubkey));
                    }
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

    // ===== Filter Management Methods =====

    /// Load filter preferences from storage
    pub fn load_filter_preferences(&mut self) {
        let prefs = self.preferences.borrow();
        self.visible_projects = prefs.selected_projects().iter().cloned().collect();
        self.only_by_me = prefs.only_by_me();
        self.time_filter = prefs.time_filter();
        self.show_llm_metadata = prefs.show_llm_metadata();
    }

    /// Save selected projects to preferences
    pub fn save_selected_projects(&self) {
        let projects: Vec<String> = self.visible_projects.iter().cloned().collect();
        self.preferences.borrow_mut().set_selected_projects(projects);
    }

    /// Toggle "only by me" filter and persist
    pub fn toggle_only_by_me(&mut self) {
        self.only_by_me = !self.only_by_me;
        self.preferences.borrow_mut().set_only_by_me(self.only_by_me);
    }

    /// Cycle through time filter options and persist
    pub fn cycle_time_filter(&mut self) {
        self.time_filter = TimeFilter::cycle_next(self.time_filter);
        self.preferences.borrow_mut().set_time_filter(self.time_filter);
    }

    /// Toggle LLM metadata display and persist
    pub fn toggle_llm_metadata(&mut self) {
        self.show_llm_metadata = !self.show_llm_metadata;
        self.preferences.borrow_mut().set_show_llm_metadata(self.show_llm_metadata);
    }

    // ===== Agent Browser Methods =====

    /// Open the agent browser view
    pub fn open_agent_browser(&mut self) {
        self.agent_browser_index = 0;
        self.agent_browser_filter.clear();
        self.agent_browser_in_detail = false;
        self.viewing_agent_id = None;
        self.scroll_offset = 0;
        self.view = View::AgentBrowser;
        self.input_mode = InputMode::Normal;
    }

    /// Get filtered agent definitions for the browser
    pub fn filtered_agent_definitions(&self) -> Vec<tenex_core::models::AgentDefinition> {
        let filter = &self.agent_browser_filter;
        self.data_store.borrow()
            .get_agent_definitions()
            .into_iter()
            .filter(|d| {
                fuzzy_matches(&d.name, filter) ||
                fuzzy_matches(&d.description, filter) ||
                fuzzy_matches(&d.role, filter)
            })
            .cloned()
            .collect()
    }

    /// Get all agent definitions
    pub fn all_agent_definitions(&self) -> Vec<tenex_core::models::AgentDefinition> {
        self.data_store.borrow()
            .get_agent_definitions()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get agent definitions filtered by a custom filter string
    pub fn agent_definitions_filtered_by(&self, filter: &str) -> Vec<tenex_core::models::AgentDefinition> {
        self.data_store.borrow()
            .get_agent_definitions()
            .into_iter()
            .filter(|d| {
                filter.is_empty() ||
                fuzzy_matches(&d.name, filter) ||
                fuzzy_matches(&d.description, filter) ||
                fuzzy_matches(&d.role, filter)
            })
            .cloned()
            .collect()
    }

    // ===== Search Methods =====

    /// Get search results based on current search_filter
    /// Searches thread titles, thread content, and message content
    /// Respects visible_projects filter
    pub fn search_results(&self) -> Vec<SearchResult> {
        if self.search_filter.trim().is_empty() {
            return vec![];
        }

        let filter = self.search_filter.to_lowercase();
        let store = self.data_store.borrow();
        let mut results = Vec::new();

        // Search threads (title and content match)
        for project in store.get_projects() {
            let a_tag = project.a_tag();

            // Skip projects not in visible_projects
            if !self.visible_projects.is_empty() && !self.visible_projects.contains(&a_tag) {
                continue;
            }

            let project_name = project.name.clone();

            for thread in store.get_threads(&a_tag) {
                // Check if thread title, content, or ID matches
                let title_matches = thread.title.to_lowercase().contains(&filter);
                let content_matches = thread.content.to_lowercase().contains(&filter);
                let id_matches = thread.id.to_lowercase().contains(&filter);

                if title_matches || content_matches || id_matches {
                    let (match_type, excerpt) = if id_matches {
                        (SearchMatchType::ConversationId, Some(format!("ID: {}", thread.id)))
                    } else if title_matches {
                        (SearchMatchType::Thread, None)
                    } else {
                        (SearchMatchType::Thread, Some(Self::extract_excerpt(&thread.content, &filter)))
                    };

                    results.push(SearchResult {
                        thread: thread.clone(),
                        project_a_tag: a_tag.clone(),
                        project_name: project_name.clone(),
                        match_type,
                        excerpt,
                    });
                    continue;
                }

                // Search messages within this thread (limit per thread for performance)
                let messages = store.get_messages(&thread.id);
                for message in messages.iter().take(100) {
                    if message.content.to_lowercase().contains(&filter) {
                        let excerpt = Self::extract_excerpt(&message.content, &filter);
                        results.push(SearchResult {
                            thread: thread.clone(),
                            project_a_tag: a_tag.clone(),
                            project_name: project_name.clone(),
                            match_type: SearchMatchType::Message {
                                message_id: message.id.clone()
                            },
                            excerpt: Some(excerpt),
                        });

                        // Only include first message match per thread
                        break;
                    }
                }
            }
        }

        // Sort by last activity (most recent first)
        results.sort_by(|a, b| b.thread.last_activity.cmp(&a.thread.last_activity));

        // Cap total results
        results.truncate(50);

        results
    }

    /// Extract a short excerpt around the first match of the filter
    fn extract_excerpt(content: &str, filter: &str) -> String {
        let content_lower = content.to_lowercase();
        if let Some(pos) = content_lower.find(filter) {
            // Get some context around the match
            let start = pos.saturating_sub(20);
            let end = (pos + filter.len() + 40).min(content.len());

            // Find safe UTF-8 boundaries
            let safe_start = (start..pos).rev()
                .find(|&i| content.is_char_boundary(i))
                .unwrap_or(start);
            let safe_end = (end..content.len())
                .find(|&i| content.is_char_boundary(i))
                .unwrap_or(content.len());

            let excerpt = &content[safe_start..safe_end];
            let excerpt = excerpt.replace('\n', " ");

            if safe_start > 0 {
                format!("...{}", excerpt.trim())
            } else {
                excerpt.trim().to_string()
            }
        } else {
            // Fallback: just take first 60 chars
            content.chars().take(60).collect::<String>().replace('\n', " ")
        }
    }

    // ===== Nudge Selector Methods =====

    /// Open the nudge selector modal
    pub fn open_nudge_selector(&mut self) {
        use crate::ui::modal::NudgeSelectorState;
        use crate::ui::selector::SelectorState;

        self.modal_state = ModalState::NudgeSelector(NudgeSelectorState {
            selector: SelectorState::new(),
            selected_nudge_ids: self.selected_nudge_ids.clone(),
        });
    }

    /// Close the nudge selector modal, applying selections
    pub fn close_nudge_selector(&mut self, apply: bool) {
        if let ModalState::NudgeSelector(ref state) = self.modal_state {
            if apply {
                self.selected_nudge_ids = state.selected_nudge_ids.clone();
            }
        }
        if matches!(self.modal_state, ModalState::NudgeSelector(_)) {
            self.modal_state = ModalState::None;
        }
    }

    /// Toggle a nudge selection in the nudge selector
    pub fn toggle_nudge_selection(&mut self, nudge_id: &str) {
        if let ModalState::NudgeSelector(ref mut state) = self.modal_state {
            if let Some(pos) = state.selected_nudge_ids.iter().position(|id| id == nudge_id) {
                state.selected_nudge_ids.remove(pos);
            } else {
                state.selected_nudge_ids.push(nudge_id.to_string());
            }
        }
    }

    /// Get filtered nudges for the selector
    pub fn filtered_nudges(&self) -> Vec<tenex_core::models::Nudge> {
        let filter = match &self.modal_state {
            ModalState::NudgeSelector(state) => &state.selector.filter,
            _ => "",
        };
        self.data_store.borrow()
            .get_nudges()
            .into_iter()
            .filter(|n| {
                fuzzy_matches(&n.title, filter) ||
                fuzzy_matches(&n.description, filter)
            })
            .cloned()
            .collect()
    }

    /// Get nudge selector index
    pub fn nudge_selector_index(&self) -> usize {
        match &self.modal_state {
            ModalState::NudgeSelector(state) => state.selector.index,
            _ => 0,
        }
    }

    /// Get nudge selector filter
    pub fn nudge_selector_filter(&self) -> &str {
        match &self.modal_state {
            ModalState::NudgeSelector(state) => &state.selector.filter,
            _ => "",
        }
    }

    /// Get the thread ID to stop operations on, based on current selection
    /// Returns the delegation's thread_id if a DelegationPreview is selected,
    /// otherwise returns the current thread's ID
    pub fn get_stop_target_thread_id(&self) -> Option<String> {
        use crate::ui::views::chat::{group_messages, DisplayItem};

        // Get current thread
        let thread = self.selected_thread.as_ref()?;
        let thread_id = thread.id.as_str();

        // Get messages and group them (same logic as rendering)
        let messages = self.messages();
        let user_pubkey = self.data_store.borrow().user_pubkey.clone();

        // Get display messages based on current view
        let display_messages: Vec<&Message> = if let Some(ref root_id) = self.subthread_root {
            messages.iter()
                .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                .collect()
        } else {
            messages.iter()
                .filter(|m| {
                    Some(m.id.as_str()) == Some(thread_id)
                        || m.reply_to.is_none()
                        || m.reply_to.as_deref() == Some(thread_id)
                })
                .collect()
        };

        // Group messages to get display items
        let grouped = group_messages(&display_messages, user_pubkey.as_deref());

        // Check if the selected item is a DelegationPreview
        if let Some(item) = grouped.get(self.selected_message_index) {
            match item {
                DisplayItem::DelegationPreview { thread_id: delegation_thread_id, .. } => {
                    return Some(delegation_thread_id.clone());
                }
                _ => {}
            }
        }

        // Default: return current thread ID
        Some(thread.id.clone())
    }

    /// Check if a nudge is selected in the nudge selector
    pub fn is_nudge_selected(&self, nudge_id: &str) -> bool {
        match &self.modal_state {
            ModalState::NudgeSelector(state) => state.selected_nudge_ids.contains(&nudge_id.to_string()),
            _ => self.selected_nudge_ids.contains(&nudge_id.to_string()),
        }
    }

    /// Remove a nudge from selected nudges (outside of modal)
    pub fn remove_selected_nudge(&mut self, nudge_id: &str) {
        self.selected_nudge_ids.retain(|id| id != nudge_id);
    }

    /// Clear all selected nudges
    pub fn clear_selected_nudges(&mut self) {
        self.selected_nudge_ids.clear();
    }

    /// Check if a thread has an unsent draft
    pub fn has_draft_for_thread(&self, thread_id: &str) -> bool {
        self.draft_storage.borrow().load(thread_id)
            .map(|d| !d.text.trim().is_empty())
            .unwrap_or(false)
    }

    /// Add a message to history (called after successful send)
    pub fn add_to_message_history(&mut self, content: String) {
        if content.trim().is_empty() {
            return;
        }
        // Avoid duplicates at the end
        if self.message_history.last().map(|s| s.as_str()) != Some(content.trim()) {
            self.message_history.push(content);
            // Limit to 50 entries
            if self.message_history.len() > 50 {
                self.message_history.remove(0);
            }
        }
        // Reset history navigation
        self.history_index = None;
        self.history_draft = None;
    }

    /// Navigate to previous message in history (↑ key)
    pub fn history_prev(&mut self) {
        if self.message_history.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                // Save current input as draft and go to last history entry
                self.history_draft = Some(self.chat_editor.text.clone());
                self.history_index = Some(self.message_history.len() - 1);
                self.chat_editor.text = self.message_history.last().cloned().unwrap_or_default();
                self.chat_editor.cursor = self.chat_editor.text.len();
            }
            Some(idx) if idx > 0 => {
                // Go to older entry
                self.history_index = Some(idx - 1);
                self.chat_editor.text = self.message_history.get(idx - 1).cloned().unwrap_or_default();
                self.chat_editor.cursor = self.chat_editor.text.len();
            }
            _ => {}
        }
        self.chat_editor.clear_selection();
    }

    /// Navigate to next message in history (↓ key)
    pub fn history_next(&mut self) {
        if let Some(idx) = self.history_index {
            if idx + 1 < self.message_history.len() {
                // Go to newer entry
                self.history_index = Some(idx + 1);
                self.chat_editor.text = self.message_history.get(idx + 1).cloned().unwrap_or_default();
                self.chat_editor.cursor = self.chat_editor.text.len();
            } else {
                // Restore draft and exit history mode
                self.chat_editor.text = self.history_draft.take().unwrap_or_default();
                self.chat_editor.cursor = self.chat_editor.text.len();
                self.history_index = None;
            }
            self.chat_editor.clear_selection();
        }
    }

    /// Check if currently browsing history
    pub fn is_browsing_history(&self) -> bool {
        self.history_index.is_some()
    }

    /// Exit history mode without changing input
    pub fn exit_history_mode(&mut self) {
        self.history_index = None;
        self.history_draft = None;
    }

    /// Toggle vim mode on/off
    pub fn toggle_vim_mode(&mut self) {
        self.vim_enabled = !self.vim_enabled;
        if self.vim_enabled {
            self.vim_mode = VimMode::Normal;
            self.notify(Notification::info("Vim mode enabled (Esc=normal, i/a=insert)"));
        } else {
            self.notify(Notification::info("Vim mode disabled"));
        }
    }

    /// Enter vim insert mode
    pub fn vim_enter_insert(&mut self) {
        self.vim_mode = VimMode::Insert;
    }

    /// Enter vim insert mode after cursor (append)
    pub fn vim_enter_append(&mut self) {
        self.vim_mode = VimMode::Insert;
        self.chat_editor.move_right();
    }

    /// Enter vim normal mode
    pub fn vim_enter_normal(&mut self) {
        self.vim_mode = VimMode::Normal;
    }

    // ===== Archive Methods =====

    /// Toggle visibility of archived conversations
    pub fn toggle_show_archived(&mut self) {
        self.show_archived = !self.show_archived;
        if self.show_archived {
            self.notify(Notification::info("Showing archived conversations"));
        } else {
            self.notify(Notification::info("Hiding archived conversations"));
        }
    }

    /// Check if a thread is archived
    pub fn is_thread_archived(&self, thread_id: &str) -> bool {
        self.preferences.borrow().is_thread_archived(thread_id)
    }

    /// Toggle archive status of a thread
    pub fn toggle_thread_archived(&mut self, thread_id: &str) -> bool {
        self.preferences.borrow_mut().toggle_thread_archived(thread_id)
    }
}

/// A search result - can match thread title/content or message content
#[derive(Debug, Clone)]
pub struct SearchResult {
    pub thread: Thread,
    pub project_a_tag: String,
    pub project_name: String,
    pub match_type: SearchMatchType,
    pub excerpt: Option<String>,
}

#[derive(Debug, Clone)]
pub enum SearchMatchType {
    Thread,
    ConversationId,
    Message { message_id: String },
}
