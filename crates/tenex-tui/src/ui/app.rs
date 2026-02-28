use crate::models::{
    AskEvent, ChatDraft, DraftImageAttachment, DraftPasteAttachment, Message, NamedDraft,
    PreferencesStorage, Project, ProjectAgent, ProjectStatus, SendState, Thread, TimeFilter,
};
use crate::nostr::{DataChange, NostrCommand};
use crate::store::{get_trace_context, AppDataStore, Database};
use crate::ui::ask_input::AskInputState;
use crate::ui::audio_player::{AudioPlaybackState, AudioPlayer};
use crate::ui::components::{ReportCoordinate, SidebarDelegation, SidebarReport, SidebarState};
use crate::ui::modal::{AgentConfigState, CommandPaletteState, ModalState};
use crate::ui::notifications::Notification;
use crate::ui::selector::SelectorState;
use crate::ui::services::{AnimationClock, DraftService, NotificationManager};
use crate::ui::state::{
    ChatSearchMatch, ChatSearchState, ConversationState, HomeViewState, LocalStreamBuffer,
    NavigationStackEntry, OpenTab, TTSQueueItemStatus, TabManager, ViewLocation,
};
use crate::ui::text_editor::{ImageAttachment, PasteAttachment, TextEditor};
use nostr_sdk::Keys;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet, VecDeque};
use std::rc::Rc;
use std::sync::mpsc::Receiver;
use std::sync::Arc;
use tenex_core::models::{AgentDefinition, MCPTool};
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

/// Check if an inbox item's timestamp is within the 48-hour cap.
///
/// This is extracted as a standalone function for testability.
/// The `now` parameter should be the current Unix timestamp in seconds.
pub fn is_within_48h_cap(created_at: u64, now: u64) -> bool {
    let cutoff = now.saturating_sub(tenex_core::constants::INBOX_48H_CAP_SECONDS);
    created_at >= cutoff
}

fn resolve_selected_agent_from_status(
    current: Option<&ProjectAgent>,
    status: &ProjectStatus,
) -> Option<ProjectAgent> {
    if let Some(current_agent) = current {
        if let Some(updated) = status
            .agents
            .iter()
            .find(|agent| agent.pubkey == current_agent.pubkey)
        {
            return Some(updated.clone());
        }
        return Some(current_agent.clone());
    }

    status.pm_agent().cloned()
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
    ActiveWork,
    Stats,
}

/// Subtabs within the Stats view
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum StatsSubtab {
    #[default]
    Rankings,
    Runtime,
    Messages,
    Activity,
}

impl StatsSubtab {
    /// Get the next subtab (wraps around)
    pub fn next(self) -> Self {
        match self {
            StatsSubtab::Rankings => StatsSubtab::Runtime,
            StatsSubtab::Runtime => StatsSubtab::Messages,
            StatsSubtab::Messages => StatsSubtab::Activity,
            StatsSubtab::Activity => StatsSubtab::Rankings,
        }
    }

    /// Get the previous subtab (wraps around)
    pub fn prev(self) -> Self {
        match self {
            StatsSubtab::Rankings => StatsSubtab::Activity,
            StatsSubtab::Runtime => StatsSubtab::Rankings,
            StatsSubtab::Messages => StatsSubtab::Runtime,
            StatsSubtab::Activity => StatsSubtab::Messages,
        }
    }
}

// ChatSearchState, ChatSearchMatch, OpenTab, TabManager, HomeViewState, ChatViewState,
// LocalStreamBuffer, ConversationState are now in ui::state module

/// Focus state for the context line (agent/project bar below input)
/// None means the text input is focused, Some(X) means item X in context line is selected
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InputContextFocus {
    /// Agent name is selected
    Agent,
    /// Project selector is selected (only available for new conversations/draft tabs)
    Project,
    /// Unified nudge/skill selector is selected
    NudgeSkill,
}

impl InputContextFocus {
    pub fn move_left(self, is_draft_tab: bool) -> Self {
        match self {
            InputContextFocus::Agent => InputContextFocus::Agent,
            InputContextFocus::Project => InputContextFocus::Agent,
            InputContextFocus::NudgeSkill => {
                if is_draft_tab {
                    InputContextFocus::Project
                } else {
                    InputContextFocus::Agent
                }
            }
        }
    }

    pub fn move_right(self, is_draft_tab: bool) -> Self {
        match self {
            InputContextFocus::Agent => {
                if is_draft_tab {
                    InputContextFocus::Project
                } else {
                    InputContextFocus::NudgeSkill
                }
            }
            InputContextFocus::Project => InputContextFocus::NudgeSkill,
            InputContextFocus::NudgeSkill => InputContextFocus::NudgeSkill,
        }
    }
}

/// Actions that can be undone
#[derive(Debug, Clone)]
pub enum UndoAction {
    /// Thread was archived (store thread_id to unarchive)
    ThreadArchived {
        thread_id: String,
        thread_title: String,
    },
    /// Thread was unarchived (store thread_id to re-archive)
    ThreadUnarchived {
        thread_id: String,
        thread_title: String,
    },
    /// Project was archived
    ProjectArchived {
        project_a_tag: String,
        project_name: String,
    },
    /// Project was unarchived
    ProjectUnarchived {
        project_a_tag: String,
        project_name: String,
    },
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

