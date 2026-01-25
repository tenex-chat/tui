use nostrdb::Note;

use crate::constants::DEFAULT_NUDGE_TITLE;

/// Nudge - kind:4201 events for agent nudges/prompts
#[derive(Debug, Clone)]
pub struct Nudge {
    pub id: String,
    pub pubkey: String,
    pub title: String,
    pub description: String,
    pub content: String,
    pub hashtags: Vec<String>,
    pub created_at: u64,
    /// Tools to add to agent's available tools (allow-tool tags)
    pub allowed_tools: Vec<String>,
    /// Tools to remove from agent's available tools (deny-tool tags)
    pub denied_tools: Vec<String>,
    /// ID of the nudge this one supersedes (supersedes tag)
    pub supersedes: Option<String>,
}

impl Nudge {
    /// Parse a Nudge from a kind:4201 note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 4201 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        let mut title: Option<String> = None;
        let mut description: Option<String> = None;
        let mut hashtags: Vec<String> = Vec::new();
        let mut allowed_tools: Vec<String> = Vec::new();
        let mut denied_tools: Vec<String> = Vec::new();
        let mut supersedes: Option<String> = None;

        for tag in note.tags() {
            if tag.count() >= 2 {
                if let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) {
                    if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                        match tag_name {
                            "title" => title = Some(value.to_string()),
                            "description" => description = Some(value.to_string()),
                            "t" => hashtags.push(value.to_string()),
                            "allow-tool" => allowed_tools.push(value.to_string()),
                            "deny-tool" => denied_tools.push(value.to_string()),
                            "supersedes" => supersedes = Some(value.to_string()),
                            _ => {}
                        }
                    }
                }
            }
        }

        Some(Nudge {
            id,
            pubkey,
            title: title.unwrap_or_else(|| DEFAULT_NUDGE_TITLE.to_string()),
            description: description.unwrap_or_default(),
            content,
            hashtags,
            created_at,
            allowed_tools,
            denied_tools,
            supersedes,
        })
    }

    /// Get a short preview of the content
    pub fn content_preview(&self, max_chars: usize) -> String {
        if self.content.len() <= max_chars {
            self.content.clone()
        } else {
            format!("{}...", &self.content[..max_chars.saturating_sub(3)])
        }
    }
}
