use crate::models::AskEvent;
use crate::ui::ask_input::AskInputState;
use crate::ui::nudge::NudgeFormState;
use crate::ui::selector::SelectorState;
use crate::ui::text_editor::TextEditor;
use tenex_core::models::NamedDraft;

/// Settings tabs for the app settings modal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsTab {
    General,
    AI,
}

impl SettingsTab {
    pub const ALL: &'static [SettingsTab] = &[SettingsTab::General, SettingsTab::AI];

    pub fn label(&self) -> &'static str {
        match self {
            SettingsTab::General => "General",
            SettingsTab::AI => "AI",
        }
    }
}

/// Settings in the General tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GeneralSetting {
    JaegerEndpoint,
}

impl GeneralSetting {
    pub const ALL: &'static [GeneralSetting] = &[GeneralSetting::JaegerEndpoint];

    pub const fn count() -> usize {
        Self::ALL.len()
    }

    pub fn from_index(index: usize) -> Option<GeneralSetting> {
        Self::ALL.get(index).copied()
    }
}

/// Settings in the AI tab
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AiSetting {
    ElevenLabsApiKey,
    OpenRouterApiKey,
}

impl AiSetting {
    pub const ALL: &'static [AiSetting] = &[
        AiSetting::ElevenLabsApiKey,
        AiSetting::OpenRouterApiKey,
    ];

    pub const fn count() -> usize {
        Self::ALL.len()
    }

    pub fn from_index(index: usize) -> Option<AiSetting> {
        Self::ALL.get(index).copied()
    }
}

/// State for AI settings - only API keys for now
#[derive(Debug, Clone)]
pub struct AiSettingsState {
    /// Input for ElevenLabs API key (always masked in UI)
    pub elevenlabs_key_input: String,
    /// Input for OpenRouter API key (always masked in UI)
    pub openrouter_key_input: String,
    /// Cached: whether ElevenLabs key exists in secure storage (checked once on modal open)
    pub elevenlabs_key_exists: bool,
    /// Cached: whether OpenRouter key exists in secure storage (checked once on modal open)
    pub openrouter_key_exists: bool,
}

impl AiSettingsState {
    pub fn new() -> Self {
        // Check secure storage once when creating state (modal opens)
        let elevenlabs_key_exists =
            tenex_core::SecureStorage::exists(tenex_core::SecureKey::ElevenLabsApiKey);
        let openrouter_key_exists =
            tenex_core::SecureStorage::exists(tenex_core::SecureKey::OpenRouterApiKey);

        Self {
            elevenlabs_key_input: String::new(),
            openrouter_key_input: String::new(),
            elevenlabs_key_exists,
            openrouter_key_exists,
        }
    }

    pub fn has_elevenlabs_key(&self) -> bool {
        !self.elevenlabs_key_input.is_empty()
    }

    pub fn has_openrouter_key(&self) -> bool {
        !self.openrouter_key_input.is_empty()
    }
}

/// State for app settings modal (global settings accessible via comma key)
#[derive(Debug, Clone)]
pub struct AppSettingsState {
    /// Currently active tab
    pub current_tab: SettingsTab,
    /// Selected setting index in General tab
    pub general_index: usize,
    /// Selected setting index in AI tab
    pub ai_index: usize,
    /// Whether a field is currently being edited
    pub editing: bool,
    /// The current value being edited for jaeger endpoint
    pub jaeger_endpoint_input: String,
    /// AI settings state
    pub ai: AiSettingsState,
}

impl AppSettingsState {
    pub fn new(current_jaeger_endpoint: &str) -> Self {
        Self {
            current_tab: SettingsTab::General,
            general_index: 0,
            ai_index: 0,
            editing: false,
            jaeger_endpoint_input: current_jaeger_endpoint.to_string(),
            ai: AiSettingsState::new(),
        }
    }

    /// Switch to next tab
    pub fn next_tab(&mut self) {
        let idx = SettingsTab::ALL
            .iter()
            .position(|&t| t == self.current_tab)
            .unwrap_or(0);
        self.current_tab = SettingsTab::ALL[(idx + 1) % SettingsTab::ALL.len()];
    }

    /// Switch to previous tab
    pub fn prev_tab(&mut self) {
        let idx = SettingsTab::ALL
            .iter()
            .position(|&t| t == self.current_tab)
            .unwrap_or(0);
        self.current_tab = SettingsTab::ALL[(idx + SettingsTab::ALL.len() - 1) % SettingsTab::ALL.len()];
    }

    /// Get selected setting in General tab
    pub fn selected_general_setting(&self) -> Option<GeneralSetting> {
        GeneralSetting::from_index(self.general_index)
    }

    /// Get selected setting in AI tab
    pub fn selected_ai_setting(&self) -> Option<AiSetting> {
        AiSetting::from_index(self.ai_index)
    }

