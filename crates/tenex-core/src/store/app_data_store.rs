use crate::events::PendingBackendApproval;
use crate::models::{
    AgentChatter, AskEvent, BookmarkList, ConversationMetadata, InboxEventType, InboxItem, Message,
    Project, ProjectAgent, ProjectStatus, Thread,
};
#[cfg(test)]
use crate::models::{AgentDefinition, Lesson, MCPTool, Nudge, OperationsStatus, Report};
use crate::store::content_store::ContentStore;
use crate::store::inbox_store::InboxStore;
use crate::store::operations_store::OperationsStore;
use crate::store::reports_store::ReportsStore;
use crate::store::state_cache;
use crate::store::statistics_store::{MessagesByDayCounts, StatisticsStore};
use crate::store::trust_store::TrustStore;
use crate::store::{RuntimeHierarchy, RUNTIME_CUTOFF_TIMESTAMP};
use nostrdb::{Filter, Ndb, Note, Transaction};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;
use tracing::{trace, warn};

/// Reactive data store - single source of truth for app-level concepts.
/// Rebuilt from nostrdb on startup, updated incrementally on new events.
pub struct AppDataStore {
    ndb: Arc<Ndb>,

    // Sub-stores
    pub content: ContentStore,
    pub reports: ReportsStore,
    pub trust: TrustStore,

    // Core app data
    pub projects: Vec<Project>,
    // Latest tombstone timestamp per project a_tag. Used to prevent stale live
    // events from reviving deleted projects when events arrive out of order.
    project_tombstones: HashMap<String, u64>,
    pub project_statuses: HashMap<String, ProjectStatus>, // keyed by project a_tag
    pub threads_by_project: HashMap<String, Vec<Thread>>, // keyed by project a_tag
    pub messages_by_thread: HashMap<String, Vec<Message>>, // keyed by thread_id
    pub profiles: HashMap<String, String>,                // pubkey -> display name

    // Inbox - events that p-tag the current user
    pub inbox: InboxStore,
    pub user_pubkey: Option<String>,

    // Agent chatter feed - kind:1 events a-tagging our projects
    pub agent_chatter: Vec<AgentChatter>,

    // Operations status and agent tracking
    pub operations: OperationsStore,
    pub statistics: StatisticsStore,

    // Pending subscriptions for new projects (drained by CoreRuntime)
    pending_project_subscriptions: Vec<String>,

    // Thread root index - maps project a_tag -> set of known thread root event IDs
    thread_root_index: HashMap<String, HashSet<String>>,

    // Runtime hierarchy - tracks individual conversation runtimes and parent-child relationships
    pub runtime_hierarchy: RuntimeHierarchy,

    // Set on logout clear(); consumed by login() to avoid duplicate rebuild work.
    needs_rebuild: bool,

    /// Bookmark lists keyed by user pubkey (kind:14202 replaceable events)
    pub bookmarks: HashMap<String, BookmarkList>,

    /// Directory for the persistent state cache file.
    /// Set to `Some(path)` when the store is created with `new_with_cache()`.
    /// `None` disables caching (used by FFI/iOS callers and tests).
    cache_dir: Option<PathBuf>,
}

impl AppDataStore {
    /// Create a new store and unconditionally rebuild from nostrdb.
    ///
    /// This is the existing constructor, preserved for backwards compatibility with
    /// FFI (iOS) callers and tests. Caching is disabled — every startup does a full rebuild.
    /// Use `new_with_cache` in the TUI runtime to get the fast-startup path.
    pub fn new(ndb: Arc<Ndb>) -> Self {
        let mut store = Self {
            ndb,
            content: ContentStore::new(),
            reports: ReportsStore::new(),
            trust: TrustStore::new(),
            projects: Vec::new(),
            project_tombstones: HashMap::new(),
            project_statuses: HashMap::new(),
            threads_by_project: HashMap::new(),
            messages_by_thread: HashMap::new(),
            profiles: HashMap::new(),
            inbox: InboxStore::new(),
            user_pubkey: None,
            agent_chatter: Vec::new(),
            operations: OperationsStore::new(),
            statistics: StatisticsStore::new(),
            pending_project_subscriptions: Vec::new(),
            thread_root_index: HashMap::new(),
            runtime_hierarchy: RuntimeHierarchy::new(),
            needs_rebuild: false,
            bookmarks: HashMap::new(),
            cache_dir: None,
        };
        store.rebuild_from_ndb();
        store
    }

    /// Create a new store, trying to load from disk cache first.
    ///
    /// On a cache **hit**: loads the cached state, then queries nostrdb for any events
    /// newer than the cache's `saved_at` timestamp (incremental catch-up).
    /// On a cache **miss** (missing file, corrupt, stale, schema mismatch): falls back
    /// to a full `rebuild_from_ndb()` — same as `new()`.
    ///
    /// The `cache_dir` path is retained so `save_cache()` can be called on shutdown.
    pub fn new_with_cache(ndb: Arc<Ndb>, cache_dir: PathBuf) -> Self {
        let mut store = Self {
            ndb,
            content: ContentStore::new(),
            reports: ReportsStore::new(),
            trust: TrustStore::new(),
            projects: Vec::new(),
            project_tombstones: HashMap::new(),
            project_statuses: HashMap::new(),
            threads_by_project: HashMap::new(),
            messages_by_thread: HashMap::new(),
            profiles: HashMap::new(),
            inbox: InboxStore::new(),
            user_pubkey: None,
            agent_chatter: Vec::new(),
            operations: OperationsStore::new(),
            statistics: StatisticsStore::new(),
            pending_project_subscriptions: Vec::new(),
            thread_root_index: HashMap::new(),
            runtime_hierarchy: RuntimeHierarchy::new(),
            needs_rebuild: false,
            bookmarks: HashMap::new(),
            cache_dir: Some(cache_dir),
        };
        if !store.try_load_from_cache() {
            store.rebuild_from_ndb();
        }
        store
    }

    pub fn needs_rebuild_for_login(&self) -> bool {
        self.needs_rebuild
    }

    /// Apply authenticated user context during login.
    ///
    /// If the store was cleared during logout, this performs a full rebuild before
    /// attaching user-specific state (inbox + user statistics).
    pub fn apply_authenticated_user(&mut self, pubkey: String) {
        let started_at = Instant::now();
        let did_rebuild = self.needs_rebuild;
        if self.needs_rebuild {
            self.rebuild_from_ndb();
        }
        self.set_user_pubkey(pubkey);
        crate::tlog!(
            "PERF",
            "AppDataStore::apply_authenticated_user rebuild={} elapsedMs={}",
            did_rebuild,
            started_at.elapsed().as_millis()
        );
    }

    pub fn set_user_pubkey(&mut self, pubkey: String) {
        let started_at = Instant::now();
        let pubkey_changed = self.user_pubkey.as_ref() != Some(&pubkey);
        self.user_pubkey = Some(pubkey.clone());
        // Load any cached bookmark list from nostrdb
        self.load_bookmarks();
        // Populate inbox from existing messages
        self.populate_inbox_from_existing(&pubkey);
        // Rebuild message counts if user changed (ensures historical counts are accurate)
        if pubkey_changed {
            self.statistics.rebuild_messages_by_day_counts_from_loaded(
                &self.user_pubkey,
                &self.messages_by_thread,
            );
        }
        // Rebuild LLM activity hourly aggregates (always, not user-dependent)
        self.statistics
            .rebuild_llm_activity_by_hour(&self.messages_by_thread);
        // Rebuild runtime-by-day aggregates (always, not user-dependent)
        self.statistics
            .rebuild_runtime_by_day_counts(&self.messages_by_thread);
        crate::tlog!(
            "PERF",
            "AppDataStore::set_user_pubkey pubkeyChanged={} inboxItems={} elapsedMs={}",
            pubkey_changed,
            self.inbox.get_items().len(),
            started_at.elapsed().as_millis()
        );
    }

    /// Clear all in-memory data (used on logout to prevent stale data leaks).
    /// Does NOT clear nostrdb - that persists across sessions.
    /// After logout and re-login with different account, rebuild_from_ndb()
    /// will repopulate with the new user's filtered view.
    pub fn clear(&mut self) {
        self.content.clear();
        self.reports.clear();
        self.trust.clear();
        self.projects.clear();
        self.project_tombstones.clear();
        self.project_statuses.clear();
        self.threads_by_project.clear();
        self.messages_by_thread.clear();
        self.profiles.clear();
        self.inbox.clear();
        self.user_pubkey = None;
        self.agent_chatter.clear();
        self.operations.clear();
        self.statistics.clear();
        self.pending_project_subscriptions.clear();
        self.thread_root_index.clear();
        self.runtime_hierarchy = RuntimeHierarchy::new();
        self.needs_rebuild = true;
        self.bookmarks.clear();
    }

    /// Scan existing messages and populate inbox with those that p-tag the user
    fn populate_inbox_from_existing(&mut self, user_pubkey: &str) {
        let started_at = Instant::now();
        let txn = Transaction::new(&self.ndb).ok();

        // First, build a set of ask event IDs that the user has already replied to
        // by checking e-tags on user's messages (not just reply_to field, but all e-tags)
        let mut answered_ask_ids: std::collections::HashSet<String> =
            std::collections::HashSet::new();
        for messages in self.messages_by_thread.values() {
            for message in messages {
                if message.pubkey == user_pubkey {
                    // Fast path: direct parent reply is already parsed on Message.
                    if let Some(reply_to_id) = &message.reply_to {
                        answered_ask_ids.insert(reply_to_id.clone());
                        continue;
                    }

                    // Fallback for legacy/non-marked replies: inspect all e-tags from note.
                    // This preserves previous behavior without requiring note fetches for
                    // every candidate inbox message.
                    if let Some(txn) = txn.as_ref() {
                        let note_id_bytes = match hex::decode(&message.id) {
                            Ok(bytes) if bytes.len() == 32 => bytes,
                            _ => continue,
                        };
                        let note_id: [u8; 32] = match note_id_bytes.try_into() {
                            Ok(arr) => arr,
                            Err(_) => continue,
                        };
                        if let Ok(note) = self.ndb.get_note_by_id(txn, &note_id) {
                            let reply_to_ids = Self::extract_e_tag_ids(&note);
                            for reply_to_id in reply_to_ids {
                                answered_ask_ids.insert(reply_to_id);
                            }
                        }
                    }
                }
            }
        }

        // Build thread->project lookup once to avoid repeated linear scans.
        let mut thread_to_project: HashMap<String, String> = HashMap::new();
        for (project_a_tag, threads) in &self.threads_by_project {
            for thread in threads {
                thread_to_project.insert(thread.id.clone(), project_a_tag.clone());
            }
        }

        // Collect all messages that need checking.
        let mut inbox_candidates: Vec<InboxItem> = Vec::new();
        let mut messages_scanned = 0usize;
        for (thread_id, messages) in &self.messages_by_thread {
            for message in messages {
                messages_scanned += 1;
                // Skip our own messages
                if message.pubkey == user_pubkey {
                    continue;
                }
                // Skip already-read items
                if self.inbox.is_read(&message.id) {
                    continue;
                }

                // Fast path: p-tags are already parsed and normalized in Message.
                if !message.p_tags.iter().any(|p| p == user_pubkey) {
                    continue;
                }

                // Ask events are parsed from message tags when available.
                let is_ask = message.ask_event.is_some();

                // Skip ask events that user has already answered
                if is_ask && answered_ask_ids.contains(&message.id) {
                    continue;
                }

                let event_type = if is_ask {
                    InboxEventType::Ask
                } else {
                    InboxEventType::Mention
                };

                let project_a_tag = thread_to_project
                    .get(thread_id)
                    .cloned()
                    .unwrap_or_default();

                inbox_candidates.push(InboxItem {
                    id: message.id.clone(),
                    event_type,
                    title: message.content.chars().take(50).collect(),
                    content: message.content.clone(),
                    project_a_tag,
                    author_pubkey: message.pubkey.clone(),
                    created_at: message.created_at,
                    is_read: false,
                    thread_id: Some(thread_id.clone()),
                    ask_event: message.ask_event.clone(),
                });
            }
        }

        for candidate in inbox_candidates {
            if !self.inbox.contains(&candidate.id) {
                self.inbox.push_raw(candidate);
            }
        }

        // Sort inbox by created_at descending (most recent first)
        self.inbox.sort();
        crate::tlog!(
            "PERF",
            "AppDataStore::populate_inbox_from_existing scannedMessages={} inboxSize={} elapsedMs={}",
            messages_scanned,
            self.inbox.get_items().len(),
            started_at.elapsed().as_millis()
        );
    }

    /// Rebuild all data from nostrdb (called on startup)
    pub fn rebuild_from_ndb(&mut self) {
        let rebuild_started_at = Instant::now();
        crate::tlog!("PERF", "AppDataStore::rebuild_from_ndb start");
        let load_projects_started_at = Instant::now();
        if let Ok(projects) = crate::store::get_projects(&self.ndb) {
            self.projects = projects;
        }
        self.project_tombstones =
            crate::store::views::get_project_tombstones(&self.ndb).unwrap_or_default();
        let load_projects_elapsed_ms = load_projects_started_at.elapsed().as_millis();
        let project_count = self.projects.len();

        // NOTE: Project statuses are loaded in set_trusted_backends() after login
        // This ensures trust validation is applied

        let a_tags: Vec<String> = self.projects.iter().map(|p| p.a_tag()).collect();

        // Step 1: Build thread root index for all projects
        // This scans kind:1 events once and identifies thread roots (no e-tags)
        let build_thread_index_started_at = Instant::now();
        if let Ok(index) = crate::store::build_thread_root_index(&self.ndb, &a_tags) {
            self.thread_root_index = index;
        }
        let build_thread_index_elapsed_ms = build_thread_index_started_at.elapsed().as_millis();

        // Step 2: Load full thread data using the index (query by known IDs)
        let load_threads_started_at = Instant::now();
        let mut loaded_thread_count = 0usize;
        for a_tag in &a_tags {
            if let Some(root_ids) = self.thread_root_index.get(a_tag) {
                if let Ok(threads) = crate::store::get_threads_by_ids(&self.ndb, root_ids) {
                    loaded_thread_count += threads.len();
                    self.threads_by_project.insert(a_tag.clone(), threads);
                }
            }
        }
        let load_threads_elapsed_ms = load_threads_started_at.elapsed().as_millis();

        // Pre-load messages for all threads
        let load_messages_started_at = Instant::now();
        let mut loaded_message_count = 0usize;
        for threads in self.threads_by_project.values() {
            for thread in threads {
                if let Ok(messages) = crate::store::get_messages_for_thread(&self.ndb, &thread.id) {
                    loaded_message_count += messages.len();
                    self.messages_by_thread.insert(thread.id.clone(), messages);
                }
            }
        }
        let load_messages_elapsed_ms = load_messages_started_at.elapsed().as_millis();

        // Update thread last_activity based on most recent message
        // (effective_last_activity will be updated after metadata is applied and hierarchy is built)
        for threads in self.threads_by_project.values_mut() {
            for thread in threads.iter_mut() {
                if let Some(messages) = self.messages_by_thread.get(&thread.id) {
                    if let Some(last_msg) = messages.last() {
                        if last_msg.created_at > thread.last_activity {
                            thread.last_activity = last_msg.created_at;
                        }
                    }
                }
            }
            // Temporary sort by last_activity - will be re-sorted by effective_last_activity after hierarchy is built
            threads.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        }

        // Rebuild pre-aggregated statistics
        let rebuild_stats_started_at = Instant::now();
        self.statistics.rebuild_messages_by_day_counts_from_loaded(
            &self.user_pubkey,
            &self.messages_by_thread,
        );
        self.statistics
            .rebuild_llm_activity_by_hour(&self.messages_by_thread);
        self.statistics
            .rebuild_runtime_by_day_counts(&self.messages_by_thread);
        let rebuild_stats_elapsed_ms = rebuild_stats_started_at.elapsed().as_millis();

        // Apply metadata (kind:513) to threads - may further update last_activity
        let apply_metadata_started_at = Instant::now();
        self.apply_existing_metadata();
        let apply_metadata_elapsed_ms = apply_metadata_started_at.elapsed().as_millis();

        // Build runtime hierarchy from loaded data
        // This sets up parent-child relationships and calculates effective_last_activity
        let rebuild_hierarchy_started_at = Instant::now();
        self.rebuild_runtime_hierarchy();
        let rebuild_hierarchy_elapsed_ms = rebuild_hierarchy_started_at.elapsed().as_millis();

        // Final sort by effective_last_activity after hierarchy is fully built
        for threads in self.threads_by_project.values_mut() {
            threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
        }

        // Rebuild report discussion thread index from loaded thread roots.
        // This ensures `get_document_threads(report_a_tag)` works after cold start/rebuild,
        // even before incremental callback updates arrive.
        self.reports.document_threads.clear();
        let mut report_thread_links: Vec<(String, Thread)> = Vec::new();
        for threads in self.threads_by_project.values() {
            for thread in threads {
                if let Some(messages) = self.messages_by_thread.get(&thread.id) {
                    if let Some(root_message) = messages.first() {
                        for report_a_tag in &root_message.a_tags {
                            report_thread_links.push((report_a_tag.clone(), thread.clone()));
                        }
                    }
                }
            }
        }
        for (report_a_tag, thread) in report_thread_links {
            self.reports.add_document_thread(&report_a_tag, thread);
        }

        // Load content definitions (kind:34199, 4199, 4200, 4201, 4202)
        let load_content_started_at = Instant::now();
        self.content.load_team_packs(&self.ndb);
        self.content.load_agent_definitions(&self.ndb);
        self.content.load_mcp_tools(&self.ndb);
        self.content.load_nudges(&self.ndb);
        self.content.load_skills(&self.ndb);
        let load_content_elapsed_ms = load_content_started_at.elapsed().as_millis();

        // NOTE: Ephemeral events (kind:24010, 24133) are intentionally NOT loaded from nostrdb.
        // They are only received via live subscriptions and stored in memory.
        // Operations status (kind:24133) will be populated when events arrive.

        // Load reports (kind:30023)
        let load_reports_started_at = Instant::now();
        let project_a_tags: Vec<String> = self.projects.iter().map(|p| p.a_tag()).collect();
        self.reports.load_reports(&self.ndb, &project_a_tags);
        let load_reports_elapsed_ms = load_reports_started_at.elapsed().as_millis();
        self.needs_rebuild = false;
        crate::tlog!(
            "PERF",
            "AppDataStore::rebuild_from_ndb complete projects={} threads={} messages={} loadProjectsMs={} buildIndexMs={} loadThreadsMs={} loadMessagesMs={} statsMs={} metadataMs={} hierarchyMs={} contentMs={} reportsMs={} totalMs={}",
            project_count,
            loaded_thread_count,
            loaded_message_count,
            load_projects_elapsed_ms,
            build_thread_index_elapsed_ms,
            load_threads_elapsed_ms,
            load_messages_elapsed_ms,
            rebuild_stats_elapsed_ms,
            apply_metadata_elapsed_ms,
            rebuild_hierarchy_elapsed_ms,
            load_content_elapsed_ms,
            load_reports_elapsed_ms,
            rebuild_started_at.elapsed().as_millis()
        );
    }

    // NOTE: load_operations_status() was removed because ephemeral events (kind:24133)
    // should NOT be read from nostrdb. Operations status is only received via live
    // subscriptions and stored in memory (operations_by_event).

    // ===== State Cache Methods =====

    /// Attempt to populate the store from the on-disk cache.
    ///
    /// Returns `true` on a cache hit (store is now populated), `false` on any miss/error
    /// (caller should fall back to `rebuild_from_ndb`).
    fn try_load_from_cache(&mut self) -> bool {
        let cache_dir = match self.cache_dir.clone() {
            Some(d) => d,
            None => return false,
        };

        let started_at = Instant::now();
        crate::tlog!("PERF", "AppDataStore::try_load_from_cache attempt");

        let (cached_state, saved_at) = match state_cache::load_cache(&cache_dir) {
            Some(result) => result,
            None => {
                crate::tlog!("PERF", "AppDataStore::try_load_from_cache miss");
                return false;
            }
        };

        let restore_started_at = Instant::now();
        self.restore_from_cached_state(cached_state);
        let restore_elapsed_ms = restore_started_at.elapsed().as_millis();

        let catchup_started_at = Instant::now();
        let catchup_ok = self.do_incremental_catchup(saved_at);
        let catchup_elapsed_ms = catchup_started_at.elapsed().as_millis();

        if !catchup_ok {
            // Incremental catch-up failed (nostrdb transaction/query error).
            // Rather than serving a potentially stale cache, signal the caller to
            // do a full rebuild.
            warn!("AppDataStore::try_load_from_cache: incremental catch-up failed — falling back to full rebuild");
            self.needs_rebuild = true;
            return false;
        }

        self.needs_rebuild = false;

        crate::tlog!(
            "PERF",
            "AppDataStore::try_load_from_cache hit projects={} threads={} messages={} restoreMs={} catchupMs={} totalMs={}",
            self.projects.len(),
            self.threads_by_project.values().map(|v| v.len()).sum::<usize>(),
            self.messages_by_thread.values().map(|v| v.len()).sum::<usize>(),
            restore_elapsed_ms,
            catchup_elapsed_ms,
            started_at.elapsed().as_millis()
        );

        true
    }

    /// Populate the store's fields from a `CachedState` snapshot.
    ///
    /// After calling this, derived data (statistics, runtime hierarchy) is rebuilt
    /// from the restored data — this is fast and does not touch nostrdb.
    fn restore_from_cached_state(&mut self, state: state_cache::CachedState) {
        self.projects = state.projects;
        self.threads_by_project = state.threads_by_project;
        self.messages_by_thread = state.messages_by_thread;
        self.profiles = state.profiles;
        self.thread_root_index = state.thread_root_index;

        self.content.agent_definitions = state.agent_definitions;
        self.content.team_packs = state.team_packs;
        self.content.mcp_tools = state.mcp_tools;
        self.content.nudges = state.nudges;
        self.content.skills = state.skills;
        self.content.lessons = state.lessons;

        self.reports.reports = state.reports;
        self.reports.reports_all_versions = state.reports_all_versions;
        self.reports.document_threads = state.document_threads;

        self.trust.approved_backends = state.approved_backends;
        self.trust.blocked_backends = state.blocked_backends;

        // Rebuild derived data from the restored snapshot.  These operate only on
        // already-loaded data and do not touch nostrdb, so they are fast.
        self.statistics.rebuild_messages_by_day_counts_from_loaded(
            &self.user_pubkey,
            &self.messages_by_thread,
        );
        self.statistics
            .rebuild_llm_activity_by_hour(&self.messages_by_thread);
        self.statistics
            .rebuild_runtime_by_day_counts(&self.messages_by_thread);
        self.rebuild_runtime_hierarchy();

        // Sort threads by effective_last_activity (descending).
        for threads in self.threads_by_project.values_mut() {
            threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
        }
    }

