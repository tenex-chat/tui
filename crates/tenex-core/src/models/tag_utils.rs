//! Tag extraction utilities for parsing nostrdb Notes
//!
//! Provides helper functions to reduce boilerplate when parsing tags from Nostr events.

use nostrdb::Note;

/// Extract a single string value from a tag by name.
/// Returns the first occurrence if multiple tags exist.
pub fn extract_tag_str<'a>(note: &'a Note<'a>, tag_name: &str) -> Option<&'a str> {
    for tag in note.tags() {
        if tag.get(0).and_then(|t| t.variant().str()) == Some(tag_name) {
            return tag.get(1).and_then(|t| t.variant().str());
        }
    }
    None
}

/// Extract a tag value that may be either a string or an ID (hex-encoded bytes).
/// nostrdb stores event IDs as binary Id variant, this handles both cases.
pub fn extract_tag_id_or_str(note: &Note, tag_name: &str) -> Option<String> {
    for tag in note.tags() {
        if tag.get(0).and_then(|t| t.variant().str()) == Some(tag_name) {
            if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                return Some(s.to_string());
            }
            if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                return Some(hex::encode(id_bytes));
            }
        }
    }
    None
}

/// Extract all string values for a given tag name.
/// Useful for tags that appear multiple times (e.g., "p", "t", "tool").
pub fn extract_all_tag_values(note: &Note, tag_name: &str) -> Vec<String> {
    let mut values = Vec::new();
    for tag in note.tags() {
        if tag.get(0).and_then(|t| t.variant().str()) == Some(tag_name) {
            if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                values.push(s.to_string());
            }
        }
    }
    values
}

/// Extract all values for a tag that may contain IDs (hex-encoded bytes) or strings.
/// Useful for tags like "p", "e", "agent" that reference other events/pubkeys.
pub fn extract_all_tag_ids_or_strs(note: &Note, tag_name: &str) -> Vec<String> {
    let mut values = Vec::new();
    for tag in note.tags() {
        if tag.get(0).and_then(|t| t.variant().str()) == Some(tag_name) {
            if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                values.push(hex::encode(id_bytes));
            } else if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                values.push(s.to_string());
            }
        }
    }
    values
}

/// Check if a note has a specific tag (regardless of value).
pub fn has_tag(note: &Note, tag_name: &str) -> bool {
    for tag in note.tags() {
        if tag.get(0).and_then(|t| t.variant().str()) == Some(tag_name) {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{events::{ingest_events, wait_for_event_processing}, Database};
    use nostr_sdk::prelude::*;
    use nostrdb::{Filter, Transaction};
    use tempfile::tempdir;

    #[test]
    fn test_extract_tag_str() {
        let dir = tempdir().expect("Failed to create temp dir");
        let db = Database::new(dir.path()).expect("Failed to create database");
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Test content")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["My Title"],
            ))
            .sign_with_keys(&keys)
            .expect("Failed to sign event");

        ingest_events(&db.ndb, &[event], None).expect("Failed to ingest events");

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).expect("Failed to create transaction");
        let results = db.ndb.query(&txn, &[filter], 10).expect("Query failed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).expect("Failed to get note");

        assert_eq!(extract_tag_str(&note, "title"), Some("My Title"));
        assert_eq!(extract_tag_str(&note, "nonexistent"), None);
    }

    #[test]
    fn test_extract_all_tag_values() {
        let dir = tempdir().expect("Failed to create temp dir");
        let db = Database::new(dir.path()).expect("Failed to create database");
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Test content")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                vec!["rust"],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                vec!["nostr"],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                vec!["testing"],
            ))
            .sign_with_keys(&keys)
            .expect("Failed to sign event");

        ingest_events(&db.ndb, &[event], None).expect("Failed to ingest events");

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).expect("Failed to create transaction");
        let results = db.ndb.query(&txn, &[filter], 10).expect("Query failed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).expect("Failed to get note");

        let hashtags = extract_all_tag_values(&note, "t");
        assert_eq!(hashtags.len(), 3);
        assert!(hashtags.contains(&"rust".to_string()));
        assert!(hashtags.contains(&"nostr".to_string()));
        assert!(hashtags.contains(&"testing".to_string()));
    }

    #[test]
    fn test_has_tag() {
        let dir = tempdir().expect("Failed to create temp dir");
        let db = Database::new(dir.path()).expect("Failed to create database");
        let keys = Keys::generate();

        let event = EventBuilder::new(Kind::from(1), "Test content")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("reasoning")),
                Vec::<String>::new(),
            ))
            .sign_with_keys(&keys)
            .expect("Failed to sign event");

        ingest_events(&db.ndb, &[event], None).expect("Failed to ingest events");

        let filter = Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).expect("Failed to create transaction");
        let results = db.ndb.query(&txn, &[filter], 10).expect("Query failed");
        let note = db.ndb.get_note_by_key(&txn, results[0].note_key).expect("Failed to get note");

        assert!(has_tag(&note, "reasoning"));
        assert!(!has_tag(&note, "nonexistent"));
    }
}