    /// Check if jaeger endpoint is being edited
    pub fn editing_jaeger_endpoint(&self) -> bool {
        self.editing
            && self.current_tab == SettingsTab::General
            && self.selected_general_setting() == Some(GeneralSetting::JaegerEndpoint)
    }

    /// Check if ElevenLabs key is being edited
    pub fn editing_elevenlabs_key(&self) -> bool {
        self.editing
            && self.current_tab == SettingsTab::AI
            && self.selected_ai_setting() == Some(AiSetting::ElevenLabsApiKey)
    }

    /// Check if OpenRouter key is being edited
    pub fn editing_openrouter_key(&self) -> bool {
        self.editing
            && self.current_tab == SettingsTab::AI
            && self.selected_ai_setting() == Some(AiSetting::OpenRouterApiKey)
    }

    pub fn move_up(&mut self) {
        match self.current_tab {
            SettingsTab::General => {
                if self.general_index > 0 {
                    self.general_index -= 1;
                }
            }
            SettingsTab::AI => {
                if self.ai_index > 0 {
                    self.ai_index -= 1;
                }
            }
        }
    }

    pub fn move_down(&mut self) {
        match self.current_tab {
            SettingsTab::General => {
                if self.general_index + 1 < GeneralSetting::count() {
                    self.general_index += 1;
                }
            }
            SettingsTab::AI => {
                if self.ai_index + 1 < AiSetting::count() {
                    self.ai_index += 1;
                }
            }
        }
    }

    /// Start editing the currently selected setting
    pub fn start_editing(&mut self) {
        self.editing = true;
    }

    /// Stop editing
    pub fn stop_editing(&mut self) {
        self.editing = false;
    }
}

/// State for the ask modal (answering multi-question ask events)
#[derive(Debug, Clone)]
pub struct AskModalState {
    pub message_id: String,
    pub ask_event: AskEvent,
    pub input_state: AskInputState,
    /// Pubkey of the ask event author (for p-tagging in response)
    pub ask_author_pubkey: String,
}

/// State for nudge selector modal (multi-select nudges for messages)
#[derive(Debug, Clone)]
pub struct NudgeSelectorState {
    pub selector: SelectorState,
    pub selected_nudge_ids: Vec<String>,  // Multi-select
}

/// State for nudge list view (browse/manage nudges)
#[derive(Debug, Clone)]
pub struct NudgeListState {
    pub filter: String,
    pub selected_index: usize,
}

impl NudgeListState {
    pub fn new() -> Self {
        Self {
            filter: String::new(),
            selected_index: 0,
        }
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self, max: usize) {
        if self.selected_index + 1 < max {
            self.selected_index += 1;
        }
    }

    pub fn add_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected_index = 0;
    }

    pub fn backspace_filter(&mut self) {
        self.filter.pop();
        self.selected_index = 0;
    }
}

impl Default for NudgeListState {
    fn default() -> Self {
        Self::new()
    }
}

/// State for nudge detail view (read-only view)
#[derive(Debug, Clone)]
pub struct NudgeDetailState {
    pub nudge_id: String,
    pub scroll_offset: usize,
}

impl NudgeDetailState {
    pub fn new(nudge_id: String) -> Self {
        Self {
            nudge_id,
            scroll_offset: 0,
        }
    }

    pub fn scroll_up(&mut self) {
        if self.scroll_offset > 0 {
            self.scroll_offset -= 1;
        }
    }

    pub fn scroll_down(&mut self, max_scroll: usize) {
        if self.scroll_offset < max_scroll {
            self.scroll_offset += 1;
        }
    }
}

/// State for nudge delete confirmation
#[derive(Debug, Clone)]
pub struct NudgeDeleteConfirmState {
    pub nudge_id: String,
    pub selected_index: usize, // 0 = Cancel, 1 = Delete
}

impl NudgeDeleteConfirmState {
    pub fn new(nudge_id: String) -> Self {
        Self {
            nudge_id,
            selected_index: 0, // Default to Cancel
        }
    }

    pub fn toggle(&mut self) {
        self.selected_index = 1 - self.selected_index;
    }
}

/// What type of item is being added in project settings
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectSettingsAddMode {
    Agent,
    McpTool,
}

/// State for project settings modal
#[derive(Debug, Clone)]
pub struct ProjectSettingsState {
    pub project_a_tag: String,
    pub project_name: String,
    pub original_agent_ids: Vec<String>,
    pub pending_agent_ids: Vec<String>,
    pub original_mcp_tool_ids: Vec<String>,
    pub pending_mcp_tool_ids: Vec<String>,
    pub selector_index: usize,
    pub in_add_mode: Option<ProjectSettingsAddMode>,
    pub add_filter: String,
    pub add_index: usize,
}

