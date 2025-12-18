# TENEX TUI Rust Client Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Build a terminal UI client for TENEX that mirrors the TypeScript/Bun version - SQLite-backed event store, Nostr protocol via nostr-sdk, ratatui TUI, encrypted nsec storage (NIP-49), project/thread/chat views, and OpenTelemetry tracing.

**Architecture:** Event-driven with message passing. App struct manages state and view transitions. SQLite stores all Nostr events. Computed views derive projects/threads/messages from event queries. Async Nostr operations via tokio runtime.

**Tech Stack:** Rust, ratatui, crossterm, nostr-sdk, rusqlite, tokio, tracing, tracing-opentelemetry

---

## Task 0: Initialize Rust Project

**Files:**
- Create: `Cargo.toml`
- Create: `src/main.rs`
- Create: `.gitignore`

**Step 1: Initialize cargo project**

Run: `cargo init`

**Step 2: Add dependencies to Cargo.toml**

```toml
[package]
name = "tenex-tui"
version = "0.1.0"
edition = "2021"

[dependencies]
# TUI
ratatui = "0.29"
crossterm = "0.28"

# Nostr
nostr-sdk = "0.38"

# Database
rusqlite = { version = "0.32", features = ["bundled"] }

# Async runtime
tokio = { version = "1", features = ["full"] }

# Tracing
tracing = "0.1"
tracing-subscriber = { version = "0.3", features = ["env-filter"] }
tracing-opentelemetry = "0.28"
opentelemetry = "0.27"
opentelemetry_sdk = { version = "0.27", features = ["rt-tokio"] }
opentelemetry-stdout = "0.27"

# Error handling
anyhow = "1"
thiserror = "2"

# Serialization
serde = { version = "1", features = ["derive"] }
serde_json = "1"

[dev-dependencies]
tempfile = "3"
```

**Step 3: Create basic main.rs**

```rust
fn main() {
    println!("TENEX TUI starting...");
}
```

**Step 4: Create .gitignore**

```
/target
*.db
.env
```

**Step 5: Build and verify**

Run: `cargo build`
Expected: Compilation success

**Step 6: Initialize git and commit**

```bash
git init
git add .
git commit -m "feat: initialize Rust project with dependencies"
```

---

## Task 1: SQLite Event Store

**Files:**
- Create: `src/store/mod.rs`
- Create: `src/store/db.rs`
- Create: `src/store/events.rs`
- Modify: `src/main.rs`

**Step 1: Create store module structure**

Create `src/store/mod.rs`:
```rust
pub mod db;
pub mod events;

pub use db::Database;
pub use events::{insert_events, get_events_by_kind, get_events_by_kind_and_pubkey};
```

**Step 2: Write failing test for database initialization**

Create `src/store/db.rs`:
```rust
use anyhow::Result;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Database {
    conn: Arc<Mutex<Connection>>,
}

impl Database {
    pub fn new<P: AsRef<Path>>(path: P) -> Result<Self> {
        let conn = Connection::open(path)?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    pub fn in_memory() -> Result<Self> {
        let conn = Connection::open_in_memory()?;
        let db = Self {
            conn: Arc::new(Mutex::new(conn)),
        };
        db.init_schema()?;
        Ok(db)
    }

    fn init_schema(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS events (
                id TEXT PRIMARY KEY,
                pubkey TEXT NOT NULL,
                kind INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                content TEXT NOT NULL,
                tags TEXT NOT NULL,
                sig TEXT NOT NULL
            );

            CREATE INDEX IF NOT EXISTS idx_events_kind ON events(kind);
            CREATE INDEX IF NOT EXISTS idx_events_pubkey ON events(pubkey);
            CREATE INDEX IF NOT EXISTS idx_events_created_at ON events(created_at);

            CREATE TABLE IF NOT EXISTS credentials (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                ncryptsec TEXT NOT NULL
            );
            "#,
        )?;
        Ok(())
    }

    pub fn connection(&self) -> Arc<Mutex<Connection>> {
        self.conn.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_database_creation() {
        let db = Database::in_memory().unwrap();
        let conn = db.conn.lock().unwrap();
        let count: i32 = conn
            .query_row("SELECT COUNT(*) FROM events", [], |row| row.get(0))
            .unwrap();
        assert_eq!(count, 0);
    }
}
```

**Step 3: Run test to verify it passes**

Run: `cargo test test_database_creation`
Expected: PASS

**Step 4: Write event insertion module**

Create `src/store/events.rs`:
```rust
use anyhow::Result;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::sync::{Arc, Mutex};

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

pub fn insert_events(conn: &Arc<Mutex<Connection>>, events: &[StoredEvent]) -> Result<usize> {
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
```

**Step 5: Update main.rs to include store module**

```rust
mod store;

fn main() {
    println!("TENEX TUI starting...");
}
```

**Step 6: Run all tests**

Run: `cargo test`
Expected: All tests PASS

**Step 7: Commit**

```bash
git add .
git commit -m "feat: add SQLite event store with insert and query"
```

---

## Task 2: Domain Models (Project, Thread, Message)

**Files:**
- Create: `src/models/mod.rs`
- Create: `src/models/project.rs`
- Create: `src/models/thread.rs`
- Create: `src/models/message.rs`
- Modify: `src/main.rs`

**Step 1: Create models module**

Create `src/models/mod.rs`:
```rust
pub mod project;
pub mod thread;
pub mod message;

pub use project::Project;
pub use thread::Thread;
pub use message::Message;
```

**Step 2: Create Project model**

