use crate::store::events::StoredEvent;

#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub content: String,
    pub pubkey: String,
    pub thread_id: String,
    pub created_at: u64,
}

impl Message {
    pub fn from_event(event: &StoredEvent) -> Option<Self> {
        if event.kind != 1111 {
            return None;
        }

        let e_tag = event.tags.iter().find(|t| t.first().map(|s| s == "e").unwrap_or(false))?;
        let thread_id = e_tag.get(1)?.clone();

        Some(Message {
            id: event.id.clone(),
            content: event.content.clone(),
            pubkey: event.pubkey.clone(),
            thread_id,
            created_at: event.created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_message() {
        let event = StoredEvent {
            id: "a".repeat(64),
            pubkey: "b".repeat(64),
            kind: 1111,
            created_at: 1000,
            content: "Hello world".to_string(),
            tags: vec![vec!["e".to_string(), "c".repeat(64), "".to_string(), "root".to_string()]],
            sig: "0".repeat(128),
        };

        let message = Message::from_event(&event).unwrap();
        assert_eq!(message.content, "Hello world");
        assert_eq!(message.thread_id, "c".repeat(64));
    }
}
