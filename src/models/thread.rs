use crate::store::events::StoredEvent;

#[derive(Debug, Clone)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub content: String,
    pub pubkey: String,
    pub project_id: String,
    pub created_at: u64,
}

impl Thread {
    pub fn from_event(event: &StoredEvent) -> Option<Self> {
        if event.kind != 11 {
            return None;
        }

        let a_tag = event.tags.iter().find(|t| t.first().map(|s| s == "a").unwrap_or(false))?;
        let project_id = a_tag.get(1)?.clone();

        let title = event
            .tags
            .iter()
            .find(|t| t.first().map(|s| s == "title").unwrap_or(false))
            .and_then(|t| t.get(1))
            .cloned()
            .unwrap_or_else(|| "Untitled".to_string());

        Some(Thread {
            id: event.id.clone(),
            title,
            content: event.content.clone(),
            pubkey: event.pubkey.clone(),
            project_id,
            created_at: event.created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_thread() {
        let event = StoredEvent {
            id: "a".repeat(64),
            pubkey: "b".repeat(64),
            kind: 11,
            created_at: 1000,
            content: "Thread content".to_string(),
            tags: vec![
                vec!["a".to_string(), "31933:pubkey:proj1".to_string()],
                vec!["title".to_string(), "My Thread".to_string()],
            ],
            sig: "0".repeat(128),
        };

        let thread = Thread::from_event(&event).unwrap();
        assert_eq!(thread.title, "My Thread");
        assert_eq!(thread.project_id, "31933:pubkey:proj1");
    }
}
