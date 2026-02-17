use anyhow::Result;
use nostr_sdk::prelude::*;
use nostrdb::{IngestMetadata, Ndb, Transaction};
use serde_json::json;

/// Ingest events into nostrdb from nostr-sdk Events
/// - relay_url: the source relay URL (None for locally created events)
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

        if let Err(_e) = result {
            // Event ingestion failed (e.g., duplicate)
        } else {
            ingested += 1;
        }
    }

    Ok(ingested)
}

/// Trace context info extracted from event tags
#[derive(Debug, Clone)]
pub struct TraceInfo {
    pub trace_id: String,
    pub span_id: String,
}

/// Get raw event JSON by message ID (hex string)
/// Returns the event as a formatted JSON string
pub fn get_raw_event_json(ndb: &Ndb, message_id: &str) -> Option<String> {
    // Decode hex message_id to [u8; 32]
    let id_bytes: [u8; 32] = hex::decode(message_id)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())?;

    let txn = Transaction::new(ndb).ok()?;
    let note_key = ndb.get_notekey_by_id(&txn, &id_bytes).ok()?;
    let note = ndb.get_note_by_key(&txn, note_key).ok()?;

    // Reconstruct JSON from note fields
    let id = hex::encode(note.id());
    let pubkey = hex::encode(note.pubkey());
    let created_at = note.created_at();
    let kind = note.kind();
    let content = note.content();

    // Extract tags
    let mut tags: Vec<Vec<String>> = Vec::new();
    for tag in note.tags() {
        let mut tag_values: Vec<String> = Vec::new();
        let count = tag.count();
        for i in 0..count {
            if let Some(val) = tag.get(i) {
                if let Some(s) = val.variant().str() {
                    tag_values.push(s.to_string());
                } else if let Some(id_bytes) = val.variant().id() {
                    tag_values.push(hex::encode(id_bytes));
                }
            }
        }
        if !tag_values.is_empty() {
            tags.push(tag_values);
        }
    }

    // Build JSON (note: sig not available from nostrdb Note)
    let event_json = json!({
        "id": id,
        "pubkey": pubkey,
        "created_at": created_at,
        "kind": kind,
        "content": content,
        "tags": tags,
        "sig": "" // nostrdb doesn't store signature
    });

    serde_json::to_string_pretty(&event_json).ok()
}

/// Get trace context from event tags
/// Looks for trace_context_llm (preferred) or trace_context tags
/// Parses W3C traceparent format: 00-{traceId}-{spanId}-{traceFlags}
pub fn get_trace_context(ndb: &Ndb, message_id: &str) -> Option<TraceInfo> {
    // Decode hex message_id to [u8; 32]
    let id_bytes: [u8; 32] = hex::decode(message_id)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())?;

    let txn = Transaction::new(ndb).ok()?;
    let note_key = ndb.get_notekey_by_id(&txn, &id_bytes).ok()?;
    let note = ndb.get_note_by_key(&txn, note_key).ok()?;

    // Look for trace_context_llm first, then trace_context
    let mut trace_context: Option<String> = None;

    for tag in note.tags() {
        let tag_name = tag.get(0).and_then(|t| t.variant().str());
        match tag_name {
            Some("trace_context_llm") => {
                if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                    trace_context = Some(val.to_string());
                    break; // Prefer trace_context_llm
                }
            }
            Some("trace_context") => {
                if trace_context.is_none() {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        trace_context = Some(val.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    // Parse W3C traceparent format: 00-{traceId}-{spanId}-{traceFlags}
    let context = trace_context?;
    let parts: Vec<&str> = context.split('-').collect();
    if parts.len() != 4 {
        return None;
    }

    Some(TraceInfo {
        trace_id: parts[1].to_string(),
        span_id: parts[2].to_string(),
    })
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
