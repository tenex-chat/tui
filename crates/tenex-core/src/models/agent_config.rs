//! Per-agent configuration event (Nostr kind:0, NIP-01 metadata).
//!
//! Each agent publishes its own kind:0 event, signed by the agent's own
//! key. The event's `content` carries the standard NIP-01 metadata JSON
//! (`name`, `about`, …); the tags enumerate every model/skill/mcp visible
//! to the agent. The currently-active selection is marked with `"active"`
//! as the tag's third element (inactive entries omit it).
//!
//! Example tag order:
//!
//! ```text
//! ["slug", "<agent_slug>"]                           // human-friendly slug
//! ["use-criteria", "<text>"]                         // when to pick this agent
//! ["p", "<backend_pubkey>"]                          // backend that runs this agent
//! ["model", "opus", "active"]                        // currently-selected model
//! ["model", "sonnet"]                                // other available models
//! ["skill", "read-access", "active"]                 // enabled skill
//! ["skill", "shell"]                                 // visible but inactive skill
//! ["mcp", "github", "active"]                        // mcp server in mcpAccess
//! ["mcp", "linear"]                                  // configured but inactive mcp
//! ```
//!
//! Agent identity = the event's signer (`pubkey` field). Slug = the
//! `["slug", ...]` tag. The first `p` tag carries the backend pubkey
//! that hosts this agent (traceability only — not identity).

use nostrdb::Note;

