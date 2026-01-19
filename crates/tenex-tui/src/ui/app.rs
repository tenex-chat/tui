use crate::models::{AskEvent, ChatDraft, DraftStorage, Message, PreferencesStorage, Project, ProjectAgent, ProjectStatus, Thread, TimeFilter};
use crate::nostr::DataChange;
use crate::store::{get_trace_context, AppDataStore, Database};
use crate::ui::ask_input::AskInputState;
use crate::ui::modal::{CommandPaletteState, ModalState, PaletteContext};
use crate::ui::notifications::{Notification, NotificationQueue};
use crate::ui::selector::SelectorState;
use crate::ui::state::{ChatSearchMatch, ChatSearchState, NavigationStackEntry, OpenTab, TabManager};
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HomeTab {
    Recent,
    Inbox,
    Reports,
    Status,
    Search,
}

// ChatSearchState, ChatSearchMatch, OpenTab, TabManager, HomeViewState, ChatViewState
// are now in ui::state module

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

/// Actions that can be undone
#[derive(Debug, Clone)]
pub enum UndoAction {
    /// Thread was archived (store thread_id to unarchive)
    ThreadArchived { thread_id: String, thread_title: String },
    /// Thread was unarchived (store thread_id to re-archive)
    ThreadUnarchived { thread_id: String, thread_title: String },
    /// Project was archived
    ProjectArchived { project_a_tag: String, project_name: String },
    /// Project was unarchived
    ProjectUnarchived { project_a_tag: String, project_name: String },
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

    /// Tab management (open tabs, history, modal state)
    pub tabs: TabManager,

    // Home view state
    pub home_panel_focus: HomeTab,
    /// Per-tab selection index (preserves position when switching tabs)
    pub tab_selection: HashMap<HomeTab, usize>,
    pub report_search_filter: String,
    /// Whether sidebar is focused (vs content area)
    pub sidebar_focused: bool,
    /// Selected index in sidebar project list
    pub sidebar_project_index: usize,
    /// Projects to show in Recent/Inbox (empty = none)
    pub visible_projects: HashSet<String>,
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

    // Search modal state (deprecated - replaced by Search tab)
    pub showing_search_modal: bool,
    pub search_filter: String,
    pub search_index: usize,

    /// In-conversation search state
    pub chat_search: ChatSearchState,

    /// Local streaming buffers by conversation_id
    pub local_stream_buffers: HashMap<String, LocalStreamBuffer>,

    /// Toggle for showing/hiding LLM metadata on messages (model, tokens, cost)
    pub show_llm_metadata: bool,

    /// Toggle for showing/hiding the todo sidebar
    pub todo_sidebar_visible: bool,

    /// Collapsed thread IDs (parent threads whose children are hidden)
    pub collapsed_threads: HashSet<String>,

