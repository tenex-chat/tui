//! Centralized Hotkey Registry
//!
//! This module provides a single source of truth for all keyboard shortcuts in the application.
//! All hotkeys are defined declaratively in the HOTKEYS constant, making it easy to:
//! - See all available hotkeys at a glance
//! - Detect conflicts between hotkeys
//! - Generate help text automatically
//! - Ensure consistency across views
//!
//! # Architecture
//!
//! - `HotkeyId`: Unique identifier for each action
//! - `HotkeyContext`: Where a hotkey is active (view/mode)
//! - `HotkeyBinding`: Declarative hotkey definition
//! - `HotkeyResolver`: Finds matching hotkey for key event + context

use crossterm::event::{KeyCode, KeyModifiers};
use std::collections::HashMap;

/// Unique identifier for each hotkey action.
/// This enum serves as the canonical list of all possible keyboard-triggered actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyId {
    // === Global Actions (work everywhere) ===
    Quit,
    CommandPalette,
    GoToHome,
    Help,

    // === Navigation ===
    NavigateUp,
    NavigateDown,
    NavigateLeft,
    NavigateRight,
    PageUp,
    PageDown,
    GoToTop,
    GoToBottom,
    Select,           // Enter
    Back,             // Escape

    // === Tab Management ===
    NextTab,
    PrevTab,
    CloseTab,
    TabModal,

    // === Home View - Recent Tab ===
    NewConversation,
    OpenSelected,
    ArchiveToggle,
    ShowHideArchived,      // Toggle visibility of archived conversations
    ShowHideArchivedProjects, // Toggle visibility of archived projects
    ExportJsonl,
    SwitchProject,
    TimeFilter,
    AgentBrowser,
    CreateProject,

    // === Home View - Inbox Tab ===
    MarkAsRead,
    MarkAllRead,

    // === Home View - Sidebar ===
    ToggleProjectVisibility,
    ProjectSettings,
    BootProject,
    StopAllAgents,

    // === Chat View - Normal Mode ===
    MentionAgent,
    SelectBranch,
    CopyMessage,
    ViewRawEvent,
    OpenTrace,
    StopAgent,
    GoToParent,
    ToggleSidebar,
    EnterEditMode,
    InConversationSearch,

    // === Chat View - Edit Mode ===
    SendMessage,
    ExpandEditor,
    InsertNewline,
    CancelEdit,
    HistorySearch,

    // === Agent Browser ===
    ViewAgent,
    CreateAgent,
    ForkAgent,
    CloneAgent,

    // === Text Editing (Vim-style) ===
    VimUp,
    VimDown,
    VimLeft,
    VimRight,
    VimWordForward,
    VimWordBackward,
    VimLineStart,
    VimLineEnd,
    VimDelete,
    VimDeleteLine,
    VimYank,
    VimPaste,
    VimUndo,
    VimInsertMode,
    VimInsertAfter,
    VimInsertLineEnd,
    VimInsertLineStart,
    VimNormalMode,

    // === Modal Actions ===
    ModalClose,
    ModalConfirm,
    ModalCancel,

    // === Report Viewer ===
    ToggleReportView,
    CopyReportId,
    CopyReportRaw,
    CopyReportMarkdown,
}

/// Context in which a hotkey is active.
/// A hotkey can be active in multiple contexts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyContext {
    /// Active everywhere, regardless of view or mode
    Global,

    /// Home view contexts
    HomeConversations,
    HomeInbox,
    HomeReports,
    HomeStatus,
    HomeSearch,
    HomeFeed,
    HomeSidebar,

    /// Chat view contexts
    ChatNormal,
    ChatEditing,
    ChatVimNormal,

    /// Modal contexts
    AnyModal,
    CommandPaletteModal,
    AgentSelectorModal,
    ProjectSelectorModal,
    AskModal,
    AttachmentModal,
    TabModal,
    SearchModal,
    ConversationActionsModal,
    ChatActionsModal,
    ProjectActionsModal,
    ViewRawEventModal,
    HotkeyHelpModal,
    NudgeSelectorModal,
    ReportViewerModal,
    AgentSettingsModal,
    ProjectSettingsModal,
    CreateProjectModal,
    CreateAgentModal,
    ExpandedEditorModal,

    /// Agent browser contexts
    AgentBrowserList,
    AgentBrowserDetail,

    /// Login view
    Login,

    /// Draft navigator modal
    DraftNavigatorModal,

    /// History search modal (Ctrl+R)
    HistorySearchModal,
}

