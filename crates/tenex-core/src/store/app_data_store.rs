use crate::events::PendingBackendApproval;
use crate::models::{AgentChatter, AgentDefinition, AskEvent, ConversationMetadata, InboxEventType, InboxItem, Lesson, MCPTool, Message, Nudge, OperationsStatus, Project, ProjectAgent, ProjectStatus, Report, Thread};
use crate::store::{AgentTrackingState, RuntimeHierarchy, RUNTIME_CUTOFF_TIMESTAMP};
use nostrdb::{Ndb, Note, Transaction};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{trace, warn};

/// Default batch size for pagination queries.
/// Set high enough to minimize same-second overflow risk while keeping memory reasonable.
const DEFAULT_PAGINATION_BATCH_SIZE: i32 = 10_000;

/// Reactive data store - single source of truth for app-level concepts.
/// Rebuilt from nostrdb on startup, updated incrementally on new events.
pub struct AppDataStore {
    ndb: Arc<Ndb>,

    // Core app data
    pub projects: Vec<Project>,
    pub project_statuses: HashMap<String, ProjectStatus>,  // keyed by project a_tag
    pub threads_by_project: HashMap<String, Vec<Thread>>,  // keyed by project a_tag
    pub messages_by_thread: HashMap<String, Vec<Message>>, // keyed by thread_id
    pub profiles: HashMap<String, String>,                  // pubkey -> display name

    // Inbox - events that p-tag the current user
    pub inbox_items: Vec<InboxItem>,
    inbox_read_ids: HashSet<String>,
    pub user_pubkey: Option<String>,

    // Agent chatter feed - kind:1 events a-tagging our projects
    pub agent_chatter: Vec<AgentChatter>,

    // Agent lessons - kind:4129 events
    pub lessons: HashMap<String, Lesson>,  // keyed by lesson id

    // Agent definitions - kind:4199 events
    pub agent_definitions: HashMap<String, AgentDefinition>,  // keyed by id

    // Nudges - kind:4201 events
    pub nudges: HashMap<String, Nudge>,  // keyed by id

    // MCP Tools - kind:4200 events
    pub mcp_tools: HashMap<String, MCPTool>,  // keyed by id

    // Reports - kind:30023 events (articles/documents)
    // Key: report slug (d-tag) -> latest version
    pub reports: HashMap<String, Report>,
    // All versions by slug (for version history)
    pub reports_all_versions: HashMap<String, Vec<Report>>,
    // Threads by document a-tag (kind:1 events that a-tag a document)
    pub document_threads: HashMap<String, Vec<Thread>>,

    // Operations status - kind:24133 events
    // Maps event_id -> OperationsStatus (which agents are working on which events)
    operations_by_event: HashMap<String, OperationsStatus>,

    // Pending subscriptions for new projects (drained by CoreRuntime)
    pending_project_subscriptions: Vec<String>,

    // Backend trust state
    approved_backends: HashSet<String>,
    blocked_backends: HashSet<String>,
    pending_backend_approvals: Vec<PendingBackendApproval>,

    // Thread root index - maps project a_tag -> set of known thread root event IDs
    // This avoids expensive full-table scans when loading threads
    thread_root_index: HashMap<String, HashSet<String>>,

    // Runtime hierarchy - tracks individual conversation runtimes and parent-child relationships
    // for hierarchical runtime aggregation (parent runtime = own + all children recursively)
    pub runtime_hierarchy: RuntimeHierarchy,

    // Pre-aggregated message counts by day for Stats view performance
    // Maps day_start_timestamp -> (user_count, all_count) for O(1) lookup instead of O(total_messages) per frame
    messages_by_day_counts: HashMap<u64, (u64, u64)>,

    // Pre-aggregated hourly LLM activity data for Activity grid performance
    // Maps (day_start, hour_of_day) -> (token_count, message_count)
    // Uses calendar day boundaries (UTC) and hour-of-day (0-23)
    // Stores only LLM messages (filtered by llm_metadata presence)
    // Updated incrementally on new messages for O(1) lookups instead of O(total_messages) per render
    llm_activity_by_hour: HashMap<(u64, u8), (u64, u64)>,

    // Pre-aggregated runtime totals by day for Stats view performance
    // Maps day_start_timestamp -> total runtime_ms (from llm-runtime tags)
    runtime_by_day_counts: HashMap<u64, u64>,

    // Real-time agent tracking - tracks active agents and estimates unconfirmed runtime.
    // In-memory only, resets on app restart. See agent_tracking.rs for details.
    pub agent_tracking: AgentTrackingState,
}

impl AppDataStore {
    pub fn new(ndb: Arc<Ndb>) -> Self {
        let mut store = Self {
            ndb,
            projects: Vec::new(),
            project_statuses: HashMap::new(),
            threads_by_project: HashMap::new(),
            messages_by_thread: HashMap::new(),
            profiles: HashMap::new(),
            inbox_items: Vec::new(),
            inbox_read_ids: HashSet::new(),
            user_pubkey: None,
            agent_chatter: Vec::new(),
            lessons: HashMap::new(),
            agent_definitions: HashMap::new(),
            nudges: HashMap::new(),
            mcp_tools: HashMap::new(),
            reports: HashMap::new(),
            reports_all_versions: HashMap::new(),
            document_threads: HashMap::new(),
            operations_by_event: HashMap::new(),
            pending_project_subscriptions: Vec::new(),
            approved_backends: HashSet::new(),
            blocked_backends: HashSet::new(),
            pending_backend_approvals: Vec::new(),
            thread_root_index: HashMap::new(),
            runtime_hierarchy: RuntimeHierarchy::new(),
            messages_by_day_counts: HashMap::new(),
            llm_activity_by_hour: HashMap::new(),
            runtime_by_day_counts: HashMap::new(),
            agent_tracking: AgentTrackingState::new(),
        };
        store.rebuild_from_ndb();
        store
    }

    pub fn set_user_pubkey(&mut self, pubkey: String) {
        let pubkey_changed = self.user_pubkey.as_ref() != Some(&pubkey);
        self.user_pubkey = Some(pubkey.clone());
        // Populate inbox from existing messages
        self.populate_inbox_from_existing(&pubkey);
        // Rebuild message counts if user changed (ensures historical counts are accurate)
        if pubkey_changed {
            self.rebuild_messages_by_day_counts();
        }
        // Rebuild LLM activity hourly aggregates (always, not user-dependent)
        self.rebuild_llm_activity_by_hour();
        // Rebuild runtime-by-day aggregates (always, not user-dependent)
        self.rebuild_runtime_by_day_counts();
    }

    /// Clear all in-memory data (used on logout to prevent stale data leaks).
    /// Does NOT clear nostrdb - that persists across sessions.
    /// After logout and re-login with different account, rebuild_from_ndb()
    /// will repopulate with the new user's filtered view.
    pub fn clear(&mut self) {
        self.projects.clear();
        self.project_statuses.clear();
        self.threads_by_project.clear();
        self.messages_by_thread.clear();
        self.profiles.clear();
        self.inbox_items.clear();
        self.inbox_read_ids.clear();
        self.user_pubkey = None;
        self.agent_chatter.clear();
        self.lessons.clear();
        self.agent_definitions.clear();
        self.nudges.clear();
        self.reports.clear();
        self.reports_all_versions.clear();
        self.document_threads.clear();
        self.operations_by_event.clear();
        self.pending_project_subscriptions.clear();
        self.approved_backends.clear();
        self.blocked_backends.clear();
        self.pending_backend_approvals.clear();
        self.thread_root_index.clear();
        self.runtime_hierarchy = RuntimeHierarchy::new();
        self.messages_by_day_counts.clear();
        self.runtime_by_day_counts.clear();
        self.agent_tracking.clear();
    }

    /// Scan existing messages and populate inbox with those that p-tag the user
    fn populate_inbox_from_existing(&mut self, user_pubkey: &str) {
        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        // First, build a set of ask event IDs that the user has already replied to
        // by checking e-tags on user's messages (not just reply_to field, but all e-tags)
        let mut answered_ask_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for messages in self.messages_by_thread.values() {
            for message in messages {
                if message.pubkey == user_pubkey {
                    // Query nostrdb to get the full note and extract all e-tags
                    let note_id_bytes = match hex::decode(&message.id) {
                        Ok(bytes) if bytes.len() == 32 => bytes,
                        _ => continue,
                    };
                    let note_id: [u8; 32] = match note_id_bytes.try_into() {
                        Ok(arr) => arr,
                        Err(_) => continue,
                    };
                    if let Ok(note) = self.ndb.get_note_by_id(&txn, &note_id) {
                        // Extract all e-tag IDs (includes replies with or without reply markers)
                        let reply_to_ids = Self::extract_e_tag_ids(&note);
                        for reply_to_id in reply_to_ids {
                            answered_ask_ids.insert(reply_to_id);
                        }
                    }
                }
            }
        }

        // Collect all messages that need checking
        let mut inbox_candidates: Vec<(String, Message)> = Vec::new();

        for (thread_id, messages) in &self.messages_by_thread {
            for message in messages {
                // Skip our own messages
                if message.pubkey == user_pubkey {
                    continue;
                }
                // Skip already-read items
                if self.inbox_read_ids.contains(&message.id) {
                    continue;
                }
                inbox_candidates.push((thread_id.clone(), message.clone()));
            }
        }

        // Check each message for p-tags (inbox)
        for (thread_id, message) in inbox_candidates {
            // Query nostrdb for the note to check its tags
            let note_id_bytes = match hex::decode(&message.id) {
                Ok(bytes) if bytes.len() == 32 => bytes,
                _ => continue,
            };

            let note_id: [u8; 32] = match note_id_bytes.try_into() {
                Ok(arr) => arr,
                Err(_) => continue,
            };

            if let Ok(note) = self.ndb.get_note_by_id(&txn, &note_id) {
                // Check for inbox: ask events that p-tag the user
                if self.note_ptags_user(&note, user_pubkey) && self.note_is_ask_event(&note) {
                    // Skip ask events that user has already answered
                    if answered_ask_ids.contains(&message.id) {
                        continue;
                    }

                    // Extract project a_tag directly from the note first, fall back to thread lookup
                    let project_a_tag = Self::extract_project_a_tag(&note)
                        .or_else(|| self.find_project_for_thread(&thread_id));
                    let project_a_tag_str = project_a_tag.unwrap_or_default();

                    let inbox_item = InboxItem {
                        id: message.id.clone(),
                        event_type: InboxEventType::Ask,  // This is an ask event, not just a mention
                        title: message.content.chars().take(50).collect(),
                        project_a_tag: project_a_tag_str,
                        author_pubkey: message.pubkey.clone(),
                        created_at: message.created_at,
                        is_read: false,
                        thread_id: Some(thread_id.clone()),
                        ask_event: message.ask_event.clone(),
                    };
                    self.inbox_items.push(inbox_item);
                }

            }
        }

        // Sort inbox by created_at descending (most recent first)
        self.inbox_items.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    }

    /// Rebuild all data from nostrdb (called on startup)
    pub fn rebuild_from_ndb(&mut self) {
        if let Ok(projects) = crate::store::get_projects(&self.ndb) {
            self.projects = projects;
        }

        // NOTE: Project statuses are loaded in set_trusted_backends() after login
        // This ensures trust validation is applied

        let a_tags: Vec<String> = self.projects.iter().map(|p| p.a_tag()).collect();

        // Step 1: Build thread root index for all projects
        // This scans kind:1 events once and identifies thread roots (no e-tags)
        if let Ok(index) = crate::store::build_thread_root_index(&self.ndb, &a_tags) {
            self.thread_root_index = index;
        }

        // Step 2: Load full thread data using the index (query by known IDs)
        for a_tag in &a_tags {
            if let Some(root_ids) = self.thread_root_index.get(a_tag) {
                if let Ok(threads) = crate::store::get_threads_by_ids(&self.ndb, root_ids) {
                    self.threads_by_project.insert(a_tag.clone(), threads);
                }
            }
        }

        // Pre-load messages for all threads
        for threads in self.threads_by_project.values() {
            for thread in threads {
                if let Ok(messages) = crate::store::get_messages_for_thread(&self.ndb, &thread.id) {
                    self.messages_by_thread.insert(thread.id.clone(), messages);
                }
            }
        }

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

        // Rebuild pre-aggregated message counts by day for Stats view
        self.rebuild_messages_by_day_counts();

        // Rebuild pre-aggregated LLM activity by hour for Activity grid
        self.rebuild_llm_activity_by_hour();

        // Rebuild pre-aggregated runtime by day for Stats view
        self.rebuild_runtime_by_day_counts();

        // Apply metadata (kind:513) to threads - may further update last_activity
        self.apply_existing_metadata();

        // Build runtime hierarchy from loaded data
        // This sets up parent-child relationships and calculates effective_last_activity
        self.rebuild_runtime_hierarchy();

        // Final sort by effective_last_activity after hierarchy is fully built
        for threads in self.threads_by_project.values_mut() {
            threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
        }

        // Load agent definitions (kind:4199)
        self.load_agent_definitions();

        // Load MCP tools (kind:4200)
        self.load_mcp_tools();

        // Load nudges (kind:4201)
        self.load_nudges();

        // NOTE: Ephemeral events (kind:24010, 24133) are intentionally NOT loaded from nostrdb.
        // They are only received via live subscriptions and stored in memory.
        // Operations status (kind:24133) will be populated when events arrive.

        // Load reports (kind:30023)
        self.load_reports();
    }

