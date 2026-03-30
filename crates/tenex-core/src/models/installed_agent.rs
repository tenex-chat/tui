#[derive(Debug, Clone, uniffi::Record, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
pub struct InstalledAgent {
    pub backend_pubkey: String,
    pub pubkey: String,
    pub slug: String,
    pub created_at: u64,
}

impl InstalledAgent {
    pub fn from_value(event: &serde_json::Value) -> Option<(String, Vec<Self>)> {
        let kind = event.get("kind")?.as_u64()?;
        if kind != 24011 {
            return None;
        }

        let backend_pubkey = event.get("pubkey")?.as_str()?.to_string();
        let created_at = event.get("created_at")?.as_u64()?;
        let tags = event.get("tags")?.as_array()?;

        let mut installed_agents = Vec::new();
        for tag in tags {
            let Some(parts) = tag.as_array() else {
                continue;
            };
            if parts.first().and_then(|value| value.as_str()) != Some("agent") {
                continue;
            }

            let Some(pubkey) = parts.get(1).and_then(|value| value.as_str()) else {
                continue;
            };
            let Some(slug) = parts.get(2).and_then(|value| value.as_str()) else {
                continue;
            };

            installed_agents.push(InstalledAgent {
                backend_pubkey: backend_pubkey.clone(),
                pubkey: pubkey.to_string(),
                slug: slug.to_string(),
                created_at,
            });
        }

        installed_agents.sort_by(|left, right| {
            left.slug
                .cmp(&right.slug)
                .then_with(|| left.pubkey.cmp(&right.pubkey))
        });

        Some((backend_pubkey, installed_agents))
    }
}