Create `src/models/project.rs`:
```rust
use crate::store::events::StoredEvent;

#[derive(Debug, Clone)]
pub struct Project {
    pub id: String,
    pub name: String,
    pub description: String,
    pub pubkey: String,
    pub participants: Vec<String>,
    pub created_at: u64,
}

impl Project {
    pub fn from_event(event: &StoredEvent) -> Option<Self> {
        if event.kind != 31933 {
            return None;
        }

        let d_tag = event.tags.iter().find(|t| t.first().map(|s| s == "d").unwrap_or(false))?;
        let id = d_tag.get(1)?.clone();

        let name = event
            .tags
            .iter()
            .find(|t| t.first().map(|s| s == "name").unwrap_or(false))
            .and_then(|t| t.get(1))
            .cloned()
            .unwrap_or_else(|| id.clone());

        let participants: Vec<String> = event
            .tags
            .iter()
            .filter(|t| t.first().map(|s| s == "p").unwrap_or(false))
            .filter_map(|t| t.get(1).cloned())
            .collect();

        Some(Project {
            id,
            name,
            description: event.content.clone(),
            pubkey: event.pubkey.clone(),
            participants,
            created_at: event.created_at,
        })
    }

    pub fn a_tag(&self) -> String {
        format!("31933:{}:{}", self.pubkey, self.id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_project() {
        let event = StoredEvent {
            id: "a".repeat(64),
            pubkey: "b".repeat(64),
            kind: 31933,
            created_at: 1000,
            content: "Description".to_string(),
            tags: vec![
                vec!["d".to_string(), "my-project".to_string()],
                vec!["name".to_string(), "My Project".to_string()],
                vec!["p".to_string(), "c".repeat(64)],
            ],
            sig: "0".repeat(128),
        };

        let project = Project::from_event(&event).unwrap();
        assert_eq!(project.id, "my-project");
        assert_eq!(project.name, "My Project");
        assert_eq!(project.participants.len(), 1);
    }

    #[test]
    fn test_a_tag() {
        let project = Project {
            id: "proj1".to_string(),
            name: "Project 1".to_string(),
            description: "".to_string(),
            pubkey: "a".repeat(64),
            participants: vec![],
            created_at: 1000,
        };

        assert_eq!(project.a_tag(), format!("31933:{}:proj1", "a".repeat(64)));
    }
}
```

**Step 3: Create Thread model**

Create `src/models/thread.rs`:
```rust
use crate::store::events::StoredEvent;

#[derive(Debug, Clone)]
pub struct Thread {
    pub id: String,
    pub title: String,
    pub content: String,
    pub pubkey: String,
    pub project_id: String,
    pub created_at: u64,
}

impl Thread {
    pub fn from_event(event: &StoredEvent) -> Option<Self> {
        if event.kind != 11 {
            return None;
        }

        let a_tag = event.tags.iter().find(|t| t.first().map(|s| s == "a").unwrap_or(false))?;
        let project_id = a_tag.get(1)?.clone();

        let title = event
            .tags
            .iter()
            .find(|t| t.first().map(|s| s == "title").unwrap_or(false))
            .and_then(|t| t.get(1))
            .cloned()
            .unwrap_or_else(|| "Untitled".to_string());

        Some(Thread {
            id: event.id.clone(),
            title,
            content: event.content.clone(),
            pubkey: event.pubkey.clone(),
            project_id,
            created_at: event.created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_thread() {
        let event = StoredEvent {
            id: "a".repeat(64),
            pubkey: "b".repeat(64),
            kind: 11,
            created_at: 1000,
            content: "Thread content".to_string(),
            tags: vec![
                vec!["a".to_string(), "31933:pubkey:proj1".to_string()],
                vec!["title".to_string(), "My Thread".to_string()],
            ],
            sig: "0".repeat(128),
        };

        let thread = Thread::from_event(&event).unwrap();
        assert_eq!(thread.title, "My Thread");
        assert_eq!(thread.project_id, "31933:pubkey:proj1");
    }
}
```

**Step 4: Create Message model**

Create `src/models/message.rs`:
```rust
use crate::store::events::StoredEvent;

#[derive(Debug, Clone)]
pub struct Message {
    pub id: String,
    pub content: String,
    pub pubkey: String,
    pub thread_id: String,
    pub created_at: u64,
}

impl Message {
    pub fn from_event(event: &StoredEvent) -> Option<Self> {
        if event.kind != 1111 {
            return None;
        }

        let e_tag = event.tags.iter().find(|t| t.first().map(|s| s == "e").unwrap_or(false))?;
        let thread_id = e_tag.get(1)?.clone();

        Some(Message {
            id: event.id.clone(),
            content: event.content.clone(),
            pubkey: event.pubkey.clone(),
            thread_id,
            created_at: event.created_at,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_message() {
        let event = StoredEvent {
            id: "a".repeat(64),
            pubkey: "b".repeat(64),
            kind: 1111,
            created_at: 1000,
            content: "Hello world".to_string(),
            tags: vec![vec!["e".to_string(), "c".repeat(64), "".to_string(), "root".to_string()]],
            sig: "0".repeat(128),
        };

        let message = Message::from_event(&event).unwrap();
        assert_eq!(message.content, "Hello world");
        assert_eq!(message.thread_id, "c".repeat(64));
    }
}
```

**Step 5: Update main.rs**

```rust
mod models;
mod store;

fn main() {
    println!("TENEX TUI starting...");
}
```

**Step 6: Run tests**

Run: `cargo test`
Expected: All tests PASS

**Step 7: Commit**

```bash
git add .
git commit -m "feat: add domain models for Project, Thread, Message"
```

---

## Task 3: Nostr Client Setup

**Files:**
- Create: `src/nostr/mod.rs`
- Create: `src/nostr/client.rs`
- Create: `src/nostr/auth.rs`
- Modify: `src/main.rs`

**Step 1: Create nostr module**

Create `src/nostr/mod.rs`:
```rust
pub mod client;
pub mod auth;

pub use client::NostrClient;
pub use auth::{login_with_nsec, get_current_pubkey, is_logged_in};
```

**Step 2: Create auth module**

