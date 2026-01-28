use crate::events::PendingBackendApproval;
use crate::models::{AgentChatter, AgentDefinition, AskEvent, ConversationMetadata, InboxEventType, InboxItem, Lesson, MCPTool, Message, Nudge, OperationsStatus, Project, ProjectAgent, ProjectStatus, Report, Thread};
use crate::store::RuntimeHierarchy;
use nostrdb::{Ndb, Note, Transaction};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{trace, warn};

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
        };
        store.rebuild_from_ndb();
        store
    }

    pub fn set_user_pubkey(&mut self, pubkey: String) {
        self.user_pubkey = Some(pubkey.clone());
        // Populate inbox from existing messages
        self.populate_inbox_from_existing(&pubkey);
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
    }

    /// Scan existing messages and populate inbox with those that p-tag the user
    fn populate_inbox_from_existing(&mut self, user_pubkey: &str) {
        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        // First, build a set of ask event IDs that the user has already replied to
        // by checking e-tags on user's messages
        let mut answered_ask_ids: std::collections::HashSet<String> = std::collections::HashSet::new();
        for messages in self.messages_by_thread.values() {
            for message in messages {
                if message.pubkey == user_pubkey {
                    if let Some(ref reply_to) = message.reply_to {
                        answered_ask_ids.insert(reply_to.clone());
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
                    // Extract project a_tag directly from the note first, fall back to thread lookup
                    let project_a_tag = Self::extract_project_a_tag(&note)
                        .or_else(|| self.find_project_for_thread(&thread_id));
                    let project_a_tag_str = project_a_tag.unwrap_or_default();

                    let inbox_item = InboxItem {
                        id: message.id.clone(),
                        event_type: InboxEventType::Mention,
                        title: message.content.chars().take(50).collect(),
                        project_a_tag: project_a_tag_str,
                        author_pubkey: message.pubkey.clone(),
                        created_at: message.created_at,
                        is_read: false,
                        thread_id: Some(thread_id.clone()),
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
    /// NOTE: Performance consideration - This scans all messages in the conversation
    /// each time it's called. For conversations with M messages, this is O(M).
    /// When called for each new message, total complexity becomes O(M^2) over the
    /// lifetime of the conversation. For most conversations this is acceptable,
    /// but extremely long conversations (1000+ messages) may see degraded performance.
    /// A future optimization could maintain a running sum delta.
    fn calculate_runtime_from_messages(messages: &[Message]) -> u64 {
        // 2026-01-24 00:00:00 UTC = 1769212800
        const CUTOFF_TIMESTAMP: u64 = 1769212800;

        messages
            .iter()
            .filter(|msg| msg.created_at >= CUTOFF_TIMESTAMP)
            .flat_map(|msg| {
                msg.llm_metadata
                    .iter()
                    .filter(|(key, _)| key == "runtime")
                    .filter_map(|(_, value)| value.parse::<u64>().ok())
            })
            .sum()
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

    /// Get the total unique runtime for conversations created TODAY only.
    /// Filters conversations by creation date (today in UTC), then sums their runtimes.
    /// Used for the global status bar to show today's cumulative runtime.
    pub fn get_today_unique_runtime(&mut self) -> u64 {
        self.runtime_hierarchy.get_today_unique_runtime()
    }

    /// Get runtime aggregated by day for the Stats tab bar chart.
    /// Returns (day_start_timestamp, total_runtime_ms) tuples.
    pub fn get_runtime_by_day(&self, num_days: usize) -> Vec<(u64, u64)> {
        self.runtime_hierarchy.get_runtime_by_day(num_days)
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
                // New project - queue subscription for its messages
                self.pending_project_subscriptions.push(a_tag.clone());
                self.projects.push(project);
            }
        }
    }

    /// Handle a status event from JSON (ephemeral events via DataChange channel)
    /// Routes to appropriate handler based on event kind (24010 or 24133)
    /// Parses JSON once and passes the value to handlers to avoid double parsing
    pub fn handle_status_event_json(&mut self, json: &str) {
        // Parse JSON once upfront
        let Ok(event) = serde_json::from_str::<serde_json::Value>(json) else {
            return;
        };

        if let Some(kind) = event.get("kind").and_then(|k| k.as_u64()) {
            match kind {
                24010 => self.handle_project_status_event_value(&event),
                24133 => self.handle_operations_status_event_value(&event),
                _ => {} // Ignore unknown kinds
            }
        }
    }

    /// Handle a project status event from pre-parsed Value (kind:24010)
    fn handle_project_status_event_value(&mut self, event: &serde_json::Value) {
        if let Some(status) = ProjectStatus::from_value(event) {
            let backend_pubkey = &status.backend_pubkey;

            // Check trust status
            if self.blocked_backends.contains(backend_pubkey) {
                return;
            }

            if self.approved_backends.contains(backend_pubkey) {
                self.project_statuses.insert(status.project_coordinate.clone(), status);
                return;
            }

            // Unknown backend - queue for approval
            let already_pending = self.pending_backend_approvals.iter().any(|p| {
                p.backend_pubkey == *backend_pubkey
            });

            if !already_pending {
                let now = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .map(|d| d.as_secs())
                    .unwrap_or(0);

                let pending = PendingBackendApproval {
                    backend_pubkey: backend_pubkey.clone(),
                    project_a_tag: status.project_coordinate.clone(),
                    first_seen: now,
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
        let status = ProjectStatus::from_note(note)?;
        let backend_pubkey = &status.backend_pubkey;

        // Check trust status
        if self.blocked_backends.contains(backend_pubkey) {
            // Silently ignore blocked backends
            return None;
        }

        if self.approved_backends.contains(backend_pubkey) {
            // Approved backend - process normally
            let event = crate::events::CoreEvent::ProjectStatus(status.clone());
            self.project_statuses.insert(status.project_coordinate.clone(), status);
            return Some(event);
        }

        // Unknown backend - queue for approval
        // Only add if not already pending for this backend
        let already_pending = self.pending_backend_approvals.iter().any(|p| {
            p.backend_pubkey == *backend_pubkey
        });

        if !already_pending {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            let pending = PendingBackendApproval {
                backend_pubkey: backend_pubkey.clone(),
                project_a_tag: status.project_coordinate.clone(),
                first_seen: now,
            };

            self.pending_backend_approvals.push(pending.clone());
            return Some(crate::events::CoreEvent::PendingBackendApproval(pending));
        }

        None
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
                            event_type: InboxEventType::Mention,
                            title: message.content.chars().take(50).collect(),
                            project_a_tag: project_a_tag_str,
                            author_pubkey: pubkey.clone(),
                            created_at: message.created_at,
                            is_read: false,
                            thread_id: Some(thread_id.clone()),
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

                // Insert in sorted position (oldest first)
                let insert_pos = messages.partition_point(|m| m.created_at < message_created_at);
                messages.insert(insert_pos, message);

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

    pub fn get_profile_name(&self, pubkey: &str) -> String {
        self.profiles.get(pubkey)
            .cloned()
            .unwrap_or_else(|| {
                crate::store::get_profile_name(&self.ndb, pubkey)
            })
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
    fn upsert_operations_status(&mut self, status: OperationsStatus) {
        let event_id = status.event_id.clone();

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
        let Ok(txn) = Transaction::new(&self.ndb) else {
            return vec![];
        };

        // nostrdb only fulltext indexes kind:1 and kind:30023, so no need to filter
        let Ok(notes) = self.ndb.text_search(&txn, query, None, limit) else {
            return vec![];
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

    /// Extract a-tag from a note's tags
    fn extract_a_tag_from_note(note: &nostrdb::Note) -> Option<String> {
        for tag in note.tags() {
            if tag.get(0).and_then(|t| t.variant().str()) == Some("a") {
                if let Some(a_tag_value) = tag.get(1).and_then(|t| t.variant().str()) {
                    return Some(a_tag_value.to_string());
                }
            }
        }
        None
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

    /// Add a backend to the approved list and process any pending status events
    pub fn add_approved_backend(&mut self, pubkey: &str) {
        self.blocked_backends.remove(pubkey);
        self.approved_backends.insert(pubkey.to_string());

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
}
