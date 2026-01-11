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

/// State for nudge selector modal (multi-select nudges for messages)
#[derive(Debug, Clone)]
pub struct NudgeSelectorState {
    pub selector: SelectorState,
    pub selected_nudge_ids: Vec<String>,  // Multi-select
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

/// Step in the create project wizard
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateProjectStep {
    Details,      // name + description
    SelectAgents, // agent picker
}

/// Which field is focused in the details step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateProjectFocus {
    Name,
    Description,
}

/// State for the create project modal
#[derive(Debug, Clone)]
pub struct CreateProjectState {
    pub step: CreateProjectStep,
    pub focus: CreateProjectFocus,
    pub name: String,
    pub description: String,
    pub agent_ids: Vec<String>,
    pub agent_selector: SelectorState,
}

impl CreateProjectState {
    pub fn new() -> Self {
        Self {
            step: CreateProjectStep::Details,
            focus: CreateProjectFocus::Name,
            name: String::new(),
            description: String::new(),
            agent_ids: Vec::new(),
            agent_selector: SelectorState::default(),
        }
    }

    pub fn can_proceed(&self) -> bool {
        match self.step {
            CreateProjectStep::Details => !self.name.trim().is_empty(),
            CreateProjectStep::SelectAgents => true, // Can always finish from agent selection
        }
    }

    pub fn toggle_agent(&mut self, agent_id: String) {
        if let Some(pos) = self.agent_ids.iter().position(|id| id == &agent_id) {
            self.agent_ids.remove(pos);
        } else {
            self.agent_ids.push(agent_id);
        }
    }
}

/// Mode for creating an agent definition
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCreateMode {
    New,
    Fork,  // increment version, add e-tag reference
    Clone, // new identity, add cloned-from tag
}

/// Step in the create agent wizard
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentCreateStep {
    Basics,       // name, description, role
    Instructions, // system prompt (multi-line)
    Review,       // preview before publish
}

/// Which field is focused in the basics step
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentFormFocus {
    Name,
    Description,
    Role,
}

/// State for the create/fork/clone agent modal
#[derive(Debug, Clone)]
pub struct CreateAgentState {
    pub mode: AgentCreateMode,
    pub step: AgentCreateStep,
    pub focus: AgentFormFocus,
    pub name: String,
    pub description: String,
    pub role: String,
    pub instructions: String,
    pub version: String,
    /// Source event ID (for fork/clone)
    pub source_id: Option<String>,
    /// Scroll offset for instructions view
    pub instructions_scroll: usize,
    /// Cursor position in instructions
    pub instructions_cursor: usize,
}

impl CreateAgentState {
    pub fn new() -> Self {
        Self {
            mode: AgentCreateMode::New,
            step: AgentCreateStep::Basics,
            focus: AgentFormFocus::Name,
            name: String::new(),
            description: String::new(),
            role: "assistant".to_string(),
            instructions: String::new(),
            version: "1".to_string(),
            source_id: None,
            instructions_scroll: 0,
            instructions_cursor: 0,
        }
    }

    pub fn fork_from(agent: &tenex_core::models::AgentDefinition) -> Self {
        // Increment version
        let version = agent.version.as_ref()
            .and_then(|v| v.parse::<u32>().ok())
            .map(|v| (v + 1).to_string())
            .unwrap_or_else(|| "2".to_string());

        Self {
            mode: AgentCreateMode::Fork,
            step: AgentCreateStep::Basics,
            focus: AgentFormFocus::Name,
            name: agent.name.clone(),
            description: agent.description.clone(),
            role: agent.role.clone(),
            instructions: agent.instructions.clone(),
            version,
            source_id: Some(agent.id.clone()),
            instructions_scroll: 0,
            instructions_cursor: agent.instructions.len(),
        }
    }

    pub fn clone_from(agent: &tenex_core::models::AgentDefinition) -> Self {
        Self {
            mode: AgentCreateMode::Clone,
            step: AgentCreateStep::Basics,
            focus: AgentFormFocus::Name,
            name: format!("{} (Copy)", agent.name),
            description: agent.description.clone(),
            role: agent.role.clone(),
            instructions: agent.instructions.clone(),
            version: "1".to_string(),
            source_id: Some(agent.id.clone()),
            instructions_scroll: 0,
            instructions_cursor: agent.instructions.len(),
        }
    }

