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

/// Unique identifier for each hotkey action.
/// This enum serves as the canonical list of all possible keyboard-triggered actions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HotkeyId {
    // === Global Actions (work everywhere) ===
    Quit,
    CommandPalette,
    GoToHome,
    Help,
    JumpToNotification,
    WorkspaceManager,

    // === Navigation ===
    NavigateUp,
    NavigateDown,
    // NavigateLeft and NavigateRight removed - conflicted with UnfocusSidebar/FocusSidebar
    PageUp,
    PageDown,
    GoToTop,
    GoToBottom,
    Select, // Enter
    Back,   // Escape

    // === Tab Management ===
    CloseTab,
    NextHomeTab, // Tab key in Home view
    PrevHomeTab, // Shift+Tab in Home view

    // === Home View - Recent Tab ===
    NewConversation,
    NewConversationWithPicker,
    OpenSelected,
    ArchiveToggle,
    ShowHideArchived, // Toggle visibility of all archived items
    ExportJsonl,
    SwitchProject,
    TimeFilter,
    AgentBrowser,
    CreateProject,
    SearchReports,       // '/' in Reports tab
    ToggleHideScheduled, // Shift+S to toggle scheduled events filter

    // === Home View - Inbox Tab ===
    MarkAsRead,
    MarkAllRead,

    // === Home View - Sidebar ===
    ToggleProjectVisibility,
    ProjectSettings,
    BootProject,
    StopAllAgents,
    FocusSidebar,   // Right arrow to focus sidebar
    UnfocusSidebar, // Left arrow to unfocus sidebar

    // === Chat View - Normal Mode ===
    MentionAgent,
    CopyMessage,
    ViewRawEvent,
    OpenTrace,
    StopAgent,
    GoToParent,
    EnterEditMode,
    InConversationSearch,

    // === Chat View - Edit Mode ===
    SendMessage,
    ExpandEditor,
    InsertNewline,
    CancelEdit,
    HistorySearch,
    OpenNudgeSkillSelector,

    // === Agent Browser ===
    ViewAgent,
    CreateAgent,
    ForkAgent,
    CloneAgent,

    // === Modal Actions ===
    ModalClose,
    ModalConfirm,

    // === Report Viewer ===
    ToggleReportView,
    CopyReportId,
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
    HomeActiveWork,
    HomeStats,
    HomeSidebar,

    /// Chat view contexts
    ChatNormal,
    ChatEditing,

    /// Modal contexts
    AnyModal,
    CommandPaletteModal,
    AgentConfigModal,
    ProjectSelectorModal,
    AskModal,
    AttachmentModal,
    ConversationActionsModal,
    ChatActionsModal,
    ProjectActionsModal,
    ViewRawEventModal,
    HotkeyHelpModal,
    NudgeSkillSelectorModal,
    ReportViewerModal,
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

    /// Workspace manager modal
    WorkspaceManagerModal,
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
        use super::{HomeTab, InputMode, ModalState, View};

        // Modal contexts take priority
        match modal_state {
            ModalState::CommandPalette(_) => return HotkeyContext::CommandPaletteModal,
            ModalState::AgentConfig(_) => return HotkeyContext::AgentConfigModal,
            ModalState::ProjectsModal { .. } => return HotkeyContext::ProjectSelectorModal,
            ModalState::ComposerProjectSelector { .. } => {
                return HotkeyContext::ProjectSelectorModal
            }
            ModalState::AskModal(_) => return HotkeyContext::AskModal,
            ModalState::AttachmentEditor { .. } => return HotkeyContext::AttachmentModal,
            ModalState::ConversationActions(_) => return HotkeyContext::ConversationActionsModal,
            ModalState::ChatActions(_) => return HotkeyContext::ChatActionsModal,
            ModalState::ProjectActions(_) => return HotkeyContext::ProjectActionsModal,
            ModalState::ViewRawEvent { .. } => return HotkeyContext::ViewRawEventModal,
            ModalState::HotkeyHelp => return HotkeyContext::HotkeyHelpModal,
            ModalState::NudgeSkillSelector(_) => return HotkeyContext::NudgeSkillSelectorModal,
            ModalState::ReportViewer(_) => return HotkeyContext::ReportViewerModal,
            ModalState::ProjectSettings(_) => return HotkeyContext::ProjectSettingsModal,
            ModalState::CreateProject(_) => return HotkeyContext::CreateProjectModal,
            ModalState::CreateAgent(_) => return HotkeyContext::CreateAgentModal,
            ModalState::ExpandedEditor { .. } => return HotkeyContext::ExpandedEditorModal,
            ModalState::DraftNavigator(_) => return HotkeyContext::DraftNavigatorModal,
            ModalState::BackendApproval(_) => return HotkeyContext::AnyModal,
            ModalState::DebugStats(_) => return HotkeyContext::AnyModal,
            ModalState::HistorySearch(_) => return HotkeyContext::HistorySearchModal,
            ModalState::WorkspaceManager(_) => return HotkeyContext::WorkspaceManagerModal,
            ModalState::NudgeList(_) => return HotkeyContext::AnyModal,
            ModalState::NudgeCreate(_) => return HotkeyContext::AnyModal,
            ModalState::NudgeDetail(_) => return HotkeyContext::AnyModal,
            ModalState::NudgeDeleteConfirm(_) => return HotkeyContext::AnyModal,
            ModalState::AgentDeletion(_) => return HotkeyContext::AnyModal,
            ModalState::AppSettings(_) => return HotkeyContext::AnyModal,
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
                        HomeTab::ActiveWork => HotkeyContext::HomeActiveWork,
                        HomeTab::Stats => HotkeyContext::HomeStats,
                    }
                }
            }
            View::Chat => match input_mode {
                InputMode::Editing => HotkeyContext::ChatEditing,
                InputMode::Normal => HotkeyContext::ChatNormal,
            },
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
    /// Contexts where this hotkey is active
    pub contexts: &'static [HotkeyContext],
    /// Priority (higher = checked first, for overlapping contexts)
    pub priority: u8,
}