/// Step in the create project wizard
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CreateProjectStep {
    Details,      // name + description
    SelectAgents, // agent picker
    SelectTools,  // MCP tool picker
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
    pub mcp_tool_ids: Vec<String>,
    pub tool_selector: SelectorState,
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
            mcp_tool_ids: Vec::new(),
            tool_selector: SelectorState::default(),
        }
    }

    pub fn can_proceed(&self) -> bool {
        match self.step {
            CreateProjectStep::Details => !self.name.trim().is_empty(),
            CreateProjectStep::SelectAgents => true,
            CreateProjectStep::SelectTools => true,
        }
    }

    pub fn toggle_agent(&mut self, agent_id: String) {
        if let Some(pos) = self.agent_ids.iter().position(|id| id == &agent_id) {
            self.agent_ids.remove(pos);
        } else {
            self.agent_ids.push(agent_id);
        }
    }

    pub fn toggle_mcp_tool(&mut self, tool_id: String) {
        if let Some(pos) = self.mcp_tool_ids.iter().position(|id| id == &tool_id) {
            self.mcp_tool_ids.remove(pos);
        } else {
            self.mcp_tool_ids.push(tool_id);
        }
    }

    pub fn all_mcp_tool_ids(&self, app: &crate::ui::app::App) -> Vec<String> {
        use std::collections::HashSet;

        let mut tool_ids = HashSet::new();

        // Add manually selected tools
        for id in &self.mcp_tool_ids {
            tool_ids.insert(id.clone());
        }

        // Add tools from selected agents
        for agent_id in &self.agent_ids {
            if let Some(agent) = app.get_agent_definition(agent_id) {
                for mcp_id in &agent.mcp_servers {
                    tool_ids.insert(mcp_id.clone());
                }
            }
        }

        tool_ids.into_iter().collect()
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
    pub fn new(project_a_tag: String, project_name: String, agent_ids: Vec<String>, mcp_tool_ids: Vec<String>) -> Self {
        Self {
            project_a_tag,
            project_name,
            original_agent_ids: agent_ids.clone(),
            pending_agent_ids: agent_ids,
            original_mcp_tool_ids: mcp_tool_ids.clone(),
            pending_mcp_tool_ids: mcp_tool_ids,
            selector_index: 0,
            in_add_mode: None,
            add_filter: String::new(),
            add_index: 0,
        }
    }

    pub fn has_changes(&self) -> bool {
        self.original_agent_ids != self.pending_agent_ids
            || self.original_mcp_tool_ids != self.pending_mcp_tool_ids
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

    pub fn add_mcp_tool(&mut self, tool_id: String) {
        if !self.pending_mcp_tool_ids.contains(&tool_id) {
            self.pending_mcp_tool_ids.push(tool_id);
        }
    }

    pub fn remove_mcp_tool(&mut self, index: usize) {
        if index < self.pending_mcp_tool_ids.len() {
            self.pending_mcp_tool_ids.remove(index);
            if self.selector_index >= self.pending_mcp_tool_ids.len() && self.selector_index > 0 {
                self.selector_index -= 1;
            }
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

/// Chat action types (for Chat view - accessible via Ctrl+T command palette)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChatAction {
    NewConversation,
    GoToParent,
    ExportJsonl,
}

impl ChatAction {
    /// Get available actions based on whether this conversation has a parent
    pub fn available(has_parent: bool) -> Vec<ChatAction> {
        if has_parent {
            vec![
                ChatAction::NewConversation,
                ChatAction::GoToParent,
                ChatAction::ExportJsonl,
            ]
        } else {
            vec![ChatAction::NewConversation, ChatAction::ExportJsonl]
        }
    }

    pub fn label(&self) -> &'static str {
        match self {
            ChatAction::NewConversation => "New Conversation (Same Context)",
            ChatAction::GoToParent => "Go to Parent Conversation",
            ChatAction::ExportJsonl => "Copy All Events as JSONL",
        }
    }

    pub fn hotkey(&self) -> char {
        match self {
            ChatAction::NewConversation => 'n',
            ChatAction::GoToParent => 'p',
            ChatAction::ExportJsonl => 'e',
        }
    }
}

/// State for chat actions modal (accessible via Ctrl+T command palette)
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

/// Project action types
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectAction {
    NewConversation,
    Boot,
    Settings,
    ToggleArchive,
}

impl ProjectAction {
    pub fn label(&self, is_archived: bool) -> &'static str {
        match self {
            ProjectAction::NewConversation => "New Conversation",
            ProjectAction::Boot => "Boot Project",
            ProjectAction::Settings => "Settings",
            ProjectAction::ToggleArchive => {
                if is_archived { "Unarchive" } else { "Archive" }
            }
        }
    }

    pub fn hotkey(&self) -> char {
        match self {
            ProjectAction::NewConversation => 'n',
            ProjectAction::Boot => 'b',
            ProjectAction::Settings => 's',
            ProjectAction::ToggleArchive => 'a',
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
    pub is_archived: bool,
    pub selected_index: usize,
}

impl ProjectActionsState {
    pub fn new(project_a_tag: String, project_name: String, project_pubkey: String, is_online: bool, is_archived: bool) -> Self {
        Self {
            project_a_tag,
            project_name,
            project_pubkey,
            is_online,
            is_archived,
            selected_index: 0,
        }
    }

    pub fn available_actions(&self) -> Vec<ProjectAction> {
        let mut actions = if self.is_online {
            vec![ProjectAction::NewConversation, ProjectAction::Settings]
        } else {
            vec![ProjectAction::Boot, ProjectAction::Settings]
        };
        actions.push(ProjectAction::ToggleArchive);
        actions
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

/// Focus area in agent settings modal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AgentSettingsFocus {
    Model,
    Tools,
}

/// A group of related tools (MCP server or common prefix)
#[derive(Debug, Clone)]
pub struct ToolGroup {
    pub name: String,
    pub tools: Vec<String>,
    pub expanded: bool,
}

impl ToolGroup {
    /// Check if all tools in the group are selected
    pub fn is_fully_selected(&self, selected: &std::collections::HashSet<String>) -> bool {
        self.tools.iter().all(|t| selected.contains(t))
    }

    /// Check if some (but not all) tools in the group are selected
    pub fn is_partially_selected(&self, selected: &std::collections::HashSet<String>) -> bool {
        let count = self.tools.iter().filter(|t| selected.contains(*t)).count();
        count > 0 && count < self.tools.len()
    }
}

/// State for the agent settings modal
#[derive(Debug, Clone)]
pub struct AgentSettingsState {
    pub agent_name: String,
    pub agent_pubkey: String,
    pub project_a_tag: String,
    pub focus: AgentSettingsFocus,
    /// Available models to choose from (from project status)
    pub available_models: Vec<String>,
    /// Index of selected model in available_models
    pub model_index: usize,
    /// Tool groups (grouped by MCP server or common prefix)
    pub tool_groups: Vec<ToolGroup>,
    /// Selected tools by name
    pub selected_tools: std::collections::HashSet<String>,
    /// Current cursor position in flat list (groups + expanded tools)
    pub tools_cursor: usize,
    /// Scroll offset for tools list
    pub tools_scroll: usize,
}

impl AgentSettingsState {
    pub fn new(
        agent_name: String,
        agent_pubkey: String,
        project_a_tag: String,
        current_model: Option<String>,
        current_tools: Vec<String>,
        available_models: Vec<String>,
        all_available_tools: Vec<String>,
    ) -> Self {
        // Find index of current model (default to 0 if not found)
        let model_index = current_model
            .as_ref()
            .and_then(|m| available_models.iter().position(|am| am == m))
            .unwrap_or(0);

        // Build tool groups using intelligent grouping (like Svelte)
        let tool_groups = Self::build_tool_groups(all_available_tools);

        // Build selected tools set from current tools
        let selected_tools: std::collections::HashSet<String> = current_tools.into_iter().collect();

        Self {
            agent_name,
            agent_pubkey,
            project_a_tag,
            focus: AgentSettingsFocus::Model,
            available_models,
            model_index,
            tool_groups,
            selected_tools,
            tools_cursor: 0,
            tools_scroll: 0,
        }
    }

    /// Build tool groups from a flat list of tools
    /// Groups by: MCP server (mcp__<server>__<method>) or common prefix
    fn build_tool_groups(tools: Vec<String>) -> Vec<ToolGroup> {
        use std::collections::HashMap;

        let mut groups: HashMap<String, Vec<String>> = HashMap::new();
        let mut ungrouped: Vec<String> = Vec::new();

        for tool in &tools {
            // MCP tools: mcp__<server>__<method>
            if tool.starts_with("mcp__") {
                let parts: Vec<&str> = tool.split("__").collect();
                if parts.len() >= 3 {
                    let group_key = format!("MCP: {}", parts[1]);
                    groups.entry(group_key).or_default().push(tool.clone());
                    continue;
                }
            }

            // Find common prefixes (underscore-separated)
            if let Some(prefix_match) = tool.find('_') {
                let prefix = &tool[..prefix_match];
                // Only group if there are at least 2 tools with this prefix
                let similar_count = tools.iter().filter(|t| t.starts_with(&format!("{}_", prefix))).count();
                if similar_count >= 2 {
                    let group_key = prefix.to_uppercase();
                    groups.entry(group_key).or_default().push(tool.clone());
                    continue;
                }
            }

            // No group found - add to ungrouped
            ungrouped.push(tool.clone());
        }

        // Convert to Vec<ToolGroup>
        let mut result: Vec<ToolGroup> = Vec::new();

        // Add grouped tools
        for (name, mut tools) in groups {
            tools.sort();
            tools.dedup();
            result.push(ToolGroup {
                name,
                tools,
                expanded: false,
            });
        }

        // Add ungrouped tools as single-item groups
        for tool in ungrouped {
            result.push(ToolGroup {
                name: tool.clone(),
                tools: vec![tool],
                expanded: false,
            });
        }

        // Sort groups by name
        result.sort_by(|a, b| a.name.cmp(&b.name));
        result
    }

    pub fn selected_model(&self) -> Option<&str> {
        self.available_models.get(self.model_index).map(|s| s.as_str())
    }

    pub fn selected_tools_vec(&self) -> Vec<String> {
        self.selected_tools.iter().cloned().collect()
    }

    /// Get total number of visible items (groups + expanded tools)
    pub fn visible_item_count(&self) -> usize {
        let mut count = 0;
        for group in &self.tool_groups {
            count += 1; // Group header
            if group.expanded {
                count += group.tools.len();
            }
        }
        count
    }

    /// Get the item at a given cursor position
    /// Returns (group_index, Some(tool_index)) if cursor is on a tool,
    /// or (group_index, None) if cursor is on a group header
    pub fn item_at_cursor(&self, cursor: usize) -> Option<(usize, Option<usize>)> {
        let mut pos = 0;
        for (group_idx, group) in self.tool_groups.iter().enumerate() {
            if pos == cursor {
                return Some((group_idx, None));
            }
            pos += 1;
            if group.expanded {
                for tool_idx in 0..group.tools.len() {
                    if pos == cursor {
                        return Some((group_idx, Some(tool_idx)));
                    }
                    pos += 1;
                }
            }
        }
        None
    }

    /// Toggle expansion of group at cursor, or toggle tool selection
    pub fn toggle_at_cursor(&mut self) {
        if let Some((group_idx, tool_idx)) = self.item_at_cursor(self.tools_cursor) {
            match tool_idx {
                None => {
                    // On group header - toggle expansion if multi-tool group,
                    // otherwise toggle the single tool
                    let group = &self.tool_groups[group_idx];
                    if group.tools.len() == 1 {
                        // Single tool group - toggle the tool
                        let tool = &group.tools[0];
                        if self.selected_tools.contains(tool) {
                            self.selected_tools.remove(tool);
                        } else {
                            self.selected_tools.insert(tool.clone());
                        }
                    } else {
                        // Multi-tool group - toggle expansion
                        self.tool_groups[group_idx].expanded = !self.tool_groups[group_idx].expanded;
                    }
                }
                Some(tool_idx) => {
                    // On a tool - toggle its selection
                    let tool = &self.tool_groups[group_idx].tools[tool_idx];
                    if self.selected_tools.contains(tool) {
                        self.selected_tools.remove(tool);
                    } else {
                        self.selected_tools.insert(tool.clone());
                    }
                }
            }
        }
    }

    /// Toggle all tools in the group at cursor (bulk toggle)
    pub fn toggle_group_all(&mut self) {
        if let Some((group_idx, _)) = self.item_at_cursor(self.tools_cursor) {
            let group = &self.tool_groups[group_idx];
            let is_fully_selected = group.is_fully_selected(&self.selected_tools);

            if is_fully_selected {
                // Deselect all tools in group
                for tool in &group.tools {
                    self.selected_tools.remove(tool);
                }
            } else {
                // Select all tools in group
                for tool in &group.tools {
                    self.selected_tools.insert(tool.clone());
                }
            }
        }
    }

    pub fn move_cursor_up(&mut self) {
        if self.tools_cursor > 0 {
            self.tools_cursor -= 1;
            // Scroll up if cursor moves above visible area
            if self.tools_cursor < self.tools_scroll {
                self.tools_scroll = self.tools_cursor;
            }
        }
    }

    pub fn move_cursor_down(&mut self) {
        let max = self.visible_item_count();
        if self.tools_cursor + 1 < max {
            self.tools_cursor += 1;
        }
        // Note: scroll adjustment for moving down is handled via adjust_tools_scroll in render
    }

    /// Adjust scroll offset to keep the cursor visible within the given visible height.
    /// Call this during render when you know the actual visible height.
    pub fn adjust_tools_scroll(&mut self, visible_height: usize) {
        if visible_height == 0 {
            return;
        }
        // If cursor is above the visible window, scroll up
        if self.tools_cursor < self.tools_scroll {
            self.tools_scroll = self.tools_cursor;
        }
        // If cursor is below the visible window, scroll down
        else if self.tools_cursor >= self.tools_scroll + visible_height {
            self.tools_scroll = self.tools_cursor.saturating_sub(visible_height - 1);
        }
    }
}

/// A history search result entry
#[derive(Debug, Clone)]
pub struct HistorySearchEntry {
    /// Event ID of the message
    pub event_id: String,
    /// Message content
    pub content: String,
    /// Created at timestamp
    pub created_at: u64,
    /// Project a-tag (if available)
    pub project_a_tag: Option<String>,
}

/// State for the history search modal (Ctrl+R to search previous messages)
#[derive(Debug, Clone)]
pub struct HistorySearchState {
    /// Search query text
    pub query: String,
    /// Selected index in results
    pub selected_index: usize,
    /// Whether to search across all projects (false = current project only)
    pub all_projects: bool,
    /// Current project a-tag (for filtering)
    pub current_project_a_tag: Option<String>,
    /// Cached search results
    pub results: Vec<HistorySearchEntry>,
}

impl HistorySearchState {
    pub fn new(current_project_a_tag: Option<String>) -> Self {
        Self {
            query: String::new(),
            selected_index: 0,
            all_projects: false,
            current_project_a_tag,
            results: Vec::new(),
        }
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self) {
        if !self.results.is_empty() && self.selected_index + 1 < self.results.len() {
            self.selected_index += 1;
        }
    }

    pub fn selected_entry(&self) -> Option<&HistorySearchEntry> {
        self.results.get(self.selected_index)
    }

    pub fn toggle_all_projects(&mut self) {
        self.all_projects = !self.all_projects;
    }

    pub fn add_char(&mut self, c: char) {
        self.query.push(c);
        self.selected_index = 0;
    }

    pub fn backspace(&mut self) {
        self.query.pop();
        self.selected_index = 0;
    }
}

/// State for the draft navigator modal (shows saved named drafts)
#[derive(Debug, Clone)]
pub struct DraftNavigatorState {
    /// Selected index in the draft list
    pub selected_index: usize,
    /// Filter text for fuzzy searching drafts
    pub filter: String,
    /// Cached list of drafts (cloned for display)
    pub drafts: Vec<NamedDraft>,
}

impl DraftNavigatorState {
    pub fn new(drafts: Vec<NamedDraft>) -> Self {
        Self {
            selected_index: 0,
            filter: String::new(),
            drafts,
        }
    }

    /// Get filtered drafts based on current filter
    pub fn filtered_drafts(&self) -> Vec<&NamedDraft> {
        if self.filter.is_empty() {
            self.drafts.iter().collect()
        } else {
            let filter_lower = self.filter.to_lowercase();
            self.drafts
                .iter()
                .filter(|d| {
                    d.name.to_lowercase().contains(&filter_lower)
                        || d.text.to_lowercase().contains(&filter_lower)
                })
                .collect()
        }
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self) {
        let max = self.filtered_drafts().len();
        if self.selected_index + 1 < max {
            self.selected_index += 1;
        }
    }

    pub fn selected_draft(&self) -> Option<&NamedDraft> {
        self.filtered_drafts().get(self.selected_index).copied()
    }

    pub fn add_filter_char(&mut self, c: char) {
        self.filter.push(c);
        self.selected_index = 0;
    }

    pub fn backspace_filter(&mut self) {
        self.filter.pop();
        self.selected_index = 0;
    }
}

/// Mode for the workspace manager modal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceMode {
    List,    // Browse/switch workspaces
    Create,  // Creating new workspace
    Edit,    // Editing existing workspace
    Delete,  // Confirming deletion
}

/// Focus within workspace create/edit mode
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkspaceFocus {
    Name,
    Projects,
}

/// State for the workspace manager modal
#[derive(Debug, Clone)]
pub struct WorkspaceManagerState {
    pub mode: WorkspaceMode,
    pub selected_index: usize,
    pub filter: String,
    /// For create/edit mode - workspace name being edited
    pub editing_name: String,
    /// For create/edit mode - selected project a-tags
    pub editing_project_ids: std::collections::HashSet<String>,
    /// For edit mode - ID of workspace being edited
    pub editing_workspace_id: Option<String>,
    /// Index in project selector list
    pub project_selector_index: usize,
    /// Which field is focused in create/edit mode
    pub focus: WorkspaceFocus,
}

impl WorkspaceManagerState {
    pub fn new() -> Self {
        Self {
            mode: WorkspaceMode::List,
            selected_index: 0,
            filter: String::new(),
            editing_name: String::new(),
            editing_project_ids: std::collections::HashSet::new(),
            editing_workspace_id: None,
            project_selector_index: 0,
            focus: WorkspaceFocus::Name,
        }
    }

    pub fn enter_create_mode(&mut self) {
        self.mode = WorkspaceMode::Create;
        self.editing_name.clear();
        self.editing_project_ids.clear();
        self.editing_workspace_id = None;
        self.project_selector_index = 0;
        self.focus = WorkspaceFocus::Name;
    }

    pub fn enter_edit_mode(&mut self, workspace: &tenex_core::models::Workspace) {
        self.mode = WorkspaceMode::Edit;
        self.editing_name = workspace.name.clone();
        self.editing_project_ids = workspace.project_ids.iter().cloned().collect();
        self.editing_workspace_id = Some(workspace.id.clone());
        self.project_selector_index = 0;
        self.focus = WorkspaceFocus::Name;
    }

    pub fn enter_delete_mode(&mut self) {
        self.mode = WorkspaceMode::Delete;
    }

    pub fn back_to_list(&mut self) {
        self.mode = WorkspaceMode::List;
        self.editing_name.clear();
        self.editing_project_ids.clear();
        self.editing_workspace_id = None;
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self, max: usize) {
        if self.selected_index + 1 < max {
            self.selected_index += 1;
        }
    }

    pub fn toggle_project(&mut self, a_tag: &str) {
        if self.editing_project_ids.contains(a_tag) {
            self.editing_project_ids.remove(a_tag);
        } else {
            self.editing_project_ids.insert(a_tag.to_string());
        }
    }

    pub fn can_save(&self) -> bool {
        !self.editing_name.trim().is_empty()
    }
}

impl Default for WorkspaceManagerState {
    fn default() -> Self {
        Self::new()
    }
}

/// State for command palette modal (Ctrl+T)
/// Commands are defined in input/commands.rs
#[derive(Debug, Clone)]
pub struct CommandPaletteState {
    pub selected_index: usize,
}

/// Actions for backend approval modal
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BackendApprovalAction {
    Approve,
    Reject,
    Block,
}

impl BackendApprovalAction {
    pub const ALL: [BackendApprovalAction; 3] = [
        BackendApprovalAction::Approve,
        BackendApprovalAction::Reject,
        BackendApprovalAction::Block,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            BackendApprovalAction::Approve => "Approve Backend",
            BackendApprovalAction::Reject => "Reject (Ask Later)",
            BackendApprovalAction::Block => "Block Backend",
        }
    }

    pub fn description(&self) -> &'static str {
        match self {
            BackendApprovalAction::Approve => "Trust this backend and show status updates",
            BackendApprovalAction::Reject => "Dismiss for now, ask again later",
            BackendApprovalAction::Block => "Never show events from this backend",
        }
    }

    pub fn hotkey(&self) -> char {
        match self {
            BackendApprovalAction::Approve => 'a',
            BackendApprovalAction::Reject => 'r',
            BackendApprovalAction::Block => 'b',
        }
    }
}

/// State for backend approval modal
#[derive(Debug, Clone)]
pub struct BackendApprovalState {
    pub backend_pubkey: String,
    pub project_a_tag: String,
    pub selected_index: usize,
}

impl BackendApprovalState {
    pub fn new(backend_pubkey: String, project_a_tag: String) -> Self {
        Self {
            backend_pubkey,
            project_a_tag,
            selected_index: 0,
        }
    }

    pub fn selected_action(&self) -> BackendApprovalAction {
        BackendApprovalAction::ALL[self.selected_index]
    }
}

impl CommandPaletteState {
    pub fn new() -> Self {
        Self { selected_index: 0 }
    }

    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    pub fn move_down(&mut self, max: usize) {
        if self.selected_index + 1 < max {
            self.selected_index += 1;
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

/// Tab options for the debug stats modal
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum DebugStatsTab {
    #[default]
    Events,
    Subscriptions,
    Negentropy,
    ETagQuery,
    DataStore,
    EventFeed,
}

impl DebugStatsTab {
    pub const ALL: [DebugStatsTab; 6] = [
        DebugStatsTab::Events,
        DebugStatsTab::Subscriptions,
        DebugStatsTab::Negentropy,
        DebugStatsTab::ETagQuery,
        DebugStatsTab::DataStore,
        DebugStatsTab::EventFeed,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            DebugStatsTab::Events => "Events",
            DebugStatsTab::Subscriptions => "Subscriptions",
            DebugStatsTab::Negentropy => "Negentropy",
            DebugStatsTab::ETagQuery => "E-Tag Query",
            DebugStatsTab::DataStore => "Data Store",
            DebugStatsTab::EventFeed => "Event Feed",
        }
    }

    pub fn index(&self) -> usize {
        match self {
            DebugStatsTab::Events => 0,
            DebugStatsTab::Subscriptions => 1,
            DebugStatsTab::Negentropy => 2,
            DebugStatsTab::ETagQuery => 3,
            DebugStatsTab::DataStore => 4,
            DebugStatsTab::EventFeed => 5,
        }
    }

    pub fn from_index(index: usize) -> Self {
        match index {
            0 => DebugStatsTab::Events,
            1 => DebugStatsTab::Subscriptions,
            2 => DebugStatsTab::Negentropy,
            3 => DebugStatsTab::ETagQuery,
            4 => DebugStatsTab::DataStore,
            5 => DebugStatsTab::EventFeed,
            _ => DebugStatsTab::Events,
        }
    }

    pub fn next(&self) -> Self {
        Self::from_index((self.index() + 1) % Self::ALL.len())
    }

    pub fn prev(&self) -> Self {
        let len = Self::ALL.len();
        Self::from_index((self.index() + len - 1) % len)
    }
}

pub use tenex_core::stats::ETagQueryResult;

/// Debug stats modal state
#[derive(Debug, Clone, Default)]
pub struct DebugStatsState {
    pub active_tab: DebugStatsTab,
    pub scroll_offset: usize,
    /// Input text for e-tag query (event ID to search for)
    pub e_tag_query_input: String,
    /// Results of the last e-tag query
    pub e_tag_query_results: Vec<ETagQueryResult>,
    /// Whether the query input is focused
    pub e_tag_input_focused: bool,
    /// Selected result index for scrolling through results
    pub e_tag_selected_index: usize,
    /// For subscriptions tab: list of available project filters (None = "All", Some = project a-tag)
    pub sub_project_filters: Vec<Option<String>>,
    /// Currently selected project filter index in sidebar
    pub sub_selected_filter_index: usize,
    /// Whether the sidebar is focused (vs the subscription list)
    pub sub_sidebar_focused: bool,
    /// For event feed tab: selected event index
    pub event_feed_selected_index: usize,
}

impl DebugStatsState {
    pub fn new() -> Self {
        Self {
            active_tab: DebugStatsTab::Events,
            scroll_offset: 0,
            e_tag_query_input: String::new(),
            e_tag_query_results: Vec::new(),
            e_tag_input_focused: false,
            e_tag_selected_index: 0,
            sub_project_filters: vec![None], // Start with just "All"
            sub_selected_filter_index: 0,
            sub_sidebar_focused: true, // Start with sidebar focused
            event_feed_selected_index: 0,
        }
    }

    pub fn switch_tab(&mut self, tab: DebugStatsTab) {
        self.active_tab = tab;
        self.scroll_offset = 0; // Reset scroll when switching tabs
        // Auto-focus input when switching to E-Tag Query tab
        self.e_tag_input_focused = tab == DebugStatsTab::ETagQuery;
        // Focus sidebar when switching to subscriptions tab
        if tab == DebugStatsTab::Subscriptions {
            self.sub_sidebar_focused = true;
        }
    }

    /// Update the project filters from subscription stats
    pub fn update_project_filters(&mut self, stats: &tenex_core::stats::SubscriptionStats) {
        use std::collections::HashSet;

        let mut projects: HashSet<String> = HashSet::new();
        for info in stats.subscriptions.values() {
            if let Some(ref a_tag) = info.project_a_tag {
                projects.insert(a_tag.clone());
            }
        }

        // Build filter list: None (All), then sorted projects
        let mut filters: Vec<Option<String>> = vec![None];
        let mut sorted_projects: Vec<_> = projects.into_iter().collect();
        sorted_projects.sort();
        for project in sorted_projects {
            filters.push(Some(project));
        }

        self.sub_project_filters = filters;
        // Ensure selected index is still valid
        if self.sub_selected_filter_index >= self.sub_project_filters.len() {
            self.sub_selected_filter_index = 0;
        }
    }

    /// Get the currently selected project filter (None = All)
    pub fn selected_project_filter(&self) -> Option<&str> {
        self.sub_project_filters
            .get(self.sub_selected_filter_index)
            .and_then(|f| f.as_deref())
    }

    pub fn next_tab(&mut self) {
        self.switch_tab(self.active_tab.next());
    }

    pub fn prev_tab(&mut self) {
        self.switch_tab(self.active_tab.prev());
    }
}

/// Unified modal state - only one modal can be open at a time
#[derive(Debug, Clone)]
pub enum ModalState {
    None,
    /// Command palette (Ctrl+T) - context-sensitive command menu
    CommandPalette(CommandPaletteState),
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
    /// Conversation action menu in Home view - shows actions for selected conversation (via Ctrl+T)
    ConversationActions(ConversationActionsState),
    /// Chat action menu in Chat view - shows actions for current conversation (via Ctrl+T)
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
    /// Agent settings modal (model and tools configuration)
    AgentSettings(AgentSettingsState),
    /// Draft navigator modal for viewing and restoring saved drafts
    DraftNavigator(DraftNavigatorState),
    /// Backend approval modal for unknown backend pubkeys
    BackendApproval(BackendApprovalState),
    /// Debug stats modal (Ctrl+T D)
    DebugStats(DebugStatsState),
    /// History search modal (Ctrl+R) - search through previous messages sent by user
    HistorySearch(HistorySearchState),
    /// Nudge list view - browse and manage nudges
    NudgeList(NudgeListState),
    /// Nudge create form - multi-step wizard for creating nudges (also used for copy)
    NudgeCreate(NudgeFormState),
    /// Nudge detail view - read-only view of a nudge
    NudgeDetail(NudgeDetailState),
    /// Nudge delete confirmation - confirm deletion of a nudge
    NudgeDeleteConfirm(NudgeDeleteConfirmState),
    /// Workspace manager modal - create, edit, delete, switch workspaces
    WorkspaceManager(WorkspaceManagerState),
    /// Global app settings modal (accessible via comma key from anywhere)
    AppSettings(AppSettingsState),
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