    /// Query nostrdb for events newer than `since_timestamp` and apply them via
    /// `handle_event` so the in-memory state is fully up to date after a cache load.
    ///
    /// This is the "incremental catch-up" step: it processes only the delta between
    /// the last cache save and the current nostrdb state, which is typically very fast.
    ///
    /// Returns `true` on success, `false` if the transaction or query failed.
    /// On failure the caller should fall back to a full `rebuild_from_ndb()`.
    fn do_incremental_catchup(&mut self, since_timestamp: u64) -> bool {
        // Clock-skew safety: subtract 5 minutes so that events whose `created_at`
        // is slightly before the cache's `max_created_at` (due to relay clock drift or
        // out-of-order delivery) are still picked up.  `handle_event` deduplicates
        // anything already present in state, so this is safe to over-fetch.
        const CLOCK_SKEW_SECS: u64 = 5 * 60;
        let since = since_timestamp.saturating_sub(CLOCK_SKEW_SECS);

        // Clone the Arc so we can hold an immutable borrow on `ndb` (for the
        // transaction + note lifetimes) while also mutably borrowing `self` for
        // `handle_event`.  The underlying Ndb data is shared; no copy is made.
        let ndb = Arc::clone(&self.ndb);

        let txn = match Transaction::new(&ndb) {
            Ok(t) => t,
            Err(e) => {
                warn!("AppDataStore::do_incremental_catchup: failed to open transaction: {e}");
                return false;
            }
        };

        // Query for all event kinds we care about, restricted to events newer than
        // the cache's max_created_at (minus clock-skew window).
        let filter = Filter::new()
            .kinds([
                31933, 1, 0, 4199, 34199, 4200, 4201, 4202, 4129, 513, 30023, 14202,
            ])
            .since(since)
            .build();

        let results = match ndb.query(&txn, &[filter], 500_000) {
            Ok(r) => r,
            Err(e) => {
                warn!("AppDataStore::do_incremental_catchup: query failed: {e}");
                return false;
            }
        };

        let new_event_count = results.len();
        for result in &results {
            if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
                self.handle_event(note.kind(), &note);
            }
        }

        if new_event_count > 0 {
            // Re-sort threads after incremental updates may have changed last_activity.
            for threads in self.threads_by_project.values_mut() {
                threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
            }
        }

        crate::tlog!(
            "PERF",
            "AppDataStore::do_incremental_catchup since={} newEvents={}",
            since,
            new_event_count
        );