    /// Project a_tag when waiting for a newly created thread to appear
    pub pending_new_thread_project: Option<String>,
    /// Draft ID when waiting for a newly created thread (to convert draft tab)
    pub pending_new_thread_draft_id: Option<String>,

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
    /// Whether to show archived projects in the sidebar
    pub show_archived_projects: bool,
    /// Whether user explicitly selected an agent in the current conversation
    /// When true, don't auto-sync agent from conversation messages
    pub user_explicitly_selected_agent: bool,
    /// Last action that can be undone (Ctrl+T + u)
    pub last_undo_action: Option<UndoAction>,
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
            tabs: TabManager::new(),
            home_panel_focus: HomeTab::Recent,
            tab_selection: HashMap::new(),
            report_search_filter: String::new(),
            sidebar_focused: false,
            sidebar_project_index: 0,
            visible_projects: HashSet::new(),
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
            chat_search: ChatSearchState::default(),
            local_stream_buffers: HashMap::new(),
            show_llm_metadata: false,
            todo_sidebar_visible: true,
            collapsed_threads: HashSet::new(),
            pending_new_thread_project: None,
            pending_new_thread_draft_id: None,
            selected_nudge_ids: Vec::new(),
            frame_counter: 0,
            message_history: Vec::new(),
            history_index: None,
            history_draft: None,
            vim_enabled: false,
            vim_mode: VimMode::Normal,
            show_archived: false,
            show_archived_projects: false,
            user_explicitly_selected_agent: false,
            last_undo_action: None,
        }
    }

    // =============================================================================
    // TAB ACCESSOR METHODS (backward compatibility)
    // =============================================================================
    // These methods delegate to TabManager for backward compatibility with code
    // that accesses app.open_tabs, app.active_tab_index, etc. directly.

    /// Get reference to open tabs (delegates to TabManager)
    #[inline]
    pub fn open_tabs(&self) -> &[OpenTab] {
        self.tabs.tabs()
    }

    /// Get mutable reference to open tabs (delegates to TabManager)
    #[inline]
    pub fn open_tabs_mut(&mut self) -> &mut Vec<OpenTab> {
        self.tabs.tabs_mut()
    }

    /// Get active tab index (delegates to TabManager)
    #[inline]
    pub fn active_tab_index(&self) -> usize {
        self.tabs.active_index()
    }

    /// Get whether tab modal is showing (delegates to TabManager)
    #[inline]
    pub fn showing_tab_modal(&self) -> bool {
        self.tabs.modal_open
    }

    /// Set whether tab modal is showing (delegates to TabManager)
    #[inline]
    pub fn set_showing_tab_modal(&mut self, showing: bool) {
        self.tabs.modal_open = showing;
    }

    /// Get tab modal index (delegates to TabManager)
    #[inline]
    pub fn tab_modal_index(&self) -> usize {
        self.tabs.modal_index
    }

    /// Set tab modal index (delegates to TabManager)
    #[inline]
    pub fn set_tab_modal_index(&mut self, index: usize) {
        self.tabs.modal_index = index;
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

    /// Get the ID of the currently selected message (if any)
    pub fn selected_message_id(&self) -> Option<String> {
        use crate::ui::views::chat::{group_messages, DisplayItem};

        let messages = self.messages();
        let thread_id = self.selected_thread.as_ref().map(|t| t.id.as_str());

        // Filter messages based on current view (subthread or main thread)
        let display_messages: Vec<&Message> = if let Some(ref root_id) = self.subthread_root {
            messages.iter()
                .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                .collect()
        } else {
            messages.iter()
                .filter(|m| {
                    Some(m.id.as_str()) == thread_id
                        || m.reply_to.is_none()
                        || m.reply_to.as_deref() == thread_id
                })
                .collect()
        };

        let grouped = group_messages(&display_messages);
        grouped.get(self.selected_message_index).and_then(|item| {
            match item {
                DisplayItem::SingleMessage { message, .. } => Some(message.id.clone()),
                DisplayItem::DelegationPreview { .. } => None,
            }
        })
    }

    /// Get the count of display items in the current chat view.
    /// Used for navigation bounds checking.
    pub fn display_item_count(&self) -> usize {
        use crate::ui::views::chat::group_messages;

        let messages = self.messages();
        let thread_id = self.selected_thread.as_ref().map(|t| t.id.as_str());

        // Filter messages based on current view (subthread or main thread)
        let display_messages: Vec<&Message> = if let Some(ref root_id) = self.subthread_root {
            messages.iter()
                .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                .collect()
        } else {
            messages.iter()
                .filter(|m| {
                    Some(m.id.as_str()) == thread_id
                        || m.reply_to.is_none()
                        || m.reply_to.as_deref() == thread_id
                })
                .collect()
        };

        group_messages(&display_messages).len()
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

    /// Save current chat editor content as draft for the selected thread or draft tab
    pub fn save_chat_draft(&self) {
        // Determine the draft key - use thread id or draft_id from active tab
        let draft_key = if let Some(ref thread) = self.selected_thread {
            Some(thread.id.clone())
        } else {
            // Check if current tab is a draft tab
            self.tabs.active_tab().and_then(|t| t.draft_id.clone())
        };

        if let Some(conversation_id) = draft_key {
            let draft = ChatDraft {
                conversation_id,
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

    /// Restore draft for the selected thread or draft tab into chat_editor
    /// Priority: draft values > conversation sync > defaults
    pub fn restore_chat_draft(&mut self) {
        // Reset explicit selection flag when switching conversations
        self.user_explicitly_selected_agent = false;

        // Always clear editor first - each conversation has its own draft
        self.chat_editor.text.clear();
        self.chat_editor.cursor = 0;

        // Determine the draft key - use thread id or draft_id from active tab
        let draft_key = if let Some(ref thread) = self.selected_thread {
            Some(thread.id.clone())
        } else {
            // Check if current tab is a draft tab
            self.tabs.active_tab().and_then(|t| t.draft_id.clone())
        };

        // Track whether draft had explicit agent/branch selections
        let mut draft_had_agent = false;
        let mut draft_had_branch = false;

        if let Some(key) = draft_key {
            // Load draft
            if let Some(draft) = self.draft_storage.borrow().load(&key) {
                self.chat_editor.text = draft.text;
                self.chat_editor.cursor = self.chat_editor.text.len();

                // Restore agent from draft if one was saved (takes priority over sync)
                if let Some(ref agent_pubkey) = draft.selected_agent_pubkey {
                    // Find agent by pubkey in available agents
                    let agent = self.available_agents()
                        .into_iter()
                        .find(|a| &a.pubkey == agent_pubkey);
                    if let Some(agent) = agent {
                        self.selected_agent = Some(agent);
                        draft_had_agent = true;
                    }
                }
                // Restore branch from draft if one was saved (takes priority over sync)
                if draft.selected_branch.is_some() {
                    self.selected_branch = draft.selected_branch;
                    draft_had_branch = true;
                }
            }
        }

        // For real threads, sync agent and branch with conversation ONLY if draft didn't have values
        // This ensures draft selections are preserved while still providing sensible defaults
        if self.selected_thread.is_some() {
            if !draft_had_agent {
                self.sync_agent_with_conversation();
            }
            if !draft_had_branch {
                self.sync_branch_with_conversation();
            }
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
    /// Archived projects are hidden unless show_archived_projects is true
    pub fn filtered_projects(&self) -> (Vec<Project>, Vec<Project>) {
        let filter = self.projects_modal_filter();
        let store = self.data_store.borrow();
        let projects = store.get_projects();
        let prefs = self.preferences.borrow();

        let mut matching: Vec<_> = projects
            .iter()
            .filter(|p| {
                // Filter out archived projects unless showing archived
                self.show_archived_projects || !prefs.is_project_archived(&p.a_tag())
            })
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

    /// Get the most recent branch from conversation messages.
    /// Returns the branch tag from the most recent message that has one.
    pub fn get_most_recent_branch_from_conversation(&self) -> Option<String> {
        let messages = self.messages();
        let user_pubkey = self.data_store.borrow().user_pubkey.clone();

        // Find the most recent message with a branch tag (not from the user)
        let mut latest_branch: Option<String> = None;
        let mut latest_timestamp: u64 = 0;

        for msg in &messages {
            // Skip messages from the user
            if user_pubkey.as_ref().map(|pk| pk == &msg.pubkey).unwrap_or(false) {
                continue;
            }

            // Check if this message has a branch and is more recent
            if msg.branch.is_some() && msg.created_at >= latest_timestamp {
                latest_timestamp = msg.created_at;
                latest_branch = msg.branch.clone();
            }
        }

        latest_branch
    }

    /// Sync selected_branch with the most recent branch in the conversation
    /// Falls back to default branch if no branch found in messages
    pub fn sync_branch_with_conversation(&mut self) {
        // First try to get the most recent branch from the conversation
        if let Some(recent_branch) = self.get_most_recent_branch_from_conversation() {
            self.selected_branch = Some(recent_branch);
            return;
        }

        // Fall back to default branch if no branch found in messages
        if let Some(status) = self.get_selected_project_status() {
            if let Some(default_branch) = status.default_branch() {
                self.selected_branch = Some(default_branch.to_string());
            }
        }
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

    /// Get the current hotkey context based on application state.
    /// Used by the hotkey registry to determine which hotkeys are active.
    pub fn hotkey_context(&self) -> super::hotkeys::HotkeyContext {
        super::hotkeys::HotkeyContext::from_app_state(
            &self.view,
            &self.input_mode,
            &self.modal_state,
            &self.home_panel_focus,
            self.sidebar_focused,
        )
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

                    // Get the actual project to check busy and archived status
                    let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
                    let (is_busy, is_archived) = all_projects.get(self.sidebar_project_index)
                        .map(|p| {
                            let a_tag = p.a_tag();
                            (
                                self.data_store.borrow().is_project_busy(&a_tag),
                                self.is_project_archived(&a_tag),
                            )
                        })
                        .unwrap_or((false, false));

                    PaletteContext::HomeSidebar { is_online, is_busy, is_archived }
                } else {
                    match self.home_panel_focus {
                        HomeTab::Recent => PaletteContext::HomeRecent,
                        HomeTab::Inbox => PaletteContext::HomeInbox,
                        HomeTab::Reports => PaletteContext::HomeReports,
                        HomeTab::Status => PaletteContext::HomeRecent, // Reuse Recent context for Status
                        HomeTab::Search => PaletteContext::HomeRecent, // Reuse Recent context for Search
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
                    let message_has_trace = self.selected_message_id()
                        .map(|id| get_trace_context(&self.db.ndb, &id).is_some())
                        .unwrap_or(false);

                    // Check if any agent is working on this thread
                    let agent_working = self.selected_thread.as_ref()
                        .map(|t| self.data_store.borrow().is_event_busy(&t.id))
                        .unwrap_or(false);

                    PaletteContext::ChatNormal { has_parent, message_has_trace, agent_working }
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
        self.tabs.open_thread(
            thread.id.clone(),
            thread.title.clone(),
            project_a_tag.to_string(),
        )
    }

    /// Open a draft tab for a new conversation (before thread is created)
    /// Returns the tab index
    pub fn open_draft_tab(&mut self, project_a_tag: &str, project_name: &str) -> usize {
        self.tabs.open_draft(project_a_tag.to_string(), project_name.to_string())
    }

    /// Convert a draft tab to a real tab when thread is created
    pub fn convert_draft_to_tab(&mut self, draft_id: &str, thread: &Thread) {
        self.tabs.convert_draft(draft_id, thread.id.clone(), thread.title.clone());
    }

    /// Find if there's an active draft tab for a project
    pub fn find_draft_tab(&self, project_a_tag: &str) -> Option<(usize, &str)> {
        self.tabs.find_draft_for_project(project_a_tag)
    }

    /// Close the current tab
    pub fn close_current_tab(&mut self) {
        if self.tabs.is_empty() {
            return;
        }

        // Close and get the new active index
        let new_active = self.tabs.close_current();

        if new_active.is_none() {
            // No more tabs - go back to home view
            self.save_chat_draft();
            self.chat_editor.clear();
            self.selected_thread = None;
            self.view = View::Home;
        } else {
            // Switch to the new active tab
            self.switch_to_tab(self.tabs.active_index());
        }
    }

    /// Switch to a specific tab by index
    pub fn switch_to_tab(&mut self, index: usize) {
        if index >= self.tabs.len() {
            return;
        }

        // Save current draft before switching
        self.save_chat_draft();

        // Switch in the tab manager (handles history tracking)
        self.tabs.switch_to(index);

        // Extract data we need
        let tab = &self.tabs.tabs()[index];
        let is_draft = tab.is_draft();
        let thread_id = tab.thread_id.clone();
        let project_a_tag = tab.project_a_tag.clone();

        if is_draft {
            // Draft tab - set up for new conversation
            self.selected_thread = None;
            self.creating_thread = true;

            // CRITICAL: Clear all context upfront to prevent stale state leaking
            // if project lookup fails below
            self.selected_project = None;
            self.selected_agent = None;
            self.selected_branch = None;

            // Set the project for this draft
            let project = self.data_store.borrow()
                .get_projects()
                .iter()
                .find(|p| p.a_tag() == project_a_tag)
                .cloned();

            if let Some(project) = project {
                let a_tag = project.a_tag();
                self.selected_project = Some(project);

                // Auto-select PM agent and default branch from status
                // (restore_chat_draft will override if draft has specific values)
                if let Some(status) = self.data_store.borrow().get_project_status(&a_tag) {
                    if let Some(pm) = status.pm_agent() {
                        self.selected_agent = Some(pm.clone());
                    }
                    // Always set default branch (removed .is_none() guard to prevent stale values)
                    self.selected_branch = status.default_branch().map(String::from);
                }
            }

            self.restore_chat_draft();
            self.scroll_offset = 0;
            self.selected_message_index = 0;
            self.subthread_root = None;
            self.subthread_root_message = None;
            self.input_mode = InputMode::Editing;
            self.view = View::Chat;
        } else {
            // Real tab - find the thread in data store
            let thread = self.data_store.borrow().get_threads(&project_a_tag)
                .iter()
                .find(|t| t.id == thread_id)
                .cloned();

            if let Some(thread) = thread {
                self.selected_thread = Some(thread);
                self.creating_thread = false;

                // CRITICAL: Set project context from the tab's project_a_tag
                // This ensures cross-tab contamination doesn't occur
                let project = self.data_store.borrow()
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == project_a_tag)
                    .cloned();

                if let Some(project) = project {
                    let a_tag = project.a_tag();
                    self.selected_project = Some(project);

                    // Load draft once upfront to check what values it contains
                    let draft = self.draft_storage.borrow().load(&thread_id);
                    let draft_has_agent = draft.as_ref().map(|d| d.selected_agent_pubkey.is_some()).unwrap_or(false);
                    let draft_has_branch = draft.as_ref().map(|d| d.selected_branch.is_some()).unwrap_or(false);

                    // Set agent and branch defaults only if draft doesn't have them
                    if let Some(status) = self.data_store.borrow().get_project_status(&a_tag) {
                        if !draft_has_agent {
                            // No draft agent, use PM as default
                            if let Some(pm) = status.pm_agent() {
                                self.selected_agent = Some(pm.clone());
                            } else {
                                // No PM available, clear to prevent stale state
                                self.selected_agent = None;
                            }
                        }

                        if !draft_has_branch {
                            self.selected_branch = status.default_branch().map(String::from);
                        }
                    } else {
                        // No project status, clear agent/branch to prevent stale state
                        self.selected_agent = None;
                        self.selected_branch = None;
                    }

                    // Now restore the draft (uses cached load if same key)
                    self.restore_chat_draft();
                } else {
                    // Project lookup failed - clear all context to prevent stale state leaking
                    self.selected_project = None;
                    self.selected_agent = None;
                    self.selected_branch = None;
                }
                self.scroll_offset = usize::MAX; // Scroll to bottom
                self.selected_message_index = 0;
                self.subthread_root = None;
                self.subthread_root_message = None;
                self.input_mode = InputMode::Editing; // Auto-focus input
            }
        }

        // Auto-open pending ask modal if there's one for this thread
        self.maybe_open_pending_ask();
    }

    /// Cycle to next tab in history (Alt+Tab behavior)
    pub fn cycle_tab_history_forward(&mut self) {
        // TabManager handles the history internally
        self.tabs.cycle_history_forward();
        self.switch_to_tab(self.tabs.active_index());
    }

    /// Cycle to previous tab in history (Alt+Shift+Tab behavior)
    pub fn cycle_tab_history_backward(&mut self) {
        // For backward, we go to next tab (simpler behavior)
        self.prev_tab();
    }

    /// Open tab modal
    pub fn open_tab_modal(&mut self) {
        self.tabs.open_modal();
    }

    /// Close tab modal
    pub fn close_tab_modal(&mut self) {
        self.tabs.close_modal();
    }

    /// Close tab at specific index (for tab modal)
    pub fn close_tab_at(&mut self, index: usize) {
        let was_active = index == self.tabs.active_index();
        let new_active = self.tabs.close_at(index);

        if new_active.is_none() {
            // No more tabs - go back to home view
            self.save_chat_draft();
            self.chat_editor.clear();
            self.selected_thread = None;
            self.view = View::Home;
        } else if was_active {
            // If the closed tab was active, switch to the new active tab
            self.switch_to_tab(self.tabs.active_index());
        }
    }

    /// Switch to next tab (Ctrl+Tab)
    pub fn next_tab(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        let next = (self.tabs.active_index() + 1) % self.tabs.len();
        self.switch_to_tab(next);
    }

    /// Switch to previous tab (Ctrl+Shift+Tab)
    pub fn prev_tab(&mut self) {
        if self.tabs.len() <= 1 {
            return;
        }
        let prev = if self.tabs.active_index() == 0 {
            self.tabs.len() - 1
        } else {
            self.tabs.active_index() - 1
        };
        self.switch_to_tab(prev);
    }

    /// Mark a thread as having unread messages (if it's open in a tab but not active)
    pub fn mark_tab_unread(&mut self, thread_id: &str) {
        self.tabs.mark_unread(thread_id);
    }

    // ===== Home View Methods =====

    /// Get the selection index for the current home tab
    pub fn current_selection(&self) -> usize {
        *self.tab_selection.get(&self.home_panel_focus).unwrap_or(&0)
    }

    /// Set the selection index for the current home tab
    pub fn set_current_selection(&mut self, index: usize) {
        self.tab_selection.insert(self.home_panel_focus, index);
    }

    /// Get threads with status metadata, sorted by activity
    /// Returns threads that have status_label OR status_current_activity
    pub fn status_threads(&self) -> Vec<(Thread, String)> {
        // Empty visible_projects = show nothing
        if self.visible_projects.is_empty() {
            return vec![];
        }

        let threads = self.data_store.borrow().get_all_recent_threads(100);
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let prefs = self.preferences.borrow();

        let mut status_threads: Vec<(Thread, String)> = threads.into_iter()
            // Project filter
            .filter(|(_, a_tag)| self.visible_projects.contains(a_tag))
            // Must have status metadata (label or current activity)
            .filter(|(thread, _)| {
                thread.status_label.is_some() || thread.status_current_activity.is_some()
            })
            // Archive filter
            .filter(|(thread, _)| {
                self.show_archived || !prefs.is_thread_archived(&thread.id)
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
            .collect();

        // Sort: threads with current_activity first, then by last_activity descending
        status_threads.sort_by(|(a, _), (b, _)| {
            let a_has_activity = a.status_current_activity.is_some();
            let b_has_activity = b.status_current_activity.is_some();
            match (a_has_activity, b_has_activity) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => b.last_activity.cmp(&a.last_activity),
            }
        });

        status_threads
    }

    /// Get recent threads across all projects for Home view (filtered by visible_projects, time_filter, archived)
    pub fn recent_threads(&self) -> Vec<(Thread, String)> {
        // Empty visible_projects = show nothing (inverted default)
        if self.visible_projects.is_empty() {
            return vec![];
        }

        let threads = self.data_store.borrow().get_all_recent_threads(50);
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

    /// Get inbox items for Home view (filtered by visible_projects, time_filter, archived)
    pub fn inbox_items(&self) -> Vec<crate::models::InboxItem> {
        // Empty visible_projects = show nothing (inverted default)
        if self.visible_projects.is_empty() {
            return vec![];
        }

        let items = self.data_store.borrow().get_inbox_items().to_vec();
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
            let a_tag = project.a_tag();
            self.selected_project = Some(project);

            // Set default agent/branch from project status
            if let Some(status) = self.data_store.borrow().get_project_status(&a_tag) {
                if let Some(pm) = status.pm_agent() {
                    self.selected_agent = Some(pm.clone());
                } else {
                    self.selected_agent = None;
                }
                self.selected_branch = status.default_branch().map(String::from);
            } else {
                self.selected_agent = None;
                self.selected_branch = None;
            }

            // Open tab and switch to chat
            self.open_tab(thread, project_a_tag);
            self.selected_thread = Some(thread.clone());
            self.restore_chat_draft();
            self.view = View::Chat;
            self.input_mode = InputMode::Editing;
            self.scroll_offset = usize::MAX;

            // Auto-open pending ask modal if there's one for this thread
            self.maybe_open_pending_ask();
        } else {
            // Project not found - clear state to prevent leaks
            self.selected_project = None;
            self.selected_agent = None;
            self.selected_branch = None;
        }
    }

    /// Push current conversation onto the navigation stack and navigate to a delegation.
    /// This allows drilling into delegations within the same tab.
    pub fn push_delegation(&mut self, delegation_thread_id: &str) {
        // Get current tab state to save
        let current_state = self.tabs.active_tab().map(|tab| NavigationStackEntry {
            thread_id: tab.thread_id.clone(),
            thread_title: tab.thread_title.clone(),
            project_a_tag: tab.project_a_tag.clone(),
            scroll_offset: self.scroll_offset,
            selected_message_index: self.selected_message_index,
        });

        // Find the delegation thread
        let thread_and_project = {
            let store = self.data_store.borrow();
            store.get_thread_by_id(delegation_thread_id).map(|t| {
                let project_a_tag = store
                    .find_project_for_thread(delegation_thread_id)
                    .unwrap_or_default();
                (t.clone(), project_a_tag)
            })
        };

        if let (Some(entry), Some((thread, project_a_tag))) = (current_state, thread_and_project) {
            // Push current state onto stack
            if let Some(tab) = self.tabs.active_tab_mut() {
                tab.navigation_stack.push(entry);
                // Update tab to point to new thread
                tab.thread_id = thread.id.clone();
                tab.thread_title = thread.title.clone();
                tab.project_a_tag = project_a_tag.clone();
            }

            // Update app state
            self.selected_thread = Some(thread);
            self.scroll_offset = usize::MAX; // Start at bottom
            self.selected_message_index = 0;

            // Update project context if needed
            let project = self.data_store.borrow().get_projects()
                .iter()
                .find(|p| p.a_tag() == project_a_tag)
                .cloned();
            if let Some(project) = project {
                self.selected_project = Some(project);
            }

            // Auto-open pending ask modal if there's one for this thread
            self.maybe_open_pending_ask();
        }
    }

    /// Pop from the navigation stack and return to the parent conversation.
    /// Returns true if popped successfully, false if stack was empty.
    pub fn pop_navigation_stack(&mut self) -> bool {
        let entry = self.tabs.active_tab_mut()
            .and_then(|tab| tab.navigation_stack.pop());

        if let Some(entry) = entry {
            // Find the parent thread
            let thread = self.data_store.borrow()
                .get_thread_by_id(&entry.thread_id)
                .cloned();

            if let Some(thread) = thread {
                // Update tab to point to parent thread
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.thread_id = entry.thread_id.clone();
                    tab.thread_title = entry.thread_title.clone();
                    tab.project_a_tag = entry.project_a_tag.clone();
                }

                // Restore app state
                self.selected_thread = Some(thread);
                self.scroll_offset = entry.scroll_offset;
                self.selected_message_index = entry.selected_message_index;

                // Update project context if needed
                let project = self.data_store.borrow().get_projects()
                    .iter()
                    .find(|p| p.a_tag() == entry.project_a_tag)
                    .cloned();
                if let Some(project) = project {
                    self.selected_project = Some(project);
                }

                // Auto-open pending ask modal if there's one for this thread
                self.maybe_open_pending_ask();

                return true;
            }
        }
        false
    }

    /// Check if the current tab has items on its navigation stack.
    pub fn has_navigation_stack(&self) -> bool {
        self.tabs.active_tab()
            .map(|tab| !tab.navigation_stack.is_empty())
            .unwrap_or(false)
    }

    /// Start a new thread for a specific project (navigates to chat without a thread selected)
    pub fn start_new_thread_for_project(&mut self, project_a_tag: &str) {
        // Find and set selected project
        let project = self.data_store.borrow().get_projects()
            .iter()
            .find(|p| p.a_tag() == project_a_tag)
            .cloned();

        if let Some(project) = project {
            let a_tag = project.a_tag();
            self.selected_project = Some(project);

            // Set default agent/branch from project status
            if let Some(status) = self.data_store.borrow().get_project_status(&a_tag) {
                if let Some(pm) = status.pm_agent() {
                    self.selected_agent = Some(pm.clone());
                } else {
                    self.selected_agent = None;
                }
                self.selected_branch = status.default_branch().map(String::from);
            } else {
                self.selected_agent = None;
                self.selected_branch = None;
            }

            self.selected_thread = None;
            self.creating_thread = true;
            self.view = View::Chat;
            self.input_mode = InputMode::Editing;
            self.chat_editor.clear();
        } else {
            // Project not found - clear state to prevent leaks
            self.selected_project = None;
            self.selected_agent = None;
            self.selected_branch = None;
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
    /// The pending ask remains until the user answers (sends a message that e-tags it)
    pub fn close_ask_modal(&mut self) {
        self.modal_state = ModalState::None;
        self.input_mode = InputMode::Editing;
    }

    /// Auto-open the ask modal if there's an unanswered ask for the current thread.
    /// Derives state at check time by looking at q-tags in messages.
    /// Only opens if no modal is currently active.
    pub fn maybe_open_pending_ask(&mut self) {
        if !matches!(self.modal_state, ModalState::None) {
            return;
        }
        let thread_id = match self.selected_thread.as_ref() {
            Some(t) => t.id.clone(),
            None => return,
        };
        // Derive unanswered ask at check time (no persistent tracking)
        let ask_info = self.data_store.borrow().get_unanswered_ask_for_thread(&thread_id);
        if let Some((id, ask_event, author_pubkey)) = ask_info {
            self.open_ask_modal(id, ask_event, author_pubkey);
        }
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

    /// Check if a specific message's ask event has been answered by the current user
    pub fn is_ask_answered_by_user(&self, message_id: &str) -> bool {
        self.get_user_response_to_ask(message_id).is_some()
    }

    /// Get the user's response to an ask event (if any)
    pub fn get_user_response_to_ask(&self, message_id: &str) -> Option<String> {
        let messages = self.messages();

        // Get current user's pubkey
        let user_pubkey = self.data_store.borrow().user_pubkey.clone()?;

        // Find user's reply to this message
        for msg in &messages {
            if msg.pubkey == user_pubkey {
                if let Some(ref reply_to) = msg.reply_to {
                    if reply_to == message_id {
                        return Some(msg.content.clone());
                    }
                }
            }
        }

        None
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
        self.time_filter = prefs.time_filter();
        self.show_llm_metadata = prefs.show_llm_metadata();
    }

    /// Save selected projects to preferences
    pub fn save_selected_projects(&self) {
        let projects: Vec<String> = self.visible_projects.iter().cloned().collect();
        self.preferences.borrow_mut().set_selected_projects(projects);
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

    // ===== Chat Search Methods (in-conversation search) =====

    /// Enter chat search mode
    pub fn enter_chat_search(&mut self) {
        self.chat_search.active = true;
        self.chat_search.query.clear();
        self.chat_search.current_match = 0;
        self.chat_search.total_matches = 0;
        self.chat_search.match_locations.clear();
    }

    /// Exit chat search mode
    pub fn exit_chat_search(&mut self) {
        self.chat_search.active = false;
        self.chat_search.query.clear();
        self.chat_search.match_locations.clear();
    }

    /// Update chat search results based on current query
    pub fn update_chat_search(&mut self) {
        self.chat_search.match_locations.clear();
        self.chat_search.current_match = 0;
        self.chat_search.total_matches = 0;

        if self.chat_search.query.trim().is_empty() {
            return;
        }

        let query_lower = self.chat_search.query.to_lowercase();
        let messages = self.messages();

        for msg in &messages {
            let content_lower = msg.content.to_lowercase();
            let mut start = 0;

            while let Some(pos) = content_lower[start..].find(&query_lower) {
                let absolute_pos = start + pos;
                self.chat_search.match_locations.push(ChatSearchMatch {
                    message_id: msg.id.clone(),
                    start_offset: absolute_pos,
                    length: self.chat_search.query.len(),
                });
                start = absolute_pos + 1;
            }
        }

        self.chat_search.total_matches = self.chat_search.match_locations.len();
    }

    /// Navigate to next search match
    pub fn chat_search_next(&mut self) {
        if self.chat_search.total_matches > 0 {
            self.chat_search.current_match =
                (self.chat_search.current_match + 1) % self.chat_search.total_matches;
            self.scroll_to_current_search_match();
        }
    }

    /// Navigate to previous search match
    pub fn chat_search_prev(&mut self) {
        if self.chat_search.total_matches > 0 {
            if self.chat_search.current_match == 0 {
                self.chat_search.current_match = self.chat_search.total_matches - 1;
            } else {
                self.chat_search.current_match -= 1;
            }
            self.scroll_to_current_search_match();
        }
    }

    /// Scroll to make the current search match visible
    fn scroll_to_current_search_match(&mut self) {
        if let Some(match_loc) = self.chat_search.match_locations.get(self.chat_search.current_match) {
            let messages = self.messages();
            // Find the message index
            if let Some((msg_idx, _)) = messages.iter().enumerate()
                .find(|(_, m)| m.id == match_loc.message_id)
            {
                self.selected_message_index = msg_idx;
                // Scroll will happen naturally in render
            }
        }
    }

    /// Check if a message ID has search matches
    pub fn message_has_search_match(&self, message_id: &str) -> bool {
        self.chat_search.active &&
            self.chat_search.match_locations.iter().any(|m| m.message_id == message_id)
    }

    /// Get search matches for a specific message
    pub fn get_message_search_matches(&self, message_id: &str) -> Vec<&ChatSearchMatch> {
        if !self.chat_search.active {
            return vec![];
        }
        self.chat_search.match_locations.iter()
            .filter(|m| m.message_id == message_id)
            .collect()
    }

    /// Check if a match is the currently focused one
    pub fn is_current_search_match(&self, message_id: &str, start_offset: usize) -> bool {
        if let Some(current) = self.chat_search.match_locations.get(self.chat_search.current_match) {
            current.message_id == message_id && current.start_offset == start_offset
        } else {
            false
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
        let grouped = group_messages(&display_messages);

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

    /// Toggle visibility of archived projects
    pub fn toggle_show_archived_projects(&mut self) {
        self.show_archived_projects = !self.show_archived_projects;
        if self.show_archived_projects {
            self.notify(Notification::info("Showing archived projects"));
        } else {
            self.notify(Notification::info("Hiding archived projects"));
        }
    }

    /// Check if a project is archived
    pub fn is_project_archived(&self, project_a_tag: &str) -> bool {
        self.preferences.borrow().is_project_archived(project_a_tag)
    }

    /// Toggle archive status of a project
    pub fn toggle_project_archived(&mut self, project_a_tag: &str) -> bool {
        self.preferences.borrow_mut().toggle_project_archived(project_a_tag)
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
