use nostrdb::Note;

#[derive(Debug, Clone)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub content: String,
    pub pubkey: String,
    /// Most recent activity (thread creation or latest reply)
    pub last_activity: u64,
}

impl Thread {
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 11 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let created_at = note.created_at();

        let mut title: Option<String> = None;

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            match tag_name {
                Some("a") => {
                    // Validate project_id exists
                    if tag.get(1).and_then(|t| t.variant().str()).is_none() {
                        return None;
                    }
                }
                Some("title") => {
                    title = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                _ => {}
            }
        }

        let content = note.content().to_string();

        Some(Thread {
            id,
            title: title.unwrap_or_else(|| "Untitled".to_string()),
            content,
            pubkey,
            last_activity: created_at,
        })
    }
}
