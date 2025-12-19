use nostrdb::Note;

#[derive(Debug, Clone)]
pub struct ConversationMetadata {
    pub thread_id: String,
    pub title: Option<String>,
    pub created_at: u64,
}

impl ConversationMetadata {
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 513 {
            return None;
        }

        let created_at = note.created_at();

        let mut thread_id: Option<String> = None;
        let mut title: Option<String> = None;

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            match tag_name {
                Some("e") => {
                    if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        thread_id = Some(s.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        thread_id = Some(hex::encode(id_bytes));
                    }
                }
                Some("title") => {
                    title = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                _ => {}
            }
        }

        let thread_id = thread_id?;

        Some(ConversationMetadata {
            thread_id,
            title,
            created_at,
        })
    }
}
