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
    /// Pubkey of the ask event author (for p-tagging in response)
    pub ask_author_pubkey: String,
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
        }
    }

    pub fn move_cursor_down(&mut self) {
        let max = self.visible_item_count();
        if self.tools_cursor + 1 < max {
            self.tools_cursor += 1;
        }
    }
}

/// Context for command palette - determines which commands are shown
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaletteContext {
    HomeRecent,
    HomeInbox,
    HomeReports,
    HomeSidebar { is_online: bool, is_busy: bool, is_archived: bool },
    ChatNormal { has_parent: bool, has_trace: bool, agent_working: bool },
    ChatEditing,
    AgentBrowserList,
    AgentBrowserDetail,
}

/// A command available in the palette
#[derive(Debug, Clone)]
pub struct PaletteCommand {
    pub key: char,
    pub label: &'static str,
    pub section: &'static str,
}

impl PaletteCommand {
    pub const fn new(key: char, label: &'static str, section: &'static str) -> Self {
        Self { key, label, section }
    }
}

/// State for command palette modal (Ctrl+T)
#[derive(Debug, Clone)]
pub struct CommandPaletteState {
    pub selected_index: usize,
    pub context: PaletteContext,
}

impl CommandPaletteState {
    pub fn new(context: PaletteContext) -> Self {
        Self {
            selected_index: 0,
            context,
        }
    }

    /// Get commands available for the current context
    ///
    /// Commands are sorted by section name to match the display order in the palette
    /// (which uses BTreeMap for grouping). This ensures selected_index matches visually.
    pub fn available_commands(&self) -> Vec<PaletteCommand> {
        let mut commands = Vec::new();

        // Global commands (always available)
        commands.push(PaletteCommand::new('1', "Go to Home", "Navigation"));
        commands.push(PaletteCommand::new('?', "Help", "Navigation"));
        commands.push(PaletteCommand::new('q', "Quit", "System"));
        commands.push(PaletteCommand::new('r', "Refresh", "System"));

        // Context-specific commands
        match self.context {
            PaletteContext::HomeRecent => {
                commands.push(PaletteCommand::new('n', "New conversation", "Conversation"));
                commands.push(PaletteCommand::new('o', "Open selected", "Conversation"));
                commands.push(PaletteCommand::new('a', "Archive/Unarchive", "Conversation"));
                commands.push(PaletteCommand::new('e', "Export JSONL", "Conversation"));
                commands.push(PaletteCommand::new('p', "Switch project", "Filter"));
                commands.push(PaletteCommand::new('f', "Time filter", "Filter"));
                commands.push(PaletteCommand::new('A', "Agent Browser", "Other"));
                commands.push(PaletteCommand::new('N', "Create project", "Other"));
            }
            PaletteContext::HomeInbox => {
                commands.push(PaletteCommand::new('o', "Open selected", "Inbox"));
                commands.push(PaletteCommand::new('R', "Mark as read", "Inbox"));
                commands.push(PaletteCommand::new('M', "Mark all read", "Inbox"));
                commands.push(PaletteCommand::new('p', "Switch project", "Filter"));
            }
            PaletteContext::HomeReports => {
                commands.push(PaletteCommand::new('o', "View report", "Reports"));
                commands.push(PaletteCommand::new('p', "Switch project", "Filter"));
            }
            PaletteContext::HomeSidebar { is_online, is_busy, is_archived } => {
                commands.push(PaletteCommand::new(' ', "Toggle visibility", "Project"));
                commands.push(PaletteCommand::new('n', "New conversation", "Project"));
                commands.push(PaletteCommand::new('s', "Settings", "Project"));
                if !is_online {
                    commands.push(PaletteCommand::new('b', "Boot project", "Project"));
                }
                if is_busy {
                    commands.push(PaletteCommand::new('.', "Stop all agents", "Project"));
                }
                if is_archived {
                    commands.push(PaletteCommand::new('a', "Unarchive", "Project"));
                } else {
                    commands.push(PaletteCommand::new('a', "Archive", "Project"));
                }
            }
            PaletteContext::ChatNormal { has_parent, has_trace, agent_working } => {
                commands.push(PaletteCommand::new('@', "Mention agent", "Input"));
                commands.push(PaletteCommand::new('%', "Select branch", "Input"));
                commands.push(PaletteCommand::new('y', "Copy content", "Message"));
                commands.push(PaletteCommand::new('v', "View raw event", "Message"));
                if has_trace {
                    commands.push(PaletteCommand::new('t', "Open trace", "Message"));
                }
                commands.push(PaletteCommand::new('S', "Agent settings", "Agent"));
                if agent_working {
                    commands.push(PaletteCommand::new('.', "Stop agent", "Agent"));
                }
                commands.push(PaletteCommand::new('n', "New conversation", "Conversation"));
                if has_parent {
                    commands.push(PaletteCommand::new('g', "Go to parent", "Conversation"));
                }
                commands.push(PaletteCommand::new('c', "Copy conversation ID", "Conversation"));
                commands.push(PaletteCommand::new('e', "Copy JSONL", "Conversation"));
                commands.push(PaletteCommand::new('a', "Archive/Unarchive", "Conversation"));
                commands.push(PaletteCommand::new('x', "Close tab", "Tab"));
                commands.push(PaletteCommand::new('X', "Archive + Close", "Tab"));
                commands.push(PaletteCommand::new('T', "Toggle sidebar", "View"));
            }
            PaletteContext::ChatEditing => {
                commands.push(PaletteCommand::new('@', "Mention agent", "Input"));
                commands.push(PaletteCommand::new('%', "Select branch", "Input"));
                commands.push(PaletteCommand::new('E', "Expand editor", "Input"));
                commands.push(PaletteCommand::new('S', "Agent settings", "Agent"));
                commands.push(PaletteCommand::new('n', "New conversation", "Conversation"));
                commands.push(PaletteCommand::new('c', "Copy conversation ID", "Conversation"));
                commands.push(PaletteCommand::new('e', "Copy JSONL", "Conversation"));
                commands.push(PaletteCommand::new('a', "Archive/Unarchive", "Conversation"));
                commands.push(PaletteCommand::new('x', "Close tab", "Tab"));
                commands.push(PaletteCommand::new('X', "Archive + Close", "Tab"));
            }
            PaletteContext::AgentBrowserList => {
                commands.push(PaletteCommand::new('o', "View agent", "Agent"));
                commands.push(PaletteCommand::new('n', "Create new agent", "Agent"));
            }
            PaletteContext::AgentBrowserDetail => {
                commands.push(PaletteCommand::new('f', "Fork agent", "Agent"));
                commands.push(PaletteCommand::new('c', "Clone agent", "Agent"));
            }
        }

        // Sort by section name to match display order (BTreeMap iterates alphabetically)
        commands.sort_by(|a, b| a.section.cmp(b.section));
        commands
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
