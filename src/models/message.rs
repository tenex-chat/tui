use nostrdb::Note;

#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub content: String,
    pub pubkey: String,
    pub thread_id: String,
    pub created_at: u64,
    /// Direct parent message ID (lowercase "e" tag per NIP-22)
    /// None for thread root or messages replying directly to thread
    pub reply_to: Option<String>,
    /// Whether this is a reasoning/thinking message (has "reasoning" tag)
    pub is_reasoning: bool,
}

impl Message {
    /// Create a Message from a kind:1111 reply note
    /// Per NIP-22:
    /// - Uppercase "E" tag = root reference (the thread/conversation, kind:11)
    /// - Lowercase "e" tag = direct parent reference (for threading)
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 1111 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        let mut thread_id: Option<String> = None;
        let mut reply_to: Option<String> = None;
        let mut is_reasoning = false;

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            match tag_name {
                Some("E") => {
                    // Uppercase E = thread root reference
                    if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        thread_id = Some(s.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        thread_id = Some(hex::encode(id_bytes));
                    }
                }
                Some("e") => {
                    // Lowercase e = direct parent reference
                    if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        reply_to = Some(s.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        reply_to = Some(hex::encode(id_bytes));
                    }
                }
                Some("reasoning") => {
                    is_reasoning = true;
                }
                _ => {}
            }
        }

        let thread_id = thread_id?;

        Some(Message {
            id,
            content,
            pubkey,
            thread_id,
            created_at,
            reply_to,
            is_reasoning,
        })
    }

    /// Create a Message from a kind:11 thread root note (the thread itself is the first message)
    pub fn from_thread_note(note: &Note) -> Option<Self> {
        if note.kind() != 11 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        Some(Message {
            id: id.clone(),
            content,
            pubkey,
            thread_id: id,
            created_at,
            reply_to: None,
            is_reasoning: false,
        })
    }
}
