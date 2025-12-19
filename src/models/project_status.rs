use nostrdb::Note;
use std::time::{SystemTime, UNIX_EPOCH};

/// Staleness threshold in seconds - status older than this is considered offline
const STALENESS_THRESHOLD_SECS: u64 = 5 * 60; // 5 minutes

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
    pub created_at: u64,
}

impl ProjectStatus {
    /// Create a ProjectStatus from a kind:24010 note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 24010 {
            return None;
        }

        let created_at = note.created_at();

        let mut project_coordinate: Option<String> = None;
        let mut agents: Vec<ProjectAgent> = Vec::new();
        let mut branches: Vec<String> = Vec::new();

        // First pass: collect agent tags
        let mut agent_map: std::collections::HashMap<String, ProjectAgent> =
            std::collections::HashMap::new();
        let mut is_first_agent = true;

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());

            match tag_name {
                Some("a") => {
                    if project_coordinate.is_none() {
                        project_coordinate = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                    }
                }
                Some("agent") => {
                    let agent_pubkey = tag.get(1).and_then(|t| t.variant().str());
                    let agent_name = tag.get(2).and_then(|t| t.variant().str());

                    if let (Some(pk), Some(name)) = (agent_pubkey, agent_name) {
                        let agent = ProjectAgent {
                            pubkey: pk.to_string(),
                            name: name.to_string(),
                            is_pm: is_first_agent,
                            model: None,
                            tools: Vec::new(),
                        };
                        agent_map.insert(name.to_string(), agent);
                        is_first_agent = false;
                    }
                }
                Some("branch") => {
                    if let Some(branch) = tag.get(1).and_then(|t| t.variant().str()) {
                        branches.push(branch.to_string());
                    }
                }
                _ => {}
            }
        }

        // Second pass: apply model and tool tags
        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());

            match tag_name {
                Some("model") => {
                    // ["model", <model-slug>, <agent-name>, ...]
                    let model_slug = tag.get(1).and_then(|t| t.variant().str());
                    if let Some(model) = model_slug {
                        // Apply to all agents named in positions 2+
                        for i in 2..tag.count() {
                            if let Some(agent_name) = tag.get(i).and_then(|t| t.variant().str()) {
                                if let Some(agent) = agent_map.get_mut(agent_name) {
                                    agent.model = Some(model.to_string());
                                }
                            }
                        }
                    }
                }
                Some("tool") => {
                    // ["tool", <tool-name>, <agent-name>, ...]
                    let tool_name = tag.get(1).and_then(|t| t.variant().str());
                    if let Some(tool) = tool_name {
                        for i in 2..tag.count() {
                            if let Some(agent_name) = tag.get(i).and_then(|t| t.variant().str()) {
                                if let Some(agent) = agent_map.get_mut(agent_name) {
                                    agent.tools.push(tool.to_string());
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        agents = agent_map.into_values().collect();

        let project_coordinate = project_coordinate?;

        Some(ProjectStatus {
            project_coordinate,
            agents,
            branches,
            created_at,
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

    /// Get the PM (project manager) agent
    pub fn pm_agent(&self) -> Option<&ProjectAgent> {
        self.agents.iter().find(|a| a.is_pm)
    }
}
