use crate::models::{ConversationMetadata, Message, Project, ProjectStatus, Thread};
use nostrdb::{Ndb, Note, Transaction};
use std::collections::HashMap;
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
        };
        store.rebuild_from_ndb();
        store
    }

    /// Rebuild all data from nostrdb (called on startup)
    pub fn rebuild_from_ndb(&mut self) {
        if let Ok(projects) = crate::store::get_projects(&self.ndb) {
            self.projects = projects;
        }
        // Load statuses and threads for all projects
        let a_tags: Vec<String> = self.projects.iter().map(|p| p.a_tag()).collect();
        for a_tag in a_tags {
            self.reload_project_status(&a_tag);
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

            // Add to existing messages list, maintaining sort order by created_at
            let messages = self.messages_by_thread.entry(thread_id).or_default();

            // Check if message already exists (avoid duplicates)
            if !messages.iter().any(|m| m.id == message.id) {
                // Insert in sorted position (oldest first)
                let insert_pos = messages.partition_point(|m| m.created_at < message.created_at);
                messages.insert(insert_pos, message);
            }
        }
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
}