    /// Whether bunker signer is currently running in this TUI session.
    pub bunker_running: bool,
    /// Active bunker URI when signer is running.
    pub bunker_uri: Option<String>,
    /// Pending bunker signing requests waiting for user decision.
    pub bunker_pending_requests: VecDeque<tenex_core::nostr::bunker::BunkerSignRequest>,
    /// Dedup set for queued bunker request IDs.
    pub bunker_pending_request_ids: HashSet<String>,
    /// Persisted bunker auto-approve rules from preferences.
    pub bunker_auto_approve_rules:
        Vec<tenex_core::models::project_draft::BunkerAutoApproveRulePref>,
    /// Session-scoped bunker audit entries from the running bunker process.
    pub bunker_audit_entries: Vec<tenex_core::nostr::bunker::BunkerAuditEntry>,

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
    /// Three-state filter for scheduled events in conversation list
    pub scheduled_filter: tenex_core::models::project_draft::ScheduledFilter,
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
    /// Audio player for notification sounds
    pub audio_player: AudioPlayer,
    /// Channel sender for voice/model browse results from background fetch tasks
    pub browse_tx: Option<tokio::sync::mpsc::Sender<crate::runtime::BrowseResult>>,
    /// Last time the user sent a message per thread (unix timestamp seconds)
    pub last_user_activity_by_thread: HashMap<String, u64>,
    /// Last time Esc was pressed (for double-Esc stop detection)
    pub last_esc_time: Option<std::time::Instant>,
    /// Last time an autosave was performed (for periodic crash protection)
    pub last_autosave: std::time::Instant,
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
        let prefs = PreferencesStorage::new(data_dir);
        let bunker_rules = prefs.bunker_auto_approve_rules().to_vec();

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
            preferences: RefCell::new(prefs),
            bunker_running: false,
            bunker_uri: None,
            bunker_pending_requests: VecDeque::new(),
            bunker_pending_request_ids: HashSet::new(),
            bunker_auto_approve_rules: bunker_rules,
            bunker_audit_entries: Vec::new(),
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
            scheduled_filter: tenex_core::models::project_draft::ScheduledFilter::ShowAll,
            user_explicitly_selected_agent: false,
            last_undo_action: None,
            input_context_focus: None,
            sidebar_search: crate::ui::search::SidebarSearchState::new(),
            publish_confirm_tx: None,
            stats_subtab: StatsSubtab::default(),
            audio_player: AudioPlayer::new(),
            browse_tx: None,
            last_user_activity_by_thread: HashMap::new(),
            last_esc_time: None,
            last_autosave: std::time::Instant::now(),
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
        self.tick_tts_playback();
    }

    /// Update TTS queue state based on audio player state.
    /// When playback completes, mark current item as Completed and clear playing_index.
    fn tick_tts_playback(&mut self) {
        // Check if audio player finished playing
        let audio_state = self.audio_player.state();

        if let Some(tts_state) = self.tabs.tts_state_mut() {
            if let Some(playing_idx) = tts_state.playing_index {
                // Audio finished playing - mark item as completed
                if audio_state == AudioPlaybackState::Stopped {
                    if let Some(item) = tts_state.queue.get_mut(playing_idx) {
                        if item.status == TTSQueueItemStatus::Playing {
                            item.status = TTSQueueItemStatus::Completed;
                            // Clear playing_index since playback is done
                            tts_state.playing_index = None;
                        }
                    }
                }
            }
        }
    }

    /// Get spinner character based on frame counter
    pub fn spinner_char(&self) -> char {
        self.animation_clock.spinner_char()
    }

    /// Get wave offset for character-by-character color animation
    pub fn wave_offset(&self) -> usize {
        self.animation_clock.wave_offset()
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

    /// Select an agent as the result of an explicit user choice (e.g. from the agent config modal).
    /// Sets both the selected agent and the `user_explicitly_selected_agent` flag so the choice
    /// is preserved through subsequent autosaves.
    pub fn select_agent_explicit(&mut self, agent: ProjectAgent) {
        self.conversation.selected_agent = Some(agent);
        self.user_explicitly_selected_agent = true;
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
        let now_collapsed = self
            .preferences
            .borrow_mut()
            .toggle_threads_default_collapsed();

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
        self.data_store
            .borrow()
            .get_project_status(&project.a_tag())
            .cloned()
    }

    /// Get project status for selected project
    pub fn get_selected_project_status(&self) -> Option<ProjectStatus> {
        self.selected_project
            .as_ref()
            .and_then(|p| self.get_project_status(p))
    }

    /// Get messages for the currently selected thread
    pub fn messages(&self) -> Vec<Message> {
        self.conversation
            .selected_thread
            .as_ref()
            .map(|t| self.data_store.borrow().get_messages(&t.id).to_vec())
            .unwrap_or_default()
    }

    /// Filter messages for current view (subthread or main thread).
    /// This helper consolidates the filtering logic used by display methods.
    fn filter_messages_for_view<'a>(
        messages: &'a [Message],
        thread_id: Option<&str>,
        subthread_root: Option<&str>,
    ) -> Vec<&'a Message> {
        if let Some(root_id) = subthread_root {
            messages
                .iter()
                .filter(|m| m.reply_to.as_deref() == Some(root_id))
                .collect()
        } else {
            messages
                .iter()
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
        let thread_id = self
            .conversation
            .selected_thread
            .as_ref()
            .map(|t| t.id.as_str());
        let subthread_root = self.conversation.subthread_root.as_deref();

        let display_messages = Self::filter_messages_for_view(&messages, thread_id, subthread_root);
        let grouped = group_messages(&display_messages);

        grouped
            .get(self.conversation.selected_message_index)
            .and_then(|item| match item {
                DisplayItem::SingleMessage { message, .. } => Some(message.id.clone()),
                DisplayItem::DelegationPreview { .. } => None,
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
        let thread_id = self
            .conversation
            .selected_thread
            .as_ref()
            .map(|t| t.id.as_str());
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
    fn convert_attachments_to_draft(
        editor: &crate::ui::text_editor::TextEditor,
    ) -> (Vec<DraftPasteAttachment>, Vec<DraftImageAttachment>) {
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

    /// Build a ChatDraft from editor content and metadata.
    /// This is the SINGLE SOURCE OF TRUTH for draft assembly, eliminating duplicate code.
    ///
    /// Arguments:
    /// - `conversation_id`: The draft key (thread_id or draft_id)
    /// - `editor`: The text editor with current content
    /// - `agent_pubkey`: The agent pubkey to save (or None if not explicitly selected)
    /// - `reference_conversation_id`: Reference conversation for context tags
    /// - `fork_message_id`: Fork message ID for fork tags
    /// - `project_a_tag_fallback`: Fallback project if no existing draft
    /// - `existing_draft`: Optional existing draft to preserve session_id, message_sequence, etc.
    fn build_chat_draft(
        conversation_id: String,
        editor: &crate::ui::text_editor::TextEditor,
        agent_pubkey: Option<String>,
        reference_conversation_id: Option<String>,
        fork_message_id: Option<String>,
        project_a_tag_fallback: Option<String>,
        existing_draft: Option<&ChatDraft>,
    ) -> ChatDraft {
        let (attachments, image_attachments) = Self::convert_attachments_to_draft(editor);

        // BULLETPROOF: Preserve session_id, message_sequence, project_a_tag from existing draft
        let (session_id, message_sequence, project_a_tag, send_state) =
            if let Some(d) = existing_draft {
                (
                    d.session_id.clone(),
                    d.message_sequence,
                    d.project_a_tag.clone(),
                    d.send_state,
                )
            } else {
                (None, 0, project_a_tag_fallback, SendState::Typing)
            };

        ChatDraft {
            conversation_id,
            session_id,
            project_a_tag,
            message_sequence,
            send_state,
            text: editor.text.clone(),
            attachments,
            image_attachments,
            selected_agent_pubkey: agent_pubkey,
            last_modified: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            reference_conversation_id,
            fork_message_id,
            // BULLETPROOF: New/updated drafts are unpublished - they stay until relay confirms
            published_at: None,
            published_event_id: None,
            confirmed_at: None,
        }
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
            let existing_draft = self.draft_service.load_chat_draft(&conversation_id);

            // Determine agent_pubkey: preserve from existing draft OR use explicit selection
            // CRITICAL FIX: If existing draft has an agent, preserve it even if user_explicitly_selected_agent is false
            let agent_pubkey = if self.user_explicitly_selected_agent {
                self.conversation
                    .selected_agent
                    .as_ref()
                    .map(|a| a.pubkey.clone())
            } else {
                // Preserve existing draft's agent if present (fixes data loss bug)
                existing_draft
                    .as_ref()
                    .and_then(|d| d.selected_agent_pubkey.clone())
            };

            tlog!(
                "AGENT",
                "save_chat_draft: key={}, explicit={}, agent={:?}",
                conversation_id,
                self.user_explicitly_selected_agent,
                agent_pubkey.as_ref().map(|p| &p[..8])
            );

            let editor = self.chat_editor();
            let (reference_conversation_id, fork_message_id) = self
                .tabs
                .active_tab()
                .map(|t| {
                    (
                        t.reference_conversation_id.clone(),
                        t.fork_message_id.clone(),
                    )
                })
                .unwrap_or((None, None));
            let project_a_tag_fallback = self.tabs.active_tab().map(|t| t.project_a_tag.clone());

            let draft = Self::build_chat_draft(
                conversation_id.clone(),
                editor,
                agent_pubkey,
                reference_conversation_id,
                fork_message_id,
                project_a_tag_fallback,
                existing_draft.as_ref(),
            );

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
            // IMPORTANT: Load existing draft to preserve its agent metadata.
            // We cannot use self.selected_agent because it belongs
            // to the ACTIVE tab, not necessarily the tab being closed.
            let existing_draft = self.draft_service.load_chat_draft(&conversation_id);
            let agent_pubkey = if let Some(ref draft) = existing_draft {
                draft.selected_agent_pubkey.clone()
            } else {
                None
            };

            tlog!(
                "AGENT",
                "save_draft_from_tab: key={}, agent={:?} (preserved from existing draft)",
                conversation_id,
                agent_pubkey
            );

            let draft = Self::build_chat_draft(
                conversation_id.clone(),
                &tab.editor,
                agent_pubkey,
                tab.reference_conversation_id.clone(),
                tab.fork_message_id.clone(),
                Some(tab.project_a_tag.clone()),
                existing_draft.as_ref(),
            );

            if let Err(e) = self.draft_service.save_chat_draft(draft) {
                // BULLETPROOF: Log I/O errors but don't interrupt - critical for tab close flow
                tlog!(
                    "DRAFT",
                    "ERROR saving draft from tab {}: {}",
                    conversation_id,
                    e
                );
            }
        }
    }

    /// Restore draft for the selected thread or draft tab into chat_editor
    /// Priority: draft values > conversation sync > defaults
    pub fn restore_chat_draft(&mut self) {
        let thread_id = self
            .conversation
            .selected_thread
            .as_ref()
            .map(|t| t.id.clone());
        tlog!(
            "AGENT",
            "restore_chat_draft called, thread_id={:?}",
            thread_id
        );

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
                        .map(|a| ImageAttachment {
                            id: a.id,
                            url: a.url.clone(),
                        })
                        .collect();

                    // Sync ID counters to prevent collisions with restored attachments
                    editor.sync_attachment_id_counters();
                }

                tlog!(
                    "AGENT",
                    "restore_chat_draft: loaded draft, agent_pubkey={:?}",
                    draft
                        .selected_agent_pubkey
                        .as_ref()
                        .map(|p| &p[..8.min(p.len())])
                );

                // Restore agent from draft if one was saved (takes priority over sync)
                if let Some(ref agent_pubkey) = draft.selected_agent_pubkey {
                    // Find agent by pubkey in available agents
                    let agent = self
                        .available_agents()
                        .into_iter()
                        .find(|a| &a.pubkey == agent_pubkey);
                    if let Some(agent) = agent {
                        tlog!(
                            "AGENT",
                            "restore_chat_draft: restoring agent from draft='{}' (pubkey={})",
                            agent.name,
                            &agent.pubkey[..8]
                        );
                        self.conversation.selected_agent = Some(agent);
                        draft_had_agent = true;
                        // CRITICAL FIX: Mark agent as explicitly selected so subsequent autosaves preserve it
                        // Without this, the next autosave would drop the agent_pubkey since the flag was reset above
                        self.user_explicitly_selected_agent = true;
                    } else {
                        tlog!("AGENT", "restore_chat_draft: draft agent_pubkey={} NOT FOUND in available_agents",
                            &agent_pubkey[..8.min(agent_pubkey.len())]);
                    }
                }
                // Restore reference_conversation_id and fork_message_id from draft into the active tab
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.reference_conversation_id = draft.reference_conversation_id.clone();
                    tab.fork_message_id = draft.fork_message_id.clone();
                }
            } else {
                tlog!("AGENT", "restore_chat_draft: no draft found for key");
            }
        }

        // For real threads, sync agent with conversation ONLY if draft didn't have a value
        // This ensures draft selections are preserved while still providing sensible defaults
        if self.conversation.selected_thread.is_some() {
            if !draft_had_agent {
                tlog!(
                    "AGENT",
                    "restore_chat_draft: draft had no agent, calling sync_agent_with_conversation"
                );
                self.sync_agent_with_conversation();
            } else {
                tlog!(
                    "AGENT",
                    "restore_chat_draft: draft had agent, skipping sync"
                );
            }
        }

        tlog!(
            "AGENT",
            "restore_chat_draft done, selected_agent={:?}",
            self.conversation.selected_agent.as_ref().map(|a| format!(
                "{}({})",
                a.name,
                &a.pubkey[..8]
            ))
        );
    }

    /// Sync selected_agent with the most recent agent in the conversation
    /// Falls back to PM agent if no agent has responded yet
    pub fn sync_agent_with_conversation(&mut self) {
        tlog!("AGENT", "sync_agent_with_conversation called");

        // First try to get the most recent agent from the conversation
        if let Some(recent_agent) = self.get_most_recent_agent_from_conversation() {
            tlog!(
                "AGENT",
                "sync_agent: setting to recent_agent='{}' (pubkey={})",
                recent_agent.name,
                &recent_agent.pubkey[..8]
            );
            self.conversation.selected_agent = Some(recent_agent);
            return;
        }

        // Fall back to PM agent if no agent has responded yet
        if let Some(status) = self.get_selected_project_status() {
            if let Some(pm) = status.pm_agent() {
                tlog!(
                    "AGENT",
                    "sync_agent: no recent agent, falling back to PM='{}' (pubkey={})",
                    pm.name,
                    &pm.pubkey[..8]
                );
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
    pub fn create_publish_snapshot(
        &self,
        conversation_id: &str,
        content: String,
    ) -> Result<String, tenex_core::models::draft::DraftStorageError> {
        self.draft_service
            .create_publish_snapshot(conversation_id, content)
    }

    /// Mark a publish snapshot as confirmed (call after relay confirmation)
    /// Uses the unique publish_id to mark the specific snapshot - doesn't affect current draft.
    /// BULLETPROOF: New typing after send won't be lost because snapshots are separate.
    pub fn mark_publish_confirmed(
        &self,
        publish_id: &str,
        event_id: Option<String>,
    ) -> Result<bool, tenex_core::models::draft::DraftStorageError> {
        self.draft_service
            .mark_publish_confirmed(publish_id, event_id)
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
    pub fn cleanup_confirmed_publishes(
        &self,
    ) -> Result<usize, tenex_core::models::draft::DraftStorageError> {
        self.draft_service.cleanup_confirmed_publishes()
    }

    /// Remove a publish snapshot (for rollback when send fails)
    /// Call this when send to relay fails AFTER snapshot was created
    pub fn remove_publish_snapshot(
        &self,
        publish_id: &str,
    ) -> Result<bool, tenex_core::models::draft::DraftStorageError> {
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
        let project_a_tag = self
            .selected_project
            .as_ref()
            .map(|p| p.a_tag())
            .unwrap_or_default();

        self.draft_service
            .get_named_drafts_for_project(&project_a_tag)
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
            self.set_warning_status(
                "No drafts for this project. Use Ctrl+T 's' in edit mode to save one.",
            );
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
        self.chat_editor_mut()
            .update_focused_attachment(new_content);
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
        // Check both ProjectsModal and ComposerProjectSelector for the active filter
        let filter = match &self.modal_state {
            ModalState::ProjectsModal { selector, .. } => &selector.filter,
            ModalState::ComposerProjectSelector { selector } => &selector.filter,
            _ => "",
        };
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
            .filter_map(|p| {
                fuzzy_score(&p.title, filter).map(|score| {
                    let is_online = store.is_project_online(&p.a_tag());
                    (p, score, is_online)
                })
            })
            .collect();

        // Sort by online status first (online projects first), then by score (lower = better match), then alphabetically for ties
        matching.sort_by(|(a, score_a, a_online), (b, score_b, b_online)| {
            // Sort online projects before offline projects (true > false, so we reverse)
            b_online
                .cmp(a_online)
                .then_with(|| score_a.cmp(score_b))
                .then_with(|| a.title.cmp(&b.title))
        });

        // Separate into online and offline, preserving sort order
        let (online, offline): (Vec<_>, Vec<_>) = matching
            .into_iter()
            .partition(|(_, _, is_online)| *is_online);

        (
            online.into_iter().map(|(p, _, _)| p).cloned().collect(),
            offline.into_iter().map(|(p, _, _)| p).cloned().collect(),
        )
    }

    /// Open the projects modal for creating a new thread
    /// Selecting a project will navigate to chat view and create a draft tab
    pub fn open_projects_selector_for_new_thread(&mut self) {
        // Guard: Check if there are any projects to select from
        let (online, offline) = self.filtered_projects();
        if online.is_empty() && offline.is_empty() {
            // Check if projects exist but are filtered out vs truly no projects
            let has_projects = !self.data_store.borrow().get_projects().is_empty();
            let message = if has_projects {
                "No projects match current filters. Check workspace/archived settings."
            } else {
                "No projects available. Create a project first."
            };
            self.set_warning_status(message);
            return;
        }

        self.modal_state = ModalState::ProjectsModal {
            selector: SelectorState::new(),
            for_new_thread: true,
        };
    }

    /// Open the projects modal for switching the active project filter
    /// Selecting a project will update the visible projects list (home view filter)
    pub fn open_projects_selector_for_switch(&mut self) {
        // Guard: Check if there are any projects to select from
        let (online, offline) = self.filtered_projects();
        if online.is_empty() && offline.is_empty() {
            // Check if projects exist but are filtered out vs truly no projects
            let has_projects = !self.data_store.borrow().get_projects().is_empty();
            let message = if has_projects {
                "No projects match current filters. Check workspace/archived settings."
            } else {
                "No projects available. Create a project first."
            };
            self.set_warning_status(message);
            return;
        }

        self.modal_state = ModalState::ProjectsModal {
            selector: SelectorState::new(),
            for_new_thread: false,
        };
    }

    /// Open the composer project selector for changing the project on a draft tab
    /// This is specifically for new conversations (draft tabs) and allows changing
    /// which project the new conversation will be tagged to.
    pub fn open_composer_project_selector(&mut self) {
        // Guard: Check if there are any projects to select from
        let (online, offline) = self.filtered_projects();
        if online.is_empty() && offline.is_empty() {
            let has_projects = !self.data_store.borrow().get_projects().is_empty();
            let message = if has_projects {
                "No projects match current filters. Check workspace/archived settings."
            } else {
                "No projects available. Create a project first."
            };
            self.set_warning_status(message);
            return;
        }

        self.modal_state = ModalState::ComposerProjectSelector {
            selector: SelectorState::new(),
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

    /// Get composer project selector index (from ModalState)
    pub fn composer_project_selector_index(&self) -> usize {
        match &self.modal_state {
            ModalState::ComposerProjectSelector { selector } => selector.index,
            _ => 0,
        }
    }

    /// Get composer project selector filter (from ModalState)
    pub fn composer_project_selector_filter(&self) -> &str {
        match &self.modal_state {
            ModalState::ComposerProjectSelector { selector } => &selector.filter,
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
        let changes: Vec<DataChange> = self
            .data_rx
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
                    self.refresh_selected_agent_from_project_status();
                }
                DataChange::MCPToolsChanged => {
                    // MCP tools are already updated in the store by the worker
                }
                DataChange::BunkerSignRequest { request } => {
                    self.enqueue_bunker_sign_request(request);
                }
                DataChange::BookmarkListChanged { bookmarked_ids: _ } => {
                    // Bookmarks are already updated in the store by the worker
                }
            }
        }
        Ok(())
    }

    /// Refresh selected_agent from the latest selected-project status.
    /// Ensures model/tool changes are reflected in the composer immediately.
    pub fn refresh_selected_agent_from_project_status(&mut self) {
        let status = self.selected_project.as_ref().and_then(|project| {
            self.data_store
                .borrow()
                .get_project_status(&project.a_tag())
                .cloned()
        });

        let Some(status) = status else {
            return;
        };

        let current = self.selected_agent().cloned();
        if let Some(resolved) = resolve_selected_agent_from_status(current.as_ref(), &status) {
            self.set_selected_agent(Some(resolved));
        }
    }

    /// Get the thread_id from the current notification (if it has one)
    pub fn current_notification_thread_id(&self) -> Option<String> {
        self.notification_manager
            .current()
            .and_then(|n| n.thread_id.clone())
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
        self.scroll_offset = self
            .scroll_offset
            .saturating_add(amount)
            .min(self.max_scroll_offset);
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
        self.selected_project
            .as_ref()
            .and_then(|p| {
                self.data_store
                    .borrow()
                    .get_project_status(&p.a_tag())
                    .map(|s| s.agents.clone())
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

        tlog!(
            "AGENT",
            "get_most_recent_agent: thread_id={}, messages={}, available_agents={:?}",
            thread.id,
            messages.len(),
            available_agents
                .iter()
                .map(|a| format!("{}({})", a.name, &a.pubkey[..8]))
                .collect::<Vec<_>>()
        );

        // Create a set of agent pubkeys for quick lookup
        let agent_pubkeys: std::collections::HashSet<&str> =
            available_agents.iter().map(|a| a.pubkey.as_str()).collect();

        // Find the most recent message from an agent (not the user)
        // Messages are typically sorted by created_at, but we'll iterate and track the latest
        let mut latest_agent_pubkey: Option<&str> = None;
        let mut latest_timestamp: u64 = 0;

        for msg in &messages {
            // Skip messages from the user
            if user_pubkey
                .as_ref()
                .map(|pk| pk == &msg.pubkey)
                .unwrap_or(false)
            {
                continue;
            }

            // Check if this message is from a known agent
            if agent_pubkeys.contains(msg.pubkey.as_str()) && msg.created_at >= latest_timestamp {
                latest_timestamp = msg.created_at;
                latest_agent_pubkey = Some(msg.pubkey.as_str());
                tlog!(
                    "AGENT",
                    "  found agent message: pubkey={}, timestamp={}",
                    &msg.pubkey[..8],
                    msg.created_at
                );
            }
        }

        // Also check the thread itself (the original message that started the thread)
        // The thread author might be an agent - use last_activity as timestamp proxy
        // Note: for the thread root, we only consider it if no messages from agents exist yet
        if latest_agent_pubkey.is_none()
            && agent_pubkeys.contains(thread.pubkey.as_str())
            && user_pubkey
                .as_ref()
                .map(|pk| pk != &thread.pubkey)
                .unwrap_or(true)
        {
            latest_agent_pubkey = Some(thread.pubkey.as_str());
            tlog!(
                "AGENT",
                "  using thread author as agent: pubkey={}",
                &thread.pubkey[..8]
            );
        }

        // Find and return the matching agent
        let result = latest_agent_pubkey
            .and_then(|pubkey| available_agents.into_iter().find(|a| a.pubkey == pubkey));

        tlog!(
            "AGENT",
            "get_most_recent_agent result: {:?}",
            result
                .as_ref()
                .map(|a| format!("{}({})", a.name, &a.pubkey[..8]))
        );

        result
    }

    fn filtered_agents_with_filter(&self, filter: &str) -> Vec<crate::models::ProjectAgent> {
        let mut agents_with_scores: Vec<_> = self
            .available_agents()
            .into_iter()
            .filter_map(|a| fuzzy_score(&a.name, filter).map(|score| (a, score)))
            .collect();
        // Sort by PM first, then score (lower = better match), then alphabetically for ties.
        agents_with_scores.sort_by(|(a, score_a), (b, score_b)| {
            b.is_pm
                .cmp(&a.is_pm)
                .then_with(|| score_a.cmp(score_b))
                .then_with(|| a.name.cmp(&b.name))
        });
        agents_with_scores.into_iter().map(|(a, _)| a).collect()
    }

    /// Get agents filtered by the active agent-config modal filter.
    pub fn filtered_agents(&self) -> Vec<crate::models::ProjectAgent> {
        let filter = match &self.modal_state {
            ModalState::AgentConfig(state) => state.selector.filter.as_str(),
            _ => "",
        };
        self.filtered_agents_with_filter(filter)
    }

    fn build_agent_settings_for(
        &self,
        agent: &crate::models::ProjectAgent,
    ) -> Option<crate::ui::modal::AgentSettingsState> {
        let project = self.selected_project.as_ref()?;
        // Use status.all_tools() to show ALL tools (including unassigned ones)
        let (all_tools, all_models) = self
            .data_store
            .borrow()
            .get_project_status(&project.a_tag())
            .map(|status| {
                let tools = status.all_tools().iter().map(|s| s.to_string()).collect();
                let models = status.all_models.clone();
                (tools, models)
            })
            .unwrap_or_default();

        Some(crate::ui::modal::AgentSettingsState::new(
            agent.name.clone(),
            agent.pubkey.clone(),
            agent.model.clone(),
            agent.tools.clone(),
            false,
            all_models,
            all_tools,
        ))
    }

    /// Refresh right-pane settings for the currently highlighted agent in the unified modal.
    pub fn refresh_agent_config_modal_state(&self, state: &mut AgentConfigState) {
        let filtered = self.filtered_agents_with_filter(&state.selector.filter);
        state.selector.clamp_index(filtered.len());

        let Some(agent) = filtered.get(state.selector.index) else {
            state.load_agent_settings(None, None, None, HashSet::new(), false);
            return;
        };

        if state.active_agent_pubkey.as_deref() == Some(agent.pubkey.as_str()) {
            return;
        }

        let settings = self.build_agent_settings_for(agent);
        state.load_agent_settings(
            Some(agent.pubkey.clone()),
            settings,
            agent.model.clone(),
            agent.tools.iter().cloned().collect(),
            false,
        );
    }

    /// Open the unified agent selection + configuration modal.
    pub fn open_agent_config_modal(&mut self) {
        let mut state = AgentConfigState::new();
        if let Some(selected_agent) = self.selected_agent() {
            let filtered = self.filtered_agents_with_filter("");
            if let Some(index) = filtered
                .iter()
                .position(|a| a.pubkey == selected_agent.pubkey)
            {
                state.selector.index = index;
            }
        }
        self.refresh_agent_config_modal_state(&mut state);
        self.modal_state = ModalState::AgentConfig(state);
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

    /// Open the command palette modal (Ctrl+T)
    pub fn open_command_palette(&mut self) {
        self.modal_state = ModalState::CommandPalette(CommandPaletteState::new());
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
        self.tabs
            .open_draft(project_a_tag.to_string(), project_name.to_string())
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

            // CRITICAL: Clear fork/reference metadata after thread creation to prevent stale state
            // These metadata fields are only relevant for the initial thread creation message
            // and should not persist after the thread is created
            draft.reference_conversation_id = None;
            draft.fork_message_id = None;

            // Delete the old draft keyed by project:new
            if let Err(e) = self.draft_service.delete_chat_draft(draft_id) {
                tlog!(
                    "DRAFT",
                    "WARNING: failed to delete old draft key '{}': {}",
                    draft_id,
                    e
                );
            }

            // Save with the new thread ID as key (only if there's content to preserve)
            if !draft.is_empty() {
                if let Err(e) = self.draft_service.save_chat_draft(draft) {
                    tlog!(
                        "DRAFT",
                        "ERROR migrating draft to new key '{}': {}",
                        thread.id,
                        e
                    );
                }
            }
        }

        // Convert the tab in the tab manager
        self.tabs
            .convert_draft(draft_id, thread.id.clone(), thread.title.clone());
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

        // Check if we're closing a TTS tab - if so, stop audio playback
        if self
            .tabs
            .active_tab()
            .map(|t| t.is_tts_control())
            .unwrap_or(false)
        {
            self.audio_player.stop();
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

        tlog!(
            "AGENT",
            "switch_to_tab: index={}, is_draft={}, thread_id={}, project={}",
            index,
            is_draft,
            thread_id,
            project_a_tag
        );

        if is_draft {
            // Draft tab - set up for new conversation
            self.conversation.selected_thread = None;
            self.creating_thread = true;

            // CRITICAL: Clear all context upfront to prevent stale state leaking
            // if project lookup fails below
            self.selected_project = None;
            self.conversation.selected_agent = None;
            tlog!("AGENT", "switch_to_tab(draft): cleared agent");

            // Set the project for this draft
            let project = self
                .data_store
                .borrow()
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
                        tlog!(
                            "AGENT",
                            "switch_to_tab(draft): setting PM='{}' (pubkey={})",
                            pm.name,
                            &pm.pubkey[..8]
                        );
                        self.conversation.selected_agent = Some(pm.clone());
                    }
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
            let thread = self
                .data_store
                .borrow()
                .get_threads(&project_a_tag)
                .iter()
                .find(|t| t.id == thread_id)
                .cloned();

            if let Some(thread) = thread {
                tlog!(
                    "AGENT",
                    "switch_to_tab(real): found thread '{}'",
                    thread.title
                );
                self.conversation.selected_thread = Some(thread);
                self.creating_thread = false;

                // CRITICAL: Set project context from the tab's project_a_tag
                // This ensures cross-tab contamination doesn't occur
                let project = self
                    .data_store
                    .borrow()
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == project_a_tag)
                    .cloned();

                if let Some(project) = project {
                    let a_tag = project.a_tag();
                    self.selected_project = Some(project);

                    // Load draft once upfront to check what values it contains
                    let draft = self.draft_service.load_chat_draft(&thread_id);
                    let draft_has_agent = draft
                        .as_ref()
                        .map(|d| d.selected_agent_pubkey.is_some())
                        .unwrap_or(false);

                    tlog!(
                        "AGENT",
                        "switch_to_tab(real): draft_has_agent={}",
                        draft_has_agent
                    );

                    // Set agent defaults only if draft doesn't have one
                    if let Some(status) = self.data_store.borrow().get_project_status(&a_tag) {
                        if !draft_has_agent {
                            // No draft agent, use PM as default
                            if let Some(pm) = status.pm_agent() {
                                tlog!("AGENT", "switch_to_tab(real): no draft agent, setting PM='{}' (pubkey={}) BEFORE restore_chat_draft",
                                    pm.name, &pm.pubkey[..8]);
                                self.conversation.selected_agent = Some(pm.clone());
                            } else {
                                // No PM available, clear to prevent stale state
                                tlog!(
                                    "AGENT",
                                    "switch_to_tab(real): no draft agent and no PM, clearing"
                                );
                                self.conversation.selected_agent = None;
                            }
                        } else {
                            tlog!(
                                "AGENT",
                                "switch_to_tab(real): draft has agent, skipping PM default"
                            );
                        }
                    } else {
                        // No project status, clear agent to prevent stale state
                        tlog!(
                            "AGENT",
                            "switch_to_tab(real): no project status, clearing agent"
                        );
                        self.conversation.selected_agent = None;
                    }

                    // Now restore the draft (uses cached load if same key)
                    self.restore_chat_draft();
                } else {
                    // Project lookup failed - clear all context to prevent stale state leaking
                    tlog!(
                        "AGENT",
                        "switch_to_tab(real): project lookup failed, clearing all"
                    );
                    self.selected_project = None;
                    self.conversation.selected_agent = None;
                }
                self.scroll_offset = usize::MAX; // Scroll to bottom
                self.conversation.selected_message_index = 0;
                self.conversation.subthread_root = None;
                self.conversation.subthread_root_message = None;
                self.input_mode = InputMode::Editing; // Auto-focus input
                self.view = View::Chat; // Switch to Chat view
            }
        }

        tlog!(
            "AGENT",
            "switch_to_tab DONE: selected_agent={:?}",
            self.conversation.selected_agent.as_ref().map(|a| format!(
                "{}({})",
                a.name,
                &a.pubkey[..8]
            ))
        );

        // Update sidebar state with delegations and reports from messages
        // (done here on tab switch rather than during render for purity)
        let messages = self.messages();
        self.update_sidebar_from_messages(&messages);

        // Auto-open pending ask modal if there's one for this thread
        self.maybe_open_pending_ask();
    }

    /// Close tab modal
    pub fn close_tab_modal(&mut self) {
        self.tabs.close_modal();
    }

    /// Close tab at specific index (for tab modal)
    pub fn close_tab_at(&mut self, index: usize) {
        let was_active = index == self.tabs.active_index();

        // Check if we're closing a TTS tab - if so, stop audio playback
        // (similar to close_current_tab behavior)
        if self
            .tabs
            .tabs()
            .get(index)
            .map(|t| t.is_tts_control())
            .unwrap_or(false)
        {
            self.audio_player.stop();
        }

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

    /// Update the agent working state for a thread tab
    /// This is used to show a blue indicator when agents are actively working
    pub fn set_tab_agent_working(&mut self, thread_id: &str, is_working: bool) {
        self.tabs.set_agent_working(thread_id, is_working);
    }

    /// Record that the user was active in a thread (for TTS inactivity gating)
    pub fn record_user_activity(&mut self, thread_id: &str) {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        self.last_user_activity_by_thread
            .insert(thread_id.to_string(), now);
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
            self.preferences
                .borrow_mut()
                .set_thread_archived(thread_id, true);
        }
        self.notify(Notification::info(format!(
            "Archived {} conversations",
            count
        )));
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
        let time_cutoff = self
            .home
            .time_filter
            .as_ref()
            .map(|tf| now.saturating_sub(tf.seconds()));

        // Get threads from visible projects with time filter applied at the data layer
        // No artificial limit - time filter is the primary constraint
        let threads = self.data_store.borrow().get_recent_threads_for_projects(
            &self.visible_projects,
            time_cutoff,
            None,
        );

        let prefs = self.preferences.borrow();

        // Remaining filters: archive status and scheduled events (user preferences)
        threads
            .into_iter()
            .filter(|(thread, _)| {
                // Archive filter
                let archive_ok = self.show_archived || !prefs.is_thread_archived(&thread.id);
                // Scheduled filter - apply three-state filter
                let scheduled_ok = self.scheduled_filter.allows(thread.is_scheduled);
                archive_ok && scheduled_ok
            })
            .collect()
    }

    /// Get inbox items for Home view (filtered by time_filter, archived)
    /// NOTE: Inbox items are NOT filtered by visible_projects - if someone asks you a question,
    /// you should see it regardless of project filtering.
    /// NOTE: A hard cap of 48 hours is always applied to keep the inbox focused on recent items.
    pub fn inbox_items(&self) -> Vec<crate::models::InboxItem> {
        let items = self.data_store.borrow().inbox.get_items().to_vec();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let prefs = self.preferences.borrow();

        items
            .into_iter()
            // NOTE: NO project filter for inbox - you should see all asks/mentions regardless of project selection
            // Hard 48-hour cap - always applied (uses shared constant via helper function)
            .filter(|item| is_within_48h_cap(item.created_at, now))
            // Archive filter - hide items from archived threads unless show_archived is true
            .filter(|item| {
                if let Some(ref thread_id) = item.thread_id {
                    self.show_archived || !prefs.is_thread_archived(thread_id)
                } else {
                    true // Keep items without thread_id
                }
            })
            // User-selectable time filter (can further restrict, but not beyond 48h cap)
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

        store
            .reports
            .get_reports()
            .into_iter()
            .filter(|r| self.visible_projects.contains(&r.project_a_tag))
            .filter(|r| {
                if filter.is_empty() {
                    return true;
                }
                r.title.to_lowercase().contains(&filter)
                    || r.summary.to_lowercase().contains(&filter)
                    || r.content.to_lowercase().contains(&filter)
                    || r.hashtags
                        .iter()
                        .any(|h| h.to_lowercase().contains(&filter))
            })
            .cloned()
            .collect()
    }

    /// Open thread from Home view (recent conversations or inbox)
    pub fn open_thread_from_home(&mut self, thread: &Thread, project_a_tag: &str) {
        // Find and set selected project
        let project = self
            .data_store
            .borrow()
            .get_projects()
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
            } else {
                self.conversation.selected_agent = None;
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
        }
    }

    /// Push current conversation onto the navigation stack and navigate to a delegation.
    /// This allows drilling into delegations within the same tab.
    pub fn push_delegation(&mut self, delegation_thread_id: &str) {
        tlog!(
            "AGENT",
            "push_delegation: entering thread_id={}",
            delegation_thread_id
        );

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
            let project = self
                .data_store
                .borrow()
                .get_projects()
                .iter()
                .find(|p| p.a_tag() == project_a_tag)
                .cloned();
            if let Some(project) = project {
                self.selected_project = Some(project);
            }

            // Restore draft and sync agent with conversation
            self.restore_chat_draft();

            tlog!(
                "AGENT",
                "push_delegation done: selected_agent={:?}",
                self.conversation.selected_agent.as_ref().map(|a| format!(
                    "{}({})",
                    a.name,
                    &a.pubkey[..8]
                ))
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

        let entry = self
            .tabs
            .active_tab_mut()
            .and_then(|tab| tab.navigation_stack.pop());

        if let Some(entry) = entry {
            tlog!(
                "AGENT",
                "pop_navigation_stack: popped entry thread_id={}",
                entry.thread_id
            );

            // Find the parent thread
            let thread = self
                .data_store
                .borrow()
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
                let project = self
                    .data_store
                    .borrow()
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == entry.project_a_tag)
                    .cloned();
                if let Some(project) = project {
                    self.selected_project = Some(project);
                }

                // Restore draft and sync agent with conversation
                self.restore_chat_draft();

                tlog!(
                    "AGENT",
                    "pop_navigation_stack done: selected_agent={:?}",
                    self.conversation.selected_agent.as_ref().map(|a| format!(
                        "{}({})",
                        a.name,
                        &a.pubkey[..8]
                    ))
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
        self.tabs
            .active_tab()
            .map(|tab| !tab.navigation_stack.is_empty())
            .unwrap_or(false)
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
    pub fn open_ask_modal(
        &mut self,
        message_id: String,
        ask_event: AskEvent,
        ask_author_pubkey: String,
    ) {
        use crate::ui::modal::AskModalState;
        let input_state = AskInputState::new(ask_event.questions.clone());
        self.modal_state = ModalState::AskModal(AskModalState {
            message_id,
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
        let ask_info = self
            .data_store
            .borrow()
            .get_unanswered_ask_for_thread(&thread_id);
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
        self.scheduled_filter = prefs.scheduled_filter();
    }

    /// Save selected projects to preferences
    pub fn save_selected_projects(&self) {
        let projects: Vec<String> = self.visible_projects.iter().cloned().collect();
        self.preferences
            .borrow_mut()
            .set_selected_projects(projects);
    }

    /// Apply a workspace - sets visible_projects based on workspace's project list
    /// Pass None to clear the active workspace and show ALL projects
    /// Closes tabs that don't belong to the new workspace's projects (when switching to a workspace)
    pub fn apply_workspace(&mut self, workspace_id: Option<&str>, project_ids: &[String]) {
        self.preferences
            .borrow_mut()
            .set_active_workspace(workspace_id);

        if workspace_id.is_some() {
            // Apply workspace's project list
            self.visible_projects = project_ids.iter().cloned().collect();
            // Also save to selected_projects for persistence
            self.save_selected_projects();

            // Close tabs that don't belong to projects in this workspace
            let workspace_projects: HashSet<String> = project_ids.iter().cloned().collect();
            self.tabs
                .tabs_mut()
                .retain(|tab| workspace_projects.contains(&tab.project_a_tag));

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
        } else {
            // No workspace - show ALL projects
            let all_projects: HashSet<String> = self
                .data_store
                .borrow()
                .get_projects()
                .iter()
                .map(|p| p.a_tag())
                .collect();
            self.visible_projects = all_projects;
            self.save_selected_projects();
            // Tabs are preserved when clearing workspace
        }
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

    /// Cycle through time filter options and persist
    pub fn cycle_time_filter(&mut self) {
        self.home.time_filter = TimeFilter::cycle_next(self.home.time_filter);
        self.preferences
            .borrow_mut()
            .set_time_filter(self.home.time_filter);
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
    pub fn filtered_agent_definitions(&self) -> Vec<AgentDefinition> {
        let filter = &self.home.agent_browser_filter;
        self.data_store
            .borrow()
            .content
            .get_agent_definitions()
            .into_iter()
            .filter(|d| {
                fuzzy_matches(&d.name, filter)
                    || fuzzy_matches(&d.description, filter)
                    || fuzzy_matches(&d.role, filter)
            })
            .cloned()
            .collect()
    }

    /// Get all agent definitions
    pub fn all_agent_definitions(&self) -> Vec<AgentDefinition> {
        self.data_store
            .borrow()
            .content
            .get_agent_definitions()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get agent definitions filtered by a custom filter string
    pub fn agent_definitions_filtered_by(&self, filter: &str) -> Vec<AgentDefinition> {
        self.data_store
            .borrow()
            .content
            .get_agent_definitions()
            .into_iter()
            .filter(|d| {
                filter.is_empty()
                    || fuzzy_matches(&d.name, filter)
                    || fuzzy_matches(&d.description, filter)
                    || fuzzy_matches(&d.role, filter)
            })
            .cloned()
            .collect()
    }

    /// Get MCP tools filtered by a custom filter string
    pub fn mcp_tools_filtered_by(&self, filter: &str) -> Vec<MCPTool> {
        self.data_store
            .borrow()
            .content
            .get_mcp_tools()
            .into_iter()
            .filter(|tool| {
                filter.is_empty()
                    || fuzzy_matches(&tool.name, filter)
                    || fuzzy_matches(&tool.description, filter)
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

            let project_name = project.title.clone();

            for thread in store.get_threads(&a_tag) {
                // Check if thread title or content matches
                let title_matches = thread.title.to_lowercase().contains(&filter);
                let content_matches = thread.content.to_lowercase().contains(&filter);
                let id_matches = thread.id.to_lowercase().contains(&filter);

                if title_matches || content_matches || id_matches {
                    seen_threads.insert(thread.id.clone());

                    let (match_type, excerpt) = if id_matches {
                        (
                            SearchMatchType::ConversationId,
                            Some(format!("ID: {}", thread.id)),
                        )
                    } else if title_matches {
                        (SearchMatchType::Thread, None)
                    } else {
                        (
                            SearchMatchType::Thread,
                            Some(Self::extract_excerpt(&thread.content, &filter)),
                        )
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
            let thread_and_project =
                store
                    .threads_by_project
                    .iter()
                    .find_map(|(a_tag, threads)| {
                        threads
                            .iter()
                            .find(|t| t.id == thread_id)
                            .map(|t| (t.clone(), a_tag.clone()))
                    });

            let Some((thread, project_a_tag)) = thread_and_project else {
                continue;
            };

            // Skip projects not in visible_projects
            if !self.visible_projects.is_empty() && !self.visible_projects.contains(&project_a_tag)
            {
                continue;
            }

            seen_threads.insert(thread_id.clone());

            let project_name = store.get_project_name(&project_a_tag);
            let excerpt = Self::extract_excerpt(&content, &filter);

            results.push(SearchResult {
                thread,
                project_a_tag,
                project_name,
                match_type: SearchMatchType::Message,
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
            let safe_start = (start..pos)
                .rev()
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
            content
                .chars()
                .take(60)
                .collect::<String>()
                .replace('\n', " ")
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

    /// Check if chat search is active for current tab
    pub fn is_chat_search_active(&self) -> bool {
        self.chat_search().map(|s| s.active).unwrap_or(false)
    }

    /// Get the chat search query for current tab
    pub fn chat_search_query(&self) -> String {
        self.chat_search()
            .map(|s| s.query.clone())
            .unwrap_or_default()
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
            tab.chat_search
                .match_locations
                .get(tab.chat_search.current_match)
                .map(|m| m.message_id.clone())
        };

        if let Some(msg_id) = match_msg_id {
            let messages = self.messages();
            // Find the message index
            if let Some((msg_idx, _)) = messages.iter().enumerate().find(|(_, m)| m.id == msg_id) {
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

    // ===== Unified Nudge/Skill Selector Methods =====

    /// Open the unified nudge/skill selector modal.
    /// Defaults to BookmarkedOnly filter so users see their curated list first.
    pub fn open_nudge_skill_selector(&mut self) {
        use crate::ui::modal::{BookmarkFilter, NudgeSkillSelectorState};
        use crate::ui::selector::SelectorState;

        let current_nudges = self
            .tabs
            .active_tab()
            .map(|t| t.selected_nudge_ids.clone())
            .unwrap_or_default();
        let current_skills = self
            .tabs
            .active_tab()
            .map(|t| t.selected_skill_ids.clone())
            .unwrap_or_default();

        self.modal_state = ModalState::NudgeSkillSelector(NudgeSkillSelectorState {
            selector: SelectorState::new(),
            selected_nudge_ids: current_nudges,
            selected_skill_ids: current_skills,
            bookmark_filter: BookmarkFilter::BookmarkedOnly,
        });
    }

    /// Get filtered items for the unified selector.
    ///
    /// Applies both the text search filter and the bookmark filter:
    /// - In `BookmarkedOnly` mode, only items whose IDs are in the user's bookmark list are shown.
    /// - In `All` mode, all items are shown (text search still applies).
    pub fn filtered_nudge_skill_items(&self) -> Vec<NudgeSkillSelectorItem> {
        use crate::ui::modal::BookmarkFilter;

        let filter = self.nudge_skill_selector_filter();
        let bookmark_filter = self.nudge_skill_bookmark_filter();
        let store = self.data_store.borrow();

        // Get the user's bookmarked IDs for filtering (if in BookmarkedOnly mode)
        let bookmarked_ids: Option<std::collections::HashSet<&str>> =
            if bookmark_filter == BookmarkFilter::BookmarkedOnly {
                let user_pubkey = store.user_pubkey.as_deref().unwrap_or("");
                let ids = store
                    .get_bookmarks(user_pubkey)
                    .map(|bl| bl.bookmarked_ids.iter().map(|s| s.as_str()).collect())
                    .unwrap_or_default();
                Some(ids)
            } else {
                None
            };

        let mut items: Vec<NudgeSkillSelectorItem> = store
            .content
            .get_nudges()
            .into_iter()
            .filter(|n| {
                // Apply bookmark filter
                if let Some(ref ids) = bookmarked_ids {
                    if !ids.contains(n.id.as_str()) {
                        return false;
                    }
                }
                // Apply text search filter
                fuzzy_matches(&n.title, filter) || fuzzy_matches(&n.description, filter)
            })
            .cloned()
            .map(NudgeSkillSelectorItem::Nudge)
            .collect();

        items.extend(
            store
                .content
                .get_skills()
                .into_iter()
                .filter(|s| {
                    // Apply bookmark filter
                    if let Some(ref ids) = bookmarked_ids {
                        if !ids.contains(s.id.as_str()) {
                            return false;
                        }
                    }
                    // Apply text search filter
                    fuzzy_matches(&s.title, filter) || fuzzy_matches(&s.description, filter)
                })
                .cloned()
                .map(NudgeSkillSelectorItem::Skill),
        );

        items.sort_by(|a, b| {
            a.title()
                .to_lowercase()
                .cmp(&b.title().to_lowercase())
                .then_with(|| a.kind_order().cmp(&b.kind_order()))
                .then_with(|| a.id().cmp(b.id()))
        });
        items
    }

    /// Get unified selector filter text.
    pub fn nudge_skill_selector_filter(&self) -> &str {
        match &self.modal_state {
            ModalState::NudgeSkillSelector(state) => &state.selector.filter,
            _ => "",
        }
    }

    /// Get the current bookmark filter mode for the nudge/skill selector.
    pub fn nudge_skill_bookmark_filter(&self) -> crate::ui::modal::BookmarkFilter {
        match &self.modal_state {
            ModalState::NudgeSkillSelector(state) => state.bookmark_filter.clone(),
            _ => crate::ui::modal::BookmarkFilter::All,
        }
    }

    /// Check if a nudge or skill ID is bookmarked by the current user.
    pub fn is_bookmarked(&self, item_id: &str) -> bool {
        let store = self.data_store.borrow();
        let user_pubkey = store.user_pubkey.as_deref().unwrap_or("");
        store.is_bookmarked(user_pubkey, item_id)
    }

    // ===== History Search Methods (Ctrl+R) =====

    /// Open the history search modal
    pub fn open_history_search(&mut self) {
        use crate::ui::modal::HistorySearchState;

        // Get current project a-tag if available
        let current_project_a_tag = self.selected_project.as_ref().map(|p| p.a_tag());

        self.modal_state =
            ModalState::HistorySearch(HistorySearchState::new(current_project_a_tag));
    }

    /// Update history search results based on current query
    /// Searches both sent messages (from Nostr) AND unsent drafts
    pub fn update_history_search(&mut self) {
        use crate::ui::modal::{HistorySearchEntry, HistorySearchEntryKind};
        use crate::ui::search::parse_search_terms;
        use tenex_core::search::text_contains_term;

        // Helper: Check if draft matches project filter using exact matching
        fn draft_matches_project(draft_project_a_tag: Option<&str>, filter_project: &str) -> bool {
            draft_project_a_tag
                .map(|p| p == filter_project)
                .unwrap_or(false)
        }

        // Helper: Check if text matches all search terms
        fn text_matches_terms(text: &str, terms: &[String]) -> bool {
            if terms.is_empty() {
                return true; // Empty query matches everything
            }
            terms.iter().all(|term| text_contains_term(text, term))
        }

        // Get query and filter settings from modal state
        let (query, filter_project) = match &self.modal_state {
            ModalState::HistorySearch(state) => {
                let filter = if state.all_projects {
                    None
                } else {
                    state.current_project_a_tag.clone()
                };
                (state.query.clone(), filter)
            }
            _ => {
                // Early return: modal not active, nothing to update
                return;
            }
        };

        // Get user pubkey - clear results on early return
        let user_pubkey = match self.data_store.borrow().user_pubkey.clone() {
            Some(pk) => pk,
            None => {
                // Clear stale results when no user pubkey
                if let ModalState::HistorySearch(ref mut state) = self.modal_state {
                    state.results.clear();
                    state.selected_index = 0;
                }
                return;
            }
        };

        // Parse search terms for unified search semantics
        let terms = parse_search_terms(&query);

        // Collect sent messages from Nostr
        let sent_results = self.data_store.borrow().search_user_messages(
            &user_pubkey,
            &query,
            filter_project.as_deref(),
            50, // limit
        );

        let mut entries: Vec<HistorySearchEntry> = sent_results
            .into_iter()
            .map(
                |(_event_id, content, created_at, _project_a_tag)| HistorySearchEntry {
                    kind: HistorySearchEntryKind::Message,
                    content,
                    created_at,
                },
            )
            .collect();

        // Collect draft entries with proper deduplication (keep most recent by last_modified)
        let all_drafts = self.draft_service.get_all_searchable_drafts();

        // Helper: Try to add a chat-style draft, keeping the most recent version if duplicate
        // Returns true if added/updated, false if filtered out
        fn try_add_chat_draft(
            drafts_by_id: &mut std::collections::HashMap<String, (String, Option<String>, u64)>,
            conversation_id: &str,
            text: &str,
            project_a_tag: Option<&str>,
            last_modified: u64,
            filter_project: Option<&str>,
            terms: &[String],
            draft_matches_project_fn: fn(Option<&str>, &str) -> bool,
            text_matches_terms_fn: fn(&str, &[String]) -> bool,
        ) -> bool {
            // Skip empty drafts
            if text.trim().is_empty() {
                return false;
            }
            // Apply project filter
            if let Some(filter) = filter_project {
                if !draft_matches_project_fn(project_a_tag, filter) {
                    return false;
                }
            }
            // Apply search term filter
            if !text_matches_terms_fn(text, terms) {
                return false;
            }
            // Keep most recent by last_modified (not first-seen)
            let should_insert = drafts_by_id
                .get(conversation_id)
                .map(|(_, _, existing_ts)| last_modified > *existing_ts)
                .unwrap_or(true);
            if should_insert {
                drafts_by_id.insert(
                    conversation_id.to_string(),
                    (
                        text.to_string(),
                        project_a_tag.map(|s| s.to_string()),
                        last_modified,
                    ),
                );
            }
            true
        }

        // Merge all chat/versioned/archived drafts, keeping most recent per conversation_id
        let mut drafts_by_id: std::collections::HashMap<String, (String, Option<String>, u64)> =
            std::collections::HashMap::new();

        // Process chat drafts
        for draft in &all_drafts.chat_drafts {
            try_add_chat_draft(
                &mut drafts_by_id,
                &draft.conversation_id,
                &draft.text,
                draft.project_a_tag.as_deref(),
                draft.last_modified,
                filter_project.as_deref(),
                &terms,
                draft_matches_project,
                text_matches_terms,
            );
        }

        // Process versioned drafts
        for draft in &all_drafts.versioned_drafts {
            try_add_chat_draft(
                &mut drafts_by_id,
                &draft.conversation_id,
                &draft.text,
                draft.project_a_tag.as_deref(),
                draft.last_modified,
                filter_project.as_deref(),
                &terms,
                draft_matches_project,
                text_matches_terms,
            );
        }

        // Process archived drafts
        for draft in &all_drafts.archived_drafts {
            try_add_chat_draft(
                &mut drafts_by_id,
                &draft.conversation_id,
                &draft.text,
                draft.project_a_tag.as_deref(),
                draft.last_modified,
                filter_project.as_deref(),
                &terms,
                draft_matches_project,
                text_matches_terms,
            );
        }

        // Convert merged drafts to entries
        for (_conversation_id, (text, _project_a_tag, last_modified)) in drafts_by_id {
            entries.push(HistorySearchEntry {
                kind: HistorySearchEntryKind::Draft,
                content: text,
                created_at: last_modified,
            });
        }

        // Process named drafts (use explicit NamedDraft variant - no conversation_id overloading)
        // Named drafts have their own ID namespace, so track separately
        let mut seen_named_draft_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for draft in &all_drafts.named_drafts {
            if draft.text.trim().is_empty() {
                continue;
            }
            if seen_named_draft_ids.contains(&draft.id) {
                continue;
            }
            if let Some(ref filter) = filter_project {
                if draft.project_a_tag != *filter {
                    continue;
                }
            }
            if !text_matches_terms(&draft.text, &terms) && !text_matches_terms(&draft.name, &terms)
            {
                continue;
            }

            seen_named_draft_ids.insert(draft.id.clone());
            entries.push(HistorySearchEntry {
                kind: HistorySearchEntryKind::NamedDraft,
                content: draft.text.clone(),
                created_at: draft.last_modified,
            });
        }

        // Sort by created_at/last_modified (most recent first)
        entries.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Limit total results
        entries.truncate(50);

        // Update modal state with results
        if let ModalState::HistorySearch(ref mut state) = self.modal_state {
            state.results = entries;
            // Clamp selected index
            if state.results.is_empty() {
                state.selected_index = 0;
            } else if state.selected_index >= state.results.len() {
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
        let display_messages: Vec<&Message> =
            if let Some(ref root_id) = self.conversation.subthread_root {
                messages
                    .iter()
                    .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                    .collect()
            } else {
                messages
                    .iter()
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
            if let DisplayItem::DelegationPreview {
                thread_id: delegation_thread_id,
                ..
            } = item
            {
                return Some(delegation_thread_id.clone());
            }
        }

        // Default: return current thread ID
        Some(thread.id.clone())
    }

    /// Send a stop command (kind:24134) for the current conversation's active agents.
    /// Used by double-Esc to quickly stop agent activity.
    pub fn stop_current_conversation(&mut self) {
        if let Some(stop_thread_id) = self.get_stop_target_thread_id() {
            let (is_busy, project_a_tag) = {
                let store = self.data_store.borrow();
                (
                    store.operations.is_event_busy(&stop_thread_id),
                    store.find_project_for_thread(&stop_thread_id),
                )
            };
            if is_busy {
                if let (Some(core_handle), Some(a_tag)) =
                    (self.core_handle.clone(), project_a_tag)
                {
                    let working_agents = self
                        .data_store
                        .borrow()
                        .operations
                        .get_working_agents(&stop_thread_id);
                    if let Err(e) = core_handle.send(NostrCommand::StopOperations {
                        project_a_tag: a_tag,
                        event_ids: vec![stop_thread_id.clone()],
                        agent_pubkeys: working_agents,
                    }) {
                        self.set_warning_status(&format!("Failed to stop: {}", e));
                    } else {
                        self.set_warning_status("Stop command sent");
                    }
                }
            }
        }
    }

    /// Periodically autosave the current draft to protect against crashes.
    /// Only saves if 5+ seconds have elapsed, we're in Chat+Editing mode,
    /// and the editor has content.
    pub fn maybe_autosave_draft(&mut self) {
        if self.last_autosave.elapsed() < std::time::Duration::from_secs(5) {
            return;
        }
        if self.view != View::Chat || self.input_mode != InputMode::Editing {
            return;
        }
        if self.chat_editor().text.is_empty() {
            return;
        }
        self.save_chat_draft();
        self.last_autosave = std::time::Instant::now();
    }

    /// Remove a nudge from selected nudges (per-tab isolated)
    pub fn remove_selected_nudge(&mut self, nudge_id: &str) {
        if let Some(tab) = self.tabs.active_tab_mut() {
            tab.selected_nudge_ids.retain(|id| id != nudge_id);
        }
    }

    /// Get selected nudge IDs for current tab (per-tab isolated)
    pub fn selected_nudge_ids(&self) -> Vec<String> {
        self.tabs
            .active_tab()
            .map(|t| t.selected_nudge_ids.clone())
            .unwrap_or_default()
    }

    // ===== Unified Selector Methods (Alt+K/Ctrl+N/Ctrl+/) =====

    /// Get selected skill IDs for current tab (per-tab isolated)
    pub fn selected_skill_ids(&self) -> Vec<String> {
        self.tabs
            .active_tab()
            .map(|t| t.selected_skill_ids.clone())
            .unwrap_or_default()
    }

    /// Check if a thread has an unsent draft
    pub fn has_draft_for_thread(&self, thread_id: &str) -> bool {
        self.draft_service
            .load_chat_draft(thread_id)
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
            (
                tab.message_history.messages.len(),
                tab.message_history.index,
            )
        };

        match current_index {
            None => {
                // Save current input as draft and go to last history entry
                let current_text = self.chat_editor().text.clone();
                if let Some(tab) = self.tabs.active_tab_mut() {
                    tab.message_history.draft = Some(current_text);
                    tab.message_history.index = Some(messages_len - 1);
                }
                let last_msg = self
                    .tabs
                    .active_tab()
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
                let msg = self
                    .tabs
                    .active_tab()
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
                let msg = self
                    .tabs
                    .active_tab()
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
        self.tabs
            .active_tab()
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
        self.preferences
            .borrow_mut()
            .toggle_thread_archived(thread_id)
    }

    /// Check if a project is archived
    pub fn is_project_archived(&self, project_a_tag: &str) -> bool {
        self.preferences.borrow().is_project_archived(project_a_tag)
    }

    /// Toggle archive status of a project
    pub fn toggle_project_archived(&mut self, project_a_tag: &str) -> bool {
        self.preferences
            .borrow_mut()
            .toggle_project_archived(project_a_tag)
    }

    // ===== Scheduled Events Filter Methods =====

    /// Cycle through scheduled event filter states: Show All → Hide → Show Only → Show All
    pub fn cycle_scheduled_filter(&mut self) {
        let new_filter = self.preferences.borrow_mut().cycle_scheduled_filter();
        self.scheduled_filter = new_filter;
        self.notify(Notification::info(&format!(
            "Scheduled events: {}",
            new_filter.label()
        )));
    }

    // ===== Bunker (NIP-46) Methods =====

    /// Reload persisted bunker auto-approve rules into local UI state.
    pub fn load_bunker_rules_from_preferences(&mut self) {
        self.bunker_auto_approve_rules = self
            .preferences
            .borrow()
            .bunker_auto_approve_rules()
            .to_vec();
    }

    /// Return persisted bunker enabled preference.
    pub fn bunker_enabled(&self) -> bool {
        self.preferences.borrow().bunker_enabled()
    }

    /// Initialize bunker lifecycle after successful login/connect.
    pub fn initialize_bunker_after_login(&mut self) {
        self.load_bunker_rules_from_preferences();
        if self.bunker_enabled() {
            if let Err(e) = self.start_bunker_with_rules() {
                self.set_warning_status(&format!("Failed to auto-start bunker: {}", e));
            }
        } else {
            self.bunker_running = false;
            self.bunker_uri = None;
        }
    }

    /// Start bunker signer and replay persisted auto-approve rules.
    pub fn start_bunker_with_rules(&mut self) -> Result<String, String> {
        let core_handle = self
            .core_handle
            .clone()
            .ok_or_else(|| "Core handle not available".to_string())?;

        let (response_tx, response_rx) = std::sync::mpsc::channel::<Result<String, String>>();
        core_handle
            .send(NostrCommand::StartBunker { response_tx })
            .map_err(|e| format!("Failed to send StartBunker command: {}", e))?;

        let uri = response_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("Timed out waiting for bunker start: {}", e))??;

        self.load_bunker_rules_from_preferences();
        for rule in &self.bunker_auto_approve_rules {
            core_handle
                .send(NostrCommand::AddBunkerAutoApproveRule {
                    requester_pubkey: rule.requester_pubkey.clone(),
                    event_kind: rule.event_kind,
                })
                .map_err(|e| format!("Failed to sync bunker rule: {}", e))?;
        }

        self.bunker_running = true;
        self.bunker_uri = Some(uri.clone());
        Ok(uri)
    }

    /// Stop bunker signer.
    pub fn stop_bunker(&mut self) -> Result<(), String> {
        let core_handle = self
            .core_handle
            .clone()
            .ok_or_else(|| "Core handle not available".to_string())?;

        let (response_tx, response_rx) = std::sync::mpsc::channel::<Result<(), String>>();
        core_handle
            .send(NostrCommand::StopBunker { response_tx })
            .map_err(|e| format!("Failed to send StopBunker command: {}", e))?;

        response_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("Timed out waiting for bunker stop: {}", e))??;

        self.bunker_running = false;
        self.bunker_uri = None;
        self.bunker_pending_requests.clear();
        self.bunker_pending_request_ids.clear();
        Ok(())
    }

    /// Persist bunker enabled flag and apply runtime lifecycle when possible.
    pub fn set_bunker_enabled(&mut self, enabled: bool) -> Result<(), String> {
        self.preferences.borrow_mut().set_bunker_enabled(enabled)?;
        if enabled {
            if self.keys.is_some() {
                self.start_bunker_with_rules()?;
            } else {
                self.bunker_running = false;
            }
        } else if self.bunker_running {
            self.stop_bunker()?;
        } else {
            self.bunker_running = false;
            self.bunker_uri = None;
        }
        Ok(())
    }

    /// Add a persisted bunker auto-approve rule and sync to runtime if needed.
    pub fn add_bunker_auto_approve_rule(
        &mut self,
        requester_pubkey: String,
        event_kind: Option<u16>,
    ) -> Result<(), String> {
        self.preferences
            .borrow_mut()
            .add_bunker_auto_approve_rule(requester_pubkey.clone(), event_kind)?;
        self.load_bunker_rules_from_preferences();

        if self.bunker_running {
            if let Some(core_handle) = self.core_handle.clone() {
                core_handle
                    .send(NostrCommand::AddBunkerAutoApproveRule {
                        requester_pubkey,
                        event_kind,
                    })
                    .map_err(|e| format!("Failed to sync bunker rule: {}", e))?;
            }
        }
        Ok(())
    }

    /// Remove a persisted bunker auto-approve rule and sync to runtime if needed.
    pub fn remove_bunker_auto_approve_rule(
        &mut self,
        requester_pubkey: &str,
        event_kind: Option<u16>,
    ) -> Result<(), String> {
        self.preferences
            .borrow_mut()
            .remove_bunker_auto_approve_rule(requester_pubkey, event_kind)?;
        self.load_bunker_rules_from_preferences();

        if self.bunker_running {
            if let Some(core_handle) = self.core_handle.clone() {
                core_handle
                    .send(NostrCommand::RemoveBunkerAutoApproveRule {
                        requester_pubkey: requester_pubkey.to_string(),
                        event_kind,
                    })
                    .map_err(|e| format!("Failed to sync bunker rule removal: {}", e))?;
            }
        }
        Ok(())
    }

    /// Refresh session-scoped bunker audit entries from core.
    pub fn refresh_bunker_audit_entries(&mut self) -> Result<(), String> {
        let core_handle = self
            .core_handle
            .clone()
            .ok_or_else(|| "Core handle not available".to_string())?;
        let (response_tx, response_rx) =
            std::sync::mpsc::channel::<Vec<tenex_core::nostr::bunker::BunkerAuditEntry>>();
        core_handle
            .send(NostrCommand::GetBunkerAuditLog { response_tx })
            .map_err(|e| format!("Failed to send GetBunkerAuditLog command: {}", e))?;

        let mut entries = response_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .map_err(|e| format!("Timed out waiting for bunker audit: {}", e))?;
        entries.sort_by(|a, b| b.completed_at_ms.cmp(&a.completed_at_ms));
        self.bunker_audit_entries = entries;
        Ok(())
    }

    /// Queue incoming bunker signing request if not already queued.
    pub fn enqueue_bunker_sign_request(
        &mut self,
        request: tenex_core::nostr::bunker::BunkerSignRequest,
    ) {
        if self
            .bunker_pending_request_ids
            .insert(request.request_id.clone())
        {
            self.bunker_pending_requests.push_back(request);
        }
    }

    /// Open next pending bunker approval modal when no modal is active.
    pub fn maybe_open_pending_bunker_approval(&mut self) {
        if !self.modal_state.is_none() {
            return;
        }

        if let Some(request) = self.bunker_pending_requests.pop_front() {
            self.bunker_pending_request_ids.remove(&request.request_id);
            self.modal_state =
                ModalState::BunkerApproval(crate::ui::modal::BunkerApprovalState::new(request));
            self.input_mode = InputMode::Normal;
        }
    }

    /// Resolve bunker approval decision and optionally persist auto-approve rule.
    pub fn resolve_bunker_sign_request(
        &mut self,
        request: tenex_core::nostr::bunker::BunkerSignRequest,
        approved: bool,
        persist_auto_approve: bool,
    ) {
        if let Some(core_handle) = self.core_handle.clone() {
            if let Err(e) = core_handle.send(NostrCommand::BunkerResponse {
                request_id: request.request_id.clone(),
                approved,
            }) {
                self.set_warning_status(&format!("Failed to send bunker response: {}", e));
            }
        }

        if approved && persist_auto_approve {
            if let Err(e) = self
                .add_bunker_auto_approve_rule(request.requester_pubkey.clone(), request.event_kind)
            {
                self.set_warning_status(&format!("Failed to save bunker rule: {}", e));
            }
        }

        let result_label = if approved { "Approved" } else { "Rejected" };
        self.set_warning_status(&format!(
            "{} bunker request {}",
            result_label, request.request_id
        ));
        self.modal_state = ModalState::None;
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
    pub fn show_backend_approval_modal(&mut self, backend_pubkey: String) {
        use crate::ui::modal::BackendApprovalState;
        self.modal_state = ModalState::BackendApproval(BackendApprovalState::new(backend_pubkey));
    }

    /// Initialize trusted backends from preferences (called on app init)
    pub fn init_trusted_backends(&mut self) {
        let prefs = self.preferences.borrow();
        let approved = prefs.approved_backend_pubkeys().clone();
        let blocked = prefs.blocked_backend_pubkeys().clone();
        drop(prefs);
        self.data_store
            .borrow_mut()
            .trust
            .set_trusted_backends(approved, blocked);
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
        self.sidebar_search.hierarchical_results.clear();
        self.sidebar_search.report_results.clear();

        // Search based on current tab
        match self.home_panel_focus {
            HomeTab::Reports => {
                self.sidebar_search.report_results =
                    search_reports(&self.sidebar_search.query, &store, &self.visible_projects);
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
                        HierarchicalSearchItem::ContextAncestor {
                            thread,
                            project_a_tag,
                            ..
                        } => (thread, project_a_tag),
                        HierarchicalSearchItem::MatchedConversation {
                            thread,
                            project_a_tag,
                            ..
                        } => (thread, project_a_tag),
                    };

                    // Close search
                    self.sidebar_search.visible = false;
                    self.sidebar_search.query.clear();
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
        use crate::ui::views::chat::grouping::should_render_q_tags;
        use std::collections::HashSet;

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

                // Try to find the target agent name
                // Priority: 1) kind:0 profile, 2) agent slug from project status, 3) short pubkey
                let target = if let Some(thread) = store.get_thread_by_id(thread_id) {
                    thread
                        .p_tags
                        .first()
                        .map(|pk| {
                            // Primary: Use kind:0 profile name
                            let profile_name = store.get_profile_name(pk);

                            // If profile name is just short pubkey, try project status as fallback
                            if profile_name.ends_with("...") {
                                // Fallback: Try agent slug from project status
                                store
                                    .find_project_for_thread(thread_id)
                                    .and_then(|a_tag| store.get_project_status(&a_tag))
                                    .and_then(|status| {
                                        status
                                            .agents
                                            .iter()
                                            .find(|a| a.pubkey == *pk)
                                            .map(|a| a.name.clone())
                                    })
                                    .unwrap_or(profile_name)
                            } else {
                                profile_name
                            }
                        })
                        .unwrap_or_else(|| "Unknown".to_string())
                } else {
                    "Unknown".to_string()
                };

                delegations.push(SidebarDelegation {
                    thread_id: thread_id.clone(),
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
                    // Look up the report to get its title (use a_tag to avoid slug collisions)
                    let title = store
                        .reports
                        .get_report_by_a_tag(a_tag)
                        .map(|r| r.title.clone())
                        .unwrap_or_else(|| coord.slug.clone());

                    reports.push(SidebarReport {
                        a_tag: a_tag.clone(),
                        title,
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
    Message,
}

#[derive(Debug, Clone)]
pub enum NudgeSkillSelectorItem {
    Nudge(tenex_core::models::Nudge),
    Skill(tenex_core::models::Skill),
}

impl NudgeSkillSelectorItem {
    pub fn id(&self) -> &str {
        match self {
            NudgeSkillSelectorItem::Nudge(nudge) => &nudge.id,
            NudgeSkillSelectorItem::Skill(skill) => &skill.id,
        }
    }

    pub fn pubkey(&self) -> &str {
        match self {
            NudgeSkillSelectorItem::Nudge(nudge) => &nudge.pubkey,
            NudgeSkillSelectorItem::Skill(skill) => &skill.pubkey,
        }
    }

    pub fn title(&self) -> &str {
        match self {
            NudgeSkillSelectorItem::Nudge(nudge) => &nudge.title,
            NudgeSkillSelectorItem::Skill(skill) => &skill.title,
        }
    }

    pub fn description(&self) -> &str {
        match self {
            NudgeSkillSelectorItem::Nudge(nudge) => &nudge.description,
            NudgeSkillSelectorItem::Skill(skill) => &skill.description,
        }
    }

    pub fn content_preview(&self, width: usize) -> String {
        match self {
            NudgeSkillSelectorItem::Nudge(nudge) => nudge.content_preview(width),
            NudgeSkillSelectorItem::Skill(skill) => skill.content_preview(width),
        }
    }

    pub fn kind_order(&self) -> u8 {
        match self {
            NudgeSkillSelectorItem::Nudge(_) => 0,
            NudgeSkillSelectorItem::Skill(_) => 1,
        }
    }

    pub fn skill_file_count(&self) -> usize {
        match self {
            NudgeSkillSelectorItem::Nudge(_) => 0,
            NudgeSkillSelectorItem::Skill(skill) => skill.file_ids.len(),
        }
    }
}

// =============================================================================
// TESTS
// =============================================================================

#[cfg(test)]
mod inbox_48h_cap_tests {
    use super::*;
    use tenex_core::constants::INBOX_48H_CAP_SECONDS;

    /// Verify the constant value is 48 hours in seconds
    #[test]
    fn test_48h_constant_value() {
        // 48 hours * 60 minutes * 60 seconds = 172,800 seconds
        assert_eq!(INBOX_48H_CAP_SECONDS, 172_800);
        assert_eq!(INBOX_48H_CAP_SECONDS, 48 * 60 * 60);
    }

    /// Items exactly at the cutoff boundary should be included
    #[test]
    fn test_boundary_at_exactly_48h() {
        let now: u64 = 200_000; // arbitrary "current" time
        let cutoff = now - INBOX_48H_CAP_SECONDS;

        // Item created exactly at cutoff should be included
        assert!(is_within_48h_cap(cutoff, now));
    }

    /// Items 1 second before the cutoff should be excluded
    #[test]
    fn test_boundary_just_before_48h() {
        let now: u64 = 200_000;
        let cutoff = now - INBOX_48H_CAP_SECONDS;

        // Item created 1 second before cutoff should be excluded
        assert!(!is_within_48h_cap(cutoff - 1, now));
    }

    /// Items 1 second after the cutoff should be included
    #[test]
    fn test_boundary_just_after_48h() {
        let now: u64 = 200_000;
        let cutoff = now - INBOX_48H_CAP_SECONDS;

        // Item created 1 second after cutoff should be included
        assert!(is_within_48h_cap(cutoff + 1, now));
    }

    /// Very recent items (within last hour) should be included
    #[test]
    fn test_recent_item_included() {
        let now: u64 = 200_000;
        let one_hour_ago = now - 3600;

        assert!(is_within_48h_cap(one_hour_ago, now));
    }

    /// Items from exactly now should be included
    #[test]
    fn test_current_time_included() {
        let now: u64 = 200_000;

        assert!(is_within_48h_cap(now, now));
    }

    /// Items from the future (edge case) should be included
    #[test]
    fn test_future_item_included() {
        let now: u64 = 200_000;
        let future = now + 1000;

        assert!(is_within_48h_cap(future, now));
    }

    /// Items from 72 hours ago should be excluded
    #[test]
    fn test_old_item_excluded() {
        let now: u64 = 300_000;
        let seventy_two_hours_ago = now - (72 * 60 * 60);

        assert!(!is_within_48h_cap(seventy_two_hours_ago, now));
    }

    /// Edge case: now is very small (near Unix epoch), no underflow
    #[test]
    fn test_no_underflow_near_epoch() {
        let now: u64 = 1000; // Very early time
                             // saturating_sub should handle this gracefully
        assert!(is_within_48h_cap(500, now)); // Item from 500 seconds ago
        assert!(is_within_48h_cap(0, now)); // Item from epoch should be included since now < 48h
    }

    /// Edge case: now is exactly 48 hours
    #[test]
    fn test_now_equals_48h() {
        let now = INBOX_48H_CAP_SECONDS;

        // Item at epoch (0) should be exactly at the boundary
        assert!(is_within_48h_cap(0, now));
    }
}

#[cfg(test)]
mod input_context_focus_tests {
    use super::InputContextFocus;

    #[test]
    fn non_draft_traversal_skips_project() {
        assert_eq!(
            InputContextFocus::Agent.move_right(false),
            InputContextFocus::NudgeSkill
        );
        assert_eq!(
            InputContextFocus::NudgeSkill.move_left(false),
            InputContextFocus::Agent
        );
    }

    #[test]
    fn draft_traversal_includes_project() {
        assert_eq!(
            InputContextFocus::Agent.move_right(true),
            InputContextFocus::Project
        );
        assert_eq!(
            InputContextFocus::Project.move_right(true),
            InputContextFocus::NudgeSkill
        );
        assert_eq!(
            InputContextFocus::NudgeSkill.move_left(true),
            InputContextFocus::Project
        );
        assert_eq!(
            InputContextFocus::Project.move_left(true),
            InputContextFocus::Agent
        );
    }
}

#[cfg(test)]
mod selected_agent_refresh_tests {
    use super::*;
    use serde_json::json;

    fn make_status(agents: Vec<ProjectAgent>) -> ProjectStatus {
        let mut tags: Vec<Vec<String>> =
            vec![vec!["a".to_string(), "31933:backend:project".to_string()]];

        for agent in &agents {
            let mut agent_tag = vec![
                "agent".to_string(),
                agent.pubkey.clone(),
                agent.name.clone(),
            ];
            if agent.is_pm {
                agent_tag.push("pm".to_string());
            }
            tags.push(agent_tag);

            if let Some(model) = &agent.model {
                tags.push(vec!["model".to_string(), model.clone(), agent.name.clone()]);
            }

            for tool in &agent.tools {
                tags.push(vec!["tool".to_string(), tool.clone(), agent.name.clone()]);
            }
        }

        let event = json!({
            "kind": 24010,
            "pubkey": "backend",
            "created_at": 1,
            "tags": tags,
        });

        ProjectStatus::from_value(&event).expect("status fixture should parse")
    }

    #[test]
    fn status_update_refreshes_selected_agent_model() {
        let current = ProjectAgent {
            pubkey: "agent-a".to_string(),
            name: "Agent A".to_string(),
            is_pm: false,
            model: Some("old-model".to_string()),
            tools: vec!["shell".to_string()],
        };
        let status = make_status(vec![ProjectAgent {
            pubkey: "agent-a".to_string(),
            name: "Agent A".to_string(),
            is_pm: false,
            model: Some("new-model".to_string()),
            tools: vec!["shell".to_string()],
        }]);

        let resolved = resolve_selected_agent_from_status(Some(&current), &status)
            .expect("selected agent should resolve");

        assert_eq!(resolved.model.as_deref(), Some("new-model"));
    }

    #[test]
    fn status_update_keeps_selected_agent_if_missing() {
        let current = ProjectAgent {
            pubkey: "agent-a".to_string(),
            name: "Agent A".to_string(),
            is_pm: false,
            model: Some("old-model".to_string()),
            tools: vec!["shell".to_string()],
        };
        let status = make_status(vec![ProjectAgent {
            pubkey: "agent-b".to_string(),
            name: "Agent B".to_string(),
            is_pm: true,
            model: Some("pm-model".to_string()),
            tools: vec![],
        }]);

        let resolved = resolve_selected_agent_from_status(Some(&current), &status)
            .expect("selected agent should remain set");

        assert_eq!(resolved.pubkey, "agent-a");
        assert_eq!(resolved.model.as_deref(), Some("old-model"));
    }

    #[test]
    fn status_update_defaults_to_pm_when_none_selected() {
        let status = make_status(vec![
            ProjectAgent {
                pubkey: "agent-a".to_string(),
                name: "Agent A".to_string(),
                is_pm: false,
                model: Some("model-a".to_string()),
                tools: vec![],
            },
            ProjectAgent {
                pubkey: "agent-pm".to_string(),
                name: "PM".to_string(),
                is_pm: true,
                model: Some("model-pm".to_string()),
                tools: vec![],
            },
        ]);

        let resolved = resolve_selected_agent_from_status(None, &status)
            .expect("pm should be selected by default");

        assert_eq!(resolved.pubkey, "agent-pm");
    }
}
