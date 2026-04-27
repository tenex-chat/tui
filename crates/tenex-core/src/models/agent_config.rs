//! Per-agent configuration event (Nostr kind:34011, NIP-33 addressable).
//!
//! The backend publishes one kind:34011 event per installed agent. Each event
//! enumerates every model/skill/mcp visible to the agent. The currently-active
//! selection is marked with `"active"` as the tag's third element; inactive
//! entries omit the marker.
//!
//! Example tag order:
//!
//! ```text
//! ["agent", "<agent_pubkey>", "<agent_slug>"]      // subject of this event
//! ["p", "<whitelisted_owner>"]                     // one per whitelisted owner
//! ["model", "opus", "active"]                      // currently-selected model
//! ["model", "sonnet"]                              // other available models
//! ["skill", "read-access", "active"]               // enabled, non-blocked skill
//! ["skill", "shell"]                               // visible but inactive skill
//! ["mcp", "github", "active"]                      // mcp server in mcpAccess
//! ["mcp", "linear"]                                // configured but inactive mcp
//! ```
//!
//! The agent this event describes is identified by the `agent` tag. Do NOT use
//! `p` tags for agent identity — those carry whitelisted owners only.

use nostrdb::Note;

/// Per-agent configuration derived from a kind:34011 event.
#[derive(Debug, Clone, uniffi::Record, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AgentConfig {
    /// Hex-encoded public key of the backend that published the event.
    pub backend_pubkey: String,
    /// Hex-encoded public key of the agent this event describes.
    pub pubkey: String,
    /// Human-friendly slug for the agent.
    pub slug: String,
    /// Unix timestamp the event was created.
    pub created_at: u64,
    /// Currently-selected model slug, if any model is active.
    pub active_model: Option<String>,
    /// Every available model slug (includes `active_model`).
    pub models: Vec<String>,
    /// Enabled, non-blocked skill IDs.
    pub active_skills: Vec<String>,
    /// Every visible skill ID (includes `active_skills`).
    pub skills: Vec<String>,
    /// MCP server slugs currently in `mcpAccess`.
    pub active_mcps: Vec<String>,
    /// Every configured MCP server slug (includes `active_mcps`).
    pub mcps: Vec<String>,
}

impl AgentConfig {
    /// Parse an `AgentConfig` from a JSON event value.
    ///
    /// Returns `None` when the event is not a kind:34011 or when the required
    /// `agent` tag is missing.
    pub fn from_value(event: &serde_json::Value) -> Option<Self> {
        let kind = event.get("kind")?.as_u64()?;
        if kind != 34011 {
            return None;
        }

        let backend_pubkey = event.get("pubkey")?.as_str()?.to_string();
        let created_at = event.get("created_at")?.as_u64()?;

        let tags: Vec<Vec<String>> = event
            .get("tags")?
            .as_array()?
            .iter()
            .filter_map(|tag| {
                tag.as_array().map(|arr| {
                    arr.iter()
                        .filter_map(|value| value.as_str().map(|s| s.to_string()))
                        .collect()
                })
            })
            .collect();

        Self::from_tags(created_at, tags, backend_pubkey)
    }

