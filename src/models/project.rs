use crate::store::events::StoredEvent;

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub pubkey: String,
    pub participants: Vec<String>,
    pub created_at: u64,
}

impl Project {
    pub fn from_event(event: &StoredEvent) -> Option<Self> {
        if event.kind != 31933 {
            return None;
        }

        let d_tag = event.tags.iter().find(|t| t.first().map(|s| s == "d").unwrap_or(false))?;
        let id = d_tag.get(1)?.clone();

        let name = event
            .tags
            .iter()
            .find(|t| t.first().map(|s| s == "name").unwrap_or(false))
            .and_then(|t| t.get(1))
            .cloned()
            .unwrap_or_else(|| id.clone());

        let participants: Vec<String> = event
            .tags
            .iter()
            .filter(|t| t.first().map(|s| s == "p").unwrap_or(false))
            .filter_map(|t| t.get(1).cloned())
            .collect();

        Some(Project {
            id,
            name,
            description: event.content.clone(),
            pubkey: event.pubkey.clone(),
            participants,
            created_at: event.created_at,
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
    fn test_parse_project() {
        let event = StoredEvent {
            id: "a".repeat(64),
            pubkey: "b".repeat(64),
            kind: 31933,
            created_at: 1000,
            content: "Description".to_string(),
            tags: vec![
                vec!["d".to_string(), "my-project".to_string()],
                vec!["name".to_string(), "My Project".to_string()],
                vec!["p".to_string(), "c".repeat(64)],
            ],
            sig: "0".repeat(128),
        };

        let project = Project::from_event(&event).unwrap();
        assert_eq!(project.id, "my-project");
        assert_eq!(project.name, "My Project");
        assert_eq!(project.participants.len(), 1);
    }

    #[test]
    fn test_a_tag() {
        let project = Project {
            id: "proj1".to_string(),
            name: "Project 1".to_string(),
            description: "".to_string(),
            pubkey: "a".repeat(64),
            participants: vec![],
            created_at: 1000,
        };

        assert_eq!(project.a_tag(), format!("31933:{}:proj1", "a".repeat(64)));
    }
}
