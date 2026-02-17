use nostrdb::Note;

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub pubkey: String,
    pub participants: Vec<String>,
    pub agent_ids: Vec<String>, // Agent definition event IDs (kind 4199)
    pub mcp_tool_ids: Vec<String>, // MCP tool event IDs (kind 4200)
    pub created_at: u64,
}

impl Project {
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 31933 {
            return None;
        }

        let pubkey = hex::encode(note.pubkey());

        let mut id: Option<String> = None;
        let mut title: Option<String> = None;
        let mut name: Option<String> = None;
        let mut participants = Vec::new();
        let mut agent_ids = Vec::new();
        let mut mcp_tool_ids = Vec::new();

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            match tag_name {
                Some("d") => {
                    id = tag
                        .get(1)
                        .and_then(|t| t.variant().str())
                        .map(|s| s.to_string());
                }
                Some("title") => {
                    title = tag
                        .get(1)
                        .and_then(|t| t.variant().str())
                        .map(|s| s.to_string());
                }
                Some("name") => {
                    name = tag
                        .get(1)
                        .and_then(|t| t.variant().str())
                        .map(|s| s.to_string());
                }
                Some("p") => {
                    if let Some(p) = tag.get(1).and_then(|t| t.variant().str()) {
                        participants.push(p.to_string());
                    }
                }
                Some("agent") => {
                    if let Some(elem) = tag.get(1) {
                        // nostrdb stores event IDs as binary Id variant, not as strings
                        if let Some(id_bytes) = elem.variant().id() {
                            agent_ids.push(hex::encode(id_bytes));
                        } else if let Some(s) = elem.variant().str() {
                            agent_ids.push(s.to_string());
                        }
                    }
                }
                Some("mcp") => {
                    if let Some(elem) = tag.get(1) {
                        // nostrdb stores event IDs as binary Id variant, not as strings
                        if let Some(id_bytes) = elem.variant().id() {
                            mcp_tool_ids.push(hex::encode(id_bytes));
                        } else if let Some(s) = elem.variant().str() {
                            mcp_tool_ids.push(s.to_string());
                        }
                    }
                }
                _ => {}
            }
        }

        let id = id?;
        // Use title first, then name, then fall back to d-tag (same as web client)
        let display_name = title.or(name).unwrap_or_else(|| id.clone());

        Some(Project {
            id: id.clone(),
            name: display_name,
            pubkey,
            participants,
            agent_ids,
            mcp_tool_ids,
            created_at: note.created_at(),
        })
    }

    pub fn a_tag(&self) -> String {
        format!("31933:{}:{}", self.pubkey, self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_a_tag() {
        let project = Project {
            id: "proj1".to_string(),
            name: "Project 1".to_string(),
            pubkey: "a".repeat(64),
            participants: vec![],
            agent_ids: vec![],
            mcp_tool_ids: vec![],
            created_at: 0,
        };

        assert_eq!(project.a_tag(), format!("31933:{}:proj1", "a".repeat(64)));
    }
}
