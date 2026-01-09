use nostrdb::Note;

/// Agent Definition - kind:4199 events describing AI agents
#[derive(Debug, Clone)]
pub struct AgentDefinition {
    pub id: String,
    pub pubkey: String,
    pub d_tag: String,
    pub name: String,
    pub description: String,
    pub role: String,
    pub instructions: String,
    pub picture: Option<String>,
    pub version: Option<String>,
    pub model: Option<String>,
    pub tools: Vec<String>,
    pub mcp_servers: Vec<String>,
    pub use_criteria: Vec<String>,
    pub created_at: u64,
}

impl AgentDefinition {
    /// Parse an AgentDefinition from a kind:4199 note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 4199 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let instructions = note.content().to_string();
        let created_at = note.created_at();

        let mut d_tag: Option<String> = None;
        let mut name: Option<String> = None;
        let mut description: Option<String> = None;
        let mut role: Option<String> = None;
        let mut picture: Option<String> = None;
        let mut version: Option<String> = None;
        let mut model: Option<String> = None;
        let mut tools: Vec<String> = Vec::new();
        let mut mcp_servers: Vec<String> = Vec::new();
        let mut use_criteria: Vec<String> = Vec::new();

        for tag in note.tags() {
            if tag.count() >= 2 {
                if let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) {
                    if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                        match tag_name {
                            "d" => d_tag = Some(value.to_string()),
                            "title" => name = Some(value.to_string()),
                            "description" => description = Some(value.to_string()),
                            "role" => role = Some(value.to_string()),
                            "picture" | "image" => picture = Some(value.to_string()),
                            "version" => version = Some(value.to_string()),
                            "model" => model = Some(value.to_string()),
                            "tool" => tools.push(value.to_string()),
                            "mcp" => mcp_servers.push(value.to_string()),
                            "use-criteria" => use_criteria.push(value.to_string()),
                            _ => {}
                        }
                    }
                }
            }
        }

        Some(AgentDefinition {
            id,
            pubkey,
            d_tag: d_tag.unwrap_or_default(),
            name: name.unwrap_or_else(|| "Unnamed Agent".to_string()),
            description: description.unwrap_or_default(),
            role: role.unwrap_or_else(|| "assistant".to_string()),
            instructions,
            picture,
            version,
            model,
            tools,
            mcp_servers,
            use_criteria,
            created_at,
        })
    }

    /// Get a short preview of the description
    pub fn description_preview(&self, max_chars: usize) -> String {
        if self.description.len() <= max_chars {
            self.description.clone()
        } else {
            format!("{}...", &self.description[..max_chars.saturating_sub(3)])
        }
    }
}
