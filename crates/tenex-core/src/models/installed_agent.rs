/// An agent available for installation from a backend (kind:24011 catalog).
#[derive(Debug, Clone, uniffi::Record, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct InstalledAgent {
    pub backend_pubkey: String,
    pub pubkey: String,
    pub slug: String,
    pub created_at: u64,
}

/// One backend entry for an agent in the approved kind:24011 inventory.
#[derive(Debug, Clone, uniffi::Record, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AgentInventoryBackend {
    pub backend_pubkey: String,
    pub slug: String,
    pub created_at: u64,
}

/// Grouped agent inventory across approved backends, preserving provenance.
#[derive(Debug, Clone, uniffi::Record, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct AgentInventoryItem {
    pub pubkey: String,
    pub slug: String,
    pub backends: Vec<AgentInventoryBackend>,
    pub is_multi_backend: bool,
}

/// One backend's kind:24011 inventory: agents that can be installed plus the
/// model slugs the backend currently exposes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendInventory {
    pub backend_pubkey: String,
    pub created_at: u64,
    pub agents: Vec<InstalledAgent>,
    pub models: Vec<String>,
}

impl InstalledAgent {
    /// Parse a kind:24011 catalog event into a `BackendInventory`.
    ///
    /// Tag shape:
    /// - `["agent", "<pubkey>", "<slug>"]` for each installable agent
    /// - `["model", "<slug>"]` for each model the backend advertises
    pub fn from_value(event: &serde_json::Value) -> Option<BackendInventory> {
        let kind = event.get("kind")?.as_u64()?;
        if kind != 24011 {
            return None;
        }

        let backend_pubkey = event.get("pubkey")?.as_str()?.to_string();
        let created_at = event.get("created_at")?.as_u64().unwrap_or(0);

        let mut agents: Vec<InstalledAgent> = Vec::new();
        let mut models: Vec<String> = Vec::new();

        for tag in event.get("tags")?.as_array()? {
            let Some(arr) = tag.as_array() else {
                continue;
            };
            let Some(label) = arr.first().and_then(|v| v.as_str()) else {
                continue;
            };
            match label {
                "agent" => {
                    if arr.len() < 3 {
                        continue;
                    }
                    let Some(pubkey) = arr[1].as_str() else {
                        continue;
                    };
                    let Some(slug) = arr[2].as_str() else {
                        continue;
                    };
                    agents.push(InstalledAgent {
                        backend_pubkey: backend_pubkey.clone(),
                        pubkey: pubkey.to_string(),
                        slug: slug.to_string(),
                        created_at,
                    });
                }
                "model" => {
                    if arr.len() < 2 {
                        continue;
                    }
                    let Some(slug) = arr[1].as_str() else {
                        continue;
                    };
                    if !slug.is_empty() {
                        models.push(slug.to_string());
                    }
                }
                _ => {}
            }
        }

        models.sort();
        models.dedup();

        Some(BackendInventory {
            backend_pubkey,
            created_at,
            agents,
            models,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_agents_and_models() {
        let event = json!({
            "kind": 24011,
            "pubkey": "backend_pk",
            "created_at": 1_700_000_000,
            "tags": [
                ["agent", "agent_pk_1", "planner"],
                ["agent", "agent_pk_2", "coder"],
                ["model", "opus"],
                ["model", "sonnet"],
                ["model", "opus"],
            ]
        });
        let inv = InstalledAgent::from_value(&event).expect("parses");
        assert_eq!(inv.backend_pubkey, "backend_pk");
        assert_eq!(inv.created_at, 1_700_000_000);
        assert_eq!(inv.agents.len(), 2);
        assert_eq!(inv.agents[0].slug, "planner");
        assert_eq!(inv.models, vec!["opus", "sonnet"]);
    }

    #[test]
    fn empty_inventory_is_valid() {
        let event = json!({
            "kind": 24011,
            "pubkey": "backend_pk",
            "created_at": 1,
            "tags": []
        });
        let inv = InstalledAgent::from_value(&event).expect("parses");
        assert!(inv.agents.is_empty());
        assert!(inv.models.is_empty());
    }

    #[test]
    fn rejects_wrong_kind() {
        let event = json!({"kind": 24010, "pubkey": "x", "created_at": 1, "tags": []});
        assert!(InstalledAgent::from_value(&event).is_none());
    }

    #[test]
    fn ignores_malformed_tags() {
        let event = json!({
            "kind": 24011,
            "pubkey": "backend_pk",
            "created_at": 1,
            "tags": [
                ["agent"],                    // missing pubkey/slug
                ["agent", "pk"],              // missing slug
                ["model"],                    // missing slug
                ["model", ""],                // empty slug
                ["model", "ok"],
                ["agent", "pk_ok", "ok-slug"],
            ]
        });
        let inv = InstalledAgent::from_value(&event).expect("parses");
        assert_eq!(inv.agents.len(), 1);
        assert_eq!(inv.agents[0].slug, "ok-slug");
        assert_eq!(inv.models, vec!["ok"]);
    }
}
