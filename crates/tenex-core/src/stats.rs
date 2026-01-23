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

/// Result of an e-tag query
#[derive(Debug, Clone)]
pub struct ETagQueryResult {
    pub event_id: String,
    pub kind: u32,
    pub pubkey: String,
    pub created_at: u64,
    pub content_preview: String,
}

/// Query nostrdb for events that have an e-tag referencing the given event ID
pub fn query_events_by_e_tag(ndb: &nostrdb::Ndb, target_event_id: &str) -> Vec<ETagQueryResult> {
    use nostrdb::{FilterBuilder, Transaction};

    let mut results = Vec::new();

    // Decode hex event ID to bytes
    let id_bytes: [u8; 32] = match hex::decode(target_event_id)
        .ok()
        .and_then(|bytes| bytes.try_into().ok())
    {
        Some(bytes) => bytes,
        None => return results, // Invalid hex ID
    };

    let txn = match Transaction::new(ndb) {
        Ok(t) => t,
        Err(_) => return results,
    };

    // Query with e-tag filter
    let filter = FilterBuilder::new().event(&id_bytes).build();

    if let Ok(query_results) = ndb.query(&txn, &[filter], 1000) {
        for result in query_results {
            if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
                let content = note.content();
                let content_preview = if content.len() > 100 {
                    format!("{}...", &content[..100])
                } else {
                    content.to_string()
                };

                results.push(ETagQueryResult {
                    event_id: hex::encode(note.id()),
                    kind: note.kind(),
                    pubkey: hex::encode(note.pubkey()),
                    created_at: note.created_at(),
                    content_preview,
                });
            }
        }
    }

    // Sort by created_at descending (newest first)
    results.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    results
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

/// Information about a single subscription
#[derive(Debug, Clone)]
pub struct SubscriptionInfo {
    /// Human-readable description of what this subscription is for
    pub description: String,
    /// Event kinds this subscription listens for
    pub kinds: Vec<u16>,
    /// Optional project a-tag filter (for project-specific subscriptions)
    pub project_a_tag: Option<String>,
    /// Number of events received on this subscription
    pub events_received: u64,
    /// Timestamp when subscription was created
    pub created_at: std::time::Instant,
}

impl SubscriptionInfo {
    pub fn new(description: String, kinds: Vec<u16>, project_a_tag: Option<String>) -> Self {
        Self {
            description,
            kinds,
            project_a_tag,
            events_received: 0,
            created_at: std::time::Instant::now(),
        }
    }

    pub fn record_event(&mut self) {
        self.events_received += 1;
    }
}

/// Stats for tracking active subscriptions
#[derive(Debug, Default, Clone)]
pub struct SubscriptionStats {
    /// All active subscriptions, keyed by subscription ID
    pub subscriptions: HashMap<String, SubscriptionInfo>,
}

impl SubscriptionStats {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a new subscription
    pub fn register(&mut self, sub_id: String, info: SubscriptionInfo) {
        self.subscriptions.insert(sub_id, info);
    }

    /// Record an event received for a subscription
    pub fn record_event(&mut self, sub_id: &str) {
        if let Some(info) = self.subscriptions.get_mut(sub_id) {
            info.record_event();
        }
    }

    /// Remove a subscription (when closed)
    pub fn remove(&mut self, sub_id: &str) {
        self.subscriptions.remove(sub_id);
    }

    /// Get all subscriptions sorted by events received (descending)
    pub fn by_events_received(&self) -> Vec<(&String, &SubscriptionInfo)> {
        let mut subs: Vec<_> = self.subscriptions.iter().collect();
        subs.sort_by(|a, b| b.1.events_received.cmp(&a.1.events_received));
        subs
    }

    /// Get total number of subscriptions
    pub fn count(&self) -> usize {
        self.subscriptions.len()
    }

    /// Get total events across all subscriptions
    pub fn total_events(&self) -> u64 {
        self.subscriptions.values().map(|s| s.events_received).sum()
    }
}

/// Thread-safe wrapper for subscription stats
#[derive(Debug, Clone)]
pub struct SharedSubscriptionStats {
    inner: Arc<RwLock<SubscriptionStats>>,
}

impl Default for SharedSubscriptionStats {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedSubscriptionStats {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(SubscriptionStats::new())),
        }
    }

    pub fn register(&self, sub_id: String, info: SubscriptionInfo) {
        if let Ok(mut stats) = self.inner.write() {
            stats.register(sub_id, info);
        }
    }

    pub fn record_event(&self, sub_id: &str) {
        if let Ok(mut stats) = self.inner.write() {
            stats.record_event(sub_id);
        }
    }

    pub fn remove(&self, sub_id: &str) {
        if let Ok(mut stats) = self.inner.write() {
            stats.remove(sub_id);
        }
    }

    pub fn snapshot(&self) -> SubscriptionStats {
        self.inner.read().map(|s| s.clone()).unwrap_or_default()
    }
}
