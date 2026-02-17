use std::collections::HashMap;
use std::sync::Arc;
use std::time::Instant;

use parking_lot::RwLock;

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
        self.inner.write().record(kind, project_a_tag);
    }

    pub fn snapshot(&self) -> EventStats {
        self.inner.read().clone()
    }
}

/// A single event in the live feed
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
                return tag
                    .get(1)
                    .and_then(|t| t.variant().str())
                    .map(|s| s.to_string());
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
    /// Raw filter JSON for debugging
    pub raw_filter: Option<String>,
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
            raw_filter: None,
            events_received: 0,
            created_at: std::time::Instant::now(),
        }
    }

    pub fn with_raw_filter(mut self, filter_json: String) -> Self {
        self.raw_filter = Some(filter_json);
        self
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
        self.inner.write().register(sub_id, info);
    }

    pub fn record_event(&self, sub_id: &str) {
        self.inner.write().record_event(sub_id);
    }

    pub fn remove(&self, sub_id: &str) {
        self.inner.write().remove(sub_id);
    }

    pub fn snapshot(&self) -> SubscriptionStats {
        self.inner.read().clone()
    }
}

/// Status of a negentropy sync operation
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NegentropySyncStatus {
    /// Sync completed successfully
    Ok,
    /// Relay doesn't support negentropy
    Unsupported,
    /// Sync failed with an error
    Failed,
}

/// Result of a single negentropy sync operation
#[derive(Debug, Clone)]
pub struct NegentropySyncResult {
    /// Event kind label (e.g., "31933", "4199")
    pub kind_label: String,
    /// Number of new events received
    pub events_received: u64,
    /// Status of the sync operation
    pub status: NegentropySyncStatus,
    /// Error message if failed (only populated for Failed status)
    pub error: Option<String>,
    /// Timestamp when this sync completed
    pub completed_at: Instant,
}

/// Statistics for negentropy synchronization
#[derive(Debug, Clone)]
pub struct NegentropySyncStats {
    /// Last time a full sync cycle completed (all filters synced)
    pub last_cycle_completed_at: Option<Instant>,
    /// Last time any individual filter sync completed
    pub last_filter_sync_at: Option<Instant>,
    /// Number of successful syncs (individual filter syncs)
    pub successful_syncs: u64,
    /// Number of failed/unsupported syncs
    pub failed_syncs: u64,
    /// Number of syncs where relay didn't support negentropy (subset of failed)
    pub unsupported_syncs: u64,
    /// Total events reconciled across all syncs
    pub total_events_reconciled: u64,
    /// Current sync interval in seconds
    pub current_interval_secs: u64,
    /// Whether negentropy sync is currently enabled
    pub enabled: bool,
    /// Whether a sync is currently in progress
    pub sync_in_progress: bool,
    /// Recent sync results (last N syncs per kind)
    pub recent_results: Vec<NegentropySyncResult>,
    /// Maximum recent results to keep
    max_recent_results: usize,
}

impl Default for NegentropySyncStats {
    fn default() -> Self {
        Self::new()
    }
}

impl NegentropySyncStats {
    pub fn new() -> Self {
        Self {
            last_cycle_completed_at: None,
            last_filter_sync_at: None,
            successful_syncs: 0,
            failed_syncs: 0,
            unsupported_syncs: 0,
            total_events_reconciled: 0,
            current_interval_secs: 60,
            enabled: false,
            sync_in_progress: false,
            recent_results: Vec::new(),
            max_recent_results: 20,
        }
    }

    /// Record a successful sync
    pub fn record_success(&mut self, kind_label: &str, events_received: u64) {
        self.successful_syncs += 1;
        self.total_events_reconciled += events_received;
        self.last_filter_sync_at = Some(Instant::now());

        self.recent_results.push(NegentropySyncResult {
            kind_label: kind_label.to_string(),
            events_received,
            status: NegentropySyncStatus::Ok,
            error: None,
            completed_at: Instant::now(),
        });

        self.trim_recent_results();
    }

    /// Record a failed sync
    pub fn record_failure(&mut self, kind_label: &str, error: &str, is_unsupported: bool) {
        let status = if is_unsupported {
            // Unsupported relays are tracked separately, not as failures
            self.unsupported_syncs += 1;
            NegentropySyncStatus::Unsupported
        } else {
            self.failed_syncs += 1;
            NegentropySyncStatus::Failed
        };
        self.last_filter_sync_at = Some(Instant::now());

        self.recent_results.push(NegentropySyncResult {
            kind_label: kind_label.to_string(),
            events_received: 0,
            status,
            error: if is_unsupported {
                None
            } else {
                Some(error.to_string())
            },
            completed_at: Instant::now(),
        });

        self.trim_recent_results();
    }

