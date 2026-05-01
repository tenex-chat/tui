/// An agent available for installation from a backend (kind:24011 catalog).
#[derive(Debug, Clone, uniffi::Record, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct InstalledAgent {
    pub backend_pubkey: String,
    pub pubkey: String,
    pub slug: String,
    pub created_at: u64,
}

impl InstalledAgent {
    /// Parse a kind:24011 catalog event.
    ///
    /// Returns `(backend_pubkey, agents)` or `None` if the event is not a valid kind:24011.
    pub fn from_value(event: &serde_json::Value) -> Option<(String, Vec<InstalledAgent>)> {
        let kind = event.get("kind")?.as_u64()?;
        if kind != 24011 {
            return None;
        }

        let backend_pubkey = event.get("pubkey")?.as_str()?.to_string();
        let created_at = event.get("created_at")?.as_u64().unwrap_or(0);

        let agents = event
            .get("tags")?
            .as_array()?
            .iter()
            .filter_map(|tag| {
                let arr = tag.as_array()?;
                if arr.len() < 3 {
                    return None;
                }
                let label = arr[0].as_str()?;
                if label != "agent" {
                    return None;
                }
                let pubkey = arr[1].as_str()?.to_string();
                let slug = arr[2].as_str()?.to_string();
                Some(InstalledAgent {
                    backend_pubkey: backend_pubkey.clone(),
                    pubkey,
                    slug,
                    created_at,
                })
            })
            .collect();

        Some((backend_pubkey, agents))
    }
}