Create `src/nostr/auth.rs`:
```rust
use anyhow::{anyhow, Result};
use nostr_sdk::prelude::*;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

pub fn login_with_nsec(nsec: &str, password: Option<&str>, conn: &Arc<Mutex<Connection>>) -> Result<Keys> {
    let secret_key = SecretKey::parse(nsec)?;
    let keys = Keys::new(secret_key);

    // Store encrypted if password provided
    if let Some(pwd) = password {
        if !pwd.is_empty() {
            let encrypted = keys.secret_key().to_bech32_encrypted(pwd)?;
            store_credentials(conn, &encrypted)?;
        }
    } else {
        // Store unencrypted (as ncryptsec with empty password)
        store_credentials(conn, nsec)?;
    }

    Ok(keys)
}

pub fn load_stored_keys(password: &str, conn: &Arc<Mutex<Connection>>) -> Result<Keys> {
    let ncryptsec = get_stored_credentials(conn)?;

    // Try to decrypt
    let secret_key = if ncryptsec.starts_with("ncryptsec") {
        SecretKey::parse_encrypted(&ncryptsec, password)?
    } else {
        SecretKey::parse(&ncryptsec)?
    };

    Ok(Keys::new(secret_key))
}

pub fn has_stored_credentials(conn: &Arc<Mutex<Connection>>) -> bool {
    get_stored_credentials(conn).is_ok()
}

fn store_credentials(conn: &Arc<Mutex<Connection>>, ncryptsec: &str) -> Result<()> {
    let conn = conn.lock().unwrap();
    conn.execute(
        "INSERT OR REPLACE INTO credentials (id, ncryptsec) VALUES (1, ?1)",
        [ncryptsec],
    )?;
    Ok(())
}

fn get_stored_credentials(conn: &Arc<Mutex<Connection>>) -> Result<String> {
    let conn = conn.lock().unwrap();
    let result: String = conn.query_row(
        "SELECT ncryptsec FROM credentials WHERE id = 1",
        [],
        |row| row.get(0),
    )?;
    Ok(result)
}

pub fn clear_credentials(conn: &Arc<Mutex<Connection>>) -> Result<()> {
    let conn = conn.lock().unwrap();
    conn.execute("DELETE FROM credentials WHERE id = 1", [])?;
    Ok(())
}

pub fn get_current_pubkey(keys: &Keys) -> String {
    keys.public_key().to_hex()
}

pub fn is_logged_in(keys: Option<&Keys>) -> bool {
    keys.is_some()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Database;

    #[test]
    fn test_login_and_store() {
        let db = Database::in_memory().unwrap();
        let conn = db.connection();

        // Generate a test nsec
        let keys = Keys::generate();
        let nsec = keys.secret_key().to_bech32().unwrap();

        let result = login_with_nsec(&nsec, Some("password123"), &conn);
        assert!(result.is_ok());

        // Should be able to load back
        let loaded = load_stored_keys("password123", &conn);
        assert!(loaded.is_ok());
        assert_eq!(loaded.unwrap().public_key(), keys.public_key());
    }
}
```

**Step 3: Create client module**

Create `src/nostr/client.rs`:
```rust
use anyhow::Result;
use nostr_sdk::prelude::*;
use std::sync::Arc;
use tokio::sync::Mutex;

const DEFAULT_RELAYS: &[&str] = &[
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://relay.nostr.band",
];

pub struct NostrClient {
    client: Arc<Mutex<Client>>,
    keys: Keys,
}

impl NostrClient {
    pub async fn new(keys: Keys) -> Result<Self> {
        let client = Client::new(keys.clone());

        for relay in DEFAULT_RELAYS {
            client.add_relay(*relay).await?;
        }

        client.connect().await;

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            keys,
        })
    }

    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    pub fn pubkey(&self) -> String {
        self.keys.public_key().to_hex()
    }

    pub async fn subscribe(&self, filters: Vec<Filter>) -> Result<()> {
        let client = self.client.lock().await;
        client.subscribe(filters, None).await?;
        Ok(())
    }

    pub async fn fetch_events(&self, filters: Vec<Filter>) -> Result<Vec<Event>> {
        let client = self.client.lock().await;
        let events = client.fetch_events(filters, None).await?;
        Ok(events.into_iter().collect())
    }

    pub async fn publish(&self, event: EventBuilder) -> Result<EventId> {
        let client = self.client.lock().await;
        let output = client.send_event_builder(event).await?;
        Ok(output.id())
    }

    pub async fn disconnect(&self) -> Result<()> {
        let client = self.client.lock().await;
        client.disconnect().await?;
        Ok(())
    }
}
```

**Step 4: Update main.rs**

```rust
mod models;
mod nostr;
mod store;

fn main() {
    println!("TENEX TUI starting...");
}
```

**Step 5: Run tests**

Run: `cargo test`
Expected: All tests PASS

**Step 6: Commit**

```bash
git add .
git commit -m "feat: add Nostr client with auth and relay connections"
```

---

## Task 4: Computed Views (Projects, Threads, Messages)

**Files:**
- Create: `src/store/views.rs`
- Modify: `src/store/mod.rs`

**Step 1: Create views module**

Create `src/store/views.rs`:
```rust
use crate::models::{Message, Project, Thread};
use crate::store::events::{get_events_by_kind, get_events_by_kind_and_pubkey, StoredEvent};
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
}
```

**Step 2: Update store/mod.rs**

```rust
pub mod db;
pub mod events;
pub mod views;

pub use db::Database;
pub use events::{insert_events, get_events_by_kind, get_events_by_kind_and_pubkey, StoredEvent};
pub use views::{get_projects, get_threads_for_project, get_messages_for_thread, get_profile_name};
```

**Step 3: Run tests**

Run: `cargo test`
Expected: All tests PASS

**Step 4: Commit**

```bash
git add .
git commit -m "feat: add computed views for projects, threads, messages"
```

---

## Task 5: TUI App Shell with Ratatui

**Files:**
- Create: `src/ui/mod.rs`
- Create: `src/ui/app.rs`
- Create: `src/ui/terminal.rs`
- Modify: `src/main.rs`

**Step 1: Create terminal setup**

Create `src/ui/terminal.rs`:
```rust
use anyhow::Result;
use crossterm::{
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout};

pub type Tui = Terminal<CrosstermBackend<Stdout>>;

pub fn init() -> Result<Tui> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let terminal = Terminal::new(backend)?;
    Ok(terminal)
}

pub fn restore() -> Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}
```

**Step 2: Create app struct**

