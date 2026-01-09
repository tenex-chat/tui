use anyhow::Result;
use nostrdb::{IngestMetadata, Ndb};
use nostr_sdk::prelude::*;
use tracing::{debug, instrument};

/// Ingest events into nostrdb from nostr-sdk Events
/// - relay_url: the source relay URL (None for locally created events)
#[instrument(skip(ndb, events), fields(event_count = events.len()))]
pub fn ingest_events(ndb: &Ndb, events: &[Event], relay_url: Option<&str>) -> Result<usize> {
    let mut ingested = 0;

    for event in events {
        let json = event.as_json();
        // nostrdb expects relay format: ["EVENT", "subid", {...}]
        let relay_json = format!(r#"["EVENT","tenex",{}]"#, json);

        let result = if let Some(url) = relay_url {
            let meta = IngestMetadata::new().client(false).relay(url);
            ndb.process_event_with(&relay_json, meta)
        } else {
            // For local/test events, use process_event which doesn't require relay metadata
            ndb.process_event(&relay_json)
        };

        if let Err(e) = result {
            debug!("Failed to ingest event {}: {}", event.id, e);
        } else {
            ingested += 1;
        }
    }

    Ok(ingested)
}

/// Helper to wait for events to be processed by nostrdb (for tests)
#[cfg(test)]
pub fn wait_for_event_processing(ndb: &Ndb, filter: nostrdb::Filter, max_wait_ms: u64) -> bool {
    use std::time::{Duration, Instant};

    let start = Instant::now();
    let timeout = Duration::from_millis(max_wait_ms);

    loop {
        if let Ok(txn) = nostrdb::Transaction::new(ndb) {
            if let Ok(results) = ndb.query(&txn, &[filter.clone()], 1) {
                if !results.is_empty() {
                    return true;
                }
            }
        }

        if start.elapsed() >= timeout {
            return false;
        }

        std::thread::sleep(Duration::from_millis(10));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Database;
    use tempfile::tempdir;

    #[test]
    fn test_ingest_events() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();

        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(31933), "Test project")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["proj1".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("name")),
                vec!["Project 1".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        let ingested = ingest_events(&db.ndb, &[event.clone()], None).unwrap();
        assert_eq!(ingested, 1);

        // Wait for async processing
        let filter = nostrdb::Filter::new().kinds([31933]).build();
        let found = wait_for_event_processing(&db.ndb, filter.clone(), 5000);
        assert!(found, "Event was not processed within timeout");

        // Query to verify
        let txn = nostrdb::Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1);
    }
}