impl HotkeyContext {
    /// Determine the current hotkey context based on application state.
    /// This is the bridge between the app state and the hotkey system.
    pub fn from_app_state(
        view: &super::View,
        input_mode: &super::InputMode,
        modal_state: &super::ModalState,
        home_panel_focus: &super::HomeTab,
        sidebar_focused: bool,
    ) -> Self {
        use super::{View, InputMode, ModalState, HomeTab};

        // Modal contexts take priority
        match modal_state {
            ModalState::CommandPalette(_) => return HotkeyContext::CommandPaletteModal,
            ModalState::AgentSelector { .. } => return HotkeyContext::AgentSelectorModal,
            ModalState::ProjectsModal { .. } => return HotkeyContext::ProjectSelectorModal,
            ModalState::AskModal(_) => return HotkeyContext::AskModal,
            ModalState::AttachmentEditor { .. } => return HotkeyContext::AttachmentModal,
            ModalState::ConversationActions(_) => return HotkeyContext::ConversationActionsModal,
            ModalState::ChatActions(_) => return HotkeyContext::ChatActionsModal,
            ModalState::ProjectActions(_) => return HotkeyContext::ProjectActionsModal,
            ModalState::ViewRawEvent { .. } => return HotkeyContext::ViewRawEventModal,
            ModalState::HotkeyHelp => return HotkeyContext::HotkeyHelpModal,
            ModalState::NudgeSelector(_) => return HotkeyContext::NudgeSelectorModal,
            ModalState::ReportViewer(_) => return HotkeyContext::ReportViewerModal,
            ModalState::AgentSettings(_) => return HotkeyContext::AgentSettingsModal,
            ModalState::ProjectSettings(_) => return HotkeyContext::ProjectSettingsModal,
            ModalState::CreateProject(_) => return HotkeyContext::CreateProjectModal,
            ModalState::CreateAgent(_) => return HotkeyContext::CreateAgentModal,
            ModalState::ExpandedEditor { .. } => return HotkeyContext::ExpandedEditorModal,
            ModalState::BranchSelector { .. } => return HotkeyContext::AnyModal,
            ModalState::DraftNavigator(_) => return HotkeyContext::DraftNavigatorModal,
            ModalState::BackendApproval(_) => return HotkeyContext::AnyModal,
            ModalState::DebugStats(_) => return HotkeyContext::AnyModal,
            ModalState::HistorySearch(_) => return HotkeyContext::HistorySearchModal,
            ModalState::None => {}
        }

        // View-based contexts
        match view {
            View::Login => HotkeyContext::Login,
            View::Home => {
                if sidebar_focused {
                    HotkeyContext::HomeSidebar
                } else {
                    match home_panel_focus {
                        HomeTab::Conversations => HotkeyContext::HomeConversations,
                        HomeTab::Inbox => HotkeyContext::HomeInbox,
                        HomeTab::Reports => HotkeyContext::HomeReports,
                        HomeTab::Status => HotkeyContext::HomeStatus,
                        HomeTab::Search => HotkeyContext::HomeSearch,
                        HomeTab::Feed => HotkeyContext::HomeFeed,
                    }
                }
            }
            View::Chat => {
                match input_mode {
                    InputMode::Editing => HotkeyContext::ChatEditing,
                    InputMode::Normal => HotkeyContext::ChatNormal,
                }
            }
            View::AgentBrowser => HotkeyContext::AgentBrowserList,
            View::LessonViewer => HotkeyContext::Global, // Fallback for now
        }
    }
}

