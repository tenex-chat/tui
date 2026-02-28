use nostrdb::Note;

use super::message::{AskEvent, Message};
use crate::constants::DEFAULT_THREAD_TITLE;

#[derive(Debug, Clone, uniffi::Record, serde::Serialize, serde::Deserialize)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub content: String,
    pub pubkey: String,
    /// Most recent activity (thread creation or latest reply)
    pub last_activity: u64,
    /// Effective last activity for sorting - max of own last_activity and all descendants
    /// This enables hierarchical sorting where a parent conversation reflects the most
    /// recent activity in its entire delegation tree.
    pub effective_last_activity: u64,
    /// Status label from kind:513 metadata (e.g., "In Progress", "Blocked", "Done")
    pub status_label: Option<String>,
    /// Current activity from kind:513 metadata (e.g., "Writing tests...")
    pub status_current_activity: Option<String>,
    /// Summary from kind:513 metadata (brief description of the conversation)
    pub summary: Option<String>,
    /// Hashtags from kind:513 metadata (repeated t-tags)
    pub hashtags: Vec<String>,
    /// Parent conversation ID from "delegation" tag (for hierarchical nesting)
    pub parent_conversation_id: Option<String>,
    /// Pubkeys mentioned in p-tags of the root event
    pub p_tags: Vec<String>,
    /// Ask event data if this thread contains questions
    pub ask_event: Option<AskEvent>,
    /// Whether this thread is a scheduled event (has scheduled-task-id tag)
    pub is_scheduled: bool,
}