Create `src/ui/app.rs`:
```rust
use crate::models::{Message, Project, Thread};
use crate::nostr::NostrClient;
use crate::store::Database;
use nostr_sdk::Keys;
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq)]
pub enum View {
    Login,
    Projects,
    Threads,
    Chat,
}

#[derive(Debug, Clone, PartialEq)]
pub enum InputMode {
    Normal,
    Editing,
}

pub struct App {
    pub running: bool,
    pub view: View,
    pub input_mode: InputMode,
    pub input: String,
    pub cursor_position: usize,

    pub db: Arc<Database>,
    pub nostr_client: Option<NostrClient>,
    pub keys: Option<Keys>,

    pub projects: Vec<Project>,
    pub threads: Vec<Thread>,
    pub messages: Vec<Message>,

    pub selected_project_index: usize,
    pub selected_thread_index: usize,
    pub selected_project: Option<Project>,
    pub selected_thread: Option<Thread>,

    pub scroll_offset: usize,
    pub status_message: Option<String>,
}

impl App {
    pub fn new(db: Database) -> Self {
        Self {
            running: true,
            view: View::Login,
            input_mode: InputMode::Normal,
            input: String::new(),
            cursor_position: 0,

            db: Arc::new(db),
            nostr_client: None,
            keys: None,

            projects: Vec::new(),
            threads: Vec::new(),
            messages: Vec::new(),

            selected_project_index: 0,
            selected_thread_index: 0,
            selected_project: None,
            selected_thread: None,

            scroll_offset: 0,
            status_message: None,
        }
    }

    pub fn set_status(&mut self, msg: &str) {
        self.status_message = Some(msg.to_string());
    }

    pub fn clear_status(&mut self) {
        self.status_message = None;
    }

    pub fn quit(&mut self) {
        self.running = false;
    }

    pub fn move_cursor_left(&mut self) {
        if self.cursor_position > 0 {
            self.cursor_position -= 1;
        }
    }

    pub fn move_cursor_right(&mut self) {
        if self.cursor_position < self.input.len() {
            self.cursor_position += 1;
        }
    }

    pub fn enter_char(&mut self, c: char) {
        self.input.insert(self.cursor_position, c);
        self.cursor_position += 1;
    }

    pub fn delete_char(&mut self) {
        if self.cursor_position > 0 && !self.input.is_empty() {
            self.cursor_position -= 1;
            self.input.remove(self.cursor_position);
        }
    }

    pub fn clear_input(&mut self) {
        self.input.clear();
        self.cursor_position = 0;
    }

    pub fn submit_input(&mut self) -> String {
        let input = self.input.clone();
        self.clear_input();
        input
    }
}
```

**Step 3: Create ui module**

Create `src/ui/mod.rs`:
```rust
pub mod app;
pub mod terminal;

pub use app::{App, View, InputMode};
pub use terminal::{init as init_terminal, restore as restore_terminal, Tui};
```

**Step 4: Update main.rs with basic app loop**

```rust
mod models;
mod nostr;
mod store;
mod ui;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::time::Duration;
use ui::{App, View, InputMode};

fn main() -> Result<()> {
    let db = store::Database::new("tenex.db")?;
    let mut app = App::new(db);
    let mut terminal = ui::init_terminal()?;

    let result = run_app(&mut terminal, &mut app);

    ui::restore_terminal()?;

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }

    Ok(())
}

fn run_app(terminal: &mut ui::Tui, app: &mut App) -> Result<()> {
    while app.running {
        terminal.draw(|f| render(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code);
                }
            }
        }
    }
    Ok(())
}

fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(f.area());

    // Header
    let title = match app.view {
        View::Login => "TENEX - Login",
        View::Projects => "TENEX - Projects",
        View::Threads => "TENEX - Threads",
        View::Chat => "TENEX - Chat",
    };
    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, chunks[0]);

    // Main content
    let content = match app.view {
        View::Login => "Enter your nsec to login:\n\nPress 'i' to start typing, Enter to submit",
        _ => "Content area",
    };
    let main = Paragraph::new(content).block(Block::default().borders(Borders::NONE));
    f.render_widget(main, chunks[1]);

    // Footer / input
    let footer_text = if app.input_mode == InputMode::Editing {
        format!("> {}", app.input)
    } else {
        "Press 'q' to quit, 'i' to edit".to_string()
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[2]);
}

fn handle_key(app: &mut App, key: KeyCode) {
    match app.input_mode {
        InputMode::Normal => match key {
            KeyCode::Char('q') => app.quit(),
            KeyCode::Char('i') => app.input_mode = InputMode::Editing,
            KeyCode::Up => {
                match app.view {
                    View::Projects if app.selected_project_index > 0 => {
                        app.selected_project_index -= 1;
                    }
                    View::Threads if app.selected_thread_index > 0 => {
                        app.selected_thread_index -= 1;
                    }
                    _ => {}
                }
            }
            KeyCode::Down => {
                match app.view {
                    View::Projects if app.selected_project_index < app.projects.len().saturating_sub(1) => {
                        app.selected_project_index += 1;
                    }
                    View::Threads if app.selected_thread_index < app.threads.len().saturating_sub(1) => {
                        app.selected_thread_index += 1;
                    }
                    _ => {}
                }
            }
            KeyCode::Enter => {
                // Select item based on view
            }
            KeyCode::Esc => {
                match app.view {
                    View::Threads => app.view = View::Projects,
                    View::Chat => app.view = View::Threads,
                    _ => {}
                }
            }
            _ => {}
        },
        InputMode::Editing => match key {
            KeyCode::Esc => app.input_mode = InputMode::Normal,
            KeyCode::Char(c) => app.enter_char(c),
            KeyCode::Backspace => app.delete_char(),
            KeyCode::Left => app.move_cursor_left(),
            KeyCode::Right => app.move_cursor_right(),
            KeyCode::Enter => {
                let _input = app.submit_input();
                app.input_mode = InputMode::Normal;
            }
            _ => {}
        },
    }
}
```

**Step 5: Run to verify it compiles**

Run: `cargo run`
Expected: TUI appears with login view, 'q' quits

**Step 6: Commit**

```bash
git add .
git commit -m "feat: add TUI shell with ratatui and basic navigation"
```

---

## Task 6: Login View

**Files:**
- Create: `src/ui/views/mod.rs`
- Create: `src/ui/views/login.rs`
- Modify: `src/ui/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create login view**

Create `src/ui/views/mod.rs`:
```rust
pub mod login;

pub use login::render_login;
```

Create `src/ui/views/login.rs`:
```rust
use crate::ui::{App, InputMode};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

#[derive(Debug, Clone, PartialEq)]
pub enum LoginStep {
    Nsec,
    Password,
}

