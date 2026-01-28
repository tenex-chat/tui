use crate::models::{AskEvent, ChatDraft, DraftImageAttachment, DraftPasteAttachment, Message, NamedDraft, PreferencesStorage, Project, ProjectAgent, ProjectStatus, Thread, TimeFilter};
use crate::nostr::DataChange;
use crate::store::{get_trace_context, AppDataStore, Database};
use crate::ui::ask_input::AskInputState;
use crate::ui::components::{ReportCoordinate, SidebarDelegation, SidebarReport, SidebarState};
use crate::ui::modal::{CommandPaletteState, ModalState};
use crate::ui::notifications::Notification;
use crate::ui::services::{AnimationClock, DraftService, NotificationManager};
use crate::ui::selector::SelectorState;
use crate::ui::state::{ChatSearchMatch, ChatSearchState, ConversationState, HomeViewState, LocalStreamBuffer, NavigationStackEntry, OpenTab, TabManager, ViewLocation};
use crate::ui::text_editor::{ImageAttachment, PasteAttachment, TextEditor};
use nostr_sdk::Keys;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use tenex_core::runtime::CoreHandle;
use tenex_core::tlog;

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
    Conversations,
    Inbox,
    Reports,
    Feed,
    ActiveWork,
    Stats,
}

/// Subtabs within the Stats view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StatsSubtab {
    #[default]
    Chart,
    Rankings,
}

impl StatsSubtab {
    /// Get the next subtab (wraps around)
    pub fn next(self) -> Self {
        match self {
            StatsSubtab::Chart => StatsSubtab::Rankings,
            StatsSubtab::Rankings => StatsSubtab::Chart,
        }
    }

    /// Get the previous subtab (wraps around)
    pub fn prev(self) -> Self {
        match self {
            StatsSubtab::Chart => StatsSubtab::Rankings,
            StatsSubtab::Rankings => StatsSubtab::Chart,
        }
    }
}

// ChatSearchState, ChatSearchMatch, OpenTab, TabManager, HomeViewState, ChatViewState,
// LocalStreamBuffer, ConversationState are now in ui::state module

/// Focus state for the context line (agent/model/branch bar below input)
/// None means the text input is focused, Some(X) means item X in context line is selected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContextFocus {
    /// Agent name is selected
    Agent,
    /// Model selector is selected
    Model,
    /// Branch is selected
    Branch,
    /// Nudge selector is selected
    Nudge,
}

/// A feed item representing a kind:1 event (text note) from a project
#[derive(Debug, Clone)]
pub struct FeedItem {
    pub content: String,
    pub pubkey: String,
    pub created_at: u64,
    pub thread_id: String,
    pub thread_title: String,
    pub project_a_tag: String,
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
    /// Conversation state (thread/agent selection, subthreads, message display) - private, use accessor methods
    conversation: ConversationState,

    pub scroll_offset: usize,
    /// Maximum scroll offset (set after rendering to enable proper scroll clamping)
    pub max_scroll_offset: usize,
    /// Notification manager for toast/status messages (private - use accessor methods)
    notification_manager: NotificationManager,
    /// Animation clock for UI spinners and pulsing indicators (private - use accessor methods)
    animation_clock: AnimationClock,

    pub creating_thread: bool,
    pub selected_branch: Option<String>,

    pub core_handle: Option<CoreHandle>,
    pub data_rx: Option<Receiver<DataChange>>,

    /// Whether user pressed Ctrl+C once (pending quit confirmation)
    pub pending_quit: bool,

    /// Unified draft service for persisting message drafts and named drafts
    draft_service: DraftService,

    /// Fallback text editor for chat input when no tab is active.
    /// This should rarely be used - only as a fallback when tabs are empty.
    /// The per-tab editors in OpenTab.editor are the primary editors.
    fallback_editor: TextEditor,

    /// Whether attachment modal is open
    pub showing_attachment_modal: bool,

    /// Editor for the attachment modal content
    pub attachment_modal_editor: TextEditor,

    /// Current wrap width for chat input (updated during rendering for visual line navigation)
    pub chat_input_wrap_width: usize,

    /// Single source of truth for app data
    pub data_store: Rc<RefCell<AppDataStore>>,

    /// Event stats for debugging (network events received)
    pub event_stats: tenex_core::stats::SharedEventStats,

    /// Subscription stats for debugging (active subscriptions and their event counts)
    pub subscription_stats: tenex_core::stats::SharedSubscriptionStats,

    /// Negentropy sync stats for debugging
    pub negentropy_stats: tenex_core::stats::SharedNegentropySyncStats,

    // NOTE: subthread_root, subthread_root_message, selected_message_index
    // are now in ConversationState (accessed via conversation field)

    /// Tab management (open tabs, history, modal state)
    pub tabs: TabManager,

    // Home view state
    pub home_panel_focus: HomeTab,
    /// Per-tab selection index (preserves position when switching tabs)
    pub tab_selection: HashMap<HomeTab, usize>,
    /// Multi-selected thread IDs for batch operations
    pub multi_selected_threads: HashSet<String>,
    pub report_search_filter: String,
    /// Whether sidebar is focused (vs content area)
    pub sidebar_focused: bool,
    /// Selected index in sidebar project list
    pub sidebar_project_index: usize,
    /// Projects to show in Recent/Inbox (empty = none)
    pub visible_projects: HashSet<String>,

    /// Home view state (time filter, archive toggle, agent browser)
    pub home: HomeViewState,

    pub preferences: RefCell<PreferencesStorage>,

    /// Unified modal state
    pub modal_state: ModalState,

    // Lesson viewer state
    pub viewing_lesson_id: Option<String>,
    pub lesson_viewer_section: usize,

    // Search modal state (deprecated - replaced by Search tab)
    pub showing_search_modal: bool,
    pub search_filter: String,
    pub search_index: usize,

    // NOTE: chat_search is now per-tab in OpenTab.chat_search
    // NOTE: local_stream_buffers, show_llm_metadata are now in ConversationState

    /// Toggle for showing/hiding the todo sidebar
    pub todo_sidebar_visible: bool,

    /// State for the chat sidebar (delegations, reports, focus)
    pub sidebar_state: SidebarState,

    /// Collapsed thread IDs (parent threads whose children are hidden)
    pub collapsed_threads: HashSet<String>,

    /// Project a_tag when waiting for a newly created thread to appear
    pub pending_new_thread_project: Option<String>,
    /// Draft ID when waiting for a newly created thread (to convert draft tab)
    pub pending_new_thread_draft_id: Option<String>,

    // NOTE: selected_nudge_ids is now per-tab in OpenTab.selected_nudge_ids
    // NOTE: frame_counter is now managed by notification_manager
    // NOTE: message_history is now per-tab in OpenTab.message_history
    // NOTE: chat_search is now per-tab in OpenTab.chat_search

    /// Whether to show archived items (conversations, projects, etc.) - unified global state
    pub show_archived: bool,
    /// Whether to hide scheduled events from conversation list
    pub hide_scheduled: bool,
    /// Whether user explicitly selected an agent in the current conversation
    /// When true, don't auto-sync agent from conversation messages
    pub user_explicitly_selected_agent: bool,
    /// Last action that can be undone (Ctrl+T + u)
    pub last_undo_action: Option<UndoAction>,
    /// Focus state for the context line (None = text input focused)
    pub input_context_focus: Option<InputContextFocus>,
    /// Sidebar search state (Ctrl+T + /)
    pub sidebar_search: crate::ui::search::SidebarSearchState,
    /// Channel sender for publish confirmations (draft_key, event_id)
    /// Used to notify the runtime when a message has been published
    pub publish_confirm_tx: Option<tokio::sync::mpsc::Sender<(String, String)>>,
    /// Current subtab within the Stats view
    pub stats_subtab: StatsSubtab,
}

impl App {
    pub fn new(
        db: Arc<Database>,
        data_store: Rc<RefCell<AppDataStore>>,
        event_stats: tenex_core::stats::SharedEventStats,
        subscription_stats: tenex_core::stats::SharedSubscriptionStats,
        negentropy_stats: tenex_core::stats::SharedNegentropySyncStats,
        data_dir: &str,
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
            conversation: ConversationState::new(),

            scroll_offset: 0,
            max_scroll_offset: 0,
            notification_manager: NotificationManager::new(),
            animation_clock: AnimationClock::new(),

            creating_thread: false,
            selected_branch: None,

            core_handle: None,
            data_rx: None,

            pending_quit: false,
            draft_service: DraftService::new(data_dir),
            fallback_editor: TextEditor::new(),
            showing_attachment_modal: false,
            attachment_modal_editor: TextEditor::new(),
            chat_input_wrap_width: 80, // Default, updated during rendering
            data_store,
            event_stats,
            subscription_stats,
            negentropy_stats,
            tabs: TabManager::new(),
            home_panel_focus: HomeTab::Conversations,
            tab_selection: HashMap::new(),
            multi_selected_threads: HashSet::new(),
            report_search_filter: String::new(),
            sidebar_focused: false,
            sidebar_project_index: 0,
            visible_projects: HashSet::new(),
            home: HomeViewState::new(),
            preferences: RefCell::new(PreferencesStorage::new(data_dir)),
            modal_state: ModalState::None,
            viewing_lesson_id: None,
            lesson_viewer_section: 0,
            showing_search_modal: false,
            search_filter: String::new(),
            search_index: 0,
            // NOTE: chat_search is now per-tab in OpenTab
            // NOTE: local_stream_buffers, show_llm_metadata are in ConversationState
            todo_sidebar_visible: true,
            sidebar_state: SidebarState::new(),
            collapsed_threads: HashSet::new(),
            pending_new_thread_project: None,
            pending_new_thread_draft_id: None,
            // NOTE: selected_nudge_ids is now per-tab in OpenTab
            // NOTE: frame_counter is now managed by notification_manager
            // NOTE: message_history is now per-tab in OpenTab
            show_archived: false,
            hide_scheduled: false,
            user_explicitly_selected_agent: false,
            last_undo_action: None,
            input_context_focus: None,
            sidebar_search: crate::ui::search::SidebarSearchState::new(),
            publish_confirm_tx: None,
            stats_subtab: StatsSubtab::default(),
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

    // =============================================================================
    // PER-TAB EDITOR ACCESSORS
    // =============================================================================
    // These methods provide access to the per-tab TextEditor, ensuring proper
    // isolation between tabs. Each tab has its own editor state.

    /// Get reference to the current tab's chat editor.
    /// Falls back to the fallback_editor if no tab is open.
    /// In debug builds, asserts if fallback is used in chat view (indicates a bug).
    #[inline]
    pub fn chat_editor(&self) -> &TextEditor {
        if let Some(tab) = self.tabs.active_tab() {
            &tab.editor
        } else {
            // Debug assertion: using fallback in chat view indicates cross-tab contamination bug
            debug_assert!(
                self.view != View::Chat,
                "chat_editor() fallback used in Chat view - this indicates a tab isolation bug"
            );
            &self.fallback_editor
        }
    }

    /// Get mutable reference to the current tab's chat editor.
    /// Falls back to the fallback_editor if no tab is open.
    /// In debug builds, asserts if fallback is used in chat view (indicates a bug).
    #[inline]
    pub fn chat_editor_mut(&mut self) -> &mut TextEditor {
        let active_index = self.tabs.active_index();
        let tabs = self.tabs.tabs_mut();
        if active_index < tabs.len() {
            &mut tabs[active_index].editor
        } else {
            // Debug assertion: using fallback in chat view indicates cross-tab contamination bug
            debug_assert!(
                self.view != View::Chat,
                "chat_editor_mut() fallback used in Chat view - this indicates a tab isolation bug"
            );
            &mut self.fallback_editor
        }
    }

    // =============================================================================
    // TICK AND ANIMATION METHODS
    // =============================================================================

    /// Increment frame counter and update notifications (call on each tick)
    pub fn tick(&mut self) {
        self.animation_clock.tick();
        self.notification_manager.tick();
    }

    /// Get spinner character based on frame counter
    pub fn spinner_char(&self) -> char {
        self.animation_clock.spinner_char()
    }

    /// Get activity indicator for pulsing displays (◉/○)
    pub fn activity_indicator(&self) -> &'static str {
        self.animation_clock.activity_indicator()
    }