impl Thread {
    /// Check if this thread was created by or p-tags the given pubkey
    pub fn involves_user(&self, user_pubkey: &str) -> bool {
        self.pubkey == user_pubkey || self.p_tags.iter().any(|p| p == user_pubkey)
    }
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
        let mut parent_conversation_id: Option<String> = None;
        let mut p_tags = Vec::new();
        let mut is_scheduled = false;

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
                    title = tag
                        .get(1)
                        .and_then(|t| t.variant().str())
                        .map(|s| s.to_string());
                }
                Some("e") => {
                    // Thread must NOT have e-tags (messages have e-tags)
                    // EXCEPTION: e-tags with "skill" marker are skill references, not thread/reply markers
                    // NIP-10 format: ["e", id, relay, marker] - marker at index 3
                    // Some clients omit relay: ["e", id, "skill"] - marker at index 2
                    let marker_at_3 = tag.get(3).and_then(|t| t.variant().str());
                    let marker_at_2 = tag.get(2).and_then(|t| t.variant().str());
                    let is_skill = marker_at_3 == Some("skill") || marker_at_2 == Some("skill");
                    if !is_skill {
                        has_e_tag = true;
                    }
                }
                Some("delegation") | Some("parent") => {
                    // Parent tag format: ["parent", "<parent-conversation-id>"]
                    // (Note: "delegation" is legacy - nostrdb has issues with NIP-26 collision)
                    // nostrdb stores 64-char hex strings as Id variant, so we need to handle both
                    parent_conversation_id = tag.get(1).map(|t| match t.variant() {
                        nostrdb::NdbStrVariant::Str(s) => s.to_string(),
                        nostrdb::NdbStrVariant::Id(bytes) => hex::encode(bytes),
                    });
                }
                Some("p") => {
                    // nostrdb stores 64-char hex strings as Id variant
                    if let Some(pubkey) = tag.get(1).and_then(|t| t.variant().str()) {
                        p_tags.push(pubkey.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        p_tags.push(hex::encode(id_bytes));
                    }
                }
                Some("scheduled-task-id") => {
                    is_scheduled = true;
                }
                _ => {}
            }
        }

        // Must have a-tag and must NOT have e-tag
        if !has_a_tag || has_e_tag {
            return None;
        }

        let content = note.content().to_string();

        // Parse ask event data if present
        let ask_event = Message::parse_ask_event(note);

        let resolved_title = title
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| DEFAULT_THREAD_TITLE.to_string());

        Some(Thread {
            id,
            title: resolved_title,
            content,
            pubkey,
            last_activity: created_at,
            effective_last_activity: created_at, // Initialized to same as last_activity
            status_label: None,
            status_current_activity: None,
            summary: None,
            hashtags: Vec::new(),
            parent_conversation_id,
            p_tags,
            ask_event,
            is_scheduled,
        })
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

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1, "Event should be indexed");

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let thread = Thread::from_note(&note);

        assert!(
            thread.is_some(),
            "Should parse kind:1 with a-tag and no e-tag as thread"
        );
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

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(!results.is_empty(), "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let thread = Thread::from_note(&note);
        assert!(
            thread.is_none(),
            "Should reject kind:1 with e-tag (it's a message, not thread)"
        );
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

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(!results.is_empty(), "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let thread = Thread::from_note(&note);
        assert!(thread.is_none(), "Should reject kind:1 without a-tag");
    }

    #[test]
    fn test_thread_uses_default_title_when_title_tag_is_empty() {
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
                vec!["".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(!results.is_empty(), "Event should be indexed");

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let thread = Thread::from_note(&note).expect("Should parse as thread");

        assert_eq!(thread.title, DEFAULT_THREAD_TITLE);
    }

    #[test]
    fn test_thread_parses_real_world_report_tagged_event_with_empty_title() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();

        let raw_event = r#"{"kind":1,"id":"0ee767fed295070e2f2a773ec6b66faa4996daccd02db8f48baf1607d5eb266c","pubkey":"09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7","created_at":1772179535,"tags":[["a","31933:09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7:TENEX-ff3ssq"],["title",""],["client","tenex-tui"],["a","30023:14925f2b4795ca6037fa7d33899c5145d3c1f264865d94ea028ba6168f394ebf:nostr-skill-events-kind-4202"],["p","14925f2b4795ca6037fa7d33899c5145d3c1f264865d94ea028ba6168f394ebf"]],"content":"test","sig":"52a6654f296f36cf4d4d52fe8e5f2163a64cf12ad8ac330d195e15d2b70b1de767eb304bda154269e49b358e3d59fa1e5eb773e419d30830fb44209aff136895"}"#;
        let event = Event::from_json(raw_event).expect("valid event json");

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter, 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let note = db
            .ndb
            .get_note_by_id(&txn, event.id.as_bytes())
            .expect("event should be retrievable by id");

        let thread = Thread::from_note(&note).expect("event should parse as thread root");
        assert_eq!(thread.id, event.id.to_hex());
        assert_eq!(thread.title, DEFAULT_THREAD_TITLE);

        assert!(
            Message::from_note(&note).is_none(),
            "thread root must not parse as message"
        );
        assert!(
            Message::from_thread_note(&note).is_some(),
            "thread root should parse as root message"
        );
    }

    #[test]
    fn test_thread_rejects_wrong_kind() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::Custom(9999), "Wrong kind")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec!["31933:pubkey:proj1".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([9999]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(!results.is_empty(), "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let thread = Thread::from_note(&note);
        assert!(thread.is_none(), "Should reject non-kind:1 notes");
    }

    #[test]
    fn test_thread_parses_delegation_tag() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let parent_id = "4f69d3302cf2d0d5fa6a8b3c5978c5c3ceac100b57a4e67b855379973d51b58e";

        let event = EventBuilder::new(Kind::from(1), "Child thread content")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec!["31933:pubkey:proj1".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Child Thread".to_string()],
            ))
            // Note: nostrdb stores 64-char hex strings as Id variant, so parsing needs to handle both
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("delegation")),
                vec![parent_id.to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1, "Event should be indexed");

        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();
        let thread = Thread::from_note(&note);

        assert!(
            thread.is_some(),
            "Should parse kind:1 with delegation tag as thread"
        );
        let thread = thread.unwrap();
        assert_eq!(thread.title, "Child Thread");
        assert_eq!(
            thread.parent_conversation_id,
            Some(parent_id.to_string()),
            "Delegation tag should be parsed into parent_conversation_id"
        );
    }
}