pub fn render_login(f: &mut Frame, app: &App, area: Rect, login_step: &LoginStep) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .split(area);

    // Instructions
    let instructions = match login_step {
        LoginStep::Nsec => "Enter your nsec (private key) to login:",
        LoginStep::Password => "Enter a password to encrypt your key (optional, press Enter to skip):",
    };
    let instruction_widget = Paragraph::new(instructions)
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center);
    f.render_widget(instruction_widget, chunks[0]);

    // Input field
    let input_style = if app.input_mode == InputMode::Editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let display_text = if *login_step == LoginStep::Nsec && !app.input.is_empty() {
        // Mask the nsec
        "*".repeat(app.input.len())
    } else if *login_step == LoginStep::Password && !app.input.is_empty() {
        "*".repeat(app.input.len())
    } else {
        app.input.clone()
    };

    let input_widget = Paragraph::new(display_text)
        .style(input_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if app.input_mode == InputMode::Editing {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::DarkGray)
                })
                .title(if app.input_mode == InputMode::Editing {
                    "Editing (Esc to cancel, Enter to submit)"
                } else {
                    "Press 'i' to start typing"
                }),
        );
    f.render_widget(input_widget, chunks[1]);

    // Status
    if let Some(ref msg) = app.status_message {
        let status = Paragraph::new(msg.as_str())
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center);
        f.render_widget(status, chunks[2]);
    }
}
```

**Step 2: Update ui/mod.rs**

```rust
pub mod app;
pub mod terminal;
pub mod views;

pub use app::{App, View, InputMode};
pub use terminal::{init as init_terminal, restore as restore_terminal, Tui};
```

**Step 3: Update main.rs to use login view**

Update the render function and add login step state. This involves more complex changes - update main.rs:

```rust
mod models;
mod nostr;
mod store;
mod ui;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::time::Duration;
use ui::{App, View, InputMode};
use ui::views::login::{render_login, LoginStep};

fn main() -> Result<()> {
    let db = store::Database::new("tenex.db")?;
    let mut app = App::new(db);
    let mut terminal = ui::init_terminal()?;
    let mut login_step = LoginStep::Nsec;
    let mut pending_nsec: Option<String> = None;

    let result = run_app(&mut terminal, &mut app, &mut login_step, &mut pending_nsec);

    ui::restore_terminal()?;

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }

    Ok(())
}

fn run_app(
    terminal: &mut ui::Tui,
    app: &mut App,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
) -> Result<()> {
    while app.running {
        terminal.draw(|f| render(f, app, login_step))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code, login_step, pending_nsec);
                }
            }
        }
    }
    Ok(())
}

fn render(f: &mut Frame, app: &App, login_step: &LoginStep) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(f.area());

    // Header
    let title = match app.view {
        View::Login => "TENEX - Login",
        View::Projects => "TENEX - Projects",
        View::Threads => "TENEX - Threads",
        View::Chat => "TENEX - Chat",
    };
    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, chunks[0]);

    // Main content
    match app.view {
        View::Login => render_login(f, app, chunks[1], login_step),
        _ => {
            let main = Paragraph::new("Content area").block(Block::default());
            f.render_widget(main, chunks[1]);
        }
    }

    // Footer
    let footer_text = match app.input_mode {
        InputMode::Editing => format!("> {}", "*".repeat(app.input.len())),
        InputMode::Normal => "Press 'q' to quit".to_string(),
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[2]);
}

fn handle_key(
    app: &mut App,
    key: KeyCode,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
) {
    match app.input_mode {
        InputMode::Normal => match key {
            KeyCode::Char('q') => app.quit(),
            KeyCode::Char('i') => app.input_mode = InputMode::Editing,
            KeyCode::Up => {
                match app.view {
                    View::Projects if app.selected_project_index > 0 => {
                        app.selected_project_index -= 1;
                    }
                    View::Threads if app.selected_thread_index > 0 => {
                        app.selected_thread_index -= 1;
                    }
                    _ => {}
                }
            }
            KeyCode::Down => {
                match app.view {
                    View::Projects if app.selected_project_index < app.projects.len().saturating_sub(1) => {
                        app.selected_project_index += 1;
                    }
                    View::Threads if app.selected_thread_index < app.threads.len().saturating_sub(1) => {
                        app.selected_thread_index += 1;
                    }
                    _ => {}
                }
            }
            KeyCode::Enter => {}
            KeyCode::Esc => {
                match app.view {
                    View::Threads => app.view = View::Projects,
                    View::Chat => app.view = View::Threads,
                    _ => {}
                }
            }
            _ => {}
        },
        InputMode::Editing => match key {
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
                app.clear_input();
            }
            KeyCode::Char(c) => app.enter_char(c),
            KeyCode::Backspace => app.delete_char(),
            KeyCode::Left => app.move_cursor_left(),
            KeyCode::Right => app.move_cursor_right(),
            KeyCode::Enter => {
                let input = app.submit_input();
                app.input_mode = InputMode::Normal;

                if app.view == View::Login {
                    match login_step {
                        LoginStep::Nsec => {
                            if input.starts_with("nsec") {
                                *pending_nsec = Some(input);
                                *login_step = LoginStep::Password;
                            } else {
                                app.set_status("Invalid nsec format");
                            }
                        }
                        LoginStep::Password => {
                            if let Some(ref nsec) = pending_nsec {
                                let password = if input.is_empty() { None } else { Some(input.as_str()) };
                                match nostr::auth::login_with_nsec(nsec, password, &app.db.connection()) {
                                    Ok(keys) => {
                                        app.keys = Some(keys);
                                        app.view = View::Projects;
                                        app.clear_status();
                                    }
                                    Err(e) => {
                                        app.set_status(&format!("Login failed: {}", e));
                                        *login_step = LoginStep::Nsec;
                                    }
                                }
                            }
                            *pending_nsec = None;
                        }
                    }
                }
            }
            _ => {}
        },
    }
}
```

**Step 4: Run to verify**

Run: `cargo run`
Expected: Login view with nsec input, password step

**Step 5: Commit**

```bash
git add .
git commit -m "feat: add login view with nsec and password flow"
```

---

## Task 7: Projects View

**Files:**
- Create: `src/ui/views/projects.rs`
- Modify: `src/ui/views/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create projects view**

Create `src/ui/views/projects.rs`:
```rust
use crate::models::Project;
use crate::store::get_profile_name;
use crate::ui::App;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

pub fn render_projects(f: &mut Frame, app: &App, area: Rect) {
    if app.projects.is_empty() {
        let empty = ratatui::widgets::Paragraph::new("No projects found. Create a project to get started.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = app
        .projects
        .iter()
        .enumerate()
        .map(|(i, project)| {
            let is_selected = i == app.selected_project_index;
            let prefix = if is_selected { "▶ " } else { "  " };

            let owner_name = get_profile_name(&app.db.connection(), &project.pubkey);
            let info = format!(
                "{} participant(s) · Owner: {}",
                project.participants.len(),
                owner_name
            );

            let style = if is_selected {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::from(Span::styled(format!("{}{}", prefix, project.name), style)),
                Line::from(Span::styled(format!("  {}", info), Style::default().fg(Color::DarkGray))),
            ];

            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::NONE)
                .title("Use ↑/↓ to navigate, Enter to select"),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));

    let mut state = ListState::default();
    state.select(Some(app.selected_project_index));

    f.render_stateful_widget(list, area, &mut state);
}
```

