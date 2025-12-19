use nostrdb::Note;

/// Represents an Agent Definition (Nostr kind:4199)
#[derive(Debug, Clone)]
pub struct Agent {
    pub pubkey: String,
    pub name: String,
    // Used in UI modules (ui/views/chat.rs and ui/views/threads.rs)
    #[allow(dead_code)]
    pub model: Option<String>,
}

impl Agent {
    /// Create an Agent from a kind:4199 note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 4199 {
            return None;
        }

        let pubkey = hex::encode(note.pubkey());

        let mut name: Option<String> = None;
        let mut model: Option<String> = None;

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            let tag_value = tag.get(1).and_then(|t| t.variant().str());

            match tag_name {
                Some("title") | Some("name") => {
                    if name.is_none() {
                        name = tag_value.map(|s| s.to_string());
                    }
                }
                Some("model") => {
                    model = tag_value.map(|s| s.to_string());
                }
                _ => {}
            }
        }

        Some(Agent {
            pubkey,
            name: name.unwrap_or_default(),
            model,
        })
    }

    /// Get display name (name or truncated pubkey)
    pub fn display_name(&self) -> &str {
        if self.name.is_empty() {
            &self.pubkey[..8.min(self.pubkey.len())]
        } else {
            &self.name
        }
    }
}
