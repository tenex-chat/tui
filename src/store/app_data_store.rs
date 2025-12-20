use crate::models::{AgentChatter, ConversationMetadata, InboxEventType, InboxItem, Message, Project, ProjectStatus, StreamingSession, Thread};
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

    // Streaming state - keyed by pubkey (one active stream per agent)
    streaming_sessions: HashMap<String, StreamingSession>,
    // Track finalized responses to ignore late-arriving chunks
    // Format: "pubkey:message_id"
    finalized_responses: HashSet<String>,

    // Inbox - events that p-tag the current user
    pub inbox_items: Vec<InboxItem>,
    inbox_read_ids: HashSet<String>,
    pub user_pubkey: Option<String>,

    // Agent chatter feed - kind:1111 events a-tagging our projects
    pub agent_chatter: Vec<AgentChatter>,
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
            streaming_sessions: HashMap::new(),
            finalized_responses: HashSet::new(),
            inbox_items: Vec::new(),
            inbox_read_ids: HashSet::new(),
            user_pubkey: None,
            agent_chatter: Vec::new(),
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

        // Check each message for p-tags
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
                if self.note_ptags_user(&note, user_pubkey) {
                    let project_a_tag = self.find_project_for_thread(&thread_id);

                    let inbox_item = InboxItem {
                        id: message.id.clone(),
                        event_type: InboxEventType::Mention,
                        title: message.content.chars().take(50).collect(),
                        preview: message.content.chars().skip(50).take(100).collect(),
                        project_a_tag: project_a_tag.unwrap_or_default(),
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
            11 => self.handle_thread_event(note),
            1111 => self.handle_message_event(note),
            0 => self.handle_profile_event(note),
            24010 => self.handle_status_event(note),
            513 => self.handle_metadata_event(note),
            _ => {}
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
            if let Some(a_tag) = Self::extract_project_a_tag(note) {
                // Add to existing threads list, maintaining sort order by last_activity
                let threads = self.threads_by_project.entry(a_tag).or_default();

                // Check if thread already exists (avoid duplicates)
                if !threads.iter().any(|t| t.id == thread.id) {
                    // Insert in sorted position (most recent first)
                    let insert_pos = threads.partition_point(|t| t.last_activity > thread.last_activity);
                    threads.insert(insert_pos, thread);
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

            // Finalize streaming: mark this response as complete and clear the session
            // Extract the 'e' tag (message being replied to) for finalization key
            if let Some(reply_to) = Self::extract_lowercase_e_tag(note) {
                let finalization_key = format!("{}:{}", pubkey, reply_to);
                self.finalized_responses.insert(finalization_key);
            }
            // Clear any streaming session for this agent
            self.streaming_sessions.remove(&pubkey);

            // Check if message a-tags one of our projects for agent chatter feed
            if let Some(a_tag) = Self::extract_project_a_tag(note) {
                if self.projects.iter().any(|p| p.a_tag() == a_tag) {
                    let chatter = AgentChatter {
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

            // Check for p-tag matching current user (for inbox)
            if let Some(ref user_pk) = self.user_pubkey.clone() {
                if pubkey != *user_pk {  // Don't inbox our own messages
                    if self.note_ptags_user(note, user_pk) {
                        // Find project a_tag for this thread
                        let project_a_tag = self.find_project_for_thread(&thread_id);

                        let inbox_item = InboxItem {
                            id: message_id.clone(),
                            event_type: InboxEventType::Mention,
                            title: message.content.chars().take(50).collect(),
                            preview: message.content.chars().skip(50).take(100).collect(),
                            project_a_tag: project_a_tag.unwrap_or_default(),
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
            let messages = self.messages_by_thread.entry(thread_id).or_default();

            // Check if message already exists (avoid duplicates)
            if !messages.iter().any(|m| m.id == message_id) {
                // Insert in sorted position (oldest first)
                let insert_pos = messages.partition_point(|m| m.created_at < message.created_at);
                messages.insert(insert_pos, message);
            }
        }
    }

    /// Check if a note p-tags a specific user
    fn note_ptags_user(&self, note: &Note, user_pubkey: &str) -> bool {
        for tag in note.tags() {
            if tag.count() >= 2 {
                let tag_name = tag.get(0).and_then(|t| t.variant().str());
                if tag_name == Some("p") {
                    // Try string first, then id bytes (same pattern as E-tag handling)
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

    /// Find which project a thread belongs to
    fn find_project_for_thread(&self, thread_id: &str) -> Option<String> {
        for (a_tag, threads) in &self.threads_by_project {
            if threads.iter().any(|t| t.id == thread_id) {
                return Some(a_tag.clone());
            }
        }
        None
    }

    fn handle_metadata_event(&mut self, note: &Note) {
        // Parse metadata directly from the note to update thread title
        if let Some(metadata) = ConversationMetadata::from_note(note) {
            // Find the thread across all projects and update its title
            for threads in self.threads_by_project.values_mut() {
                if let Some(thread) = threads.iter_mut().find(|t| t.id == metadata.thread_id) {
                    if let Some(title) = metadata.title {
                        thread.title = title;
                    }
                    // Update last_activity and maintain sort order
                    thread.last_activity = metadata.created_at;
                    // Re-sort to maintain order by last_activity (most recent first)
                    threads.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
                    break;
                }
            }
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

    /// Extract lowercase 'e' tag (NIP-22: message being replied to)
    fn extract_lowercase_e_tag(note: &Note) -> Option<String> {
        for tag in note.tags() {
            if tag.count() >= 2 {
                let tag_name = tag.get(0).and_then(|t| t.variant().str());
                if tag_name == Some("e") {
                    // Try string first, then id bytes
                    if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                        return Some(s.to_string());
                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                        return Some(hex::encode(id_bytes));
                    }
                }
            }
        }
        None
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

    /// Get project name for an a_tag
    pub fn get_project_name(&self, a_tag: &str) -> String {
        self.projects
            .iter()
            .find(|p| p.a_tag() == a_tag)
            .map(|p| p.name.clone())
            .unwrap_or_else(|| "Unknown".to_string())
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

    pub fn unread_inbox_count(&self) -> usize {
        self.inbox_items.iter().filter(|i| !i.is_read).count()
    }

    // ===== Agent Chatter Methods =====

    /// Get agent chatter feed items (most recent first)
    pub fn get_agent_chatter(&self) -> &[AgentChatter] {
        &self.agent_chatter
    }

    /// Add an agent chatter item, maintaining sort order and limiting to 100 items
    pub fn add_agent_chatter(&mut self, item: AgentChatter) {
        // Deduplicate by id
        if self.agent_chatter.iter().any(|i| i.id == item.id) {
            return;
        }

        // Insert sorted by created_at (most recent first)
        let pos = self.agent_chatter.partition_point(|i| i.created_at > item.created_at);
        self.agent_chatter.insert(pos, item);

        // Limit to 100 items
        if self.agent_chatter.len() > 100 {
            self.agent_chatter.truncate(100);
        }
    }

    // ===== Streaming Methods =====

    /// Handle an incoming streaming delta (kind 21111)
    /// Returns true if the delta was processed, false if it was rejected (late chunk)
    pub fn handle_streaming_delta(
        &mut self,
        pubkey: String,
        message_id: String,
        thread_id: String,
        sequence: Option<u32>,
        created_at: u64,
        delta: String,
    ) -> bool {
        eprintln!(
            "[STORE] handle_streaming_delta: pubkey={}, thread_id={}, seq={:?}, delta_len={}",
            &pubkey[..16.min(pubkey.len())],
            &thread_id[..16.min(thread_id.len())],
            sequence,
            delta.len()
        );

        // Check if this is a late chunk (response already finalized)
        let finalization_key = format!("{}:{}", pubkey, message_id);
        if self.finalized_responses.contains(&finalization_key) {
            eprintln!("[STORE]   Rejected: late chunk (already finalized)");
            return false;
        }

        // Get or create streaming session for this agent
        if let Some(session) = self.streaming_sessions.get_mut(&pubkey) {
            // Update existing session
            session.add_delta(sequence, &delta, created_at);
            eprintln!("[STORE]   Updated existing session, content_len={}", session.content().len());
        } else {
            // Create new session
            let mut session = StreamingSession::new(
                pubkey.clone(),
                message_id,
                thread_id.clone(),
                created_at,
            );
            session.add_delta(sequence, &delta, created_at);
            eprintln!("[STORE]   Created new session for thread_id={}", &thread_id[..16.min(thread_id.len())]);
            self.streaming_sessions.insert(pubkey, session);
        }

        eprintln!("[STORE]   Total active sessions: {}", self.streaming_sessions.len());
        true
    }

    /// Get streaming sessions for a specific thread
    pub fn streaming_sessions_for_thread(&self, thread_id: &str) -> Vec<&StreamingSession> {
        self.streaming_sessions
            .values()
            .filter(|session| session.thread_id == thread_id)
            .collect()
    }

    /// Get typing indicators for a thread (streaming sessions with empty content)
    pub fn typing_indicators_for_thread(&self, thread_id: &str) -> Vec<&str> {
        self.streaming_sessions
            .values()
            .filter(|session| session.thread_id == thread_id && !session.has_content())
            .map(|session| session.pubkey.as_str())
            .collect()
    }

    /// Get streaming sessions with content for a thread (for display)
    pub fn streaming_with_content_for_thread(&self, thread_id: &str) -> Vec<&StreamingSession> {
        if !self.streaming_sessions.is_empty() {
            eprintln!(
                "[QUERY] streaming_with_content_for_thread: looking for thread_id={}, total_sessions={}",
                &thread_id[..16.min(thread_id.len())],
                self.streaming_sessions.len()
            );

            for session in self.streaming_sessions.values() {
                eprintln!(
                    "[QUERY]   session: pubkey={}, session_thread_id={}, has_content={}, matches={}",
                    &session.pubkey[..16.min(session.pubkey.len())],
                    &session.thread_id[..16.min(session.thread_id.len())],
                    session.has_content(),
                    session.thread_id == thread_id
                );
            }
        }

        self.streaming_sessions
            .values()
            .filter(|session| session.thread_id == thread_id && session.has_content())
            .collect()
    }
}
