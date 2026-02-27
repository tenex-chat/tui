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
        agent_definition_ids: Vec<String>,
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
    /// Start NIP-46 bunker signer
    BunkerStart,
    /// Stop NIP-46 bunker signer
    BunkerStop,
    /// Get bunker runtime status
    BunkerStatus,
    /// Watch pending bunker signing requests (interactive local mode)
    BunkerWatch,
    /// Enable bunker auto-start and persist preference
    BunkerEnable,
    /// Disable bunker auto-start and persist preference
    BunkerDisable,
    /// List persisted bunker auto-approve rules
    BunkerRulesList,
    /// Add persisted bunker auto-approve rule
    BunkerRulesAdd {
        requester_pubkey: String,
        event_kind: Option<u16>,
    },
    /// Remove persisted bunker auto-approve rule
    BunkerRulesRemove {
        requester_pubkey: String,
        event_kind: Option<u16>,
    },
    /// Get bunker session audit entries
    BunkerAudit { limit: Option<usize> },
    /// Internal: list pending bunker signing requests
    BunkerListPending,
    /// Internal: respond to pending bunker signing request
    BunkerRespond { request_id: String, approved: bool },
}

impl CliCommand {
    /// Convert to a Request for sending to daemon
    pub fn to_request(&self, id: u64) -> Option<Request> {
        let (method, params) = match self {
            CliCommand::Daemon | CliCommand::BunkerWatch => return None, // Not sent to daemon
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
                agent_definition_ids,
                mcp_tool_ids,
            } => (
                "save_project",
                serde_json::json!({
                    "slug": slug,
                    "name": name,
                    "description": description,
                    "agent_definition_ids": agent_definition_ids,
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
            CliCommand::BunkerStart => ("bunker_start", serde_json::json!({})),
            CliCommand::BunkerStop => ("bunker_stop", serde_json::json!({})),
            CliCommand::BunkerStatus => ("bunker_status", serde_json::json!({})),
            CliCommand::BunkerEnable => {
                ("bunker_set_enabled", serde_json::json!({ "enabled": true }))
            }
            CliCommand::BunkerDisable => (
                "bunker_set_enabled",
                serde_json::json!({ "enabled": false }),
            ),
            CliCommand::BunkerRulesList => ("bunker_rules_list", serde_json::json!({})),
            CliCommand::BunkerRulesAdd {
                requester_pubkey,
                event_kind,
            } => (
                "bunker_rules_add",
                serde_json::json!({
                    "requester_pubkey": requester_pubkey,
                    "event_kind": event_kind
                }),
            ),
            CliCommand::BunkerRulesRemove {
                requester_pubkey,
                event_kind,
            } => (
                "bunker_rules_remove",
                serde_json::json!({
                    "requester_pubkey": requester_pubkey,
                    "event_kind": event_kind
                }),
            ),
            CliCommand::BunkerAudit { limit } => (
                "bunker_audit",
                serde_json::json!({
                    "limit": limit
                }),
            ),
            CliCommand::BunkerListPending => ("bunker_list_pending", serde_json::json!({})),
            CliCommand::BunkerRespond {
                request_id,
                approved,
            } => (
                "bunker_respond",
                serde_json::json!({
                    "request_id": request_id,
                    "approved": approved
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bunker_watch_is_local_only() {
        assert!(CliCommand::BunkerWatch.to_request(1).is_none());
    }

    #[test]
    fn bunker_start_to_request_mapping() {
        let req = CliCommand::BunkerStart.to_request(42).expect("request");
        assert_eq!(req.id, 42);
        assert_eq!(req.method, "bunker_start");
        assert_eq!(req.params, serde_json::json!({}));
    }

    #[test]
    fn bunker_stop_to_request_mapping() {
        let req = CliCommand::BunkerStop.to_request(7).expect("request");
        assert_eq!(req.method, "bunker_stop");
        assert_eq!(req.params, serde_json::json!({}));
    }

    #[test]
    fn bunker_status_to_request_mapping() {
        let req = CliCommand::BunkerStatus.to_request(8).expect("request");
        assert_eq!(req.method, "bunker_status");
        assert_eq!(req.params, serde_json::json!({}));
    }

    #[test]
    fn bunker_enable_disable_to_request_mapping() {
        let enable_req = CliCommand::BunkerEnable.to_request(9).expect("request");
        assert_eq!(enable_req.method, "bunker_set_enabled");
        assert_eq!(enable_req.params, serde_json::json!({ "enabled": true }));

        let disable_req = CliCommand::BunkerDisable.to_request(10).expect("request");
        assert_eq!(disable_req.method, "bunker_set_enabled");
        assert_eq!(disable_req.params, serde_json::json!({ "enabled": false }));
    }

    #[test]
    fn bunker_rules_list_to_request_mapping() {
        let req = CliCommand::BunkerRulesList.to_request(11).expect("request");
        assert_eq!(req.method, "bunker_rules_list");
        assert_eq!(req.params, serde_json::json!({}));
    }

    #[test]
    fn bunker_rules_add_to_request_mapping() {
        let req = CliCommand::BunkerRulesAdd {
            requester_pubkey: "abc123".to_string(),
            event_kind: Some(1),
        }
        .to_request(12)
        .expect("request");
        assert_eq!(req.method, "bunker_rules_add");
        assert_eq!(
            req.params,
            serde_json::json!({
                "requester_pubkey": "abc123",
                "event_kind": 1
            })
        );
    }

    #[test]
    fn bunker_rules_remove_to_request_mapping() {
        let req = CliCommand::BunkerRulesRemove {
            requester_pubkey: "abc123".to_string(),
            event_kind: None,
        }
        .to_request(13)
        .expect("request");
        assert_eq!(req.method, "bunker_rules_remove");
        assert_eq!(
            req.params,
            serde_json::json!({
                "requester_pubkey": "abc123",
                "event_kind": null
            })
        );
    }

    #[test]
    fn bunker_audit_to_request_mapping() {
        let req = CliCommand::BunkerAudit { limit: Some(25) }
            .to_request(14)
            .expect("request");
        assert_eq!(req.method, "bunker_audit");
        assert_eq!(req.params, serde_json::json!({ "limit": 25 }));
    }

    #[test]
    fn bunker_list_pending_to_request_mapping() {
        let req = CliCommand::BunkerListPending
            .to_request(15)
            .expect("request");
        assert_eq!(req.method, "bunker_list_pending");
        assert_eq!(req.params, serde_json::json!({}));
    }

    #[test]
    fn bunker_respond_to_request_mapping() {
        let req = CliCommand::BunkerRespond {
            request_id: "req-1".to_string(),
            approved: true,
        }
        .to_request(16)
        .expect("request");
        assert_eq!(req.method, "bunker_respond");
        assert_eq!(
            req.params,
            serde_json::json!({
                "request_id": "req-1",
                "approved": true
            })
        );
    }
}
