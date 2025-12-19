use crate::models::{Message, Project, Thread};
use crate::store::events::{get_events_by_kind, get_events_by_kind_and_pubkey};
#[cfg(test)]
use crate::store::events::StoredEvent;
use anyhow::Result;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

pub fn get_projects(conn: &Arc<Mutex<Connection>>) -> Result<Vec<Project>> {
    let events = get_events_by_kind(conn, 31933)?;
    let projects: Vec<Project> = events
        .iter()
        .filter_map(|e| Project::from_event(e))
        .collect();
    Ok(projects)
}

pub fn get_threads_for_project(conn: &Arc<Mutex<Connection>>, project_a_tag: &str) -> Result<Vec<Thread>> {
    let events = get_events_by_kind(conn, 11)?;
    let threads: Vec<Thread> = events
        .iter()
        .filter_map(|e| Thread::from_event(e))
        .filter(|t| t.project_id == project_a_tag)
        .collect();
    Ok(threads)
}

pub fn get_messages_for_thread(conn: &Arc<Mutex<Connection>>, thread_id: &str) -> Result<Vec<Message>> {
    let events = get_events_by_kind(conn, 1111)?;
    let messages: Vec<Message> = events
        .iter()
        .filter_map(|e| Message::from_event(e))
        .filter(|m| m.thread_id == thread_id)
        .collect();
    Ok(messages)
}

pub fn get_profile_name(conn: &Arc<Mutex<Connection>>, pubkey: &str) -> String {
    if let Ok(events) = get_events_by_kind_and_pubkey(conn, 0, pubkey) {
        if let Some(event) = events.first() {
            if let Ok(profile) = serde_json::from_str::<serde_json::Value>(&event.content) {
                if let Some(name) = profile.get("display_name").or(profile.get("name")) {
                    if let Some(s) = name.as_str() {
                        if !s.is_empty() {
                            return s.to_string();
                        }
                    }
                }
            }
        }
    }
    format!("{}...", &pubkey[..8])
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{events::insert_events, Database};

    #[test]
    fn test_get_projects() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        let events = vec![StoredEvent {
            id: "a".repeat(64),
            pubkey: "b".repeat(64),
            kind: 31933,
            created_at: 1000,
            content: "Description".to_string(),
            tags: vec![
                vec!["d".to_string(), "proj1".to_string()],
                vec!["name".to_string(), "Project 1".to_string()],
            ],
            sig: "0".repeat(128),
        }];

        insert_events(&conn, &events).unwrap();

        let projects = get_projects(&conn).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "Project 1");
    }

    #[test]
    fn test_get_threads_for_project() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        let project_tag = format!("31933:{}:proj1", "b".repeat(64));

        let events = vec![
            StoredEvent {
                id: "1".repeat(64),
                pubkey: "a".repeat(64),
                kind: 11,
                created_at: 1000,
                content: "Thread 1".to_string(),
                tags: vec![
                    vec!["a".to_string(), project_tag.clone()],
                    vec!["title".to_string(), "First Thread".to_string()],
                ],
                sig: "0".repeat(128),
            },
            StoredEvent {
                id: "2".repeat(64),
                pubkey: "a".repeat(64),
                kind: 11,
                created_at: 2000,
                content: "Thread 2".to_string(),
                tags: vec![
                    vec!["a".to_string(), "31933:other:proj".to_string()],
                    vec!["title".to_string(), "Other Thread".to_string()],
                ],
                sig: "0".repeat(128),
            },
        ];

        insert_events(&conn, &events).unwrap();

        let threads = get_threads_for_project(&conn, &project_tag).unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].title, "First Thread");
    }

    #[test]
    fn test_get_messages_for_thread() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        let thread_id = "t".repeat(64);

        let events = vec![
            StoredEvent {
                id: "1".repeat(64),
                pubkey: "a".repeat(64),
                kind: 1111,
                created_at: 1000,
                content: "Message 1".to_string(),
                tags: vec![vec!["e".to_string(), thread_id.clone()]],
                sig: "0".repeat(128),
            },
            StoredEvent {
                id: "2".repeat(64),
                pubkey: "a".repeat(64),
                kind: 1111,
                created_at: 2000,
                content: "Message 2".to_string(),
                tags: vec![vec!["e".to_string(), "other".repeat(8)]],
                sig: "0".repeat(128),
            },
        ];

        insert_events(&conn, &events).unwrap();

        let messages = get_messages_for_thread(&conn, &thread_id).unwrap();
        assert_eq!(messages.len(), 1);
        assert_eq!(messages[0].content, "Message 1");
    }

    #[test]
    fn test_get_profile_name_with_display_name() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        let pubkey = "a".repeat(64);
        let profile_json = r#"{"display_name":"Alice","name":"alice"}"#;

        let events = vec![StoredEvent {
            id: "p".repeat(64),
            pubkey: pubkey.clone(),
            kind: 0,
            created_at: 1000,
            content: profile_json.to_string(),
            tags: vec![],
            sig: "0".repeat(128),
        }];

        insert_events(&conn, &events).unwrap();

        let name = get_profile_name(&conn, &pubkey);
        assert_eq!(name, "Alice");
    }

    #[test]
    fn test_get_profile_name_with_name_only() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        let pubkey = "b".repeat(64);
        let profile_json = r#"{"name":"bob"}"#;

        let events = vec![StoredEvent {
            id: "p".repeat(64),
            pubkey: pubkey.clone(),
            kind: 0,
            created_at: 1000,
            content: profile_json.to_string(),
            tags: vec![],
            sig: "0".repeat(128),
        }];

        insert_events(&conn, &events).unwrap();

        let name = get_profile_name(&conn, &pubkey);
        assert_eq!(name, "bob");
    }

    #[test]
    fn test_get_profile_name_fallback() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        let pubkey = "c".repeat(64);

        let name = get_profile_name(&conn, &pubkey);
        assert_eq!(name, "cccccccc...");
    }
}