    /// Get activity pulse state (true = on, false = off)
    pub fn activity_pulse(&self) -> bool {
        self.animation_clock.activity_pulse()
    }

    // =============================================================================
    // NOTIFICATION METHODS
    // =============================================================================

    /// Add a notification to the queue
    pub fn notify(&mut self, notification: Notification) {
        self.notification_manager.notify(notification);
    }

    /// Set a warning status message (legacy compatibility)
    pub fn set_warning_status(&mut self, message: &str) {
        self.notification_manager.set_warning_status(message);
    }

    /// Dismiss the current notification
    pub fn dismiss_notification(&mut self) {
        self.notification_manager.dismiss();
    }

    /// Get the current notification being displayed
    pub fn current_notification(&self) -> Option<&Notification> {
        self.notification_manager.current()
    }

    /// Check if there are any active notifications
    pub fn has_notifications(&self) -> bool {
        self.notification_manager.has_notifications()
    }

    // =============================================================================
    // CONVERSATION STATE ACCESSOR METHODS (backward compatibility)
    // =============================================================================
    // These methods delegate to ConversationState for backward compatibility with code
    // that accesses app.selected_thread, app.selected_agent, etc. directly.

    /// Get reference to the selected thread
    #[inline]
    pub fn selected_thread(&self) -> Option<&Thread> {
        self.conversation.selected_thread.as_ref()
    }

    /// Set the selected thread
    #[inline]
    pub fn set_selected_thread(&mut self, thread: Option<Thread>) {
        self.conversation.selected_thread = thread;
    }

    /// Get reference to the selected agent
    #[inline]
    pub fn selected_agent(&self) -> Option<&ProjectAgent> {
        self.conversation.selected_agent.as_ref()
    }

    /// Set the selected agent
    #[inline]
    pub fn set_selected_agent(&mut self, agent: Option<ProjectAgent>) {
        self.conversation.selected_agent = agent;
    }

    /// Get the subthread root message ID (if viewing a subthread)
    #[inline]
    pub fn subthread_root(&self) -> Option<&String> {
        self.conversation.subthread_root.as_ref()
    }

    /// Get the subthread root message (if viewing a subthread)
    #[inline]
    pub fn subthread_root_message(&self) -> Option<&Message> {
        self.conversation.subthread_root_message.as_ref()
    }

    /// Get the selected message index
    #[inline]
    pub fn selected_message_index(&self) -> usize {
        self.conversation.selected_message_index
    }

    /// Set the selected message index
    #[inline]
    pub fn set_selected_message_index(&mut self, index: usize) {
        self.conversation.selected_message_index = index;
    }

    /// Get the LLM metadata display toggle
    #[inline]
    pub fn show_llm_metadata(&self) -> bool {
        self.conversation.show_llm_metadata
    }

    /// Set the LLM metadata display toggle
    #[inline]
    pub fn set_show_llm_metadata(&mut self, show: bool) {
        self.conversation.show_llm_metadata = show;
    }

    /// Get reference to local stream buffers
    #[inline]
    pub fn local_stream_buffers(&self) -> &HashMap<String, LocalStreamBuffer> {
        &self.conversation.local_stream_buffers
    }

    /// Get mutable reference to local stream buffers
    #[inline]
    pub fn local_stream_buffers_mut(&mut self) -> &mut HashMap<String, LocalStreamBuffer> {
        &mut self.conversation.local_stream_buffers
    }

    // =============================================================================
    // THREAD COLLAPSE METHODS
    // =============================================================================

    /// Toggle collapse state for a thread (for hierarchical folding)
    pub fn toggle_thread_collapse(&mut self, thread_id: &str) {
        if self.collapsed_threads.contains(thread_id) {
            self.collapsed_threads.remove(thread_id);
        } else {
            self.collapsed_threads.insert(thread_id.to_string());
        }
    }

    /// Toggle collapse/expand all threads and persist the setting.
    /// Returns true if threads are now collapsed, false if expanded.
    ///
    /// With the new inverted logic:
    /// - When default_collapsed is true: presence in collapsed_threads means EXPANDED
    /// - When default_collapsed is false: presence in collapsed_threads means COLLAPSED
    ///
    /// So when toggling, we clear the set to reset all overrides.
    pub fn toggle_collapse_all_threads(&mut self) -> bool {
        let now_collapsed = self.preferences.borrow_mut().toggle_threads_default_collapsed();

        // Clear all individual overrides so the default takes effect
        self.collapsed_threads.clear();

        now_collapsed
    }

    /// Check if threads are default collapsed (from preferences)
    pub fn threads_default_collapsed(&self) -> bool {
        self.preferences.borrow().threads_default_collapsed()
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
        self.conversation.selected_thread.as_ref()
            .map(|t| self.data_store.borrow().get_messages(&t.id).to_vec())
            .unwrap_or_default()
    }