    pub fn can_proceed(&self) -> bool {
        match self.step {
            AgentCreateStep::Basics => {
                !self.name.trim().is_empty() && !self.description.trim().is_empty()
            }
            AgentCreateStep::Instructions => true,
            AgentCreateStep::Review => true,
        }
    }

    pub fn mode_label(&self) -> &'static str {
        match self.mode {
            AgentCreateMode::New => "New Agent",
            AgentCreateMode::Fork => "Fork Agent",
            AgentCreateMode::Clone => "Clone Agent",
        }
    }
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

/// Conversation action types (for Home view)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConversationAction {
    Open,
    ExportJsonl,
    ToggleArchive,
}

impl ConversationAction {
    pub const ALL: [ConversationAction; 3] = [
        ConversationAction::Open,
        ConversationAction::ExportJsonl,
        ConversationAction::ToggleArchive,
    ];

    pub fn label(&self, is_archived: bool) -> &'static str {
        match self {
            ConversationAction::Open => "Open Conversation",
            ConversationAction::ExportJsonl => "Export as JSONL",
            ConversationAction::ToggleArchive => {
                if is_archived { "Unarchive" } else { "Archive" }
            }
        }
    }

    pub fn hotkey(&self) -> char {
        match self {
            ConversationAction::Open => 'o',
            ConversationAction::ExportJsonl => 'e',
            ConversationAction::ToggleArchive => 'a',
        }
    }
}

/// State for conversation actions modal
#[derive(Debug, Clone)]
pub struct ConversationActionsState {
    pub thread_id: String,
    pub thread_title: String,
    pub project_a_tag: String,
    pub is_archived: bool,
    pub selected_index: usize,
}

impl ConversationActionsState {
    pub fn new(thread_id: String, thread_title: String, project_a_tag: String, is_archived: bool) -> Self {
        Self {
            thread_id,
            thread_title,
            project_a_tag,
            is_archived,
            selected_index: 0,
        }
    }

    pub fn selected_action(&self) -> ConversationAction {
        ConversationAction::ALL[self.selected_index]
    }
}

/// Chat action types (for Chat view input - Ctrl+T /)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAction {
    GoToParent,
    ExportJsonl,
}

impl ChatAction {
    /// Get available actions based on whether this conversation has a parent
    pub fn available(has_parent: bool) -> Vec<ChatAction> {
        if has_parent {
            vec![ChatAction::GoToParent, ChatAction::ExportJsonl]
        } else {
            vec![ChatAction::ExportJsonl]
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ChatAction::GoToParent => "Go to Parent Conversation",
            ChatAction::ExportJsonl => "Copy All Events as JSONL",
        }
    }

    pub fn hotkey(&self) -> char {
        match self {
            ChatAction::GoToParent => 'p',
            ChatAction::ExportJsonl => 'e',
        }
    }
}

/// State for chat actions modal (opened from chat input with Ctrl+T /)
#[derive(Debug, Clone)]
pub struct ChatActionsState {
    pub thread_id: String,
    pub thread_title: String,
    pub project_a_tag: String,
    pub parent_conversation_id: Option<String>,
    pub selected_index: usize,
}

impl ChatActionsState {
    pub fn new(
        thread_id: String,
        thread_title: String,
        project_a_tag: String,
        parent_conversation_id: Option<String>,
    ) -> Self {
        Self {
            thread_id,
            thread_title,
            project_a_tag,
            parent_conversation_id,
            selected_index: 0,
        }
    }

    pub fn has_parent(&self) -> bool {
        self.parent_conversation_id.is_some()
    }

    pub fn available_actions(&self) -> Vec<ChatAction> {
        ChatAction::available(self.has_parent())
    }

    pub fn selected_action(&self) -> Option<ChatAction> {
        self.available_actions().get(self.selected_index).copied()
    }
}

/// Message action types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageAction {
    CopyRawEvent,
    SendAgain,
    ViewRawEvent,
    OpenTrace,
}

impl MessageAction {
    pub const ALL: [MessageAction; 4] = [
        MessageAction::CopyRawEvent,
        MessageAction::SendAgain,
        MessageAction::ViewRawEvent,
        MessageAction::OpenTrace,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            MessageAction::CopyRawEvent => "Copy Raw Event",
            MessageAction::SendAgain => "Send Again (New Conversation)",
            MessageAction::ViewRawEvent => "View Raw Event",
            MessageAction::OpenTrace => "Open Trace in Jaeger",
        }
    }

    pub fn hotkey(&self) -> char {
        match self {
            MessageAction::CopyRawEvent => 'c',
            MessageAction::SendAgain => 's',
            MessageAction::ViewRawEvent => 'v',
            MessageAction::OpenTrace => 't',
        }
    }
}