/// A declarative hotkey binding definition.
#[derive(Debug, Clone)]
pub struct HotkeyBinding {
    /// Unique identifier for this action
    pub id: HotkeyId,
    /// The key code that triggers this hotkey
    pub key: KeyCode,
    /// Required modifiers (Ctrl, Alt, Shift)
    pub modifiers: KeyModifiers,
    /// Human-readable label for help text
    pub label: &'static str,
    /// Section/category for grouping in help
    pub section: &'static str,
    /// Contexts where this hotkey is active
    pub contexts: &'static [HotkeyContext],
    /// Priority (higher = checked first, for overlapping contexts)
    pub priority: u8,
}

impl HotkeyBinding {
    /// Create a new hotkey binding with no modifiers
    pub const fn new(
        id: HotkeyId,
        key: KeyCode,
        label: &'static str,
        section: &'static str,
        contexts: &'static [HotkeyContext],
    ) -> Self {
        Self {
            id,
            key,
            modifiers: KeyModifiers::NONE,
            label,
            section,
            contexts,
            priority: 0,
        }
    }

    /// Create a new hotkey binding with modifiers
    pub const fn with_modifiers(
        id: HotkeyId,
        key: KeyCode,
        modifiers: KeyModifiers,
        label: &'static str,
        section: &'static str,
        contexts: &'static [HotkeyContext],
    ) -> Self {
        Self {
            id,
            key,
            modifiers,
            label,
            section,
            contexts,
            priority: 0,
        }
    }

    /// Create with Ctrl modifier
    pub const fn ctrl(
        id: HotkeyId,
        key: KeyCode,
        label: &'static str,
        section: &'static str,
        contexts: &'static [HotkeyContext],
    ) -> Self {
        Self::with_modifiers(id, key, KeyModifiers::CONTROL, label, section, contexts)
    }

    /// Create with Alt modifier
    pub const fn alt(
        id: HotkeyId,
        key: KeyCode,
        label: &'static str,
        section: &'static str,
        contexts: &'static [HotkeyContext],
    ) -> Self {
        Self::with_modifiers(id, key, KeyModifiers::ALT, label, section, contexts)
    }

    /// Create with Shift modifier
    pub const fn shift(
        id: HotkeyId,
        key: KeyCode,
        label: &'static str,
        section: &'static str,
        contexts: &'static [HotkeyContext],
    ) -> Self {
        Self::with_modifiers(id, key, KeyModifiers::SHIFT, label, section, contexts)
    }

    /// Set priority (higher = checked first)
    pub const fn with_priority(mut self, priority: u8) -> Self {
        self.priority = priority;
        self
    }

    /// Check if this hotkey matches the given key event
    pub fn matches(&self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        self.key == key && self.modifiers == modifiers
    }

    /// Check if this hotkey is active in the given context
    pub fn is_active_in(&self, context: HotkeyContext) -> bool {
        self.contexts.contains(&HotkeyContext::Global) || self.contexts.contains(&context)
    }

    /// Get a display string for the key combination
    pub fn key_display(&self) -> String {
        let mut parts = Vec::new();

        if self.modifiers.contains(KeyModifiers::CONTROL) {
            parts.push("Ctrl");
        }
        if self.modifiers.contains(KeyModifiers::ALT) {
            parts.push("Alt");
        }
        if self.modifiers.contains(KeyModifiers::SHIFT) {
            parts.push("Shift");
        }

        let key_str = match self.key {
            KeyCode::Char(' ') => "Space".to_string(),
            KeyCode::Char(c) => c.to_string(),
            KeyCode::Enter => "Enter".to_string(),
            KeyCode::Esc => "Esc".to_string(),
            KeyCode::Tab => "Tab".to_string(),
            KeyCode::BackTab => "Shift+Tab".to_string(),
            KeyCode::Backspace => "Backspace".to_string(),
            KeyCode::Delete => "Delete".to_string(),
            KeyCode::Up => "↑".to_string(),
            KeyCode::Down => "↓".to_string(),
            KeyCode::Left => "←".to_string(),
            KeyCode::Right => "→".to_string(),
            KeyCode::PageUp => "PgUp".to_string(),
            KeyCode::PageDown => "PgDn".to_string(),
            KeyCode::Home => "Home".to_string(),
            KeyCode::End => "End".to_string(),
            KeyCode::F(n) => format!("F{}", n),
            _ => "?".to_string(),
        };

        parts.push(&key_str);

        // Handle BackTab specially (already includes Shift)
        if matches!(self.key, KeyCode::BackTab) && self.modifiers.contains(KeyModifiers::SHIFT) {
            return "Shift+Tab".to_string();
        }

        parts.join("+")
    }
}

