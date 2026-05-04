use nostrdb::Note;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants::STALENESS_THRESHOLD_SECS;

/// Represents an agent in a project roster.
///
/// Roster membership and PM/default status come from ordered kind:31933 `p`
/// tags. kind:24011 inventories set `is_online` and backend availability.
/// kind:34011 configs set current model/tools/skills/MCP fields.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProjectAgent {
    pub pubkey: String,
    pub name: String,
    pub backend_pubkey: String,
    pub is_pm: bool,
    pub is_online: bool,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub skills: Vec<String>,
    pub mcp_servers: Vec<String>,
}

/// Represents a TENEX project status (Nostr kind:24010).
///
/// 24010 is project-level heartbeat/config-option data. It does not define the
/// project roster, PM/default agent, agent availability, or current per-agent
/// config.
#[derive(Debug, Clone)]
pub struct ProjectStatus {
    pub project_coordinate: String,
    pub agents: Vec<ProjectAgent>,
    pub branches: Vec<String>,
    /// All available models from model tags (including unassigned ones)
    pub all_models: Vec<String>,
    /// All project-level tools from tool tags.
    /// Current per-agent tools are sourced from kind:34011.
    pub(crate) all_tools: Vec<String>,
    /// All available skills from skill tags (including unassigned ones).
    pub(crate) all_skills: Vec<String>,
    /// All available MCP servers from mcp tags (including unassigned ones).
    pub(crate) all_mcp_servers: Vec<String>,
    pub created_at: u64,
    /// The pubkey of the backend that published this status event
    pub backend_pubkey: String,
    /// When this status was last seen by the client (seconds since UNIX epoch)
    pub last_seen_at: u64,
}

impl ProjectStatus {
    fn agent_aggregation_key(agent: &ProjectAgent) -> String {
        agent.pubkey.clone()
    }

    /// Create from JSON string (for ephemeral events received via DataChange channel)
    pub fn from_json(json: &str) -> Option<Self> {
        let event: serde_json::Value = serde_json::from_str(json).ok()?;
        Self::from_value(&event)
    }

