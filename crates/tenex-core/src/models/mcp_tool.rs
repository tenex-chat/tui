use nostrdb::Note;

/// MCP Tool Definition - kind:4200 events describing MCP servers/tools
#[derive(Debug, Clone)]
pub struct MCPTool {
    pub id: String,
    pub pubkey: String,
    pub d_tag: String,
    pub name: String,
    pub description: String,
    pub server_url: Option<String>,
    pub version: Option<String>,
    pub created_at: u64,
}

impl MCPTool {
    /// Parse an MCPTool from a kind:4200 note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 4200 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let description = note.content().to_string();
        let created_at = note.created_at();

        let mut d_tag = None;
        let mut name = None;
        let mut server_url = None;
        let mut version = None;

        for tag in note.tags() {
            if tag.count() >= 2 {
                if let (Some(tag_name), Some(value)) = (
                    tag.get(0).and_then(|t| t.variant().str()),
                    tag.get(1).and_then(|t| t.variant().str())
                ) {
                    match tag_name {
                        "d" => d_tag = Some(value.to_string()),
                        "title" | "name" => name = Some(value.to_string()),
                        "server" | "url" => server_url = Some(value.to_string()),
                        "version" => version = Some(value.to_string()),
                        _ => {}
                    }
                }
            }
        }

        Some(MCPTool {
            id,
            pubkey,
            d_tag: d_tag.unwrap_or_default(),
            name: name.unwrap_or_else(|| "Unnamed Tool".to_string()),
            description,
            server_url,
            version,
            created_at,
        })
    }

    pub fn description_preview(&self, max_chars: usize) -> String {
        if self.description.len() <= max_chars {
            self.description.clone()
        } else {
            format!("{}...", &self.description[..max_chars.saturating_sub(3)])
        }
    }
}
