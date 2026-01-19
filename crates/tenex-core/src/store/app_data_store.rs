use crate::models::{AgentChatter, AgentDefinition, AskEvent, ConversationMetadata, InboxEventType, InboxItem, Lesson, Message, Nudge, OperationsStatus, Project, ProjectStatus, Report, Thread};
use nostrdb::{Ndb, Note, Transaction};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

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
            reports: HashMap::new(),
            reports_all_versions: HashMap::new(),
            document_threads: HashMap::new(),
            operations_by_event: HashMap::new(),
            pending_project_subscriptions: Vec::new(),
        };
        store.rebuild_from_ndb();
        store
    }

    pub fn set_user_pubkey(&mut self, pubkey: String) {
        self.user_pubkey = Some(pubkey.clone());
        // Populate inbox from existing messages
        self.populate_inbox_from_existing(&pubkey);
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
                    let project_a_tag = self.find_project_for_thread(&thread_id);
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

        // Load statuses and threads for all projects
        let a_tags: Vec<String> = self.projects.iter().map(|p| p.a_tag()).collect();

        for a_tag in &a_tags {
            self.reload_project_status(a_tag);
        }

        for a_tag in a_tags {
            // Pre-load threads for each project
            if let Ok(threads) = crate::store::get_threads_for_project(&self.ndb, &a_tag) {
                self.threads_by_project.insert(a_tag.clone(), threads);
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
            // Re-sort after updating last_activity
            threads.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        }

        // Apply metadata (kind:513) to threads - may further update last_activity
        self.apply_existing_metadata();

        // Load agent definitions (kind:4199)
        self.load_agent_definitions();

        // Load nudges (kind:4201)
        self.load_nudges();

        // Load operations status (kind:24133) - only recent ones matter
        self.load_operations_status();

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

        for result in results {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(nudge) = Nudge::from_note(&note) {
                    self.nudges.insert(nudge.id.clone(), nudge);
                }
            }
        }
    }

    /// Load reports from nostrdb (kind:30023) that belong to known projects
    fn load_reports(&mut self) {
        use nostrdb::{Filter, Transaction};

        // Collect project a-tags to filter reports
        let project_a_tags: std::collections::HashSet<String> = self
            .projects
            .iter()
            .map(|p| p.a_tag())
            .collect();

        if project_a_tags.is_empty() {
            // No projects loaded, skipping report loading
            return;
        }

        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        let filter = Filter::new().kinds([30023]).build();
        let Ok(results) = self.ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        let mut loaded_count = 0;
        for result in results {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(report) = Report::from_note(&note) {
                    // Only add reports that belong to known projects
                    if project_a_tags.contains(&report.project_a_tag) {
                        self.add_report(report);
                        loaded_count += 1;
                    }
                }
            }
        }

        let _ = (loaded_count, project_a_tags.len()); // Loaded reports for projects
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

    /// Load recent operations status events (kind:24133)
    /// Only keeps the most recent status per event_id, and only if agents are still working
    fn load_operations_status(&mut self) {
        use nostrdb::{Filter, Transaction};

        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        // Only load recent events (last 5 minutes) since older ones are stale
        let five_mins_ago = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs().saturating_sub(300))
            .unwrap_or(0);

        let filter = Filter::new()
            .kinds([24133])
            .since(five_mins_ago)
            .build();
        let Ok(results) = self.ndb.query(&txn, &[filter], 500) else {
            return;
        };

        // Loading operations status events (kind:24133)

        for result in results {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(status) = OperationsStatus::from_note(&note) {
                    let event_id = status.event_id.clone();

                    // Skip if no agents working (event finished)
                    if status.agent_pubkeys.is_empty() {
                        continue;
                    }

                    // Only keep newest status per event
                    if let Some(existing) = self.operations_by_event.get(&event_id) {
                        if existing.created_at > status.created_at {
                            continue;
                        }
                    }
                    self.operations_by_event.insert(event_id, status);
                }
            }
        }

        // Loaded active operations
    }

    /// Apply all existing kind:513 metadata events to threads (called during rebuild)
    /// Only applies the MOST RECENT metadata event for each thread
    fn apply_existing_metadata(&mut self) {
        use nostrdb::{Filter, Transaction};
        use std::collections::HashMap;

        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        let filter = Filter::new().kinds([513]).build();
        let Ok(results) = self.ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        // Processing existing kind:513 metadata events

        // Group metadata events by thread_id, keeping only the most recent
        let mut latest_metadata: HashMap<String, ConversationMetadata> = HashMap::new();

        for result in results {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(metadata) = ConversationMetadata::from_note(&note) {
                    let thread_id = metadata.thread_id.clone();

                    // Keep only the most recent metadata for each thread
                    match latest_metadata.get(&thread_id) {
                        Some(existing) if existing.created_at > metadata.created_at => {
                            // Existing is newer, skip this one
                        }
                        _ => {
                            // This one is newer (or first for this thread)
                            latest_metadata.insert(thread_id, metadata);
                        }
                    }
                }
            }
        }

        // Applying latest metadata for unique threads

        // Apply the latest metadata to each thread
        for (thread_id, metadata) in latest_metadata {
            for threads in self.threads_by_project.values_mut() {
                if let Some(thread) = threads.iter_mut().find(|t| t.id == thread_id) {
                    if let Some(title) = metadata.title {
                        thread.title = title;
                    }
                    thread.status_label = metadata.status_label;
                    thread.status_current_activity = metadata.status_current_activity;
                    thread.summary = metadata.summary;
                    thread.last_activity = metadata.created_at;
                    break;
                }
            }
        }

        // Re-sort all thread lists after applying metadata
        for threads in self.threads_by_project.values_mut() {
            threads.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
        }
    }

    fn reload_project_status(&mut self, a_tag: &str) {
        if let Some(status) = crate::store::get_project_status(&self.ndb, a_tag) {
            self.project_statuses.insert(a_tag.to_string(), status);
        }
    }

    /// Handle a new event from SubscriptionStream - incrementally update data
    pub fn handle_event(&mut self, kind: u32, note: &Note) {
        match kind {
            31933 => self.handle_project_event(note),
            1 => self.handle_text_event(note),
            0 => self.handle_profile_event(note),
            24010 => self.handle_status_event(note),
            513 => self.handle_metadata_event(note),
            4129 => self.handle_lesson_event(note),
            4199 => self.handle_agent_definition_event(note),
            4201 => self.handle_nudge_event(note),
            24133 => self.handle_operations_status_event(note),
            30023 => self.handle_report_event(note),
            _ => {}
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

    fn handle_status_event(&mut self, note: &Note) {
        if let Some(status) = ProjectStatus::from_note(note) {
            self.project_statuses.insert(status.project_coordinate.clone(), status);
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

            if let Some(a_tag) = Self::extract_project_a_tag(note) {
                // Add to existing threads list, maintaining sort order by last_activity
                let threads = self.threads_by_project.entry(a_tag).or_default();

                // Check if thread already exists (avoid duplicates)
                if !threads.iter().any(|t| t.id == thread_id) {
                    // Insert in sorted position (most recent first)
                    let insert_pos = threads.partition_point(|t| t.last_activity > thread.last_activity);
                    threads.insert(insert_pos, thread);
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
                        // Find project a_tag for this thread
                        let project_a_tag = self.find_project_for_thread(&thread_id);
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

                // Update thread's last_activity so it appears in Recent tab
                for threads in self.threads_by_project.values_mut() {
                    if let Some(thread) = threads.iter_mut().find(|t| t.id == thread_id) {
                        // Only update if this message is newer than current last_activity
                        if message_created_at > thread.last_activity {
                            thread.last_activity = message_created_at;
                            // Re-sort to maintain order by last_activity (most recent first)
                            threads.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
                        }
                        break;
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
            for threads in self.threads_by_project.values_mut() {
                if let Some(thread) = threads.iter_mut().find(|t| t.id == thread_id) {
                    if let Some(new_title) = title.clone() {
                        thread.title = new_title;
                    }
                    // Update status fields
                    thread.status_label = status_label;
                    thread.status_current_activity = status_current_activity;
                    thread.summary = summary;
                    // Update last_activity and maintain sort order
                    thread.last_activity = created_at;
                    // Re-sort to maintain order by last_activity (most recent first)
                    threads.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
                    break;
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
                        return Some(value.to_string());
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

    pub fn is_project_online(&self, a_tag: &str) -> bool {
        self.project_statuses.get(a_tag)
            .map(|s| s.is_online())
            .unwrap_or(false)
    }

    pub fn get_threads(&self, project_a_tag: &str) -> &[Thread] {
        self.threads_by_project.get(project_a_tag)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
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

    /// Get all threads across all projects, sorted by last_activity descending
    /// Returns (Thread, project_a_tag) tuples
    pub fn get_all_recent_threads(&self, limit: usize) -> Vec<(Thread, String)> {
        let mut all_threads: Vec<(Thread, String)> = self.threads_by_project
            .iter()
            .flat_map(|(a_tag, threads)| {
                threads.iter().map(|t| (t.clone(), a_tag.clone()))
            })
            .collect();

        all_threads.sort_by(|a, b| b.0.last_activity.cmp(&a.0.last_activity));
        all_threads.truncate(limit);
        all_threads
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

    // ===== Nudge Methods (kind:4201) =====

    fn handle_nudge_event(&mut self, note: &Note) {
        if let Some(nudge) = Nudge::from_note(note) {
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
}
