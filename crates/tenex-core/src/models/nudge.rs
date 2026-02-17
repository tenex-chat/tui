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
    /// Used in additive/subtractive mode - mutually exclusive with only_tools
    pub allowed_tools: Vec<String>,
    /// Tools to remove from agent's available tools (deny-tool tags)
    /// Used in additive/subtractive mode - mutually exclusive with only_tools
    pub denied_tools: Vec<String>,
    /// Exclusive tool list (only-tool tags)
    /// When present, overrides all other tool permissions - agent gets EXACTLY these tools
    /// Mutually exclusive with allow_tools/deny_tools
    pub only_tools: Vec<String>,
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
        let mut only_tools: Vec<String> = Vec::new();
        let mut supersedes: Option<String> = None;

        for tag in note.tags() {
            if tag.count() >= 2 {
                if let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) {
                    // Handle supersedes tag specially - nostrdb stores 64-char hex strings as Id variant
                    if tag_name == "supersedes" {
                        supersedes = tag.get(1).map(|t| match t.variant() {
                            nostrdb::NdbStrVariant::Str(s) => s.to_string(),
                            nostrdb::NdbStrVariant::Id(bytes) => hex::encode(bytes),
                        });
                        continue;
                    }

                    if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                        match tag_name {
                            "title" => title = Some(value.to_string()),
                            "description" => description = Some(value.to_string()),
                            "t" => hashtags.push(value.to_string()),
                            "allow-tool" => allowed_tools.push(value.to_string()),
                            "deny-tool" => denied_tools.push(value.to_string()),
                            "only-tool" => only_tools.push(value.to_string()),
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
            only_tools,
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

    /// Helper to create a nudge event with the given tags and ingest it
    fn create_nudge_event_builder(content: &str) -> EventBuilder {
        EventBuilder::new(Kind::Custom(4201), content)
    }

    #[test]
    fn test_from_note_extracts_allow_tool_tags() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = create_nudge_event_builder("Test content")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Test Nudge".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("allow-tool")),
                vec!["Bash".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("allow-tool")),
                vec!["Read".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("allow-tool")),
                vec!["Write".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([4201]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let nudge = Nudge::from_note(&note).expect("Should parse nudge");

        assert_eq!(nudge.allowed_tools, vec!["Bash", "Read", "Write"]);
        assert!(
            nudge.denied_tools.is_empty(),
            "denied_tools should be empty"
        );
        assert!(nudge.only_tools.is_empty(), "only_tools should be empty");
    }

    #[test]
    fn test_from_note_extracts_deny_tool_tags() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = create_nudge_event_builder("Test content")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Test Nudge".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deny-tool")),
                vec!["Bash".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deny-tool")),
                vec!["Write".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([4201]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let nudge = Nudge::from_note(&note).expect("Should parse nudge");

        assert!(
            nudge.allowed_tools.is_empty(),
            "allowed_tools should be empty"
        );
        assert_eq!(nudge.denied_tools, vec!["Bash", "Write"]);
        assert!(nudge.only_tools.is_empty(), "only_tools should be empty");
    }

    #[test]
    fn test_from_note_extracts_only_tool_tags() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = create_nudge_event_builder("Exclusive mode content")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Exclusive Nudge".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("only-tool")),
                vec!["Read".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("only-tool")),
                vec!["Grep".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([4201]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let nudge = Nudge::from_note(&note).expect("Should parse nudge");

        assert!(
            nudge.allowed_tools.is_empty(),
            "allowed_tools should be empty in exclusive mode"
        );
        assert!(
            nudge.denied_tools.is_empty(),
            "denied_tools should be empty in exclusive mode"
        );
        assert_eq!(nudge.only_tools, vec!["Read", "Grep"]);
    }

    #[test]
    fn test_from_note_extracts_supersedes_tag() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let original_id = "abc123def456789012345678901234567890123456789012345678901234abcd";

        let event = create_nudge_event_builder("Updated content with supersedes")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Updated Nudge with Supersedes".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("supersedes")),
                vec![original_id.to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        let expected_event_id = event.id.to_hex();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([4201]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(!results.is_empty(), "Should find at least one event");

        // Find our specific event by ID
        let mut found_note = None;
        for result in &results {
            let note = db.ndb.get_note_by_key(&txn, result.note_key).unwrap();
            let note_id = hex::encode(note.id());
            if note_id == expected_event_id {
                found_note = Some(note);
                break;
            }
        }

        let note = found_note.expect("Should find our specific event");
        let nudge = Nudge::from_note(&note).expect("Should parse nudge");

        assert_eq!(nudge.title, "Updated Nudge with Supersedes");
        assert_eq!(nudge.supersedes, Some(original_id.to_string()));
    }

    #[test]
    fn test_from_note_provides_defaults_when_tags_missing() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        // Minimal nudge with only content, no title/description/tools
        let event = create_nudge_event_builder("Minimal content")
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([4201]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let nudge = Nudge::from_note(&note).expect("Should parse nudge");

        // Should use default title
        assert_eq!(nudge.title, DEFAULT_NUDGE_TITLE);
        // Description defaults to empty
        assert_eq!(nudge.description, "");
        // All tool lists should be empty
        assert!(nudge.allowed_tools.is_empty());
        assert!(nudge.denied_tools.is_empty());
        assert!(nudge.only_tools.is_empty());
        // No supersedes
        assert!(nudge.supersedes.is_none());
        // Hashtags should be empty
        assert!(nudge.hashtags.is_empty());
    }

    #[test]
    fn test_from_note_accumulates_multiple_tags_of_same_type() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = create_nudge_event_builder("Multi-tag content")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Multi-Tag Nudge".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                vec!["rust".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                vec!["nostr".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                vec!["coding".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("allow-tool")),
                vec!["Tool1".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("allow-tool")),
                vec!["Tool2".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deny-tool")),
                vec!["Tool3".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deny-tool")),
                vec!["Tool4".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([4201]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let nudge = Nudge::from_note(&note).expect("Should parse nudge");

        // Verify accumulation of hashtags
        assert_eq!(nudge.hashtags.len(), 3);
        assert!(nudge.hashtags.contains(&"rust".to_string()));
        assert!(nudge.hashtags.contains(&"nostr".to_string()));
        assert!(nudge.hashtags.contains(&"coding".to_string()));

        // Verify accumulation of allow-tool tags
        assert_eq!(nudge.allowed_tools.len(), 2);
        assert!(nudge.allowed_tools.contains(&"Tool1".to_string()));
        assert!(nudge.allowed_tools.contains(&"Tool2".to_string()));

        // Verify accumulation of deny-tool tags
        assert_eq!(nudge.denied_tools.len(), 2);
        assert!(nudge.denied_tools.contains(&"Tool3".to_string()));
        assert!(nudge.denied_tools.contains(&"Tool4".to_string()));
    }

    #[test]
    fn test_from_note_rejects_wrong_kind() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        // Create a kind:1 event (not a nudge)
        let event = EventBuilder::new(Kind::from(1), "Not a nudge")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Fake Nudge".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let nudge = Nudge::from_note(&note);

        assert!(nudge.is_none(), "Should reject non-kind:4201 notes");
    }

    #[test]
    fn test_from_note_extracts_description() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = create_nudge_event_builder("Content here")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["My Nudge".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("description")),
                vec!["This is a detailed description".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([4201]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let nudge = Nudge::from_note(&note).expect("Should parse nudge");

        assert_eq!(nudge.title, "My Nudge");
        assert_eq!(nudge.description, "This is a detailed description");
    }
}
