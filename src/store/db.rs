use anyhow::Result;
use nostrdb::Ndb;
use rusqlite::Connection;
use std::path::Path;
use std::sync::{Arc, Mutex};

pub struct Database {
    #[allow(dead_code)] // Used in tests
    pub ndb: Arc<Ndb>,
    creds_conn: Arc<Mutex<Connection>>,
}

impl Database {
    /// Create a new Database with its own nostrdb instance
    /// Used in tests
    #[allow(dead_code)]
    pub fn new<P: AsRef<Path>>(db_dir: P) -> Result<Self> {
        let db_dir = db_dir.as_ref();
        std::fs::create_dir_all(db_dir)?;

        let config = nostrdb::Config::new();
        let ndb = Ndb::new(db_dir.to_str().unwrap_or("tenex_data"), &config)?;

        Self::with_ndb(Arc::new(ndb), db_dir)
    }

    /// Create a Database with a shared nostrdb instance
    pub fn with_ndb<P: AsRef<Path>>(ndb: Arc<Ndb>, db_dir: P) -> Result<Self> {
        let db_dir = db_dir.as_ref();
        std::fs::create_dir_all(db_dir)?;

        let creds_path = db_dir.join("credentials.db");
        let creds_conn = Connection::open(&creds_path)?;
        creds_conn.execute_batch(
            r#"
            CREATE TABLE IF NOT EXISTS credentials (
                id INTEGER PRIMARY KEY CHECK (id = 1),
                ncryptsec TEXT NOT NULL
            );
            "#,
        )?;

        Ok(Self {
            ndb,
            creds_conn: Arc::new(Mutex::new(creds_conn)),
        })
    }

    pub fn credentials_conn(&self) -> Arc<Mutex<Connection>> {
        self.creds_conn.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_database_creation() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();

        let txn = nostrdb::Transaction::new(&db.ndb).unwrap();
        let filter = nostrdb::Filter::new().kinds([31933]).build();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 0);
    }
}
