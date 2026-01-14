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
    pub created_at: u64,
}

impl ProjectStatus {
    /// Create from nostrdb::Note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 24010 {
            return None;
        }

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

        Self::from_tags(note.created_at(), tags)
    }

    /// Common parsing logic for tags
    fn from_tags(created_at: u64, tags: Vec<Vec<String>>) -> Option<Self> {
        let mut project_coordinate: Option<String> = None;
        let mut agent_map: HashMap<String, ProjectAgent> = HashMap::new();
        let mut branches: Vec<String> = Vec::new();
        let mut is_first_agent = true;

        // First pass: collect project coordinate, agents, and branches
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
                _ => {}
            }
        }

        // Second pass: apply model and tool tags
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

    /// All unique models used by agents
    pub fn models(&self) -> Vec<&str> {
        let mut models: Vec<&str> = self
            .agents
            .iter()
            .filter_map(|a| a.model.as_deref())
            .collect();
        models.sort();
        models.dedup();
        models
    }

    /// Get the PM (project manager) agent
    pub fn pm_agent(&self) -> Option<&ProjectAgent> {
        self.agents.iter().find(|a| a.is_pm)
    }
}