**Step 2: Update views/mod.rs**

```rust
pub mod login;
pub mod projects;

pub use login::{render_login, LoginStep};
pub use projects::render_projects;
```

**Step 3: Update main.rs render function**

Add projects view rendering and load projects when entering projects view:

In the render function, update the match:
```rust
match app.view {
    View::Login => render_login(f, app, chunks[1], login_step),
    View::Projects => ui::views::render_projects(f, app, chunks[1]),
    _ => {
        let main = Paragraph::new("Content area").block(Block::default());
        f.render_widget(main, chunks[1]);
    }
}
```

And add project loading after successful login:
```rust
Ok(keys) => {
    app.keys = Some(keys);
    // Load projects
    if let Ok(projects) = store::get_projects(&app.db.connection()) {
        app.projects = projects;
    }
    app.view = View::Projects;
    app.clear_status();
}
```

**Step 4: Run to verify**

Run: `cargo run`
Expected: After login, shows projects list (empty if no projects)

**Step 5: Commit**

```bash
git add .
git commit -m "feat: add projects list view"
```

---

## Task 8: Threads View

**Files:**
- Create: `src/ui/views/threads.rs`
- Modify: `src/ui/views/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create threads view**

Create `src/ui/views/threads.rs`:
```rust
use crate::models::Thread;
use crate::store::get_profile_name;
use crate::ui::App;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