// ============================================================================
// CENTRALIZED HOTKEY DEFINITIONS
// ============================================================================
//
// This is THE SINGLE SOURCE OF TRUTH for all hotkeys in the application.
// To add a new hotkey:
// 1. Add an entry to HotkeyId enum above
// 2. Add the binding to this array
// 3. Handle the HotkeyId in your handler function
//
// Conventions:
// - Ctrl+T is ALWAYS the command palette (never "/" or other keys)
// - Escape always goes back/closes
// - Enter always confirms/selects
// - Navigation uses arrows OR vim keys (j/k) depending on context
// ============================================================================

/// All hotkey bindings in the application
pub static HOTKEYS: &[HotkeyBinding] = &[
    // === Global Actions ===
    HotkeyBinding::new(
        HotkeyId::Quit,
        KeyCode::Char('q'),
        "Quit",
        "Global",
        &[HotkeyContext::Global],
    ),
    HotkeyBinding::ctrl(
        HotkeyId::CommandPalette,
        KeyCode::Char('t'),
        "Command Palette",
        "Global",
        &[HotkeyContext::Global],
    ).with_priority(100), // High priority - always available
    HotkeyBinding::new(
        HotkeyId::GoToHome,
        KeyCode::Char('1'),
        "Go to Home",
        "Navigation",
        &[HotkeyContext::Global],
    ),
    HotkeyBinding::new(
        HotkeyId::Help,
        KeyCode::Char('?'),
        "Help",
        "Global",
        &[HotkeyContext::Global],
    ),

    // === Navigation (Universal) ===
    HotkeyBinding::new(
        HotkeyId::NavigateUp,
        KeyCode::Up,
        "Navigate Up",
        "Navigation",
        &[HotkeyContext::Global],
    ),
    HotkeyBinding::new(
        HotkeyId::NavigateDown,
        KeyCode::Down,
        "Navigate Down",
        "Navigation",
        &[HotkeyContext::Global],
    ),
    HotkeyBinding::new(
        HotkeyId::NavigateLeft,
        KeyCode::Left,
        "Navigate Left",
        "Navigation",
        &[HotkeyContext::HomeSidebar],
    ),
    HotkeyBinding::new(
        HotkeyId::NavigateRight,
        KeyCode::Right,
        "Navigate Right",
        "Navigation",
        &[HotkeyContext::HomeSidebar],
    ),
    HotkeyBinding::new(
        HotkeyId::PageUp,
        KeyCode::PageUp,
        "Page Up",
        "Navigation",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::PageDown,
        KeyCode::PageDown,
        "Page Down",
        "Navigation",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::GoToTop,
        KeyCode::Home,
        "Go to Top",
        "Navigation",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::GoToBottom,
        KeyCode::End,
        "Go to Bottom",
        "Navigation",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::Select,
        KeyCode::Enter,
        "Select",
        "Navigation",
        &[HotkeyContext::Global],
    ),
    HotkeyBinding::new(
        HotkeyId::Back,
        KeyCode::Esc,
        "Back / Close",
        "Navigation",
        &[HotkeyContext::Global],
    ),

    // === Tab Management ===
    // Tab navigation is done via Ctrl+T + Left/Right (handled in command palette)
    // These are documented here but not used for direct key resolution
    HotkeyBinding::new(
        HotkeyId::CloseTab,
        KeyCode::Char('x'),
        "Close Tab",
        "Tabs",
        &[HotkeyContext::ChatNormal],
    ),

    // === Home View - Recent Tab ===
    HotkeyBinding::new(
        HotkeyId::NewConversation,
        KeyCode::Char('n'),
        "New Conversation",
        "Conversation",
        &[HotkeyContext::HomeConversations, HotkeyContext::HomeSidebar, HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::OpenSelected,
        KeyCode::Char('o'),
        "Open Selected",
        "Conversation",
        &[HotkeyContext::HomeConversations, HotkeyContext::HomeInbox, HotkeyContext::HomeReports],
    ),
    HotkeyBinding::new(
        HotkeyId::ArchiveToggle,
        KeyCode::Char('a'),
        "Archive/Unarchive",
        "Conversation",
        &[HotkeyContext::HomeConversations],
    ),
    HotkeyBinding::new(
        HotkeyId::ExportJsonl,
        KeyCode::Char('e'),
        "Export JSONL",
        "Conversation",
        &[HotkeyContext::HomeConversations, HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::SwitchProject,
        KeyCode::Char('p'),
        "Switch Project",
        "Filter",
        &[HotkeyContext::HomeConversations, HotkeyContext::HomeInbox, HotkeyContext::HomeReports],
    ),
    HotkeyBinding::new(
        HotkeyId::TimeFilter,
        KeyCode::Char('f'),
        "Time Filter",
        "Filter",
        &[HotkeyContext::HomeConversations, HotkeyContext::HomeFeed],
    ),
    HotkeyBinding::shift(
        HotkeyId::AgentBrowser,
        KeyCode::Char('A'),
        "Agent Browser",
        "Other",
        &[HotkeyContext::HomeConversations],
    ),
    HotkeyBinding::shift(
        HotkeyId::CreateProject,
        KeyCode::Char('N'),
        "Create Project",
        "Other",
        &[HotkeyContext::HomeConversations],
    ),
    HotkeyBinding::shift(
        HotkeyId::ShowHideArchived,
        KeyCode::Char('H'),
        "Show/Hide Archived Conversations",
        "Filter",
        &[HotkeyContext::HomeConversations, HotkeyContext::HomeInbox, HotkeyContext::HomeStatus, HotkeyContext::HomeSearch, HotkeyContext::HomeFeed],
    ),
    HotkeyBinding::shift(
        HotkeyId::ShowHideArchivedProjects,
        KeyCode::Char('P'),
        "Show/Hide Archived Projects",
        "Filter",
        &[HotkeyContext::HomeConversations, HotkeyContext::HomeInbox, HotkeyContext::HomeReports, HotkeyContext::HomeStatus, HotkeyContext::HomeSearch, HotkeyContext::HomeFeed],
    ),
    HotkeyBinding::shift(
        HotkeyId::ShowHideArchivedProjects,
        KeyCode::Char('H'),
        "Show/Hide Archived Projects",
        "Filter",
        &[HotkeyContext::HomeSidebar],
    ),

    // === Home View - Inbox Tab ===
    HotkeyBinding::shift(
        HotkeyId::MarkAsRead,
        KeyCode::Char('R'),
        "Mark as Read",
        "Inbox",
        &[HotkeyContext::HomeInbox],
    ),
    HotkeyBinding::shift(
        HotkeyId::MarkAllRead,
        KeyCode::Char('M'),
        "Mark All Read",
        "Inbox",
        &[HotkeyContext::HomeInbox],
    ),

    // === Home View - Sidebar ===
    HotkeyBinding::new(
        HotkeyId::ToggleProjectVisibility,
        KeyCode::Char(' '),
        "Toggle Visibility",
        "Project",
        &[HotkeyContext::HomeSidebar],
    ),
    HotkeyBinding::new(
        HotkeyId::ProjectSettings,
        KeyCode::Char('s'),
        "Settings",
        "Project",
        &[HotkeyContext::HomeSidebar],
    ),
    HotkeyBinding::new(
        HotkeyId::BootProject,
        KeyCode::Char('b'),
        "Boot Project",
        "Project",
        &[HotkeyContext::HomeSidebar],
    ),
    HotkeyBinding::new(
        HotkeyId::StopAllAgents,
        KeyCode::Char('.'),
        "Stop All Agents",
        "Project",
        &[HotkeyContext::HomeSidebar],
    ),

    // === Chat View - Normal Mode ===
    HotkeyBinding::new(
        HotkeyId::MentionAgent,
        KeyCode::Char('@'),
        "Mention Agent",
        "Input",
        &[HotkeyContext::ChatNormal, HotkeyContext::ChatEditing],
    ),
    HotkeyBinding::new(
        HotkeyId::SelectBranch,
        KeyCode::Char('%'),
        "Select Branch",
        "Input",
        &[HotkeyContext::ChatNormal, HotkeyContext::ChatEditing],
    ),
    HotkeyBinding::new(
        HotkeyId::CopyMessage,
        KeyCode::Char('y'),
        "Copy Content",
        "Message",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::ViewRawEvent,
        KeyCode::Char('v'),
        "View Raw Event",
        "Message",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::OpenTrace,
        KeyCode::Char('t'),
        "Open Trace",
        "Message",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::StopAgent,
        KeyCode::Char('.'),
        "Stop Agent",
        "Agent",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::GoToParent,
        KeyCode::Char('g'),
        "Go to Parent",
        "Conversation",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::shift(
        HotkeyId::ToggleSidebar,
        KeyCode::Char('T'),
        "Toggle Sidebar",
        "View",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::EnterEditMode,
        KeyCode::Char('i'),
        "Enter Edit Mode",
        "Input",
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::ctrl(
        HotkeyId::InConversationSearch,
        KeyCode::Char('f'),
        "Search in Conversation",
        "Search",
        &[HotkeyContext::ChatNormal],
    ),

    // === Chat View - Edit Mode ===
    HotkeyBinding::ctrl(
        HotkeyId::SendMessage,
        KeyCode::Enter,
        "Send Message",
        "Input",
        &[HotkeyContext::ChatEditing],
    ).with_priority(50),
    HotkeyBinding::ctrl(
        HotkeyId::ExpandEditor,
        KeyCode::Char('e'),
        "Expand Editor",
        "Input",
        &[HotkeyContext::ChatEditing],
    ),
    HotkeyBinding::shift(
        HotkeyId::InsertNewline,
        KeyCode::Enter,
        "Insert Newline",
        "Input",
        &[HotkeyContext::ChatEditing],
    ),
    HotkeyBinding::new(
        HotkeyId::CancelEdit,
        KeyCode::Esc,
        "Cancel Edit",
        "Input",
        &[HotkeyContext::ChatEditing],
    ).with_priority(50),
    HotkeyBinding::ctrl(
        HotkeyId::HistorySearch,
        KeyCode::Char('r'),
        "Search History",
        "Input",
        &[HotkeyContext::ChatEditing],
    ),

    // === Agent Browser ===
    HotkeyBinding::new(
        HotkeyId::ViewAgent,
        KeyCode::Char('o'),
        "View Agent",
        "Agent",
        &[HotkeyContext::AgentBrowserList],
    ),
    HotkeyBinding::new(
        HotkeyId::CreateAgent,
        KeyCode::Char('n'),
        "Create Agent",
        "Agent",
        &[HotkeyContext::AgentBrowserList],
    ),
    HotkeyBinding::new(
        HotkeyId::ForkAgent,
        KeyCode::Char('f'),
        "Fork Agent",
        "Agent",
        &[HotkeyContext::AgentBrowserDetail],
    ),
    HotkeyBinding::new(
        HotkeyId::CloneAgent,
        KeyCode::Char('c'),
        "Clone Agent",
        "Agent",
        &[HotkeyContext::AgentBrowserDetail],
    ),

    // === Report Viewer ===
    HotkeyBinding::new(
        HotkeyId::ToggleReportView,
        KeyCode::Tab,
        "Toggle View Mode",
        "Report",
        &[HotkeyContext::ReportViewerModal],
    ),
    HotkeyBinding::new(
        HotkeyId::CopyReportId,
        KeyCode::Char('c'),
        "Copy Options",
        "Report",
        &[HotkeyContext::ReportViewerModal],
    ),

    // === Modal Actions ===
    HotkeyBinding::new(
        HotkeyId::ModalClose,
        KeyCode::Esc,
        "Close",
        "Modal",
        &[HotkeyContext::AnyModal],
    ).with_priority(10),
    HotkeyBinding::new(
        HotkeyId::ModalConfirm,
        KeyCode::Enter,
        "Confirm",
        "Modal",
        &[HotkeyContext::AnyModal],
    ),
];

// ============================================================================
// HOTKEY RESOLVER
// ============================================================================

/// Resolver for finding matching hotkeys based on key events and context.
pub struct HotkeyResolver {
    /// Hotkeys sorted by priority (highest first)
    bindings: Vec<&'static HotkeyBinding>,
}

impl Default for HotkeyResolver {
    fn default() -> Self {
        Self::new()
    }
}

impl HotkeyResolver {
    /// Create a new resolver with all hotkey bindings
    pub fn new() -> Self {
        let mut bindings: Vec<&'static HotkeyBinding> = HOTKEYS.iter().collect();
        // Sort by priority descending (higher priority first)
        bindings.sort_by(|a, b| b.priority.cmp(&a.priority));
        Self { bindings }
    }

    /// Find the matching hotkey for a key event in the given context.
    /// Returns the first matching hotkey (respecting priority).
    pub fn resolve(&self, key: KeyCode, modifiers: KeyModifiers, context: HotkeyContext) -> Option<HotkeyId> {
        for binding in &self.bindings {
            if binding.matches(key, modifiers) && binding.is_active_in(context) {
                return Some(binding.id);
            }
        }
        None
    }

    /// Get all hotkeys available in the given context.
    pub fn hotkeys_for_context(&self, context: HotkeyContext) -> Vec<&'static HotkeyBinding> {
        self.bindings
            .iter()
            .filter(|b| b.is_active_in(context))
            .copied()
            .collect()
    }

    /// Get all hotkeys grouped by section for help display.
    pub fn hotkeys_by_section(&self, context: HotkeyContext) -> HashMap<&'static str, Vec<&'static HotkeyBinding>> {
        let mut sections: HashMap<&'static str, Vec<&'static HotkeyBinding>> = HashMap::new();

        for binding in self.hotkeys_for_context(context) {
            sections
                .entry(binding.section)
                .or_default()
                .push(binding);
        }

        sections
    }

    /// Generate help text for the given context.
    pub fn generate_help(&self, context: HotkeyContext) -> Vec<(String, String)> {
        self.hotkeys_for_context(context)
            .iter()
            .map(|b| (b.key_display(), b.label.to_string()))
            .collect()
    }

    /// Check for conflicting hotkeys (same key+modifiers in same context).
    /// Returns pairs of conflicting hotkey IDs.
    pub fn find_conflicts(&self) -> Vec<(HotkeyId, HotkeyId)> {
        let mut conflicts = Vec::new();

        for (i, a) in self.bindings.iter().enumerate() {
            for b in self.bindings.iter().skip(i + 1) {
                // Same key and modifiers?
                if a.key == b.key && a.modifiers == b.modifiers {
                    // Check for overlapping contexts
                    for ctx_a in a.contexts {
                        for ctx_b in b.contexts {
                            if ctx_a == ctx_b || *ctx_a == HotkeyContext::Global || *ctx_b == HotkeyContext::Global {
                                conflicts.push((a.id, b.id));
                            }
                        }
                    }
                }
            }
        }

        conflicts
    }
}

/// Global resolver instance (lazy initialized)
pub fn resolver() -> &'static HotkeyResolver {
    use std::sync::OnceLock;
    static RESOLVER: OnceLock<HotkeyResolver> = OnceLock::new();
    RESOLVER.get_or_init(HotkeyResolver::new)
}

/// Resolve a key event to a HotkeyId given the current context.
/// This is the main entry point for the hotkey system.
pub fn resolve_hotkey(key: KeyCode, modifiers: KeyModifiers, context: HotkeyContext) -> Option<HotkeyId> {
    resolver().resolve(key, modifiers, context)
}

/// Get the binding for a specific hotkey ID (for help display).
pub fn get_binding(id: HotkeyId) -> Option<&'static HotkeyBinding> {
    HOTKEYS.iter().find(|b| b.id == id)
}

/// Get all bindings for a context (for help display).
pub fn get_bindings_for_context(context: HotkeyContext) -> Vec<&'static HotkeyBinding> {
    resolver().hotkeys_for_context(context)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_matching() {
        let binding = HotkeyBinding::ctrl(
            HotkeyId::CommandPalette,
            KeyCode::Char('t'),
            "Test",
            "Test",
            &[HotkeyContext::Global],
        );

        assert!(binding.matches(KeyCode::Char('t'), KeyModifiers::CONTROL));
        assert!(!binding.matches(KeyCode::Char('t'), KeyModifiers::NONE));
        assert!(!binding.matches(KeyCode::Char('x'), KeyModifiers::CONTROL));
    }

    #[test]
    fn test_context_check() {
        let binding = HotkeyBinding::new(
            HotkeyId::OpenSelected,
            KeyCode::Char('o'),
            "Open",
            "Test",
            &[HotkeyContext::HomeConversations, HotkeyContext::HomeInbox],
        );

        assert!(binding.is_active_in(HotkeyContext::HomeConversations));
        assert!(binding.is_active_in(HotkeyContext::HomeInbox));
        assert!(!binding.is_active_in(HotkeyContext::ChatNormal));
    }

    #[test]
    fn test_global_context() {
        let binding = HotkeyBinding::new(
            HotkeyId::Quit,
            KeyCode::Char('q'),
            "Quit",
            "Test",
            &[HotkeyContext::Global],
        );

        // Global hotkeys should be active in any context
        assert!(binding.is_active_in(HotkeyContext::HomeConversations));
        assert!(binding.is_active_in(HotkeyContext::ChatNormal));
        assert!(binding.is_active_in(HotkeyContext::AnyModal));
    }

    #[test]
    fn test_resolver() {
        let resolver = HotkeyResolver::new();

        // Test Ctrl+T resolves to CommandPalette
        let result = resolver.resolve(
            KeyCode::Char('t'),
            KeyModifiers::CONTROL,
            HotkeyContext::HomeConversations,
        );
        assert_eq!(result, Some(HotkeyId::CommandPalette));

        // Test 'q' resolves to Quit
        let result = resolver.resolve(
            KeyCode::Char('q'),
            KeyModifiers::NONE,
            HotkeyContext::ChatNormal,
        );
        assert_eq!(result, Some(HotkeyId::Quit));
    }

    #[test]
    fn test_key_display() {
        let binding = HotkeyBinding::ctrl(
            HotkeyId::CommandPalette,
            KeyCode::Char('t'),
            "Test",
            "Test",
            &[HotkeyContext::Global],
        );
        assert_eq!(binding.key_display(), "Ctrl+t");

        let binding = HotkeyBinding::new(
            HotkeyId::NavigateUp,
            KeyCode::Up,
            "Test",
            "Test",
            &[HotkeyContext::Global],
        );
        assert_eq!(binding.key_display(), "↑");
    }
}
