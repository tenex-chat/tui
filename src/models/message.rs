use nostrdb::Note;

#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub content: String,
    pub pubkey: String,
    pub thread_id: String,
    pub created_at: u64,
    /// Direct parent message ID (for threaded replies)
    /// None for messages replying directly to thread root
    pub reply_to: Option<String>,
    /// Whether this is a reasoning/thinking message (has "reasoning" tag)
    pub is_reasoning: bool,
}

impl Message {
    /// Create a Message from a kind:1 note with e-tag (NIP-10 "root" marker).
    /// Message detection: kind:1 + has e-tag with "root" marker
    ///
    /// NIP-10: ["e", <event-id>, <relay-url>, <marker>]
    /// - First e-tag with "root" marker = thread root reference
    /// - First e-tag with "reply" marker (or no marker for backwards compat) = direct parent
    pub fn from_note(note: &Note) -> Option<Self> {
        if note.kind() != 1 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        let mut thread_id: Option<String> = None;
        let mut reply_to: Option<String> = None;
        let mut is_reasoning = false;

        // Parse e-tags per NIP-10
        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            match tag_name {
                Some("e") => {
                    // Extract event ID
                    let event_id = if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        Some(s.to_string())
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        Some(hex::encode(id_bytes))
                    } else {
                        None
                    };

                    if let Some(eid) = event_id {
                        // Check marker (4th element in NIP-10: ["e", id, relay, marker])
                        let marker = tag.get(3).and_then(|t| t.variant().str());

                        match marker {
                            Some("root") => {
                                thread_id = Some(eid);
                            }
                            Some("reply") => {
                                reply_to = Some(eid);
                            }
                            None => {
                                // No marker: backwards compat - if we don't have root yet, use as root
                                if thread_id.is_none() {
                                    thread_id = Some(eid);
                                } else {
                                    // Second e-tag without marker = reply
                                    reply_to = Some(eid);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                Some("reasoning") => {
                    is_reasoning = true;
                }
                _ => {}
            }
        }

        // Must have at least one e-tag (for thread_id)
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

    /// Create a Message from a kind:1 thread root note (the thread itself as first message).
    /// For displaying thread content as the first message in the conversation.
    pub fn from_thread_note(note: &Note) -> Option<Self> {
        if note.kind() != 1 {
            return None;
        }

        let id = hex::encode(note.id());
        let pubkey = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        // Verify it's a thread (has a-tag, no e-tags)
        let mut has_a_tag = false;
        let mut has_e_tag = false;

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            match tag_name {
                Some("a") => has_a_tag = true,
                Some("e") => has_e_tag = true,
                _ => {}
            }
        }

        if !has_a_tag || has_e_tag {
            return None;
        }

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{events::{ingest_events, wait_for_event_processing}, Database};
    use nostr_sdk::prelude::*;
    use nostrdb::{Filter, Transaction};
    use tempfile::tempdir;

    #[test]
    fn test_message_from_kind1_with_root_marker() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();
        let thread_id = "a".repeat(64);

        let event = EventBuilder::new(Kind::from(1), "Message content")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.clone(), "".to_string(), "root".to_string()],
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

        let message = Message::from_note(&note);
        assert!(message.is_some(), "Should parse kind:1 with e-tag root marker");
        let message = message.unwrap();
        assert_eq!(message.thread_id, thread_id);
        assert_eq!(message.content, "Message content");
        assert!(message.reply_to.is_none());
    }

    #[test]
    fn test_message_with_root_and_reply_markers() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();
        let thread_id = "a".repeat(64);
        let parent_id = "b".repeat(64);

        let event = EventBuilder::new(Kind::from(1), "Reply content")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.clone(), "".to_string(), "root".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![parent_id.clone(), "".to_string(), "reply".to_string()],
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

        let message = Message::from_note(&note);
        assert!(message.is_some(), "Should parse kind:1 with root and reply markers");
        let message = message.unwrap();
        assert_eq!(message.thread_id, thread_id);
        assert_eq!(message.reply_to, Some(parent_id));
    }

    #[test]
    fn test_message_backwards_compat_no_markers() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();
        let thread_id = "a".repeat(64);
        let parent_id = "b".repeat(64);

        // Old style: first e-tag = root, second = reply
        let event = EventBuilder::new(Kind::from(1), "Old style")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.clone()],
            ))
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![parent_id.clone()],
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

        let message = Message::from_note(&note);
        assert!(message.is_some(), "Should parse backwards-compatible format");
        let message = message.unwrap();
        assert_eq!(message.thread_id, thread_id);
        assert_eq!(message.reply_to, Some(parent_id));
    }

    #[test]
    fn test_message_rejects_kind1_without_e_tag() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "No e-tag")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec!["31933:pubkey:proj1".to_string()],
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

        let message = Message::from_note(&note);
        assert!(message.is_none(), "Should reject kind:1 without e-tag (it's a thread, not message)");
    }

    #[test]
    fn test_message_rejects_wrong_kind() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();
        let thread_id = "a".repeat(64);

        let event = EventBuilder::new(Kind::Custom(1111), "Old kind:1111")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("E")),
                vec![thread_id],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event.clone()], None).unwrap();

        // Wait for async processing
        let filter = Filter::new().kinds([1111]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(results.len() > 0, "Event should be indexed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).unwrap();

        let message = Message::from_note(&note);
        assert!(message.is_none(), "Should reject kind:1111 (deprecated)");
    }

    #[test]
    fn test_from_thread_note_creates_message() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Thread as message")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec!["31933:pubkey:proj1".to_string()],
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

        let message = Message::from_thread_note(&note);
        assert!(message.is_some(), "Should create message from thread note");
        let message = message.unwrap();
        assert_eq!(message.thread_id, message.id);
        assert!(message.reply_to.is_none());
    }
}