pub fn render_threads(f: &mut Frame, app: &App, area: Rect) {
    if app.threads.is_empty() {
        let empty = Paragraph::new("No threads found. Press 'n' to create a new thread.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = app
        .threads
        .iter()
        .enumerate()
        .map(|(i, thread)| {
            let is_selected = i == app.selected_thread_index;
            let prefix = if is_selected { "▶ " } else { "  " };

            let author_name = get_profile_name(&app.db.connection(), &thread.pubkey);

            let style = if is_selected {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::from(Span::styled(format!("{}{}", prefix, thread.title), style)),
                Line::from(Span::styled(
                    format!("  by {}", author_name),
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            ListItem::new(content)
        })
        .collect();

    let project_name = app
        .selected_project
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::NONE)
                .title(format!("{} - Threads (Esc to go back, 'n' for new)", project_name)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));

    let mut state = ListState::default();
    state.select(Some(app.selected_thread_index));

    f.render_stateful_widget(list, area, &mut state);
}
```

**Step 2: Update views/mod.rs**

```rust
pub mod login;
pub mod projects;
pub mod threads;

pub use login::{render_login, LoginStep};
pub use projects::render_projects;
pub use threads::render_threads;
```

**Step 3: Update main.rs**

Update render to include threads view:
```rust
View::Threads => ui::views::render_threads(f, app, chunks[1]),
```

Add Enter key handling for projects to navigate to threads:
```rust
KeyCode::Enter => {
    match app.view {
        View::Projects if !app.projects.is_empty() => {
            let project = app.projects[app.selected_project_index].clone();
            app.selected_project = Some(project.clone());
            // Load threads for this project
            if let Ok(threads) = store::get_threads_for_project(&app.db.connection(), &project.a_tag()) {
                app.threads = threads;
            }
            app.selected_thread_index = 0;
            app.view = View::Threads;
        }
        View::Threads if !app.threads.is_empty() => {
            let thread = app.threads[app.selected_thread_index].clone();
            app.selected_thread = Some(thread.clone());
            // Load messages for this thread
            if let Ok(messages) = store::get_messages_for_thread(&app.db.connection(), &thread.id) {
                app.messages = messages;
            }
            app.view = View::Chat;
        }
        _ => {}
    }
}
```

**Step 4: Run to verify**

Run: `cargo run`
Expected: Can navigate from projects to threads

**Step 5: Commit**

```bash
git add .
git commit -m "feat: add threads list view with navigation"
```

---

## Task 9: Chat View

**Files:**
- Create: `src/ui/views/chat.rs`
- Modify: `src/ui/views/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create chat view**

Create `src/ui/views/chat.rs`:
```rust
use crate::models::Message;
use crate::store::get_profile_name;
use crate::ui::{App, InputMode};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState, Wrap},
    Frame,
};

pub fn render_chat(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(area);

    // Messages area
    let thread_title = app
        .selected_thread
        .as_ref()
        .map(|t| t.title.clone())
        .unwrap_or_else(|| "Chat".to_string());

    if app.messages.is_empty() {
        let empty = Paragraph::new("No messages yet. Press 'i' to start typing.")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(thread_title));
        f.render_widget(empty, chunks[0]);
    } else {
        let messages_text: Vec<Line> = app
            .messages
            .iter()
            .rev() // Show oldest first
            .flat_map(|msg| {
                let author = get_profile_name(&app.db.connection(), &msg.pubkey);
                vec![
                    Line::from(Span::styled(
                        format!("{}: ", author),
                        Style::default().fg(Color::Cyan),
                    )),
                    Line::from(Span::styled(
                        msg.content.clone(),
                        Style::default().fg(Color::White),
                    )),
                    Line::from(""),
                ]
            })
            .collect();

        let messages = Paragraph::new(messages_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("{} (Esc to go back)", thread_title)),
            )
            .wrap(Wrap { trim: false })
            .scroll((app.scroll_offset as u16, 0));

        f.render_widget(messages, chunks[0]);
    }

    // Input area
    let input_style = if app.input_mode == InputMode::Editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let input = Paragraph::new(app.input.as_str())
        .style(input_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(input_style)
                .title(if app.input_mode == InputMode::Editing {
                    "Type your message (Enter to send, Esc to cancel)"
                } else {
                    "Press 'i' to compose"
                }),
        );
    f.render_widget(input, chunks[1]);

    // Show cursor in input mode
    if app.input_mode == InputMode::Editing {
        f.set_cursor_position((
            chunks[1].x + app.cursor_position as u16 + 1,
            chunks[1].y + 1,
        ));
    }
}
```

**Step 2: Update views/mod.rs**

```rust
pub mod login;
pub mod projects;
pub mod threads;
pub mod chat;

pub use login::{render_login, LoginStep};
pub use projects::render_projects;
pub use threads::render_threads;
pub use chat::render_chat;
```

**Step 3: Update main.rs render**

```rust
View::Chat => ui::views::render_chat(f, app, chunks[1]),
```

**Step 4: Run to verify**

Run: `cargo run`
Expected: Chat view shows messages, input field at bottom

**Step 5: Commit**

```bash
git add .
git commit -m "feat: add chat view with message display and input"
```

---

## Task 10: Nostr Subscriptions

**Files:**
- Create: `src/nostr/subscriptions.rs`
- Modify: `src/nostr/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create subscriptions module**

Create `src/nostr/subscriptions.rs`:
```rust
use crate::nostr::NostrClient;
use crate::store::{events::StoredEvent, insert_events};
use anyhow::Result;
use nostr_sdk::prelude::*;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

pub async fn subscribe_to_projects(client: &NostrClient, user_pubkey: &str, conn: &Arc<Mutex<Connection>>) -> Result<()> {
    let pubkey = PublicKey::parse(user_pubkey)?;

    let filter = Filter::new()
        .kind(Kind::Custom(31933))
        .author(pubkey);

    let events = client.fetch_events(vec![filter]).await?;

    let stored: Vec<StoredEvent> = events
        .into_iter()
        .map(|e| StoredEvent {
            id: e.id.to_hex(),
            pubkey: e.pubkey.to_hex(),
            kind: e.kind.as_u16() as u32,
            created_at: e.created_at.as_u64(),
            content: e.content.clone(),
            tags: e.tags.iter().map(|t| t.as_slice().iter().map(|s| s.to_string()).collect()).collect(),
            sig: e.sig.to_string(),
        })
        .collect();

    insert_events(conn, &stored)?;
    Ok(())
}

pub async fn subscribe_to_project_content(
    client: &NostrClient,
    project_a_tag: &str,
    conn: &Arc<Mutex<Connection>>,
) -> Result<()> {
    // Subscribe to threads (kind 11)
    let thread_filter = Filter::new()
        .kind(Kind::Custom(11))
        .custom_tag(SingleLetterTag::lowercase(Alphabet::A), [project_a_tag]);

    let thread_events = client.fetch_events(vec![thread_filter]).await?;

    let stored_threads: Vec<StoredEvent> = thread_events
        .iter()
        .map(|e| StoredEvent {
            id: e.id.to_hex(),
            pubkey: e.pubkey.to_hex(),
            kind: e.kind.as_u16() as u32,
            created_at: e.created_at.as_u64(),
            content: e.content.clone(),
            tags: e.tags.iter().map(|t| t.as_slice().iter().map(|s| s.to_string()).collect()).collect(),
            sig: e.sig.to_string(),
        })
        .collect();

    insert_events(conn, &stored_threads)?;

    // Subscribe to messages (kind 1111) for each thread
    let thread_ids: Vec<EventId> = thread_events.iter().map(|e| e.id).collect();

    if !thread_ids.is_empty() {
        let message_filter = Filter::new()
            .kind(Kind::Custom(1111))
            .events(thread_ids);

        let message_events = client.fetch_events(vec![message_filter]).await?;

        let stored_messages: Vec<StoredEvent> = message_events
            .into_iter()
            .map(|e| StoredEvent {
                id: e.id.to_hex(),
                pubkey: e.pubkey.to_hex(),
                kind: e.kind.as_u16() as u32,
                created_at: e.created_at.as_u64(),
                content: e.content.clone(),
                tags: e.tags.iter().map(|t| t.as_slice().iter().map(|s| s.to_string()).collect()).collect(),
                sig: e.sig.to_string(),
            })
            .collect();

        insert_events(conn, &stored_messages)?;
    }

    Ok(())
}

pub async fn subscribe_to_profiles(client: &NostrClient, pubkeys: &[String], conn: &Arc<Mutex<Connection>>) -> Result<()> {
    if pubkeys.is_empty() {
        return Ok(());
    }

    let pks: Vec<PublicKey> = pubkeys
        .iter()
        .filter_map(|p| PublicKey::parse(p).ok())
        .collect();

    if pks.is_empty() {
        return Ok(());
    }

    let filter = Filter::new()
        .kind(Kind::Metadata)
        .authors(pks);

    let events = client.fetch_events(vec![filter]).await?;

    let stored: Vec<StoredEvent> = events
        .into_iter()
        .map(|e| StoredEvent {
            id: e.id.to_hex(),
            pubkey: e.pubkey.to_hex(),
            kind: e.kind.as_u16() as u32,
            created_at: e.created_at.as_u64(),
            content: e.content.clone(),
            tags: e.tags.iter().map(|t| t.as_slice().iter().map(|s| s.to_string()).collect()).collect(),
            sig: e.sig.to_string(),
        })
        .collect();

    insert_events(conn, &stored)?;
    Ok(())
}
```

**Step 2: Update nostr/mod.rs**

```rust
pub mod client;
pub mod auth;
pub mod subscriptions;

pub use client::NostrClient;
pub use auth::{login_with_nsec, load_stored_keys, has_stored_credentials, get_current_pubkey, is_logged_in, clear_credentials};
pub use subscriptions::{subscribe_to_projects, subscribe_to_project_content, subscribe_to_profiles};
```

**Step 3: Update main.rs**

Add tokio runtime and async subscription calls. This requires significant refactoring:

```rust
mod models;
mod nostr;
mod store;
mod ui;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::time::Duration;
use tokio::runtime::Runtime;
use ui::{App, View, InputMode};
use ui::views::login::LoginStep;

fn main() -> Result<()> {
    let rt = Runtime::new()?;
    let db = store::Database::new("tenex.db")?;
    let mut app = App::new(db);
    let mut terminal = ui::init_terminal()?;
    let mut login_step = LoginStep::Nsec;
    let mut pending_nsec: Option<String> = None;

    let result = run_app(&mut terminal, &mut app, &mut login_step, &mut pending_nsec, &rt);

    ui::restore_terminal()?;

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }

    Ok(())
}

fn run_app(
    terminal: &mut ui::Tui,
    app: &mut App,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
    rt: &Runtime,
) -> Result<()> {
    while app.running {
        terminal.draw(|f| render(f, app, login_step))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code, login_step, pending_nsec, rt);
                }
            }
        }
    }
    Ok(())
}

