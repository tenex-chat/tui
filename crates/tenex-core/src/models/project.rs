use nostrdb::Note;

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: Option<String>,
    pub repo_url: Option<String>,
    pub picture_url: Option<String>,
    pub is_deleted: bool,
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
        let content = note.content();

        let mut id: Option<String> = None;
        let mut title: Option<String> = None;
        let mut name: Option<String> = None;
        let mut repo_url: Option<String> = None;
        let mut picture_url: Option<String> = None;
        let mut is_deleted = false;
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
                Some("repo") => {
                    repo_url = tag
                        .get(1)
                        .and_then(|t| t.variant().str())
                        .map(|s| s.to_string());
                }
                Some("picture") | Some("image") => {
                    picture_url = tag
                        .get(1)
                        .and_then(|t| t.variant().str())
                        .map(|s| s.to_string());
                }
                Some("deleted") => {
                    is_deleted = true;
                }
                Some("p") => {
                    if let Some(p) = tag.get(1).and_then(|t| t.variant().str()) {
                        participants.push(p.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        participants.push(hex::encode(id_bytes));
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
        let description = if content.trim().is_empty() {
            None
        } else {
            Some(content.to_string())
        };

        Some(Project {
            id: id.clone(),
            name: display_name,
            description,
            repo_url,
            picture_url,
            is_deleted,
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
    use crate::store::{
        events::{ingest_events, wait_for_event_processing},
        Database,
    };
    use nostr_sdk::prelude::*;
    use nostrdb::{Filter, Transaction};
    use tempfile::tempdir;

    fn parse_project_from_event(event: Event) -> Project {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([31933]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        Project::from_note(&note).unwrap()
    }

    #[test]
    fn test_a_tag() {
        let project = Project {
            id: "proj1".to_string(),
            name: "Project 1".to_string(),
            description: None,
            repo_url: None,
            picture_url: None,
            is_deleted: false,
            pubkey: "a".repeat(64),
            participants: vec![],
            agent_ids: vec![],
            mcp_tool_ids: vec![],
            created_at: 0,
        };

        assert_eq!(project.a_tag(), format!("31933:{}:proj1", "a".repeat(64)));
    }

    #[test]
    fn test_from_note_parses_project_metadata_and_assignments() {
        let keys = Keys::generate();
        let participant = "b".repeat(64);
        let agent_id = "c".repeat(64);
        let mcp_id = "d".repeat(64);

        let event = EventBuilder::new(Kind::Custom(31933), "Project description")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("d")),
                vec!["proj-meta".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Project Meta".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("repo")),
                vec!["https://github.com/tenex/meta".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("picture")),
                vec!["https://cdn.example.com/project.png".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("p")),
                vec![participant.clone()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("agent")),
                vec![agent_id.clone()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("mcp")),
                vec![mcp_id.clone()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        let project = parse_project_from_event(event);
        assert_eq!(project.id, "proj-meta");
        assert_eq!(project.name, "Project Meta");
        assert_eq!(project.description.as_deref(), Some("Project description"));
        assert_eq!(
            project.repo_url.as_deref(),
            Some("https://github.com/tenex/meta")
        );
        assert_eq!(
            project.picture_url.as_deref(),
            Some("https://cdn.example.com/project.png")
        );
        assert_eq!(project.participants, vec![participant]);
        assert_eq!(project.agent_ids, vec![agent_id]);
        assert_eq!(project.mcp_tool_ids, vec![mcp_id]);
        assert!(!project.is_deleted);
    }

    #[test]
    fn test_from_note_parses_deleted_and_image_fallback() {
        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(31933), "   ")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("d")),
                vec!["proj-deleted".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Deleted Project".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("image")),
                vec!["https://cdn.example.com/image-fallback.png".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deleted")),
                Vec::<String>::new(),
            ))
            .sign_with_keys(&keys)
            .unwrap();

        let project = parse_project_from_event(event);
        assert_eq!(project.id, "proj-deleted");
        assert_eq!(
            project.picture_url.as_deref(),
            Some("https://cdn.example.com/image-fallback.png")
        );
        assert_eq!(project.description, None);
        assert!(project.is_deleted);
    }
}