    /// Record that a full sync cycle has completed
    pub fn record_cycle_complete(&mut self) {
        self.last_cycle_completed_at = Some(Instant::now());
    }

    /// Update the current sync interval
    pub fn set_interval(&mut self, secs: u64) {
        self.current_interval_secs = secs;
    }

    /// Set whether sync is enabled
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Set whether sync is in progress
    pub fn set_in_progress(&mut self, in_progress: bool) {
        self.sync_in_progress = in_progress;
    }

    fn trim_recent_results(&mut self) {
        while self.recent_results.len() > self.max_recent_results {
            self.recent_results.remove(0);
        }
    }

    /// Get the instant when the last full sync cycle completed
    pub fn last_cycle_time(&self) -> Option<Instant> {
        self.last_cycle_completed_at
    }

    /// Get the instant when any filter was last synced
    pub fn last_filter_time(&self) -> Option<Instant> {
        self.last_filter_sync_at
    }
}

/// Thread-safe wrapper for negentropy sync stats
#[derive(Debug, Clone)]
pub struct SharedNegentropySyncStats {
    inner: Arc<RwLock<NegentropySyncStats>>,
}

impl Default for SharedNegentropySyncStats {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedNegentropySyncStats {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(RwLock::new(NegentropySyncStats::new())),
        }
    }

    pub fn record_success(&self, kind_label: &str, events_received: u64) {
        self.inner
            .write()
            .record_success(kind_label, events_received);
    }

    pub fn record_failure(&self, kind_label: &str, error: &str, is_unsupported: bool) {
        self.inner
            .write()
            .record_failure(kind_label, error, is_unsupported);
    }

    pub fn record_cycle_complete(&self) {
        self.inner.write().record_cycle_complete();
    }

    pub fn set_interval(&self, secs: u64) {
        self.inner.write().set_interval(secs);
    }

    pub fn set_enabled(&self, enabled: bool) {
        self.inner.write().set_enabled(enabled);
    }

    pub fn set_in_progress(&self, in_progress: bool) {
        self.inner.write().set_in_progress(in_progress);
    }

    pub fn snapshot(&self) -> NegentropySyncStats {
        self.inner.read().clone()
    }
}

/// Debug info for threads in a project
#[derive(Debug, Clone, Default)]
pub struct ProjectThreadDebugInfo {
    /// Project a-tag
    pub a_tag: String,
    /// Human-readable project name
    pub name: String,
    /// Total kind:1 events with this a-tag in the database
    pub raw_db_kind1_count: usize,
    /// How many of those have e-tags (messages, not threads)
    pub messages_count: usize,
    /// How many pass Thread::from_note (no e-tags, valid threads)
    pub threads_count: usize,
}

/// Query nostrdb directly for detailed thread statistics per project
/// This bypasses the AppDataStore and queries raw DB
pub fn query_project_thread_stats(
    ndb: &nostrdb::Ndb,
    project_a_tags: &[String],
) -> Vec<ProjectThreadDebugInfo> {
    use crate::models::Thread;
    use nostrdb::{Filter, Transaction};

    let mut results = Vec::new();

    let txn = match Transaction::new(ndb) {
        Ok(t) => t,
        Err(_) => return results,
    };

    for a_tag in project_a_tags {
        let project_name = a_tag.split(':').nth(2).unwrap_or(a_tag).to_string();

        // Query all kind:1 with this a-tag
        let filter = Filter::new().kinds([1]).tags([a_tag.as_str()], 'a').build();

        let mut info = ProjectThreadDebugInfo {
            a_tag: a_tag.clone(),
            name: project_name,
            ..Default::default()
        };

        // Use a very high limit to get everything
        if let Ok(query_results) = ndb.query(&txn, &[filter], 1_000_000) {
            info.raw_db_kind1_count = query_results.len();

            for r in query_results.iter() {
                if let Ok(note) = ndb.get_note_by_key(&txn, r.note_key) {
                    // Check if this note has e-tags
                    let has_e_tag = note
                        .tags()
                        .iter()
                        .any(|tag| tag.get(0).and_then(|t| t.variant().str()) == Some("e"));

                    if has_e_tag {
                        info.messages_count += 1;
                    } else if Thread::from_note(&note).is_some() {
                        info.threads_count += 1;
                    }
                }
            }
        }

        results.push(info);
    }

    results
}