    /// Create from pre-parsed serde_json::Value (avoids double parsing)
    pub fn from_value(event: &serde_json::Value) -> Option<Self> {
        let kind = event.get("kind")?.as_u64()?;
        if kind != 24010 {
            return None;
        }

        let backend_pubkey = event.get("pubkey")?.as_str()?.to_string();
        let created_at = event.get("created_at")?.as_u64()?;

        let tags_value = event.get("tags")?.as_array()?;
        let tags: Vec<Vec<String>> = tags_value
            .iter()
            .filter_map(|t| {
                t.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect()
                })
            })
            .collect();

        Self::from_tags(created_at, tags, backend_pubkey)
    }

    /// Create from nostrdb::Note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 24010 {
            return None;
        }

        // Extract backend pubkey from note author
        let backend_pubkey = hex::encode(note.pubkey());

        // Extract tags, handling the id variant for agent pubkeys
        let mut tags: Vec<Vec<String>> = Vec::new();
        for tag in note.tags() {
            let mut parts: Vec<String> = Vec::new();
            for i in 0..tag.count() {
                if let Some(t) = tag.get(i) {
                    if let Some(s) = t.variant().str() {
                        parts.push(s.to_string());
                    } else if let Some(id) = t.variant().id() {
                        parts.push(hex::encode(id));
                    }
                }
            }
            tags.push(parts);
        }

        Self::from_tags(note.created_at(), tags, backend_pubkey)
    }

    /// Common parsing logic for tags
    fn from_tags(created_at: u64, tags: Vec<Vec<String>>, backend_pubkey: String) -> Option<Self> {
        let mut project_coordinate: Option<String> = None;
        let mut branches: Vec<String> = Vec::new();
        let mut all_models: Vec<String> = Vec::new();
        let mut all_tools: Vec<String> = Vec::new();
        let mut all_skills: Vec<String> = Vec::new();
        let mut all_mcp_servers: Vec<String> = Vec::new();

        // Collect project coordinate, branches, and project-level option tags.
        for tag in &tags {
            if tag.is_empty() {
                continue;
            }

            match tag[0].as_str() {
                "a" => {
                    if project_coordinate.is_none() && tag.len() > 1 {
                        project_coordinate = Some(tag[1].clone());
                    }
                }
                "agent" => {}
                "branch" => {
                    if tag.len() > 1 {
                        branches.push(tag[1].clone());
                    }
                }
                "model" => {
                    // Collect model name (tag[1]) regardless of agent assignments
                    if tag.len() >= 2 {
                        all_models.push(tag[1].clone());
                    }
                }
                "tool" => {
                    // Collect tool name (tag[1]) regardless of agent assignments
                    if tag.len() >= 2 {
                        all_tools.push(tag[1].clone());
                    }
                }
                "skill" => {
                    // Collect skill name (tag[1]) regardless of agent assignments
                    if tag.len() >= 2 {
                        all_skills.push(tag[1].clone());
                    }
                }
                "mcp" => {
                    // Collect MCP server slug (tag[1]) regardless of agent assignments
                    if tag.len() >= 2 {
                        all_mcp_servers.push(tag[1].clone());
                    }
                }
                _ => {}
            }
        }

        // Deduplicate and sort models and tools
        all_models.sort();
        all_models.dedup();
        all_tools.sort();
        all_tools.dedup();
        all_skills.sort();
        all_skills.dedup();
        all_mcp_servers.sort();
        all_mcp_servers.dedup();

        // Agent roster and current configuration are not derived from 24010.
        // Membership/defaults come from 31933, availability comes from 24011,
        // and per-agent model/tool/skill/MCP state comes from 34011.

        let project_coordinate = project_coordinate?;

        Some(ProjectStatus {
            project_coordinate,
            agents: Vec::new(),
            branches,
            all_models,
            all_tools,
            all_skills,
            all_mcp_servers,
            created_at,
            backend_pubkey,
            last_seen_at: created_at,
        })
    }

    /// Whether this status is considered online (not stale)
    pub fn is_online(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.last_seen_at) < STALENESS_THRESHOLD_SECS
    }

    /// The default branch (first in the branches array)
    pub fn default_branch(&self) -> Option<&str> {
        self.branches.first().map(|s| s.as_str())
    }

    /// Returns all available tools (including unassigned tools).
    ///
    /// **⚠️ DEPRECATED**: Use `all_tools()` directly.
    #[deprecated(since = "0.1.0", note = "Use `all_tools()` instead")]
    pub fn tools(&self) -> Vec<&str> {
        self.all_tools()
    }

    /// Returns current tools carried on ProjectAgent records.
    /// 24010 parsing leaves this empty; roster enrichment happens outside
    /// ProjectStatus via the shared store roster builder.
    pub fn agent_assigned_tools(&self) -> Vec<&str> {
        let mut tools: Vec<&str> = self
            .agents
            .iter()
            .flat_map(|a| a.tools.iter().map(|s| s.as_str()))
            .collect();
        tools.sort();
        tools.dedup();
        tools
    }

    /// Returns all available tools (including both assigned and unassigned tools).
    ///
    /// **Recommended for UI display** to show all available tools to users.
    pub fn all_tools(&self) -> Vec<&str> {
        self.all_tools.iter().map(|s| s.as_str()).collect()
    }

    /// Returns all project-level skills mentioned by the heartbeat.
    pub fn all_skills(&self) -> Vec<&str> {
        self.all_skills.iter().map(|s| s.as_str()).collect()
    }

    /// Returns current skills carried on ProjectAgent records.
    /// 24010 parsing leaves this empty; roster enrichment happens outside
    /// ProjectStatus via the shared store roster builder.
    pub fn agent_assigned_skills(&self) -> Vec<&str> {
        let mut skills: Vec<&str> = self
            .agents
            .iter()
            .flat_map(|a| a.skills.iter().map(|s| s.as_str()))
            .collect();
        skills.sort();
        skills.dedup();
        skills
    }

    /// Returns all available MCP servers (including both assigned and unassigned servers).
    pub fn all_mcp_servers(&self) -> Vec<&str> {
        self.all_mcp_servers.iter().map(|s| s.as_str()).collect()
    }

    /// All available models from the project (including unassigned ones)
    pub fn models(&self) -> Vec<&str> {
        self.all_models.iter().map(|s| s.as_str()).collect()
    }

    /// Get the PM (project manager) agent
    pub fn pm_agent(&self) -> Option<&ProjectAgent> {
        self.agents.iter().find(|a| a.is_pm)
    }

    /// Aggregate per-backend project statuses into one project-level view.
    pub fn aggregate<'a, I>(project_coordinate: String, statuses: I) -> Option<Self>
    where
        I: IntoIterator<Item = &'a ProjectStatus>,
    {
        let mut agent_map: HashMap<String, (u64, u64, ProjectAgent)> = HashMap::new();
        let mut branches: Vec<String> = Vec::new();
        let mut all_models: Vec<String> = Vec::new();
        let mut all_tools: Vec<String> = Vec::new();
        let mut all_skills: Vec<String> = Vec::new();
        let mut all_mcp_servers: Vec<String> = Vec::new();
        let mut newest_backend_pubkey = String::new();
        let mut newest_created_at = 0;
        let mut newest_last_seen_at = 0;
        let mut saw_status = false;

        for status in statuses {
            if !status.is_online() {
                continue;
            }

            saw_status = true;
            if status.created_at >= newest_created_at {
                newest_created_at = status.created_at;
                newest_backend_pubkey = status.backend_pubkey.clone();
            }
            newest_last_seen_at = newest_last_seen_at.max(status.last_seen_at);

            branches.extend(status.branches.iter().cloned());
            all_models.extend(status.all_models.iter().cloned());
            all_tools.extend(status.all_tools.iter().cloned());
            all_skills.extend(status.all_skills.iter().cloned());
            all_mcp_servers.extend(status.all_mcp_servers.iter().cloned());

            for agent in &status.agents {
                let key = Self::agent_aggregation_key(agent);
                let should_replace =
                    agent_map
                        .get(&key)
                        .map_or(true, |(last_seen_at, created_at, _)| {
                            status.last_seen_at > *last_seen_at
                                || (status.last_seen_at == *last_seen_at
                                    && status.created_at >= *created_at)
                        });
                if should_replace {
                    let mut agent = agent.clone();
                    agent.backend_pubkey = status.backend_pubkey.clone();
                    agent_map.insert(key, (status.last_seen_at, status.created_at, agent));
                }
            }
        }

        if !saw_status {
            return None;
        }

        branches.sort();
        branches.dedup();
        all_models.sort();
        all_models.dedup();
        all_tools.sort();
        all_tools.dedup();
        all_skills.sort();
        all_skills.dedup();
        all_mcp_servers.sort();
        all_mcp_servers.dedup();

        let mut agents: Vec<ProjectAgent> =
            agent_map.into_values().map(|(_, _, agent)| agent).collect();
        agents.sort_by(|a, b| a.name.cmp(&b.name));

        Some(Self {
            project_coordinate,
            agents,
            branches,
            all_models,
            all_tools,
            all_skills,
            all_mcp_servers,
            created_at: newest_created_at,
            backend_pubkey: newest_backend_pubkey,
            last_seen_at: newest_last_seen_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper to generate common tool tags for testing
    fn tool_tag_fixtures() -> Vec<Vec<String>> {
        vec![
            // Tool tags with agent assignments (3+ elements)
            vec!["tool".to_string(), "Read".to_string(), "agent1".to_string()],
            vec![
                "tool".to_string(),
                "Write".to_string(),
                "agent1".to_string(),
            ],
            vec!["tool".to_string(), "Bash".to_string(), "agent1".to_string()],
            // Tool tags WITHOUT agent assignments (2 elements) - these should still be collected
            vec!["tool".to_string(), "rag_create_collection".to_string()],
            vec!["tool".to_string(), "rag_add_documents".to_string()],
            vec!["tool".to_string(), "rag_query".to_string()],
            vec!["tool".to_string(), "rag_delete_collection".to_string()],
            vec!["tool".to_string(), "rag_list_collections".to_string()],
            vec!["tool".to_string(), "rag_subscription_create".to_string()],
            vec!["tool".to_string(), "rag_subscription_list".to_string()],
            vec!["tool".to_string(), "rag_subscription_get".to_string()],
            vec!["tool".to_string(), "rag_subscription_delete".to_string()],
            vec!["tool".to_string(), "schedule_task_cancel".to_string()],
            vec!["tool".to_string(), "schedule_task".to_string()],
            vec!["tool".to_string(), "schedule_task_once".to_string()],
            vec!["tool".to_string(), "schedule_tasks_list".to_string()],
            vec!["tool".to_string(), "kill_shell".to_string()],
            vec!["tool".to_string(), "conversation_index".to_string()],
        ]
    }

    /// Helper to assert that critical must-have tools are present
    /// Only checks for unassigned tools that should always be in all_tools
    fn assert_must_have_unassigned_tools(all_tools: &[String]) {
        // RAG tools (unassigned)
        assert!(
            all_tools.contains(&"rag_create_collection".to_string()),
            "Missing rag_create_collection"
        );
        assert!(
            all_tools.contains(&"rag_add_documents".to_string()),
            "Missing rag_add_documents"
        );
        assert!(
            all_tools.contains(&"rag_query".to_string()),
            "Missing rag_query"
        );
        assert!(
            all_tools.contains(&"rag_delete_collection".to_string()),
            "Missing rag_delete_collection"
        );
        assert!(
            all_tools.contains(&"rag_list_collections".to_string()),
            "Missing rag_list_collections"
        );

        // Scheduling tools (unassigned)
        assert!(
            all_tools.contains(&"schedule_task_cancel".to_string()),
            "Missing schedule_task_cancel"
        );
        assert!(
            all_tools.contains(&"schedule_task".to_string()),
            "Missing schedule_task"
        );
        assert!(
            all_tools.contains(&"schedule_task_once".to_string()),
            "Missing schedule_task_once"
        );
        assert!(
            all_tools.contains(&"schedule_tasks_list".to_string()),
            "Missing schedule_tasks_list"
        );

        // Other critical tools (unassigned)
        assert!(
            all_tools.contains(&"kill_shell".to_string()),
            "Missing kill_shell"
        );
        assert!(
            all_tools.contains(&"conversation_index".to_string()),
            "Missing conversation_index"
        );
    }

    /// Helper to assert that tools with agent assignments are present
    fn assert_assigned_tools(all_tools: &[String]) {
        assert!(all_tools.contains(&"Read".to_string()), "Missing Read");
        assert!(all_tools.contains(&"Write".to_string()), "Missing Write");
        assert!(all_tools.contains(&"Bash".to_string()), "Missing Bash");
    }

    #[test]
    fn test_aggregate_deduplicates_agents_by_pubkey_and_prefers_fresher_backend() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let older_status = ProjectStatus {
            project_coordinate: "31933:user:project".to_string(),
            agents: vec![ProjectAgent {
                pubkey: "agent1-pubkey".to_string(),
                name: "old-status-name".to_string(),
                backend_pubkey: "backend-old".to_string(),
                is_pm: false,
                is_online: true,
                model: None,
                tools: Vec::new(),
                skills: Vec::new(),
                mcp_servers: Vec::new(),
            }],
            branches: Vec::new(),
            all_models: Vec::new(),
            all_tools: Vec::new(),
            all_skills: Vec::new(),
            all_mcp_servers: Vec::new(),
            created_at: now,
            backend_pubkey: "backend-old".to_string(),
            last_seen_at: now.saturating_sub(5),
        };
        let fresher_status = ProjectStatus {
            project_coordinate: "31933:user:project".to_string(),
            agents: vec![ProjectAgent {
                pubkey: "agent1-pubkey".to_string(),
                name: "new-status-name".to_string(),
                backend_pubkey: "backend-fresh".to_string(),
                is_pm: false,
                is_online: true,
                model: None,
                tools: Vec::new(),
                skills: Vec::new(),
                mcp_servers: Vec::new(),
            }],
            branches: Vec::new(),
            all_models: Vec::new(),
            all_tools: Vec::new(),
            all_skills: Vec::new(),
            all_mcp_servers: Vec::new(),
            created_at: now.saturating_sub(10),
            backend_pubkey: "backend-fresh".to_string(),
            last_seen_at: now,
        };

        let aggregate = ProjectStatus::aggregate(
            "31933:user:project".to_string(),
            [&older_status, &fresher_status],
        )
        .expect("expected aggregate status");

        assert_eq!(aggregate.agents.len(), 1);
        assert_eq!(aggregate.agents[0].pubkey, "agent1-pubkey");
        assert_eq!(aggregate.agents[0].name, "new-status-name");
        assert_eq!(aggregate.agents[0].backend_pubkey, "backend-fresh");
    }

    #[test]
    fn test_aggregate_keeps_same_name_distinct_pubkeys() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let status = ProjectStatus {
            project_coordinate: "31933:user:project".to_string(),
            agents: vec![
                ProjectAgent {
                    pubkey: "agent-a-pubkey".to_string(),
                    name: "same-name".to_string(),
                    backend_pubkey: "backend".to_string(),
                    is_pm: false,
                    is_online: true,
                    model: None,
                    tools: Vec::new(),
                    skills: Vec::new(),
                    mcp_servers: Vec::new(),
                },
                ProjectAgent {
                    pubkey: "agent-b-pubkey".to_string(),
                    name: "same-name".to_string(),
                    backend_pubkey: "backend".to_string(),
                    is_pm: false,
                    is_online: true,
                    model: None,
                    tools: Vec::new(),
                    skills: Vec::new(),
                    mcp_servers: Vec::new(),
                },
            ],
            branches: Vec::new(),
            all_models: Vec::new(),
            all_tools: Vec::new(),
            all_skills: Vec::new(),
            all_mcp_servers: Vec::new(),
            created_at: now,
            backend_pubkey: "backend".to_string(),
            last_seen_at: now,
        };

        let aggregate = ProjectStatus::aggregate("31933:user:project".to_string(), [&status])
            .expect("expected aggregate status");

        assert_eq!(aggregate.agents.len(), 2);
        assert!(aggregate
            .agents
            .iter()
            .any(|agent| agent.pubkey == "agent-a-pubkey"));
        assert!(aggregate
            .agents
            .iter()
            .any(|agent| agent.pubkey == "agent-b-pubkey"));
    }

    #[test]
    fn test_all_tools_extraction() {
        // Simulate a kind:24010 event with various tool tags
        let mut tags = vec![
            vec!["a".to_string(), "31933:pubkey:identifier".to_string()],
            vec![
                "agent".to_string(),
                "agent1_pubkey".to_string(),
                "agent1".to_string(),
            ],
        ];
        tags.extend(tool_tag_fixtures());

        let backend_pubkey = "backend123".to_string();
        let status = ProjectStatus::from_tags(1234567890, tags, backend_pubkey).unwrap();

        // Use helpers to assert must-have tools
        assert_must_have_unassigned_tools(&status.all_tools);
        assert_assigned_tools(&status.all_tools);

        // Verify we have all tools from fixtures
        assert_eq!(status.all_tools.len(), 18, "Expected 18 tools");
    }

    #[test]
    fn test_all_tools_extraction_from_json() {
        // Simulate a kind:24010 event as JSON (how it comes from the backend)
        let json = r#"{
            "kind": 24010,
            "pubkey": "backend123",
            "created_at": 1234567890,
            "tags": [
                ["a", "31933:pubkey:identifier"],
                ["agent", "agent1_pubkey", "agent1"],
                ["tool", "Read", "agent1"],
                ["tool", "Write", "agent1"],
                ["tool", "Bash", "agent1"],
                ["tool", "rag_create_collection"],
                ["tool", "rag_add_documents"],
                ["tool", "rag_query"],
                ["tool", "rag_delete_collection"],
                ["tool", "rag_list_collections"],
                ["tool", "rag_subscription_create"],
                ["tool", "rag_subscription_list"],
                ["tool", "rag_subscription_get"],
                ["tool", "rag_subscription_delete"],
                ["tool", "schedule_task_cancel"],
                ["tool", "schedule_task"],
                ["tool", "schedule_task_once"],
                ["tool", "schedule_tasks_list"],
                ["tool", "kill_shell"],
                ["tool", "conversation_index"]
            ]
        }"#;

        let status = ProjectStatus::from_json(json).unwrap();

        // Use helpers to assert must-have tools
        assert_must_have_unassigned_tools(&status.all_tools);
        assert_assigned_tools(&status.all_tools);

        // Verify total count
        assert_eq!(status.all_tools.len(), 18, "Expected 18 tools from JSON");
    }

    /// This test simulates the exact scenario the user reported:
    /// A kind:24010 event with tool tags that have NO agent assignments
    #[test]
    fn test_tools_without_agent_assignments() {
        let json = r#"{
            "kind": 24010,
            "pubkey": "backend_pubkey_hex",
            "created_at": 1706400000,
            "tags": [
                ["a", "31933:user_pubkey:project_id"],
                ["agent", "agent_pubkey_1", "claude-code"],
                ["agent", "agent_pubkey_2", "architect"],
                ["tool", "Read", "claude-code"],
                ["tool", "Write", "claude-code"],
                ["tool", "Bash", "claude-code", "architect"],
                ["tool", "rag_create_collection"],
                ["tool", "rag_add_documents"],
                ["tool", "rag_query"],
                ["tool", "rag_delete_collection"],
                ["tool", "rag_list_collections"],
                ["tool", "rag_subscription_create"],
                ["tool", "rag_subscription_list"],
                ["tool", "rag_subscription_get"],
                ["tool", "rag_subscription_delete"],
                ["tool", "schedule_task_cancel"],
                ["tool", "schedule_task"],
                ["tool", "schedule_task_once"],
                ["tool", "schedule_tasks_list"],
                ["tool", "kill_shell"],
                ["tool", "conversation_index"]
            ]
        }"#;

        let status = ProjectStatus::from_json(json).expect("Failed to parse status");

        // These tools have NO agent assignments (2-element tags)
        let unassigned_tools = vec![
            "rag_create_collection",
            "rag_add_documents",
            "rag_query",
            "rag_delete_collection",
            "rag_list_collections",
            "rag_subscription_create",
            "rag_subscription_list",
            "rag_subscription_get",
            "rag_subscription_delete",
            "schedule_task_cancel",
            "schedule_task",
            "schedule_task_once",
            "schedule_tasks_list",
            "kill_shell",
            "conversation_index",
        ];

        // ALL unassigned tools MUST be in all_tools
        for tool in &unassigned_tools {
            assert!(
                status.all_tools.contains(&tool.to_string()),
                "Tool '{}' missing from all_tools",
                tool
            );
        }

        // Assigned tools should also be there
        assert!(status.all_tools.contains(&"Read".to_string()));
        assert!(status.all_tools.contains(&"Write".to_string()));
        assert!(status.all_tools.contains(&"Bash".to_string()));
    }

    #[test]
    fn test_real_user_event_parsing() {
        // This is the EXACT event from the user's bug report
        // It contains 128 tool tags total (verified with jq)
        let json = include_str!("../../tests/fixtures/real_status_event_128_tools.json");

        let status = ProjectStatus::from_json(json).expect("Failed to parse real event");

        // Use helper to assert must-have unassigned tools (the ones that were missing in the bug)
        assert_must_have_unassigned_tools(&status.all_tools);

        // Assert we have a good number of tools (at least the must-haves)
        assert!(
            status.all_tools.len() >= 100,
            "Expected at least 100 tools from real event"
        );
    }

    /// 24010 exposes project-level tool availability, but does not define
    /// current per-agent tool configuration. Per-agent config comes from 34011.
    #[test]
    fn test_24010_tools_are_project_level_only() {
        let json = r#"{
            "kind": 24010,
            "pubkey": "backend_pubkey",
            "created_at": 1706400000,
            "tags": [
                ["a", "31933:user_pubkey:project_id"],
                ["agent", "agent1_pk", "agent1"],
                ["tool", "Read", "agent1"],
                ["tool", "Write", "agent1"],
                ["tool", "Bash", "agent1"],
                ["tool", "rag_create_collection"],
                ["tool", "rag_query"],
                ["tool", "schedule_task"]
            ]
        }"#;

        let status = ProjectStatus::from_json(json).unwrap();

        // 24010 assignments are not interpreted as current per-agent config.
        let assigned = status.agent_assigned_tools();
        assert!(
            assigned.is_empty(),
            "24010 must not set current agent tools"
        );

        // all_tools() should return ALL tools (assigned + unassigned)
        let all = status.all_tools();
        assert_eq!(all.len(), 6, "Should have 6 total tools");
        assert!(all.contains(&"Read"));
        assert!(all.contains(&"Write"));
        assert!(all.contains(&"Bash"));
        assert!(all.contains(&"rag_create_collection"));
        assert!(all.contains(&"rag_query"));
        assert!(all.contains(&"schedule_task"));

        // Critical assertion: all_tools() still exposes every tool mentioned by
        // the heartbeat for legacy/project-level availability surfaces.
        assert!(
            all.len() > assigned.len(),
            "all_tools() must include unassigned tools"
        );
    }

    /// Integration test simulating UI layer usage
    /// This ensures UI components use the correct method to display all tools
    #[test]
    fn test_ui_layer_displays_all_tools() {
        // Simulate a realistic project status event
        let json = r#"{
            "kind": 24010,
            "pubkey": "backend_pubkey",
            "created_at": 1706400000,
            "tags": [
                ["a", "31933:user_pubkey:project_id"],
                ["agent", "claude_pk", "claude-code"],
                ["agent", "architect_pk", "architect"],
                ["tool", "Read", "claude-code"],
                ["tool", "Write", "claude-code"],
                ["tool", "Bash", "claude-code", "architect"],
                ["tool", "rag_create_collection"],
                ["tool", "rag_add_documents"],
                ["tool", "rag_query"],
                ["tool", "rag_delete_collection"],
                ["tool", "schedule_task"],
                ["tool", "kill_shell"],
                ["tool", "conversation_index"]
            ]
        }"#;

        let status = ProjectStatus::from_json(json).unwrap();

        // ✅ CORRECT: UI should use all_tools() to display all available tools
        let ui_tools = status.all_tools();

        // Verify UI sees ALL tools (both assigned and unassigned)
        assert!(ui_tools.contains(&"Read"), "UI must show Read");
        assert!(ui_tools.contains(&"Write"), "UI must show Write");
        assert!(ui_tools.contains(&"Bash"), "UI must show Bash");
        assert!(
            ui_tools.contains(&"rag_create_collection"),
            "UI must show rag_create_collection"
        );
        assert!(
            ui_tools.contains(&"rag_add_documents"),
            "UI must show rag_add_documents"
        );
        assert!(ui_tools.contains(&"rag_query"), "UI must show rag_query");
        assert!(
            ui_tools.contains(&"rag_delete_collection"),
            "UI must show rag_delete_collection"
        );
        assert!(
            ui_tools.contains(&"schedule_task"),
            "UI must show schedule_task"
        );
        assert!(ui_tools.contains(&"kill_shell"), "UI must show kill_shell");
        assert!(
            ui_tools.contains(&"conversation_index"),
            "UI must show conversation_index"
        );

        assert_eq!(ui_tools.len(), 10, "UI should display all 10 tools");

        // 24010 no longer feeds per-agent current tool config.
        assert!(status.agent_assigned_tools().is_empty());
    }

    /// Test that verifies both assigned and unassigned tools are handled correctly
    #[test]
    fn test_mixed_assigned_and_unassigned_tools() {
        let json = r#"{
            "kind": 24010,
            "pubkey": "backend_pk",
            "created_at": 1706400000,
            "tags": [
                ["a", "31933:user_pk:project_id"],
                ["agent", "agent1_pk", "agent1"],
                ["agent", "agent2_pk", "agent2"],
                ["tool", "Read", "agent1"],
                ["tool", "Write", "agent1", "agent2"],
                ["tool", "Bash", "agent2"],
                ["tool", "unassigned_tool_1"],
                ["tool", "unassigned_tool_2"],
                ["tool", "unassigned_tool_3"]
            ]
        }"#;

        let status = ProjectStatus::from_json(json).unwrap();

        // Test all_tools() returns everything
        let all = status.all_tools();
        assert_eq!(all.len(), 6, "Should have 6 total tools");

        // 24010 assignments are intentionally not treated as current
        // per-agent config.
        let assigned = status.agent_assigned_tools();
        assert!(
            assigned.is_empty(),
            "24010 must not set current agent tools"
        );

        // Verify all tools stay visible at project level while no current
        // per-agent tools are inferred.
        assert!(all.contains(&"unassigned_tool_1"));
        assert!(all.contains(&"unassigned_tool_2"));
        assert!(all.contains(&"unassigned_tool_3"));
        assert!(!assigned.contains(&"unassigned_tool_1"));
        assert!(!assigned.contains(&"unassigned_tool_2"));
        assert!(!assigned.contains(&"unassigned_tool_3"));
    }

    // Note: from_note() coverage
    // We don't have a dedicated test for from_note() because:
    // 1. It requires complex nostrdb::Note object creation (C FFI)
    // 2. from_note() simply extracts tags from Note and delegates to from_tags()
    // 3. from_tags() is thoroughly tested above (including with real event data)
    // 4. The tag extraction logic in from_note() is straightforward string/hex conversion
    // 5. Production usage validates that from_note() works correctly
    //
    // If you need to test from_note() specifically, you would need to:
    // - Set up a nostrdb instance
    // - Import an event into it
    // - Query it to get a Note reference
    // This is better suited for integration tests rather than unit tests.

    /// kind:24010 agent tags are legacy heartbeat details only. They must not
    /// create roster agents or mark the PM/default agent.
    #[test]
    fn test_24010_agent_tags_do_not_create_roster_agents() {
        let json = r#"{
            "kind": 24010,
            "pubkey": "backend_pubkey",
            "created_at": 1706400000,
            "tags": [
                ["a", "31933:user_pubkey:project_id"],
                ["agent", "agent1_pubkey", "architect", "pm"],
                ["agent", "agent2_pubkey", "claude-code"],
                ["agent", "agent3_pubkey", "researcher"]
            ]
        }"#;

        let status = ProjectStatus::from_json(json).unwrap();

        assert!(status.agents.is_empty());
        assert!(status.pm_agent().is_none());
    }

    /// Real fixture data may still contain legacy `agent` tags, but 24010 must
    /// not turn them into roster entries.
    #[test]
    fn test_real_fixture_24010_agent_tags_are_ignored_for_roster() {
        // The real fixture has: ["agent","bd2b5117...","architect-orchestrator","pm"]
        let json = include_str!("../../tests/fixtures/real_status_event_128_tools.json");

        let status = ProjectStatus::from_json(json).expect("Failed to parse real event");

        assert!(status.agents.is_empty());
        assert!(status.pm_agent().is_none());
    }

    #[test]
    fn test_skill_tags_parsing() {
        let json = r#"{
            "kind": 24010,
            "pubkey": "backend_pk",
            "created_at": 1706400000,
            "tags": [
                ["a", "31933:user_pk:project_id"],
                ["agent", "agent1_pk", "claude-code"],
                ["agent", "agent2_pk", "architect"],
                ["tool", "Read", "claude-code"],
                ["skill", "code-review", "claude-code"],
                ["skill", "testing", "claude-code", "architect"],
                ["skill", "deployment"]
            ]
        }"#;

        let status = ProjectStatus::from_json(json).unwrap();

        // all_skills() returns all skills (assigned + unassigned)
        let all = status.all_skills();
        assert_eq!(all.len(), 3);
        assert!(all.contains(&"code-review"));
        assert!(all.contains(&"testing"));
        assert!(all.contains(&"deployment"));

        // 24010 assignments are not interpreted as current per-agent config.
        let assigned = status.agent_assigned_skills();
        assert!(assigned.is_empty());

        // Verify 24010 did not create per-agent records.
        assert!(status.agents.is_empty());
    }
}
