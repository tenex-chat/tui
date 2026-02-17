use nostrdb::Note;

use crate::constants::DEFAULT_SKILL_TITLE;

/// Skill - kind:4202 events for agent skills
/// Skills are reusable instruction sets that can be attached to conversations.
/// Unlike nudges, skills do not have tool permission modifiers.
#[derive(Debug, Clone)]
pub struct Skill {
    pub id: String,
    pub pubkey: String,
    pub title: String,
    pub description: String,
    pub content: String,
    pub hashtags: Vec<String>,
    pub created_at: u64,
    /// File attachment event IDs (e-tags referencing NIP-94 kind:1063 events)
    pub file_ids: Vec<String>,
}

impl Skill {
    /// Parse a Skill from a kind:4202 note
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 4202 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        let mut title: Option<String> = None;
        let mut description: Option<String> = None;
        let mut hashtags: Vec<String> = Vec::new();
        let mut file_ids: Vec<String> = Vec::new();

        for tag in note.tags() {
            if tag.count() >= 2 {
                if let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) {
                    // Handle e-tags for file references (NIP-94 kind:1063)
                    if tag_name == "e" {
                        if let Some(event_id) = tag.get(1).and_then(|t| match t.variant() {
                            nostrdb::NdbStrVariant::Str(s) => Some(s.to_string()),
                            nostrdb::NdbStrVariant::Id(bytes) => Some(hex::encode(bytes)),
                        }) {
                            file_ids.push(event_id);
                        }
                        continue;
                    }

                    if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                        match tag_name {
                            "title" => title = Some(value.to_string()),
                            "description" => description = Some(value.to_string()),
                            "t" => hashtags.push(value.to_string()),
                            _ => {}
                        }
                    }
                }
            }
        }

        Some(Skill {
            id,
            pubkey,
            title: title.unwrap_or_else(|| DEFAULT_SKILL_TITLE.to_string()),
            description: description.unwrap_or_default(),
            content,
            hashtags,
            created_at,
            file_ids,
        })
    }

    /// Get a short preview of the content (character-safe truncation)
    pub fn content_preview(&self, max_chars: usize) -> String {
        let char_count = self.content.chars().count();
        if char_count <= max_chars {
            self.content.clone()
        } else {
            let truncate_at = max_chars.saturating_sub(3);
            let truncated: String = self.content.chars().take(truncate_at).collect();
            format!("{}...", truncated)
        }
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

    /// Helper to create a skill event with the given tags and ingest it
    fn create_skill_event_builder(content: &str) -> EventBuilder {
        EventBuilder::new(Kind::Custom(4202), content)
    }

    #[test]
    fn test_from_note_parses_basic_skill() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = create_skill_event_builder("Skill content here")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["My Skill".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("description")),
                vec!["A helpful skill".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([4202]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let skill = Skill::from_note(&note).expect("Should parse skill");

        assert_eq!(skill.title, "My Skill");
        assert_eq!(skill.description, "A helpful skill");
        assert_eq!(skill.content, "Skill content here");
    }

    #[test]
    fn test_from_note_extracts_hashtags() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = create_skill_event_builder("Skill with tags")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Tagged Skill".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                vec!["rust".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                vec!["nostr".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([4202]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let skill = Skill::from_note(&note).expect("Should parse skill");

        assert_eq!(skill.hashtags.len(), 2);
        assert!(skill.hashtags.contains(&"rust".to_string()));
        assert!(skill.hashtags.contains(&"nostr".to_string()));
    }

    #[test]
    fn test_from_note_extracts_file_ids() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let file_event_id = "abc123def456789012345678901234567890123456789012345678901234abcd";

        let event = create_skill_event_builder("Skill with files")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Skill With Files".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![file_event_id.to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([4202]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let skill = Skill::from_note(&note).expect("Should parse skill");

        assert_eq!(skill.file_ids.len(), 1);
        assert_eq!(skill.file_ids[0], file_event_id);
    }

    #[test]
    fn test_from_note_provides_defaults_when_tags_missing() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        // Minimal skill with only content, no title/description
        let event = create_skill_event_builder("Minimal content")
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([4202]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let skill = Skill::from_note(&note).expect("Should parse skill");

        // Should use default title
        assert_eq!(skill.title, DEFAULT_SKILL_TITLE);
        // Description defaults to empty
        assert_eq!(skill.description, "");
        // Hashtags should be empty
        assert!(skill.hashtags.is_empty());
        // File IDs should be empty
        assert!(skill.file_ids.is_empty());
    }

    #[test]
    fn test_from_note_rejects_wrong_kind() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        // Create a kind:1 event (not a skill)
        let event = EventBuilder::new(Kind::from(1), "Not a skill")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Fake Skill".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let skill = Skill::from_note(&note);

        assert!(skill.is_none(), "Should reject non-kind:4202 notes");
    }

    #[test]
    fn test_content_preview_short_content() {
        let skill = Skill {
            id: "test".to_string(),
            pubkey: "pubkey".to_string(),
            title: "Test".to_string(),
            description: String::new(),
            content: "Short".to_string(),
            hashtags: vec![],
            created_at: 0,
            file_ids: vec![],
        };
        assert_eq!(skill.content_preview(100), "Short");
    }

    #[test]
    fn test_content_preview_long_content() {
        let skill = Skill {
            id: "test".to_string(),
            pubkey: "pubkey".to_string(),
            title: "Test".to_string(),
            description: String::new(),
            content: "This is a very long content that should be truncated".to_string(),
            hashtags: vec![],
            created_at: 0,
            file_ids: vec![],
        };
        assert_eq!(skill.content_preview(20), "This is a very lo...");
    }
}
