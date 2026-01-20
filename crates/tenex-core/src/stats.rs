use std::collections::HashMap;
use std::sync::{Arc, RwLock};

/// Stats for events received from the network
#[derive(Debug, Default, Clone)]
pub struct EventStats {
    /// Counts by kind -> project_a_tag -> count
    /// Empty string for project_a_tag means "no project" (global events like agent definitions)
    pub by_kind_project: HashMap<u16, HashMap<String, u64>>,
    /// Total count by kind
    pub by_kind_total: HashMap<u16, u64>,
    /// Total events received
    pub total: u64,
}

impl EventStats {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record(&mut self, kind: u16, project_a_tag: Option<&str>) {
        self.total += 1;
        *self.by_kind_total.entry(kind).or_insert(0) += 1;

        let project_key = project_a_tag.unwrap_or("").to_string();
        *self
            .by_kind_project
            .entry(kind)
            .or_default()
            .entry(project_key)
            .or_insert(0) += 1;
    }

    /// Get summary organized by project
    pub fn by_project(&self) -> HashMap<String, HashMap<u16, u64>> {
        let mut result: HashMap<String, HashMap<u16, u64>> = HashMap::new();

        for (kind, projects) in &self.by_kind_project {
            for (project, count) in projects {
                result
                    .entry(project.clone())
                    .or_default()
                    .insert(*kind, *count);
            }
        }

        result
    }

    /// Get list of kinds sorted by total count (descending)
    pub fn kinds_by_count(&self) -> Vec<(u16, u64)> {
        let mut kinds: Vec<_> = self.by_kind_total.iter().map(|(&k, &c)| (k, c)).collect();
        kinds.sort_by(|a, b| b.1.cmp(&a.1));
        kinds
    }
}

/// Thread-safe wrapper for event stats
#[derive(Debug, Clone)]
pub struct SharedEventStats {
    inner: Arc<RwLock<EventStats>>,
}

impl Default for SharedEventStats {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedEventStats {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(EventStats::new())),
        }
    }

    pub fn record(&self, kind: u16, project_a_tag: Option<&str>) {
        if let Ok(mut stats) = self.inner.write() {
            stats.record(kind, project_a_tag);
        }
    }

    pub fn snapshot(&self) -> EventStats {
        self.inner.read().map(|s| s.clone()).unwrap_or_default()
    }
}

/// Query nostrdb for cache statistics
pub fn query_ndb_stats(ndb: &nostrdb::Ndb) -> HashMap<u16, u64> {
    use nostrdb::{FilterBuilder, Transaction};

    let mut stats = HashMap::new();

    // Query for each known kind
    let kinds = [
        1,     // messages
        513,   // conversation metadata
        4129,  // agent lessons
        4199,  // agent definitions
        4201,  // nudges
        24010, // project status
        24133, // operations status
        31933, // projects
    ];

    let txn = Transaction::new(ndb).ok();
    if let Some(txn) = txn {
        for kind in kinds {
            let filter = FilterBuilder::new().kinds([kind as u64]).build();
            if let Ok(results) = ndb.query(&txn, &[filter], 100000) {
                stats.insert(kind, results.len() as u64);
            }
        }
    }

    stats
}

/// Query nostrdb for detailed cache statistics per kind and project
pub fn query_ndb_detailed_stats(ndb: &nostrdb::Ndb) -> HashMap<u16, HashMap<String, u64>> {
    use nostrdb::{FilterBuilder, Transaction};

    let mut stats: HashMap<u16, HashMap<String, u64>> = HashMap::new();

    let kinds = [
        1,     // messages
        513,   // conversation metadata
        4129,  // agent lessons
        4199,  // agent definitions
        4201,  // nudges
        24010, // project status
        24133, // operations status
        31933, // projects
    ];

    let txn = match Transaction::new(ndb) {
        Ok(t) => t,
        Err(_) => return stats,
    };

    for kind in kinds {
        let filter = FilterBuilder::new().kinds([kind as u64]).build();
        if let Ok(results) = ndb.query(&txn, &[filter], 100000) {
            let kind_stats = stats.entry(kind).or_default();

            for result in results {
                let note = ndb.get_note_by_key(&txn, result.note_key).ok();
                if let Some(note) = note {
                    // Try to extract project a-tag
                    let project_a_tag = extract_a_tag(&note);
                    let key = project_a_tag.unwrap_or_else(|| "(global)".to_string());
                    *kind_stats.entry(key).or_insert(0) += 1;
                }
            }
        }
    }

    stats
}

fn extract_a_tag(note: &nostrdb::Note) -> Option<String> {
    let tags = note.tags();
    for tag in tags.iter() {
        if tag.count() >= 2 {
            if let Some("a") = tag.get(0).and_then(|t| t.variant().str()) {
                return tag.get(1).and_then(|t| t.variant().str()).map(|s| s.to_string());
            }
        }
    }
    None
}
