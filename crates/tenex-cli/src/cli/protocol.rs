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
    ListThreads { project_id: String },
    /// List messages in a thread
    ListMessages { thread_id: String },
    /// Get full state dump
    GetState,
    /// Send a message to a thread
    SendMessage { thread_id: String, content: String },
    /// Create a new thread in a project
    CreateThread { project_id: String, title: String },
    /// Boot/start a project
    BootProject { project_id: String },
    /// Get daemon status
    Status,
    /// Shutdown the daemon
    Shutdown,
    /// List all agent definitions (kind:4199)
    ListAgentDefinitions,
    /// Create a new project (kind:31933)
    CreateProject {
        name: String,
        description: String,
        agent_ids: Vec<String>,
    },
}

impl CliCommand {
    /// Convert to a Request for sending to daemon
    pub fn to_request(&self, id: u64) -> Option<Request> {
        let (method, params) = match self {
            CliCommand::Daemon => return None, // Not sent to daemon
            CliCommand::ListProjects => ("list_projects", serde_json::json!({})),
            CliCommand::ListThreads { project_id } => {
                ("list_threads", serde_json::json!({ "project_id": project_id }))
            }
            CliCommand::ListMessages { thread_id } => {
                ("list_messages", serde_json::json!({ "thread_id": thread_id }))
            }
            CliCommand::GetState => ("get_state", serde_json::json!({})),
            CliCommand::SendMessage { thread_id, content } => (
                "send_message",
                serde_json::json!({ "thread_id": thread_id, "content": content }),
            ),
            CliCommand::CreateThread { project_id, title } => (
                "create_thread",
                serde_json::json!({ "project_id": project_id, "title": title }),
            ),
            CliCommand::BootProject { project_id } => {
                ("boot_project", serde_json::json!({ "project_id": project_id }))
            }
            CliCommand::Status => ("status", serde_json::json!({})),
            CliCommand::Shutdown => ("shutdown", serde_json::json!({})),
            CliCommand::ListAgentDefinitions => ("list_agent_definitions", serde_json::json!({})),
            CliCommand::CreateProject { name, description, agent_ids } => (
                "create_project",
                serde_json::json!({
                    "name": name,
                    "description": description,
                    "agent_ids": agent_ids
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