    /// Parse an `AgentConfig` from a nostrdb `Note`.
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 34011 {
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

    fn from_tags(
        created_at: u64,
        tags: Vec<Vec<String>>,
        backend_pubkey: String,
    ) -> Option<Self> {
        let mut agent_pubkey: Option<String> = None;
        let mut agent_slug: Option<String> = None;
        let mut active_model: Option<String> = None;
        let mut models: Vec<String> = Vec::new();
        let mut active_skills: Vec<String> = Vec::new();
        let mut skills: Vec<String> = Vec::new();
        let mut active_mcps: Vec<String> = Vec::new();
        let mut mcps: Vec<String> = Vec::new();

        for tag in &tags {
            let name = match tag.first() {
                Some(name) => name.as_str(),
                None => continue,
            };
            match name {
                "agent" => {
                    if agent_pubkey.is_none() && tag.len() >= 3 {
                        agent_pubkey = Some(tag[1].clone());
                        agent_slug = Some(tag[2].clone());
                    }
                }
                "model" => {
                    if let Some(slug) = tag.get(1) {
                        if !slug.is_empty() {
                            let is_active = tag.get(2).map(String::as_str) == Some("active");
                            if is_active {
                                active_model = Some(slug.clone());
                            }
                            models.push(slug.clone());
                        }
                    }
                }
                "skill" => {
                    if let Some(id) = tag.get(1) {
                        if !id.is_empty() {
                            let is_active = tag.get(2).map(String::as_str) == Some("active");
                            if is_active {
                                active_skills.push(id.clone());
                            }
                            skills.push(id.clone());
                        }
                    }
                }
                "mcp" => {
                    if let Some(slug) = tag.get(1) {
                        if !slug.is_empty() {
                            let is_active = tag.get(2).map(String::as_str) == Some("active");
                            if is_active {
                                active_mcps.push(slug.clone());
                            }
                            mcps.push(slug.clone());
                        }
                    }
                }
                _ => {}
            }
        }

        models.sort();
        models.dedup();
        active_skills.sort();
        active_skills.dedup();
        skills.sort();
        skills.dedup();
        active_mcps.sort();
        active_mcps.dedup();
        mcps.sort();
        mcps.dedup();

        Some(AgentConfig {
            backend_pubkey,
            pubkey: agent_pubkey?,
            slug: agent_slug?,
            created_at,
            active_model,
            models,
            active_skills,
            skills,
            active_mcps,
            mcps,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_new_per_agent_shape() {
        let event = json!({
            "kind": 34011,
            "pubkey": "backend_pk",
            "created_at": 1_700_000_000,
            "tags": [
                ["agent", "agent_pk", "planner"],
                ["p", "owner_1_pk"],
                ["p", "owner_2_pk"],
                ["model", "opus", "active"],
                ["model", "sonnet"],
                ["skill", "read-access", "active"],
                ["skill", "shell"],
                ["skill", "write-access", "active"],
                ["mcp", "github", "active"],
                ["mcp", "linear"],
            ]
        });

        let config = AgentConfig::from_value(&event).expect("should parse");
        assert_eq!(config.backend_pubkey, "backend_pk");
        assert_eq!(config.pubkey, "agent_pk");
        assert_eq!(config.slug, "planner");
        assert_eq!(config.active_model.as_deref(), Some("opus"));
        assert_eq!(config.models, vec!["opus", "sonnet"]);
        assert_eq!(config.active_skills, vec!["read-access", "write-access"]);
        assert_eq!(config.skills, vec!["read-access", "shell", "write-access"]);
        assert_eq!(config.active_mcps, vec!["github"]);
        assert_eq!(config.mcps, vec!["github", "linear"]);
    }

    #[test]
    fn ignores_wrong_kind() {
        let event = json!({
            "kind": 24010,
            "pubkey": "backend_pk",
            "created_at": 1,
            "tags": [["agent", "agent_pk", "planner"]]
        });
        assert!(AgentConfig::from_value(&event).is_none());
    }

    #[test]
    fn requires_agent_tag() {
        let event = json!({
            "kind": 34011,
            "pubkey": "backend_pk",
            "created_at": 1,
            "tags": [["p", "owner"]]
        });
        assert!(AgentConfig::from_value(&event).is_none());
    }

    #[test]
    fn ignores_d_tag_for_addressability() {
        // kind:34011 events carry a ["d", <agent_pubkey>] tag for NIP-33 addressability.
        // The parser must ignore it — agent identity still comes from the "agent" tag.
        let event = json!({
            "kind": 34011,
            "pubkey": "backend_pk",
            "created_at": 1_700_000_000,
            "tags": [
                ["d", "agent_pk"],
                ["agent", "agent_pk", "planner"],
                ["model", "sonnet", "active"],
            ]
        });
        let config = AgentConfig::from_value(&event).expect("should parse");
        assert_eq!(config.pubkey, "agent_pk");
        assert_eq!(config.slug, "planner");
        assert_eq!(config.active_model.as_deref(), Some("sonnet"));
    }

    #[test]
    fn does_not_treat_p_tag_as_agent_identity() {
        // The agent pubkey must come from the `agent` tag, not from a `p` tag.
        let event = json!({
            "kind": 34011,
            "pubkey": "backend_pk",
            "created_at": 1,
            "tags": [
                ["p", "owner_pk"],
                ["agent", "real_agent_pk", "planner"],
            ]
        });
        let config = AgentConfig::from_value(&event).expect("should parse");
        assert_eq!(config.pubkey, "real_agent_pk");
    }
}