/// Per-agent configuration derived from a kind:0 event.
#[derive(Debug, Clone, uniffi::Record, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AgentConfig {
    /// Hex-encoded public key of the agent — the event signer.
    pub pubkey: String,
    /// Human-friendly slug for the agent (the `["slug", ...]` tag).
    pub slug: String,
    /// Hex-encoded public key of the backend that runs this agent, sourced
    /// from the first `["p", "<backend_pubkey>"]` tag on the event. Optional
    /// because the tag may be absent on malformed events.
    pub backend_pubkey: Option<String>,
    /// Free-form text describing when to pick this agent, sourced from the
    /// `["use-criteria", "<text>"]` tag. Optional.
    pub use_criteria: Option<String>,
    /// Unix timestamp the event was created.
    pub created_at: u64,
    /// Currently-selected model slug, if any model is active.
    pub active_model: Option<String>,
    /// Every available model slug (includes `active_model`).
    pub models: Vec<String>,
    /// Enabled tool IDs.
    pub active_tools: Vec<String>,
    /// Every visible tool ID (includes `active_tools`).
    pub tools: Vec<String>,
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
    /// Returns `None` when the event is not a kind:0 or when the required
    /// `slug` tag is missing.
    pub fn from_value(event: &serde_json::Value) -> Option<Self> {
        let kind = event.get("kind")?.as_u64()?;
        if kind != 0 {
            return None;
        }

        let pubkey = event.get("pubkey")?.as_str()?.to_string();
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

        Self::from_tags(created_at, tags, pubkey)
    }

    /// Parse an `AgentConfig` from a nostrdb `Note`.
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 0 {
            return None;
        }

        let pubkey = hex::encode(note.pubkey());
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

        Self::from_tags(note.created_at(), tags, pubkey)
    }

    fn from_tags(created_at: u64, tags: Vec<Vec<String>>, pubkey: String) -> Option<Self> {
        let mut slug: Option<String> = None;
        let mut backend_pubkey: Option<String> = None;
        let mut use_criteria: Option<String> = None;
        let mut active_model: Option<String> = None;
        let mut models: Vec<String> = Vec::new();
        let mut active_tools: Vec<String> = Vec::new();
        let mut tools: Vec<String> = Vec::new();
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
                "slug" => {
                    if slug.is_none() {
                        if let Some(s) = tag.get(1) {
                            if !s.is_empty() {
                                slug = Some(s.clone());
                            }
                        }
                    }
                }
                "use-criteria" => {
                    if use_criteria.is_none() {
                        if let Some(s) = tag.get(1) {
                            if !s.is_empty() {
                                use_criteria = Some(s.clone());
                            }
                        }
                    }
                }
                "p" => {
                    // First `p` tag carries the backend pubkey that hosts this
                    // agent. Subsequent `p` tags (if any) are ignored here.
                    if backend_pubkey.is_none() {
                        if let Some(pk) = tag.get(1) {
                            if !pk.is_empty() {
                                backend_pubkey = Some(pk.clone());
                            }
                        }
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
                "tool" => {
                    if let Some(id) = tag.get(1) {
                        if !id.is_empty() {
                            let is_active = tag.get(2).map(String::as_str) == Some("active");
                            if is_active {
                                active_tools.push(id.clone());
                            }
                            tools.push(id.clone());
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
        active_tools.sort();
        active_tools.dedup();
        tools.sort();
        tools.dedup();
        active_skills.sort();
        active_skills.dedup();
        skills.sort();
        skills.dedup();
        active_mcps.sort();
        active_mcps.dedup();
        mcps.sort();
        mcps.dedup();

        Some(AgentConfig {
            pubkey,
            slug: slug?,
            backend_pubkey,
            use_criteria,
            created_at,
            active_model,
            models,
            active_tools,
            tools,
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
        // Under the new design the agent signs the event, so `pubkey` ==
        // agent pubkey. The first `p` tag carries the backend pubkey.
        let event = json!({
            "kind": 0,
            "pubkey": "agent_pk",
            "created_at": 1_700_000_000,
            "tags": [
                ["slug", "planner"],
                ["use-criteria", "Pick when planning multi-step work"],
                ["p", "backend_pk"],
                ["p", "extra_pk"],
                ["model", "opus", "active"],
                ["model", "sonnet"],
                ["tool", "shell", "active"],
                ["tool", "web-search"],
                ["skill", "read-access", "active"],
                ["skill", "shell"],
                ["skill", "write-access", "active"],
                ["mcp", "github", "active"],
                ["mcp", "linear"],
            ]
        });

        let config = AgentConfig::from_value(&event).expect("should parse");
        assert_eq!(config.pubkey, "agent_pk");
        assert_eq!(config.slug, "planner");
        assert_eq!(config.backend_pubkey.as_deref(), Some("backend_pk"));
        assert_eq!(
            config.use_criteria.as_deref(),
            Some("Pick when planning multi-step work")
        );
        assert_eq!(config.active_model.as_deref(), Some("opus"));
        assert_eq!(config.models, vec!["opus", "sonnet"]);
        assert_eq!(config.active_tools, vec!["shell"]);
        assert_eq!(config.tools, vec!["shell", "web-search"]);
        assert_eq!(config.active_skills, vec!["read-access", "write-access"]);
        assert_eq!(config.skills, vec!["read-access", "shell", "write-access"]);
        assert_eq!(config.active_mcps, vec!["github"]);
        assert_eq!(config.mcps, vec!["github", "linear"]);
    }

    #[test]
    fn backend_pubkey_is_none_when_no_p_tag() {
        let event = json!({
            "kind": 0,
            "pubkey": "agent_pk",
            "created_at": 1,
            "tags": [
                ["slug", "planner"],
                ["model", "opus", "active"],
            ]
        });
        let config = AgentConfig::from_value(&event).expect("should parse");
        assert_eq!(config.pubkey, "agent_pk");
        assert!(config.backend_pubkey.is_none());
        assert!(config.use_criteria.is_none());
    }

    #[test]
    fn ignores_wrong_kind() {
        let event = json!({
            "kind": 24010,
            "pubkey": "backend_pk",
            "created_at": 1,
            "tags": [["slug", "planner"]]
        });
        assert!(AgentConfig::from_value(&event).is_none());
    }

    #[test]
    fn requires_slug_tag() {
        let event = json!({
            "kind": 0,
            "pubkey": "agent_pk",
            "created_at": 1,
            "tags": [["p", "backend_pk"]]
        });
        assert!(AgentConfig::from_value(&event).is_none());
    }

    #[test]
    fn does_not_treat_p_tag_as_agent_identity() {
        // The agent pubkey must come from the event's `pubkey` field (signer);
        // the first `p` tag is captured as the backend pubkey.
        let event = json!({
            "kind": 0,
            "pubkey": "real_agent_pk",
            "created_at": 1,
            "tags": [
                ["slug", "planner"],
                ["p", "backend_pk"],
            ]
        });
        let config = AgentConfig::from_value(&event).expect("should parse");
        assert_eq!(config.pubkey, "real_agent_pk");
        assert_eq!(config.backend_pubkey.as_deref(), Some("backend_pk"));
    }
}
