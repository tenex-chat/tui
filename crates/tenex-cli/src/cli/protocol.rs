use serde::{Deserialize, Serialize};

/// Request from CLI client to daemon
#[derive(Debug, Serialize, Deserialize)]
pub struct Request {
    pub id: u64,
    pub method: String,
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Response from daemon to CLI client
#[derive(Debug, Serialize, Deserialize)]
pub struct Response {
    pub id: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<ErrorInfo>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct ErrorInfo {
    pub code: String,
    pub message: String,
}

impl Response {
    pub fn success(id: u64, result: serde_json::Value) -> Self {
        Self {
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn error(id: u64, code: &str, message: &str) -> Self {
        Self {
            id,
            result: None,
            error: Some(ErrorInfo {
                code: code.to_string(),
                message: message.to_string(),
            }),
        }
    }
}

/// CLI command parsed from arguments
#[derive(Debug, Clone)]
pub enum CliCommand {
    /// Start daemon in foreground
    Daemon,
    /// List all projects
    ListProjects,
    /// List threads for a project
    ListThreads {
        project_slug: String,
        wait_for_project: bool,
    },
    /// List agents for a project
    ListAgents {
        project_slug: String,
        wait_for_project: bool,
    },
    /// List messages in a thread
    ListMessages { thread_id: String },
    /// Get full state dump
    GetState,
    /// Send a message to a thread (with recipient targeting)
    SendMessage {
        project_slug: String,
        thread_id: String,
        recipient_slug: String,
        content: String,
        wait_secs: Option<u64>,
        wait_for_project: bool,
        skill_ids: Vec<String>,
        nudge_ids: Vec<String>,
    },
    /// Create a new thread in a project (with recipient targeting)
    CreateThread {
        project_slug: String,
        recipient_slug: String,
        content: String,
        wait_secs: Option<u64>,
        wait_for_project: bool,
        skill_ids: Vec<String>,
        nudge_ids: Vec<String>,
    },
    /// Boot/start a project
    BootProject { project_slug: String, wait: bool },
    /// Get daemon status
    Status,
    /// Shutdown the daemon
    Shutdown,
    /// List all agent definitions (kind:4199)
    ListAgentDefinitions,
    /// List all MCP tools (kind:4200)
    ListMCPTools,
    /// List all skills (kind:4202)
    ListSkills,
    /// List all nudges (kind:4201)
    ListNudges,
    /// Show detailed project information (kind:24010)
    ShowProject {
        project_slug: String,
        wait_for_project: bool,
    },
    /// Save a project - create new or update existing (kind:31933)
    SaveProject {
        slug: Option<String>,
        name: String,
        description: String,
        agent_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
    },
    /// Set agent settings (kind:24020)
    SetAgentSettings {
        project_slug: String,
        agent_slug: String,
        model: String,
        tools: Vec<String>,
        wait_for_project: bool,
        wait: bool,
    },
}

impl CliCommand {
    /// Convert to a Request for sending to daemon
    pub fn to_request(&self, id: u64) -> Option<Request> {
        let (method, params) = match self {
            CliCommand::Daemon => return None, // Not sent to daemon
            CliCommand::ListProjects => ("list_projects", serde_json::json!({})),
            CliCommand::ListThreads {
                project_slug,
                wait_for_project,
            } => (
                "list_threads",
                serde_json::json!({ "project_slug": project_slug, "wait_for_project": wait_for_project }),
            ),
            CliCommand::ListAgents {
                project_slug,
                wait_for_project,
            } => (
                "list_agents",
                serde_json::json!({ "project_slug": project_slug, "wait_for_project": wait_for_project }),
            ),
            CliCommand::ListMessages { thread_id } => (
                "list_messages",
                serde_json::json!({ "thread_id": thread_id }),
            ),
            CliCommand::GetState => ("get_state", serde_json::json!({})),
            CliCommand::SendMessage {
                project_slug,
                thread_id,
                recipient_slug,
                content,
                wait_for_project,
                skill_ids,
                nudge_ids,
                ..
            } => (
                "send_message",
                serde_json::json!({
                    "project_slug": project_slug,
                    "thread_id": thread_id,
                    "recipient_slug": recipient_slug,
                    "content": content,
                    "wait_for_project": wait_for_project,
                    "skill_ids": skill_ids,
                    "nudge_ids": nudge_ids
                }),
            ),
            CliCommand::CreateThread {
                project_slug,
                recipient_slug,
                content,
                wait_for_project,
                skill_ids,
                nudge_ids,
                ..
            } => (
                "create_thread",
                serde_json::json!({
                    "project_slug": project_slug,
                    "recipient_slug": recipient_slug,
                    "content": content,
                    "wait_for_project": wait_for_project,
                    "skill_ids": skill_ids,
                    "nudge_ids": nudge_ids
                }),
            ),
            CliCommand::BootProject { project_slug, .. } => (
                "boot_project",
                serde_json::json!({ "project_slug": project_slug }),
            ),
            CliCommand::Status => ("status", serde_json::json!({})),
            CliCommand::Shutdown => ("shutdown", serde_json::json!({})),
            CliCommand::ListAgentDefinitions => ("list_agent_definitions", serde_json::json!({})),
            CliCommand::ListMCPTools => ("list_mcp_tools", serde_json::json!({})),
            CliCommand::ListSkills => ("list_skills", serde_json::json!({})),
            CliCommand::ListNudges => ("list_nudges", serde_json::json!({})),
            CliCommand::ShowProject {
                project_slug,
                wait_for_project,
            } => (
                "show_project",
                serde_json::json!({ "project_slug": project_slug, "wait_for_project": wait_for_project }),
            ),
            CliCommand::SaveProject {
                slug,
                name,
                description,
                agent_ids,
                mcp_tool_ids,
            } => (
                "save_project",
                serde_json::json!({
                    "slug": slug,
                    "name": name,
                    "description": description,
                    "agent_ids": agent_ids,
                    "mcp_tool_ids": mcp_tool_ids,
                    "client": "tenex-cli"
                }),
            ),
            CliCommand::SetAgentSettings {
                project_slug,
                agent_slug,
                model,
                tools,
                wait_for_project,
                wait,
            } => (
                "set_agent_settings",
                serde_json::json!({
                    "project_slug": project_slug,
                    "agent_slug": agent_slug,
                    "model": model,
                    "tools": tools,
                    "wait_for_project": wait_for_project,
                    "wait": wait
                }),
            ),
        };

        Some(Request {
            id,
            method: method.to_string(),
            params,
        })
    }
}
