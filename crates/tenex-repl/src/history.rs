use rusqlite::{params, Connection};
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

fn now_secs() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

pub(crate) struct HistoryEntry {
    pub(crate) content: String,
    pub(crate) project_atag: Option<String>,
    pub(crate) source: String,
    pub(crate) updated_at: i64,
}

pub(crate) struct HistoryStore {
    conn: Connection,
}

impl HistoryStore {
    pub(crate) fn open(data_dir: &Path) -> rusqlite::Result<Self> {
        std::fs::create_dir_all(data_dir).ok();
        let db_path = data_dir.join("history.db");
        let conn = Connection::open(db_path)?;
        conn.execute_batch("PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL;")?;
        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entries (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                content TEXT NOT NULL,
                project_atag TEXT,
                source TEXT NOT NULL,
                nostr_event_id TEXT,
                created_at INTEGER NOT NULL,
                updated_at INTEGER NOT NULL
            );
            CREATE INDEX IF NOT EXISTS idx_project ON entries(project_atag);
            CREATE INDEX IF NOT EXISTS idx_updated ON entries(updated_at DESC);
            CREATE INDEX IF NOT EXISTS idx_source ON entries(source);
            CREATE UNIQUE INDEX IF NOT EXISTS idx_nostr_event ON entries(nostr_event_id) WHERE nostr_event_id IS NOT NULL;",
        )?;
        Ok(Self { conn })
    }

    pub(crate) fn upsert_draft(
        &self,
        draft_id: Option<i64>,
        content: &str,
        project_atag: Option<&str>,
    ) -> rusqlite::Result<i64> {
        let now = now_secs();
        if let Some(id) = draft_id {
            self.conn.execute(
                "UPDATE entries SET content = ?1, project_atag = ?2, updated_at = ?3 WHERE id = ?4 AND source = 'draft'",
                params![content, project_atag, now, id],
            )?;
            Ok(id)
        } else {
            self.conn.execute(
                "INSERT INTO entries (content, project_atag, source, created_at, updated_at) VALUES (?1, ?2, 'draft', ?3, ?3)",
                params![content, project_atag, now],
            )?;
            Ok(self.conn.last_insert_rowid())
        }
    }

    pub(crate) fn delete_draft(&self, id: i64) -> rusqlite::Result<()> {
        self.conn.execute(
            "DELETE FROM entries WHERE id = ?1 AND source = 'draft'",
            params![id],
        )?;
        Ok(())
    }

    pub(crate) fn record_sent(
        &self,
        content: &str,
        project_atag: Option<&str>,
        active_draft_id: Option<i64>,
    ) -> rusqlite::Result<()> {
        let now = now_secs();
        self.conn.execute(
            "INSERT INTO entries (content, project_atag, source, created_at, updated_at) VALUES (?1, ?2, 'sent', ?3, ?3)",
            params![content, project_atag, now],
        )?;
        if let Some(draft_id) = active_draft_id {
            self.delete_draft(draft_id)?;
        }
        Ok(())
    }

    pub(crate) fn import_kind1(
        &self,
        content: &str,
        project_atag: Option<&str>,
        event_id: &str,
        created_at: i64,
    ) -> rusqlite::Result<()> {
        self.conn.execute(
            "INSERT OR IGNORE INTO entries (content, project_atag, source, nostr_event_id, created_at, updated_at) VALUES (?1, ?2, 'kind1', ?3, ?4, ?4)",
            params![content, project_atag, event_id, created_at],
        )?;
        Ok(())
    }

    pub(crate) fn search(
        &self,
        query: &str,
        current_project: Option<&str>,
        active_draft_id: Option<i64>,
        limit: usize,
    ) -> rusqlite::Result<Vec<HistoryEntry>> {
        let pattern = format!("%{}%", query);
        let exclude_id = active_draft_id.unwrap_or(-1);

        let mut stmt = self.conn.prepare(
            "SELECT content, project_atag, source, updated_at
             FROM entries
             WHERE content LIKE ?1 AND id != ?4
             ORDER BY
                CASE WHEN project_atag = ?2 THEN 0
                     WHEN project_atag IS NULL THEN 1
                     ELSE 2 END,
                updated_at DESC
             LIMIT ?3",
        )?;

        let rows = stmt.query_map(
            params![pattern, current_project, limit as i64, exclude_id],
            |row| {
                Ok(HistoryEntry {
                    content: row.get(0)?,
                    project_atag: row.get(1)?,
                    source: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        )?;

        rows.collect()
    }

    pub(crate) fn search_all(
        &self,
        query: &str,
        active_draft_id: Option<i64>,
        limit: usize,
    ) -> rusqlite::Result<Vec<HistoryEntry>> {
        let pattern = format!("%{}%", query);
        let exclude_id = active_draft_id.unwrap_or(-1);

        let mut stmt = self.conn.prepare(
            "SELECT content, project_atag, source, updated_at
             FROM entries
             WHERE content LIKE ?1 AND id != ?3
             ORDER BY updated_at DESC
             LIMIT ?2",
        )?;

        let rows = stmt.query_map(
            params![pattern, limit as i64, exclude_id],
            |row| {
                Ok(HistoryEntry {
                    content: row.get(0)?,
                    project_atag: row.get(1)?,
                    source: row.get(2)?,
                    updated_at: row.get(3)?,
                })
            },
        )?;

        rows.collect()
    }
}

pub(crate) fn relative_time(timestamp: i64) -> String {
    let now = now_secs();
    let diff = (now - timestamp).max(0);
    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else {
        format!("{}d ago", diff / 86400)
    }
}
