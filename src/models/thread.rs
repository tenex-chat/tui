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
    /// Create a Thread from a kind:1 note with `a` tag and NO `e` tags.
    /// Thread detection: kind:1 + has a-tag + NO e-tags
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 1 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let created_at = note.created_at();

        let mut title: Option<String> = None;
        let mut has_a_tag = false;
        let mut has_e_tag = false;

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            match tag_name {
                Some("a") => {
                    // Validate project_id exists
                    if tag.get(1).and_then(|t| t.variant().str()).is_some() {
                        has_a_tag = true;
                    }
                }
                Some("title") => {
                    title = tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
                }
                Some("e") => {
                    // Thread must NOT have e-tags (messages have e-tags)
                    has_e_tag = true;
                }
                _ => {}
            }
        }

        // Must have a-tag and must NOT have e-tag
        if !has_a_tag || has_e_tag {
            return None;
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{events::{ingest_events, wait_for_event_processing}, Database};
    use nostr_sdk::prelude::*;
    use nostrdb::{Filter, Transaction};
    use tempfile::tempdir;

    #[test]
    fn test_thread_from_kind1_with_a_tag_no_e_tag() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Thread content")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec!["31933:pubkey:proj1".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Test Thread".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1, "Event should be indexed");

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let thread = Thread::from_note(&note);

        assert!(thread.is_some(), "Should parse kind:1 with a-tag and no e-tag as thread");
        let thread = thread.unwrap();
        assert_eq!(thread.title, "Test Thread");
        assert_eq!(thread.content, "Thread content");
    }

    #[test]
    fn test_thread_rejects_kind1_with_e_tag() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Not a thread")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec!["31933:pubkey:proj1".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec!["some_thread_id".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0, "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let thread = Thread::from_note(&note);
        assert!(thread.is_none(), "Should reject kind:1 with e-tag (it's a message, not thread)");
    }

    #[test]
    fn test_thread_rejects_kind1_without_a_tag() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Missing a-tag")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Test".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0, "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let thread = Thread::from_note(&note);
        assert!(thread.is_none(), "Should reject kind:1 without a-tag");
    }

    #[test]
    fn test_thread_rejects_wrong_kind() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::Custom(11), "Old kind:11")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec!["31933:pubkey:proj1".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([11]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0, "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let thread = Thread::from_note(&note);
        assert!(thread.is_none(), "Should reject kind:11 (deprecated)");
    }
}