    /// Filter messages for current view (subthread or main thread).
    /// This helper consolidates the filtering logic used by display methods.
    fn filter_messages_for_view<'a>(messages: &'a [Message], thread_id: Option<&str>, subthread_root: Option<&str>) -> Vec<&'a Message> {
        if let Some(root_id) = subthread_root {
            messages.iter()
                .filter(|m| m.reply_to.as_deref() == Some(root_id))
                .collect()
        } else {
            messages.iter()
                .filter(|m| {
                    Some(m.id.as_str()) == thread_id
                        || m.reply_to.is_none()
                        || m.reply_to.as_deref() == thread_id
                })
                .collect()
        }
    }

    /// Get the ID of the currently selected message (if any)
    pub fn selected_message_id(&self) -> Option<String> {
        use crate::ui::views::chat::{group_messages, DisplayItem};

        let messages = self.messages();
        let thread_id = self.conversation.selected_thread.as_ref().map(|t| t.id.as_str());
        let subthread_root = self.conversation.subthread_root.as_deref();

        let display_messages = Self::filter_messages_for_view(&messages, thread_id, subthread_root);
        let grouped = group_messages(&display_messages);

        grouped.get(self.conversation.selected_message_index).and_then(|item| {
            match item {
                DisplayItem::SingleMessage { message, .. } => Some(message.id.clone()),
                DisplayItem::DelegationPreview { .. } => None,
            }
        })
    }

    /// Check if the currently selected message has a trace context
    pub fn selected_message_has_trace(&self) -> bool {
        self.selected_message_id()
            .map(|id| get_trace_context(&self.db.ndb, &id).is_some())
            .unwrap_or(false)
    }

    /// Get the count of display items in the current chat view.
    /// Used for navigation bounds checking.
    pub fn display_item_count(&self) -> usize {
        use crate::ui::views::chat::group_messages;

        let messages = self.messages();
        let thread_id = self.conversation.selected_thread.as_ref().map(|t| t.id.as_str());
        let subthread_root = self.conversation.subthread_root.as_deref();

        let display_messages = Self::filter_messages_for_view(&messages, thread_id, subthread_root);
        group_messages(&display_messages).len()
    }

    /// Enter a subthread view rooted at the given message
    pub fn enter_subthread(&mut self, message: Message) {
        self.conversation.enter_subthread(message);
        self.scroll_offset = 0;
    }

    /// Exit the current subthread view and return to parent
    pub fn exit_subthread(&mut self) {
        self.conversation.exit_subthread();
    }

    /// Check if we're currently viewing a subthread
    pub fn in_subthread(&self) -> bool {
        self.conversation.in_subthread()
    }

    /// Convert TextEditor attachments to draft format.
    /// This is the single source of truth for attachment conversion.
    fn convert_attachments_to_draft(editor: &crate::ui::text_editor::TextEditor) -> (Vec<DraftPasteAttachment>, Vec<DraftImageAttachment>) {
        let paste_attachments: Vec<DraftPasteAttachment> = editor
            .attachments
            .iter()
            .map(|a| DraftPasteAttachment {
                id: a.id,
                content: a.content.clone(),
            })
            .collect();

        let image_attachments: Vec<DraftImageAttachment> = editor
            .image_attachments
            .iter()
            .map(|a| DraftImageAttachment {
                id: a.id,
                url: a.url.clone(),
            })
            .collect();

        (paste_attachments, image_attachments)
    }

    /// Save current chat editor content as draft for the selected thread or draft tab
    pub fn save_chat_draft(&self) {
        // Determine the draft key - use thread id or draft_id from active tab
        let draft_key = if let Some(ref thread) = self.conversation.selected_thread {
            Some(thread.id.clone())
        } else {
            // Check if current tab is a draft tab
            self.tabs.active_tab().and_then(|t| t.draft_id.clone())
        };

        if let Some(conversation_id) = draft_key {
            // Only save agent to draft if user explicitly selected it
            // Otherwise let sync_agent_with_conversation() determine it each time
            let agent_pubkey = if self.user_explicitly_selected_agent {
                self.conversation.selected_agent.as_ref().map(|a| a.pubkey.clone())
            } else {
                None
            };
            tlog!("AGENT", "save_chat_draft: key={}, explicit={}, agent={:?}",
                conversation_id,
                self.user_explicitly_selected_agent,
                agent_pubkey.as_ref().map(|p| &p[..8])
            );

            // Convert attachments using helper
            let editor = self.chat_editor();
            let (attachments, image_attachments) = Self::convert_attachments_to_draft(editor);

            // Get reference_conversation_id from active tab if it exists
            let reference_conversation_id = self.tabs.active_tab()
                .and_then(|t| t.reference_conversation_id.clone());

            let draft = ChatDraft {
                conversation_id: conversation_id.clone(),
                text: editor.text.clone(), // Raw text, not build_full_content()
                attachments,
                image_attachments,
                selected_agent_pubkey: agent_pubkey,
                selected_branch: self.selected_branch.clone(),
                last_modified: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                reference_conversation_id,
                // BULLETPROOF: New drafts are unpublished - they stay until relay confirms
                published_at: None,
                published_event_id: None,
            };
            if let Err(e) = self.draft_service.save_chat_draft(draft) {
                // BULLETPROOF: Log I/O errors but don't interrupt user - draft may still be in memory
                tlog!("DRAFT", "ERROR saving draft for {}: {}", conversation_id, e);
            }
        }
    }

    /// Save draft from a removed tab's editor (used when closing tabs).
    /// This is needed because after a tab is removed, the per-tab accessor
    /// would return the wrong editor.
    pub fn save_draft_from_tab(&self, tab: &crate::ui::state::OpenTab) {
        // Determine the draft key from the removed tab
        let draft_key = if !tab.thread_id.is_empty() {
            Some(tab.thread_id.clone())
        } else {
            tab.draft_id.clone()
        };

        if let Some(conversation_id) = draft_key {
            // IMPORTANT: Load existing draft to preserve its agent/branch metadata.
            // We cannot use self.selected_agent/selected_branch because those belong
            // to the ACTIVE tab, not necessarily the tab being closed.
            let existing_draft = self.draft_service.load_chat_draft(&conversation_id);
            let (agent_pubkey, branch) = if let Some(ref draft) = existing_draft {
                (draft.selected_agent_pubkey.clone(), draft.selected_branch.clone())
            } else {
                // No existing draft - this is a new draft, use None for both
                // (the agent/branch will be set when the user selects them for this tab)
                (None, None)
            };

            tlog!("AGENT", "save_draft_from_tab: key={}, agent={:?}, branch={:?} (preserved from existing draft)",
                conversation_id,
                agent_pubkey,
                branch
            );

            // Convert attachments using helper
            let (attachments, image_attachments) = Self::convert_attachments_to_draft(&tab.editor);

            let draft = ChatDraft {
                conversation_id: conversation_id.clone(),
                text: tab.editor.text.clone(),
                attachments,
                image_attachments,
                selected_agent_pubkey: agent_pubkey,
                selected_branch: branch,
                last_modified: std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0),
                reference_conversation_id: tab.reference_conversation_id.clone(),
                // BULLETPROOF: Drafts from closed tabs are unpublished
                published_at: None,
                published_event_id: None,
            };
            if let Err(e) = self.draft_service.save_chat_draft(draft) {
                // BULLETPROOF: Log I/O errors but don't interrupt - critical for tab close flow
                tlog!("DRAFT", "ERROR saving draft from tab {}: {}", conversation_id, e);
            }
        }
    }

    /// Restore draft for the selected thread or draft tab into chat_editor
    /// Priority: draft values > conversation sync > defaults
    pub fn restore_chat_draft(&mut self) {
        let thread_id = self.conversation.selected_thread.as_ref().map(|t| t.id.clone());
        tlog!("AGENT", "restore_chat_draft called, thread_id={:?}", thread_id);

        // Reset explicit selection flag when switching conversations
        self.user_explicitly_selected_agent = false;

        // Always clear editor first - each conversation has its own draft (per-tab editor)
        {
            let editor = self.chat_editor_mut();
            editor.text.clear();
            editor.cursor = 0;
            editor.attachments.clear();
            editor.image_attachments.clear();
            editor.focused_attachment = None;
        }

        // Determine the draft key - use thread id or draft_id from active tab
        let draft_key = if let Some(ref thread) = self.conversation.selected_thread {
            Some(thread.id.clone())
        } else {
            // Check if current tab is a draft tab
            self.tabs.active_tab().and_then(|t| t.draft_id.clone())
        };

        tlog!("AGENT", "restore_chat_draft: draft_key={:?}", draft_key);

        // Track whether draft had explicit agent/branch selections
        let mut draft_had_agent = false;
        let mut draft_had_branch = false;

        if let Some(key) = draft_key {
            // Load draft into per-tab editor
            let draft_opt = self.draft_service.load_chat_draft(&key);
            if let Some(draft) = draft_opt {
                {
                    let editor = self.chat_editor_mut();
                    editor.text = draft.text.clone();
                    editor.cursor = editor.text.len();

                    // Restore attachments from draft
                    editor.attachments = draft
                        .attachments
                        .iter()
                        .map(|a| PasteAttachment {
                            id: a.id,
                            content: a.content.clone(),
                        })
                        .collect();

                    editor.image_attachments = draft
                        .image_attachments
                        .iter()
                        .map(|a| ImageAttachment { id: a.id, url: a.url.clone() })
                        .collect();
                }

                tlog!("AGENT", "restore_chat_draft: loaded draft, agent_pubkey={:?}, branch={:?}",
                    draft.selected_agent_pubkey.as_ref().map(|p| &p[..8.min(p.len())]),
                    draft.selected_branch
                );

                // Restore agent from draft if one was saved (takes priority over sync)
                if let Some(ref agent_pubkey) = draft.selected_agent_pubkey {
                    // Find agent by pubkey in available agents
                    let agent = self.available_agents()
                        .into_iter()
                        .find(|a| &a.pubkey == agent_pubkey);
                    if let Some(agent) = agent {
                        tlog!("AGENT", "restore_chat_draft: restoring agent from draft='{}' (pubkey={})",
                            agent.name, &agent.pubkey[..8]);
                        self.conversation.selected_agent = Some(agent);
                        draft_had_agent = true;
                    } else {
                        tlog!("AGENT", "restore_chat_draft: draft agent_pubkey={} NOT FOUND in available_agents",
                            &agent_pubkey[..8.min(agent_pubkey.len())]);
                    }
                }
                // Restore branch from draft if one was saved (takes priority over sync)
                if draft.selected_branch.is_some() {
                    self.selected_branch = draft.selected_branch.clone();
                    draft_had_branch = true;
                }

                // Restore reference_conversation_id from draft into the active tab
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.reference_conversation_id = draft.reference_conversation_id.clone();
                }
            } else {
                tlog!("AGENT", "restore_chat_draft: no draft found for key");
            }
        }

        // For real threads, sync agent and branch with conversation ONLY if draft didn't have values
        // This ensures draft selections are preserved while still providing sensible defaults
        if self.conversation.selected_thread.is_some() {
            if !draft_had_agent {
                tlog!("AGENT", "restore_chat_draft: draft had no agent, calling sync_agent_with_conversation");
                self.sync_agent_with_conversation();
            } else {
                tlog!("AGENT", "restore_chat_draft: draft had agent, skipping sync");
            }
            if !draft_had_branch {
                self.sync_branch_with_conversation();
            }
        }

        tlog!("AGENT", "restore_chat_draft done, selected_agent={:?}",
            self.conversation.selected_agent.as_ref().map(|a| format!("{}({})", a.name, &a.pubkey[..8]))
        );
    }

    /// Sync selected_agent with the most recent agent in the conversation
    /// Falls back to PM agent if no agent has responded yet
    pub fn sync_agent_with_conversation(&mut self) {
        tlog!("AGENT", "sync_agent_with_conversation called");

        // First try to get the most recent agent from the conversation
        if let Some(recent_agent) = self.get_most_recent_agent_from_conversation() {
            tlog!("AGENT", "sync_agent: setting to recent_agent='{}' (pubkey={})",
                recent_agent.name, &recent_agent.pubkey[..8]);
            self.conversation.selected_agent = Some(recent_agent);
            return;
        }

        // Fall back to PM agent if no agent has responded yet
        if let Some(status) = self.get_selected_project_status() {
            if let Some(pm) = status.pm_agent() {
                tlog!("AGENT", "sync_agent: no recent agent, falling back to PM='{}' (pubkey={})",
                    pm.name, &pm.pubkey[..8]);
                self.conversation.selected_agent = Some(pm.clone());
            } else {
                tlog!("AGENT", "sync_agent: no recent agent and no PM available");
            }
        } else {
            tlog!("AGENT", "sync_agent: no project status available");
        }
    }

    /// Create a publish snapshot for a message about to be sent.
    /// Returns the unique publish_id for tracking confirmation.
    /// BULLETPROOF: This captures exactly what was sent, separate from the current draft.
    pub fn create_publish_snapshot(&self, conversation_id: &str, content: String) -> Result<String, tenex_core::models::draft::DraftStorageError> {
        self.draft_service.create_publish_snapshot(conversation_id, content)
    }

    /// Mark a publish snapshot as confirmed (call after relay confirmation)
    /// Uses the unique publish_id to mark the specific snapshot - doesn't affect current draft.
    /// BULLETPROOF: New typing after send won't be lost because snapshots are separate.
    pub fn mark_publish_confirmed(&self, publish_id: &str, event_id: Option<String>) -> Result<bool, tenex_core::models::draft::DraftStorageError> {
        self.draft_service.mark_publish_confirmed(publish_id, event_id)
    }

    /// Set the publish confirmation sender (called from runtime)
    pub fn set_publish_confirm_tx(&mut self, tx: tokio::sync::mpsc::Sender<(String, String)>) {
        self.publish_confirm_tx = Some(tx);
    }

    /// Get the publish confirmation sender (clone for async use)
    pub fn get_publish_confirm_tx(&self) -> Option<tokio::sync::mpsc::Sender<(String, String)>> {
        self.publish_confirm_tx.clone()
    }

    /// Clean up old confirmed publish snapshots (call on app startup or after confirmations)
    /// Returns the number of snapshots cleaned up
    pub fn cleanup_confirmed_publishes(&self) -> Result<usize, tenex_core::models::draft::DraftStorageError> {
        self.draft_service.cleanup_confirmed_publishes()
    }

    /// Remove a publish snapshot (for rollback when send fails)
    /// Call this when send to relay fails AFTER snapshot was created
    pub fn remove_publish_snapshot(&self, publish_id: &str) -> Result<bool, tenex_core::models::draft::DraftStorageError> {
        self.draft_service.remove_publish_snapshot(publish_id)
    }

    /// Get the last draft storage error (if any)
    pub fn draft_storage_last_error(&self) -> Option<String> {
        self.draft_service.chat_draft_last_error()
    }

    /// Clear the last draft storage error
    pub fn draft_storage_clear_error(&self) {
        self.draft_service.chat_draft_clear_error();
    }

    /// Get unpublished drafts for recovery (call on app startup)
    pub fn get_unpublished_drafts(&self) -> Vec<tenex_core::models::draft::ChatDraft> {
        self.draft_service.get_unpublished_drafts()
    }

    /// Get pending (unconfirmed) publish snapshots
    pub fn get_pending_publishes(&self) -> Vec<tenex_core::models::draft::PendingPublishSnapshot> {
        self.draft_service.get_pending_publishes()
    }

    /// Save current chat editor content as a named draft for the current project
    pub fn save_named_draft(&mut self) {
        let text = self.chat_editor().build_full_content();
        if text.trim().is_empty() {
            self.set_warning_status("Cannot save empty draft");
            return;
        }

        // Require a project to be selected for saving drafts
        let project_a_tag = match &self.selected_project {
            Some(p) => p.a_tag(),
            None => {
                self.set_warning_status("Cannot save draft: no project selected");
                return;
            }
        };

        let draft = NamedDraft::new(text, project_a_tag);
        let draft_name = draft.name.clone();

        // Perform the save and capture result before calling set_status
        let result = self.draft_service.save_named_draft(draft);

        match result {
            Ok(()) => self.set_warning_status(&format!("Draft saved: {}", draft_name)),
            Err(e) => self.set_warning_status(&format!("Failed to save draft: {}", e)),
        }
    }

    /// Get named drafts for the current project
    pub fn get_named_drafts_for_current_project(&self) -> Vec<NamedDraft> {
        let project_a_tag = self.selected_project
            .as_ref()
            .map(|p| p.a_tag())
            .unwrap_or_default();

        self.draft_service.get_named_drafts_for_project(&project_a_tag)
    }

    /// Get all named drafts
    pub fn get_all_named_drafts(&self) -> Vec<NamedDraft> {
        self.draft_service.get_all_named_drafts()
    }

    /// Delete a named draft by ID
    pub fn delete_named_draft(&mut self, id: &str) {
        // Perform the delete and capture result before calling set_status
        let result = self.draft_service.delete_named_draft(id);

        match result {
            Ok(()) => self.set_warning_status("Draft deleted"),
            Err(e) => self.set_warning_status(&format!("Failed to delete draft: {}", e)),
        }
    }

    /// Restore a named draft to the chat editor
    pub fn restore_named_draft(&mut self, draft: &NamedDraft) {
        let editor = self.chat_editor_mut();
        editor.text = draft.text.clone();
        editor.cursor = draft.text.len();
        self.input_mode = InputMode::Editing;
        self.set_warning_status(&format!("Draft restored: {}", draft.name));
    }

    /// Open the draft navigator modal (scoped to current project)
    pub fn open_draft_navigator(&mut self) {
        use crate::ui::modal::DraftNavigatorState;

        // Check for storage errors on init and surface them (extract error message first)
        let storage_error = self.draft_service.named_draft_last_error();
        if let Some(error) = storage_error {
            self.set_warning_status(&format!("Warning: {}", error));
            // Clear the error after surfacing it to avoid repeated warnings
            self.draft_service.named_draft_clear_error();
        }

        // Scope to current project - only show drafts for the selected project
        let drafts = self.get_named_drafts_for_current_project();
        let has_project = self.selected_project.is_some();

        if drafts.is_empty() && has_project {
            self.set_warning_status("No drafts for this project. Use Ctrl+T 's' in edit mode to save one.");
            return;
        } else if drafts.is_empty() {
            self.set_warning_status("No project selected and no drafts available.");
            return;
        }

        self.modal_state = ModalState::DraftNavigator(DraftNavigatorState::new(drafts));
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
        if let Some(attachment) = self.chat_editor().get_focused_attachment() {
            self.attachment_modal_editor.text = attachment.content.clone();
            self.attachment_modal_editor.cursor = 0;
            self.showing_attachment_modal = true;
        }
    }

    /// Save attachment modal changes and close
    pub fn save_and_close_attachment_modal(&mut self) {
        let new_content = self.attachment_modal_editor.text.clone();
        self.chat_editor_mut().update_focused_attachment(new_content);
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
        self.chat_editor_mut().delete_focused_attachment();
        self.attachment_modal_editor.clear();
        self.showing_attachment_modal = false;
    }

    /// Open expanded editor modal (Ctrl+E) for full-screen editing
    pub fn open_expanded_editor_modal(&mut self) {
        let mut editor = TextEditor::new();
        let chat_ed = self.chat_editor();
        editor.text = chat_ed.text.clone();
        editor.cursor = chat_ed.cursor;
        self.modal_state = ModalState::ExpandedEditor { editor };
    }

    /// Save expanded editor changes and close
    pub fn save_and_close_expanded_editor(&mut self) {
        if let ModalState::ExpandedEditor { editor } = &self.modal_state {
            let text = editor.text.clone();
            let cursor = editor.cursor;
            let chat_ed = self.chat_editor_mut();
            chat_ed.text = text;
            chat_ed.cursor = cursor;
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
    /// Archived projects are hidden unless show_archived is true
    /// When a workspace is active, only shows projects in that workspace
    pub fn filtered_projects(&self) -> (Vec<Project>, Vec<Project>) {
        let filter = self.projects_modal_filter();
        let store = self.data_store.borrow();
        let projects = store.get_projects();
        let prefs = self.preferences.borrow();

        // Get active workspace project IDs (if any)
        let workspace_project_ids: Option<std::collections::HashSet<&str>> = prefs
            .active_workspace()
            .map(|ws| ws.project_ids.iter().map(|s| s.as_str()).collect());

        let mut matching: Vec<_> = projects
            .iter()
            .filter(|p| {
                // Filter by workspace if active
                if let Some(ref ws_ids) = workspace_project_ids {
                    if !ws_ids.contains(p.a_tag().as_str()) {
                        return false;
                    }
                }
                // Filter out archived projects unless showing archived
                self.show_archived || !prefs.is_project_archived(&p.a_tag())
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
                DataChange::ProjectStatus { json } => {
                    self.data_store.borrow_mut().handle_status_event_json(&json);
                }
            }
        }
        Ok(())
    }

    /// Get the thread_id from the current notification (if it has one)
    pub fn current_notification_thread_id(&self) -> Option<String> {
        self.notification_manager.current().and_then(|n| n.thread_id.clone())
    }

    /// Jump to the thread referenced by the current notification (if any)
    /// Uses the canonical open_thread_from_home navigation flow to ensure proper
    /// project/agent/branch context, draft restore, sidebar updates, and view history.
    /// Returns true if we successfully navigated to the thread.
    pub fn jump_to_notification_thread(&mut self) -> bool {
        // Get thread_id from current notification
        let Some(thread_id) = self.current_notification_thread_id() else {
            // No notification with thread_id - show error (no notification to dismiss here)
            self.notify(Notification::warning("No message to jump to"));
            return false;
        };

        // Get thread and project info from data store
        let lookup_result = {
            let store = self.data_store.borrow();
            let thread = store.get_thread_by_id(&thread_id).cloned();
            let a_tag = store.find_project_for_thread(&thread_id);
            (thread, a_tag)
        };

        match lookup_result {
            (Some(thread), Some(project_a_tag)) => {
                // Save current draft before navigating (mirrors other navigation paths)
                self.save_chat_draft();

                // Dismiss the notification first since we're about to navigate
                self.dismiss_notification();

                // Use canonical navigation flow - this handles:
                // - Project/agent/branch context setup
                // - Draft restoration
                // - Sidebar updates
                // - View history tracking
                // - Auto-opening pending ask modals
                self.open_thread_from_home(&thread, &project_a_tag);
                true
            }
            (None, _) => {
                // Thread not found - show error and dismiss stale notification
                self.dismiss_notification();
                self.notify(Notification::warning("Thread no longer exists"));
                false
            }
            (_, None) => {
                // Project not found - show error and dismiss stale notification
                self.dismiss_notification();
                self.notify(Notification::warning("Project for thread not found"));
                false
            }
        }
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
        let thread = self.conversation.selected_thread.as_ref()?;
        let messages = self.messages();
        let available_agents = self.available_agents();
        let user_pubkey = self.data_store.borrow().user_pubkey.clone();

        tlog!("AGENT", "get_most_recent_agent: thread_id={}, messages={}, available_agents={:?}",
            thread.id,
            messages.len(),
            available_agents.iter().map(|a| format!("{}({})", a.name, &a.pubkey[..8])).collect::<Vec<_>>()
        );

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
                tlog!("AGENT", "  found agent message: pubkey={}, timestamp={}", &msg.pubkey[..8], msg.created_at);
            }
        }

        // Also check the thread itself (the original message that started the thread)
        // The thread author might be an agent - use last_activity as timestamp proxy
        // Note: for the thread root, we only consider it if no messages from agents exist yet
        if latest_agent_pubkey.is_none() && agent_pubkeys.contains(thread.pubkey.as_str()) {
            if user_pubkey.as_ref().map(|pk| pk != &thread.pubkey).unwrap_or(true) {
                latest_agent_pubkey = Some(thread.pubkey.as_str());
                tlog!("AGENT", "  using thread author as agent: pubkey={}", &thread.pubkey[..8]);
            }
        }

        // Find and return the matching agent
        let result = latest_agent_pubkey.and_then(|pubkey| {
            available_agents.into_iter().find(|a| a.pubkey == pubkey)
        });

        tlog!("AGENT", "get_most_recent_agent result: {:?}",
            result.as_ref().map(|a| format!("{}({})", a.name, &a.pubkey[..8]))
        );

        result
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
        self.modal_state = ModalState::CommandPalette(CommandPaletteState::new());
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
            self.conversation.selected_agent = Some(project_agent.clone());
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

    /// Convert a draft tab to a real tab when thread is created.
    /// Also migrates the draft storage entry from the project-based key (e.g., "project_a_tag:new")
    /// to the new thread-based key (thread.id).
    pub fn convert_draft_to_tab(&mut self, draft_id: &str, thread: &Thread) {
        // Migrate the draft storage: load from old key, delete old entry, save with new key
        let draft_opt = self.draft_service.load_chat_draft(draft_id);
        if let Some(mut draft) = draft_opt {
            // Update the conversation_id to the new thread ID
            draft.conversation_id = thread.id.clone();

            // Delete the old draft keyed by project:new
            if let Err(e) = self.draft_service.delete_chat_draft(draft_id) {
                tlog!("DRAFT", "WARNING: failed to delete old draft key '{}': {}", draft_id, e);
            }

            // Save with the new thread ID as key (only if there's content to preserve)
            if !draft.is_empty() {
                if let Err(e) = self.draft_service.save_chat_draft(draft) {
                    tlog!("DRAFT", "ERROR migrating draft to new key '{}': {}", thread.id, e);
                }
            }
        }

        // Convert the tab in the tab manager
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

        // Close and get both the removed tab and the previous view location
        let (removed_tab, previous_view) = self.tabs.close_current();

        // Save draft from the removed tab's editor (before it's lost)
        if let Some(ref tab) = removed_tab {
            self.save_draft_from_tab(tab);
        }

        // Navigate to the previous view location from history
        match previous_view {
            Some(ViewLocation::Home) | None => {
                // Go back to home view
                self.fallback_editor.clear();
                self.conversation.selected_thread = None;
                self.view = View::Home;
            }
            Some(ViewLocation::Tab(index)) => {
                // Switch to the previous tab from history
                self.switch_to_tab(index);
            }
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

        tlog!("AGENT", "switch_to_tab: index={}, is_draft={}, thread_id={}, project={}",
            index, is_draft, thread_id, project_a_tag);

        if is_draft {
            // Draft tab - set up for new conversation
            self.conversation.selected_thread = None;
            self.creating_thread = true;

            // CRITICAL: Clear all context upfront to prevent stale state leaking
            // if project lookup fails below
            self.selected_project = None;
            self.conversation.selected_agent = None;
            self.selected_branch = None;
            tlog!("AGENT", "switch_to_tab(draft): cleared agent/branch");

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
                        tlog!("AGENT", "switch_to_tab(draft): setting PM='{}' (pubkey={})",
                            pm.name, &pm.pubkey[..8]);
                        self.conversation.selected_agent = Some(pm.clone());
                    }
                    // Always set default branch (removed .is_none() guard to prevent stale values)
                    self.selected_branch = status.default_branch().map(String::from);
                }
            }

            self.restore_chat_draft();
            self.scroll_offset = 0;
            self.conversation.selected_message_index = 0;
            self.conversation.subthread_root = None;
            self.conversation.subthread_root_message = None;
            self.input_mode = InputMode::Editing;
            self.view = View::Chat;
        } else {
            // Real tab - find the thread in data store
            let thread = self.data_store.borrow().get_threads(&project_a_tag)
                .iter()
                .find(|t| t.id == thread_id)
                .cloned();

            if let Some(thread) = thread {
                tlog!("AGENT", "switch_to_tab(real): found thread '{}'", thread.title);
                self.conversation.selected_thread = Some(thread);
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
                    let draft = self.draft_service.load_chat_draft(&thread_id);
                    let draft_has_agent = draft.as_ref().map(|d| d.selected_agent_pubkey.is_some()).unwrap_or(false);
                    let draft_has_branch = draft.as_ref().map(|d| d.selected_branch.is_some()).unwrap_or(false);

                    tlog!("AGENT", "switch_to_tab(real): draft_has_agent={}, draft_has_branch={}",
                        draft_has_agent, draft_has_branch);

                    // Set agent and branch defaults only if draft doesn't have them
                    if let Some(status) = self.data_store.borrow().get_project_status(&a_tag) {
                        if !draft_has_agent {
                            // No draft agent, use PM as default
                            if let Some(pm) = status.pm_agent() {
                                tlog!("AGENT", "switch_to_tab(real): no draft agent, setting PM='{}' (pubkey={}) BEFORE restore_chat_draft",
                                    pm.name, &pm.pubkey[..8]);
                                self.conversation.selected_agent = Some(pm.clone());
                            } else {
                                // No PM available, clear to prevent stale state
                                tlog!("AGENT", "switch_to_tab(real): no draft agent and no PM, clearing");
                                self.conversation.selected_agent = None;
                            }
                        } else {
                            tlog!("AGENT", "switch_to_tab(real): draft has agent, skipping PM default");
                        }

                        if !draft_has_branch {
                            self.selected_branch = status.default_branch().map(String::from);
                        }
                    } else {
                        // No project status, clear agent/branch to prevent stale state
                        tlog!("AGENT", "switch_to_tab(real): no project status, clearing agent/branch");
                        self.conversation.selected_agent = None;
                        self.selected_branch = None;
                    }

                    // Now restore the draft (uses cached load if same key)
                    self.restore_chat_draft();
                } else {
                    // Project lookup failed - clear all context to prevent stale state leaking
                    tlog!("AGENT", "switch_to_tab(real): project lookup failed, clearing all");
                    self.selected_project = None;
                    self.conversation.selected_agent = None;
                    self.selected_branch = None;
                }
                self.scroll_offset = usize::MAX; // Scroll to bottom
                self.conversation.selected_message_index = 0;
                self.conversation.subthread_root = None;
                self.conversation.subthread_root_message = None;
                self.input_mode = InputMode::Editing; // Auto-focus input
                self.view = View::Chat; // Switch to Chat view
            }
        }

        tlog!("AGENT", "switch_to_tab DONE: selected_agent={:?}",
            self.conversation.selected_agent.as_ref().map(|a| format!("{}({})", a.name, &a.pubkey[..8]))
        );

        // Update sidebar state with delegations and reports from messages
        // (done here on tab switch rather than during render for purity)
        let messages = self.messages();
        self.update_sidebar_from_messages(&messages);

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
        let (removed_tab, new_active) = self.tabs.close_at(index);

        // Save draft from the removed tab's editor (before it's lost)
        if let Some(ref tab) = removed_tab {
            self.save_draft_from_tab(tab);
        }

        if new_active.is_none() {
            // No more tabs - go back to home view
            self.fallback_editor.clear();
            self.conversation.selected_thread = None;
            self.tabs.record_home_visit();
            self.view = View::Home;
        } else if was_active {
            // If the closed tab was active, switch to the new active tab
            self.switch_to_tab(self.tabs.active_index());
        }
    }

    /// Switch to next tab (Ctrl+Tab)
    /// Tab order: Home (0) -> Tab 1 -> Tab 2 -> ... -> Home
    pub fn next_tab(&mut self) {
        if self.tabs.is_empty() {
            return; // Only Home exists, nothing to switch to
        }

        if self.view == View::Home {
            // From Home, go to first conversation tab (index 0)
            self.switch_to_tab(0);
        } else {
            // From a conversation tab
            let current = self.tabs.active_index();
            if current + 1 >= self.tabs.len() {
                // At last conversation tab, wrap to Home
                self.go_home();
            } else {
                // Go to next conversation tab
                self.switch_to_tab(current + 1);
            }
        }
    }

    /// Switch to previous tab (Ctrl+Shift+Tab)
    /// Tab order: Home (0) <- Tab 1 <- Tab 2 <- ... <- Home
    pub fn prev_tab(&mut self) {
        if self.tabs.is_empty() {
            return; // Only Home exists, nothing to switch to
        }

        if self.view == View::Home {
            // From Home, go to last conversation tab
            self.switch_to_tab(self.tabs.len() - 1);
        } else {
            // From a conversation tab
            let current = self.tabs.active_index();
            if current == 0 {
                // At first conversation tab, go to Home
                self.go_home();
            } else {
                // Go to previous conversation tab
                self.switch_to_tab(current - 1);
            }
        }
    }

    /// Navigate to Home view and record it in view history
    /// Use this instead of directly setting `self.view = View::Home` to ensure
    /// the navigation is tracked for the "go back to previous view" feature
    pub fn go_home(&mut self) {
        self.save_chat_draft();
        self.tabs.record_home_visit();
        self.view = View::Home;
    }

    /// Mark a thread as having unread messages (if it's open in a tab but not active)
    pub fn mark_tab_unread(&mut self, thread_id: &str) {
        self.tabs.mark_unread(thread_id);
    }

    /// Mark a thread as waiting for user response (if it's open in a tab but not active)
    /// This is triggered when the last message p-tags the current user
    pub fn mark_tab_waiting_for_user(&mut self, thread_id: &str) {
        self.tabs.mark_waiting_for_user(thread_id);
    }

    /// Clear the waiting_for_user state for a thread
    pub fn clear_tab_waiting_for_user(&mut self, thread_id: &str) {
        self.tabs.clear_waiting_for_user(thread_id);
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

    /// Check if a thread is multi-selected
    pub fn is_thread_multi_selected(&self, thread_id: &str) -> bool {
        self.multi_selected_threads.contains(thread_id)
    }

    /// Toggle multi-selection for a thread
    pub fn toggle_thread_multi_select(&mut self, thread_id: &str) {
        if self.multi_selected_threads.contains(thread_id) {
            self.multi_selected_threads.remove(thread_id);
        } else {
            self.multi_selected_threads.insert(thread_id.to_string());
        }
    }

    /// Add a thread to multi-selection
    pub fn add_thread_to_multi_select(&mut self, thread_id: &str) {
        self.multi_selected_threads.insert(thread_id.to_string());
    }

    /// Clear all multi-selections
    pub fn clear_multi_selection(&mut self) {
        self.multi_selected_threads.clear();
    }

    /// Archive all multi-selected threads
    pub fn archive_multi_selected(&mut self) {
        let count = self.multi_selected_threads.len();
        if count == 0 {
            return;
        }
        let thread_ids: Vec<String> = self.multi_selected_threads.drain().collect();
        for thread_id in &thread_ids {
            self.preferences.borrow_mut().set_thread_archived(thread_id, true);
        }
        self.notify(Notification::info(&format!("Archived {} conversations", count)));
    }

    /// Get recent threads across all projects for Home view (filtered by visible_projects, time_filter, archived)
    /// Now properly filters by visible projects FIRST, then applies time filter without arbitrary limits.
    pub fn recent_threads(&self) -> Vec<(Thread, String)> {
        // Empty visible_projects = show nothing (inverted default)
        if self.visible_projects.is_empty() {
            return vec![];
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Calculate time cutoff if time filter is active
        let time_cutoff = self.home.time_filter.as_ref().map(|tf| now.saturating_sub(tf.seconds()));

        // Get threads from visible projects with time filter applied at the data layer
        // No artificial limit - time filter is the primary constraint
        let threads = self.data_store.borrow().get_recent_threads_for_projects(
            &self.visible_projects,
            time_cutoff,
            None,
        );

        let prefs = self.preferences.borrow();

        // Remaining filters: archive status and scheduled events (user preferences)
        threads.into_iter()
            .filter(|(thread, _)| {
                // Archive filter
                let archive_ok = self.show_archived || !prefs.is_thread_archived(&thread.id);
                // Scheduled filter - hide scheduled if hide_scheduled is true
                let scheduled_ok = !self.hide_scheduled || !thread.is_scheduled;
                archive_ok && scheduled_ok
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
                if let Some(ref tf) = self.home.time_filter {
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

    /// Get feed items (kind:1 text notes) for Home view Feed tab
    /// Aggregates all messages from visible projects, sorted by created_at descending (newest first)
    pub fn feed_items(&self) -> Vec<FeedItem> {
        // Empty visible_projects = show nothing
        if self.visible_projects.is_empty() {
            return vec![];
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Calculate time cutoff if time filter is active
        let time_cutoff = self.home.time_filter.as_ref().map(|tf| now.saturating_sub(tf.seconds()));

        let store = self.data_store.borrow();
        let prefs = self.preferences.borrow();
        let mut feed_items: Vec<FeedItem> = Vec::new();

        // Iterate over visible projects and collect messages from their threads
        for project_a_tag in &self.visible_projects {
            let threads = store.get_threads(project_a_tag);

            for thread in threads {
                // Apply archive filter
                if !self.show_archived && prefs.is_thread_archived(&thread.id) {
                    continue;
                }

                // Apply scheduled filter
                if self.hide_scheduled && thread.is_scheduled {
                    continue;
                }

                let messages = store.get_messages(&thread.id);
                for msg in messages {
                    // Apply time filter
                    if let Some(cutoff) = time_cutoff {
                        if msg.created_at < cutoff {
                            continue;
                        }
                    }

                    // Skip reasoning messages from feed
                    if msg.is_reasoning {
                        continue;
                    }

                    feed_items.push(FeedItem {
                        content: msg.content.clone(),
                        pubkey: msg.pubkey.clone(),
                        created_at: msg.created_at,
                        thread_id: thread.id.clone(),
                        thread_title: thread.title.clone(),
                        project_a_tag: project_a_tag.clone(),
                    });
                }
            }
        }

        // Sort by created_at descending (newest first)
        feed_items.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Limit to reasonable number for UI performance
        feed_items.truncate(200);

        feed_items
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
                    self.conversation.selected_agent = Some(pm.clone());
                } else {
                    self.conversation.selected_agent = None;
                }
                self.selected_branch = status.default_branch().map(String::from);
            } else {
                self.conversation.selected_agent = None;
                self.selected_branch = None;
            }

            // Open tab and switch to chat
            self.open_tab(thread, project_a_tag);
            self.conversation.selected_thread = Some(thread.clone());
            self.restore_chat_draft();
            self.view = View::Chat;
            self.input_mode = InputMode::Editing;
            self.scroll_offset = usize::MAX;

            // Update sidebar for new thread
            let messages = self.messages();
            self.update_sidebar_from_messages(&messages);

            // Auto-open pending ask modal if there's one for this thread
            self.maybe_open_pending_ask();
        } else {
            // Project not found - clear state to prevent leaks
            self.selected_project = None;
            self.conversation.selected_agent = None;
            self.selected_branch = None;
        }
    }

    /// Push current conversation onto the navigation stack and navigate to a delegation.
    /// This allows drilling into delegations within the same tab.
    pub fn push_delegation(&mut self, delegation_thread_id: &str) {
        tlog!("AGENT", "push_delegation: entering thread_id={}", delegation_thread_id);

        // Save current draft before navigating
        self.save_chat_draft();

        // Get current tab state to save
        let current_state = self.tabs.active_tab().map(|tab| NavigationStackEntry {
            thread_id: tab.thread_id.clone(),
            thread_title: tab.thread_title.clone(),
            project_a_tag: tab.project_a_tag.clone(),
            scroll_offset: self.scroll_offset,
            selected_message_index: self.conversation.selected_message_index,
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
            self.conversation.selected_thread = Some(thread);
            self.scroll_offset = usize::MAX; // Start at bottom
            self.conversation.selected_message_index = 0;
            self.input_mode = InputMode::Editing;

            // Update project context if needed
            let project = self.data_store.borrow().get_projects()
                .iter()
                .find(|p| p.a_tag() == project_a_tag)
                .cloned();
            if let Some(project) = project {
                self.selected_project = Some(project);
            }

            // Restore draft and sync agent with conversation
            self.restore_chat_draft();

            tlog!("AGENT", "push_delegation done: selected_agent={:?}",
                self.conversation.selected_agent.as_ref().map(|a| format!("{}({})", a.name, &a.pubkey[..8]))
            );

            // Update sidebar for new thread
            let messages = self.messages();
            self.update_sidebar_from_messages(&messages);

            // Auto-open pending ask modal if there's one for this thread
            self.maybe_open_pending_ask();
        }
    }

    /// Pop from the navigation stack and return to the parent conversation.
    /// Returns true if popped successfully, false if stack was empty.
    pub fn pop_navigation_stack(&mut self) -> bool {
        tlog!("AGENT", "pop_navigation_stack: returning to parent");

        // Save current draft before navigating
        self.save_chat_draft();

        let entry = self.tabs.active_tab_mut()
            .and_then(|tab| tab.navigation_stack.pop());

        if let Some(entry) = entry {
            tlog!("AGENT", "pop_navigation_stack: popped entry thread_id={}", entry.thread_id);

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
                self.conversation.selected_thread = Some(thread);
                self.scroll_offset = entry.scroll_offset;
                self.conversation.selected_message_index = entry.selected_message_index;

                // Update project context if needed
                let project = self.data_store.borrow().get_projects()
                    .iter()
                    .find(|p| p.a_tag() == entry.project_a_tag)
                    .cloned();
                if let Some(project) = project {
                    self.selected_project = Some(project);
                }

                // Restore draft and sync agent with conversation
                self.restore_chat_draft();

                tlog!("AGENT", "pop_navigation_stack done: selected_agent={:?}",
                    self.conversation.selected_agent.as_ref().map(|a| format!("{}({})", a.name, &a.pubkey[..8]))
                );

                // Update sidebar for restored thread
                let messages = self.messages();
                self.update_sidebar_from_messages(&messages);

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
                    self.conversation.selected_agent = Some(pm.clone());
                } else {
                    self.conversation.selected_agent = None;
                }
                self.selected_branch = status.default_branch().map(String::from);
            } else {
                self.conversation.selected_agent = None;
                self.selected_branch = None;
            }

            self.conversation.selected_thread = None;
            self.creating_thread = true;
            self.view = View::Chat;
            self.input_mode = InputMode::Editing;
            self.chat_editor_mut().clear();
        } else {
            // Project not found - clear state to prevent leaks
            self.selected_project = None;
            self.conversation.selected_agent = None;
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
        // Set selection to last message so the view stays at the bottom
        // (consistent with Escape behavior in editor_handlers.rs)
        let count = self.display_item_count();
        self.conversation.selected_message_index = count.saturating_sub(1);
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
        let thread_id = match self.conversation.selected_thread.as_ref() {
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
    /// Searches ALL messages for a reply to the ask event, not just the current thread
    pub fn get_user_response_to_ask(&self, message_id: &str) -> Option<String> {
        let store = self.data_store.borrow();
        let user_pubkey = store.user_pubkey.as_ref()?;

        // Search all messages across all threads for a reply to this ask event
        for messages in store.messages_by_thread.values() {
            for msg in messages {
                if msg.pubkey == *user_pubkey {
                    if let Some(ref reply_to) = msg.reply_to {
                        if reply_to == message_id {
                            return Some(msg.content.clone());
                        }
                    }
                }
            }
        }

        None
    }

    // ===== Local Streaming Methods =====

    /// Get streaming content for current conversation
    pub fn local_streaming_content(&self) -> Option<&LocalStreamBuffer> {
        self.conversation.local_streaming_content()
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
        self.conversation.handle_local_stream_chunk(
            agent_pubkey,
            conversation_id,
            text_delta,
            reasoning_delta,
            is_finish,
        );
    }

    /// Clear the local stream buffer for a conversation
    pub fn clear_local_stream_buffer(&mut self, conversation_id: &str) {
        self.conversation.clear_local_stream_buffer(conversation_id);
    }

    /// Get current conversation ID (thread ID)
    pub fn current_conversation_id(&self) -> Option<String> {
        self.conversation.current_conversation_id()
    }

    // ===== Filter Management Methods =====

    /// Load filter preferences from storage
    pub fn load_filter_preferences(&mut self) {
        let prefs = self.preferences.borrow();

        // If there's an active workspace, use its projects; otherwise use manual selection
        if let Some(workspace) = prefs.active_workspace() {
            self.visible_projects = workspace.project_ids.iter().cloned().collect();
        } else {
            self.visible_projects = prefs.selected_projects().iter().cloned().collect();
        }

        self.home.time_filter = prefs.time_filter();
        self.conversation.show_llm_metadata = prefs.show_llm_metadata();
        self.hide_scheduled = prefs.hide_scheduled();
    }

    /// Save selected projects to preferences
    pub fn save_selected_projects(&self) {
        let projects: Vec<String> = self.visible_projects.iter().cloned().collect();
        self.preferences.borrow_mut().set_selected_projects(projects);
    }

    /// Apply a workspace - sets visible_projects based on workspace's project list
    /// Pass None to clear the active workspace (returns to manual project selection mode)
    /// Closes tabs that don't belong to the new workspace's projects
    pub fn apply_workspace(&mut self, workspace_id: Option<&str>, project_ids: &[String]) {
        self.preferences.borrow_mut().set_active_workspace(workspace_id);

        if workspace_id.is_some() {
            // Apply workspace's project list
            self.visible_projects = project_ids.iter().cloned().collect();
            // Also save to selected_projects for persistence
            self.save_selected_projects();

            // Close tabs that don't belong to projects in this workspace
            let workspace_projects: HashSet<String> = project_ids.iter().cloned().collect();
            self.tabs.tabs_mut().retain(|tab| {
                workspace_projects.contains(&tab.project_a_tag)
            });

            // Reset tab index if it's now out of bounds
            let tab_count = self.tabs.tabs().len();
            if self.tabs.active_index() >= tab_count {
                self.tabs.set_active_index(tab_count.saturating_sub(1));
            }

            // Clear selected project/thread if they don't belong to this workspace
            if let Some(ref project) = self.selected_project {
                if !workspace_projects.contains(&project.a_tag()) {
                    self.selected_project = None;
                    self.conversation.clear_selection();
                }
            }
        }
        // If None, visible_projects stays as-is (manual selection mode), tabs preserved
    }

    /// Toggle a project's visibility (manual selection mode)
    /// Clears the active workspace since we're now in manual mode
    pub fn toggle_project_visibility(&mut self, a_tag: &str) {
        // Clear active workspace - we're now in manual mode
        if self.preferences.borrow().active_workspace_id().is_some() {
            self.preferences.borrow_mut().set_active_workspace(None);
        }

        if self.visible_projects.contains(a_tag) {
            self.visible_projects.remove(a_tag);
        } else {
            self.visible_projects.insert(a_tag.to_string());
        }
        self.save_selected_projects();
    }

    /// Add a project to visible projects (manual selection mode)
    /// Clears the active workspace since we're now in manual mode
    pub fn add_visible_project(&mut self, a_tag: String) {
        // Clear active workspace - we're now in manual mode
        if self.preferences.borrow().active_workspace_id().is_some() {
            self.preferences.borrow_mut().set_active_workspace(None);
        }

        self.visible_projects.insert(a_tag);
        self.save_selected_projects();
    }

    /// Cycle through time filter options and persist
    pub fn cycle_time_filter(&mut self) {
        self.home.time_filter = TimeFilter::cycle_next(self.home.time_filter);
        self.preferences.borrow_mut().set_time_filter(self.home.time_filter);
    }

    /// Toggle LLM metadata display and persist
    pub fn toggle_llm_metadata(&mut self) {
        self.conversation.toggle_llm_metadata();
        self.preferences.borrow_mut().set_show_llm_metadata(self.conversation.show_llm_metadata);
    }

    // ===== Agent Browser Methods =====

    /// Open the agent browser view
    pub fn open_agent_browser(&mut self) {
        self.home.reset_agent_browser();
        self.scroll_offset = 0;
        self.view = View::AgentBrowser;
        self.input_mode = InputMode::Normal;
    }

    /// Get filtered agent definitions for the browser
    pub fn filtered_agent_definitions(&self) -> Vec<tenex_core::models::AgentDefinition> {
        let filter = &self.home.agent_browser_filter;
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
    /// Uses nostrdb fulltext search for kind:1 message content
    /// Plus in-memory search for thread titles (from kind:513 metadata)
    /// Respects visible_projects filter
    pub fn search_results(&self) -> Vec<SearchResult> {
        if self.search_filter.trim().is_empty() {
            return vec![];
        }

        let filter = self.search_filter.to_lowercase();
        let store = self.data_store.borrow();
        let mut results = Vec::new();
        let mut seen_threads: std::collections::HashSet<String> = std::collections::HashSet::new();

        // 1. Search thread titles/content (in-memory, from kind:513 metadata)
        for project in store.get_projects() {
            let a_tag = project.a_tag();

            // Skip projects not in visible_projects
            if !self.visible_projects.is_empty() && !self.visible_projects.contains(&a_tag) {
                continue;
            }

            let project_name = project.name.clone();

            for thread in store.get_threads(&a_tag) {
                // Check if thread title or content matches
                let title_matches = thread.title.to_lowercase().contains(&filter);
                let content_matches = thread.content.to_lowercase().contains(&filter);
                let id_matches = thread.id.to_lowercase().contains(&filter);

                if title_matches || content_matches || id_matches {
                    seen_threads.insert(thread.id.clone());

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
                }
            }
        }

        // 2. Search message content using nostrdb fulltext (kind:1 only)
        let search_hits = store.text_search(&self.search_filter, 200);

        for (event_id, thread_id_opt, content, _kind) in search_hits {
            // Determine which thread this belongs to
            let thread_id = thread_id_opt.unwrap_or_else(|| event_id.clone());

            // Skip if we already have this thread from title/content match
            if seen_threads.contains(&thread_id) {
                continue;
            }

            // Find the thread in our data
            let thread_and_project = store.threads_by_project
                .iter()
                .find_map(|(a_tag, threads)| {
                    threads.iter()
                        .find(|t| t.id == thread_id)
                        .map(|t| (t.clone(), a_tag.clone()))
                });

            let Some((thread, project_a_tag)) = thread_and_project else {
                continue;
            };

            // Skip projects not in visible_projects
            if !self.visible_projects.is_empty() && !self.visible_projects.contains(&project_a_tag) {
                continue;
            }

            seen_threads.insert(thread_id.clone());

            let project_name = store.get_project_name(&project_a_tag);
            let excerpt = Self::extract_excerpt(&content, &filter);

            results.push(SearchResult {
                thread,
                project_a_tag,
                project_name,
                match_type: SearchMatchType::Message { message_id: event_id },
                excerpt: Some(excerpt),
            });
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

    // ===== Chat Search Methods (in-conversation search) - Per-tab isolated =====

    /// Enter chat search mode (per-tab isolated)
    pub fn enter_chat_search(&mut self) {
        if let Some(tab) = self.tabs.active_tab_mut() {
            tab.chat_search.enter();
        }
    }

    /// Exit chat search mode (per-tab isolated)
    pub fn exit_chat_search(&mut self) {
        if let Some(tab) = self.tabs.active_tab_mut() {
            tab.chat_search.exit();
        }
    }

    /// Get reference to current tab's chat search state
    pub fn chat_search(&self) -> Option<&ChatSearchState> {
        self.tabs.active_tab().map(|t| &t.chat_search)
    }

    /// Get mutable reference to current tab's chat search state
    pub fn chat_search_mut(&mut self) -> Option<&mut ChatSearchState> {
        self.tabs.active_tab_mut().map(|t| &mut t.chat_search)
    }

    /// Check if chat search is active for current tab
    pub fn is_chat_search_active(&self) -> bool {
        self.chat_search().map(|s| s.active).unwrap_or(false)
    }

    /// Get the chat search query for current tab
    pub fn chat_search_query(&self) -> String {
        self.chat_search().map(|s| s.query.clone()).unwrap_or_default()
    }

    /// Update chat search results based on current query (per-tab isolated)
    pub fn update_chat_search(&mut self) {
        // Get the query first
        let query = match self.tabs.active_tab() {
            Some(tab) => {
                if tab.chat_search.query.trim().is_empty() {
                    // Clear and return early
                    if let Some(t) = self.tabs.active_tab_mut() {
                        t.chat_search.match_locations.clear();
                        t.chat_search.current_match = 0;
                        t.chat_search.total_matches = 0;
                    }
                    return;
                }
                tab.chat_search.query.clone()
            }
            None => return,
        };

        let query_lower = query.to_lowercase();
        let messages = self.messages();

        // Build match locations
        let mut new_matches = Vec::new();
        for msg in &messages {
            let content_lower = msg.content.to_lowercase();
            let mut start = 0;

            while let Some(pos) = content_lower[start..].find(&query_lower) {
                let absolute_pos = start + pos;
                new_matches.push(ChatSearchMatch {
                    message_id: msg.id.clone(),
                    start_offset: absolute_pos,
                    length: query.len(),
                });
                start = absolute_pos + 1;
            }
        }

        // Apply to tab
        if let Some(tab) = self.tabs.active_tab_mut() {
            tab.chat_search.total_matches = new_matches.len();
            tab.chat_search.match_locations = new_matches;
            tab.chat_search.current_match = 0;
        }
    }

    /// Navigate to next search match (per-tab isolated)
    pub fn chat_search_next(&mut self) {
        let should_scroll = {
            let tab = match self.tabs.active_tab_mut() {
                Some(t) => t,
                None => return,
            };
            if tab.chat_search.total_matches > 0 {
                tab.chat_search.current_match =
                    (tab.chat_search.current_match + 1) % tab.chat_search.total_matches;
                true
            } else {
                false
            }
        };
        if should_scroll {
            self.scroll_to_current_search_match();
        }
    }

    /// Navigate to previous search match (per-tab isolated)
    pub fn chat_search_prev(&mut self) {
        let should_scroll = {
            let tab = match self.tabs.active_tab_mut() {
                Some(t) => t,
                None => return,
            };
            if tab.chat_search.total_matches > 0 {
                if tab.chat_search.current_match == 0 {
                    tab.chat_search.current_match = tab.chat_search.total_matches - 1;
                } else {
                    tab.chat_search.current_match -= 1;
                }
                true
            } else {
                false
            }
        };
        if should_scroll {
            self.scroll_to_current_search_match();
        }
    }

    /// Scroll to make the current search match visible (per-tab isolated)
    fn scroll_to_current_search_match(&mut self) {
        let match_msg_id = {
            let tab = match self.tabs.active_tab() {
                Some(t) => t,
                None => return,
            };
            tab.chat_search.match_locations
                .get(tab.chat_search.current_match)
                .map(|m| m.message_id.clone())
        };

        if let Some(msg_id) = match_msg_id {
            let messages = self.messages();
            // Find the message index
            if let Some((msg_idx, _)) = messages.iter().enumerate()
                .find(|(_, m)| m.id == msg_id)
            {
                self.conversation.selected_message_index = msg_idx;
                // Scroll will happen naturally in render
            }
        }
    }

    /// Check if a message ID has search matches (per-tab isolated)
    pub fn message_has_search_match(&self, message_id: &str) -> bool {
        self.chat_search()
            .map(|s| s.active && s.match_locations.iter().any(|m| m.message_id == message_id))
            .unwrap_or(false)
    }

    /// Get search matches for a specific message (per-tab isolated)
    pub fn get_message_search_matches(&self, message_id: &str) -> Vec<ChatSearchMatch> {
        match self.chat_search() {
            Some(s) if s.active => {
                s.match_locations.iter()
                    .filter(|m| m.message_id == message_id)
                    .cloned()
                    .collect()
            }
            _ => vec![],
        }
    }

    /// Check if a match is the currently focused one (per-tab isolated)
    pub fn is_current_search_match(&self, message_id: &str, start_offset: usize) -> bool {
        self.chat_search()
            .and_then(|s| s.match_locations.get(s.current_match))
            .map(|current| current.message_id == message_id && current.start_offset == start_offset)
            .unwrap_or(false)
    }

    // ===== Nudge Selector Methods =====

    /// Open the nudge selector modal
    pub fn open_nudge_selector(&mut self) {
        use crate::ui::modal::NudgeSelectorState;
        use crate::ui::selector::SelectorState;

        // Get current nudge selections from active tab (per-tab isolation)
        let current_nudges = self.tabs.active_tab()
            .map(|t| t.selected_nudge_ids.clone())
            .unwrap_or_default();

        self.modal_state = ModalState::NudgeSelector(NudgeSelectorState {
            selector: SelectorState::new(),
            selected_nudge_ids: current_nudges,
        });
    }

    /// Close the nudge selector modal, applying selections to current tab
    pub fn close_nudge_selector(&mut self, apply: bool) {
        if let ModalState::NudgeSelector(ref state) = self.modal_state {
            if apply {
                // Apply to current tab (per-tab isolation)
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.selected_nudge_ids = state.selected_nudge_ids.clone();
                }
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

    // ===== History Search Methods (Ctrl+R) =====

    /// Open the history search modal
    pub fn open_history_search(&mut self) {
        use crate::ui::modal::HistorySearchState;

        // Get current project a-tag if available
        let current_project_a_tag = self.selected_project.as_ref().map(|p| p.a_tag());

        self.modal_state = ModalState::HistorySearch(HistorySearchState::new(current_project_a_tag));
    }

    /// Update history search results based on current query
    pub fn update_history_search(&mut self) {
        use crate::ui::modal::HistorySearchEntry;

        // Get user pubkey
        let user_pubkey = match self.data_store.borrow().user_pubkey.clone() {
            Some(pk) => pk,
            None => return,
        };

        // Get query and filter settings from modal state
        let (query, _all_projects, project_a_tag) = match &self.modal_state {
            ModalState::HistorySearch(state) => {
                let filter_project = if state.all_projects {
                    None
                } else {
                    state.current_project_a_tag.as_deref()
                };
                (state.query.clone(), state.all_projects, filter_project.map(String::from))
            }
            _ => return,
        };

        // Search for messages
        let results = self.data_store.borrow().search_user_messages(
            &user_pubkey,
            &query,
            project_a_tag.as_deref(),
            50, // limit
        );

        // Convert to HistorySearchEntry
        let entries: Vec<HistorySearchEntry> = results
            .into_iter()
            .map(|(event_id, content, created_at, project_a_tag)| HistorySearchEntry {
                event_id,
                content,
                created_at,
                project_a_tag,
            })
            .collect();

        // Update modal state with results
        if let ModalState::HistorySearch(ref mut state) = self.modal_state {
            state.results = entries;
            // Clamp selected index
            if !state.results.is_empty() && state.selected_index >= state.results.len() {
                state.selected_index = state.results.len() - 1;
            }
        }
    }

    /// Get the thread ID to stop operations on, based on current selection
    /// Returns the delegation's thread_id if a DelegationPreview is selected,
    /// otherwise returns the current thread's ID
    pub fn get_stop_target_thread_id(&self) -> Option<String> {
        use crate::ui::views::chat::{group_messages, DisplayItem};

        // Get current thread
        let thread = self.conversation.selected_thread.as_ref()?;
        let thread_id = thread.id.as_str();

        // Get messages and group them (same logic as rendering)
        let messages = self.messages();

        // Get display messages based on current view
        let display_messages: Vec<&Message> = if let Some(ref root_id) = self.conversation.subthread_root {
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
        if let Some(item) = grouped.get(self.conversation.selected_message_index) {
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

    /// Check if a nudge is selected (per-tab isolated)
    pub fn is_nudge_selected(&self, nudge_id: &str) -> bool {
        match &self.modal_state {
            ModalState::NudgeSelector(state) => state.selected_nudge_ids.contains(&nudge_id.to_string()),
            _ => {
                // Use per-tab state
                self.tabs.active_tab()
                    .map(|t| t.selected_nudge_ids.contains(&nudge_id.to_string()))
                    .unwrap_or(false)
            }
        }
    }

    /// Remove a nudge from selected nudges (per-tab isolated)
    pub fn remove_selected_nudge(&mut self, nudge_id: &str) {
        if let Some(tab) = self.tabs.active_tab_mut() {
            tab.selected_nudge_ids.retain(|id| id != nudge_id);
        }
    }

    /// Get selected nudge IDs for current tab (per-tab isolated)
    pub fn selected_nudge_ids(&self) -> Vec<String> {
        self.tabs.active_tab()
            .map(|t| t.selected_nudge_ids.clone())
            .unwrap_or_default()
    }

    /// Check if a thread has an unsent draft
    pub fn has_draft_for_thread(&self, thread_id: &str) -> bool {
        self.draft_service.load_chat_draft(thread_id)
            .map(|d| !d.text.trim().is_empty())
            .unwrap_or(false)
    }

    /// Add a message to history for the current tab (called after successful send)
    pub fn add_to_message_history(&mut self, content: String) {
        if let Some(tab) = self.tabs.active_tab_mut() {
            tab.message_history.add(content);
        }
    }

    /// Navigate to previous message in history (↑ key) - per-tab isolated
    pub fn history_prev(&mut self) {
        let history = self.tabs.active_tab().map(|t| &t.message_history);
        if history.map(|h| h.messages.is_empty()).unwrap_or(true) {
            return;
        }

        // Get current state from tab
        let (messages_len, current_index) = {
            let tab = match self.tabs.active_tab() {
                Some(t) => t,
                None => return,
            };
            (tab.message_history.messages.len(), tab.message_history.index)
        };

        match current_index {
            None => {
                // Save current input as draft and go to last history entry
                let current_text = self.chat_editor().text.clone();
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.message_history.draft = Some(current_text);
                    tab.message_history.index = Some(messages_len - 1);
                }
                let last_msg = self.tabs.active_tab()
                    .and_then(|t| t.message_history.messages.last())
                    .cloned()
                    .unwrap_or_default();
                let editor = self.chat_editor_mut();
                editor.text = last_msg;
                editor.cursor = editor.text.len();
            }
            Some(idx) if idx > 0 => {
                // Go to older entry
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.message_history.index = Some(idx - 1);
                }
                let msg = self.tabs.active_tab()
                    .and_then(|t| t.message_history.messages.get(idx - 1))
                    .cloned()
                    .unwrap_or_default();
                let editor = self.chat_editor_mut();
                editor.text = msg;
                editor.cursor = editor.text.len();
            }
            _ => {}
        }
        self.chat_editor_mut().clear_selection();
    }

    /// Navigate to next message in history (↓ key) - per-tab isolated
    pub fn history_next(&mut self) {
        let (messages_len, current_index, draft) = {
            let tab = match self.tabs.active_tab() {
                Some(t) => t,
                None => return,
            };
            (
                tab.message_history.messages.len(),
                tab.message_history.index,
                tab.message_history.draft.clone(),
            )
        };

        if let Some(idx) = current_index {
            if idx + 1 < messages_len {
                // Go to newer entry
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.message_history.index = Some(idx + 1);
                }
                let msg = self.tabs.active_tab()
                    .and_then(|t| t.message_history.messages.get(idx + 1))
                    .cloned()
                    .unwrap_or_default();
                let editor = self.chat_editor_mut();
                editor.text = msg;
                editor.cursor = editor.text.len();
            } else {
                // Restore draft and exit history mode
                let editor = self.chat_editor_mut();
                editor.text = draft.unwrap_or_default();
                editor.cursor = editor.text.len();
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.message_history.index = None;
                    tab.message_history.draft = None;
                }
            }
            self.chat_editor_mut().clear_selection();
        }
    }

    /// Check if currently browsing history (per-tab)
    pub fn is_browsing_history(&self) -> bool {
        self.tabs.active_tab()
            .map(|t| t.message_history.is_browsing())
            .unwrap_or(false)
    }

    /// Exit history mode without changing input (per-tab)
    pub fn exit_history_mode(&mut self) {
        if let Some(tab) = self.tabs.active_tab_mut() {
            tab.message_history.exit();
        }
    }

    // ===== Archive Methods =====

    /// Toggle visibility of archived items (conversations and projects)
    pub fn toggle_show_archived(&mut self) {
        self.show_archived = !self.show_archived;
        if self.show_archived {
            self.notify(Notification::info("Showing archived items"));
        } else {
            self.notify(Notification::info("Hiding archived items"));
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

    /// Check if a project is archived
    pub fn is_project_archived(&self, project_a_tag: &str) -> bool {
        self.preferences.borrow().is_project_archived(project_a_tag)
    }

    /// Toggle archive status of a project
    pub fn toggle_project_archived(&mut self, project_a_tag: &str) -> bool {
        self.preferences.borrow_mut().toggle_project_archived(project_a_tag)
    }

    // ===== Scheduled Events Filter Methods =====

    /// Toggle visibility of scheduled events
    pub fn toggle_hide_scheduled(&mut self) {
        self.hide_scheduled = !self.hide_scheduled;
        // Persist to preferences
        self.preferences.borrow_mut().set_hide_scheduled(self.hide_scheduled);
        if self.hide_scheduled {
            self.notify(Notification::info("Hiding scheduled events"));
        } else {
            self.notify(Notification::info("Showing scheduled events"));
        }
    }

    // ===== Backend Trust Methods =====

    /// Approve a backend pubkey (persist to preferences and update data store)
    pub fn approve_backend(&mut self, pubkey: &str) {
        self.preferences.borrow_mut().approve_backend(pubkey);
        self.data_store.borrow_mut().add_approved_backend(pubkey);
    }

    /// Block a backend pubkey (persist to preferences and update data store)
    pub fn block_backend(&mut self, pubkey: &str) {
        self.preferences.borrow_mut().block_backend(pubkey);
        self.data_store.borrow_mut().add_blocked_backend(pubkey);
    }

    /// Show the backend approval modal for a pending approval
    pub fn show_backend_approval_modal(&mut self, backend_pubkey: String, project_a_tag: String) {
        use crate::ui::modal::BackendApprovalState;
        self.modal_state = ModalState::BackendApproval(BackendApprovalState::new(
            backend_pubkey,
            project_a_tag,
        ));
    }

    /// Initialize trusted backends from preferences (called on app init)
    pub fn init_trusted_backends(&mut self) {
        let prefs = self.preferences.borrow();
        let approved = prefs.approved_backend_pubkeys().clone();
        let blocked = prefs.blocked_backend_pubkeys().clone();
        drop(prefs);
        self.data_store.borrow_mut().set_trusted_backends(approved, blocked);
    }

    // ===== Sidebar Search Methods =====

    /// Toggle the sidebar search visibility (Ctrl+T + /)
    pub fn toggle_sidebar_search(&mut self) {
        self.sidebar_search.toggle();
        if self.sidebar_search.visible {
            // Reset results when opening
            self.update_sidebar_search_results();
        }
    }

    /// Update sidebar search results based on current query and active tab
    pub fn update_sidebar_search_results(&mut self) {
        use crate::ui::search::{search_conversations_hierarchical, search_reports};
        let store = self.data_store.borrow();

        // Clear all result sets
        self.sidebar_search.results.clear();
        self.sidebar_search.hierarchical_results.clear();
        self.sidebar_search.report_results.clear();

        // Search based on current tab
        match self.home_panel_focus {
            HomeTab::Reports => {
                self.sidebar_search.report_results = search_reports(
                    &self.sidebar_search.query,
                    &store,
                    &self.visible_projects,
                );
            }
            _ => {
                // Use hierarchical search for conversations
                self.sidebar_search.hierarchical_results = search_conversations_hierarchical(
                    &self.sidebar_search.query,
                    &store,
                    &self.visible_projects,
                );
            }
        }

        // Reset selection when results change
        self.sidebar_search.selected_index = 0;
    }

    /// Open the selected search result (conversation or report based on current tab)
    pub fn open_selected_search_result(&mut self) {
        use crate::ui::search::HierarchicalSearchItem;

        match self.home_panel_focus {
            HomeTab::Reports => {
                // Open report (using clamped accessor)
                if let Some(report) = self.sidebar_search.selected_report().cloned() {
                    // Close search
                    self.sidebar_search.visible = false;
                    self.sidebar_search.query.clear();
                    self.sidebar_search.results.clear();
                    self.sidebar_search.hierarchical_results.clear();
                    self.sidebar_search.report_results.clear();

                    // Open the report in a viewer modal
                    use crate::ui::modal::{ModalState, ReportViewerState};
                    self.modal_state = ModalState::ReportViewer(ReportViewerState::new(report));
                }
            }
            _ => {
                // Open conversation from hierarchical results
                if let Some(item) = self.sidebar_search.selected_hierarchical_result().cloned() {
                    // Extract thread and project_a_tag from the item
                    let (thread, project_a_tag) = match item {
                        HierarchicalSearchItem::ContextAncestor { thread, project_a_tag, .. } => {
                            (thread, project_a_tag)
                        }
                        HierarchicalSearchItem::MatchedConversation { thread, project_a_tag, .. } => {
                            (thread, project_a_tag)
                        }
                    };

                    // Close search
                    self.sidebar_search.visible = false;
                    self.sidebar_search.query.clear();
                    self.sidebar_search.results.clear();
                    self.sidebar_search.hierarchical_results.clear();
                    self.sidebar_search.report_results.clear();

                    // Open the thread
                    self.open_thread_from_home(&thread, &project_a_tag);
                }
            }
        }
    }

    // ===== Sidebar Methods =====

    /// Update the sidebar state with delegations and reports from the current conversation
    pub fn update_sidebar_from_messages(&mut self, messages: &[Message]) {
        use std::collections::HashSet;
        use crate::ui::views::chat::grouping::should_render_q_tags;

        let store = self.data_store.borrow();

        // Extract delegations from q-tags, but filter out tools that use q_tags for internal purposes
        // (e.g., report_write uses q_tags to link to the article it creates)
        let mut delegations = Vec::new();
        let mut seen_thread_ids: HashSet<String> = HashSet::new();

        for msg in messages {
            // Skip q_tags from denylisted tools (they use q_tags for internal purposes)
            if !should_render_q_tags(msg.tool_name.as_deref()) {
                continue;
            }
            for thread_id in &msg.q_tags {
                if seen_thread_ids.contains(thread_id) {
                    continue;
                }
                seen_thread_ids.insert(thread_id.clone());

                // Look up the thread to get its title
                let (title, target) = if let Some(thread) = store.get_thread_by_id(thread_id) {
                    // Try to find the target agent name
                    // Priority: 1) kind:0 profile, 2) agent slug from project status, 3) short pubkey
                    let target_name = thread.p_tags.first()
                        .map(|pk| {
                            // Primary: Use kind:0 profile name
                            let profile_name = store.get_profile_name(pk);

                            // If profile name is just short pubkey, try project status as fallback
                            if profile_name.ends_with("...") {
                                // Fallback: Try agent slug from project status
                                store.find_project_for_thread(thread_id)
                                    .and_then(|a_tag| store.get_project_status(&a_tag))
                                    .and_then(|status| {
                                        status.agents.iter()
                                            .find(|a| a.pubkey == *pk)
                                            .map(|a| a.name.clone())
                                    })
                                    .unwrap_or(profile_name)
                            } else {
                                profile_name
                            }
                        })
                        .unwrap_or_else(|| "Unknown".to_string());

                    (thread.title.clone(), target_name)
                } else {
                    // Thread not found, use short ID
                    (format!("Thread {}...", &thread_id[..8.min(thread_id.len())]), "Unknown".to_string())
                };

                delegations.push(SidebarDelegation {
                    thread_id: thread_id.clone(),
                    title,
                    target,
                });
            }
        }

        // Extract reports from a-tags (dedupe by a_tag)
        let mut reports = Vec::new();
        let mut seen_a_tags: HashSet<String> = HashSet::new();

        for msg in messages {
            for a_tag in &msg.a_tags {
                if seen_a_tags.contains(a_tag) {
                    continue;
                }
                seen_a_tags.insert(a_tag.clone());

                // Parse a_tag using shared helper
                if let Some(coord) = ReportCoordinate::parse(a_tag) {
                    // Look up the report to get its title
                    let title = store.get_report(&coord.slug)
                        .map(|r| r.title.clone())
                        .unwrap_or_else(|| coord.slug.clone());

                    reports.push(SidebarReport {
                        a_tag: a_tag.clone(),
                        title,
                        slug: coord.slug,
                    });
                }
            }
        }

        drop(store);
        self.sidebar_state.update(delegations, reports);
    }

    /// Toggle sidebar focus
    pub fn toggle_sidebar_focus(&mut self) {
        if self.sidebar_state.has_items() {
            self.sidebar_state.toggle_focus();
        }
    }

    /// Set sidebar focus
    pub fn set_sidebar_focused(&mut self, focused: bool) {
        self.sidebar_state.set_focused(focused);
    }

    /// Check if sidebar is focused
    pub fn is_sidebar_focused(&self) -> bool {
        self.sidebar_state.focused
    }

    /// Move sidebar selection up
    pub fn sidebar_move_up(&mut self) {
        self.sidebar_state.move_up();
    }

    /// Move sidebar selection down
    pub fn sidebar_move_down(&mut self) {
        self.sidebar_state.move_down();
    }

    /// Activate the currently selected sidebar item
    /// Returns the selection if one was made
    pub fn sidebar_activate(&mut self) -> Option<crate::ui::components::SidebarSelection> {
        if self.sidebar_state.focused {
            self.sidebar_state.selected_item()
        } else {
            None
        }
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
