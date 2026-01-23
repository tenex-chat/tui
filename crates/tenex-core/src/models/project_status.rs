use nostrdb::Note;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants::STALENESS_THRESHOLD_SECS;

/// Represents an agent within a project status
#[derive(Debug, Clone)]
pub struct ProjectAgent {
    pub pubkey: String,
    pub name: String,
    pub is_pm: bool,
    pub model: Option<String>,
    pub tools: Vec<String>,
}

/// Represents a TENEX project status (Nostr kind:24010)
/// Contains online agents with their models and tools
#[derive(Debug, Clone)]
pub struct ProjectStatus {
    pub project_coordinate: String,
    pub agents: Vec<ProjectAgent>,
    pub branches: Vec<String>,
    /// All available models from model tags (including unassigned ones)
    pub all_models: Vec<String>,
    /// All available tools from tool tags (including unassigned ones)
    pub all_tools: Vec<String>,
    pub created_at: u64,
    /// The pubkey of the backend that published this status event
    pub backend_pubkey: String,
}

impl ProjectStatus {
    /// Create from JSON string (for ephemeral events received via DataChange channel)
    pub fn from_json(json: &str) -> Option<Self> {
        let event: serde_json::Value = serde_json::from_str(json).ok()?;

        let kind = event.get("kind")?.as_u64()?;
        if kind != 24010 {
            return None;
        }

        let backend_pubkey = event.get("pubkey")?.as_str()?.to_string();
        let created_at = event.get("created_at")?.as_u64()?;

        let tags_value = event.get("tags")?.as_array()?;
        let tags: Vec<Vec<String>> = tags_value.iter()
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
        let mut agent_map: HashMap<String, ProjectAgent> = HashMap::new();
        let mut branches: Vec<String> = Vec::new();
        let mut all_models: Vec<String> = Vec::new();
        let mut all_tools: Vec<String> = Vec::new();
        let mut is_first_agent = true;

        // First pass: collect project coordinate, agents, branches, all models, and all tools
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
                "agent" => {
                    if tag.len() >= 3 {
                        let agent = ProjectAgent {
                            pubkey: tag[1].clone(),
                            name: tag[2].clone(),
                            is_pm: is_first_agent,
                            model: None,
                            tools: Vec::new(),
                        };
                        agent_map.insert(tag[2].clone(), agent);
                        is_first_agent = false;
                    }
                }
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
                _ => {}
            }
        }

        // Deduplicate and sort models and tools
        all_models.sort();
        all_models.dedup();
        all_tools.sort();
        all_tools.dedup();

        // Second pass: apply model and tool tags to agents
        for tag in &tags {
            if tag.is_empty() {
                continue;
            }

            match tag[0].as_str() {
                "model" => {
                    if tag.len() >= 3 {
                        let model = &tag[1];
                        for agent_name in &tag[2..] {
                            if let Some(agent) = agent_map.get_mut(agent_name) {
                                agent.model = Some(model.clone());
                            }
                        }
                    }
                }
                "tool" => {
                    if tag.len() >= 3 {
                        let tool = &tag[1];
                        for agent_name in &tag[2..] {
                            if let Some(agent) = agent_map.get_mut(agent_name) {
                                agent.tools.push(tool.clone());
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        let project_coordinate = project_coordinate?;
        let agents = agent_map.into_values().collect();

        Some(ProjectStatus {
            project_coordinate,
            agents,
            branches,
            all_models,
            all_tools,
            created_at,
            backend_pubkey,
        })
    }

    /// Whether this status is considered online (not stale)
    pub fn is_online(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.created_at) < STALENESS_THRESHOLD_SECS
    }

    /// The default branch (first in the branches array)
    pub fn default_branch(&self) -> Option<&str> {
        self.branches.first().map(|s| s.as_str())
    }

    /// All unique tools used by agents
    pub fn tools(&self) -> Vec<&str> {
        let mut tools: Vec<&str> = self
            .agents
            .iter()
            .flat_map(|a| a.tools.iter().map(|s| s.as_str()))
            .collect();
        tools.sort();
        tools.dedup();
        tools
    }

    /// All available models from the project (including unassigned ones)
    pub fn models(&self) -> Vec<&str> {
        self.all_models.iter().map(|s| s.as_str()).collect()
    }

    /// Get the PM (project manager) agent
    pub fn pm_agent(&self) -> Option<&ProjectAgent> {
        self.agents.iter().find(|a| a.is_pm)
    }
}
