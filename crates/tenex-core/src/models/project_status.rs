use nostrdb::Note;
use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::constants::STALENESS_THRESHOLD_SECS;

/// Represents an agent listed in a project status event (kind:24010).
///
/// `is_pm` is set by the store layer based on the agent's position in the
/// kind:31933 project event — the first `p`-tag agent is the PM.
/// Model/skill/mcp/tool configuration lives in per-agent kind:34011
/// events — use `AgentConfig` to retrieve those.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProjectAgent {
    pub pubkey: String,
    pub name: String,
    pub backend_pubkey: String,
    pub is_pm: bool,
    pub is_online: bool,
}

/// Represents a TENEX project status (Nostr kind:24010).
///
/// Carries the project coordinate, each active agent's identity, and the set of
/// active worktrees. Per-agent configuration (model, skills, mcp servers) is
/// delivered separately via kind:34011.
#[derive(Debug, Clone)]
pub struct ProjectStatus {
    pub project_coordinate: String,
    pub agents: Vec<ProjectAgent>,
    pub branches: Vec<String>,
    pub created_at: u64,
    /// The pubkey of the backend that published this status event.
    pub backend_pubkey: String,
    /// When this status was last seen by the client (seconds since UNIX epoch).
    pub last_seen_at: u64,
}

impl ProjectStatus {
    fn agent_aggregation_key(agent: &ProjectAgent) -> String {
        if !agent.name.is_empty() {
            return agent.name.clone();
        }
        agent.pubkey.clone()
    }

    /// Create from JSON string (for ephemeral events received via DataChange channel).
    pub fn from_json(json: &str) -> Option<Self> {
        let event: serde_json::Value = serde_json::from_str(json).ok()?;
        Self::from_value(&event)
    }

    /// Create from pre-parsed `serde_json::Value` (avoids double parsing).
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

    /// Create from `nostrdb::Note`.
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 24010 {
            return None;
        }

        let backend_pubkey = hex::encode(note.pubkey());

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

    fn from_tags(created_at: u64, tags: Vec<Vec<String>>, backend_pubkey: String) -> Option<Self> {
        let mut project_coordinate: Option<String> = None;
        let mut agent_map: HashMap<String, ProjectAgent> = HashMap::new();
        let mut branches: Vec<String> = Vec::new();

        for tag in &tags {
            let name = match tag.first() {
                Some(name) => name.as_str(),
                None => continue,
            };
            match name {
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
                            backend_pubkey: backend_pubkey.clone(),
                            is_pm: false,
                            is_online: true,
                        };
                        agent_map.insert(tag[2].clone(), agent);
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

        let project_coordinate = project_coordinate?;
        let agents = agent_map.into_values().collect();

        Some(ProjectStatus {
            project_coordinate,
            agents,
            branches,
            created_at,
            backend_pubkey,
            last_seen_at: created_at,
        })
    }

    /// Whether this status is considered online (not stale).
    pub fn is_online(&self) -> bool {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        now.saturating_sub(self.last_seen_at) < STALENESS_THRESHOLD_SECS
    }

    /// The default branch (first in the `branches` array).
    pub fn default_branch(&self) -> Option<&str> {
        self.branches.first().map(|s| s.as_str())
    }

    /// Get the PM (project manager) agent, if one is marked.
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

        let mut agents: Vec<ProjectAgent> =
            agent_map.into_values().map(|(_, _, agent)| agent).collect();
        agents.sort_by(|a, b| a.name.cmp(&b.name));

        Some(Self {
            project_coordinate,
            agents,
            branches,
            created_at: newest_created_at,
            backend_pubkey: newest_backend_pubkey,
            last_seen_at: newest_last_seen_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_aggregate_deduplicates_agents_by_slug_and_prefers_fresher_backend() {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let older_status = ProjectStatus {
            project_coordinate: "31933:user:project".to_string(),
            agents: vec![ProjectAgent {
                pubkey: String::new(),
                name: "agent1".to_string(),
                backend_pubkey: "backend-old".to_string(),
                is_pm: false,
                is_online: true,
            }],
            branches: Vec::new(),
            created_at: now,
            backend_pubkey: "backend-old".to_string(),
            last_seen_at: now.saturating_sub(5),
        };
        let fresher_status = ProjectStatus {
            project_coordinate: "31933:user:project".to_string(),
            agents: vec![ProjectAgent {
                pubkey: "agent1-pubkey".to_string(),
                name: "agent1".to_string(),
                backend_pubkey: "backend-fresh".to_string(),
                is_pm: false,
                is_online: true,
            }],
            branches: Vec::new(),
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
        assert_eq!(aggregate.agents[0].backend_pubkey, "backend-fresh");
    }

    /// The new protocol only carries `a`, `p`, `agent`, `branch`, and
    /// `scheduled-task` tags on kind:24010 — per-agent `model`/`tool`/`skill`/
    /// `mcp` tags were removed and must be ignored if a legacy backend still
    /// emits them.
    #[test]
    fn test_ignores_legacy_per_agent_tags() {
        let json = r#"{
            "kind": 24010,
            "pubkey": "backend_pk",
            "created_at": 1706400000,
            "tags": [
                ["a", "31933:user_pk:project_id"],
                ["p", "owner_pk"],
                ["agent", "agent1_pk", "agent1"],
                ["agent", "agent2_pk", "agent2"],
                ["branch", "main"],
                ["model", "opus", "agent1"],
                ["tool", "Read", "agent1"],
                ["skill", "code-review", "agent1"],
                ["mcp", "github", "agent1"]
            ]
        }"#;

        let status = ProjectStatus::from_json(json).expect("should parse");

        assert_eq!(status.project_coordinate, "31933:user_pk:project_id");
        assert_eq!(status.agents.len(), 2);
        assert_eq!(status.branches, vec!["main"]);

        let names: Vec<&str> = status.agents.iter().map(|a| a.name.as_str()).collect();
        assert!(names.contains(&"agent1"));
        assert!(names.contains(&"agent2"));
    }

}