/// Project action types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectAction {
    Boot,
    Settings,
}

impl ProjectAction {
    pub fn label(&self) -> &'static str {
        match self {
            ProjectAction::Boot => "Boot Project",
            ProjectAction::Settings => "Settings",
        }
    }

    pub fn hotkey(&self) -> char {
        match self {
            ProjectAction::Boot => 'b',
            ProjectAction::Settings => 's',
        }
    }
}

/// State for project actions modal
#[derive(Debug, Clone)]
pub struct ProjectActionsState {
    pub project_a_tag: String,
    pub project_name: String,
    pub project_pubkey: String,
    pub is_online: bool,
    pub selected_index: usize,
}

impl ProjectActionsState {
    pub fn new(project_a_tag: String, project_name: String, project_pubkey: String, is_online: bool) -> Self {
        Self {
            project_a_tag,
            project_name,
            project_pubkey,
            is_online,
            selected_index: 0,
        }
    }

    pub fn available_actions(&self) -> Vec<ProjectAction> {
        if self.is_online {
            vec![ProjectAction::Settings]
        } else {
            vec![ProjectAction::Boot, ProjectAction::Settings]
        }
    }

    pub fn selected_action(&self) -> Option<ProjectAction> {
        self.available_actions().get(self.selected_index).copied()
    }
}

/// Focus area in report viewer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportViewerFocus {
    Content,
    Threads,
}

/// View mode in report viewer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportViewMode {
    Current,
    Changes,
}

/// State for the report viewer modal
#[derive(Debug, Clone)]
pub struct ReportViewerState {
    pub report: tenex_core::models::Report,
    pub focus: ReportViewerFocus,
    pub view_mode: ReportViewMode,
    pub content_scroll: usize,
    pub threads_scroll: usize,
    pub selected_thread_index: usize,
    pub version_index: usize,
    pub show_threads: bool,
    pub show_copy_menu: bool,
    pub copy_menu_index: usize,
}

impl ReportViewerState {
    pub fn new(report: tenex_core::models::Report) -> Self {
        Self {
            report,
            focus: ReportViewerFocus::Content,
            view_mode: ReportViewMode::Current,
            content_scroll: 0,
            threads_scroll: 0,
            selected_thread_index: 0,
            version_index: 0,
            show_threads: false,
            show_copy_menu: false,
            copy_menu_index: 0,
        }
    }
}

/// Copy menu options for report viewer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportCopyOption {
    Bech32Id,
    RawEvent,
    Markdown,
}

impl ReportCopyOption {
    pub const ALL: [ReportCopyOption; 3] = [
        ReportCopyOption::Bech32Id,
        ReportCopyOption::RawEvent,
        ReportCopyOption::Markdown,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            ReportCopyOption::Bech32Id => "Copy Event ID (bech32)",
            ReportCopyOption::RawEvent => "Copy Raw Event (JSON)",
            ReportCopyOption::Markdown => "Copy Markdown Content",
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
    /// Create new project wizard
    CreateProject(CreateProjectState),
    /// Nudge selector for adding nudges to messages
    NudgeSelector(NudgeSelectorState),
    /// Message action menu (/) - shows available actions for selected message
    MessageActions {
        message_id: String,
        selected_index: usize,
        has_trace: bool,
    },
    /// Conversation action menu (/) in Home view - shows actions for selected conversation
    ConversationActions(ConversationActionsState),
    /// Chat action menu (Ctrl+T /) in Chat view - shows actions for current conversation
    ChatActions(ChatActionsState),
    /// View raw event JSON in a scrollable modal
    ViewRawEvent {
        message_id: String,
        json: String,
        scroll_offset: usize,
    },
    /// Hotkey help modal (Ctrl+T+?)
    HotkeyHelp,
    /// Create/fork/clone agent definition wizard
    CreateAgent(CreateAgentState),
    /// Project actions modal (boot, settings)
    ProjectActions(ProjectActionsState),
    /// Report viewer modal with document, versions, and threads
    ReportViewer(ReportViewerState),
    /// Expanded editor modal for full-screen text editing (Ctrl+E)
    ExpandedEditor {
        editor: TextEditor,
    },
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