        true
    }

    /// Snapshot the current in-memory state and write it to the cache file.
    ///
    /// Called from `CoreRuntime::shutdown()` so that the next startup can skip the
    /// expensive `rebuild_from_ndb()`.  Any errors are logged as warnings; this method
    /// never panics.
    pub fn save_cache(&self) {
        let cache_dir = match self.cache_dir.as_ref() {
            Some(d) => d,
            None => return, // caching disabled for this instance
        };

        // HIGH-2: Do not save an empty or invalid cache.  If `needs_rebuild` is true
        // (e.g. the user just logged out), the in-memory state has been cleared and
        // saving it would produce an empty cache that silently suppresses the full
        // rebuild on the next startup.  Also guard against a missing `user_pubkey`
        // for the same reason.
        if self.needs_rebuild || self.user_pubkey.is_none() {
            tracing::info!(
                "AppDataStore::save_cache skipped (needs_rebuild={} no_user={})",
                self.needs_rebuild,
                self.user_pubkey.is_none()
            );
            return;
        }

        let started_at = Instant::now();

        // HIGH-1: Compute the highest Nostr event `created_at` across all cached
        // events so the incremental catch-up filter on the next startup is based on
        // actual event timestamps rather than wall-clock save time.
        let max_created_at = self.compute_max_created_at();

        // MED-2: `CachedState` is moved (not cloned again) into `state_cache::save_cache`,
        // which takes ownership.  This means we pay for exactly one clone of the store
        // data here, not two.
        let cached_state = state_cache::CachedState {
            projects: self.projects.clone(),
            threads_by_project: self.threads_by_project.clone(),
            messages_by_thread: self.messages_by_thread.clone(),
            profiles: self.profiles.clone(),
            thread_root_index: self.thread_root_index.clone(),
            agent_definitions: self.content.agent_definitions.clone(),
            team_packs: self.content.team_packs.clone(),
            mcp_tools: self.content.mcp_tools.clone(),
            nudges: self.content.nudges.clone(),
            skills: self.content.skills.clone(),
            lessons: self.content.lessons.clone(),
            reports: self.reports.reports.clone(),
            reports_all_versions: self.reports.reports_all_versions.clone(),
            document_threads: self.reports.document_threads.clone(),
            approved_backends: self.trust.approved_backends.clone(),
            blocked_backends: self.trust.blocked_backends.clone(),
        };

        match state_cache::save_cache(cache_dir, cached_state, max_created_at) {
            Ok(()) => crate::tlog!(
                "PERF",
                "AppDataStore::save_cache ok projects={} threads={} messages={} maxCreatedAt={} elapsedMs={}",
                self.projects.len(),
                self.threads_by_project.values().map(|v| v.len()).sum::<usize>(),
                self.messages_by_thread.values().map(|v| v.len()).sum::<usize>(),
                max_created_at,
                started_at.elapsed().as_millis()
            ),
            Err(e) => warn!("AppDataStore::save_cache failed: {}", e),
        }
    }

    /// Compute the highest Nostr event `created_at` timestamp seen across all
    /// data currently held in this store.
    ///
    /// Used when saving the cache so the next startup's incremental catch-up
    /// filter uses the actual event-time high-water mark rather than wall-clock
    /// save time.  This ensures late-arriving or backfilled events with
    /// `created_at < saved_at` are never permanently missed.
    fn compute_max_created_at(&self) -> u64 {
        let mut max: u64 = 0;

        // Messages are the highest-volume events and carry the most recent timestamps.
        for messages in self.messages_by_thread.values() {
            for msg in messages {
                if msg.created_at > max {
                    max = msg.created_at;
                }
            }
        }

        // Thread root events (conversation starts).
        for threads in self.threads_by_project.values() {
            for thread in threads {
                if thread.last_activity > max {
                    max = thread.last_activity;
                }
            }
        }

        // Projects.
        for project in &self.projects {
            if project.created_at > max {
                max = project.created_at;
            }
        }

        // Content definitions.
        for ad in self.content.agent_definitions.values() {
            if ad.created_at > max {
                max = ad.created_at;
            }
        }
        for nudge in self.content.nudges.values() {
            if nudge.created_at > max {
                max = nudge.created_at;
            }
        }
        for skill in self.content.skills.values() {
            if skill.created_at > max {
                max = skill.created_at;
            }
        }
        for lesson in self.content.lessons.values() {
            if lesson.created_at > max {
                max = lesson.created_at;
            }
        }
        for mcp_tool in self.content.mcp_tools.values() {
            if mcp_tool.created_at > max {
                max = mcp_tool.created_at;
            }
        }

        // Reports.
        for report in self.reports.reports.values() {
            if report.created_at > max {
                max = report.created_at;
            }
        }

        max
    }

    // ===== Runtime Hierarchy Methods =====

    /// Rebuild the runtime hierarchy from loaded messages and threads
    /// Called during startup after messages_by_thread is populated
    fn rebuild_runtime_hierarchy(&mut self) {
        self.runtime_hierarchy.clear();

        // Step 1: Build parent-child relationships from threads (delegation tags)
        // Also track individual last_activity for each thread
        for threads in self.threads_by_project.values() {
            for thread in threads {
                if let Some(ref parent_id) = thread.parent_conversation_id {
                    self.runtime_hierarchy.set_parent(&thread.id, parent_id);
                }
                // Initialize individual last_activity from thread
                self.runtime_hierarchy
                    .set_individual_last_activity(&thread.id, thread.last_activity);
            }
        }

        // Step 2: Build parent-child relationships from q-tags in messages
        // Also determine conversation creation timestamps (earliest message)
        for (thread_id, messages) in &self.messages_by_thread {
            // Find the earliest message timestamp as the conversation creation time
            if let Some(earliest_created_at) = messages.iter().map(|m| m.created_at).min() {
                self.runtime_hierarchy
                    .set_conversation_created_at(thread_id, earliest_created_at);
            }

            for message in messages {
                // q-tags indicate children of this conversation
                for child_id in &message.q_tags {
                    self.runtime_hierarchy.add_child(thread_id, child_id);
                }
                // delegation tags indicate this is a child of another conversation
                if let Some(ref parent_id) = message.delegation_tag {
                    if parent_id != thread_id {
                        self.runtime_hierarchy.set_parent(thread_id, parent_id);
                    }
                }
            }
        }

        // Step 3: Calculate individual runtimes for each conversation
        for (thread_id, messages) in &self.messages_by_thread {
            let runtime_ms = Self::calculate_runtime_from_messages(messages);
            if runtime_ms > 0 {
                self.runtime_hierarchy
                    .set_individual_runtime(thread_id, runtime_ms);
            }
        }

        // Step 4: Update effective_last_activity on all threads
        // We need to do this after all relationships are established
        self.rebuild_all_effective_last_activity();
    }

    /// Rebuild effective_last_activity for all threads.
    /// Called after runtime hierarchy relationships are fully established.
    fn rebuild_all_effective_last_activity(&mut self) {
        // Collect all thread IDs first to avoid borrow issues
        let thread_ids: Vec<String> = self
            .threads_by_project
            .values()
            .flat_map(|threads| threads.iter().map(|t| t.id.clone()))
            .collect();

        // Update each thread's effective_last_activity
        for thread_id in thread_ids {
            let effective = self
                .runtime_hierarchy
                .get_effective_last_activity(&thread_id);
            for threads in self.threads_by_project.values_mut() {
                if let Some(thread) = threads.iter_mut().find(|t| t.id == thread_id) {
                    thread.effective_last_activity = effective;
                    break;
                }
            }
        }
    }

    /// Calculate total LLM runtime from a set of messages.
    ///
    /// Returns runtime in MILLISECONDS (llm-runtime tag values are already in milliseconds)
    ///
    /// Filters out messages created before RUNTIME_CUTOFF_TIMESTAMP (Jan 24, 2025)
    /// due to tracking methodology changes that make older data incomparable.
    ///
    /// NOTE: Performance consideration - This scans all messages in the conversation
    /// each time it's called. For conversations with M messages, this is O(M).
    /// When called for each new message, total complexity becomes O(M^2) over the
    /// lifetime of the conversation. For most conversations this is acceptable,
    /// but extremely long conversations (1000+ messages) may see degraded performance.
    /// A future optimization could maintain a running sum delta.
    fn calculate_runtime_from_messages(messages: &[Message]) -> u64 {
        messages
            .iter()
            .filter(|msg| msg.created_at >= RUNTIME_CUTOFF_TIMESTAMP)
            .filter_map(|msg| {
                msg.llm_metadata
                    .get("runtime")
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .sum::<u64>()
        // Values are already in milliseconds from llm-runtime tags
    }

    /// Update runtime hierarchy for a thread after messages change
    /// Called from handle_message_event after the message is added
    /// Returns true if relationships changed (new parent or children discovered)
    fn update_runtime_hierarchy_for_thread_id(&mut self, thread_id: &str) -> bool {
        let mut relationships_changed = false;

        if let Some(messages) = self.messages_by_thread.get(thread_id) {
            // Update conversation creation time if not already set
            // (Use the earliest message timestamp as the creation time)
            if self
                .runtime_hierarchy
                .get_conversation_created_at(thread_id)
                .is_none()
            {
                if let Some(earliest_created_at) = messages.iter().map(|m| m.created_at).min() {
                    self.runtime_hierarchy
                        .set_conversation_created_at(thread_id, earliest_created_at);
                }
            }

            // Update relationships from all messages
            for message in messages {
                // Update q-tag relationships (this message's children)
                for child_id in &message.q_tags {
                    if self.runtime_hierarchy.add_child(thread_id, child_id) {
                        relationships_changed = true;
                    }
                }

                // Update delegation relationship (this conversation's parent)
                if let Some(ref parent_id) = message.delegation_tag {
                    if parent_id != thread_id
                        && self.runtime_hierarchy.set_parent(thread_id, parent_id)
                    {
                        relationships_changed = true;
                    }
                }
            }

            // Recalculate this conversation's individual runtime
            let runtime_ms = Self::calculate_runtime_from_messages(messages);
            self.runtime_hierarchy
                .set_individual_runtime(thread_id, runtime_ms);
        }

        relationships_changed
    }

    /// Update runtime hierarchy when a new thread is discovered
    /// Called from handle_thread_event after the thread is added
    fn update_runtime_hierarchy_for_thread(&mut self, thread: &Thread) {
        if let Some(ref parent_id) = thread.parent_conversation_id {
            self.runtime_hierarchy.set_parent(&thread.id, parent_id);
        }
        // Set the thread's creation time from last_activity (which equals created_at for new threads)
        self.runtime_hierarchy
            .set_conversation_created_at(&thread.id, thread.last_activity);
        // Set the thread's individual last_activity
        self.runtime_hierarchy
            .set_individual_last_activity(&thread.id, thread.last_activity);
    }

    /// Get the total hierarchical runtime for a conversation (own + all children recursively)
    pub fn get_hierarchical_runtime(&self, thread_id: &str) -> u64 {
        self.runtime_hierarchy.get_total_runtime(thread_id)
    }

    /// Get the individual (net) runtime for a conversation (just this conversation, no children)
    pub fn get_individual_runtime(&self, thread_id: &str) -> u64 {
        self.runtime_hierarchy.get_individual_runtime(thread_id)
    }

    /// Get all ancestor conversation IDs that would be affected by this conversation's runtime change
    pub fn get_runtime_ancestors(&self, thread_id: &str) -> Vec<String> {
        self.runtime_hierarchy.get_ancestors(thread_id)
    }

    /// Get the total unique runtime across all conversations (flat aggregation).
    /// Each conversation's runtime is counted exactly once, regardless of hierarchy.
    /// Used for the global status bar cumulative runtime display.
    pub fn get_total_unique_runtime(&self) -> u64 {
        self.runtime_hierarchy.get_total_unique_runtime()
    }

    /// Get the effective last_activity for a conversation (own + all descendants).
    /// Used for hierarchical sorting in the Conversations tab.
    pub fn get_effective_last_activity(&self, thread_id: &str) -> u64 {
        self.runtime_hierarchy
            .get_effective_last_activity(thread_id)
    }

    /// Update effective_last_activity on a thread and propagate up the ancestor chain.
    /// This should be called whenever a thread's last_activity changes.
    fn propagate_effective_last_activity(&mut self, thread_id: &str) {
        // First, update the individual last_activity in RuntimeHierarchy
        // (get it from the actual thread)
        if let Some(last_activity) = self.get_thread_last_activity(thread_id) {
            self.runtime_hierarchy
                .set_individual_last_activity(thread_id, last_activity);
        }

        // Now update this thread's effective_last_activity
        self.update_thread_effective_last_activity(thread_id);

        // Walk up the ancestor chain and update each ancestor's effective_last_activity
        let ancestors = self.runtime_hierarchy.get_ancestors(thread_id);
        for ancestor_id in ancestors {
            self.update_thread_effective_last_activity(&ancestor_id);
        }
    }

    /// Get the last_activity from a thread (helper for propagation)
    fn get_thread_last_activity(&self, thread_id: &str) -> Option<u64> {
        for threads in self.threads_by_project.values() {
            if let Some(thread) = threads.iter().find(|t| t.id == thread_id) {
                return Some(thread.last_activity);
            }
        }
        None
    }

    /// Update effective_last_activity on a specific thread
    fn update_thread_effective_last_activity(&mut self, thread_id: &str) {
        let effective = self
            .runtime_hierarchy
            .get_effective_last_activity(thread_id);

        for threads in self.threads_by_project.values_mut() {
            if let Some(thread) = threads.iter_mut().find(|t| t.id == thread_id) {
                thread.effective_last_activity = effective;
                break;
            }
        }
    }

    pub fn get_today_unique_runtime(&self) -> u64 {
        self.statistics.get_today_unique_runtime()
    }

    /// Helper: iterate over all (message, cost_usd) pairs across all threads.
    /// Extracts cost-usd from llm_metadata, returning only messages with valid costs.
    fn iter_message_costs(&self) -> impl Iterator<Item = (&Message, f64)> {
        self.messages_by_thread
            .values()
            .flat_map(|messages| messages.iter())
            .filter_map(|msg| {
                msg.llm_metadata
                    .get("cost-usd")
                    .and_then(|value| value.parse::<f64>().ok())
                    .map(|cost| (msg, cost))
            })
    }

    /// Get total cost across all messages (sum of llm-cost-usd tags).
    /// Returns the total cost in USD as a float.
    pub fn get_total_cost(&self) -> f64 {
        self.iter_message_costs().map(|(_, cost)| cost).sum()
    }

    /// Get total cost for messages created since a given timestamp.
    ///
    /// # Arguments
    /// * `since_timestamp_secs` - Unix epoch timestamp in seconds. Messages with
    ///   `created_at >= since_timestamp_secs` are included in the sum.
    ///
    /// # Returns
    /// Total cost in USD as a float. Returns 0.0 if no messages match.
    ///
    /// # Edge Cases
    /// * If `since_timestamp_secs` is in the future, returns 0.0 (no messages match)
    /// * If `since_timestamp_secs` is 0, effectively returns all-time cost
    pub fn get_total_cost_since(&self, since_timestamp_secs: u64) -> f64 {
        self.iter_message_costs()
            .filter(|(msg, _)| msg.created_at >= since_timestamp_secs)
            .map(|(_, cost)| cost)
            .sum()
    }

    /// Get cost aggregated by project.
    /// Returns (project_a_tag, project_name, total_cost) tuples sorted by cost descending.
    pub fn get_cost_by_project(&self) -> Vec<(String, String, f64)> {
        let mut costs: HashMap<String, f64> = HashMap::new();

        for (a_tag, threads) in &self.threads_by_project {
            let thread_ids: std::collections::HashSet<&str> =
                threads.iter().map(|t| t.id.as_str()).collect();

            let project_cost: f64 = self
                .iter_message_costs()
                .filter(|(msg, _)| thread_ids.contains(msg.thread_id.as_str()))
                .map(|(_, cost)| cost)
                .sum();

            if project_cost > 0.0 {
                costs.insert(a_tag.clone(), project_cost);
            }
        }

        // Convert to vec with project names and sort by cost descending
        let mut result: Vec<(String, String, f64)> = costs
            .into_iter()
            .map(|(a_tag, cost)| {
                let name = self
                    .projects
                    .iter()
                    .find(|p| p.a_tag() == a_tag)
                    .map(|p| p.title.clone())
                    .unwrap_or_else(|| "Unknown".to_string());
                (a_tag, name, cost)
            })
            .collect();
        result.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        result
    }

    /// Get message counts aggregated by day for the Stats tab bar chart.
    /// Returns two vectors:
    /// - First: messages from the current user (day_start_timestamp, count) tuples
    /// - Second: all messages from anyone a-tagging our projects (day_start_timestamp, count) tuples
    ///
    ///   Both vectors cover the same time window (num_days).
    ///
    /// Uses pre-aggregated counters for O(num_days) performance instead of O(total_messages).
    /// Data is queried directly from nostrdb using the `.authors()` filter for user messages
    /// and a-tag filters for project messages (no double-counting from agent_chatter).
    // ===== Statistics Methods (delegated to StatisticsStore) =====
    pub fn get_messages_by_day(&self, num_days: usize) -> MessagesByDayCounts {
        self.statistics.get_messages_by_day(num_days)
    }

    pub fn get_tokens_by_hour(&self, num_hours: usize) -> HashMap<u64, u64> {
        self.statistics.get_tokens_by_hour(num_hours)
    }

    pub fn get_tokens_by_hour_from(
        &self,
        current_hour_start: u64,
        num_hours: usize,
    ) -> HashMap<u64, u64> {
        self.statistics
            .get_tokens_by_hour_from(current_hour_start, num_hours)
    }

    pub fn get_message_count_by_hour(&self, num_hours: usize) -> HashMap<u64, u64> {
        self.statistics.get_message_count_by_hour(num_hours)
    }

    pub fn get_message_count_by_hour_from(
        &self,
        current_hour_start: u64,
        num_hours: usize,
    ) -> HashMap<u64, u64> {
        self.statistics
            .get_message_count_by_hour_from(current_hour_start, num_hours)
    }

    /// Apply all existing kind:513 metadata events to threads (called during rebuild)
    /// Only applies the MOST RECENT metadata event for each thread.
    /// Uses project-scoped metadata loading to avoid global query limits.
    fn apply_existing_metadata(&mut self) {
        // Step 1: Collect all thread IDs across all projects and fetch their metadata
        // This avoids the global 1000-event limit that caused metadata to be missing
        // for older conversations when there are many projects/threads
        let all_thread_ids: HashSet<String> = self
            .threads_by_project
            .values()
            .flat_map(|threads| threads.iter().map(|t| t.id.clone()))
            .collect();

        if all_thread_ids.is_empty() {
            return;
        }

        // Fetch metadata for all threads at once (still project-scoped by thread IDs)
        let Ok(metadata_map) = crate::store::get_metadata_for_threads(&self.ndb, &all_thread_ids)
        else {
            return;
        };

        // Step 2: Apply metadata to all threads
        for threads in self.threads_by_project.values_mut() {
            for thread in threads.iter_mut() {
                if let Some(metadata) = metadata_map.get(&thread.id) {
                    if let Some(ref title) = metadata.title {
                        thread.title = title.clone();
                    }
                    thread.status_label = metadata.status_label.clone();
                    thread.status_current_activity = metadata.status_current_activity.clone();
                    thread.summary = metadata.summary.clone();
                    thread.hashtags = metadata.hashtags.clone();
                    // Only update last_activity if metadata is newer than current
                    // to avoid regressing timestamps when metadata is older than newest message
                    if metadata.created_at > thread.last_activity {
                        thread.last_activity = metadata.created_at;
                    }
                }
            }
        }

        // Update individual_last_activity in RuntimeHierarchy for threads that had metadata applied
        // and rebuild effective_last_activity
        for threads in self.threads_by_project.values() {
            for thread in threads {
                self.runtime_hierarchy
                    .set_individual_last_activity(&thread.id, thread.last_activity);
            }
        }

        // Rebuild effective_last_activity for all threads after metadata is applied
        self.rebuild_all_effective_last_activity();

        // Re-sort all thread lists by effective_last_activity after applying metadata
        for threads in self.threads_by_project.values_mut() {
            threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
        }
    }

    /// Handle a new event from SubscriptionStream - incrementally update data
    /// Returns an optional CoreEvent for kinds that need special handling (24010)
    pub fn handle_event(&mut self, kind: u32, note: &Note) -> Option<crate::events::CoreEvent> {
        match kind {
            31933 => {
                self.handle_project_event(note);
                None
            }
            1 => {
                self.handle_text_event(note);
                None
            }
            0 => {
                self.handle_profile_event(note);
                None
            }
            24010 => self.handle_status_event(note),
            513 => {
                self.handle_metadata_event(note);
                None
            }
            4129 => {
                self.handle_lesson_event(note);
                None
            }
            4199 => {
                self.content.handle_agent_definition_event(note);
                None
            }
            34199 => {
                self.content.handle_team_pack_event(note);
                None
            }
            4200 => {
                self.content.handle_mcp_tool_event(note);
                None
            }
            4201 => {
                self.content.handle_nudge_event(note);
                None
            }
            4202 => {
                self.content.handle_skill_event(note);
                None
            }
            14202 => {
                self.handle_bookmark_list_event(note);
                None
            }
            24133 => {
                self.operations.handle_operations_status_event(note);
                None
            }
            30023 => {
                let known_a_tags: Vec<String> = self.projects.iter().map(|p| p.a_tag()).collect();
                self.reports
                    .handle_report_event(note, &known_a_tags)
                    .map(crate::events::CoreEvent::ReportUpsert)
            }
            _ => None,
        }
    }

    // ===== Bookmark Methods (kind:14202) =====

    /// Handle a kind:14202 bookmark list event.
    /// Replaces existing bookmark list for the author's pubkey if this event is newer.
    fn handle_bookmark_list_event(&mut self, note: &Note) {
        if let Some(bookmark_list) = BookmarkList::from_note(note) {
            let should_update = self
                .bookmarks
                .get(&bookmark_list.pubkey)
                .map_or(true, |existing| {
                    bookmark_list.last_updated > existing.last_updated
                });

            if should_update {
                self.bookmarks
                    .insert(bookmark_list.pubkey.clone(), bookmark_list);
            }
        }
    }

    /// Get the bookmark list for a given pubkey.
    pub fn get_bookmarks(&self, pubkey: &str) -> Option<&BookmarkList> {
        self.bookmarks.get(pubkey)
    }

    /// Check whether a given item ID is bookmarked by the specified pubkey.
    pub fn is_bookmarked(&self, pubkey: &str, item_id: &str) -> bool {
        self.bookmarks
            .get(pubkey)
            .map(|bl| bl.contains(item_id))
            .unwrap_or(false)
    }

    /// Store a bookmark list for a given pubkey (used for optimistic updates).
    pub fn set_bookmarks(&mut self, pubkey: &str, bookmarks: BookmarkList) {
        self.bookmarks.insert(pubkey.to_string(), bookmarks);
    }

    /// Load bookmarks from nostrdb for the current user (called after login).
    pub fn load_bookmarks(&mut self) {
        let Some(user_pubkey) = self.user_pubkey.clone() else {
            return;
        };

        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        // Decode the user's hex pubkey to a 32-byte array for the nostrdb author filter.
        // This scopes the DB query to only events authored by the current user,
        // preventing the 100-result cap from hiding the user's own bookmark event
        // when other users' kind:14202 events are also stored locally.
        let pubkey_bytes: [u8; 32] = {
            let Ok(decoded) = hex::decode(&user_pubkey) else {
                return;
            };
            let Ok(arr) = decoded.try_into() else {
                return;
            };
            arr
        };

        let filter = nostrdb::Filter::new()
            .kinds([14202])
            .authors([&pubkey_bytes])
            .limit(1)
            .build();
        let Ok(results) = self.ndb.query(&txn, &[filter], 1) else {
            return;
        };

        let mut all_lists: Vec<BookmarkList> = Vec::new();
        for result in results {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(bl) = BookmarkList::from_note(&note) {
                    all_lists.push(bl);
                }
            }
        }

        // Keep the most recent bookmark list
        all_lists.sort_by_key(|bl| bl.last_updated);
        if let Some(latest) = all_lists.pop() {
            self.bookmarks.insert(latest.pubkey.clone(), latest);
        }
    }

    /// Unified handler for kind:1 events - dispatches to thread or message handler based on e-tag presence
    /// Thread detection: kind:1 + has a-tag + NO e-tags (ignoring skill marker e-tags)
    /// Message detection: kind:1 + has e-tag with "root" or "reply" marker per NIP-10
    fn handle_text_event(&mut self, note: &Note) {
        // Check for e-tags to determine if this is a thread or message
        // IMPORTANT: Ignore e-tags with "skill" marker - those are skill references, not thread/reply markers
        let mut has_e_tag = false;
        for tag in note.tags() {
            if tag.get(0).and_then(|t| t.variant().str()) == Some("e") {
                // Check if this e-tag has a "skill" marker
                // NIP-10 format: ["e", id, relay, marker] - marker at index 3
                // Some clients omit relay: ["e", id, "skill"] - marker at index 2
                let marker_at_3 = tag.get(3).and_then(|t| t.variant().str());
                let marker_at_2 = tag.get(2).and_then(|t| t.variant().str());
                let is_skill = marker_at_3 == Some("skill") || marker_at_2 == Some("skill");
                if !is_skill {
                    has_e_tag = true;
                    break;
                }
            }
        }

        if has_e_tag {
            // Has e-tag: it's a message
            self.handle_message_event(note);
        } else {
            // No e-tag: it's a thread
            self.handle_thread_event(note);

            // Check if this is a document discussion thread (has a-tag for a report)
            for tag in note.tags() {
                if tag.get(0).and_then(|t| t.variant().str()) == Some("a") {
                    if let Some(a_val) = tag.get(1).and_then(|t| t.variant().str()) {
                        // Check if it's a report a-tag (30023:pubkey:slug)
                        if a_val.starts_with("30023:") {
                            if let Some(thread) = Thread::from_note(note) {
                                self.reports.add_document_thread(a_val, thread);
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    fn project_revision_is_newer_or_preferred(
        candidate_created_at: u64,
        candidate_is_deleted: bool,
        current_created_at: u64,
        current_is_deleted: bool,
    ) -> bool {
        candidate_created_at > current_created_at
            || (candidate_created_at == current_created_at
                && candidate_is_deleted
                && !current_is_deleted)
    }

    fn current_project_revision(&self, a_tag: &str) -> Option<(u64, bool)> {
        let live_revision = self
            .projects
            .iter()
            .find(|p| p.a_tag() == a_tag)
            .map(|p| (p.created_at, false));
        let tombstone_revision = self
            .project_tombstones
            .get(a_tag)
            .copied()
            .map(|ts| (ts, true));

        match (live_revision, tombstone_revision) {
            (Some(live), Some(tombstone)) => {
                if Self::project_revision_is_newer_or_preferred(
                    tombstone.0,
                    tombstone.1,
                    live.0,
                    live.1,
                ) {
                    Some(tombstone)
                } else {
                    Some(live)
                }
            }
            (Some(live), None) => Some(live),
            (None, Some(tombstone)) => Some(tombstone),
            (None, None) => None,
        }
    }

    fn handle_project_event(&mut self, note: &Note) {
        // Parse project directly from the note we already have
        // (Don't re-query - nostrdb indexes asynchronously, so query might miss it)
        if let Some(project) = Project::from_note(note) {
            let a_tag = project.a_tag();
            let existing_index = self.projects.iter().position(|p| p.a_tag() == a_tag);

            if let Some((current_created_at, current_is_deleted)) =
                self.current_project_revision(&a_tag)
            {
                if !Self::project_revision_is_newer_or_preferred(
                    project.created_at,
                    project.is_deleted,
                    current_created_at,
                    current_is_deleted,
                ) {
                    return;
                }
            }

            if project.is_deleted {
                if let Some(index) = existing_index {
                    self.projects.remove(index);
                }
                self.project_tombstones.insert(a_tag, project.created_at);
            } else {
                self.project_tombstones.remove(&a_tag);
                if let Some(index) = existing_index {
                    self.projects[index] = project;
                } else {
                    self.projects.push(project);
                }
            }
        }
    }

    /// Handle a status event from JSON (ephemeral events via DataChange channel)
    /// Routes to appropriate handler based on event kind (24010 or 24133)
    /// Parses JSON once and passes the value to handlers to avoid double parsing
    pub fn handle_status_event_json(&mut self, json: &str) {
        if let Ok(event) = serde_json::from_str::<serde_json::Value>(json) {
            self.handle_status_event_value(&event);
        }
    }

    /// Handle a status event from a pre-parsed Value (avoids double parsing).
    pub fn handle_status_event_value(&mut self, event: &serde_json::Value) {
        if let Some(kind) = event.get("kind").and_then(|k| k.as_u64()) {
            match kind {
                24010 => self.handle_project_status_event_value(event),
                24133 => self.handle_operations_status_event_value(event),
                _ => {} // Ignore unknown kinds
            }
        }
    }

    /// Handle a project status event from pre-parsed Value (kind:24010)
    fn handle_project_status_event_value(&mut self, event: &serde_json::Value) {
        if let Some(status) = ProjectStatus::from_value(event) {
            let backend_pubkey = status.backend_pubkey.clone();
            let should_update = self
                .project_statuses
                .get(&status.project_coordinate)
                .map(|existing| status.created_at >= existing.created_at)
                .unwrap_or(true);
            let mut status = status;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            status.last_seen_at = now;

            if self.trust.is_blocked(&backend_pubkey) {
                return;
            }

            if self.trust.is_approved(&backend_pubkey) {
                if should_update {
                    self.project_statuses
                        .insert(status.project_coordinate.clone(), status);
                }
                return;
            }

            // Unknown backend - queue for approval
            let project_coord = status.project_coordinate.clone();
            self.trust
                .queue_or_update_pending(&backend_pubkey, &project_coord, status);
        }
    }

    /// Handle an operations status event from pre-parsed Value (kind:24133)
    fn handle_operations_status_event_value(&mut self, event: &serde_json::Value) {
        self.operations.handle_operations_status_event_value(event);
    }

    fn handle_status_event(&mut self, note: &Note) -> Option<crate::events::CoreEvent> {
        let mut status = ProjectStatus::from_note(note)?;
        let backend_pubkey = status.backend_pubkey.clone();

        if self.trust.is_blocked(&backend_pubkey) {
            return None;
        }

        if self.trust.is_approved(&backend_pubkey) {
            let should_update = self
                .project_statuses
                .get(&status.project_coordinate)
                .map(|existing| status.created_at >= existing.created_at)
                .unwrap_or(true);
            if should_update {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                status.last_seen_at = now;
                let event = crate::events::CoreEvent::ProjectStatus(status.clone());
                self.project_statuses
                    .insert(status.project_coordinate.clone(), status);
                return Some(event);
            }
            return None;
        }

        // Unknown backend - check if already pending
        let already_pending = self
            .trust
            .has_pending_approval(&backend_pubkey, &status.project_coordinate);
        self.trust.queue_or_update_pending(
            &backend_pubkey,
            &status.project_coordinate,
            status.clone(),
        );

        if already_pending {
            None
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let pending = PendingBackendApproval {
                backend_pubkey,
                project_a_tag: status.project_coordinate.clone(),
                first_seen: now,
                status,
            };
            Some(crate::events::CoreEvent::PendingBackendApproval(pending))
        }
    }

    fn handle_profile_event(&mut self, note: &Note) {
        let pubkey = hex::encode(note.pubkey());
        if let Some(name) = self.extract_profile_name(note) {
            self.profiles.insert(pubkey, name);
        }
    }

    fn handle_thread_event(&mut self, note: &Note) {
        // Parse thread directly from the note we already have
        // (Don't re-query - nostrdb indexes asynchronously, so query might miss it)
        if let Some(thread) = Thread::from_note(note) {
            let thread_id = thread.id.clone();
            // Capture whether thread has parent before moving thread
            let has_parent_from_thread = thread.parent_conversation_id.is_some();

            // Update runtime hierarchy for parent-child relationship
            self.update_runtime_hierarchy_for_thread(&thread);

            if let Some(a_tag) = Self::extract_project_a_tag(note) {
                // Add to thread root index
                self.thread_root_index
                    .entry(a_tag.clone())
                    .or_default()
                    .insert(thread_id.clone());

                // Reconcile last_activity with any messages that arrived before this thread
                // (handles out-of-order message arrival)
                let mut reconciled_thread = thread;
                let mut was_reconciled = false;
                if let Some(messages) = self.messages_by_thread.get(&thread_id) {
                    if let Some(max_message_time) = messages.iter().map(|m| m.created_at).max() {
                        if max_message_time > reconciled_thread.last_activity {
                            reconciled_thread.last_activity = max_message_time;
                            reconciled_thread.effective_last_activity = max_message_time;
                            // Update runtime hierarchy with reconciled timestamp
                            self.runtime_hierarchy
                                .set_individual_last_activity(&thread_id, max_message_time);
                            was_reconciled = true;
                        }
                    }
                }

                // Add to existing threads list, maintaining sort order by effective_last_activity
                let threads = self.threads_by_project.entry(a_tag).or_default();

                // Check if thread already exists (avoid duplicates)
                if !threads.iter().any(|t| t.id == thread_id) {
                    // Insert in sorted position by effective_last_activity (most recent first)
                    let insert_pos = threads.partition_point(|t| {
                        t.effective_last_activity > reconciled_thread.effective_last_activity
                    });
                    threads.insert(insert_pos, reconciled_thread);
                }

                // Check if this thread has a parent - either from Thread struct or from runtime hierarchy
                // (parent links can be discovered via q-tags in older messages)
                let has_parent = has_parent_from_thread
                    || self.runtime_hierarchy.get_parent(&thread_id).is_some();

                // Check if this thread has children already in the hierarchy
                // (children can arrive before parent due to out-of-order message delivery)
                let has_children = self
                    .runtime_hierarchy
                    .get_children(&thread_id)
                    .map(|c| !c.is_empty())
                    .unwrap_or(false);

                // Propagate/recompute effective_last_activity if:
                // 1. This thread has a parent (new child bumps ancestors), OR
                // 2. We reconciled with preloaded messages (late-arriving thread root needs to propagate), OR
                // 3. This thread has children already (parent arrived after children - need to pick up child activity)
                if has_parent || was_reconciled || has_children {
                    // If this thread has children, first recompute its own effective_last_activity
                    // from its descendants before propagating up to ancestors
                    if has_children {
                        self.update_thread_effective_last_activity(&thread_id);
                    }

                    self.propagate_effective_last_activity(&thread_id);

                    // Re-sort threads by effective_last_activity
                    for threads in self.threads_by_project.values_mut() {
                        threads.sort_by(|a, b| {
                            b.effective_last_activity.cmp(&a.effective_last_activity)
                        });
                    }
                }
            }

            // Also add the thread root as the first message in the conversation
            // This ensures the initial kind:1 that started the conversation is rendered
            if let Some(root_message) = Message::from_thread_note(note) {
                self.check_and_add_inbox_item(note, &thread_id, &root_message);

                let messages = self.messages_by_thread.entry(thread_id).or_default();

                // Check if message already exists (avoid duplicates)
                if !messages.iter().any(|m| m.id == root_message.id) {
                    // Thread root is always the first message (oldest), insert at beginning
                    messages.insert(0, root_message);
                }
            }
        }
    }

    fn handle_message_event(&mut self, note: &Note) {
        // Parse message directly from the note we already have
        // (Don't re-query - nostrdb indexes asynchronously, so query might miss it)
        if let Some(message) = Message::from_note(note) {
            let thread_id = message.thread_id.clone();
            let pubkey = message.pubkey.clone();
            let message_id = message.id.clone();

            // Check if message a-tags one of our projects for agent chatter feed
            if let Some(a_tag) = Self::extract_project_a_tag(note) {
                if self.projects.iter().any(|p| p.a_tag() == a_tag) {
                    let chatter = AgentChatter::Message {
                        id: message_id.clone(),
                        content: message.content.clone(),
                        project_a_tag: a_tag,
                        author_pubkey: pubkey.clone(),
                        created_at: message.created_at,
                        thread_id: thread_id.clone(),
                    };
                    self.add_agent_chatter(chatter);
                }
            }

            // Auto-mark inbox items as read when user replies
            // If this message is from the current user and has e-tags, mark those items read
            if let Some(ref user_pk) = self.user_pubkey.clone() {
                if pubkey == *user_pk {
                    // This is a message from the current user - check if it replies to an inbox item
                    let reply_to_ids = Self::extract_e_tag_ids(note);
                    for reply_to_id in &reply_to_ids {
                        if self.inbox.contains(reply_to_id) {
                            self.inbox.mark_read(reply_to_id);
                        }
                    }
                }
            }

            // Check for inbox: any kind:1 that p-tags the user (Ask or Mention)
            self.check_and_add_inbox_item(note, &thread_id, &message);

            // Add to existing messages list, maintaining sort order by created_at
            let messages = self
                .messages_by_thread
                .entry(thread_id.clone())
                .or_default();

            // Check if message already exists (avoid duplicates)
            if !messages.iter().any(|m| m.id == message_id) {
                let message_created_at = message.created_at;
                let message_pubkey = message.pubkey.clone();
                let message_llm_metadata = message.llm_metadata.clone();

                // Check if this message has llm-runtime tag (confirms runtime, resets unconfirmed timer)
                let has_llm_runtime = message.llm_metadata.contains_key("runtime");

                // Insert in sorted position (oldest first)
                let insert_pos = messages.partition_point(|m| m.created_at < message_created_at);
                messages.insert(insert_pos, message);

                // If this message has llm-runtime tag, reset the unconfirmed timer for this agent on this conversation
                // This ensures unconfirmed runtime only tracks time since the last kind:1 confirmation
                // The recency guard prevents stale/backfilled messages from resetting active timers
                if has_llm_runtime {
                    self.operations.agent_tracking.reset_unconfirmed_timer(
                        &thread_id,
                        &message_pubkey,
                        message_created_at,
                    );
                }

                // Update pre-aggregated statistics (O(1) per message)
                self.statistics.increment_message_day_count(
                    message_created_at,
                    &message_pubkey,
                    self.user_pubkey.as_deref(),
                );
                self.statistics
                    .increment_llm_activity_hour(message_created_at, &message_llm_metadata);
                self.statistics
                    .increment_runtime_day_count(message_created_at, &message_llm_metadata);

                // Update runtime hierarchy after inserting message
                // (captures q-tags and delegation tags, then recalculates runtime from messages)
                // Returns true if relationships changed (new parent or children discovered)
                let relationships_changed = self.update_runtime_hierarchy_for_thread_id(&thread_id);

                // Update thread's last_activity so it appears in Conversations tab
                let mut last_activity_updated = false;
                for threads in self.threads_by_project.values_mut() {
                    if let Some(thread) = threads.iter_mut().find(|t| t.id == thread_id) {
                        // Only update if this message is newer than current last_activity
                        if message_created_at > thread.last_activity {
                            thread.last_activity = message_created_at;
                            last_activity_updated = true;
                        }
                        break;
                    }
                }

                // Propagate effective_last_activity up the hierarchy if:
                // - last_activity changed (new activity in this conversation)
                // - relationships changed (new parent/child discovered, ancestors need update)
                if last_activity_updated || relationships_changed {
                    self.propagate_effective_last_activity(&thread_id);

                    // Re-sort threads by effective_last_activity (most recent first)
                    for threads in self.threads_by_project.values_mut() {
                        threads.sort_by(|a, b| {
                            b.effective_last_activity.cmp(&a.effective_last_activity)
                        });
                    }
                }
            }
        }
    }

    /// Check if a note p-tags a specific user
    fn note_ptags_user(&self, note: &Note, user_pubkey: &str) -> bool {
        for tag in note.tags() {
            if tag.count() >= 2 {
                let tag_name = tag.get(0).and_then(|t| t.variant().str());
                if tag_name == Some("p") {
                    // Try string first, then id bytes (same pattern as e-tag handling)
                    let pk = if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        Some(s.to_string())
                    } else {
                        tag.get(1).and_then(|t| t.variant().id()).map(hex::encode)
                    };
                    if pk.as_deref() == Some(user_pubkey) {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Check if a note has an ask tag
    /// Supports: ["ask", "true"], ["ask", "1"], or just ["ask"]
    fn note_is_ask_event(&self, note: &Note) -> bool {
        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            if tag_name == Some("ask") {
                // Check if it's ["ask"], ["ask", "true"], or ["ask", "1"]
                if tag.count() == 1 {
                    return true;
                }
                if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                    if value == "true" || value == "1" {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// Find which project a thread belongs to
    pub fn find_project_for_thread(&self, thread_id: &str) -> Option<String> {
        for (a_tag, threads) in &self.threads_by_project {
            if threads.iter().any(|t| t.id == thread_id) {
                return Some(a_tag.clone());
            }
        }
        None
    }

    fn handle_metadata_event(&mut self, note: &Note) {
        // Parse metadata directly from the note to update thread title and status
        if let Some(metadata) = ConversationMetadata::from_note(note) {
            let thread_id = metadata.thread_id.clone();
            let thread_id_short = thread_id[..16.min(thread_id.len())].to_string();
            let title = metadata.title.clone();
            let status_label = metadata.status_label;
            let status_current_activity = metadata.status_current_activity;
            let summary = metadata.summary;
            let hashtags = metadata.hashtags;
            let created_at = metadata.created_at;

            let _ = thread_id_short; // For debugging

            // Find the thread across all projects and update its fields
            let mut last_activity_updated = false;
            for threads in self.threads_by_project.values_mut() {
                if let Some(thread) = threads.iter_mut().find(|t| t.id == thread_id) {
                    if let Some(new_title) = title.clone() {
                        thread.title = new_title;
                    }
                    // Update status fields
                    thread.status_label = status_label;
                    thread.status_current_activity = status_current_activity;
                    thread.summary = summary;
                    thread.hashtags = hashtags;
                    // Update last_activity
                    if created_at > thread.last_activity {
                        thread.last_activity = created_at;
                        last_activity_updated = true;
                    }
                    break;
                }
            }

            // Propagate effective_last_activity up the hierarchy if last_activity changed
            if last_activity_updated {
                self.propagate_effective_last_activity(&thread_id);

                // Re-sort threads by effective_last_activity (most recent first)
                for threads in self.threads_by_project.values_mut() {
                    threads
                        .sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
                }
            }
        } else {
            // Failed to parse kind:513 metadata event
        }
    }

    fn extract_project_a_tag(note: &Note) -> Option<String> {
        for tag in note.tags() {
            if tag.count() >= 2 {
                let tag_name = tag.get(0).and_then(|t| t.variant().str());
                if tag_name == Some("a") {
                    if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                        // Only return project a-tags (31933:pubkey:id), not report a-tags (30023:...)
                        if value.starts_with("31933:") {
                            return Some(value.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Extract all e-tag event IDs from a note (used for auto-marking inbox items as read)
    fn extract_e_tag_ids(note: &Note) -> Vec<String> {
        let mut ids = Vec::new();
        for tag in note.tags() {
            if tag.count() >= 2 {
                let tag_name = tag.get(0).and_then(|t| t.variant().str());
                if tag_name == Some("e") {
                    // Try string first, then id bytes
                    let event_id = if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        Some(s.to_string())
                    } else {
                        tag.get(1).and_then(|t| t.variant().id()).map(hex::encode)
                    };
                    if let Some(id) = event_id {
                        ids.push(id);
                    }
                }
            }
        }
        ids
    }

    fn extract_profile_name(&self, note: &Note) -> Option<String> {
        let txn = Transaction::new(&self.ndb).ok()?;
        let pubkey_bytes = note.pubkey();

        if let Ok(profile) = self.ndb.get_profile_by_pubkey(&txn, pubkey_bytes) {
            let record = profile.record();
            if let Some(profile_data) = record.profile() {
                if let Some(name) = profile_data.display_name() {
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
                if let Some(name) = profile_data.name() {
                    if !name.is_empty() {
                        return Some(name.to_string());
                    }
                }
            }
        }
        None
    }

    // Getters - return references for efficient access

    pub fn get_projects(&self) -> &[Project] {
        &self.projects
    }

    /// Query projects directly from nostrdb (bypasses in-memory cache).
    /// Use this when you need fresh project data that may not be in the cache yet.
    /// For example, the HTTP server uses this because it doesn't receive project
    /// update events like the main data store does.
    pub fn query_projects_from_ndb(&self) -> Vec<Project> {
        crate::store::get_projects(&self.ndb).unwrap_or_default()
    }

    /// Find a project by its d-tag (slug) directly from nostrdb.
    /// Returns the project's a_tag if found.
    pub fn find_project_a_tag_by_dtag_from_ndb(&self, dtag: &str) -> Option<String> {
        self.query_projects_from_ndb()
            .into_iter()
            .find(|p| p.id == dtag)
            .map(|p| p.a_tag())
    }

    /// Drain pending project subscriptions (called by CoreRuntime after processing events)
    pub fn drain_pending_project_subscriptions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_project_subscriptions)
    }

    pub fn get_project_status(&self, a_tag: &str) -> Option<&ProjectStatus> {
        self.project_statuses.get(a_tag)
    }

    /// Get online agents for a project (from ProjectStatus if online)
    pub fn get_online_agents(&self, a_tag: &str) -> Option<&[ProjectAgent]> {
        self.project_statuses
            .get(a_tag)
            .filter(|s| s.is_online())
            .map(|s| s.agents.as_slice())
    }

    pub fn is_project_online(&self, a_tag: &str) -> bool {
        self.get_online_agents(a_tag).is_some()
    }

    pub fn get_threads(&self, project_a_tag: &str) -> &[Thread] {
        self.threads_by_project
            .get(project_a_tag)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get thread root index for a project (set of known root event IDs)
    pub fn get_thread_root_index(&self, project_a_tag: &str) -> Option<&HashSet<String>> {
        self.thread_root_index.get(project_a_tag)
    }

    /// Get count of known thread roots for a project
    pub fn get_thread_root_count(&self, project_a_tag: &str) -> usize {
        self.thread_root_index
            .get(project_a_tag)
            .map(|s| s.len())
            .unwrap_or(0)
    }

    pub fn get_messages(&self, thread_id: &str) -> &[Message] {
        self.messages_by_thread
            .get(thread_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    fn get_agent_slug_from_status(&self, pubkey: &str) -> Option<String> {
        self.project_statuses
            .values()
            .flat_map(|status| status.agents.iter())
            .find(|agent| agent.pubkey == pubkey)
            .map(|agent| agent.name.clone())
            .filter(|name| !name.is_empty())
    }

    pub fn get_profile_name(&self, pubkey: &str) -> String {
        if let Some(name) = self.profiles.get(pubkey) {
            return name.clone();
        }

        let lookup_started_at = Instant::now();
        let fallback = format!("{}...", &pubkey[..8.min(pubkey.len())]);
        let name = crate::store::get_profile_name(&self.ndb, pubkey);

        if name == fallback {
            if let Some(slug) = self.get_agent_slug_from_status(pubkey) {
                crate::tlog!(
                    "PERF",
                    "AppDataStore::get_profile_name cacheMiss pubkey={} source=agentStatus elapsedMs={}",
                    &pubkey[..12.min(pubkey.len())],
                    lookup_started_at.elapsed().as_millis()
                );
                return slug;
            }
        }

        let elapsed_ms = lookup_started_at.elapsed().as_millis();
        if elapsed_ms >= 2 {
            crate::tlog!(
                "PERF",
                "AppDataStore::get_profile_name cacheMiss pubkey={} fallback={} elapsedMs={}",
                &pubkey[..12.min(pubkey.len())],
                name == fallback,
                elapsed_ms
            );
        }

        name
    }

    /// Get profile picture URL for a pubkey
    pub fn get_profile_picture(&self, pubkey: &str) -> Option<String> {
        crate::store::get_profile_picture(&self.ndb, pubkey)
    }

    /// Get project name for an a_tag
    pub fn get_project_name(&self, a_tag: &str) -> String {
        self.projects
            .iter()
            .find(|p| p.a_tag() == a_tag)
            .map(|p| p.title.clone())
            .unwrap_or_else(|| "Unknown".to_string())
    }

    /// Get a thread by its ID (searches across all projects)
    pub fn get_thread_by_id(&self, thread_id: &str) -> Option<&Thread> {
        for threads in self.threads_by_project.values() {
            if let Some(thread) = threads.iter().find(|t| t.id == thread_id) {
                return Some(thread);
            }
        }
        None
    }

    /// Get the project a-tag for a given thread ID (searches across all projects)
    pub fn get_project_a_tag_for_thread(&self, thread_id: &str) -> Option<String> {
        for (project_a_tag, threads) in &self.threads_by_project {
            if threads.iter().any(|t| t.id == thread_id) {
                return Some(project_a_tag.clone());
            }
        }
        None
    }

    /// Lazy-load metadata for a specific thread if it wasn't loaded initially.
    /// This is useful when a conversation's metadata wasn't in the initial load window.
    /// Returns true if metadata was found and applied, false otherwise.
    pub fn load_metadata_for_thread(&mut self, thread_id: &str) -> bool {
        // First check if thread exists
        let thread_exists = self
            .threads_by_project
            .values()
            .any(|threads| threads.iter().any(|t| t.id == thread_id));

        if !thread_exists {
            return false;
        }

        // Try to fetch metadata for this thread
        let Ok(Some(metadata)) = crate::store::get_metadata_for_thread(&self.ndb, thread_id) else {
            return false;
        };

        // Apply metadata to the thread
        let mut metadata_applied = false;
        let mut needs_hierarchy_propagation = false;
        for threads in self.threads_by_project.values_mut() {
            if let Some(thread) = threads.iter_mut().find(|t| t.id == thread_id) {
                // Apply all metadata fields consistently (title, status, summary, last_activity)
                if let Some(ref title) = metadata.title {
                    thread.title = title.clone();
                }
                thread.status_label = metadata.status_label.clone();
                thread.status_current_activity = metadata.status_current_activity.clone();
                thread.summary = metadata.summary.clone();
                thread.hashtags = metadata.hashtags.clone();
                // Only update last_activity if metadata is newer
                if metadata.created_at > thread.last_activity {
                    thread.last_activity = metadata.created_at;
                    // Update runtime hierarchy - propagation will happen after the loop
                    self.runtime_hierarchy
                        .set_individual_last_activity(thread_id, metadata.created_at);
                    needs_hierarchy_propagation = true;
                }
                metadata_applied = true;
                break;
            }
        }

        // CRITICAL: Propagate effective_last_activity to ancestors after updating last_activity.
        // This ensures hierarchical recency sorting remains correct when metadata updates
        // a thread's timestamp. Without this, parent threads wouldn't bubble up correctly.
        if needs_hierarchy_propagation {
            self.propagate_effective_last_activity(thread_id);
        }

        // Re-sort if metadata was applied (thread position may have changed due to hierarchy propagation)
        if metadata_applied {
            for threads in self.threads_by_project.values_mut() {
                threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
            }
        }

        metadata_applied
    }

    /// Get an ask event by its ID (for q-tags that point to ask events).
    /// Returns the ask event and the pubkey of the event.
    /// Used when a delegation preview's q-tag points to an ask event rather than a thread.
    pub fn get_ask_event_by_id(&self, event_id: &str) -> Option<(crate::models::AskEvent, String)> {
        let txn = Transaction::new(&self.ndb).ok()?;
        let note_id_bytes = hex::decode(event_id).ok()?;
        let note_id: [u8; 32] = note_id_bytes.try_into().ok()?;
        let note = self.ndb.get_note_by_id(&txn, &note_id).ok()?;

        let ask_event = crate::models::Message::parse_ask_event(&note)?;
        let pubkey = hex::encode(note.pubkey());

        Some((ask_event, pubkey))
    }

    /// Get all threads across all projects, sorted by effective_last_activity descending
    /// Returns (Thread, project_a_tag) tuples
    #[deprecated(
        since = "0.1.0",
        note = "Use get_recent_threads_for_projects instead to avoid pre-filter truncation"
    )]
    pub fn get_all_recent_threads(&self, limit: usize) -> Vec<(Thread, String)> {
        let mut all_threads: Vec<(Thread, String)> = self
            .threads_by_project
            .iter()
            .flat_map(|(a_tag, threads)| threads.iter().map(|t| (t.clone(), a_tag.clone())))
            .collect();

        all_threads.sort_by(|a, b| {
            b.0.effective_last_activity
                .cmp(&a.0.effective_last_activity)
        });
        all_threads.truncate(limit);
        all_threads
    }

    /// Get recent threads for specific projects, sorted by effective_last_activity descending.
    /// Filters by project first (before any truncation), then applies optional time filter,
    /// then applies optional limit to final sorted results.
    /// Uses hierarchical sorting where parent conversations reflect the most recent activity
    /// in their entire delegation tree.
    /// Returns (Thread, project_a_tag) tuples.
    ///
    /// # Arguments
    /// * `visible_projects` - Set of project a_tags to include threads from
    /// * `time_cutoff` - Optional Unix timestamp; only threads with effective_last_activity >= cutoff are included
    /// * `limit` - Optional limit on the number of threads returned (applied AFTER sorting)
    pub fn get_recent_threads_for_projects(
        &self,
        visible_projects: &std::collections::HashSet<String>,
        time_cutoff: Option<u64>,
        limit: Option<usize>,
    ) -> Vec<(Thread, String)> {
        let mut threads: Vec<(Thread, String)> = self
            .threads_by_project
            .iter()
            // Filter by visible projects FIRST (before any collection)
            .filter(|(a_tag, _)| visible_projects.contains(a_tag.as_str()))
            .flat_map(|(a_tag, threads)| threads.iter().map(|t| (t.clone(), a_tag.clone())))
            // Apply time filter using effective_last_activity if specified
            .filter(|(thread, _)| match time_cutoff {
                Some(cutoff) => thread.effective_last_activity >= cutoff,
                None => true,
            })
            .collect();

        // Sort by 60-second bucketed effective_last_activity for stable ordering.
        // Within the same 60-second bucket, tie-break by event ID alphabetically ascending
        // to prevent conversations from jumping positions due to near-simultaneous activity.
        // This matches the bucketing logic applied in the FFI path (get_all_conversations).
        threads.sort_by(|a, b| {
            let a_bucket = a.0.effective_last_activity / 60;
            let b_bucket = b.0.effective_last_activity / 60;
            match b_bucket.cmp(&a_bucket) {
                std::cmp::Ordering::Equal => a.0.id.cmp(&b.0.id),
                other => other,
            }
        });

        // Apply optional limit AFTER sorting
        if let Some(max) = limit {
            threads.truncate(max);
        }

        threads
    }

    /// Get parent-child relationships from q-tags in messages.
    /// Returns a HashMap where key is parent thread ID and value is list of child conversation IDs.
    /// This is used as a fallback when the delegation tag is not present on child conversations.
    pub fn get_q_tag_relationships(&self) -> HashMap<String, Vec<String>> {
        let mut relationships: HashMap<String, Vec<String>> = HashMap::new();

        for (thread_id, messages) in &self.messages_by_thread {
            for message in messages {
                for child_conv_id in &message.q_tags {
                    relationships
                        .entry(thread_id.clone())
                        .or_default()
                        .push(child_conv_id.clone());
                }
            }
        }

        relationships
    }

    /// Find parent conversation ID for a thread by checking delegation tags in messages.
    /// This is a fallback for when the thread root doesn't have a delegation tag,
    /// but one of its messages does (e.g., the first message that was delegated).
    /// Returns the first delegation tag value found in any message of the thread.
    pub fn get_parent_conversation_from_messages(&self, thread_id: &str) -> Option<String> {
        if let Some(messages) = self.messages_by_thread.get(thread_id) {
            for message in messages {
                if let Some(ref parent_id) = message.delegation_tag {
                    // Don't return if the delegation tag points to the same thread
                    // (that would be self-referential)
                    if parent_id != thread_id {
                        return Some(parent_id.clone());
                    }
                }
            }
        }
        None
    }

    // ===== Inbox Methods =====

    /// Check if a note should be added to inbox and add it if so.
    /// Handles both Ask and Mention event types for any kind:1 that p-tags the user.
    pub fn check_and_add_inbox_item(&mut self, note: &Note, thread_id: &str, message: &Message) {
        let user_pk = match self.user_pubkey.clone() {
            Some(pk) => pk,
            None => return,
        };

        // Skip own messages
        if message.pubkey == user_pk {
            return;
        }

        // Must p-tag the current user
        if !self.note_ptags_user(note, &user_pk) {
            return;
        }

        let event_type = if self.note_is_ask_event(note) {
            InboxEventType::Ask
        } else {
            InboxEventType::Mention
        };

        let project_a_tag = Self::extract_project_a_tag(note)
            .or_else(|| self.find_project_for_thread(thread_id))
            .unwrap_or_default();

        let inbox_item = InboxItem {
            id: message.id.clone(),
            event_type,
            title: message.content.chars().take(50).collect(),
            content: message.content.clone(),
            project_a_tag,
            author_pubkey: message.pubkey.clone(),
            created_at: message.created_at,
            is_read: false,
            thread_id: Some(thread_id.to_string()),
            ask_event: message.ask_event.clone(),
        };
        self.inbox.add_item(inbox_item);
    }

    // ===== Agent Chatter Methods =====

    /// Add an agent chatter item, maintaining sort order and limiting to 100 items
    pub fn add_agent_chatter(&mut self, item: AgentChatter) {
        // Deduplicate by id
        if self.agent_chatter.iter().any(|i| i.id() == item.id()) {
            return;
        }

        // Insert sorted by created_at (most recent first)
        let pos = self
            .agent_chatter
            .partition_point(|i| i.created_at() > item.created_at());
        self.agent_chatter.insert(pos, item);

        // Limit to 100 items
        if self.agent_chatter.len() > 100 {
            self.agent_chatter.truncate(100);
        }
    }

    // ===== Lesson Methods =====

    fn handle_lesson_event(&mut self, note: &Note) {
        // Store lesson in content sub-store, get ref back for agent_chatter cross-cut
        if let Some(lesson) = self.content.insert_lesson(note) {
            let chatter = AgentChatter::Lesson {
                id: lesson.id.clone(),
                title: lesson.title.clone(),
                content: lesson.content.clone(),
                author_pubkey: lesson.pubkey.clone(),
                created_at: lesson.created_at,
                category: lesson.category.clone(),
            };
            self.add_agent_chatter(chatter);
        }
    }

    // ===== Operations Status Methods (kind:24133) =====

    pub fn get_statusbar_runtime_ms(&self) -> (u64, bool, usize) {
        let today_runtime_ms = self.statistics.get_today_unique_runtime();
        let unconfirmed_runtime_ms = self.operations.unconfirmed_runtime_secs() * 1000;
        let cumulative_runtime_ms = today_runtime_ms.saturating_add(unconfirmed_runtime_ms);
        let has_active_agents = self.operations.has_active_agents();
        let active_agent_count = self.operations.active_agent_count();
        (cumulative_runtime_ms, has_active_agents, active_agent_count)
    }

    /// Get thread info for an event ID (could be thread root or message within thread).
    /// Returns (thread_id, thread_title) if found.
    pub fn get_thread_info_for_event(&self, event_id: &str) -> Option<(String, String)> {
        // First check if event_id matches a thread root
        if let Some(thread) = self.get_thread_by_id(event_id) {
            return Some((thread.id.clone(), thread.title.clone()));
        }

        // Otherwise, search messages to find which thread contains this event
        for (thread_id, messages) in &self.messages_by_thread {
            if messages.iter().any(|m| m.id == event_id) {
                if let Some(thread) = self.get_thread_by_id(thread_id) {
                    return Some((thread.id.clone(), thread.title.clone()));
                }
            }
        }

        None
    }

    /// Get unanswered ask event for a thread (derived at check time).
    /// Looks at messages in the thread, finds any with q-tags pointing to ask events,
    /// and returns the first one that the current user hasn't replied to.
    pub fn get_unanswered_ask_for_thread(
        &self,
        thread_id: &str,
    ) -> Option<(String, AskEvent, String)> {
        let user_pubkey = self.user_pubkey.as_ref()?;
        let messages = self.messages_by_thread.get(thread_id)?;

        // Build set of message IDs the user has replied to (via e-tag)
        let mut user_replied_to: HashSet<String> = HashSet::new();
        for msg in messages {
            if msg.pubkey == *user_pubkey {
                if let Some(ref reply_to) = msg.reply_to {
                    user_replied_to.insert(reply_to.clone());
                }
            }
        }

        // Find messages with q-tags pointing to ask events
        for msg in messages {
            for q_tag in &msg.q_tags {
                // Skip if user has already replied to this ask
                if user_replied_to.contains(q_tag) {
                    continue;
                }
                // Look up the ask event and return if found
                if let Some((ask_event, author_pubkey)) = self.get_ask_event_by_id(q_tag) {
                    return Some((q_tag.clone(), ask_event, author_pubkey));
                }
            }
        }

        // Also check if the thread root itself is an ask event (direct viewing case)
        // If the thread root is an ask event and user hasn't replied, return it
        if let Some(thread) = self.get_thread_by_id(thread_id) {
            if thread.pubkey != *user_pubkey && !user_replied_to.contains(thread_id) {
                if let Some(ref ask_event) = thread.ask_event {
                    return Some((
                        thread_id.to_string(),
                        ask_event.clone(),
                        thread.pubkey.clone(),
                    ));
                }
            }
        }

        None
    }

    /// Check if a specific ask event has been answered by the current user.
    /// Searches all messages across all threads for a reply to the given message ID.
    pub fn is_ask_answered_by_user(&self, message_id: &str) -> bool {
        self.get_user_response_to_ask(message_id).is_some()
    }

    /// Get the user's response content to an ask event (if any).
    /// Searches all messages across all threads for a reply to the given message ID.
    pub fn get_user_response_to_ask(&self, message_id: &str) -> Option<String> {
        let user_pubkey = self.user_pubkey.as_ref()?;

        for messages in self.messages_by_thread.values() {
            for msg in messages {
                if msg.pubkey == *user_pubkey {
                    if let Some(ref reply_to) = msg.reply_to {
                        if reply_to == message_id {
                            return Some(msg.content.clone());
                        }
                    }
                }
            }
        }

        None
    }

    // ===== Text Search Methods =====

    /// Search content using nostrdb's fulltext search.
    /// Returns (event_id, thread_id, content, kind) for matching events.
    /// thread_id is extracted from e-tags (root marker) or is the event itself if it's a thread root.
    pub fn text_search(
        &self,
        query: &str,
        limit: i32,
    ) -> Vec<(String, Option<String>, String, u32)> {
        eprintln!(
            "[text_search] Starting search for query='{}', limit={}",
            query, limit
        );

        let Ok(txn) = Transaction::new(&self.ndb) else {
            eprintln!("[text_search] Failed to create transaction");
            return vec![];
        };

        // nostrdb only fulltext indexes kind:1 and kind:30023, so no need to filter
        let notes = match self.ndb.text_search(&txn, query, None, limit) {
            Ok(n) => {
                eprintln!("[text_search] ndb.text_search returned {} notes", n.len());
                n
            }
            Err(e) => {
                eprintln!("[text_search] ndb.text_search failed: {:?}", e);
                return vec![];
            }
        };
        notes
            .iter()
            .map(|note| {
                let event_id = hex::encode(note.id());
                let content = note.content().to_string();
                let kind = note.kind();

                // Find thread_id: look for e-tag with "root" marker, or use the event itself
                let thread_id = Self::extract_thread_id_from_note(note);

                (event_id, thread_id, content, kind)
            })
            .collect()
    }

    /// Get metadata for an event by ID.
    /// Returns (author_name, created_at, project_a_tag) if found.
    pub fn get_event_metadata(&self, event_id: &str) -> Option<(String, u64, Option<String>)> {
        let Ok(txn) = Transaction::new(&self.ndb) else {
            return None;
        };

        let Ok(id_bytes) = hex::decode(event_id) else {
            return None;
        };

        if id_bytes.len() != 32 {
            return None;
        }

        let mut id_arr = [0u8; 32];
        id_arr.copy_from_slice(&id_bytes);

        let Ok(note) = self.ndb.get_note_by_id(&txn, &id_arr) else {
            return None;
        };

        let pubkey = hex::encode(note.pubkey());
        let author_name = self.get_profile_name(&pubkey);
        let created_at = note.created_at();
        let project_a_tag = Self::extract_project_a_tag(&note);

        Some((author_name, created_at, project_a_tag))
    }

    /// Search for kind:1 messages sent by a specific pubkey.
    /// Returns (event_id, content, created_at, project_a_tag) sorted by recency.
    /// If query is empty, returns all messages from the pubkey (up to limit).
    /// If project_a_tag is Some, filters to only that project.
    ///
    /// For non-empty queries, uses NostrDB fulltext search to find candidate messages
    /// first (which also finds messages not yet in memory), then filters by user pubkey.
    /// For empty queries, falls back to in-memory scan of loaded messages.
    ///
    /// Uses the same search semantics as conversation search:
    /// - '+' operator splits query into multiple terms (AND semantics)
    /// - ASCII case-insensitive matching for consistency with highlighting
    pub fn search_user_messages(
        &self,
        user_pubkey: &str,
        query: &str,
        project_a_tag: Option<&str>,
        limit: usize,
    ) -> Vec<(String, String, u64, Option<String>)> {
        use crate::search::parse_search_terms;

        // Parse query into search terms (splits on '+', lowercases, trims)
        let terms = parse_search_terms(query);

        // For non-empty queries, use NostrDB fulltext search first to get candidates
        // This catches messages that may not be loaded into memory yet
        if !terms.is_empty() {
            return self.search_user_messages_with_db(user_pubkey, &terms, project_a_tag, limit);
        }

        // Empty query: fall back to in-memory scan (shows all recent messages)
        self.search_user_messages_in_memory(user_pubkey, &terms, project_a_tag, limit)
    }

    /// Search user messages using NostrDB fulltext search for each term,
    /// then intersect results and post-filter by user pubkey.
    fn search_user_messages_with_db(
        &self,
        user_pubkey: &str,
        terms: &[String],
        project_a_tag: Option<&str>,
        limit: usize,
    ) -> Vec<(String, String, u64, Option<String>)> {
        use crate::search::text_contains_term;
        use nostrdb::Transaction;
        use std::collections::HashMap;

        let Ok(txn) = Transaction::new(&self.ndb) else {
            // Fall back to in-memory if DB unavailable
            return self.search_user_messages_in_memory(user_pubkey, terms, project_a_tag, limit);
        };

        // Precompute thread_id -> project_a_tag mapping
        let thread_to_project: HashMap<&str, &str> = self
            .threads_by_project
            .iter()
            .flat_map(|(a_tag, threads)| {
                threads.iter().map(move |t| (t.id.as_str(), a_tag.as_str()))
            })
            .collect();

        // Search using the first term to get candidate notes
        // (text_search returns notes directly, which is more efficient)
        let db_limit = (limit * 10).min(1000) as i32;
        let notes = match self.ndb.text_search(&txn, &terms[0], None, db_limit) {
            Ok(notes) if !notes.is_empty() => notes,
            Ok(_empty) => {
                // NostrDB fulltext index may not be fully populated, so use in-memory
                // as a reliable fallback that searches already-loaded messages
                trace!(
                    query = %terms[0],
                    "NostrDB text_search returned empty results, falling back to in-memory search"
                );
                return self.search_user_messages_in_memory(
                    user_pubkey,
                    terms,
                    project_a_tag,
                    limit,
                );
            }
            Err(e) => {
                // Log the DB error and fall back to in-memory search
                warn!(
                    query = %terms[0],
                    error = %e,
                    "NostrDB text_search failed, falling back to in-memory search"
                );
                return self.search_user_messages_in_memory(
                    user_pubkey,
                    terms,
                    project_a_tag,
                    limit,
                );
            }
        };

        // Filter and process candidates directly from the search results
        let mut results: Vec<(String, String, u64, Option<String>)> = Vec::new();

        for note in notes.iter() {
            // Only kind:1 messages
            if note.kind() != 1 {
                continue;
            }

            // Filter by user pubkey
            let note_pubkey = hex::encode(note.pubkey());
            if note_pubkey != user_pubkey {
                continue;
            }

            let content = note.content().to_string();
            let event_id = hex::encode(note.id());

            // Post-filter: verify ALL terms match with our ASCII case-insensitive logic
            // (NostrDB search may use different matching semantics, and we need multi-term AND)
            let all_match = terms.iter().all(|term| text_contains_term(&content, term));
            if !all_match {
                continue;
            }

            // Get thread ID and project for filtering
            let thread_id = Self::extract_thread_id_from_note(note);
            let thread_project = thread_id
                .as_ref()
                .and_then(|tid| thread_to_project.get(tid.as_str()).copied());

            // Filter by project if specified
            if let Some(filter_a_tag) = project_a_tag {
                if thread_project != Some(filter_a_tag) {
                    continue;
                }
            }

            results.push((
                event_id,
                content,
                note.created_at(),
                thread_project.map(String::from),
            ));
        }

        // Sort by recency and apply limit
        results.sort_by(|a, b| b.2.cmp(&a.2));
        results.truncate(limit);
        results
    }

    /// Search user messages using in-memory scan (for empty queries)
    fn search_user_messages_in_memory(
        &self,
        user_pubkey: &str,
        terms: &[String],
        project_a_tag: Option<&str>,
        limit: usize,
    ) -> Vec<(String, String, u64, Option<String>)> {
        use crate::search::text_contains_term;
        use std::collections::{HashMap, HashSet};

        // Precompute thread_id -> project_a_tag mapping once
        let thread_to_project: HashMap<&str, &str> = self
            .threads_by_project
            .iter()
            .flat_map(|(a_tag, threads)| {
                threads.iter().map(move |t| (t.id.as_str(), a_tag.as_str()))
            })
            .collect();

        let mut results: Vec<(String, String, u64, Option<String>)> = Vec::new();
        let mut seen_ids: HashSet<String> = HashSet::new();

        // Phase 1: In-memory scan of loaded messages
        for (thread_id, messages) in &self.messages_by_thread {
            let thread_project_a_tag = thread_to_project.get(thread_id.as_str()).copied();

            if let Some(filter_a_tag) = project_a_tag {
                if thread_project_a_tag != Some(filter_a_tag) {
                    continue;
                }
            }

            for message in messages {
                if message.pubkey != user_pubkey {
                    continue;
                }

                if !terms.is_empty() {
                    let all_match = terms
                        .iter()
                        .all(|term| text_contains_term(&message.content, term));
                    if !all_match {
                        continue;
                    }
                }

                seen_ids.insert(message.id.clone());
                results.push((
                    message.id.clone(),
                    message.content.clone(),
                    message.created_at,
                    thread_project_a_tag.map(String::from),
                ));
            }
        }

        // Phase 2: Query nostrdb for cross-client kind:1 messages not in memory
        // This catches messages sent from other clients (web, iOS) that were
        // seeded via the user messages history subscription.
        if let Ok(pubkey_bytes) = hex::decode(user_pubkey)
            .ok()
            .and_then(|d| <[u8; 32]>::try_from(d).ok())
            .ok_or(())
        {
            if let Ok(txn) = nostrdb::Transaction::new(&self.ndb) {
                let db_limit = (limit * 5).min(500) as i32;
                let filter = nostrdb::Filter::new()
                    .kinds([1])
                    .authors([&pubkey_bytes])
                    .limit(db_limit as u64)
                    .build();
                if let Ok(query_results) = self.ndb.query(&txn, &[filter], db_limit) {
                    for result in query_results {
                        if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                            let event_id = hex::encode(note.id());
                            if seen_ids.contains(&event_id) {
                                continue;
                            }

                            let content = note.content().to_string();

                            if !terms.is_empty() {
                                let all_match = terms
                                    .iter()
                                    .all(|term| text_contains_term(&content, term));
                                if !all_match {
                                    continue;
                                }
                            }

                            let thread_id = Self::extract_thread_id_from_note(&note);
                            let thread_project = thread_id
                                .as_ref()
                                .and_then(|tid| thread_to_project.get(tid.as_str()).copied());

                            if let Some(filter_a_tag) = project_a_tag {
                                if thread_project != Some(filter_a_tag) {
                                    continue;
                                }
                            }

                            seen_ids.insert(event_id.clone());
                            results.push((
                                event_id,
                                content,
                                note.created_at(),
                                thread_project.map(String::from),
                            ));
                        }
                    }
                }
            }
        }

        // Sort by recency (newest first)
        results.sort_by(|a, b| b.2.cmp(&a.2));
        results.truncate(limit);
        results
    }

    /// Extract thread ID from a note's e-tags (looking for "root" marker per NIP-10)
    fn extract_thread_id_from_note(note: &nostrdb::Note) -> Option<String> {
        for tag in note.tags() {
            if tag.get(0).and_then(|t| t.variant().str()) == Some("e") {
                // Check for "root" marker in position 3
                let marker = tag.get(3).and_then(|t| t.variant().str());
                if marker == Some("root") {
                    // Get the event ID from position 1
                    if let Some(id) = tag.get(1).and_then(|t| t.variant().str()) {
                        return Some(id.to_string());
                    }
                    if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        return Some(hex::encode(id_bytes));
                    }
                }
            }
        }
        None
    }

    // ===== Backend Trust Methods =====

    // ===== Trust Methods =====

    pub fn add_approved_backend(&mut self, pubkey: &str) {
        let pending_statuses = self.trust.add_approved(pubkey);
        // Cross-cutting: apply pending statuses to project_statuses
        for mut status in pending_statuses {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            status.last_seen_at = now;
            self.project_statuses
                .insert(status.project_coordinate.clone(), status);
        }
    }

    pub fn add_blocked_backend(&mut self, pubkey: &str) {
        self.trust.add_blocked(pubkey);
    }

    pub fn drain_pending_backend_approvals(&mut self) -> Vec<PendingBackendApproval> {
        self.trust.drain_pending()
    }

    pub fn approve_pending_backends(&mut self, pending: Vec<PendingBackendApproval>) -> u32 {
        let mut approved_pubkeys: HashSet<String> = HashSet::new();

        for approval in pending {
            let mut status = approval.status;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            status.last_seen_at = now;
            self.project_statuses
                .insert(status.project_coordinate.clone(), status);
            approved_pubkeys.insert(approval.backend_pubkey);
        }

        for pubkey in approved_pubkeys.iter() {
            self.add_approved_backend(pubkey);
        }

        approved_pubkeys.len() as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::constants::COST_WINDOW_DAYS;
    use crate::store::Database;
    use tempfile::tempdir;

    /// Helper to create a test message with minimal required fields
    fn make_test_message(
        id: &str,
        pubkey: &str,
        thread_id: &str,
        content: &str,
        created_at: u64,
    ) -> Message {
        Message {
            id: id.to_string(),
            content: content.to_string(),
            pubkey: pubkey.to_string(),
            thread_id: thread_id.to_string(),
            created_at,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            a_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: HashMap::new(),
            delegation_tag: None,
            branch: None,
        }
    }

    #[test]
    fn test_handle_project_event_removes_tombstoned_project() {
        use crate::store::events::ingest_events;
        use nostr_sdk::prelude::*;
        use nostrdb::Transaction;

        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());
        let keys = Keys::generate();

        let live_event = EventBuilder::new(Kind::Custom(31933), "Live project")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["delete-me".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Delete Me".to_string()],
            ))
            .custom_created_at(Timestamp::from(1_700_000_100))
            .sign_with_keys(&keys)
            .unwrap();

        let tombstone_event = EventBuilder::new(Kind::Custom(31933), "")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["delete-me".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Delete Me".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deleted")),
                Vec::<String>::new(),
            ))
            .custom_created_at(Timestamp::from(1_700_000_200))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[live_event, tombstone_event], None).unwrap();

        let txn = Transaction::new(&db.ndb).unwrap();
        let filter = nostrdb::Filter::new().kinds([31933]).build();
        let mut notes: Vec<_> = db
            .ndb
            .query(&txn, &[filter], 100)
            .unwrap()
            .into_iter()
            .filter_map(|r| db.ndb.get_note_by_key(&txn, r.note_key).ok())
            .collect();
        notes.sort_by_key(|n| n.created_at());

        for note in notes {
            store.handle_project_event(&note);
        }

        assert!(store
            .projects
            .iter()
            .all(|p| p.id != "delete-me" && !p.is_deleted));
    }

    #[test]
    fn test_handle_project_event_ignores_older_live_after_tombstone() {
        use crate::store::events::{ingest_events, wait_for_event_processing};
        use nostr_sdk::prelude::*;
        use nostrdb::Transaction;

        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());
        let keys = Keys::generate();

        let live_event = EventBuilder::new(Kind::Custom(31933), "Live project")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["out-of-order-delete".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Out of Order Delete".to_string()],
            ))
            .custom_created_at(Timestamp::from(1_700_000_100))
            .sign_with_keys(&keys)
            .unwrap();

        let tombstone_event = EventBuilder::new(Kind::Custom(31933), "")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["out-of-order-delete".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Out of Order Delete".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deleted")),
                Vec::<String>::new(),
            ))
            .custom_created_at(Timestamp::from(1_700_000_200))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[live_event, tombstone_event], None).unwrap();
        let filter = nostrdb::Filter::new().kinds([31933]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let notes: Vec<_> = db
            .ndb
            .query(&txn, &[filter], 100)
            .unwrap()
            .into_iter()
            .filter_map(|r| db.ndb.get_note_by_key(&txn, r.note_key).ok())
            .collect();

        let live_note = notes
            .iter()
            .find(|n| n.created_at() == 1_700_000_100)
            .unwrap();
        let tombstone_note = notes
            .iter()
            .find(|n| n.created_at() == 1_700_000_200)
            .unwrap();

        // Simulate out-of-order delivery: tombstone first, then stale live.
        store.handle_project_event(tombstone_note);
        store.handle_project_event(live_note);

        assert!(
            store
                .projects
                .iter()
                .all(|p| p.id != "out-of-order-delete" && !p.is_deleted),
            "Older live project should not resurrect after tombstone"
        );
    }

    #[test]
    fn test_handle_project_event_prefers_tombstone_on_equal_timestamp() {
        use crate::store::events::{ingest_events, wait_for_event_processing};
        use nostr_sdk::prelude::*;
        use nostrdb::Transaction;

        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());
        let keys = Keys::generate();

        let live_event = EventBuilder::new(Kind::Custom(31933), "Live project")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["equal-ts-runtime-delete".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Equal TS Runtime Delete".to_string()],
            ))
            .custom_created_at(Timestamp::from(1_700_000_300))
            .sign_with_keys(&keys)
            .unwrap();

        let tombstone_event = EventBuilder::new(Kind::Custom(31933), "")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["equal-ts-runtime-delete".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Equal TS Runtime Delete".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deleted")),
                Vec::<String>::new(),
            ))
            .custom_created_at(Timestamp::from(1_700_000_300))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[live_event, tombstone_event], None).unwrap();
        let filter = nostrdb::Filter::new().kinds([31933]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let notes: Vec<_> = db
            .ndb
            .query(&txn, &[filter], 100)
            .unwrap()
            .into_iter()
            .filter_map(|r| db.ndb.get_note_by_key(&txn, r.note_key).ok())
            .collect();

        let has_deleted_tag = |note: &Note| {
            for tag in note.tags() {
                if tag.get(0).and_then(|t| t.variant().str()) == Some("deleted") {
                    return true;
                }
            }
            false
        };

        let live_note = notes.iter().find(|n| !has_deleted_tag(n)).unwrap();
        let tombstone_note = notes.iter().find(|n| has_deleted_tag(n)).unwrap();

        store.handle_project_event(live_note);
        store.handle_project_event(tombstone_note);

        assert!(
            store
                .projects
                .iter()
                .all(|p| p.id != "equal-ts-runtime-delete" && !p.is_deleted),
            "Tombstone should win equal-timestamp conflict"
        );
    }

    /// Regression test: Verify that when NostrDB text_search returns empty results,
    /// the in-memory fallback is used and correctly finds messages.
    ///
    /// This ensures the fix for message search stays intact - previously the code
    /// would silently fail when the fulltext index wasn't populated, returning no
    /// results even when messages existed in memory.
    #[test]
    fn test_search_user_messages_falls_back_to_in_memory_when_db_empty() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let user_pubkey = "abc123def456abc123def456abc123def456abc123def456abc123def456abc1";
        let thread_id = "thread123";

        // Add a message directly to in-memory store (simulating loaded messages)
        // This message won't be in NostrDB's fulltext index since we're adding it directly
        let message = make_test_message(
            "msg1",
            user_pubkey,
            thread_id,
            "This is a test message with searchable content about rust programming",
            1000,
        );

        store
            .messages_by_thread
            .entry(thread_id.to_string())
            .or_default()
            .push(message);

        // Search for a term that exists in the message
        // NostrDB's text_search will return empty (no indexed content),
        // so this should fall back to in-memory search
        let results = store.search_user_messages(user_pubkey, "rust", None, 10);

        // Verify the in-memory fallback found our message
        assert_eq!(
            results.len(),
            1,
            "Expected 1 result from in-memory fallback"
        );
        assert_eq!(
            results[0].0, "msg1",
            "Expected to find the message we added"
        );
        assert!(
            results[0].1.contains("rust programming"),
            "Content should match the message we added"
        );
    }

    /// Test that multi-term search (using '+' operator) works with in-memory fallback
    #[test]
    fn test_search_user_messages_multi_term_in_memory_fallback() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let user_pubkey = "abc123def456abc123def456abc123def456abc123def456abc123def456abc1";
        let thread_id = "thread123";

        // Add messages - one with both terms, one with only one term
        let message1 = make_test_message(
            "msg1",
            user_pubkey,
            thread_id,
            "Error occurred with timeout after 5 seconds",
            1000,
        );
        let message2 = make_test_message(
            "msg2",
            user_pubkey,
            thread_id,
            "Error occurred while processing request", // has "error" but NOT "timeout"
            900,                                       // older
        );

        store
            .messages_by_thread
            .entry(thread_id.to_string())
            .or_default()
            .extend(vec![message1, message2]);

        // Search for messages containing both "error" AND "timeout"
        // Only msg1 should match (msg2 has error but not timeout)
        let results = store.search_user_messages(user_pubkey, "error+timeout", None, 10);

        assert_eq!(results.len(), 1, "Expected 1 result matching both terms");
        assert_eq!(results[0].0, "msg1", "Only msg1 has both error and timeout");
    }

    /// Test that empty query returns all messages via in-memory scan
    #[test]
    fn test_search_user_messages_empty_query_returns_all() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let user_pubkey = "abc123def456abc123def456abc123def456abc123def456abc123def456abc1";
        let thread_id = "thread123";

        // Add multiple messages
        for i in 0..3 {
            let message = make_test_message(
                &format!("msg{}", i),
                user_pubkey,
                thread_id,
                &format!("Message number {}", i),
                1000 + i as u64,
            );
            store
                .messages_by_thread
                .entry(thread_id.to_string())
                .or_default()
                .push(message);
        }

        // Empty query should return all messages
        let results = store.search_user_messages(user_pubkey, "", None, 10);

        assert_eq!(results.len(), 3, "Empty query should return all 3 messages");
    }

    #[test]
    fn test_get_profile_name_falls_back_to_project_status_slug() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let pubkey = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
        let slug = "architect-orchestrator";

        let status = ProjectStatus {
            project_coordinate: "31933:deadbeef:example".to_string(),
            agents: vec![ProjectAgent {
                pubkey: pubkey.to_string(),
                name: slug.to_string(),
                is_pm: true,
                model: None,
                tools: vec![],
            }],
            branches: vec![],
            all_models: vec![],
            all_tools: vec![],
            created_at: 0,
            backend_pubkey: "backend".to_string(),
            last_seen_at: 0,
        };

        store
            .project_statuses
            .insert(status.project_coordinate.clone(), status);

        let name = store.get_profile_name(pubkey);
        assert_eq!(name, slug);
    }

    // ===== Agent Tracking Integration Tests =====

    /// Test handle_status_event_json parses and routes 24133 events correctly
    #[test]
    fn test_handle_status_event_json_24133() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Valid 24133 event JSON
        let json = r#"{
            "kind": 24133,
            "id": "abc123",
            "created_at": 1000,
            "tags": [
                ["e", "conv123"],
                ["p", "agent1"],
                ["p", "agent2"],
                ["a", "31933:user:project"]
            ]
        }"#;

        // Should not panic and should update state
        store.handle_status_event_json(json);

        // Verify agent tracking state was updated
        assert!(store.operations.has_active_agents());
        assert_eq!(store.operations.active_agent_count(), 2);
    }

    /// Test handle_status_event_json ignores malformed JSON
    #[test]
    fn test_handle_status_event_json_malformed() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Malformed JSON should not panic
        store.handle_status_event_json("{ invalid json }");
        store.handle_status_event_json("");
        store.handle_status_event_json("null");

        // State should remain empty
        assert!(!store.operations.has_active_agents());
    }

    /// Test handle_status_event_json ignores unknown kinds
    #[test]
    fn test_handle_status_event_json_unknown_kind() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Event with unknown kind (not 24010 or 24133)
        let json = r#"{
            "kind": 12345,
            "id": "xyz",
            "created_at": 1000,
            "tags": []
        }"#;

        store.handle_status_event_json(json);

        // Should not affect any state
        assert!(!store.operations.has_active_agents());
    }

    /// Test upsert_operations_status updates agent tracking with deduplication
    #[test]
    fn test_upsert_operations_status_runtime_deduplication() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // First 24133 event with llm-runtime (agent finishes work - empty p-tags)
        // Using empty p-tags to test only confirmed runtime (no active agents to inflate total)
        let json1 = r#"{
            "kind": 24133,
            "id": "event1",
            "created_at": 1000,
            "tags": [
                ["e", "conv123"],
                ["a", "31933:user:project"],
                ["llm-runtime", "100"]
            ]
        }"#;

        store.handle_status_event_json(json1);
        // Use confirmed_runtime_secs to isolate from unconfirmed runtime
        assert_eq!(store.operations.confirmed_runtime_secs(), 100);

        // Same event again (simulating replay) - should NOT double-count
        store.handle_status_event_json(json1);
        assert_eq!(store.operations.confirmed_runtime_secs(), 100); // Still 100, not 200

        // Different event with runtime - should add
        let json2 = r#"{
            "kind": 24133,
            "id": "event2",
            "created_at": 1001,
            "tags": [
                ["e", "conv123"],
                ["a", "31933:user:project"],
                ["llm-runtime", "50"]
            ]
        }"#;

        store.handle_status_event_json(json2);
        assert_eq!(store.operations.confirmed_runtime_secs(), 150); // 100 + 50
    }

    /// Test upsert_operations_status handles same-second events with different event IDs
    #[test]
    fn test_upsert_operations_status_same_second_ordering() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // First event at t=1000
        let json1 = r#"{
            "kind": 24133,
            "id": "aaa",
            "created_at": 1000,
            "tags": [
                ["e", "conv123"],
                ["p", "agent1"],
                ["a", "31933:user:project"]
            ]
        }"#;

        store.handle_status_event_json(json1);
        assert_eq!(store.operations.active_agent_count(), 1);

        // Second event at same timestamp with different (larger) event_id
        let json2 = r#"{
            "kind": 24133,
            "id": "bbb",
            "created_at": 1000,
            "tags": [
                ["e", "conv123"],
                ["p", "agent2"],
                ["a", "31933:user:project"]
            ]
        }"#;

        store.handle_status_event_json(json2);
        // Should accept the second event (bbb > aaa lexicographically)
        assert_eq!(store.operations.active_agent_count(), 1);
    }

    /// Test upsert_operations_status uses thread_id for conversation tracking
    #[test]
    fn test_upsert_operations_status_thread_id_tracking() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Event with q-tag (thread_id)
        let json = r#"{
            "kind": 24133,
            "id": "event1",
            "created_at": 1000,
            "tags": [
                ["e", "message123"],
                ["q", "thread_root"],
                ["p", "agent1"],
                ["a", "31933:user:project"]
            ]
        }"#;

        store.handle_status_event_json(json);

        // Second event for same thread but different message
        let json2 = r#"{
            "kind": 24133,
            "id": "event2",
            "created_at": 1001,
            "tags": [
                ["e", "message456"],
                ["q", "thread_root"],
                ["p", "agent2"],
                ["a", "31933:user:project"]
            ]
        }"#;

        store.handle_status_event_json(json2);

        // Should replace agent1 with agent2 (same conversation via thread_id)
        assert_eq!(store.operations.active_agent_count(), 1);
    }

    /// Test empty p-tags removes agents from conversation
    #[test]
    fn test_upsert_operations_status_empty_p_tags() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Add agent
        let json1 = r#"{
            "kind": 24133,
            "id": "event1",
            "created_at": 1000,
            "tags": [
                ["e", "conv123"],
                ["p", "agent1"],
                ["a", "31933:user:project"]
            ]
        }"#;

        store.handle_status_event_json(json1);
        assert!(store.operations.has_active_agents());

        // Remove all agents with empty p-tags
        let json2 = r#"{
            "kind": 24133,
            "id": "event2",
            "created_at": 1001,
            "tags": [
                ["e", "conv123"],
                ["a", "31933:user:project"]
            ]
        }"#;

        store.handle_status_event_json(json2);
        assert!(!store.operations.has_active_agents());
    }

    /// Integration test for authoritative replacement semantics.
    ///
    /// This test verifies that when two 24133 events are sent for the SAME conversation
    /// with DIFFERENT agent lists, the second event's agents completely REPLACE the first.
    /// This is the critical "authoritative per-conversation contract" described in the
    /// agent_tracking.rs documentation.
    ///
    /// The test checks actual agent identities (not just counts) to ensure proper replacement.
    #[test]
    fn test_authoritative_replacement_integration() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // PHASE 1: Initial event with agents [alpha, beta, gamma]
        let json1 = r#"{
            "kind": 24133,
            "id": "event001",
            "created_at": 1000,
            "tags": [
                ["e", "conversation_xyz"],
                ["p", "agent_alpha"],
                ["p", "agent_beta"],
                ["p", "agent_gamma"],
                ["a", "31933:owner:test-project"]
            ]
        }"#;

        store.handle_status_event_json(json1);

        // Verify initial state: 3 agents
        assert_eq!(store.operations.active_agent_count(), 3);
        let agents1 = store
            .operations
            .get_active_agents_for_conversation("conversation_xyz");
        assert_eq!(agents1.len(), 3);
        assert!(agents1.contains(&"agent_alpha".to_string()));
        assert!(agents1.contains(&"agent_beta".to_string()));
        assert!(agents1.contains(&"agent_gamma".to_string()));

        // PHASE 2: Second event for SAME conversation with DIFFERENT agents [delta, epsilon]
        // This should COMPLETELY REPLACE the previous agent list (authoritative semantics)
        let json2 = r#"{
            "kind": 24133,
            "id": "event002",
            "created_at": 1001,
            "tags": [
                ["e", "conversation_xyz"],
                ["p", "agent_delta"],
                ["p", "agent_epsilon"],
                ["a", "31933:owner:test-project"]
            ]
        }"#;

        store.handle_status_event_json(json2);

        // Verify replacement: now only 2 agents (delta, epsilon)
        assert_eq!(store.operations.active_agent_count(), 2);
        let agents2 = store
            .operations
            .get_active_agents_for_conversation("conversation_xyz");
        assert_eq!(
            agents2.len(),
            2,
            "Expected 2 agents after replacement, got {}",
            agents2.len()
        );

        // CRITICAL: Original agents should be GONE
        assert!(
            !agents2.contains(&"agent_alpha".to_string()),
            "agent_alpha should have been replaced"
        );
        assert!(
            !agents2.contains(&"agent_beta".to_string()),
            "agent_beta should have been replaced"
        );
        assert!(
            !agents2.contains(&"agent_gamma".to_string()),
            "agent_gamma should have been replaced"
        );

        // CRITICAL: New agents should be present
        assert!(
            agents2.contains(&"agent_delta".to_string()),
            "agent_delta should be active"
        );
        assert!(
            agents2.contains(&"agent_epsilon".to_string()),
            "agent_epsilon should be active"
        );

        // PHASE 3: Verify other conversations are NOT affected
        // Add agents to a different conversation
        let json3 = r#"{
            "kind": 24133,
            "id": "event003",
            "created_at": 1002,
            "tags": [
                ["e", "conversation_other"],
                ["p", "agent_omega"],
                ["a", "31933:owner:test-project"]
            ]
        }"#;

        store.handle_status_event_json(json3);

        // conversation_xyz should still have delta, epsilon
        let agents_xyz = store
            .operations
            .get_active_agents_for_conversation("conversation_xyz");
        assert_eq!(agents_xyz.len(), 2);
        assert!(agents_xyz.contains(&"agent_delta".to_string()));
        assert!(agents_xyz.contains(&"agent_epsilon".to_string()));

        // conversation_other should have omega
        let agents_other = store
            .operations
            .get_active_agents_for_conversation("conversation_other");
        assert_eq!(agents_other.len(), 1);
        assert!(agents_other.contains(&"agent_omega".to_string()));

        // Total count should be 3 (2 from xyz + 1 from other)
        assert_eq!(store.operations.active_agent_count(), 3);
    }

    mod timer_tests {
        use super::*;
        use nostr_sdk::prelude::*;

        /// Integration test: Verify that kind:1 messages with llm-runtime tags
        /// trigger unconfirmed timer resets via reset_unconfirmed_timer
        #[test]
        fn test_kind1_message_resets_unconfirmed_timer() {
            use std::thread;
            use std::time::Duration;

            let dir = tempdir().unwrap();
            let db = Database::new(dir.path()).unwrap();
            let mut store = AppDataStore::new(db.ndb.clone());

            let keys = Keys::generate();
            let agent_pubkey = keys.public_key().to_hex();

            // Simulate agent starting work on a conversation
            store.operations.agent_tracking.process_24133_event(
                "conv1",
                "event1",
                std::slice::from_ref(&agent_pubkey),
                1000,
                "31933:user:project",
                None,
            );

            // Wait to accumulate unconfirmed runtime
            thread::sleep(Duration::from_millis(1100));

            let runtime_before_reset = store.operations.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_before_reset >= 1,
                "Expected unconfirmed runtime >= 1 second before reset, got {}",
                runtime_before_reset
            );

            // Simulate a kind:1 message with llm-runtime tag arriving
            // (call reset_unconfirmed_timer directly as handle_message_event would)
            store
                .operations
                .agent_tracking
                .reset_unconfirmed_timer("conv1", &agent_pubkey, 1100);

            // Verify that unconfirmed runtime was reset (should be near 0)
            let runtime_after_reset = store.operations.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_after_reset < 1,
                "Expected unconfirmed runtime < 1 second after llm-runtime reset, got {}",
                runtime_after_reset
            );

            // Wait again to accumulate more unconfirmed runtime
            thread::sleep(Duration::from_millis(1100));

            let runtime_before_non_reset =
                store.operations.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_before_non_reset >= 1,
                "Expected unconfirmed runtime >= 1 second before testing no reset, got {}",
                runtime_before_non_reset
            );

            // Simulate a kind:1 message WITHOUT llm-runtime tag (don't call reset_unconfirmed_timer)
            // Runtime should continue accumulating

            // Verify that unconfirmed runtime was NOT reset (should still be >= 1)
            let runtime_after_non_reset =
                store.operations.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_after_non_reset >= 1,
                "Expected unconfirmed runtime >= 1 second when no reset happens, got {}",
                runtime_after_non_reset
            );

            // Test recency guard: stale message should not reset timer
            thread::sleep(Duration::from_millis(500));
            let runtime_before_stale = store.operations.agent_tracking.unconfirmed_runtime_secs();

            // Try to reset with a stale timestamp (older than last reset at 1100)
            store
                .operations
                .agent_tracking
                .reset_unconfirmed_timer("conv1", &agent_pubkey, 1050);

            // Runtime should NOT have been reset (blocked by recency guard)
            let runtime_after_stale = store.operations.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_after_stale >= runtime_before_stale,
                "Stale message should not reset timer: before {}, after {}",
                runtime_before_stale,
                runtime_after_stale
            );

            // Reset with a newer timestamp should work
            store
                .operations
                .agent_tracking
                .reset_unconfirmed_timer("conv1", &agent_pubkey, 1200);
            let runtime_after_newer = store.operations.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_after_newer < 1,
                "Newer message should reset timer, got {}",
                runtime_after_newer
            );
        }
    }

    // ===== Runtime Calculation Tests =====

    /// Helper to create a test message with llm runtime metadata
    fn make_message_with_runtime(
        id: &str,
        pubkey: &str,
        thread_id: &str,
        created_at: u64,
        runtime_ms: u64,
    ) -> Message {
        Message {
            id: id.to_string(),
            content: "test".to_string(),
            pubkey: pubkey.to_string(),
            thread_id: thread_id.to_string(),
            created_at,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            a_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: HashMap::from([("runtime".to_string(), runtime_ms.to_string())]),
            delegation_tag: None,
            branch: None,
        }
    }

    #[test]
    fn test_calculate_runtime_from_messages_filters_pre_cutoff() {
        // Test that messages before RUNTIME_CUTOFF_TIMESTAMP are excluded
        let pre_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 86400; // 1 day before cutoff
        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 86400; // 1 day after cutoff

        let messages = vec![
            make_message_with_runtime("msg1", "pubkey1", "thread1", pre_cutoff, 100000), // Should be filtered out
            make_message_with_runtime("msg2", "pubkey1", "thread1", post_cutoff, 50), // Should be included (50ms)
        ];

        let runtime_ms = AppDataStore::calculate_runtime_from_messages(&messages);

        // Only the post-cutoff message should be counted: 50 milliseconds
        assert_eq!(
            runtime_ms, 50,
            "Only post-cutoff messages should be counted"
        );
    }

    #[test]
    fn test_calculate_runtime_from_messages_at_cutoff_boundary() {
        // Test that messages exactly at the cutoff are included
        let at_cutoff = RUNTIME_CUTOFF_TIMESTAMP;
        let before_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 1;

        let messages = vec![
            make_message_with_runtime("msg1", "pubkey1", "thread1", before_cutoff, 100000), // Should be filtered out
            make_message_with_runtime("msg2", "pubkey1", "thread1", at_cutoff, 50), // Should be included (50ms)
        ];

        let runtime_ms = AppDataStore::calculate_runtime_from_messages(&messages);

        // Only the message at cutoff should be counted: 50 milliseconds
        assert_eq!(runtime_ms, 50, "Messages at cutoff should be included");
    }

    #[test]
    fn test_calculate_runtime_from_messages_sums_milliseconds() {
        // Test that llm-runtime values (already in milliseconds) are summed correctly
        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 86400;

        let messages = vec![
            make_message_with_runtime("msg1", "pubkey1", "thread1", post_cutoff, 5000), // 5 seconds = 5000ms
            make_message_with_runtime("msg2", "pubkey1", "thread1", post_cutoff, 10000), // 10 seconds = 10000ms
            make_message_with_runtime("msg3", "pubkey1", "thread1", post_cutoff, 120000), // 2 minutes = 120000ms
        ];

        let runtime_ms = AppDataStore::calculate_runtime_from_messages(&messages);

        // Total: (5000 + 10000 + 120000) milliseconds = 135000 milliseconds
        assert_eq!(runtime_ms, 135_000, "Should sum milliseconds correctly");
    }

    #[test]
    fn test_calculate_runtime_from_messages_empty_list() {
        let messages: Vec<Message> = vec![];
        let runtime_ms = AppDataStore::calculate_runtime_from_messages(&messages);
        assert_eq!(runtime_ms, 0, "Empty message list should return 0");
    }

    #[test]
    fn test_calculate_runtime_from_messages_no_runtime_metadata() {
        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 86400;

        // Message without runtime metadata
        let message = make_test_message("msg1", "pubkey1", "thread1", "content", post_cutoff);
        let messages = vec![message];

        let runtime_ms = AppDataStore::calculate_runtime_from_messages(&messages);
        assert_eq!(
            runtime_ms, 0,
            "Messages without runtime metadata should return 0"
        );
    }

    #[test]
    fn test_calculate_runtime_from_messages_mixed_pre_and_post_cutoff() {
        // Comprehensive test with multiple messages spanning the cutoff
        let pre_cutoff_1 = RUNTIME_CUTOFF_TIMESTAMP - 100_000;
        let pre_cutoff_2 = RUNTIME_CUTOFF_TIMESTAMP - 1;
        let at_cutoff = RUNTIME_CUTOFF_TIMESTAMP;
        let post_cutoff_1 = RUNTIME_CUTOFF_TIMESTAMP + 1;
        let post_cutoff_2 = RUNTIME_CUTOFF_TIMESTAMP + 100_000;

        let messages = vec![
            make_message_with_runtime("msg1", "pubkey1", "thread1", pre_cutoff_1, 1000000), // Excluded
            make_message_with_runtime("msg2", "pubkey1", "thread1", pre_cutoff_2, 500000), // Excluded
            make_message_with_runtime("msg3", "pubkey1", "thread1", at_cutoff, 10), // Included (10ms)
            make_message_with_runtime("msg4", "pubkey1", "thread1", post_cutoff_1, 20), // Included (20ms)
            make_message_with_runtime("msg5", "pubkey1", "thread1", post_cutoff_2, 30), // Included (30ms)
        ];

        let runtime_ms = AppDataStore::calculate_runtime_from_messages(&messages);

        // Only msg3, msg4, msg5 should be counted: (10 + 20 + 30) = 60 milliseconds
        assert_eq!(
            runtime_ms, 60,
            "Should only count messages at or after cutoff"
        );
    }

    #[test]
    fn test_runtime_by_day_counts_use_message_timestamps() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let seconds_per_day: u64 = 86400;
        let today_start = (now / seconds_per_day) * seconds_per_day;
        let yesterday_start = today_start.saturating_sub(seconds_per_day);

        let messages = vec![
            make_message_with_runtime("msg1", "pubkey1", "thread1", yesterday_start + 60, 1000),
            make_message_with_runtime("msg2", "pubkey1", "thread1", today_start + 120, 2000),
        ];

        store
            .messages_by_thread
            .insert("thread1".to_string(), messages);
        store
            .statistics
            .rebuild_runtime_by_day_counts(&store.messages_by_thread);

        assert_eq!(store.get_today_unique_runtime(), 2000);
    }

    #[test]
    fn test_end_to_end_runtime_flow_with_cutoff() {
        // End-to-end test: llm metadata (ms) → calculate_runtime (ms) → RuntimeHierarchy → stats
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let pre_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 86400;
        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 86400;

        // Thread 1: Messages before cutoff (should be filtered out)
        let thread1_messages = vec![
            make_message_with_runtime("msg1", "pubkey1", "thread1", pre_cutoff, 100000),
            make_message_with_runtime("msg2", "pubkey1", "thread1", pre_cutoff, 200000),
        ];

        // Thread 2: Messages after cutoff (should be included)
        let thread2_messages = vec![
            make_message_with_runtime("msg3", "pubkey1", "thread2", post_cutoff, 50),
            make_message_with_runtime("msg4", "pubkey1", "thread2", post_cutoff, 75),
        ];

        // Add messages to store
        store
            .messages_by_thread
            .insert("thread1".to_string(), thread1_messages);
        store
            .messages_by_thread
            .insert("thread2".to_string(), thread2_messages);

        // Update runtime hierarchy (simulating what happens in handle_message_event)
        store.update_runtime_hierarchy_for_thread_id("thread1");
        store.update_runtime_hierarchy_for_thread_id("thread2");

        // Verify thread1 runtime is calculated but filtered out in stats
        let thread1_individual = store.runtime_hierarchy.get_individual_runtime("thread1");
        // calculate_runtime_from_messages filters at message level, so thread1 should have 0
        assert_eq!(
            thread1_individual, 0,
            "Thread1 should have 0 runtime (pre-cutoff messages filtered)"
        );

        // Verify thread2 runtime is calculated correctly
        let thread2_individual = store.runtime_hierarchy.get_individual_runtime("thread2");
        // (50 + 75) milliseconds = 125 milliseconds
        assert_eq!(
            thread2_individual, 125,
            "Thread2 should have correct runtime in milliseconds"
        );

        // Verify total unique runtime only includes thread2
        let total = store.runtime_hierarchy.get_total_unique_runtime();
        assert_eq!(
            total, 125,
            "Total should only include post-cutoff conversations"
        );

        // Verify top conversations includes only thread2
        let top = store.runtime_hierarchy.get_top_conversations_by_runtime(10);
        assert_eq!(top.len(), 1, "Should only have 1 conversation in top list");
        assert_eq!(top[0].0, "thread2");
        assert_eq!(top[0].1, 125);
    }

    #[test]
    fn test_end_to_end_hierarchical_runtime_with_cutoff() {
        // Test hierarchical runtime with parent-child relationships across cutoff
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let pre_cutoff = RUNTIME_CUTOFF_TIMESTAMP - 1;
        let post_cutoff = RUNTIME_CUTOFF_TIMESTAMP + 1;

        // Parent conversation: created before cutoff, with q-tag pointing to child
        let mut parent_msg =
            make_message_with_runtime("msg1", "pubkey1", "parent", pre_cutoff, 100000);
        parent_msg.q_tags.push("child".to_string());
        let parent_messages = vec![parent_msg];

        // Child conversation: created after cutoff
        let child_messages = vec![make_message_with_runtime(
            "msg2",
            "pubkey1",
            "child",
            post_cutoff,
            50,
        )];

        store
            .messages_by_thread
            .insert("parent".to_string(), parent_messages);
        store
            .messages_by_thread
            .insert("child".to_string(), child_messages);

        // Update runtime hierarchy
        store.update_runtime_hierarchy_for_thread_id("parent");
        store.update_runtime_hierarchy_for_thread_id("child");

        // Parent should have 0 runtime (pre-cutoff messages filtered)
        let parent_runtime = store.runtime_hierarchy.get_individual_runtime("parent");
        assert_eq!(parent_runtime, 0);

        // Child should have 50 milliseconds
        let child_runtime = store.runtime_hierarchy.get_individual_runtime("child");
        assert_eq!(child_runtime, 50);

        // Unfiltered total for parent includes child
        let parent_total_unfiltered = store.runtime_hierarchy.get_total_runtime("parent");
        assert_eq!(parent_total_unfiltered, 50);

        // Top conversations should show parent with only child's runtime (filtered)
        let top = store.runtime_hierarchy.get_top_conversations_by_runtime(10);
        assert_eq!(top.len(), 1);
        assert_eq!(top[0].0, "parent");
        assert_eq!(
            top[0].1, 50,
            "Parent's filtered total should only include child"
        );
    }

    /// Test UTC day/hour boundary bucketing works correctly for LLM activity.
    /// Verifies that messages on different sides of UTC day boundaries are bucketed separately.
    #[test]
    fn test_llm_activity_utc_day_hour_bucketing() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Test timestamps around UTC day boundary
        // Day 1: 2024-01-15 23:30:00 UTC (timestamp: 1705361400)
        // Day 2: 2024-01-16 01:30:00 UTC (timestamp: 1705368600) - crosses midnight boundary
        let day1_timestamp = 1705361400_u64; // 23:30:00 UTC on day 1
        let day2_timestamp = 1705368600_u64; // 01:30:00 UTC on day 2 (7200 seconds later)

        // Calculate expected day_start values
        let seconds_per_day: u64 = 86400;
        let day1_start = (day1_timestamp / seconds_per_day) * seconds_per_day;
        let day2_start = (day2_timestamp / seconds_per_day) * seconds_per_day;

        // Verify different day boundaries
        assert_ne!(
            day1_start, day2_start,
            "Timestamps should be in different UTC days"
        );

        // Create messages with LLM metadata on different days
        let mut msg1 = make_test_message("msg1", "pubkey1", "thread1", "test", day1_timestamp);
        msg1.llm_metadata = HashMap::from([("total-tokens".to_string(), "100".to_string())]);

        let mut msg2 = make_test_message("msg2", "pubkey1", "thread1", "test", day2_timestamp);
        msg2.llm_metadata = HashMap::from([("total-tokens".to_string(), "200".to_string())]);

        store
            .messages_by_thread
            .insert("thread1".to_string(), vec![msg1, msg2]);
        store
            .statistics
            .rebuild_llm_activity_by_hour(&store.messages_by_thread);

        // Verify both buckets exist with correct values
        assert_eq!(
            store.statistics.llm_activity_by_hour.len(),
            2,
            "Should have 2 separate hour buckets"
        );

        // Day 1: hour 23
        let key1 = (day1_start, 23_u8);
        assert_eq!(
            store.statistics.llm_activity_by_hour.get(&key1),
            Some(&(100, 1)),
            "Day 1 hour 23 should have 100 tokens, 1 message"
        );

        // Day 2: hour 1
        let key2 = (day2_start, 1_u8);
        assert_eq!(
            store.statistics.llm_activity_by_hour.get(&key2),
            Some(&(200, 1)),
            "Day 2 hour 1 should have 200 tokens, 1 message"
        );
    }

    /// Test that only LLM messages (those with llm_metadata) are counted in activity tracking.
    /// Non-LLM messages should be completely ignored.
    #[test]
    fn test_llm_activity_only_counts_llm_messages() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let timestamp = 1705363800_u64;

        // Message WITHOUT llm_metadata (user message)
        let user_msg = make_test_message("msg1", "pubkey1", "thread1", "user message", timestamp);

        // Message WITH llm_metadata (LLM response)
        let mut llm_msg =
            make_test_message("msg2", "pubkey1", "thread1", "LLM response", timestamp);
        llm_msg.llm_metadata = HashMap::from([("total-tokens".to_string(), "150".to_string())]);

        // Message WITH empty llm_metadata (should NOT be counted)
        let mut empty_metadata_msg =
            make_test_message("msg3", "pubkey1", "thread1", "empty metadata", timestamp);
        empty_metadata_msg.llm_metadata = HashMap::new();

        store.messages_by_thread.insert(
            "thread1".to_string(),
            vec![user_msg, llm_msg, empty_metadata_msg],
        );
        store
            .statistics
            .rebuild_llm_activity_by_hour(&store.messages_by_thread);

        // Should only have 1 bucket for the LLM message
        assert_eq!(
            store.statistics.llm_activity_by_hour.len(),
            1,
            "Should only count LLM messages"
        );

        // Verify the bucket contains only the LLM message data
        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;
        let day_start = (timestamp / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = timestamp - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;
        let key = (day_start, hour_of_day);

        assert_eq!(
            store.statistics.llm_activity_by_hour.get(&key),
            Some(&(150, 1)),
            "Should only have 1 LLM message with 150 tokens"
        );
    }

    /// Test that token counts are correctly parsed and aggregated from llm_metadata.
    /// Verifies both successful parsing and fallback to 0 for missing/invalid values.
    #[test]
    fn test_llm_activity_token_parsing() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let timestamp = 1705363800_u64;

        // Message with valid token count
        let mut msg1 = make_test_message("msg1", "pubkey1", "thread1", "test", timestamp);
        msg1.llm_metadata = HashMap::from([("total-tokens".to_string(), "500".to_string())]);

        // Message with missing total-tokens (should default to 0)
        let mut msg2 = make_test_message("msg2", "pubkey1", "thread1", "test", timestamp);
        msg2.llm_metadata = HashMap::from([("other-key".to_string(), "value".to_string())]);

        // Message with invalid token count (should default to 0)
        let mut msg3 = make_test_message("msg3", "pubkey1", "thread1", "test", timestamp);
        msg3.llm_metadata = HashMap::from([("total-tokens".to_string(), "invalid".to_string())]);

        store
            .messages_by_thread
            .insert("thread1".to_string(), vec![msg1, msg2, msg3]);
        store
            .statistics
            .rebuild_llm_activity_by_hour(&store.messages_by_thread);

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;
        let day_start = (timestamp / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = timestamp - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;
        let key = (day_start, hour_of_day);

        // All 3 messages counted, but only msg1 has valid tokens (500 + 0 + 0 = 500)
        assert_eq!(
            store.statistics.llm_activity_by_hour.get(&key),
            Some(&(500, 3)),
            "Should have 500 total tokens and 3 messages"
        );
    }

    /// Test window slicing logic in get_tokens_by_hour and get_message_count_by_hour.
    /// Verifies that only messages within the requested time window are returned,
    /// and that the O(num_hours) implementation correctly uses direct HashMap lookups.
    #[test]
    fn test_llm_activity_window_slicing() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Create messages at specific hour boundaries for deterministic testing
        // Use timestamps that are exact hour boundaries for easier verification
        let seconds_per_hour: u64 = 3600;

        // Base timestamp: 2024-01-15 10:00:00 UTC
        let base_timestamp = 1705316400_u64;

        // Create messages at hour boundaries: 10:00, 11:00, 12:00, 13:00, 14:00
        for i in 0..5 {
            let timestamp = base_timestamp + (i * seconds_per_hour);
            let mut msg = make_test_message(
                &format!("msg{}", i),
                "pubkey1",
                "thread1",
                "test",
                timestamp,
            );
            msg.llm_metadata =
                HashMap::from([("total-tokens".to_string(), format!("{}", (i + 1) * 100))]);

            store
                .messages_by_thread
                .entry("thread1".to_string())
                .or_default()
                .push(msg);
        }

        store
            .statistics
            .rebuild_llm_activity_by_hour(&store.messages_by_thread);

        // Verify all 5 buckets were created
        assert_eq!(
            store.statistics.llm_activity_by_hour.len(),
            5,
            "Should have 5 hour buckets"
        );

        // NOW TEST ACTUAL WINDOW SLICING using the testable _from variant
        // Set "now" to be at 14:00:00 (the last hour with data)
        let current_hour_start = base_timestamp + (4 * seconds_per_hour);

        // Test 1: Window of 3 hours should return hours 14:00, 13:00, 12:00
        let result = store.get_tokens_by_hour_from(current_hour_start, 3);
        assert_eq!(result.len(), 3, "Window of 3 hours should return 3 entries");

        // Verify each hour in the window
        assert_eq!(
            result.get(&(base_timestamp + 4 * seconds_per_hour)),
            Some(&500_u64),
            "Hour 14:00 should have 500 tokens"
        );
        assert_eq!(
            result.get(&(base_timestamp + 3 * seconds_per_hour)),
            Some(&400_u64),
            "Hour 13:00 should have 400 tokens"
        );
        assert_eq!(
            result.get(&(base_timestamp + 2 * seconds_per_hour)),
            Some(&300_u64),
            "Hour 12:00 should have 300 tokens"
        );

        // Test 2: Window of 5 hours should return all 5 hours
        let result = store.get_tokens_by_hour_from(current_hour_start, 5);
        assert_eq!(result.len(), 5, "Window of 5 hours should return 5 entries");
        assert_eq!(
            result.get(&(base_timestamp + 4 * seconds_per_hour)),
            Some(&500_u64)
        );
        assert_eq!(
            result.get(&(base_timestamp + 3 * seconds_per_hour)),
            Some(&400_u64)
        );
        assert_eq!(
            result.get(&(base_timestamp + 2 * seconds_per_hour)),
            Some(&300_u64)
        );
        assert_eq!(
            result.get(&(base_timestamp + seconds_per_hour)),
            Some(&200_u64)
        );
        assert_eq!(result.get(&(base_timestamp)), Some(&100_u64));

        // Test 3: Window extending beyond available data should only return available hours
        let result = store.get_tokens_by_hour_from(current_hour_start, 10);
        assert_eq!(
            result.len(),
            5,
            "Window of 10 hours should return only 5 entries (available data)"
        );

        // Test 4: Window starting at 11:00 should only see hours <= 11:00
        let earlier_current = base_timestamp + seconds_per_hour;
        let result = store.get_tokens_by_hour_from(earlier_current, 2);
        assert_eq!(
            result.len(),
            2,
            "Window from 11:00 looking back 2 hours should return 2 entries"
        );
        assert_eq!(
            result.get(&(base_timestamp + seconds_per_hour)),
            Some(&200_u64),
            "Hour 11:00 should have 200 tokens"
        );
        assert_eq!(
            result.get(&base_timestamp),
            Some(&100_u64),
            "Hour 10:00 should have 100 tokens"
        );
        assert_eq!(
            result.get(&(base_timestamp + 2 * seconds_per_hour)),
            None,
            "Hour 12:00 should NOT be in window"
        );

        // Test 5: Verify day boundary handling by adding messages that cross midnight
        // Current base_timestamp is 2024-01-15 10:00:00 UTC
        // Create buckets on BOTH sides of midnight to properly test rollover
        let seconds_per_day: u64 = 86400;
        let day_start = (base_timestamp / seconds_per_day) * seconds_per_day;

        // Prior day's 23:00 (one hour before midnight)
        let prior_day_23 = day_start - seconds_per_hour;

        // Current day's 00:00 (midnight)
        let current_day_00 = day_start;

        // Current day's 01:00
        let current_day_01 = day_start + seconds_per_hour;

        // Current day's 02:00
        let current_day_02 = day_start + (2 * seconds_per_hour);

        // Add message at 23:00 prior day
        let mut msg_23 = make_test_message("msg_23", "pubkey1", "thread1", "test", prior_day_23);
        msg_23.llm_metadata = HashMap::from([("total-tokens".to_string(), "600".to_string())]);
        store
            .messages_by_thread
            .entry("thread1".to_string())
            .or_default()
            .push(msg_23);

        // Add message at 00:00 current day
        let mut msg_00 = make_test_message("msg_00", "pubkey1", "thread1", "test", current_day_00);
        msg_00.llm_metadata = HashMap::from([("total-tokens".to_string(), "700".to_string())]);
        store
            .messages_by_thread
            .entry("thread1".to_string())
            .or_default()
            .push(msg_00);

        // Add message at 01:00 current day
        let mut msg_01 = make_test_message("msg_01", "pubkey1", "thread1", "test", current_day_01);
        msg_01.llm_metadata = HashMap::from([("total-tokens".to_string(), "800".to_string())]);
        store
            .messages_by_thread
            .entry("thread1".to_string())
            .or_default()
            .push(msg_01);

        // Add message at 02:00 current day
        let mut msg_02 = make_test_message("msg_02", "pubkey1", "thread1", "test", current_day_02);
        msg_02.llm_metadata = HashMap::from([("total-tokens".to_string(), "900".to_string())]);
        store
            .messages_by_thread
            .entry("thread1".to_string())
            .or_default()
            .push(msg_02);

        store
            .statistics
            .rebuild_llm_activity_by_hour(&store.messages_by_thread);

        // Window from 02:00 current day looking back 4 hours should cross midnight
        // Should see: 02:00, 01:00, 00:00, 23:00 (prior day)
        let result = store.get_tokens_by_hour_from(current_day_02, 4);

        // Verify all four hours are present
        assert_eq!(
            result.len(),
            4,
            "Window from 02:00 looking back 4 hours should return 4 entries spanning midnight"
        );

        // Verify each hour has the correct token count
        assert_eq!(
            result.get(&current_day_02),
            Some(&900_u64),
            "Current day 02:00 should have 900 tokens"
        );
        assert_eq!(
            result.get(&current_day_01),
            Some(&800_u64),
            "Current day 01:00 should have 800 tokens"
        );
        assert_eq!(
            result.get(&current_day_00),
            Some(&700_u64),
            "Current day 00:00 should have 700 tokens"
        );
        assert_eq!(
            result.get(&prior_day_23),
            Some(&600_u64),
            "Prior day 23:00 should have 600 tokens"
        );

        // Verify proper day_start bucketing: 23:00 should be on prior day, others on current day
        let prior_day_start = day_start - seconds_per_day;

        // Calculate expected day_start for each hour
        let day_start_23 = (prior_day_23 / seconds_per_day) * seconds_per_day;
        let day_start_00 = (current_day_00 / seconds_per_day) * seconds_per_day;
        let day_start_01 = (current_day_01 / seconds_per_day) * seconds_per_day;
        let day_start_02 = (current_day_02 / seconds_per_day) * seconds_per_day;

        // Verify 23:00 is on prior day
        assert_eq!(
            day_start_23, prior_day_start,
            "23:00 should be bucketed to prior day"
        );

        // Verify 00:00, 01:00, 02:00 are all on current day
        assert_eq!(
            day_start_00, day_start,
            "00:00 should be bucketed to current day"
        );
        assert_eq!(
            day_start_01, day_start,
            "01:00 should be bucketed to current day"
        );
        assert_eq!(
            day_start_02, day_start,
            "02:00 should be bucketed to current day"
        );
    }

    /// Test that get_tokens_by_hour and get_message_count_by_hour return empty results
    /// when num_hours is 0, and handle the direct lookup logic correctly.
    #[test]
    fn test_llm_activity_zero_hours_returns_empty() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let store = AppDataStore::new(db.ndb.clone());

        let tokens_result = store.get_tokens_by_hour(0);
        let messages_result = store.get_message_count_by_hour(0);

        assert_eq!(
            tokens_result.len(),
            0,
            "get_tokens_by_hour(0) should return empty"
        );
        assert_eq!(
            messages_result.len(),
            0,
            "get_message_count_by_hour(0) should return empty"
        );
    }

    /// Test that get_message_count_by_hour correctly returns message counts (not token counts)
    /// across a time window. This ensures we're not accidentally swapping tokens and messages.
    #[test]
    fn test_llm_activity_message_count_window() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let seconds_per_hour: u64 = 3600;
        let base_timestamp = 1705316400_u64; // 2024-01-15 10:00:00 UTC

        // Create DIFFERENT numbers of messages per hour with DIFFERENT token counts
        // Hour 0 (10:00): 1 message with 1000 tokens
        // Hour 1 (11:00): 2 messages with 500 tokens each (1000 total tokens)
        // Hour 2 (12:00): 3 messages with 333 tokens each (~1000 total tokens)
        // This ensures we're counting messages, not tokens

        // Hour 0: 1 message
        let mut msg = make_test_message("msg0", "pubkey1", "thread1", "test", base_timestamp);
        msg.llm_metadata = HashMap::from([("total-tokens".to_string(), "1000".to_string())]);
        store
            .messages_by_thread
            .entry("thread1".to_string())
            .or_default()
            .push(msg);

        // Hour 1: 2 messages
        for i in 0..2 {
            let mut msg = make_test_message(
                &format!("msg1_{}", i),
                "pubkey1",
                "thread1",
                "test",
                base_timestamp + seconds_per_hour,
            );
            msg.llm_metadata = HashMap::from([("total-tokens".to_string(), "500".to_string())]);
            store
                .messages_by_thread
                .entry("thread1".to_string())
                .or_default()
                .push(msg);
        }

        // Hour 2: 3 messages
        for i in 0..3 {
            let mut msg = make_test_message(
                &format!("msg2_{}", i),
                "pubkey1",
                "thread1",
                "test",
                base_timestamp + 2 * seconds_per_hour,
            );
            msg.llm_metadata = HashMap::from([("total-tokens".to_string(), "333".to_string())]);
            store
                .messages_by_thread
                .entry("thread1".to_string())
                .or_default()
                .push(msg);
        }

        store
            .statistics
            .rebuild_llm_activity_by_hour(&store.messages_by_thread);

        // Test using the _from variant with a fixed "now" at hour 2 (12:00)
        let current_hour_start = base_timestamp + 2 * seconds_per_hour;

        // Get message counts for all 3 hours
        let message_result = store.get_message_count_by_hour_from(current_hour_start, 3);
        assert_eq!(
            message_result.len(),
            3,
            "Should have 3 hours of message count data"
        );

        // Verify MESSAGE COUNTS (not token counts)
        assert_eq!(
            message_result.get(&(base_timestamp + 2 * seconds_per_hour)),
            Some(&3_u64),
            "Hour 12:00 should have 3 messages"
        );
        assert_eq!(
            message_result.get(&(base_timestamp + seconds_per_hour)),
            Some(&2_u64),
            "Hour 11:00 should have 2 messages"
        );
        assert_eq!(
            message_result.get(&base_timestamp),
            Some(&1_u64),
            "Hour 10:00 should have 1 message"
        );

        // Get token counts for comparison
        let token_result = store.get_tokens_by_hour_from(current_hour_start, 3);
        assert_eq!(
            token_result.len(),
            3,
            "Should have 3 hours of token count data"
        );

        // Verify TOKEN COUNTS are different from message counts
        assert_eq!(
            token_result.get(&(base_timestamp + 2 * seconds_per_hour)),
            Some(&999_u64),
            "Hour 12:00 should have 999 tokens (3*333)"
        );
        assert_eq!(
            token_result.get(&(base_timestamp + seconds_per_hour)),
            Some(&1000_u64),
            "Hour 11:00 should have 1000 tokens (2*500)"
        );
        assert_eq!(
            token_result.get(&base_timestamp),
            Some(&1000_u64),
            "Hour 10:00 should have 1000 tokens"
        );

        // Critical assertion: Verify we're not swapping tokens and messages
        assert_ne!(
            message_result.get(&(base_timestamp + 2 * seconds_per_hour)),
            token_result.get(&(base_timestamp + 2 * seconds_per_hour)),
            "Message count should differ from token count for hour 12:00"
        );
    }

    /// Test that multiple messages in the same hour are correctly aggregated.
    /// Verifies that both token counts and message counts accumulate properly.
    #[test]
    fn test_llm_activity_same_hour_aggregation() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let timestamp = 1705363800_u64; // Same hour for all messages

        // Create 3 messages in the same hour
        let mut msg1 = make_test_message("msg1", "pubkey1", "thread1", "test1", timestamp);
        msg1.llm_metadata = HashMap::from([("total-tokens".to_string(), "100".to_string())]);

        let mut msg2 = make_test_message("msg2", "pubkey1", "thread1", "test2", timestamp + 60);
        msg2.llm_metadata = HashMap::from([("total-tokens".to_string(), "200".to_string())]);

        let mut msg3 = make_test_message("msg3", "pubkey1", "thread1", "test3", timestamp + 120);
        msg3.llm_metadata = HashMap::from([("total-tokens".to_string(), "300".to_string())]);

        store
            .messages_by_thread
            .insert("thread1".to_string(), vec![msg1, msg2, msg3]);
        store
            .statistics
            .rebuild_llm_activity_by_hour(&store.messages_by_thread);

        // Should only have 1 bucket since all messages are in the same hour
        assert_eq!(
            store.statistics.llm_activity_by_hour.len(),
            1,
            "All messages should be in same hour bucket"
        );

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;
        let day_start = (timestamp / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = timestamp - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;
        let key = (day_start, hour_of_day);

        // Total tokens: 100 + 200 + 300 = 600, Total messages: 3
        assert_eq!(
            store.statistics.llm_activity_by_hour.get(&key),
            Some(&(600, 3)),
            "Should aggregate all tokens and count all messages"
        );
    }

    // ===== get_total_cost_since Tests =====

    /// Helper to create a test message with cost metadata
    fn make_test_message_with_cost(
        id: &str,
        pubkey: &str,
        thread_id: &str,
        created_at: u64,
        cost_usd: f64,
    ) -> Message {
        let mut msg = make_test_message(id, pubkey, thread_id, "test", created_at);
        msg.llm_metadata = HashMap::from([("cost-usd".to_string(), cost_usd.to_string())]);
        msg
    }

    /// Test get_total_cost_since with empty message list returns 0.0
    #[test]
    fn test_get_total_cost_since_empty_messages() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let store = AppDataStore::new(db.ndb.clone());

        let result = store.get_total_cost_since(0);
        assert_eq!(result, 0.0, "Empty store should return 0.0 cost");
    }

    /// Test get_total_cost_since with future timestamp returns 0.0
    #[test]
    fn test_get_total_cost_since_future_timestamp() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Add a message with cost at timestamp 1000
        let msg = make_test_message_with_cost("msg1", "pubkey1", "thread1", 1000, 0.50);
        store
            .messages_by_thread
            .insert("thread1".to_string(), vec![msg]);

        // Query with a future timestamp (greater than message timestamp)
        let result = store.get_total_cost_since(2000);
        assert_eq!(
            result, 0.0,
            "Future timestamp should return 0.0 (no matching messages)"
        );
    }

    /// Test get_total_cost_since boundary case: message exactly at cutoff is included
    #[test]
    fn test_get_total_cost_since_boundary_exact_match() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let cutoff = 1000_u64;

        // Message exactly at cutoff (should be included: created_at >= since_timestamp_secs)
        let msg_at_cutoff = make_test_message_with_cost("msg1", "pubkey1", "thread1", cutoff, 0.25);
        // Message before cutoff (should NOT be included)
        let msg_before =
            make_test_message_with_cost("msg2", "pubkey1", "thread1", cutoff - 1, 0.75);
        // Message after cutoff (should be included)
        let msg_after = make_test_message_with_cost("msg3", "pubkey1", "thread1", cutoff + 1, 1.00);

        store.messages_by_thread.insert(
            "thread1".to_string(),
            vec![msg_at_cutoff, msg_before, msg_after],
        );

        let result = store.get_total_cost_since(cutoff);
        // Expected: 0.25 (at cutoff) + 1.00 (after) = 1.25
        // NOT included: 0.75 (before cutoff)
        assert!(
            (result - 1.25).abs() < 0.001,
            "Should include message exactly at cutoff: expected 1.25, got {}",
            result
        );
    }

    /// Test get_total_cost_since with timestamp 0 returns all-time cost
    #[test]
    fn test_get_total_cost_since_zero_returns_all() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Add messages at various timestamps
        let msg1 = make_test_message_with_cost("msg1", "pubkey1", "thread1", 100, 0.10);
        let msg2 = make_test_message_with_cost("msg2", "pubkey1", "thread1", 1000, 0.20);
        let msg3 = make_test_message_with_cost("msg3", "pubkey1", "thread1", 10000, 0.30);

        store
            .messages_by_thread
            .insert("thread1".to_string(), vec![msg1, msg2, msg3]);

        let result = store.get_total_cost_since(0);
        // Expected: 0.10 + 0.20 + 0.30 = 0.60
        assert!(
            (result - 0.60).abs() < 0.001,
            "Timestamp 0 should return all messages: expected 0.60, got {}",
            result
        );
    }

    /// Test get_total_cost_since correctly filters by the COST_WINDOW_DAYS window boundary
    #[test]
    fn test_get_total_cost_since_cost_window() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let now = 1700000000_u64; // Some arbitrary "now" timestamp
        let seconds_per_day = 86400_u64;
        let window_start = now.saturating_sub(COST_WINDOW_DAYS * seconds_per_day);

        // Message within window (should be included)
        let msg_within = make_test_message_with_cost(
            "msg1",
            "pubkey1",
            "thread1",
            now - (7 * seconds_per_day), // 7 days ago
            1.00,
        );
        // Message exactly at boundary (should be included)
        let msg_boundary = make_test_message_with_cost(
            "msg2",
            "pubkey1",
            "thread1",
            window_start, // exactly at COST_WINDOW_DAYS boundary
            2.00,
        );
        // Message outside window (should NOT be included)
        let msg_outside = make_test_message_with_cost(
            "msg3",
            "pubkey1",
            "thread1",
            window_start - 1, // COST_WINDOW_DAYS + 1 second ago
            5.00,
        );

        store.messages_by_thread.insert(
            "thread1".to_string(),
            vec![msg_within, msg_boundary, msg_outside],
        );

        let result = store.get_total_cost_since(window_start);
        // Expected: 1.00 (within) + 2.00 (boundary) = 3.00
        // NOT included: 5.00 (outside)
        assert!(
            (result - 3.00).abs() < 0.001,
            "COST_WINDOW_DAYS window should include boundary: expected 3.00, got {}",
            result
        );
    }

    // ===== Test Helpers for Decomposition Safety Net =====

    fn make_test_thread(id: &str, pubkey: &str, last_activity: u64) -> Thread {
        Thread {
            id: id.to_string(),
            title: format!("Thread {}", id),
            content: String::new(),
            pubkey: pubkey.to_string(),
            last_activity,
            effective_last_activity: last_activity,
            status_label: None,
            status_current_activity: None,
            summary: None,
            hashtags: vec![],
            parent_conversation_id: None,
            p_tags: vec![],
            ask_event: None,
            is_scheduled: false,
        }
    }

    fn make_test_report(slug: &str, project_a_tag: &str, created_at: u64) -> Report {
        Report {
            id: format!("report-{}-{}", slug, created_at),
            slug: slug.to_string(),
            project_a_tag: project_a_tag.to_string(),
            author: "author1".to_string(),
            title: format!("Report {}", slug),
            summary: String::new(),
            content: "Report content".to_string(),
            hashtags: vec![],
            created_at,
            reading_time_mins: 1,
        }
    }

    fn make_test_agent_def(id: &str, name: &str, created_at: u64) -> AgentDefinition {
        AgentDefinition {
            id: id.to_string(),
            pubkey: "pubkey1".to_string(),
            d_tag: id.to_string(),
            name: name.to_string(),
            description: String::new(),
            role: String::new(),
            content: String::new(),
            instructions: String::new(),
            picture: None,
            version: None,
            model: None,
            tools: vec![],
            mcp_servers: vec![],
            use_criteria: vec![],
            file_ids: vec![],
            created_at,
        }
    }

    fn make_test_nudge(id: &str, title: &str, created_at: u64) -> Nudge {
        Nudge {
            id: id.to_string(),
            pubkey: "pubkey1".to_string(),
            d_tag: id.to_string(),
            title: title.to_string(),
            description: String::new(),
            content: String::new(),
            hashtags: vec![],
            created_at,
            allowed_tools: vec![],
            denied_tools: vec![],
            only_tools: vec![],
            supersedes: None,
        }
    }

    fn make_test_mcp_tool(id: &str, name: &str, created_at: u64) -> MCPTool {
        MCPTool {
            id: id.to_string(),
            pubkey: "pubkey1".to_string(),
            d_tag: id.to_string(),
            name: name.to_string(),
            description: String::new(),
            command: String::new(),
            parameters: None,
            capabilities: vec![],
            server_url: None,
            version: None,
            created_at,
        }
    }

    fn make_test_inbox_item(id: &str, event_type: InboxEventType, created_at: u64) -> InboxItem {
        InboxItem {
            id: id.to_string(),
            event_type,
            title: format!("Inbox {}", id),
            content: "content".to_string(),
            project_a_tag: "31933:pk:proj1".to_string(),
            author_pubkey: "author1".to_string(),
            created_at,
            is_read: false,
            thread_id: None,
            ask_event: None,
        }
    }

    fn make_test_operations_status(
        event_id: &str,
        project: &str,
        agents: Vec<&str>,
        created_at: u64,
    ) -> OperationsStatus {
        OperationsStatus {
            nostr_event_id: format!("nostr-{}", event_id),
            event_id: event_id.to_string(),
            agent_pubkeys: agents.into_iter().map(|s| s.to_string()).collect(),
            project_coordinate: project.to_string(),
            created_at,
            thread_id: None,
            llm_runtime_secs: None,
        }
    }

    fn make_test_lesson(id: &str, title: &str, created_at: u64) -> Lesson {
        Lesson {
            id: id.to_string(),
            pubkey: "pubkey1".to_string(),
            title: title.to_string(),
            content: "lesson content".to_string(),
            detailed: None,
            reasoning: None,
            metacognition: None,
            reflection: None,
            category: None,
            created_at,
        }
    }

    // ===== A. Content/Definitions Tests =====

    #[test]
    fn test_content_empty_store_returns_empty() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let store = AppDataStore::new(db.ndb.clone());

        assert!(store.content.get_agent_definitions().is_empty());
        assert!(store.content.get_mcp_tools().is_empty());
        assert!(store.content.get_nudges().is_empty());
        assert!(store.content.get_lesson("nonexistent").is_none());
    }

    #[test]
    fn test_content_agent_definitions_sorted_descending() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.content.agent_definitions.insert(
            "a1".to_string(),
            make_test_agent_def("a1", "Older Agent", 100),
        );
        store.content.agent_definitions.insert(
            "a2".to_string(),
            make_test_agent_def("a2", "Newer Agent", 200),
        );
        store.content.agent_definitions.insert(
            "a3".to_string(),
            make_test_agent_def("a3", "Middle Agent", 150),
        );

        let defs = store.content.get_agent_definitions();
        assert_eq!(defs.len(), 3);
        assert_eq!(defs[0].name, "Newer Agent");
        assert_eq!(defs[1].name, "Middle Agent");
        assert_eq!(defs[2].name, "Older Agent");
    }

    #[test]
    fn test_content_agent_definition_lookup() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.content.agent_definitions.insert(
            "a1".to_string(),
            make_test_agent_def("a1", "Agent One", 100),
        );

        assert!(store.content.get_agent_definition("a1").is_some());
        assert_eq!(
            store.content.get_agent_definition("a1").unwrap().name,
            "Agent One"
        );
        assert!(store.content.get_agent_definition("nonexistent").is_none());
    }

    #[test]
    fn test_content_mcp_tools_sorted_descending() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .content
            .mcp_tools
            .insert("t1".to_string(), make_test_mcp_tool("t1", "Old Tool", 100));
        store
            .content
            .mcp_tools
            .insert("t2".to_string(), make_test_mcp_tool("t2", "New Tool", 200));

        let tools = store.content.get_mcp_tools();
        assert_eq!(tools.len(), 2);
        assert_eq!(tools[0].name, "New Tool");
        assert_eq!(tools[1].name, "Old Tool");
    }

    #[test]
    fn test_content_mcp_tool_lookup() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .content
            .mcp_tools
            .insert("t1".to_string(), make_test_mcp_tool("t1", "Tool One", 100));

        assert!(store.content.get_mcp_tool("t1").is_some());
        assert!(store.content.get_mcp_tool("missing").is_none());
    }

    #[test]
    fn test_content_nudges_sorted_descending() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .content
            .nudges
            .insert("n1".to_string(), make_test_nudge("n1", "Old Nudge", 100));
        store
            .content
            .nudges
            .insert("n2".to_string(), make_test_nudge("n2", "New Nudge", 200));

        let nudges = store.content.get_nudges();
        assert_eq!(nudges.len(), 2);
        assert_eq!(nudges[0].title, "New Nudge");
        assert_eq!(nudges[1].title, "Old Nudge");
    }

    #[test]
    fn test_content_lesson_lookup() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .content
            .lessons
            .insert("l1".to_string(), make_test_lesson("l1", "Lesson One", 100));

        assert!(store.content.get_lesson("l1").is_some());
        assert_eq!(store.content.get_lesson("l1").unwrap().title, "Lesson One");
        assert!(store.content.get_lesson("missing").is_none());
    }

    #[test]
    fn test_content_cleared_on_clear() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .content
            .agent_definitions
            .insert("a1".to_string(), make_test_agent_def("a1", "Agent", 100));
        store
            .content
            .mcp_tools
            .insert("t1".to_string(), make_test_mcp_tool("t1", "Tool", 100));
        store
            .content
            .nudges
            .insert("n1".to_string(), make_test_nudge("n1", "Nudge", 100));
        store
            .content
            .lessons
            .insert("l1".to_string(), make_test_lesson("l1", "Lesson", 100));

        store.clear();

        assert!(store.content.get_agent_definitions().is_empty());
        assert!(store.content.get_mcp_tools().is_empty());
        assert!(store.content.get_nudges().is_empty());
        assert!(store.content.get_lesson("l1").is_none());
    }

    // ===== B. Trust Tests =====

    #[test]
    fn test_trust_set_trusted_backends() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let approved: HashSet<String> = ["pk1".to_string(), "pk2".to_string()].into();
        let blocked: HashSet<String> = ["pk3".to_string()].into();

        store.trust.set_trusted_backends(approved, blocked);

        assert!(store.trust.approved_backends.contains("pk1"));
        assert!(store.trust.approved_backends.contains("pk2"));
        assert!(store.trust.blocked_backends.contains("pk3"));
    }

    #[test]
    fn test_trust_add_approved_removes_from_blocked() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.trust.blocked_backends.insert("pk1".to_string());
        store.add_approved_backend("pk1");

        assert!(store.trust.approved_backends.contains("pk1"));
        assert!(!store.trust.blocked_backends.contains("pk1"));
    }

    #[test]
    fn test_trust_add_blocked_removes_from_approved() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.trust.approved_backends.insert("pk1".to_string());
        store.add_blocked_backend("pk1");

        assert!(!store.trust.approved_backends.contains("pk1"));
        assert!(store.trust.blocked_backends.contains("pk1"));
    }

    #[test]
    fn test_trust_pending_approvals_queued_and_drained() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let approval = PendingBackendApproval {
            backend_pubkey: "pk1".to_string(),
            project_a_tag: "31933:pk:proj1".to_string(),
            first_seen: 100,
            status: ProjectStatus {
                project_coordinate: "31933:pk:proj1".to_string(),
                agents: vec![],
                branches: vec![],
                all_models: vec![],
                all_tools: vec![],
                created_at: 100,
                backend_pubkey: "pk1".to_string(),
                last_seen_at: 100,
            },
        };

        store.trust.pending_backend_approvals.push(approval);

        assert!(store.trust.has_pending_approvals());
        assert!(store.trust.has_pending_approval("pk1", "31933:pk:proj1"));
        assert!(!store.trust.has_pending_approval("pk1", "other_project"));
        assert!(!store.trust.has_pending_approval("pk2", "31933:pk:proj1"));

        let drained = store.drain_pending_backend_approvals();
        assert_eq!(drained.len(), 1);
        assert!(!store.trust.has_pending_approvals());
    }

    #[test]
    fn test_trust_blocking_removes_pending_approvals() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .trust
            .pending_backend_approvals
            .push(PendingBackendApproval {
                backend_pubkey: "pk1".to_string(),
                project_a_tag: "proj1".to_string(),
                first_seen: 100,
                status: ProjectStatus {
                    project_coordinate: "proj1".to_string(),
                    agents: vec![],
                    branches: vec![],
                    all_models: vec![],
                    all_tools: vec![],
                    created_at: 100,
                    backend_pubkey: "pk1".to_string(),
                    last_seen_at: 100,
                },
            });

        store.add_blocked_backend("pk1");

        assert!(store.trust.pending_backend_approvals.is_empty());
        assert!(store.trust.blocked_backends.contains("pk1"));
    }

    #[test]
    fn test_trust_approving_applies_pending_statuses() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .trust
            .pending_backend_approvals
            .push(PendingBackendApproval {
                backend_pubkey: "pk1".to_string(),
                project_a_tag: "31933:pk:proj1".to_string(),
                first_seen: 100,
                status: ProjectStatus {
                    project_coordinate: "31933:pk:proj1".to_string(),
                    agents: vec![],
                    branches: vec![],
                    all_models: vec![],
                    all_tools: vec![],
                    created_at: 100,
                    backend_pubkey: "pk1".to_string(),
                    last_seen_at: 100,
                },
            });

        store.add_approved_backend("pk1");

        assert!(store.trust.approved_backends.contains("pk1"));
        assert!(store.trust.pending_backend_approvals.is_empty());
        assert!(store.project_statuses.contains_key("31933:pk:proj1"));
    }

    #[test]
    fn test_trust_cleared_on_clear() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.trust.approved_backends.insert("pk1".to_string());
        store.trust.blocked_backends.insert("pk2".to_string());
        store
            .trust
            .pending_backend_approvals
            .push(PendingBackendApproval {
                backend_pubkey: "pk3".to_string(),
                project_a_tag: "proj".to_string(),
                first_seen: 100,
                status: ProjectStatus {
                    project_coordinate: "proj".to_string(),
                    agents: vec![],
                    branches: vec![],
                    all_models: vec![],
                    all_tools: vec![],
                    created_at: 100,
                    backend_pubkey: "pk3".to_string(),
                    last_seen_at: 100,
                },
            });

        store.clear();

        assert!(store.trust.approved_backends.is_empty());
        assert!(store.trust.blocked_backends.is_empty());
        assert!(store.trust.pending_backend_approvals.is_empty());
    }

    // ===== C. Reports Tests =====

    #[test]
    fn test_reports_empty_store() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let store = AppDataStore::new(db.ndb.clone());

        assert!(store.reports.get_reports().is_empty());
        assert!(store.reports.get_report("slug").is_none());
        assert!(store.reports.get_reports_by_project("proj").is_empty());
        assert!(store.reports.get_document_threads("doc").is_empty());
    }

    #[test]
    fn test_reports_sorted_by_created_at_descending() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .reports
            .add_report(make_test_report("slug-a", "proj1", 100));
        store
            .reports
            .add_report(make_test_report("slug-b", "proj1", 300));
        store
            .reports
            .add_report(make_test_report("slug-c", "proj1", 200));

        let reports = store.reports.get_reports();
        assert_eq!(reports.len(), 3);
        assert_eq!(reports[0].slug, "slug-b");
        assert_eq!(reports[1].slug, "slug-c");
        assert_eq!(reports[2].slug, "slug-a");
    }

    #[test]
    fn test_reports_filtered_by_project() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .reports
            .add_report(make_test_report("slug-a", "proj1", 100));
        store
            .reports
            .add_report(make_test_report("slug-b", "proj2", 200));
        store
            .reports
            .add_report(make_test_report("slug-c", "proj1", 300));

        let proj1_reports = store.reports.get_reports_by_project("proj1");
        assert_eq!(proj1_reports.len(), 2);
        assert_eq!(proj1_reports[0].slug, "slug-c");
        assert_eq!(proj1_reports[1].slug, "slug-a");

        let proj2_reports = store.reports.get_reports_by_project("proj2");
        assert_eq!(proj2_reports.len(), 1);
    }

    #[test]
    fn test_reports_version_history() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Two versions of the same slug
        let v1 = Report {
            id: "v1-id".to_string(),
            slug: "my-report".to_string(),
            project_a_tag: "proj1".to_string(),
            author: "author".to_string(),
            title: "Version 1".to_string(),
            summary: String::new(),
            content: "v1 content".to_string(),
            hashtags: vec![],
            created_at: 100,
            reading_time_mins: 1,
        };
        let v2 = Report {
            id: "v2-id".to_string(),
            slug: "my-report".to_string(),
            project_a_tag: "proj1".to_string(),
            author: "author".to_string(),
            title: "Version 2".to_string(),
            summary: String::new(),
            content: "v2 content".to_string(),
            hashtags: vec![],
            created_at: 200,
            reading_time_mins: 1,
        };

        store.reports.add_report(v1);
        store.reports.add_report(v2);

        // Latest version should be v2
        let latest = store.reports.get_report("my-report").unwrap();
        assert_eq!(latest.title, "Version 2");

        // All versions
        let versions = store.reports.get_report_versions("my-report");
        assert_eq!(versions.len(), 2);
        assert_eq!(versions[0].id, "v2-id"); // newest first
        assert_eq!(versions[1].id, "v1-id");

        // Previous version
        let prev = store
            .reports
            .get_previous_report_version("my-report", "v2-id");
        assert!(prev.is_some());
        assert_eq!(prev.unwrap().id, "v1-id");

        // No previous for oldest
        assert!(store
            .reports
            .get_previous_report_version("my-report", "v1-id")
            .is_none());
    }

    #[test]
    fn test_reports_document_threads() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let thread = make_test_thread("t1", "pk1", 100);
        store
            .reports
            .document_threads
            .entry("doc-atag".to_string())
            .or_default()
            .push(thread);

        let threads = store.reports.get_document_threads("doc-atag");
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].id, "t1");

        assert!(store.reports.get_document_threads("missing").is_empty());
    }

    #[test]
    fn test_kind1_root_with_project_and_report_a_tags_indexes_in_both_views() {
        use crate::store::events::{ingest_events, wait_for_event_processing};
        use nostr_sdk::prelude::*;
        use nostrdb::Transaction;

        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());
        let keys = Keys::generate();

        let project_a_tag = format!("31933:{}:TENEX-ff3ssq", keys.public_key().to_hex());
        let report_author =
            "14925f2b4795ca6037fa7d33899c5145d3c1f264865d94ea028ba6168f394ebf".to_string();
        let report_a_tag = format!("30023:{}:nostr-skill-events-kind-4202", report_author);

        let event = EventBuilder::new(Kind::from(1), "test")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec![project_a_tag.clone()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec![report_a_tag.clone()],
            ))
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::P)),
                vec![report_author],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, std::slice::from_ref(&event), None).unwrap();

        let filter = nostrdb::Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        let txn = Transaction::new(&db.ndb).unwrap();
        let notes: Vec<_> = db
            .ndb
            .query(&txn, &[filter], 10)
            .unwrap()
            .into_iter()
            .filter_map(|r| db.ndb.get_note_by_key(&txn, r.note_key).ok())
            .collect();
        assert_eq!(notes.len(), 1, "expected one kind:1 root note");

        store.handle_event(1, &notes[0]);

        let thread_id = event.id.to_hex();

        assert!(
            store.get_thread_by_id(&thread_id).is_some(),
            "thread should be indexed in global conversation store"
        );
        assert_eq!(
            store.get_threads(&project_a_tag).len(),
            1,
            "thread should be indexed under project threads"
        );
        assert_eq!(
            store.get_thread_root_count(&project_a_tag),
            1,
            "thread root index should include the new thread"
        );
        assert!(
            store
                .reports
                .get_document_threads(&report_a_tag)
                .iter()
                .any(|t| t.id == thread_id),
            "thread should be indexed under report document threads"
        );
    }

    #[test]
    fn test_rebuild_indexes_existing_report_document_threads() {
        use crate::store::events::{ingest_events, wait_for_event_processing};
        use nostr_sdk::prelude::*;

        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let d_tag = "TENEX-ff3ssq".to_string();
        let project_a_tag = format!("31933:{}:{}", keys.public_key().to_hex(), d_tag);
        let report_author =
            "14925f2b4795ca6037fa7d33899c5145d3c1f264865d94ea028ba6168f394ebf".to_string();
        let report_a_tag = format!("30023:{}:nostr-skill-events-kind-4202", report_author);

        let project_event = EventBuilder::new(Kind::Custom(31933), "Project description")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec![d_tag],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("name")),
                vec!["TENEX".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        let thread_event = EventBuilder::new(Kind::from(1), "test")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec![project_a_tag],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec![report_a_tag.clone()],
            ))
            .sign_with_keys(&keys)
            .unwrap();
        let thread_id = thread_event.id.to_hex();

        ingest_events(&db.ndb, &[project_event, thread_event], None).unwrap();

        let filter = nostrdb::Filter::new().kinds([31933, 1]).build();
        wait_for_event_processing(&db.ndb, filter, 5000);

        let store = AppDataStore::new(db.ndb.clone());
        assert!(
            store
                .reports
                .get_document_threads(&report_a_tag)
                .iter()
                .any(|t| t.id == thread_id),
            "rebuild_from_ndb should recover report-tagged thread roots"
        );
    }

    #[test]
    fn test_reports_cleared_on_clear() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .reports
            .add_report(make_test_report("slug", "proj1", 100));
        store
            .reports
            .document_threads
            .entry("doc".to_string())
            .or_default()
            .push(make_test_thread("t1", "pk1", 100));

        store.clear();

        assert!(store.reports.get_reports().is_empty());
        assert!(store.reports.get_document_threads("doc").is_empty());
    }

    // ===== D. Inbox Tests =====

    #[test]
    fn test_inbox_add_and_sort_order() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .inbox
            .add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        store
            .inbox
            .add_item(make_test_inbox_item("i3", InboxEventType::Mention, 300));
        store
            .inbox
            .add_item(make_test_inbox_item("i2", InboxEventType::Ask, 200));

        let items = store.inbox.get_items();
        assert_eq!(items.len(), 3);
        assert_eq!(items[0].id, "i3"); // most recent first
        assert_eq!(items[1].id, "i2");
        assert_eq!(items[2].id, "i1");
    }

    #[test]
    fn test_inbox_deduplication() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .inbox
            .add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        store
            .inbox
            .add_item(make_test_inbox_item("i1", InboxEventType::Ask, 200)); // same id

        assert_eq!(store.inbox.get_items().len(), 1);
    }

    #[test]
    fn test_inbox_mark_read() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .inbox
            .add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        assert!(!store.inbox.get_items()[0].is_read);

        store.inbox.mark_read("i1");
        assert!(store.inbox.get_items()[0].is_read);
    }

    #[test]
    fn test_inbox_read_state_persists_for_new_items() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Mark as read before adding
        store.inbox.mark_read("i1");

        // Adding item with same id should be marked as read
        store
            .inbox
            .add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        assert!(store.inbox.get_items()[0].is_read);
    }

    #[test]
    fn test_inbox_cleared_on_clear() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .inbox
            .add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        store.clear();

        assert!(store.inbox.get_items().is_empty());
    }

    // ===== E. Operations Tests =====

    #[test]
    fn test_operations_empty_store() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let store = AppDataStore::new(db.ndb.clone());

        assert!(store.operations.get_working_agents("ev1").is_empty());
        assert!(!store.operations.is_event_busy("ev1"));
        assert_eq!(store.operations.get_active_operations_count("proj1"), 0);
        assert!(store.operations.get_active_event_ids("proj1").is_empty());
        assert!(!store.operations.is_project_busy("proj1"));
        assert!(store.operations.get_all_active_operations().is_empty());
        assert_eq!(store.operations.active_operations_count(), 0);
    }

    #[test]
    fn test_operations_working_agents() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.operations.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["agent1", "agent2"], 100),
        );

        let agents = store.operations.get_working_agents("ev1");
        assert_eq!(agents.len(), 2);
        assert!(store.operations.is_event_busy("ev1"));
        assert!(!store.operations.is_event_busy("ev2"));
    }

    #[test]
    fn test_operations_per_project_counts() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.operations.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["a1"], 100),
        );
        store.operations.operations_by_event.insert(
            "ev2".to_string(),
            make_test_operations_status("ev2", "proj1", vec!["a2"], 200),
        );
        store.operations.operations_by_event.insert(
            "ev3".to_string(),
            make_test_operations_status("ev3", "proj2", vec!["a3"], 300),
        );

        assert_eq!(store.operations.get_active_operations_count("proj1"), 2);
        assert_eq!(store.operations.get_active_operations_count("proj2"), 1);
        assert!(store.operations.is_project_busy("proj1"));
        assert!(store.operations.is_project_busy("proj2"));
        assert!(!store.operations.is_project_busy("proj3"));

        let proj1_events = store.operations.get_active_event_ids("proj1");
        assert_eq!(proj1_events.len(), 2);
    }

    #[test]
    fn test_operations_empty_agents_not_counted() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.operations.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec![], 100), // empty agents
        );
        store.operations.operations_by_event.insert(
            "ev2".to_string(),
            make_test_operations_status("ev2", "proj1", vec!["a1"], 200),
        );

        assert_eq!(store.operations.get_active_operations_count("proj1"), 1);
        assert!(!store.operations.is_event_busy("ev1"));
        assert!(store.operations.is_event_busy("ev2"));
        assert_eq!(store.operations.active_operations_count(), 1);
    }

    #[test]
    fn test_operations_all_active_sorted_by_created_at() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.operations.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["a1"], 300),
        );
        store.operations.operations_by_event.insert(
            "ev2".to_string(),
            make_test_operations_status("ev2", "proj1", vec!["a2"], 100),
        );
        store.operations.operations_by_event.insert(
            "ev3".to_string(),
            make_test_operations_status("ev3", "proj1", vec!["a3"], 200),
        );

        let all = store.operations.get_all_active_operations();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].event_id, "ev2"); // oldest first
        assert_eq!(all[1].event_id, "ev3");
        assert_eq!(all[2].event_id, "ev1");
    }

    #[test]
    fn test_operations_project_working_agents_deduped() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Same agent working on two events in same project
        store.operations.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["agent1", "agent2"], 100),
        );
        store.operations.operations_by_event.insert(
            "ev2".to_string(),
            make_test_operations_status("ev2", "proj1", vec!["agent1"], 200),
        );

        let agents = store.operations.get_project_working_agents("proj1");
        // agent1 appears in both events, but should be deduped
        assert_eq!(agents.len(), 2); // agent1, agent2
    }

    #[test]
    fn test_operations_cleared_on_clear() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.operations.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["a1"], 100),
        );

        store.clear();

        assert!(store.operations.get_all_active_operations().is_empty());
        assert_eq!(store.operations.active_operations_count(), 0);
    }

    // ===== F. Statistics Tests =====
    // (Note: increment_* methods are private, so we test through public getters
    //  by directly populating the pre-aggregated hashmaps)

    #[test]
    fn test_statistics_messages_by_day_empty() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let store = AppDataStore::new(db.ndb.clone());

        let (user, all) = store.get_messages_by_day(7);
        assert!(user.is_empty());
        assert!(all.is_empty());
    }

    #[test]
    fn test_statistics_messages_by_day_zero_days() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let store = AppDataStore::new(db.ndb.clone());

        let (user, all) = store.get_messages_by_day(0);
        assert!(user.is_empty());
        assert!(all.is_empty());
    }

    #[test]
    fn test_statistics_messages_by_day_window() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let today_start = (now / 86400) * 86400;

        // Insert data for today and yesterday
        store
            .statistics
            .messages_by_day_counts
            .insert(today_start, (5, 10));
        store
            .statistics
            .messages_by_day_counts
            .insert(today_start - 86400, (3, 7));
        // Insert data for 10 days ago (should not appear in 3-day window)
        store
            .statistics
            .messages_by_day_counts
            .insert(today_start - 86400 * 10, (1, 2));

        let (user, all) = store.get_messages_by_day(3);
        assert_eq!(user.len(), 2); // today + yesterday
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_statistics_tokens_by_hour_window() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Use a fixed reference hour for deterministic test
        let reference_hour_start: u64 = 86400 * 100; // day 100, hour 0
        let day_start = (reference_hour_start / 86400) * 86400;

        // Insert tokens for hour 0 of day 100
        store
            .statistics
            .llm_activity_by_hour
            .insert((day_start, 0), (500, 10));

        let tokens = store.get_tokens_by_hour_from(reference_hour_start, 24);
        assert_eq!(tokens.get(&reference_hour_start), Some(&500));
    }

    #[test]
    fn test_statistics_message_count_by_hour() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        let reference_hour_start: u64 = 86400 * 100;
        let day_start = (reference_hour_start / 86400) * 86400;

        store
            .statistics
            .llm_activity_by_hour
            .insert((day_start, 0), (500, 10));

        let counts = store.get_message_count_by_hour_from(reference_hour_start, 24);
        assert_eq!(counts.get(&reference_hour_start), Some(&10));
    }

    #[test]
    fn test_statistics_zero_hours_returns_empty() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let store = AppDataStore::new(db.ndb.clone());

        let tokens = store.get_tokens_by_hour_from(86400, 0);
        assert!(tokens.is_empty());

        let counts = store.get_message_count_by_hour_from(86400, 0);
        assert!(counts.is_empty());
    }

    // ===== G. Core Cross-Cutting Tests =====

    #[test]
    fn test_core_projects_threads_messages() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        assert!(store.get_projects().is_empty());
        assert!(store.get_threads("proj1").is_empty());
        assert!(store.get_messages("thread1").is_empty());

        store.projects.push(Project {
            id: "p1".to_string(),
            title: "Project One".to_string(),
            description: None,
            repo_url: None,
            picture_url: None,
            is_deleted: false,
            pubkey: "pk".to_string(),
            participants: vec![],
            agent_definition_ids: vec![],
            mcp_tool_ids: vec![],
            created_at: 100,
        });

        assert_eq!(store.get_projects().len(), 1);
    }

    #[test]
    fn test_core_thread_by_id_cross_project() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .threads_by_project
            .entry("proj1".to_string())
            .or_default()
            .push(make_test_thread("t1", "pk1", 100));
        store
            .threads_by_project
            .entry("proj2".to_string())
            .or_default()
            .push(make_test_thread("t2", "pk1", 200));

        // Should find thread in project1
        let t1 = store.get_thread_by_id("t1");
        assert!(t1.is_some());
        assert_eq!(t1.unwrap().id, "t1");

        // Should find thread in project2
        let t2 = store.get_thread_by_id("t2");
        assert!(t2.is_some());

        // Missing thread
        assert!(store.get_thread_by_id("t3").is_none());
    }

    #[test]
    fn test_core_clear_resets_everything() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        // Populate multiple domains
        store.projects.push(Project {
            id: "p1".to_string(),
            title: "Proj".to_string(),
            description: None,
            repo_url: None,
            picture_url: None,
            is_deleted: false,
            pubkey: "pk".to_string(),
            participants: vec![],
            agent_definition_ids: vec![],
            mcp_tool_ids: vec![],
            created_at: 0,
        });
        store
            .threads_by_project
            .entry("proj1".to_string())
            .or_default()
            .push(make_test_thread("t1", "pk1", 100));
        store
            .messages_by_thread
            .entry("t1".to_string())
            .or_default()
            .push(make_test_message("m1", "pk1", "t1", "hello", 100));
        store
            .content
            .agent_definitions
            .insert("a1".to_string(), make_test_agent_def("a1", "Agent", 100));
        store
            .inbox
            .add_item(make_test_inbox_item("i1", InboxEventType::Ask, 100));
        store.operations.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["a1"], 100),
        );
        store
            .reports
            .add_report(make_test_report("slug", "proj1", 100));
        store.trust.approved_backends.insert("pk1".to_string());
        store.user_pubkey = Some("user1".to_string());

        store.clear();

        assert!(store.get_projects().is_empty());
        assert!(store.get_threads("proj1").is_empty());
        assert!(store.get_messages("t1").is_empty());
        assert!(store.content.get_agent_definitions().is_empty());
        assert!(store.inbox.get_items().is_empty());
        assert!(store.operations.get_all_active_operations().is_empty());
        assert!(store.reports.get_reports().is_empty());
        assert!(store.trust.approved_backends.is_empty());
        assert!(store.user_pubkey.is_none());
    }

    #[test]
    fn test_login_rebuild_flag_transitions_after_clear() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        assert!(!store.needs_rebuild_for_login());

        store.clear();
        assert!(store.needs_rebuild_for_login());

        store.rebuild_from_ndb();
        assert!(!store.needs_rebuild_for_login());
    }

    #[test]
    fn test_apply_authenticated_user_rebuilds_after_clear() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store.clear();
        assert!(store.needs_rebuild_for_login());

        store.apply_authenticated_user("user1".to_string());

        assert_eq!(store.user_pubkey.as_deref(), Some("user1"));
        assert!(!store.needs_rebuild_for_login());
    }

    #[test]
    fn test_set_user_pubkey_skips_answered_ask_without_ndb_candidate_fetches() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .threads_by_project
            .entry("proj1".to_string())
            .or_default()
            .push(make_test_thread("t1", "agent1", 102));

        let mut ask = make_test_message("ask1", "agent1", "t1", "Need input", 100);
        ask.p_tags = vec!["user1".to_string()];
        ask.ask_event = Some(AskEvent {
            title: Some("Question".to_string()),
            context: "Need input".to_string(),
            questions: vec![crate::models::AskQuestion::SingleSelect {
                title: "q1".to_string(),
                question: "What now?".to_string(),
                suggestions: vec!["A".to_string()],
            }],
        });

        let mut reply = make_test_message("reply1", "user1", "t1", "Answer", 101);
        reply.reply_to = Some("ask1".to_string());

        let mut mention = make_test_message("mention1", "agent2", "t1", "FYI", 102);
        mention.p_tags = vec!["user1".to_string()];

        store
            .messages_by_thread
            .insert("t1".to_string(), vec![ask, reply, mention]);

        store.set_user_pubkey("user1".to_string());

        let inbox = store.inbox.get_items();
        assert_eq!(inbox.len(), 1);
        assert_eq!(inbox[0].id, "mention1");
        assert_eq!(inbox[0].event_type, InboxEventType::Mention);
    }

    #[test]
    fn test_core_get_threads_returns_slice() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let mut store = AppDataStore::new(db.ndb.clone());

        store
            .threads_by_project
            .entry("proj1".to_string())
            .or_default()
            .push(make_test_thread("t1", "pk1", 100));
        store
            .threads_by_project
            .entry("proj1".to_string())
            .or_default()
            .push(make_test_thread("t2", "pk1", 200));

        let threads = store.get_threads("proj1");
        assert_eq!(threads.len(), 2);

        let empty = store.get_threads("nonexistent");
        assert!(empty.is_empty());
    }
}
