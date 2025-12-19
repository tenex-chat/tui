use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};
use tracing::{info_span, instrument};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredEvent {
    pub id: String,
    pub pubkey: String,
    pub kind: u32,
    pub created_at: u64,
    pub content: String,
    pub tags: Vec<Vec<String>>,
    pub sig: String,
}

#[instrument(skip(conn, events), fields(event_count = events.len()))]
pub fn insert_events(conn: &Arc<Mutex<Connection>>, events: &[StoredEvent]) -> Result<usize> {
    let _span = info_span!("store.insert").entered();
    let conn = conn.lock().unwrap();
    let mut inserted = 0;

    for event in events {
        let tags_json = serde_json::to_string(&event.tags)?;
        let result = conn.execute(
            "INSERT OR IGNORE INTO events (id, pubkey, kind, created_at, content, tags, sig) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            (
                &event.id,
                &event.pubkey,
                event.kind,
                event.created_at as i64,
                &event.content,
                &tags_json,
                &event.sig,
            ),
        );
        if let Ok(n) = result {
            inserted += n;
        }
    }

    Ok(inserted)
}

pub fn get_events_by_kind(conn: &Arc<Mutex<Connection>>, kind: u32) -> Result<Vec<StoredEvent>> {
    let conn = conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, pubkey, kind, created_at, content, tags, sig FROM events WHERE kind = ?1 ORDER BY created_at DESC",
    )?;

    let events = stmt
        .query_map([kind], |row| {
            let tags_json: String = row.get(5)?;
            let tags: Vec<Vec<String>> = serde_json::from_str(&tags_json).unwrap_or_default();
            Ok(StoredEvent {
                id: row.get(0)?,
                pubkey: row.get(1)?,
                kind: row.get(2)?,
                created_at: row.get::<_, i64>(3)? as u64,
                content: row.get(4)?,
                tags,
                sig: row.get(6)?,
            })
        })?
        .filter_map(|e| e.ok())
        .collect();

    Ok(events)
}

pub fn get_events_by_kind_and_pubkey(
    conn: &Arc<Mutex<Connection>>,
    kind: u32,
    pubkey: &str,
) -> Result<Vec<StoredEvent>> {
    let conn = conn.lock().unwrap();
    let mut stmt = conn.prepare(
        "SELECT id, pubkey, kind, created_at, content, tags, sig FROM events WHERE kind = ?1 AND pubkey = ?2 ORDER BY created_at DESC",
    )?;

    let events = stmt
        .query_map([kind.to_string(), pubkey.to_string()], |row| {
            let tags_json: String = row.get(5)?;
            let tags: Vec<Vec<String>> = serde_json::from_str(&tags_json).unwrap_or_default();
            Ok(StoredEvent {
                id: row.get(0)?,
                pubkey: row.get(1)?,
                kind: row.get(2)?,
                created_at: row.get::<_, i64>(3)? as u64,
                content: row.get(4)?,
                tags,
                sig: row.get(6)?,
            })
        })?
        .filter_map(|e| e.ok())
        .collect();

    Ok(events)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Database;

    #[test]
    fn test_insert_and_query_events() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        let events = vec![
            StoredEvent {
                id: "a".repeat(64),
                pubkey: "b".repeat(64),
                kind: 31933,
                created_at: 1000,
                content: "Test project".to_string(),
                tags: vec![vec!["d".to_string(), "proj1".to_string()]],
                sig: "0".repeat(128),
            },
            StoredEvent {
                id: "c".repeat(64),
                pubkey: "d".repeat(64),
                kind: 11,
                created_at: 2000,
                content: "Test thread".to_string(),
                tags: vec![],
                sig: "0".repeat(128),
            },
        ];

        let inserted = insert_events(&conn, &events).unwrap();
        assert_eq!(inserted, 2);

        let projects = get_events_by_kind(&conn, 31933).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].content, "Test project");

        let threads = get_events_by_kind(&conn, 11).unwrap();
        assert_eq!(threads.len(), 1);
    }

    #[test]
    fn test_query_by_pubkey() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        let pubkey1 = "a".repeat(64);
        let pubkey2 = "b".repeat(64);

        let events = vec![
            StoredEvent {
                id: "1".repeat(64),
                pubkey: pubkey1.clone(),
                kind: 31933,
                created_at: 1000,
                content: "Project 1".to_string(),
                tags: vec![],
                sig: "0".repeat(128),
            },
            StoredEvent {
                id: "2".repeat(64),
                pubkey: pubkey2.clone(),
                kind: 31933,
                created_at: 2000,
                content: "Project 2".to_string(),
                tags: vec![],
                sig: "0".repeat(128),
            },
        ];

        insert_events(&conn, &events).unwrap();

        let result = get_events_by_kind_and_pubkey(&conn, 31933, &pubkey1).unwrap();
        assert_eq!(result.len(), 1);
        assert_eq!(result[0].content, "Project 1");
    }
}