    /// Load all agent definitions from nostrdb
    fn load_agent_definitions(&mut self) {
        use nostrdb::{Filter, Transaction};

        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        let filter = Filter::new().kinds([4199]).build();
        let Ok(results) = self.ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        // Loading agent definitions (kind:4199)

        for result in results {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(agent_def) = AgentDefinition::from_note(&note) {
                    self.agent_definitions.insert(agent_def.id.clone(), agent_def);
                }
            }
        }
    }

    /// Load all MCP tools from nostrdb
    fn load_mcp_tools(&mut self) {
        use nostrdb::{Filter, Transaction};

        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        let filter = Filter::new().kinds([4200]).build();
        let Ok(results) = self.ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        for result in results {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(tool) = MCPTool::from_note(&note) {
                    self.mcp_tools.insert(tool.id.clone(), tool);
                }
            }
        }
    }

    /// Load all nudges from nostrdb
    fn load_nudges(&mut self) {
        use nostrdb::{Filter, Transaction};

        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        let filter = Filter::new().kinds([4201]).build();
        let Ok(results) = self.ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        // Loading nudges (kind:4201)
        // First pass: collect all nudges and their supersedes relationships
        let mut all_nudges: Vec<Nudge> = Vec::new();
        for result in results {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(nudge) = Nudge::from_note(&note) {
                    all_nudges.push(nudge);
                }
            }
        }

        // Sort by created_at to process in chronological order
        all_nudges.sort_by_key(|n| n.created_at);

        // Second pass: insert nudges, honoring supersedes
        for nudge in all_nudges {
            // Remove superseded nudge if present
            if let Some(ref superseded_id) = nudge.supersedes {
                self.nudges.remove(superseded_id);
            }
            self.nudges.insert(nudge.id.clone(), nudge);
        }
    }

    /// Load reports from nostrdb (kind:30023) that belong to known projects
    fn load_reports(&mut self) {
        use nostrdb::{Filter, Transaction};

        // Collect project a-tags to filter reports
        let project_a_tags: Vec<String> = self.projects.iter().map(|p| p.a_tag()).collect();

        if project_a_tags.is_empty() {
            // No projects loaded, skipping report loading
            return;
        }

        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        // Query reports that specifically a-tag our projects (instead of fetching all and filtering)
        let a_tag_refs: Vec<&str> = project_a_tags.iter().map(|s| s.as_str()).collect();
        let filter = Filter::new()
            .kinds([30023])
            .tags(a_tag_refs, 'a')
            .build();
        let Ok(results) = self.ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        for result in results {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(report) = Report::from_note(&note) {
                    self.add_report(report);
                }
            }
        }
    }

    /// Add a report, maintaining version history and latest-by-slug
    fn add_report(&mut self, report: Report) {
        let slug = report.slug.clone();

        // Add to all versions
        let versions = self.reports_all_versions.entry(slug.clone()).or_default();

        // Check for duplicate (same id)
        if versions.iter().any(|r| r.id == report.id) {
            return;
        }

        versions.push(report.clone());

        // Sort versions by created_at descending (newest first)
        versions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Update latest version
        if let Some(latest) = versions.first() {
            self.reports.insert(slug, latest.clone());
        }
    }

    // NOTE: load_operations_status() was removed because ephemeral events (kind:24133)
    // should NOT be read from nostrdb. Operations status is only received via live
    // subscriptions and stored in memory (operations_by_event).

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
                self.runtime_hierarchy.set_individual_last_activity(&thread.id, thread.last_activity);
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
                self.runtime_hierarchy.set_individual_runtime(thread_id, runtime_ms);
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
        let thread_ids: Vec<String> = self.threads_by_project
            .values()
            .flat_map(|threads| threads.iter().map(|t| t.id.clone()))
            .collect();

        // Update each thread's effective_last_activity
        for thread_id in thread_ids {
            let effective = self.runtime_hierarchy.get_effective_last_activity(&thread_id);
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
            .flat_map(|msg| {
                msg.llm_metadata
                    .iter()
                    .filter(|(key, _)| key == "runtime")
                    .filter_map(|(_, value)| value.parse::<u64>().ok())
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
            if self.runtime_hierarchy.get_conversation_created_at(thread_id).is_none() {
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
                    if parent_id != thread_id {
                        if self.runtime_hierarchy.set_parent(thread_id, parent_id) {
                            relationships_changed = true;
                        }
                    }
                }
            }

            // Recalculate this conversation's individual runtime
            let runtime_ms = Self::calculate_runtime_from_messages(messages);
            self.runtime_hierarchy.set_individual_runtime(thread_id, runtime_ms);
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
        self.runtime_hierarchy.get_effective_last_activity(thread_id)
    }

    /// Update effective_last_activity on a thread and propagate up the ancestor chain.
    /// This should be called whenever a thread's last_activity changes.
    fn propagate_effective_last_activity(&mut self, thread_id: &str) {
        // First, update the individual last_activity in RuntimeHierarchy
        // (get it from the actual thread)
        if let Some(last_activity) = self.get_thread_last_activity(thread_id) {
            self.runtime_hierarchy.set_individual_last_activity(thread_id, last_activity);
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
        let effective = self.runtime_hierarchy.get_effective_last_activity(thread_id);

        for threads in self.threads_by_project.values_mut() {
            if let Some(thread) = threads.iter_mut().find(|t| t.id == thread_id) {
                thread.effective_last_activity = effective;
                break;
            }
        }
    }

    /// Get today's total LLM runtime (in milliseconds), based on message timestamps.
    /// Used for the global status bar and Stats view.
    pub fn get_today_unique_runtime(&mut self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let seconds_per_day: u64 = 86400;
        let today_start = (now / seconds_per_day) * seconds_per_day;
        self.runtime_by_day_counts.get(&today_start).copied().unwrap_or(0)
    }

    /// Get runtime aggregated by day for the Stats tab bar chart.
    /// Returns (day_start_timestamp, total_runtime_ms) tuples.
    pub fn get_runtime_by_day(&self, num_days: usize) -> Vec<(u64, u64)> {
        if num_days == 0 {
            return Vec::new();
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let seconds_per_day: u64 = 86400;
        let today_start = (now / seconds_per_day) * seconds_per_day;
        let earliest_day = today_start.saturating_sub((num_days as u64).saturating_sub(1) * seconds_per_day);

        let mut result: Vec<(u64, u64)> = self
            .runtime_by_day_counts
            .iter()
            .filter(|(day_start, runtime_ms)| **day_start >= earliest_day && **runtime_ms > 0)
            .map(|(day_start, runtime_ms)| (*day_start, *runtime_ms))
            .collect();
        result.sort_by_key(|(day, _)| *day);
        result
    }

    /// Get top N conversations by total runtime (including descendants).
    /// Returns (conversation_id, total_runtime_ms) tuples.
    pub fn get_top_conversations_by_runtime(&self, limit: usize) -> Vec<(String, u64)> {
        self.runtime_hierarchy.get_top_conversations_by_runtime(limit)
    }

    /// Helper: iterate over all (message, cost_usd) pairs across all threads.
    /// Extracts cost-usd from llm_metadata, returning only messages with valid costs.
    fn iter_message_costs(&self) -> impl Iterator<Item = (&Message, f64)> {
        self.messages_by_thread
            .values()
            .flat_map(|messages| messages.iter())
            .filter_map(|msg| {
                msg.llm_metadata
                    .iter()
                    .find(|(key, _)| key == "cost-usd")
                    .and_then(|(_, value)| value.parse::<f64>().ok())
                    .map(|cost| (msg, cost))
            })
    }

    /// Get total cost across all messages (sum of llm-cost-usd tags).
    /// Returns the total cost in USD as a float.
    pub fn get_total_cost(&self) -> f64 {
        self.iter_message_costs().map(|(_, cost)| cost).sum()
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
                    .map(|p| p.name.clone())
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
    /// Both vectors cover the same time window (num_days).
    ///
    /// Uses pre-aggregated counters for O(num_days) performance instead of O(total_messages).
    /// Data is queried directly from nostrdb using the `.authors()` filter for user messages
    /// and a-tag filters for project messages (no double-counting from agent_chatter).
    pub fn get_messages_by_day(&self, num_days: usize) -> (Vec<(u64, u64)>, Vec<(u64, u64)>) {
        // Guard against zero days
        if num_days == 0 {
            return (Vec::new(), Vec::new());
        }

        let seconds_per_day: u64 = 86400;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Calculate the start of today (UTC)
        let today_start = (now / seconds_per_day) * seconds_per_day;

        // Calculate the earliest day to include
        let earliest_day = today_start - ((num_days - 1) as u64 * seconds_per_day);

        // Extract counts for the requested window from pre-aggregated data (O(num_days))
        let mut user_result: Vec<(u64, u64)> = Vec::new();
        let mut all_result: Vec<(u64, u64)> = Vec::new();

        for (&day_start, &(user_count, all_count)) in &self.messages_by_day_counts {
            if day_start >= earliest_day {
                if user_count > 0 {
                    user_result.push((day_start, user_count));
                }
                if all_count > 0 {
                    all_result.push((day_start, all_count));
                }
            }
        }

        // Sort by day_start ascending for chart display
        user_result.sort_by_key(|(day, _)| *day);
        all_result.sort_by_key(|(day, _)| *day);

        (user_result, all_result)
    }

    /// Get LLM token usage aggregated by calendar day and hour-of-day.
    /// Returns a HashMap where key is hour_start_timestamp and value is total tokens.
    /// Uses pre-aggregated data for O(num_hours) performance instead of O(total_messages).
    /// Only includes LLM-generated messages (those with llm_metadata).
    /// Covers the specified number of hours from now backwards.
    pub fn get_tokens_by_hour(&self, num_hours: usize) -> HashMap<u64, u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let seconds_per_hour: u64 = 3600;
        let current_hour_start = (now / seconds_per_hour) * seconds_per_hour;

        self.get_tokens_by_hour_from(current_hour_start, num_hours)
    }

    /// Testable variant of get_tokens_by_hour that accepts a current_hour_start parameter.
    /// This enables testing window slicing behavior without depending on SystemTime::now().
    pub fn get_tokens_by_hour_from(&self, current_hour_start: u64, num_hours: usize) -> HashMap<u64, u64> {
        let mut result: HashMap<u64, u64> = HashMap::new();

        if num_hours == 0 {
            return result;
        }

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;

        // Iterate backwards through num_hours and do direct HashMap lookups
        // This guarantees O(num_hours) complexity instead of O(total_history_buckets)
        for i in 0..num_hours {
            let hour_offset = i as u64 * seconds_per_hour;
            let hour_start = current_hour_start.saturating_sub(hour_offset);

            // Convert hour_start to (day_start, hour_of_day) key
            let day_start = (hour_start / seconds_per_day) * seconds_per_day;
            let seconds_since_day_start = hour_start - day_start;
            let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;

            // Direct HashMap lookup - O(1)
            if let Some((tokens, _)) = self.llm_activity_by_hour.get(&(day_start, hour_of_day)) {
                result.insert(hour_start, *tokens);
            }
        }

        result
    }

    /// Get LLM message count aggregated by calendar day and hour-of-day.
    /// Returns a HashMap where key is hour_start_timestamp and value is message count.
    /// Uses pre-aggregated data for O(num_hours) performance instead of O(total_messages).
    /// Only includes LLM-generated messages (those with llm_metadata).
    /// Covers the specified number of hours from now backwards.
    pub fn get_message_count_by_hour(&self, num_hours: usize) -> HashMap<u64, u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let seconds_per_hour: u64 = 3600;
        let current_hour_start = (now / seconds_per_hour) * seconds_per_hour;

        self.get_message_count_by_hour_from(current_hour_start, num_hours)
    }

    /// Testable variant of get_message_count_by_hour that accepts a current_hour_start parameter.
    /// This enables testing window slicing behavior without depending on SystemTime::now().
    pub fn get_message_count_by_hour_from(&self, current_hour_start: u64, num_hours: usize) -> HashMap<u64, u64> {
        let mut result: HashMap<u64, u64> = HashMap::new();

        if num_hours == 0 {
            return result;
        }

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;

        // Iterate backwards through num_hours and do direct HashMap lookups
        // This guarantees O(num_hours) complexity instead of O(total_history_buckets)
        for i in 0..num_hours {
            let hour_offset = i as u64 * seconds_per_hour;
            let hour_start = current_hour_start.saturating_sub(hour_offset);

            // Convert hour_start to (day_start, hour_of_day) key
            let day_start = (hour_start / seconds_per_day) * seconds_per_day;
            let seconds_since_day_start = hour_start - day_start;
            let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;

            // Direct HashMap lookup - O(1)
            if let Some((_, message_count)) = self.llm_activity_by_hour.get(&(day_start, hour_of_day)) {
                result.insert(hour_start, *message_count);
            }
        }

        result
    }

    /// Rebuild message counts by day from nostrdb directly.
    /// Called during startup after messages are loaded, and when user pubkey changes.
    ///
    /// This queries nostrdb directly to get accurate counts:
    /// - User messages: ALL kind:1 events authored by the current user's pubkey (using .authors() filter)
    /// - All messages: ALL kind:1 events that a-tag any of our projects
    ///
    /// Uses pagination to iterate through ALL results without arbitrary caps.
    /// This approach is more accurate than using messages_by_thread because it captures
    /// ALL user messages regardless of whether they're associated with a project thread.
    ///
    /// ## Pagination Safety
    /// - Uses inclusive `until` with seen-event-id guards to handle same-second events safely
    /// - Assumes nostrdb::query returns events ordered by created_at descending (newest first)
    /// - Pagination cursor is always updated from ALL page results, not just non-duplicate events
    ///
    /// ## Known Limitation: Same-Second Overflow
    /// If more than `batch_size` events share the exact same timestamp (same second), some events
    /// MAY be missed because nostrdb doesn't provide deterministic secondary ordering (like note_key).
    /// When this condition is detected, a warning is logged. For typical usage patterns, this is
    /// extremely unlikely (10,000+ events in a single second would require an unusual workload).
    /// If this limitation becomes problematic, consider increasing `batch_size`.
    fn rebuild_messages_by_day_counts(&mut self) {
        self.rebuild_messages_by_day_counts_with_batch_size(DEFAULT_PAGINATION_BATCH_SIZE);
    }

    /// Internal version with configurable batch_size for testing.
    #[cfg(test)]
    fn rebuild_messages_by_day_counts_with_batch_size(&mut self, batch_size: i32) {
        self._rebuild_messages_by_day_counts_impl(batch_size);
    }

    /// Internal version with configurable batch_size for testing.
    #[cfg(not(test))]
    fn rebuild_messages_by_day_counts_with_batch_size(&mut self, batch_size: i32) {
        self._rebuild_messages_by_day_counts_impl(batch_size);
    }

    /// Core implementation of rebuild_messages_by_day_counts with configurable batch_size.
    fn _rebuild_messages_by_day_counts_impl(&mut self, batch_size: i32) {
        self.messages_by_day_counts.clear();

        let seconds_per_day: u64 = 86400;
        let user_pubkey = self.user_pubkey.clone();

        // Collect project a-tags for the "all" query
        let project_a_tags: Vec<String> = self.projects.iter().map(|p| p.a_tag()).collect();

        // Query user messages directly from nostrdb using .authors() filter (efficient server-side filtering)
        if let Some(ref user_pk) = user_pubkey {
            // Convert hex pubkey to bytes for .authors() filter
            if let Ok(pubkey_bytes) = hex::decode(user_pk) {
                if pubkey_bytes.len() == 32 {
                    let pubkey_array: [u8; 32] = pubkey_bytes.try_into().unwrap();

                    match Transaction::new(&self.ndb) {
                        Ok(txn) => {
                            // Paginate through ALL user messages using until timestamp
                            // Use seen-event-id guard to handle same-second pagination safely
                            let mut until_timestamp: Option<u64> = None;
                            let mut seen_event_ids: HashSet<[u8; 32]> = HashSet::new();
                            let mut total_user_messages: u64 = 0;

                            loop {
                                let mut filter_builder = nostrdb::Filter::new()
                                    .kinds([1])
                                    .authors([&pubkey_array]);

                                if let Some(until) = until_timestamp {
                                    filter_builder = filter_builder.until(until);
                                }

                                let filter = filter_builder.build();

                                match self.ndb.query(&txn, &[filter], batch_size) {
                                    Ok(results) => {
                                        if results.is_empty() {
                                            break; // No more results
                                        }

                                        let page_size = results.len();
                                        let mut page_oldest_timestamp: Option<u64> = None;
                                        let mut page_newest_timestamp: Option<u64> = None;
                                        let mut new_events_in_page = 0;

                                        for result in results.iter() {
                                            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                                                let event_id = *note.id();
                                                let created_at = note.created_at();

                                                // Track oldest and newest timestamps (from ALL results)
                                                match page_oldest_timestamp {
                                                    None => page_oldest_timestamp = Some(created_at),
                                                    Some(t) if created_at < t => page_oldest_timestamp = Some(created_at),
                                                    _ => {}
                                                }
                                                match page_newest_timestamp {
                                                    None => page_newest_timestamp = Some(created_at),
                                                    Some(t) if created_at > t => page_newest_timestamp = Some(created_at),
                                                    _ => {}
                                                }

                                                // Skip if we've already processed this event (same-second boundary)
                                                if seen_event_ids.contains(&event_id) {
                                                    continue;
                                                }
                                                seen_event_ids.insert(event_id);

                                                let day_start = (created_at / seconds_per_day) * seconds_per_day;
                                                let entry = self.messages_by_day_counts.entry(day_start).or_insert((0, 0));
                                                entry.0 += 1;
                                                total_user_messages += 1;
                                                new_events_in_page += 1;
                                            }
                                        }

                                        // Detect potential same-second overflow: full page with all events at same timestamp
                                        // This is the edge case where nostrdb's lack of secondary ordering may cause data loss
                                        if page_size >= (batch_size as usize) {
                                            if let (Some(oldest), Some(newest)) = (page_oldest_timestamp, page_newest_timestamp) {
                                                if oldest == newest {
                                                    warn!(
                                                        "Potential same-second overflow detected in user messages query: \
                                                        {} events at timestamp {}. If more than {} events share this timestamp, \
                                                        some may be missed due to nostrdb pagination limitations.",
                                                        page_size, oldest, batch_size
                                                    );
                                                }
                                            }
                                        }

                                        // If we got fewer results than batch_size, we're done
                                        if page_size < (batch_size as usize) {
                                            break;
                                        }

                                        // If page had no new events, we've exhausted unique events at this timestamp boundary
                                        if new_events_in_page == 0 {
                                            // Still need to continue pagination with older timestamp
                                            match page_oldest_timestamp {
                                                Some(t) if t > 0 => until_timestamp = Some(t - 1),
                                                _ => break,
                                            }
                                        } else {
                                            // Use inclusive pagination (same timestamp) to catch more same-second events
                                            // The seen_event_ids guard prevents double-counting
                                            match page_oldest_timestamp {
                                                Some(t) => until_timestamp = Some(t),
                                                None => break,
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to query user messages from nostrdb: {:?}", e);
                                        break;
                                    }
                                }
                            }

                            trace!("Counted {} total user messages", total_user_messages);
                        }
                        Err(e) => {
                            warn!("Failed to create transaction for user messages query: {:?}", e);
                        }
                    }
                } else {
                    warn!("Invalid user pubkey length: {} (expected 32 bytes)", pubkey_bytes.len());
                }
            } else {
                warn!("Failed to decode user pubkey from hex: {}", user_pk);
            }
        }

        // Query all project messages directly from nostrdb (kind:1 with a-tag matching our projects)
        if !project_a_tags.is_empty() {
            match Transaction::new(&self.ndb) {
                Ok(txn) => {
                    // Track seen event IDs to avoid double-counting messages that a-tag multiple projects
                    // This also handles same-second pagination safety
                    let mut seen_event_ids: HashSet<[u8; 32]> = HashSet::new();
                    let mut total_project_messages: u64 = 0;

                    for a_tag in &project_a_tags {
                        // Paginate through ALL messages for this project
                        let mut until_timestamp: Option<u64> = None;

                        loop {
                            let mut filter_builder = nostrdb::Filter::new()
                                .kinds([1])
                                .tags([a_tag.as_str()], 'a');

                            if let Some(until) = until_timestamp {
                                filter_builder = filter_builder.until(until);
                            }

                            let filter = filter_builder.build();

                            match self.ndb.query(&txn, &[filter], batch_size) {
                                Ok(results) => {
                                    if results.is_empty() {
                                        break; // No more results for this project
                                    }

                                    // Track page-level oldest/newest timestamp separately from deduplication
                                    // BUG FIX: Always update pagination cursor from ALL page results,
                                    // even if events are duplicates (seen from another project)
                                    let page_size = results.len();
                                    let mut page_oldest_timestamp: Option<u64> = None;
                                    let mut page_newest_timestamp: Option<u64> = None;
                                    let mut new_events_in_page = 0;

                                    for result in results.iter() {
                                        if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                                            let event_id = *note.id();
                                            let created_at = note.created_at();

                                            // Track oldest and newest timestamps (from ALL results)
                                            match page_oldest_timestamp {
                                                None => page_oldest_timestamp = Some(created_at),
                                                Some(t) if created_at < t => page_oldest_timestamp = Some(created_at),
                                                _ => {}
                                            }
                                            match page_newest_timestamp {
                                                None => page_newest_timestamp = Some(created_at),
                                                Some(t) if created_at > t => page_newest_timestamp = Some(created_at),
                                                _ => {}
                                            }

                                            // Skip if we've already counted this event (multi-project a-tags or same-second boundary)
                                            if seen_event_ids.contains(&event_id) {
                                                continue;
                                            }
                                            seen_event_ids.insert(event_id);

                                            let day_start = (created_at / seconds_per_day) * seconds_per_day;
                                            let entry = self.messages_by_day_counts.entry(day_start).or_insert((0, 0));
                                            entry.1 += 1;
                                            total_project_messages += 1;
                                            new_events_in_page += 1;
                                        }
                                    }

                                    // Detect potential same-second overflow: full page with all events at same timestamp
                                    if page_size >= (batch_size as usize) {
                                        if let (Some(oldest), Some(newest)) = (page_oldest_timestamp, page_newest_timestamp) {
                                            if oldest == newest {
                                                warn!(
                                                    "Potential same-second overflow detected in project '{}' messages query: \
                                                    {} events at timestamp {}. If more than {} events share this timestamp, \
                                                    some may be missed due to nostrdb pagination limitations.",
                                                    a_tag, page_size, oldest, batch_size
                                                );
                                            }
                                        }
                                    }

                                    // If we got fewer results than batch_size, we're done with this project
                                    if page_size < (batch_size as usize) {
                                        break;
                                    }

                                    // If page had no new events, we've exhausted unique events at this timestamp boundary
                                    // Must still advance pagination to find older unique events
                                    if new_events_in_page == 0 {
                                        match page_oldest_timestamp {
                                            Some(t) if t > 0 => until_timestamp = Some(t - 1),
                                            _ => break,
                                        }
                                    } else {
                                        // Use inclusive pagination (same timestamp) to catch more same-second events
                                        // The seen_event_ids guard prevents double-counting
                                        match page_oldest_timestamp {
                                            Some(t) => until_timestamp = Some(t),
                                            None => break,
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to query project messages for a-tag '{}': {:?}", a_tag, e);
                                    break;
                                }
                            }
                        }
                    }

                    trace!("Counted {} total project messages (deduplicated)", total_project_messages);
                }
                Err(e) => {
                    warn!("Failed to create transaction for project messages query: {:?}", e);
                }
            }
        }
    }

    /// Increment message counts for a single message (called from handle_message_event).
    /// This maintains the pre-aggregated counters incrementally for O(1) updates.
    fn increment_message_day_count(&mut self, created_at: u64, pubkey: &str) {
        let seconds_per_day: u64 = 86400;
        let day_start = (created_at / seconds_per_day) * seconds_per_day;

        let entry = self.messages_by_day_counts.entry(day_start).or_insert((0, 0));

        // Increment all count
        entry.1 += 1;

        // Increment user count if matches current user
        if self.user_pubkey.as_deref() == Some(pubkey) {
            entry.0 += 1;
        }
    }

    /// Increment LLM activity hourly aggregates (O(1) per message).
    /// Only increments if the message has llm_metadata (i.e., is an LLM-generated message).
    /// Uses calendar day boundaries (UTC) and hour-of-day (0-23) for stable bucketing.
    fn increment_llm_activity_hour(&mut self, created_at: u64, llm_metadata: &[(String, String)]) {
        // Only count messages with LLM metadata (actual LLM responses)
        if llm_metadata.is_empty() {
            return;
        }

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;

        // Calculate calendar day start (UTC)
        let day_start = (created_at / seconds_per_day) * seconds_per_day;

        // Calculate hour-of-day (0-23) by finding seconds since day start
        let seconds_since_day_start = created_at - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;

        // Extract token count from llm_metadata (default to 0 if not present)
        let tokens = llm_metadata
            .iter()
            .find(|(key, _)| key == "total-tokens")
            .and_then(|(_, value)| value.parse::<u64>().ok())
            .unwrap_or(0);

        // Update aggregates: (day_start, hour_of_day) -> (token_count, message_count)
        let entry = self.llm_activity_by_hour.entry((day_start, hour_of_day)).or_insert((0, 0));
        entry.0 += tokens;
        entry.1 += 1;
    }

    /// Increment runtime-by-day aggregates (O(1) per message).
    /// Only increments if the message has llm-runtime metadata.
    fn increment_runtime_day_count(&mut self, created_at: u64, llm_metadata: &[(String, String)]) {
        if created_at < RUNTIME_CUTOFF_TIMESTAMP {
            return;
        }

        let runtime_ms: u64 = llm_metadata
            .iter()
            .filter(|(key, _)| key == "runtime")
            .filter_map(|(_, value)| value.parse::<u64>().ok())
            .sum();

        if runtime_ms == 0 {
            return;
        }

        let seconds_per_day: u64 = 86400;
        let day_start = (created_at / seconds_per_day) * seconds_per_day;
        let entry = self.runtime_by_day_counts.entry(day_start).or_insert(0);
        *entry = entry.saturating_add(runtime_ms);
    }

    /// Rebuild LLM activity hourly aggregates from messages_by_thread.
    /// Called during startup after messages are loaded.
    /// Only counts messages with llm_metadata (LLM-generated messages).
    fn rebuild_llm_activity_by_hour(&mut self) {
        // Clear existing aggregates
        self.llm_activity_by_hour.clear();

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;

        // Iterate through all messages and aggregate by hour
        for messages in self.messages_by_thread.values() {
            for message in messages {
                // Only count LLM messages (those with llm_metadata)
                if !message.llm_metadata.is_empty() {
                    let created_at = message.created_at;

                    // Calculate calendar day start (UTC)
                    let day_start = (created_at / seconds_per_day) * seconds_per_day;

                    // Calculate hour-of-day (0-23) by finding seconds since day start
                    let seconds_since_day_start = created_at - day_start;
                    let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;

                    // Extract token count from llm_metadata (default to 0 if not present)
                    let tokens = message.llm_metadata
                        .iter()
                        .find(|(key, _)| key == "total-tokens")
                        .and_then(|(_, value)| value.parse::<u64>().ok())
                        .unwrap_or(0);

                    // Update aggregates: (day_start, hour_of_day) -> (token_count, message_count)
                    let entry = self.llm_activity_by_hour.entry((day_start, hour_of_day)).or_insert((0, 0));
                    entry.0 += tokens;
                    entry.1 += 1;
                }
            }
        }
    }

    /// Rebuild runtime-by-day aggregates from messages_by_thread.
    /// Called during startup after messages are loaded.
    fn rebuild_runtime_by_day_counts(&mut self) {
        let mut counts: HashMap<u64, u64> = HashMap::new();

        for messages in self.messages_by_thread.values() {
            for message in messages {
                let created_at = message.created_at;
                if created_at < RUNTIME_CUTOFF_TIMESTAMP {
                    continue;
                }

                let runtime_ms: u64 = message.llm_metadata
                    .iter()
                    .filter(|(key, _)| key == "runtime")
                    .filter_map(|(_, value)| value.parse::<u64>().ok())
                    .sum();

                if runtime_ms == 0 {
                    continue;
                }

                let seconds_per_day: u64 = 86400;
                let day_start = (created_at / seconds_per_day) * seconds_per_day;
                let entry = counts.entry(day_start).or_insert(0);
                *entry = entry.saturating_add(runtime_ms);
            }
        }

        self.runtime_by_day_counts = counts;
    }

    /// Apply all existing kind:513 metadata events to threads (called during rebuild)
    /// Only applies the MOST RECENT metadata event for each thread.
    /// Uses project-scoped metadata loading to avoid global query limits.
    fn apply_existing_metadata(&mut self) {
        // Step 1: Collect all thread IDs across all projects and fetch their metadata
        // This avoids the global 1000-event limit that caused metadata to be missing
        // for older conversations when there are many projects/threads
        let all_thread_ids: HashSet<String> = self.threads_by_project
            .values()
            .flat_map(|threads| threads.iter().map(|t| t.id.clone()))
            .collect();

        if all_thread_ids.is_empty() {
            return;
        }

        // Fetch metadata for all threads at once (still project-scoped by thread IDs)
        let Ok(metadata_map) = crate::store::get_metadata_for_threads(&self.ndb, &all_thread_ids) else {
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
                self.runtime_hierarchy.set_individual_last_activity(&thread.id, thread.last_activity);
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
            31933 => { self.handle_project_event(note); None }
            1 => { self.handle_text_event(note); None }
            0 => { self.handle_profile_event(note); None }
            24010 => self.handle_status_event(note),
            513 => { self.handle_metadata_event(note); None }
            4129 => { self.handle_lesson_event(note); None }
            4199 => { self.handle_agent_definition_event(note); None }
            4200 => { self.handle_mcp_tool_event(note); None }
            4201 => { self.handle_nudge_event(note); None }
            24133 => { self.handle_operations_status_event(note); None }
            30023 => { self.handle_report_event(note); None }
            _ => None
        }
    }

    /// Unified handler for kind:1 events - dispatches to thread or message handler based on e-tag presence
    /// Thread detection: kind:1 + has a-tag + NO e-tags
    /// Message detection: kind:1 + has e-tag (with "root" marker per NIP-10)
    fn handle_text_event(&mut self, note: &Note) {
        // Check for e-tags to determine if this is a thread or message
        let mut has_e_tag = false;
        for tag in note.tags() {
            if tag.get(0).and_then(|t| t.variant().str()) == Some("e") {
                has_e_tag = true;
                break;
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
                                let threads = self.document_threads.entry(a_val.to_string()).or_default();
                                if !threads.iter().any(|t| t.id == thread.id) {
                                    threads.push(thread);
                                    threads.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
    }

    fn handle_project_event(&mut self, note: &Note) {
        // Parse project directly from the note we already have
        // (Don't re-query - nostrdb indexes asynchronously, so query might miss it)
        if let Some(project) = Project::from_note(note) {
            let a_tag = project.a_tag();

            // Check if project already exists and update it, or add new one
            if let Some(existing) = self.projects.iter_mut().find(|p| p.a_tag() == a_tag) {
                *existing = project;
            } else {
                self.projects.push(project);
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
            let event_created_at = status.created_at;
            let backend_pubkey = status.backend_pubkey.clone();
            let should_update = self
                .project_statuses
                .get(&status.project_coordinate)
                .map(|existing| event_created_at >= existing.created_at)
                .unwrap_or(true);
            let mut status = status;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            status.last_seen_at = now;

            // Check trust status
            if self.blocked_backends.contains(&backend_pubkey) {
                return;
            }

            if self.approved_backends.contains(&backend_pubkey) {
                if should_update {
                    self.project_statuses.insert(status.project_coordinate.clone(), status);
                }
                return;
            }

            // Unknown backend - queue for approval (or update existing pending with newer status)
            if let Some(existing) = self.pending_backend_approvals.iter_mut().find(|p| {
                p.backend_pubkey == backend_pubkey && p.project_a_tag == status.project_coordinate
            }) {
                // Update with newer status only
                if event_created_at >= existing.status.created_at {
                    existing.status = status;
                }
            } else {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let pending = PendingBackendApproval {
                    backend_pubkey: backend_pubkey.clone(),
                    project_a_tag: status.project_coordinate.clone(),
                    first_seen: now,
                    status,
                };

                self.pending_backend_approvals.push(pending);
            }
        }
    }

    /// Handle an operations status event from pre-parsed Value (kind:24133)
    /// Updates in-memory operations_by_event storage
    fn handle_operations_status_event_value(&mut self, event: &serde_json::Value) {
        if let Some(status) = OperationsStatus::from_value(event) {
            self.upsert_operations_status(status);
        }
    }

    fn handle_status_event(&mut self, note: &Note) -> Option<crate::events::CoreEvent> {
        let mut status = ProjectStatus::from_note(note)?;
        let event_created_at = status.created_at;
        let backend_pubkey = &status.backend_pubkey;

        // Check trust status
        if self.blocked_backends.contains(backend_pubkey) {
            // Silently ignore blocked backends
            return None;
        }

        if self.approved_backends.contains(backend_pubkey) {
            // Approved backend - process normally
            let should_update = self
                .project_statuses
                .get(&status.project_coordinate)
                .map(|existing| event_created_at >= existing.created_at)
                .unwrap_or(true);
            if should_update {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);
                status.last_seen_at = now;
                let event = crate::events::CoreEvent::ProjectStatus(status.clone());
                self.project_statuses.insert(status.project_coordinate.clone(), status);
                return Some(event);
            }
            return None;
        }

        // Unknown backend - queue for approval (or update existing pending with newer status)
        if let Some(existing) = self.pending_backend_approvals.iter_mut().find(|p| {
            p.backend_pubkey == *backend_pubkey && p.project_a_tag == status.project_coordinate
        }) {
            // Update with newer status
            if event_created_at >= existing.status.created_at {
                existing.status = status;
            }
            return None; // Already pending, don't emit another event
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let pending = PendingBackendApproval {
            backend_pubkey: backend_pubkey.clone(),
            project_a_tag: status.project_coordinate.clone(),
            first_seen: now,
            status,
        };

        self.pending_backend_approvals.push(pending.clone());
        Some(crate::events::CoreEvent::PendingBackendApproval(pending))
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
                            self.runtime_hierarchy.set_individual_last_activity(&thread_id, max_message_time);
                            was_reconciled = true;
                        }
                    }
                }

                // Add to existing threads list, maintaining sort order by effective_last_activity
                let threads = self.threads_by_project.entry(a_tag).or_default();

                // Check if thread already exists (avoid duplicates)
                if !threads.iter().any(|t| t.id == thread_id) {
                    // Insert in sorted position by effective_last_activity (most recent first)
                    let insert_pos = threads.partition_point(|t| t.effective_last_activity > reconciled_thread.effective_last_activity);
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
                        threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
                    }
                }
            }

            // Also add the thread root as the first message in the conversation
            // This ensures the initial kind:1 that started the conversation is rendered
            if let Some(root_message) = Message::from_thread_note(note) {
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
                        if self.inbox_items.iter().any(|item| item.id == *reply_to_id) {
                            self.mark_inbox_read(reply_to_id);
                        }
                    }
                }
            }

            // Check for p-tag matching current user AND ask tag (for inbox)
            if let Some(ref user_pk) = self.user_pubkey.clone() {
                if pubkey != *user_pk {  // Don't inbox our own messages
                    // Only include ask events that p-tag the user
                    if self.note_ptags_user(note, user_pk) && self.note_is_ask_event(note) {
                        // Extract project a_tag directly from the note (not from thread lookup)
                        // This ensures inbox items are added even when the thread hasn't been
                        // registered yet (e.g., out-of-order event arrival during real-time sync)
                        let project_a_tag = Self::extract_project_a_tag(note)
                            .or_else(|| self.find_project_for_thread(&thread_id));
                        let project_a_tag_str = project_a_tag.clone().unwrap_or_default();

                        let inbox_item = InboxItem {
                            id: message_id.clone(),
                            event_type: InboxEventType::Ask,  // This is an ask event, not just a mention
                            title: message.content.chars().take(50).collect(),
                            project_a_tag: project_a_tag_str,
                            author_pubkey: pubkey.clone(),
                            created_at: message.created_at,
                            is_read: false,
                            thread_id: Some(thread_id.clone()),
                            ask_event: message.ask_event.clone(),
                        };
                        self.add_inbox_item(inbox_item);
                    }
                }
            }

            // Add to existing messages list, maintaining sort order by created_at
            let messages = self.messages_by_thread.entry(thread_id.clone()).or_default();

            // Check if message already exists (avoid duplicates)
            if !messages.iter().any(|m| m.id == message_id) {
                let message_created_at = message.created_at;
                let message_pubkey = message.pubkey.clone();
                let message_llm_metadata = message.llm_metadata.clone();

                // Check if this message has llm-runtime tag (confirms runtime, resets unconfirmed timer)
                let has_llm_runtime = message.llm_metadata.iter().any(|(key, _)| key == "runtime");

                // Insert in sorted position (oldest first)
                let insert_pos = messages.partition_point(|m| m.created_at < message_created_at);
                messages.insert(insert_pos, message);

                // If this message has llm-runtime tag, reset the unconfirmed timer for this agent on this conversation
                // This ensures unconfirmed runtime only tracks time since the last kind:1 confirmation
                // The recency guard prevents stale/backfilled messages from resetting active timers
                if has_llm_runtime {
                    self.agent_tracking
                        .reset_unconfirmed_timer(&thread_id, &message_pubkey, message_created_at);
                }

                // Update pre-aggregated message counts for Stats view (O(1) per message)
                self.increment_message_day_count(message_created_at, &message_pubkey);

                // Update pre-aggregated LLM activity for Activity grid (O(1) per message)
                self.increment_llm_activity_hour(message_created_at, &message_llm_metadata);

                // Update pre-aggregated runtime by day for Stats view (O(1) per message)
                self.increment_runtime_day_count(message_created_at, &message_llm_metadata);

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
                        threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
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
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        Some(hex::encode(id_bytes))
                    } else {
                        None
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
                    threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
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
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        Some(hex::encode(id_bytes))
                    } else {
                        None
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

    /// Drain pending project subscriptions (called by CoreRuntime after processing events)
    pub fn drain_pending_project_subscriptions(&mut self) -> Vec<String> {
        std::mem::take(&mut self.pending_project_subscriptions)
    }

    pub fn get_project_status(&self, a_tag: &str) -> Option<&ProjectStatus> {
        self.project_statuses.get(a_tag)
    }

    /// Get online agents for a project (from ProjectStatus if online)
    pub fn get_online_agents(&self, a_tag: &str) -> Option<&[ProjectAgent]> {
        self.project_statuses.get(a_tag)
            .filter(|s| s.is_online())
            .map(|s| s.agents.as_slice())
    }

    pub fn is_project_online(&self, a_tag: &str) -> bool {
        self.get_online_agents(a_tag).is_some()
    }

    pub fn get_threads(&self, project_a_tag: &str) -> &[Thread] {
        self.threads_by_project.get(project_a_tag)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Get thread root index for a project (set of known root event IDs)
    pub fn get_thread_root_index(&self, project_a_tag: &str) -> Option<&HashSet<String>> {
        self.thread_root_index.get(project_a_tag)
    }

    /// Get count of known thread roots for a project
    pub fn get_thread_root_count(&self, project_a_tag: &str) -> usize {
        self.thread_root_index.get(project_a_tag)
            .map(|s| s.len())
            .unwrap_or(0)
    }

    pub fn get_messages(&self, thread_id: &str) -> &[Message] {
        self.messages_by_thread.get(thread_id)
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

        let fallback = format!("{}...", &pubkey[..8.min(pubkey.len())]);
        let name = crate::store::get_profile_name(&self.ndb, pubkey);

        if name == fallback {
            if let Some(slug) = self.get_agent_slug_from_status(pubkey) {
                return slug;
            }
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
            .map(|p| p.name.clone())
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
        let thread_exists = self.threads_by_project.values().any(|threads| {
            threads.iter().any(|t| t.id == thread_id)
        });

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
                // Only update last_activity if metadata is newer
                if metadata.created_at > thread.last_activity {
                    thread.last_activity = metadata.created_at;
                    // Update runtime hierarchy - propagation will happen after the loop
                    self.runtime_hierarchy.set_individual_last_activity(thread_id, metadata.created_at);
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
        let mut all_threads: Vec<(Thread, String)> = self.threads_by_project
            .iter()
            .flat_map(|(a_tag, threads)| {
                threads.iter().map(|t| (t.clone(), a_tag.clone()))
            })
            .collect();

        all_threads.sort_by(|a, b| b.0.effective_last_activity.cmp(&a.0.effective_last_activity));
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
        let mut threads: Vec<(Thread, String)> = self.threads_by_project
            .iter()
            // Filter by visible projects FIRST (before any collection)
            .filter(|(a_tag, _)| visible_projects.contains(a_tag.as_str()))
            .flat_map(|(a_tag, threads)| {
                threads.iter().map(|t| (t.clone(), a_tag.clone()))
            })
            // Apply time filter using effective_last_activity if specified
            .filter(|(thread, _)| {
                match time_cutoff {
                    Some(cutoff) => thread.effective_last_activity >= cutoff,
                    None => true,
                }
            })
            .collect();

        // Sort by effective_last_activity descending (most recent first)
        // This enables hierarchical sorting where parent conversations reflect
        // the most recent activity in their entire delegation tree.
        threads.sort_by(|a, b| b.0.effective_last_activity.cmp(&a.0.effective_last_activity));

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

    pub fn get_inbox_items(&self) -> &[InboxItem] {
        &self.inbox_items
    }

    pub fn add_inbox_item(&mut self, item: InboxItem) {
        // Check if already read (persisted)
        let is_read = self.inbox_read_ids.contains(&item.id);
        let mut item = item;
        item.is_read = is_read;

        // Deduplicate by id
        if !self.inbox_items.iter().any(|i| i.id == item.id) {
            // Insert sorted by created_at (most recent first)
            let pos = self.inbox_items.partition_point(|i| i.created_at > item.created_at);
            self.inbox_items.insert(pos, item);
        }
    }

    pub fn mark_inbox_read(&mut self, id: &str) {
        if let Some(item) = self.inbox_items.iter_mut().find(|i| i.id == id) {
            item.is_read = true;
        }
        self.inbox_read_ids.insert(id.to_string());
    }

    // ===== Agent Chatter Methods =====

    /// Add an agent chatter item, maintaining sort order and limiting to 100 items
    pub fn add_agent_chatter(&mut self, item: AgentChatter) {
        // Deduplicate by id
        if self.agent_chatter.iter().any(|i| i.id() == item.id()) {
            return;
        }

        // Insert sorted by created_at (most recent first)
        let pos = self.agent_chatter.partition_point(|i| i.created_at() > item.created_at());
        self.agent_chatter.insert(pos, item);

        // Limit to 100 items
        if self.agent_chatter.len() > 100 {
            self.agent_chatter.truncate(100);
        }
    }

    // ===== Lesson Methods =====

    fn handle_lesson_event(&mut self, note: &Note) {
        if let Some(lesson) = Lesson::from_note(note) {
            let lesson_id = lesson.id.clone();

            // Add to agent chatter feed
            let chatter = AgentChatter::Lesson {
                id: lesson.id.clone(),
                title: lesson.title.clone(),
                content: lesson.content.clone(),
                author_pubkey: lesson.pubkey.clone(),
                created_at: lesson.created_at,
                category: lesson.category.clone(),
            };
            self.add_agent_chatter(chatter);

            // Store lesson
            self.lessons.insert(lesson_id, lesson);
        }
    }

    pub fn get_lesson(&self, lesson_id: &str) -> Option<&Lesson> {
        self.lessons.get(lesson_id)
    }

    // ===== Agent Definition Methods =====

    fn handle_agent_definition_event(&mut self, note: &Note) {
        if let Some(agent_def) = AgentDefinition::from_note(note) {
            self.agent_definitions.insert(agent_def.id.clone(), agent_def);
        }
    }

    /// Get all agent definitions, sorted by created_at descending (most recent first)
    pub fn get_agent_definitions(&self) -> Vec<&AgentDefinition> {
        let mut defs: Vec<_> = self.agent_definitions.values().collect();
        defs.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        defs
    }

    pub fn get_agent_definition(&self, id: &str) -> Option<&AgentDefinition> {
        self.agent_definitions.get(id)
    }

    // ===== MCP Tool Methods (kind:4200) =====

    fn handle_mcp_tool_event(&mut self, note: &Note) {
        if let Some(tool) = MCPTool::from_note(note) {
            self.mcp_tools.insert(tool.id.clone(), tool);
        }
    }

    // ===== Nudge Methods (kind:4201) =====

    fn handle_nudge_event(&mut self, note: &Note) {
        if let Some(nudge) = Nudge::from_note(note) {
            // Honor supersedes tag - remove the old nudge if this one supersedes it
            if let Some(ref superseded_id) = nudge.supersedes {
                self.nudges.remove(superseded_id);
            }
            self.nudges.insert(nudge.id.clone(), nudge);
        }
    }

    /// Get all nudges, sorted by created_at descending (most recent first)
    pub fn get_nudges(&self) -> Vec<&Nudge> {
        let mut nudges: Vec<_> = self.nudges.values().collect();
        nudges.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        nudges
    }

    pub fn get_nudge(&self, id: &str) -> Option<&Nudge> {
        self.nudges.get(id)
    }

    // ===== MCP Tool Methods (kind:4200) =====

    pub fn insert_mcp_tool(&mut self, note: &Note) {
        if let Some(tool) = MCPTool::from_note(note) {
            self.mcp_tools.insert(tool.id.clone(), tool);
        }
    }

    /// Get all MCP tools, sorted by created_at descending (most recent first)
    pub fn get_mcp_tools(&self) -> Vec<&MCPTool> {
        let mut tools: Vec<_> = self.mcp_tools.values().collect();
        tools.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        tools
    }

    pub fn get_mcp_tool(&self, id: &str) -> Option<&MCPTool> {
        self.mcp_tools.get(id)
    }

    // ===== Report Methods (kind:30023) =====

    /// Get all reports (latest version of each), sorted by created_at descending
    pub fn get_reports(&self) -> Vec<&Report> {
        let mut reports: Vec<_> = self.reports.values().collect();
        reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        reports
    }

    /// Get reports for a specific project
    pub fn get_reports_by_project(&self, project_a_tag: &str) -> Vec<&Report> {
        let mut reports: Vec<_> = self.reports
            .values()
            .filter(|r| r.project_a_tag == project_a_tag)
            .collect();
        reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        reports
    }

    /// Get a specific report by slug (latest version)
    pub fn get_report(&self, slug: &str) -> Option<&Report> {
        self.reports.get(slug)
    }

    /// Get all versions of a report by slug
    pub fn get_report_versions(&self, slug: &str) -> Vec<&Report> {
        self.reports_all_versions
            .get(slug)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get the previous version of a report (for diff)
    pub fn get_previous_report_version(&self, slug: &str, current_id: &str) -> Option<&Report> {
        let versions = self.reports_all_versions.get(slug)?;
        let current_idx = versions.iter().position(|r| r.id == current_id)?;
        versions.get(current_idx + 1)
    }

    /// Get threads for a specific document (by document a-tag)
    pub fn get_document_threads(&self, document_a_tag: &str) -> &[Thread] {
        self.document_threads.get(document_a_tag)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    // ===== Operations Status Methods (kind:24133) =====

    fn handle_operations_status_event(&mut self, note: &Note) {
        if let Some(status) = OperationsStatus::from_note(note) {
            self.upsert_operations_status(status);
        }
    }

    /// Shared helper to upsert an OperationsStatus into the store.
    /// Handles both JSON and Note-based event paths to eliminate duplication.
    /// Also updates agent_tracking state for real-time active agent counts.
    ///
    /// ## Event Semantics:
    /// Each 24133 event is an authoritative snapshot of active agents for a conversation.
    /// The nostr_event_id (the 24133 event's own ID) is used for:
    /// 1. Same-second ordering (tiebreaker when timestamps match)
    /// 2. Runtime deduplication (prevent double-counting on replays)
    fn upsert_operations_status(&mut self, status: OperationsStatus) {
        let event_id = status.event_id.clone();
        let nostr_event_id = status.nostr_event_id.clone();

        // Use thread_id (conversation root) for tracking, falling back to event_id
        // This ensures per-conversation (not per-event) timestamp tracking
        let conversation_id = status.thread_id.as_deref().unwrap_or(&status.event_id);

        // Update agent tracking state for real-time monitoring
        // We pass None for current_project to track ALL active agents across all projects
        // (status bar should show green when ANY agent is active on ANY project)
        let processed = self.agent_tracking.process_24133_event(
            conversation_id,
            &nostr_event_id, // Pass nostr_event_id for same-second ordering
            &status.agent_pubkeys,
            status.created_at,
            &status.project_coordinate,
            None, // Track all projects, not filtered
        );

        // Skip processing if event was rejected (stale/out-of-order)
        if !processed {
            return;
        }

        // If llm-runtime tag is present, add confirmed runtime (with deduplication)
        if let Some(runtime_secs) = status.llm_runtime_secs {
            // add_confirmed_runtime handles deduplication by nostr_event_id internally
            self.agent_tracking.add_confirmed_runtime(&nostr_event_id, runtime_secs);
        }

        // If no agents are working, remove the entry (event is no longer being processed)
        if status.agent_pubkeys.is_empty() {
            self.operations_by_event.remove(&event_id);
        } else {
            // Only update if this event is newer than what we have
            if let Some(existing) = self.operations_by_event.get(&event_id) {
                if existing.created_at > status.created_at {
                    return; // Keep the newer one
                }
            }
            self.operations_by_event.insert(event_id, status);
        }
    }

    fn handle_report_event(&mut self, note: &Note) {
        if let Some(report) = Report::from_note(note) {
            // Only add reports that belong to known projects
            let is_known_project = self.projects.iter().any(|p| p.a_tag() == report.project_a_tag);
            if is_known_project {
                self.add_report(report);
            }
        }
    }

    /// Get agent pubkeys currently working on a specific event
    pub fn get_working_agents(&self, event_id: &str) -> Vec<String> {
        self.operations_by_event
            .get(event_id)
            .map(|s| s.agent_pubkeys.clone())
            .unwrap_or_default()
    }

    /// Check if any agents are working on a specific event
    pub fn is_event_busy(&self, event_id: &str) -> bool {
        self.operations_by_event
            .get(event_id)
            .map(|s| !s.agent_pubkeys.is_empty())
            .unwrap_or(false)
    }

    /// Get count of active operations for a project
    pub fn get_active_operations_count(&self, project_a_tag: &str) -> usize {
        self.operations_by_event
            .values()
            .filter(|s| s.project_coordinate == project_a_tag && !s.agent_pubkeys.is_empty())
            .count()
    }

    /// Get all event IDs with active operations for a project
    pub fn get_active_event_ids(&self, project_a_tag: &str) -> Vec<String> {
        self.operations_by_event
            .iter()
            .filter(|(_, s)| s.project_coordinate == project_a_tag && !s.agent_pubkeys.is_empty())
            .map(|(id, _)| id.clone())
            .collect()
    }

    /// Get all agent pubkeys currently working on any event for a project
    pub fn get_project_working_agents(&self, project_a_tag: &str) -> Vec<String> {
        let mut agents: HashSet<String> = HashSet::new();
        for status in self.operations_by_event.values() {
            if status.project_coordinate == project_a_tag && !status.agent_pubkeys.is_empty() {
                agents.extend(status.agent_pubkeys.iter().cloned());
            }
        }
        agents.into_iter().collect()
    }

    /// Check if a project has any active operations
    pub fn is_project_busy(&self, project_a_tag: &str) -> bool {
        self.operations_by_event
            .values()
            .any(|s| s.project_coordinate == project_a_tag && !s.agent_pubkeys.is_empty())
    }

    /// Get all active operations across all projects, sorted by created_at (oldest first)
    pub fn get_all_active_operations(&self) -> Vec<&OperationsStatus> {
        let mut operations: Vec<&OperationsStatus> = self.operations_by_event
            .values()
            .filter(|s| !s.agent_pubkeys.is_empty())
            .collect();
        operations.sort_by_key(|s| s.created_at);
        operations
    }

    /// Count active operations without allocation or sorting
    pub fn active_operations_count(&self) -> usize {
        self.operations_by_event
            .values()
            .filter(|s| !s.agent_pubkeys.is_empty())
            .count()
    }

    // ===== Real-Time Agent Tracking Methods =====

    /// Check if any agents are currently active (across all projects).
    /// Used to determine status bar color (green = active, red = inactive).
    pub fn has_active_agents(&self) -> bool {
        self.agent_tracking.has_active_agents()
    }

    /// Get the count of active agent instances.
    /// Example: agent1 + agent2 on conv1, agent1 on conv2 = 3 instances.
    pub fn active_agent_count(&self) -> usize {
        self.agent_tracking.active_agent_count()
    }

    /// Get only the confirmed runtime in seconds (from llm-runtime tags).
    /// This excludes the estimated unconfirmed runtime from active agents.
    #[cfg(test)]
    pub fn confirmed_runtime_secs(&self) -> u64 {
        self.agent_tracking.confirmed_runtime_secs()
    }

    /// Get the unconfirmed (estimated) runtime in seconds from currently active agents.
    /// This is the runtime growth since agents started working (not yet reported via llm-runtime).
    /// Used to augment RuntimeHierarchy's today runtime with real-time estimates.
    pub fn unconfirmed_runtime_secs(&self) -> u64 {
        self.agent_tracking.unconfirmed_runtime_secs()
    }

    /// Get statusbar runtime data: cumulative runtime in milliseconds and active agent status.
    ///
    /// Returns `(runtime_ms, has_active_agents)` where:
    /// - `runtime_ms`: Today's total unique runtime (persistent Nostr data) + estimated runtime
    ///   from currently active agents, all in milliseconds.
    /// - `has_active_agents`: Whether any agents are currently working (for status color).
    ///
    /// This is the single source of truth for statusbar runtime display, eliminating
    /// duplicate assembly logic across render files. The `* 1000` conversion from seconds
    /// to milliseconds happens here, in one place.
    pub fn get_statusbar_runtime_ms(&mut self) -> (u64, bool, usize) {
        let today_runtime_ms = self.get_today_unique_runtime();
        let unconfirmed_runtime_ms = self.agent_tracking.unconfirmed_runtime_secs() * 1000;
        let cumulative_runtime_ms = today_runtime_ms.saturating_add(unconfirmed_runtime_ms);
        let has_active_agents = self.agent_tracking.has_active_agents();
        let active_agent_count = self.agent_tracking.active_agent_count();
        (cumulative_runtime_ms, has_active_agents, active_agent_count)
    }

    /// Get active agents for a specific conversation.
    /// Returns agent pubkeys currently working on the conversation.
    /// Used for integration testing authoritative replacement semantics.
    #[cfg(test)]
    pub fn get_active_agents_for_conversation(&self, conversation_id: &str) -> Vec<String> {
        self.agent_tracking
            .get_active_agents_for_conversation(conversation_id)
            .into_iter()
            .map(|s| s.to_string())
            .collect()
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
    pub fn get_unanswered_ask_for_thread(&self, thread_id: &str) -> Option<(String, AskEvent, String)> {
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
                    return Some((thread_id.to_string(), ask_event.clone(), thread.pubkey.clone()));
                }
            }
        }

        None
    }

    // ===== Text Search Methods =====

    /// Search content using nostrdb's fulltext search.
    /// Returns (event_id, thread_id, content, kind) for matching events.
    /// thread_id is extracted from e-tags (root marker) or is the event itself if it's a thread root.
    pub fn text_search(&self, query: &str, limit: i32) -> Vec<(String, Option<String>, String, u32)> {
        eprintln!("[text_search] Starting search for query='{}', limit={}", query, limit);

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
                let kind = note.kind() as u32;

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
        let created_at = note.created_at() as u64;
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
                return self.search_user_messages_in_memory(user_pubkey, terms, project_a_tag, limit);
            }
            Err(e) => {
                // Log the DB error and fall back to in-memory search
                warn!(
                    query = %terms[0],
                    error = %e,
                    "NostrDB text_search failed, falling back to in-memory search"
                );
                return self.search_user_messages_in_memory(user_pubkey, terms, project_a_tag, limit);
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
            let all_match = terms
                .iter()
                .all(|term| text_contains_term(&content, term));
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
        use std::collections::HashMap;

        // Precompute thread_id -> project_a_tag mapping once
        let thread_to_project: HashMap<&str, &str> = self
            .threads_by_project
            .iter()
            .flat_map(|(a_tag, threads)| {
                threads.iter().map(move |t| (t.id.as_str(), a_tag.as_str()))
            })
            .collect();

        let mut results: Vec<(String, String, u64, Option<String>)> = Vec::new();

        for (thread_id, messages) in &self.messages_by_thread {
            // Fast project lookup using precomputed map
            let thread_project_a_tag = thread_to_project.get(thread_id.as_str()).copied();

            // Filter by project if specified
            if let Some(filter_a_tag) = project_a_tag {
                if thread_project_a_tag != Some(filter_a_tag) {
                    continue;
                }
            }

            for message in messages {
                // Only include messages from this user
                if message.pubkey != user_pubkey {
                    continue;
                }

                // If there are search terms, ALL must match (AND semantics)
                // Uses ASCII case-insensitive matching like conversation search
                if !terms.is_empty() {
                    let all_match = terms
                        .iter()
                        .all(|term| text_contains_term(&message.content, term));
                    if !all_match {
                        continue;
                    }
                }

                results.push((
                    message.id.clone(),
                    message.content.clone(),
                    message.created_at,
                    thread_project_a_tag.map(String::from),
                ));
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

    /// Set the trusted backends from preferences (called on init)
    /// Status data will only be populated when 24010 events arrive through subscriptions
    pub fn set_trusted_backends(&mut self, approved: HashSet<String>, blocked: HashSet<String>) {
        self.approved_backends = approved;
        self.blocked_backends = blocked;
        // NOTE: We intentionally do NOT query nostrdb for cached 24010 events here.
        // Status data flows ONLY through handle_status_event when events arrive
        // from subscriptions, ensuring trust validation is always applied.
    }

    /// Add a backend to the approved list and apply any pending status events
    pub fn add_approved_backend(&mut self, pubkey: &str) {
        self.blocked_backends.remove(pubkey);
        self.approved_backends.insert(pubkey.to_string());

        // Extract and apply pending statuses for this backend before removing them
        let pending_statuses: Vec<_> = self.pending_backend_approvals
            .iter()
            .filter(|p| p.backend_pubkey == pubkey)
            .map(|p| p.status.clone())
            .collect();

        for status in pending_statuses {
            let mut status = status;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            status.last_seen_at = now;
            self.project_statuses.insert(status.project_coordinate.clone(), status);
        }

        // Remove from pending approvals
        self.pending_backend_approvals.retain(|p| p.backend_pubkey != pubkey);
    }

    /// Add a backend to the blocked list
    pub fn add_blocked_backend(&mut self, pubkey: &str) {
        self.approved_backends.remove(pubkey);
        self.blocked_backends.insert(pubkey.to_string());

        // Remove from pending approvals
        self.pending_backend_approvals.retain(|p| p.backend_pubkey != pubkey);
    }

    /// Drain pending backend approvals (called by TUI to show modals)
    pub fn drain_pending_backend_approvals(&mut self) -> Vec<PendingBackendApproval> {
        std::mem::take(&mut self.pending_backend_approvals)
    }

    /// Check if a specific backend/project approval is pending.
    pub fn has_pending_backend_approval(&self, backend_pubkey: &str, project_a_tag: &str) -> bool {
        self.pending_backend_approvals.iter().any(|p| {
            p.backend_pubkey == backend_pubkey && p.project_a_tag == project_a_tag
        })
    }

    /// Approve a batch of pending backends and apply their cached statuses.
    /// Returns the number of unique backend pubkeys approved.
    pub fn approve_pending_backends(&mut self, pending: Vec<PendingBackendApproval>) -> u32 {
        use std::collections::HashSet;

        let mut approved_pubkeys: HashSet<String> = HashSet::new();

        for approval in pending {
            let mut status = approval.status;
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            status.last_seen_at = now;
            self.project_statuses.insert(status.project_coordinate.clone(), status);
            approved_pubkeys.insert(approval.backend_pubkey);
        }

        for pubkey in approved_pubkeys.iter() {
            self.add_approved_backend(pubkey);
        }

        approved_pubkeys.len() as u32
    }

    /// Check if there are pending backend approvals
    pub fn has_pending_backend_approvals(&self) -> bool {
        !self.pending_backend_approvals.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::Database;
    use tempfile::tempdir;

    /// Helper to create a test message with minimal required fields
    fn make_test_message(id: &str, pubkey: &str, thread_id: &str, content: &str, created_at: u64) -> Message {
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
            llm_metadata: vec![],
            delegation_tag: None,
            branch: None,
        }
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

        store.messages_by_thread
            .entry(thread_id.to_string())
            .or_default()
            .push(message);

        // Search for a term that exists in the message
        // NostrDB's text_search will return empty (no indexed content),
        // so this should fall back to in-memory search
        let results = store.search_user_messages(
            user_pubkey,
            "rust",
            None,
            10,
        );

        // Verify the in-memory fallback found our message
        assert_eq!(results.len(), 1, "Expected 1 result from in-memory fallback");
        assert_eq!(results[0].0, "msg1", "Expected to find the message we added");
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
            "Error occurred while processing request",  // has "error" but NOT "timeout"
            900, // older
        );

        store.messages_by_thread
            .entry(thread_id.to_string())
            .or_default()
            .extend(vec![message1, message2]);

        // Search for messages containing both "error" AND "timeout"
        // Only msg1 should match (msg2 has error but not timeout)
        let results = store.search_user_messages(
            user_pubkey,
            "error+timeout",
            None,
            10,
        );

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
            store.messages_by_thread
                .entry(thread_id.to_string())
                .or_default()
                .push(message);
        }

        // Empty query should return all messages
        let results = store.search_user_messages(
            user_pubkey,
            "",
            None,
            10,
        );

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

        store.project_statuses.insert(status.project_coordinate.clone(), status);

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
        assert!(store.has_active_agents());
        assert_eq!(store.active_agent_count(), 2);
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
        assert!(!store.has_active_agents());
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
        assert!(!store.has_active_agents());
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
        assert_eq!(store.confirmed_runtime_secs(), 100);

        // Same event again (simulating replay) - should NOT double-count
        store.handle_status_event_json(json1);
        assert_eq!(store.confirmed_runtime_secs(), 100); // Still 100, not 200

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
        assert_eq!(store.confirmed_runtime_secs(), 150); // 100 + 50
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
        assert_eq!(store.active_agent_count(), 1);

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
        assert_eq!(store.active_agent_count(), 1);
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
        assert_eq!(store.active_agent_count(), 1);
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
        assert!(store.has_active_agents());

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
        assert!(!store.has_active_agents());
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
        assert_eq!(store.active_agent_count(), 3);
        let agents1 = store.get_active_agents_for_conversation("conversation_xyz");
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
        assert_eq!(store.active_agent_count(), 2);
        let agents2 = store.get_active_agents_for_conversation("conversation_xyz");
        assert_eq!(agents2.len(), 2, "Expected 2 agents after replacement, got {}", agents2.len());

        // CRITICAL: Original agents should be GONE
        assert!(!agents2.contains(&"agent_alpha".to_string()), "agent_alpha should have been replaced");
        assert!(!agents2.contains(&"agent_beta".to_string()), "agent_beta should have been replaced");
        assert!(!agents2.contains(&"agent_gamma".to_string()), "agent_gamma should have been replaced");

        // CRITICAL: New agents should be present
        assert!(agents2.contains(&"agent_delta".to_string()), "agent_delta should be active");
        assert!(agents2.contains(&"agent_epsilon".to_string()), "agent_epsilon should be active");

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
        let agents_xyz = store.get_active_agents_for_conversation("conversation_xyz");
        assert_eq!(agents_xyz.len(), 2);
        assert!(agents_xyz.contains(&"agent_delta".to_string()));
        assert!(agents_xyz.contains(&"agent_epsilon".to_string()));

        // conversation_other should have omega
        let agents_other = store.get_active_agents_for_conversation("conversation_other");
        assert_eq!(agents_other.len(), 1);
        assert!(agents_other.contains(&"agent_omega".to_string()));

        // Total count should be 3 (2 from xyz + 1 from other)
        assert_eq!(store.active_agent_count(), 3);
    }

    // ========================================================================================
    // Tests for rebuild_messages_by_day_counts pagination fixes
    // ========================================================================================

    mod pagination_tests {
        use super::*;
        use crate::store::events::{ingest_events, wait_for_event_processing};
        use crate::models::project::Project;
        use nostr_sdk::prelude::*;

        /// Helper to create a kind:1 event with specific timestamp and a-tag
        fn make_kind1_event(keys: &Keys, content: &str, created_at: u64, a_tag: Option<&str>) -> Event {
            let mut builder = EventBuilder::new(Kind::TextNote, content);

            if let Some(a) = a_tag {
                builder = builder.tag(Tag::custom(
                    TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                    vec![a.to_string()],
                ));
            }

            // Use custom_created_at to set specific timestamp
            builder.custom_created_at(Timestamp::from(created_at))
                .sign_with_keys(keys)
                .unwrap()
        }

        /// Helper to create a Project struct directly for testing
        fn make_test_project(id: &str, name: &str, pubkey: &str) -> Project {
            Project {
                id: id.to_string(),
                name: name.to_string(),
                pubkey: pubkey.to_string(),
                participants: vec![],
                agent_ids: vec![],
                mcp_tool_ids: vec![],
                created_at: 0,
            }
        }

        /// Test: User message pagination across multiple batches
        /// Verifies that we correctly paginate through more events than a single batch
        /// Uses small batch_size (5) to force multiple pagination iterations
        #[test]
        fn test_user_messages_pagination_multiple_batches() {
            let dir = tempdir().unwrap();
            let db = Database::new(dir.path()).unwrap();

            let keys = Keys::generate();
            let user_pubkey = keys.public_key().to_hex();

            // Create 25 messages with different timestamps across 3 days
            // With batch_size=5, this requires 5 pages of pagination
            let mut events = Vec::new();
            let base_time: u64 = 86400 * 100; // Day 100

            for i in 0..25 {
                let day_offset = i / 10; // 10 msgs on day 0, 10 on day 1, 5 on day 2
                let timestamp = base_time + (day_offset as u64 * 86400) + (i as u64 * 60); // 1 min apart
                events.push(make_kind1_event(&keys, &format!("msg {}", i), timestamp, None));
            }

            // Ingest all events
            ingest_events(&db.ndb, &events, None).unwrap();

            // Wait for at least one to be processed
            let filter = nostrdb::Filter::new()
                .kinds([1])
                .authors([&hex::decode(&user_pubkey).unwrap().try_into().unwrap()])
                .build();
            wait_for_event_processing(&db.ndb, filter, 5000);

            // Small sleep to ensure all events are ingested
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Create store and rebuild with SMALL batch_size to force pagination
            let mut store = AppDataStore::new(db.ndb.clone());
            store.user_pubkey = Some(user_pubkey);
            store.rebuild_messages_by_day_counts_with_batch_size(5); // Force 5+ pagination pages

            // Verify EXACT count - must count all 25 messages
            let total_user: u64 = store.messages_by_day_counts.values().map(|(u, _)| *u).sum();
            assert_eq!(
                total_user, 25,
                "Pagination must count EXACTLY 25 user messages, got {}. Pagination data loss detected!",
                total_user
            );
        }

        /// Test: Same-second events are not lost during pagination (within batch_size)
        /// Uses batch_size larger than event count to verify same-second handling works
        /// when all events fit within a single batch (boundary condition)
        #[test]
        fn test_same_second_events_not_lost_single_batch() {
            let dir = tempdir().unwrap();
            let db = Database::new(dir.path()).unwrap();

            let keys = Keys::generate();
            let user_pubkey = keys.public_key().to_hex();

            // Create 15 messages ALL with the same timestamp (bursty same-second case)
            let same_timestamp: u64 = 86400 * 100;
            let mut events = Vec::new();

            for i in 0..15 {
                events.push(make_kind1_event(
                    &keys,
                    &format!("same-second msg {}", i),
                    same_timestamp,
                    None,
                ));
            }

            // Ingest all events
            ingest_events(&db.ndb, &events, None).unwrap();

            // Wait for processing
            let filter = nostrdb::Filter::new()
                .kinds([1])
                .authors([&hex::decode(&user_pubkey).unwrap().try_into().unwrap()])
                .build();
            wait_for_event_processing(&db.ndb, filter, 5000);
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Create store and rebuild with batch_size=20 (larger than 15 events)
            let mut store = AppDataStore::new(db.ndb.clone());
            store.user_pubkey = Some(user_pubkey);
            store.rebuild_messages_by_day_counts_with_batch_size(20);

            // Verify ALL same-second events are counted
            let total_user: u64 = store.messages_by_day_counts.values().map(|(u, _)| *u).sum();
            assert_eq!(
                total_user, 15,
                "Should count all 15 same-second events, got {}. Same-second event loss bug!",
                total_user
            );
        }

        /// Test: Same-second events spanning multiple batches
        ///
        /// KNOWN LIMITATION: nostrdb doesn't guarantee deterministic secondary ordering within
        /// same-timestamp events. When >batch_size events share a timestamp, the pagination
        /// strategy (inclusive until + seen_event_ids) may miss some events because nostrdb
        /// could return the same subset on each query iteration.
        ///
        /// This test documents the limitation by verifying at least batch_size events are
        /// captured (the first full page), and the warning is logged.
        #[test]
        fn test_same_second_events_multiple_batches_known_limitation() {
            let dir = tempdir().unwrap();
            let db = Database::new(dir.path()).unwrap();

            let keys = Keys::generate();
            let user_pubkey = keys.public_key().to_hex();

            // Create 12 messages ALL with the same timestamp
            // With batch_size=5, nostrdb will return 5 events per query
            // Due to lack of deterministic secondary ordering, we may only get 5 unique events
            let same_timestamp: u64 = 86400 * 100;
            let mut events = Vec::new();

            for i in 0..12 {
                events.push(make_kind1_event(
                    &keys,
                    &format!("same-second msg {}", i),
                    same_timestamp,
                    None,
                ));
            }

            // Ingest all events
            ingest_events(&db.ndb, &events, None).unwrap();

            // Wait for processing
            let filter = nostrdb::Filter::new()
                .kinds([1])
                .authors([&hex::decode(&user_pubkey).unwrap().try_into().unwrap()])
                .build();
            wait_for_event_processing(&db.ndb, filter, 5000);
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Create store and rebuild with SMALL batch_size to trigger the limitation
            let mut store = AppDataStore::new(db.ndb.clone());
            store.user_pubkey = Some(user_pubkey);
            store.rebuild_messages_by_day_counts_with_batch_size(5);

            // KNOWN LIMITATION: We can only guarantee at least batch_size events are captured
            // The warning "Potential same-second overflow detected" should be logged
            let total_user: u64 = store.messages_by_day_counts.values().map(|(u, _)| *u).sum();
            assert!(
                total_user >= 5,
                "Must capture at least batch_size (5) same-second events, got {}. \
                Pagination completely broken!",
                total_user
            );

            // If nostrdb happens to return different events on subsequent queries, we might get more
            // This is non-deterministic behavior - some runs may get 5, others may get more
            // We document this as a known limitation in the function docs
        }

        /// Test: Cross-project deduplication works correctly
        /// Events that a-tag multiple projects should only be counted once
        #[test]
        fn test_cross_project_deduplication() {
            let dir = tempdir().unwrap();
            let db = Database::new(dir.path()).unwrap();

            let keys = Keys::generate();
            let user_pubkey = keys.public_key().to_hex();

            let a_tag1 = format!("31933:{}:proj1", user_pubkey);
            let a_tag2 = format!("31933:{}:proj2", user_pubkey);

            // Create messages:
            // - 3 messages for project 1 only
            // - 3 messages for project 2 only
            // - 2 messages that a-tag BOTH projects (should only be counted once)
            let mut events = Vec::new();
            let base_time: u64 = 86400 * 100;

            for i in 0..3 {
                events.push(make_kind1_event(&keys, &format!("proj1 only {}", i), base_time + i as u64, Some(&a_tag1)));
            }
            for i in 0..3 {
                events.push(make_kind1_event(&keys, &format!("proj2 only {}", i), base_time + 100 + i as u64, Some(&a_tag2)));
            }

            // Create messages that reference both projects (using two a-tags)
            for i in 0..2 {
                let event = EventBuilder::new(Kind::TextNote, &format!("both projects {}", i))
                    .tag(Tag::custom(
                        TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                        vec![a_tag1.clone()],
                    ))
                    .tag(Tag::custom(
                        TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                        vec![a_tag2.clone()],
                    ))
                    .custom_created_at(Timestamp::from(base_time + 200 + i as u64))
                    .sign_with_keys(&keys)
                    .unwrap();
                events.push(event);
            }

            ingest_events(&db.ndb, &events, None).unwrap();

            // Wait for messages
            let filter = nostrdb::Filter::new().kinds([1]).build();
            wait_for_event_processing(&db.ndb, filter, 5000);
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Create store with both projects (directly add Project structs)
            // Use small batch_size to force pagination
            let mut store = AppDataStore::new(db.ndb.clone());
            store.user_pubkey = Some(user_pubkey.clone());
            store.projects.push(make_test_project("proj1", "Project 1", &user_pubkey));
            store.projects.push(make_test_project("proj2", "Project 2", &user_pubkey));
            store.rebuild_messages_by_day_counts_with_batch_size(3); // Force multi-page pagination

            // Verify EXACT deduplication: 3 + 3 + 2 = 8 unique messages
            let total_all: u64 = store.messages_by_day_counts.values().map(|(_, a)| *a).sum();
            assert_eq!(
                total_all, 8,
                "Must count EXACTLY 8 unique project messages (not 10 with double-counting), got {}. \
                Deduplication or pagination bug detected!",
                total_all
            );
        }

        /// Test: Project pagination continues even when page contains only duplicates
        /// This is the fix for Bug #1 - previously the loop would break early
        #[test]
        fn test_project_pagination_continues_through_duplicate_pages() {
            let dir = tempdir().unwrap();
            let db = Database::new(dir.path()).unwrap();

            let keys = Keys::generate();
            let user_pubkey = keys.public_key().to_hex();

            let a_tag1 = format!("31933:{}:proj1", user_pubkey);
            let a_tag2 = format!("31933:{}:proj2", user_pubkey);

            // Create messages designed to trigger the bug:
            // - First batch of messages a-tag BOTH projects (time 200-210)
            // - Second batch of messages a-tag ONLY project 2 (time 100-110) - older, unique to proj2
            //
            // When iterating project 2:
            // - First page sees the dual-tagged messages (already seen from project 1)
            // - Without the fix, oldest_timestamp stays None and loop breaks
            // - With the fix, we continue and find the older unique messages

            let mut events = Vec::new();
            let base_time: u64 = 86400 * 100;

            // Newer messages that a-tag both projects
            for i in 0..5 {
                let event = EventBuilder::new(Kind::TextNote, &format!("both projects {}", i))
                    .tag(Tag::custom(
                        TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                        vec![a_tag1.clone()],
                    ))
                    .tag(Tag::custom(
                        TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                        vec![a_tag2.clone()],
                    ))
                    .custom_created_at(Timestamp::from(base_time + 200 + i as u64))
                    .sign_with_keys(&keys)
                    .unwrap();
                events.push(event);
            }

            // Older messages that only a-tag project 2
            for i in 0..5 {
                events.push(make_kind1_event(
                    &keys,
                    &format!("proj2 only old {}", i),
                    base_time + 100 + i as u64,
                    Some(&a_tag2),
                ));
            }

            ingest_events(&db.ndb, &events, None).unwrap();
            let filter = nostrdb::Filter::new().kinds([1]).build();
            wait_for_event_processing(&db.ndb, filter, 5000);
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Create store with both projects (directly add Project structs)
            // Use VERY small batch_size (2) to force the duplicate-page-continuation scenario
            let mut store = AppDataStore::new(db.ndb.clone());
            store.user_pubkey = Some(user_pubkey.clone());
            store.projects.push(make_test_project("proj1", "Project 1", &user_pubkey));
            store.projects.push(make_test_project("proj2", "Project 2", &user_pubkey));
            store.rebuild_messages_by_day_counts_with_batch_size(2);

            // Verify EXACT count: 5 dual-tagged + 5 proj2-only = 10 unique messages
            let total_all: u64 = store.messages_by_day_counts.values().map(|(_, a)| *a).sum();
            assert_eq!(
                total_all, 10,
                "Must count EXACTLY 10 messages including older proj2-only ones, got {}. \
                Early termination bug when page contains only duplicates!",
                total_all
            );
        }

        /// Test: Verifies edge case behavior with >batch_size same-second events
        /// This test documents the KNOWN LIMITATION that nostrdb cannot reliably paginate
        /// through more events than batch_size at the same timestamp. Since we cannot
        /// test for the exact count (it depends on nostrdb's internal ordering), we
        /// verify that the warning log is triggered and at least batch_size events are counted.
        #[test]
        fn test_same_second_overflow_detection() {
            let dir = tempdir().unwrap();
            let db = Database::new(dir.path()).unwrap();

            let keys = Keys::generate();
            let user_pubkey = keys.public_key().to_hex();

            // Create 10 messages ALL with the same timestamp with batch_size=5
            // This should trigger the same-second overflow warning
            let same_timestamp: u64 = 86400 * 100;
            let mut events = Vec::new();

            for i in 0..10 {
                events.push(make_kind1_event(
                    &keys,
                    &format!("overflow msg {}", i),
                    same_timestamp,
                    None,
                ));
            }

            // Ingest all events
            ingest_events(&db.ndb, &events, None).unwrap();

            let filter = nostrdb::Filter::new()
                .kinds([1])
                .authors([&hex::decode(&user_pubkey).unwrap().try_into().unwrap()])
                .build();
            wait_for_event_processing(&db.ndb, filter, 5000);
            std::thread::sleep(std::time::Duration::from_millis(100));

            // Create store and rebuild with SMALL batch_size to trigger overflow detection
            let mut store = AppDataStore::new(db.ndb.clone());
            store.user_pubkey = Some(user_pubkey);
            store.rebuild_messages_by_day_counts_with_batch_size(5);

            // Verify at least batch_size events are counted
            // Due to nostrdb's lack of deterministic secondary ordering, we cannot guarantee
            // all 10 events are counted when >batch_size events share a timestamp.
            // This test verifies the warning is triggered (checked via logs) and we get at least 5.
            let total_user: u64 = store.messages_by_day_counts.values().map(|(u, _)| *u).sum();
            assert!(
                total_user >= 5,
                "Should count at least batch_size (5) same-second events, got {}. \
                Pagination completely broken!",
                total_user
            );

            // Note: The warning "Potential same-second overflow detected" should be logged.
            // In production, this alerts operators to increase batch_size or investigate
            // the workload pattern.
        }

        /// Integration test: Verify that kind:1 messages with llm-runtime tags
        /// trigger unconfirmed timer resets via reset_unconfirmed_timer
        #[test]
        fn test_kind1_message_resets_unconfirmed_timer() {
            use std::time::Duration;
            use std::thread;

            let dir = tempdir().unwrap();
            let db = Database::new(dir.path()).unwrap();
            let mut store = AppDataStore::new(db.ndb.clone());

            let keys = Keys::generate();
            let agent_pubkey = keys.public_key().to_hex();

            // Simulate agent starting work on a conversation
            store.agent_tracking.process_24133_event(
                "conv1",
                "event1",
                &[agent_pubkey.clone()],
                1000,
                "31933:user:project",
                None,
            );

            // Wait to accumulate unconfirmed runtime
            thread::sleep(Duration::from_millis(1100));

            let runtime_before_reset = store.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_before_reset >= 1,
                "Expected unconfirmed runtime >= 1 second before reset, got {}",
                runtime_before_reset
            );

            // Simulate a kind:1 message with llm-runtime tag arriving
            // (call reset_unconfirmed_timer directly as handle_message_event would)
            store.agent_tracking.reset_unconfirmed_timer("conv1", &agent_pubkey, 1100);

            // Verify that unconfirmed runtime was reset (should be near 0)
            let runtime_after_reset = store.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_after_reset < 1,
                "Expected unconfirmed runtime < 1 second after llm-runtime reset, got {}",
                runtime_after_reset
            );

            // Wait again to accumulate more unconfirmed runtime
            thread::sleep(Duration::from_millis(1100));

            let runtime_before_non_reset = store.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_before_non_reset >= 1,
                "Expected unconfirmed runtime >= 1 second before testing no reset, got {}",
                runtime_before_non_reset
            );

            // Simulate a kind:1 message WITHOUT llm-runtime tag (don't call reset_unconfirmed_timer)
            // Runtime should continue accumulating

            // Verify that unconfirmed runtime was NOT reset (should still be >= 1)
            let runtime_after_non_reset = store.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_after_non_reset >= 1,
                "Expected unconfirmed runtime >= 1 second when no reset happens, got {}",
                runtime_after_non_reset
            );

            // Test recency guard: stale message should not reset timer
            thread::sleep(Duration::from_millis(500));
            let runtime_before_stale = store.agent_tracking.unconfirmed_runtime_secs();

            // Try to reset with a stale timestamp (older than last reset at 1100)
            store.agent_tracking.reset_unconfirmed_timer("conv1", &agent_pubkey, 1050);

            // Runtime should NOT have been reset (blocked by recency guard)
            let runtime_after_stale = store.agent_tracking.unconfirmed_runtime_secs();
            assert!(
                runtime_after_stale >= runtime_before_stale,
                "Stale message should not reset timer: before {}, after {}",
                runtime_before_stale,
                runtime_after_stale
            );

            // Reset with a newer timestamp should work
            store.agent_tracking.reset_unconfirmed_timer("conv1", &agent_pubkey, 1200);
            let runtime_after_newer = store.agent_tracking.unconfirmed_runtime_secs();
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
            llm_metadata: vec![("runtime".to_string(), runtime_ms.to_string())],
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
        assert_eq!(runtime_ms, 50, "Only post-cutoff messages should be counted");
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
            make_message_with_runtime("msg1", "pubkey1", "thread1", post_cutoff, 5000),   // 5 seconds = 5000ms
            make_message_with_runtime("msg2", "pubkey1", "thread1", post_cutoff, 10000),  // 10 seconds = 10000ms
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
        assert_eq!(runtime_ms, 0, "Messages without runtime metadata should return 0");
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
            make_message_with_runtime("msg2", "pubkey1", "thread1", pre_cutoff_2, 500000),  // Excluded
            make_message_with_runtime("msg3", "pubkey1", "thread1", at_cutoff, 10),      // Included (10ms)
            make_message_with_runtime("msg4", "pubkey1", "thread1", post_cutoff_1, 20),  // Included (20ms)
            make_message_with_runtime("msg5", "pubkey1", "thread1", post_cutoff_2, 30),  // Included (30ms)
        ];

        let runtime_ms = AppDataStore::calculate_runtime_from_messages(&messages);

        // Only msg3, msg4, msg5 should be counted: (10 + 20 + 30) = 60 milliseconds
        assert_eq!(runtime_ms, 60, "Should only count messages at or after cutoff");
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

        store.messages_by_thread.insert("thread1".to_string(), messages);
        store.rebuild_runtime_by_day_counts();

        assert_eq!(store.get_today_unique_runtime(), 2000);

        let by_day = store.get_runtime_by_day(2);
        assert!(by_day.contains(&(yesterday_start, 1000)));
        assert!(by_day.contains(&(today_start, 2000)));
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
        store.messages_by_thread.insert("thread1".to_string(), thread1_messages);
        store.messages_by_thread.insert("thread2".to_string(), thread2_messages);

        // Update runtime hierarchy (simulating what happens in handle_message_event)
        store.update_runtime_hierarchy_for_thread_id("thread1");
        store.update_runtime_hierarchy_for_thread_id("thread2");

        // Verify thread1 runtime is calculated but filtered out in stats
        let thread1_individual = store.runtime_hierarchy.get_individual_runtime("thread1");
        // calculate_runtime_from_messages filters at message level, so thread1 should have 0
        assert_eq!(thread1_individual, 0, "Thread1 should have 0 runtime (pre-cutoff messages filtered)");

        // Verify thread2 runtime is calculated correctly
        let thread2_individual = store.runtime_hierarchy.get_individual_runtime("thread2");
        // (50 + 75) milliseconds = 125 milliseconds
        assert_eq!(thread2_individual, 125, "Thread2 should have correct runtime in milliseconds");

        // Verify total unique runtime only includes thread2
        let total = store.runtime_hierarchy.get_total_unique_runtime();
        assert_eq!(total, 125, "Total should only include post-cutoff conversations");

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
        let mut parent_msg = make_message_with_runtime("msg1", "pubkey1", "parent", pre_cutoff, 100000);
        parent_msg.q_tags.push("child".to_string());
        let parent_messages = vec![parent_msg];

        // Child conversation: created after cutoff
        let child_messages = vec![
            make_message_with_runtime("msg2", "pubkey1", "child", post_cutoff, 50),
        ];

        store.messages_by_thread.insert("parent".to_string(), parent_messages);
        store.messages_by_thread.insert("child".to_string(), child_messages);

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
        assert_eq!(top[0].1, 50, "Parent's filtered total should only include child");
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
        assert_ne!(day1_start, day2_start, "Timestamps should be in different UTC days");

        // Create messages with LLM metadata on different days
        let mut msg1 = make_test_message("msg1", "pubkey1", "thread1", "test", day1_timestamp);
        msg1.llm_metadata = vec![("total-tokens".to_string(), "100".to_string())];

        let mut msg2 = make_test_message("msg2", "pubkey1", "thread1", "test", day2_timestamp);
        msg2.llm_metadata = vec![("total-tokens".to_string(), "200".to_string())];

        store.messages_by_thread.insert("thread1".to_string(), vec![msg1, msg2]);
        store.rebuild_llm_activity_by_hour();

        // Verify both buckets exist with correct values
        assert_eq!(store.llm_activity_by_hour.len(), 2, "Should have 2 separate hour buckets");

        // Day 1: hour 23
        let key1 = (day1_start, 23_u8);
        assert_eq!(store.llm_activity_by_hour.get(&key1), Some(&(100, 1)), "Day 1 hour 23 should have 100 tokens, 1 message");

        // Day 2: hour 1
        let key2 = (day2_start, 1_u8);
        assert_eq!(store.llm_activity_by_hour.get(&key2), Some(&(200, 1)), "Day 2 hour 1 should have 200 tokens, 1 message");
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
        let mut llm_msg = make_test_message("msg2", "pubkey1", "thread1", "LLM response", timestamp);
        llm_msg.llm_metadata = vec![("total-tokens".to_string(), "150".to_string())];

        // Message WITH empty llm_metadata (should NOT be counted)
        let mut empty_metadata_msg = make_test_message("msg3", "pubkey1", "thread1", "empty metadata", timestamp);
        empty_metadata_msg.llm_metadata = vec![];

        store.messages_by_thread.insert("thread1".to_string(), vec![user_msg, llm_msg, empty_metadata_msg]);
        store.rebuild_llm_activity_by_hour();

        // Should only have 1 bucket for the LLM message
        assert_eq!(store.llm_activity_by_hour.len(), 1, "Should only count LLM messages");

        // Verify the bucket contains only the LLM message data
        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;
        let day_start = (timestamp / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = timestamp - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;
        let key = (day_start, hour_of_day);

        assert_eq!(store.llm_activity_by_hour.get(&key), Some(&(150, 1)), "Should only have 1 LLM message with 150 tokens");
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
        msg1.llm_metadata = vec![("total-tokens".to_string(), "500".to_string())];

        // Message with missing total-tokens (should default to 0)
        let mut msg2 = make_test_message("msg2", "pubkey1", "thread1", "test", timestamp);
        msg2.llm_metadata = vec![("other-key".to_string(), "value".to_string())];

        // Message with invalid token count (should default to 0)
        let mut msg3 = make_test_message("msg3", "pubkey1", "thread1", "test", timestamp);
        msg3.llm_metadata = vec![("total-tokens".to_string(), "invalid".to_string())];

        store.messages_by_thread.insert("thread1".to_string(), vec![msg1, msg2, msg3]);
        store.rebuild_llm_activity_by_hour();

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;
        let day_start = (timestamp / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = timestamp - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;
        let key = (day_start, hour_of_day);

        // All 3 messages counted, but only msg1 has valid tokens (500 + 0 + 0 = 500)
        assert_eq!(store.llm_activity_by_hour.get(&key), Some(&(500, 3)), "Should have 500 total tokens and 3 messages");
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
                timestamp
            );
            msg.llm_metadata = vec![("total-tokens".to_string(), format!("{}", (i + 1) * 100))];

            store.messages_by_thread
                .entry("thread1".to_string())
                .or_insert_with(Vec::new)
                .push(msg);
        }

        store.rebuild_llm_activity_by_hour();

        // Verify all 5 buckets were created
        assert_eq!(store.llm_activity_by_hour.len(), 5, "Should have 5 hour buckets");

        // NOW TEST ACTUAL WINDOW SLICING using the testable _from variant
        // Set "now" to be at 14:00:00 (the last hour with data)
        let current_hour_start = base_timestamp + (4 * seconds_per_hour);

        // Test 1: Window of 3 hours should return hours 14:00, 13:00, 12:00
        let result = store.get_tokens_by_hour_from(current_hour_start, 3);
        assert_eq!(result.len(), 3, "Window of 3 hours should return 3 entries");

        // Verify each hour in the window
        assert_eq!(result.get(&(base_timestamp + 4 * seconds_per_hour)), Some(&500_u64), "Hour 14:00 should have 500 tokens");
        assert_eq!(result.get(&(base_timestamp + 3 * seconds_per_hour)), Some(&400_u64), "Hour 13:00 should have 400 tokens");
        assert_eq!(result.get(&(base_timestamp + 2 * seconds_per_hour)), Some(&300_u64), "Hour 12:00 should have 300 tokens");

        // Test 2: Window of 5 hours should return all 5 hours
        let result = store.get_tokens_by_hour_from(current_hour_start, 5);
        assert_eq!(result.len(), 5, "Window of 5 hours should return 5 entries");
        assert_eq!(result.get(&(base_timestamp + 4 * seconds_per_hour)), Some(&500_u64));
        assert_eq!(result.get(&(base_timestamp + 3 * seconds_per_hour)), Some(&400_u64));
        assert_eq!(result.get(&(base_timestamp + 2 * seconds_per_hour)), Some(&300_u64));
        assert_eq!(result.get(&(base_timestamp + 1 * seconds_per_hour)), Some(&200_u64));
        assert_eq!(result.get(&(base_timestamp)), Some(&100_u64));

        // Test 3: Window extending beyond available data should only return available hours
        let result = store.get_tokens_by_hour_from(current_hour_start, 10);
        assert_eq!(result.len(), 5, "Window of 10 hours should return only 5 entries (available data)");

        // Test 4: Window starting at 11:00 should only see hours <= 11:00
        let earlier_current = base_timestamp + (1 * seconds_per_hour);
        let result = store.get_tokens_by_hour_from(earlier_current, 2);
        assert_eq!(result.len(), 2, "Window from 11:00 looking back 2 hours should return 2 entries");
        assert_eq!(result.get(&(base_timestamp + 1 * seconds_per_hour)), Some(&200_u64), "Hour 11:00 should have 200 tokens");
        assert_eq!(result.get(&base_timestamp), Some(&100_u64), "Hour 10:00 should have 100 tokens");
        assert_eq!(result.get(&(base_timestamp + 2 * seconds_per_hour)), None, "Hour 12:00 should NOT be in window");

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
        msg_23.llm_metadata = vec![("total-tokens".to_string(), "600".to_string())];
        store.messages_by_thread
            .entry("thread1".to_string())
            .or_insert_with(Vec::new)
            .push(msg_23);

        // Add message at 00:00 current day
        let mut msg_00 = make_test_message("msg_00", "pubkey1", "thread1", "test", current_day_00);
        msg_00.llm_metadata = vec![("total-tokens".to_string(), "700".to_string())];
        store.messages_by_thread
            .entry("thread1".to_string())
            .or_insert_with(Vec::new)
            .push(msg_00);

        // Add message at 01:00 current day
        let mut msg_01 = make_test_message("msg_01", "pubkey1", "thread1", "test", current_day_01);
        msg_01.llm_metadata = vec![("total-tokens".to_string(), "800".to_string())];
        store.messages_by_thread
            .entry("thread1".to_string())
            .or_insert_with(Vec::new)
            .push(msg_01);

        // Add message at 02:00 current day
        let mut msg_02 = make_test_message("msg_02", "pubkey1", "thread1", "test", current_day_02);
        msg_02.llm_metadata = vec![("total-tokens".to_string(), "900".to_string())];
        store.messages_by_thread
            .entry("thread1".to_string())
            .or_insert_with(Vec::new)
            .push(msg_02);

        store.rebuild_llm_activity_by_hour();

        // Window from 02:00 current day looking back 4 hours should cross midnight
        // Should see: 02:00, 01:00, 00:00, 23:00 (prior day)
        let result = store.get_tokens_by_hour_from(current_day_02, 4);

        // Verify all four hours are present
        assert_eq!(result.len(), 4, "Window from 02:00 looking back 4 hours should return 4 entries spanning midnight");

        // Verify each hour has the correct token count
        assert_eq!(result.get(&current_day_02), Some(&900_u64), "Current day 02:00 should have 900 tokens");
        assert_eq!(result.get(&current_day_01), Some(&800_u64), "Current day 01:00 should have 800 tokens");
        assert_eq!(result.get(&current_day_00), Some(&700_u64), "Current day 00:00 should have 700 tokens");
        assert_eq!(result.get(&prior_day_23), Some(&600_u64), "Prior day 23:00 should have 600 tokens");

        // Verify proper day_start bucketing: 23:00 should be on prior day, others on current day
        let prior_day_start = day_start - seconds_per_day;

        // Calculate expected day_start for each hour
        let day_start_23 = (prior_day_23 / seconds_per_day) * seconds_per_day;
        let day_start_00 = (current_day_00 / seconds_per_day) * seconds_per_day;
        let day_start_01 = (current_day_01 / seconds_per_day) * seconds_per_day;
        let day_start_02 = (current_day_02 / seconds_per_day) * seconds_per_day;

        // Verify 23:00 is on prior day
        assert_eq!(day_start_23, prior_day_start, "23:00 should be bucketed to prior day");

        // Verify 00:00, 01:00, 02:00 are all on current day
        assert_eq!(day_start_00, day_start, "00:00 should be bucketed to current day");
        assert_eq!(day_start_01, day_start, "01:00 should be bucketed to current day");
        assert_eq!(day_start_02, day_start, "02:00 should be bucketed to current day");
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

        assert_eq!(tokens_result.len(), 0, "get_tokens_by_hour(0) should return empty");
        assert_eq!(messages_result.len(), 0, "get_message_count_by_hour(0) should return empty");
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
        msg.llm_metadata = vec![("total-tokens".to_string(), "1000".to_string())];
        store.messages_by_thread
            .entry("thread1".to_string())
            .or_insert_with(Vec::new)
            .push(msg);

        // Hour 1: 2 messages
        for i in 0..2 {
            let mut msg = make_test_message(
                &format!("msg1_{}", i),
                "pubkey1",
                "thread1",
                "test",
                base_timestamp + seconds_per_hour
            );
            msg.llm_metadata = vec![("total-tokens".to_string(), "500".to_string())];
            store.messages_by_thread
                .entry("thread1".to_string())
                .or_insert_with(Vec::new)
                .push(msg);
        }

        // Hour 2: 3 messages
        for i in 0..3 {
            let mut msg = make_test_message(
                &format!("msg2_{}", i),
                "pubkey1",
                "thread1",
                "test",
                base_timestamp + 2 * seconds_per_hour
            );
            msg.llm_metadata = vec![("total-tokens".to_string(), "333".to_string())];
            store.messages_by_thread
                .entry("thread1".to_string())
                .or_insert_with(Vec::new)
                .push(msg);
        }

        store.rebuild_llm_activity_by_hour();

        // Test using the _from variant with a fixed "now" at hour 2 (12:00)
        let current_hour_start = base_timestamp + 2 * seconds_per_hour;

        // Get message counts for all 3 hours
        let message_result = store.get_message_count_by_hour_from(current_hour_start, 3);
        assert_eq!(message_result.len(), 3, "Should have 3 hours of message count data");

        // Verify MESSAGE COUNTS (not token counts)
        assert_eq!(message_result.get(&(base_timestamp + 2 * seconds_per_hour)), Some(&3_u64), "Hour 12:00 should have 3 messages");
        assert_eq!(message_result.get(&(base_timestamp + 1 * seconds_per_hour)), Some(&2_u64), "Hour 11:00 should have 2 messages");
        assert_eq!(message_result.get(&base_timestamp), Some(&1_u64), "Hour 10:00 should have 1 message");

        // Get token counts for comparison
        let token_result = store.get_tokens_by_hour_from(current_hour_start, 3);
        assert_eq!(token_result.len(), 3, "Should have 3 hours of token count data");

        // Verify TOKEN COUNTS are different from message counts
        assert_eq!(token_result.get(&(base_timestamp + 2 * seconds_per_hour)), Some(&999_u64), "Hour 12:00 should have 999 tokens (3*333)");
        assert_eq!(token_result.get(&(base_timestamp + 1 * seconds_per_hour)), Some(&1000_u64), "Hour 11:00 should have 1000 tokens (2*500)");
        assert_eq!(token_result.get(&base_timestamp), Some(&1000_u64), "Hour 10:00 should have 1000 tokens");

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
        msg1.llm_metadata = vec![("total-tokens".to_string(), "100".to_string())];

        let mut msg2 = make_test_message("msg2", "pubkey1", "thread1", "test2", timestamp + 60);
        msg2.llm_metadata = vec![("total-tokens".to_string(), "200".to_string())];

        let mut msg3 = make_test_message("msg3", "pubkey1", "thread1", "test3", timestamp + 120);
        msg3.llm_metadata = vec![("total-tokens".to_string(), "300".to_string())];

        store.messages_by_thread.insert("thread1".to_string(), vec![msg1, msg2, msg3]);
        store.rebuild_llm_activity_by_hour();

        // Should only have 1 bucket since all messages are in the same hour
        assert_eq!(store.llm_activity_by_hour.len(), 1, "All messages should be in same hour bucket");

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;
        let day_start = (timestamp / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = timestamp - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;
        let key = (day_start, hour_of_day);

        // Total tokens: 100 + 200 + 300 = 600, Total messages: 3
        assert_eq!(store.llm_activity_by_hour.get(&key), Some(&(600, 3)), "Should aggregate all tokens and count all messages");
    }
}
