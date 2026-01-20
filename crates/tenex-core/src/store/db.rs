use anyhow::Result;
use nostrdb::Ndb;
use std::path::Path;
use std::sync::Arc;

pub struct Database {
    pub ndb: Arc<Ndb>,
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

        Ok(Self { ndb })
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