// ... rest of the code with rt passed to handle_key for async operations
```

**Step 4: Run to verify**

Run: `cargo run`
Expected: Compiles with tokio runtime

**Step 5: Commit**

```bash
git add .
git commit -m "feat: add Nostr subscriptions for projects, threads, profiles"
```

---

## Task 11: Publishing Messages and Threads

**Files:**
- Create: `src/nostr/publish.rs`
- Modify: `src/nostr/mod.rs`
- Modify: `src/main.rs`

**Step 1: Create publish module**

Create `src/nostr/publish.rs`:
```rust
use crate::nostr::NostrClient;
use anyhow::Result;
use nostr_sdk::prelude::*;

pub async fn publish_message(
    client: &NostrClient,
    thread_id: &str,
    content: &str,
) -> Result<EventId> {
    let thread_event_id = EventId::parse(thread_id)?;

    let event = EventBuilder::new(
        Kind::Custom(1111),
        content,
    )
    .tag(Tag::event(thread_event_id));

    let id = client.publish(event).await?;
    Ok(id)
}

pub async fn publish_thread(
    client: &NostrClient,
    project_a_tag: &str,
    title: &str,
    content: &str,
) -> Result<EventId> {
    let event = EventBuilder::new(
        Kind::Custom(11),
        content,
    )
    .tag(Tag::custom(
        TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
        [project_a_tag],
    ))
    .tag(Tag::custom(
        TagKind::Custom("title".into()),
        [title],
    ));

    let id = client.publish(event).await?;
    Ok(id)
}
```

**Step 2: Update nostr/mod.rs**

```rust
pub mod client;
pub mod auth;
pub mod subscriptions;
pub mod publish;

pub use client::NostrClient;
pub use auth::{login_with_nsec, load_stored_keys, has_stored_credentials, get_current_pubkey, is_logged_in, clear_credentials};
pub use subscriptions::{subscribe_to_projects, subscribe_to_project_content, subscribe_to_profiles};
pub use publish::{publish_message, publish_thread};
```

**Step 3: Update main.rs to handle message publishing**

In handle_key, for the Chat view with Enter key in editing mode:
```rust
InputMode::Editing => match key {
    // ... other cases
    KeyCode::Enter => {
        let input = app.submit_input();
        app.input_mode = InputMode::Normal;

        match app.view {
            View::Login => { /* existing login logic */ }
            View::Chat => {
                if !input.is_empty() {
                    if let (Some(ref client), Some(ref thread)) = (&app.nostr_client, &app.selected_thread) {
                        let thread_id = thread.id.clone();
                        let content = input.clone();
                        let conn = app.db.connection();

                        rt.block_on(async {
                            if let Err(e) = nostr::publish_message(client, &thread_id, &content).await {
                                // Handle error
                            } else {
                                // Refresh messages
                                if let Ok(messages) = store::get_messages_for_thread(&conn, &thread_id) {
                                    // Update app.messages
                                }
                            }
                        });
                    }
                }
            }
            _ => {}
        }
    }
}
```

**Step 4: Run to verify**

Run: `cargo run`
Expected: Can send messages in chat

**Step 5: Commit**

```bash
git add .
git commit -m "feat: add message and thread publishing"
```

---

## Task 12: OpenTelemetry Tracing

**Files:**
- Create: `src/tracing_setup.rs`
- Modify: `src/main.rs`
- Modify: `src/store/events.rs`

**Step 1: Create tracing setup**

Create `src/tracing_setup.rs`:
```rust
use opentelemetry::trace::TracerProvider;
use opentelemetry_sdk::trace::SdkTracerProvider;
use opentelemetry_stdout::SpanExporter;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, Layer};

pub fn init_tracing() {
    let provider = SdkTracerProvider::builder()
        .with_simple_exporter(SpanExporter::default())
        .build();

    let tracer = provider.tracer("tenex-tui");
    let telemetry = tracing_opentelemetry::layer().with_tracer(tracer);

    tracing_subscriber::registry()
        .with(telemetry.with_filter(tracing_subscriber::filter::LevelFilter::INFO))
        .init();
}
```

**Step 2: Update store/events.rs with tracing**

```rust
use tracing::{info_span, instrument};

#[instrument(skip(conn, events), fields(event_count = events.len()))]
pub fn insert_events(conn: &Arc<Mutex<Connection>>, events: &[StoredEvent]) -> Result<usize> {
    let _span = info_span!("store.insert").entered();
    // ... existing code
}
```

**Step 3: Update main.rs**

```rust
mod tracing_setup;

fn main() -> Result<()> {
    tracing_setup::init_tracing();
    // ... rest
}
```

**Step 4: Run to verify**

Run: `cargo run`
Expected: Trace output visible

**Step 5: Commit**

```bash
git add .
git commit -m "feat: add OpenTelemetry tracing"
```

---

## Task 13: Final Integration and Polish

**Files:**
- Modify: `src/main.rs` (complete integration)
- Create: `README.md`

**Step 1: Final integration**

Ensure all pieces work together:
- Login flow with stored credentials
- Project subscription on login
- Thread navigation with content subscription
- Message sending and receiving
- Proper cleanup on exit

**Step 2: Create README**

```markdown
# TENEX TUI Client (Rust)

A terminal user interface client for TENEX built with Rust.

## Features

- SQLite-backed event store
- Nostr protocol integration via nostr-sdk
- Encrypted nsec storage (NIP-49)
- Project, thread, and chat views
- OpenTelemetry tracing

## Usage

```bash
cargo run
```

## Controls

- `i` - Start editing/input mode
- `Esc` - Cancel editing / go back
- `↑/↓` - Navigate lists
- `Enter` - Select item / submit input
- `q` - Quit

## Build

```bash
cargo build --release
```
```

**Step 3: Run final verification**

Run: `cargo run`
Expected: Full application works end-to-end

**Step 4: Final commit**

```bash
git add .
git commit -m "feat: complete TENEX TUI Rust client"
```

---

Plan complete and saved to `docs/plans/2025-12-19-tenex-tui-rust.md`. Two execution options:

**1. Subagent-Driven (this session)** - I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** - Open new session with executing-plans, batch execution with checkpoints

Which approach?
