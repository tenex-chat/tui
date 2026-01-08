use nostrdb::Note;

#[derive(Debug, Clone)]
pub struct ConversationMetadata {
    pub thread_id: String,
    pub title: Option<String>,
    pub created_at: u64,
    pub status_label: Option<String>,
    pub status_current_activity: Option<String>,
}

impl ConversationMetadata {
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 513 {
            return None;
        }

        let created_at = note.created_at();

        let mut thread_id: Option<String> = None;
        let mut title: Option<String> = None;
        let mut status_label: Option<String> = None;
        let mut status_current_activity: Option<String> = None;

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
                Some("status-label") => {
                    status_label = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                Some("status-current-activity") => {
                    status_current_activity = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                _ => {}
            }
        }

        let thread_id = thread_id?;

        Some(ConversationMetadata {
            thread_id,
            title,
            created_at,
            status_label,
            status_current_activity,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{events::{ingest_events, wait_for_event_processing}, Database};
    use nostr_sdk::prelude::*;
    use nostrdb::{Filter, Transaction};
    use tempfile::tempdir;

    #[test]
    fn test_parse_metadata_with_status_label() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let thread_id = "test_thread_123";
        let event = EventBuilder::new(Kind::from(513), "")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Test Thread".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("status-label")),
                vec!["In Progress".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([513]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1, "Event should be indexed");

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let metadata = ConversationMetadata::from_note(&note);

        assert!(metadata.is_some(), "Should parse kind:513 metadata event");
        let metadata = metadata.unwrap();
        assert_eq!(metadata.thread_id, thread_id);
        assert_eq!(metadata.title, Some("Test Thread".to_string()));
        assert_eq!(metadata.status_label, Some("In Progress".to_string()));
        assert_eq!(metadata.status_current_activity, None);
    }

    #[test]
    fn test_parse_metadata_with_current_activity() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let thread_id = "test_thread_456";
        let event = EventBuilder::new(Kind::from(513), "")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("status-current-activity")),
                vec!["Writing integration tests...".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([513]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1, "Event should be indexed");

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let metadata = ConversationMetadata::from_note(&note);

        assert!(metadata.is_some(), "Should parse kind:513 metadata event");
        let metadata = metadata.unwrap();
        assert_eq!(metadata.thread_id, thread_id);
        assert_eq!(metadata.status_current_activity, Some("Writing integration tests...".to_string()));
    }

    #[test]
    fn test_parse_metadata_with_both_status_fields() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let thread_id = "test_thread_789";
        let event = EventBuilder::new(Kind::from(513), "")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Complex Task".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("status-label")),
                vec!["In Progress".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("status-current-activity")),
                vec!["Refactoring data models".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([513]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1, "Event should be indexed");

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let metadata = ConversationMetadata::from_note(&note);

        assert!(metadata.is_some(), "Should parse kind:513 metadata event");
        let metadata = metadata.unwrap();
        assert_eq!(metadata.thread_id, thread_id);
        assert_eq!(metadata.title, Some("Complex Task".to_string()));
        assert_eq!(metadata.status_label, Some("In Progress".to_string()));
        assert_eq!(metadata.status_current_activity, Some("Refactoring data models".to_string()));
    }

    #[test]
    fn test_parse_metadata_without_status_fields() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let thread_id = "test_thread_000";
        let event = EventBuilder::new(Kind::from(513), "")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Simple Thread".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        let filter = Filter::new().kinds([513]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1, "Event should be indexed");

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let metadata = ConversationMetadata::from_note(&note);

        assert!(metadata.is_some(), "Should parse kind:513 metadata event");
        let metadata = metadata.unwrap();
        assert_eq!(metadata.thread_id, thread_id);
        assert_eq!(metadata.title, Some("Simple Thread".to_string()));
        assert_eq!(metadata.status_label, None);
        assert_eq!(metadata.status_current_activity, None);
    }
}