impl HotkeyBinding {
    /// Create a new hotkey binding with no modifiers
    pub const fn new(id: HotkeyId, key: KeyCode, contexts: &'static [HotkeyContext]) -> Self {
        Self {
            id,
            key,
            modifiers: KeyModifiers::NONE,
            contexts,
            priority: 0,
        }
    }

    /// Create a new hotkey binding with modifiers
    pub const fn with_modifiers(
        id: HotkeyId,
        key: KeyCode,
        modifiers: KeyModifiers,
        contexts: &'static [HotkeyContext],
    ) -> Self {
        Self {
            id,
            key,
            modifiers,
            contexts,
            priority: 0,
        }
    }

    /// Create with Ctrl modifier
    pub const fn ctrl(id: HotkeyId, key: KeyCode, contexts: &'static [HotkeyContext]) -> Self {
        Self::with_modifiers(id, key, KeyModifiers::CONTROL, contexts)
    }

    /// Create with Alt modifier
    pub const fn alt(id: HotkeyId, key: KeyCode, contexts: &'static [HotkeyContext]) -> Self {
        Self::with_modifiers(id, key, KeyModifiers::ALT, contexts)
    }

    /// Create with Shift modifier
    pub const fn shift(id: HotkeyId, key: KeyCode, contexts: &'static [HotkeyContext]) -> Self {
        Self::with_modifiers(id, key, KeyModifiers::SHIFT, contexts)
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
    HotkeyBinding::new(HotkeyId::Quit, KeyCode::Char('q'), &[HotkeyContext::Global]),
    HotkeyBinding::ctrl(
        HotkeyId::CommandPalette,
        KeyCode::Char('t'),
        &[HotkeyContext::Global],
    )
    .with_priority(100), // High priority - always available
    HotkeyBinding::new(
        HotkeyId::GoToHome,
        KeyCode::Char('1'),
        &[HotkeyContext::Global],
    ),
    HotkeyBinding::new(HotkeyId::Help, KeyCode::Char('?'), &[HotkeyContext::Global]),
    HotkeyBinding::alt(
        HotkeyId::JumpToNotification,
        KeyCode::Char('m'),
        &[HotkeyContext::Global],
    )
    .with_priority(90), // High priority - works almost everywhere
    HotkeyBinding::ctrl(
        HotkeyId::WorkspaceManager,
        KeyCode::Char('p'),
        &[HotkeyContext::Global],
    )
    .with_priority(95), // High priority - opens workspace manager
    // === Navigation (Universal) ===
    HotkeyBinding::new(HotkeyId::NavigateUp, KeyCode::Up, &[HotkeyContext::Global]),
    HotkeyBinding::new(
        HotkeyId::NavigateDown,
        KeyCode::Down,
        &[HotkeyContext::Global],
    ),
    // NOTE: NavigateLeft and NavigateRight for HomeSidebar were removed
    // because they conflicted with UnfocusSidebar (Left) which is the
    // correct binding for unfocusing the sidebar.
    HotkeyBinding::new(
        HotkeyId::PageUp,
        KeyCode::PageUp,
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::PageDown,
        KeyCode::PageDown,
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::GoToTop,
        KeyCode::Home,
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::GoToBottom,
        KeyCode::End,
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(HotkeyId::Select, KeyCode::Enter, &[HotkeyContext::Global]),
    HotkeyBinding::new(HotkeyId::Back, KeyCode::Esc, &[HotkeyContext::Global]),
    // === Tab Management ===
    // Tab navigation is done via Ctrl+T + Left/Right (handled in command palette)
    // These are documented here but not used for direct key resolution
    HotkeyBinding::new(
        HotkeyId::CloseTab,
        KeyCode::Char('x'),
        &[HotkeyContext::ChatNormal],
    ),
    // === Home View - Recent Tab ===
    HotkeyBinding::new(
        HotkeyId::NewConversation,
        KeyCode::Char('n'),
        &[
            HotkeyContext::HomeConversations,
            HotkeyContext::HomeSidebar,
            HotkeyContext::ChatNormal,
        ],
    ),
    HotkeyBinding::new(
        HotkeyId::OpenSelected,
        KeyCode::Char('o'),
        &[
            HotkeyContext::HomeConversations,
            HotkeyContext::HomeInbox,
            HotkeyContext::HomeReports,
        ],
    ),
    HotkeyBinding::new(
        HotkeyId::ArchiveToggle,
        KeyCode::Char('a'),
        &[HotkeyContext::HomeConversations],
    ),
    HotkeyBinding::new(
        HotkeyId::ExportJsonl,
        KeyCode::Char('e'),
        &[HotkeyContext::HomeConversations, HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::SwitchProject,
        KeyCode::Char('p'),
        &[
            HotkeyContext::HomeConversations,
            HotkeyContext::HomeInbox,
            HotkeyContext::HomeReports,
        ],
    ),
    HotkeyBinding::new(
        HotkeyId::TimeFilter,
        KeyCode::Char('f'),
        &[HotkeyContext::HomeConversations],
    ),
    HotkeyBinding::shift(
        HotkeyId::AgentBrowser,
        KeyCode::Char('B'),
        &[HotkeyContext::HomeConversations],
    ),
    HotkeyBinding::shift(
        HotkeyId::CreateProject,
        KeyCode::Char('C'),
        &[
            HotkeyContext::HomeConversations,
            HotkeyContext::AgentBrowserList,
            HotkeyContext::AgentBrowserDetail,
        ],
    ),
    HotkeyBinding::shift(
        HotkeyId::NewConversation,
        KeyCode::Char('N'),
        &[HotkeyContext::HomeConversations],
    ),
    HotkeyBinding::shift(
        HotkeyId::NewConversationWithPicker,
        KeyCode::Char('P'),
        &[HotkeyContext::HomeConversations],
    ),
    HotkeyBinding::shift(
        HotkeyId::ShowHideArchived,
        KeyCode::Char('A'),
        &[HotkeyContext::Global],
    ),
    // === Home View Tab Navigation ===
    HotkeyBinding::new(
        HotkeyId::NextHomeTab,
        KeyCode::Tab,
        &[
            HotkeyContext::HomeConversations,
            HotkeyContext::HomeInbox,
            HotkeyContext::HomeReports,
            HotkeyContext::HomeActiveWork,
            HotkeyContext::HomeStats,
        ],
    ),
    HotkeyBinding::shift(
        HotkeyId::PrevHomeTab,
        KeyCode::BackTab,
        &[
            HotkeyContext::HomeConversations,
            HotkeyContext::HomeInbox,
            HotkeyContext::HomeReports,
            HotkeyContext::HomeActiveWork,
            HotkeyContext::HomeStats,
        ],
    ),
    HotkeyBinding::new(
        HotkeyId::SearchReports,
        KeyCode::Char('/'),
        &[HotkeyContext::HomeReports],
    ),
    HotkeyBinding::shift(
        HotkeyId::ToggleHideScheduled,
        KeyCode::Char('S'),
        &[
            HotkeyContext::HomeConversations,
            HotkeyContext::HomeInbox,
            HotkeyContext::HomeActiveWork,
        ],
    ),
    HotkeyBinding::new(
        HotkeyId::FocusSidebar,
        KeyCode::Right,
        &[
            HotkeyContext::HomeConversations,
            HotkeyContext::HomeInbox,
            HotkeyContext::HomeReports,
            HotkeyContext::HomeActiveWork,
        ],
    ),
    HotkeyBinding::new(
        HotkeyId::UnfocusSidebar,
        KeyCode::Left,
        &[HotkeyContext::HomeSidebar],
    ),
    // === Home View - Inbox Tab ===
    HotkeyBinding::shift(
        HotkeyId::MarkAsRead,
        KeyCode::Char('R'),
        &[HotkeyContext::HomeInbox],
    ),
    HotkeyBinding::shift(
        HotkeyId::MarkAllRead,
        KeyCode::Char('M'),
        &[HotkeyContext::HomeInbox],
    ),
    // === Home View - Sidebar ===
    HotkeyBinding::new(
        HotkeyId::ToggleProjectVisibility,
        KeyCode::Char(' '),
        &[HotkeyContext::HomeSidebar],
    ),
    HotkeyBinding::new(
        HotkeyId::ProjectSettings,
        KeyCode::Char('s'),
        &[HotkeyContext::HomeSidebar],
    ),
    HotkeyBinding::new(
        HotkeyId::BootProject,
        KeyCode::Char('b'),
        &[HotkeyContext::HomeSidebar],
    ),
    HotkeyBinding::new(
        HotkeyId::StopAllAgents,
        KeyCode::Char('.'),
        &[HotkeyContext::HomeSidebar],
    ),
    // === Chat View - Normal Mode ===
    HotkeyBinding::new(
        HotkeyId::MentionAgent,
        KeyCode::Char('@'),
        &[HotkeyContext::ChatNormal, HotkeyContext::ChatEditing],
    ),
    HotkeyBinding::new(
        HotkeyId::CopyMessage,
        KeyCode::Char('y'),
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::ViewRawEvent,
        KeyCode::Char('v'),
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::OpenTrace,
        KeyCode::Char('t'),
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::StopAgent,
        KeyCode::Char('.'),
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::GoToParent,
        KeyCode::Char('g'),
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::new(
        HotkeyId::EnterEditMode,
        KeyCode::Char('i'),
        &[HotkeyContext::ChatNormal],
    ),
    HotkeyBinding::ctrl(
        HotkeyId::InConversationSearch,
        KeyCode::Char('f'),
        &[HotkeyContext::ChatNormal],
    ),
    // === Chat View - Edit Mode ===
    HotkeyBinding::ctrl(
        HotkeyId::SendMessage,
        KeyCode::Enter,
        &[HotkeyContext::ChatEditing],
    )
    .with_priority(50),
    HotkeyBinding::ctrl(
        HotkeyId::ExpandEditor,
        KeyCode::Char('e'),
        &[HotkeyContext::ChatEditing],
    ),
    HotkeyBinding::shift(
        HotkeyId::InsertNewline,
        KeyCode::Enter,
        &[HotkeyContext::ChatEditing],
    ),
    HotkeyBinding::new(
        HotkeyId::CancelEdit,
        KeyCode::Esc,
        &[HotkeyContext::ChatEditing],
    )
    .with_priority(50),
    HotkeyBinding::ctrl(
        HotkeyId::HistorySearch,
        KeyCode::Char('r'),
        &[HotkeyContext::ChatEditing],
    ),
    // Unified [/] nudges/skills selector has two bindings: Ctrl+/ (primary) and Ctrl+N (alternative)
    // Note: Ctrl+_ is also handled in editor_handlers.rs for terminal compatibility
    // (some terminals report Ctrl+/ as Ctrl+_)
    HotkeyBinding::ctrl(
        HotkeyId::OpenNudgeSkillSelector,
        KeyCode::Char('/'),
        &[HotkeyContext::ChatEditing],
    ),
    HotkeyBinding::ctrl(
        HotkeyId::OpenNudgeSkillSelector,
        KeyCode::Char('n'),
        &[HotkeyContext::ChatEditing],
    ),
    HotkeyBinding::alt(
        HotkeyId::OpenNudgeSkillSelector,
        KeyCode::Char('k'),
        &[HotkeyContext::ChatEditing],
    ),
    // === Agent Browser ===
    HotkeyBinding::new(
        HotkeyId::ViewAgent,
        KeyCode::Char('o'),
        &[HotkeyContext::AgentBrowserList],
    ),
    HotkeyBinding::new(
        HotkeyId::CreateAgent,
        KeyCode::Char('n'),
        &[HotkeyContext::AgentBrowserList],
    ),
    HotkeyBinding::new(
        HotkeyId::ForkAgent,
        KeyCode::Char('f'),
        &[HotkeyContext::AgentBrowserDetail],
    ),
    HotkeyBinding::new(
        HotkeyId::CloneAgent,
        KeyCode::Char('c'),
        &[HotkeyContext::AgentBrowserDetail],
    ),
    // === Report Viewer ===
    HotkeyBinding::new(
        HotkeyId::ToggleReportView,
        KeyCode::Tab,
        &[HotkeyContext::ReportViewerModal],
    ),
    HotkeyBinding::new(
        HotkeyId::CopyReportId,
        KeyCode::Char('c'),
        &[HotkeyContext::ReportViewerModal],
    ),
    // === Modal Actions ===
    HotkeyBinding::new(
        HotkeyId::ModalClose,
        KeyCode::Esc,
        &[HotkeyContext::AnyModal],
    )
    .with_priority(10),
    HotkeyBinding::new(
        HotkeyId::ModalConfirm,
        KeyCode::Enter,
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
    pub fn resolve(
        &self,
        key: KeyCode,
        modifiers: KeyModifiers,
        context: HotkeyContext,
    ) -> Option<HotkeyId> {
        for binding in &self.bindings {
            if binding.matches(key, modifiers) && binding.is_active_in(context) {
                return Some(binding.id);
            }
        }
        None
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
pub fn resolve_hotkey(
    key: KeyCode,
    modifiers: KeyModifiers,
    context: HotkeyContext,
) -> Option<HotkeyId> {
    resolver().resolve(key, modifiers, context)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hotkey_matching() {
        let binding = HotkeyBinding::ctrl(
            HotkeyId::CommandPalette,
            KeyCode::Char('t'),
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
            &[HotkeyContext::HomeConversations, HotkeyContext::HomeInbox],
        );

        assert!(binding.is_active_in(HotkeyContext::HomeConversations));
        assert!(binding.is_active_in(HotkeyContext::HomeInbox));
        assert!(!binding.is_active_in(HotkeyContext::ChatNormal));
    }

    #[test]
    fn test_global_context() {
        let binding =
            HotkeyBinding::new(HotkeyId::Quit, KeyCode::Char('q'), &[HotkeyContext::Global]);

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
}
