use crate::models::{Message, Project, ProjectStatus, Thread};
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
        // Reload statuses for all projects
        let a_tags: Vec<String> = self.projects.iter().map(|p| p.a_tag()).collect();
        for a_tag in a_tags {
            self.reload_project_status(&a_tag);
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

    fn handle_project_event(&mut self, _note: &Note) {
        if let Ok(projects) = crate::store::get_projects(&self.ndb) {
            self.projects = projects;
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
        if let Some(a_tag) = Self::extract_project_a_tag(note) {
            self.reload_threads_for_project(&a_tag);
        }
    }

    fn handle_message_event(&mut self, note: &Note) {
        let note_id = hex::encode(note.id());
        tracing::info!("handle_message_event: processing message {}", note_id);
        if let Some(thread_id) = Self::extract_thread_id(note) {
            tracing::info!("handle_message_event: found thread_id={}, reloading messages", thread_id);
            self.reload_messages_for_thread(&thread_id);
        } else {
            tracing::warn!("handle_message_event: could not extract thread_id from message {}", note_id);
        }
    }

    fn handle_metadata_event(&mut self, note: &Note) {
        if let Some(thread_id) = Self::extract_thread_id_from_metadata(note) {
            let a_tag_to_reload = self.threads_by_project
                .iter()
                .find(|(_, threads)| threads.iter().any(|t| t.id == thread_id))
                .map(|(a_tag, _)| a_tag.clone());

            if let Some(a_tag) = a_tag_to_reload {
                self.reload_threads_for_project(&a_tag);
            }
        }
    }

    pub fn reload_threads_for_project(&mut self, a_tag: &str) {
        if let Ok(threads) = crate::store::get_threads_for_project(&self.ndb, a_tag) {
            self.threads_by_project.insert(a_tag.to_string(), threads);
        }
    }

    pub fn reload_messages_for_thread(&mut self, thread_id: &str) {
        tracing::info!("reload_messages_for_thread: loading messages for thread {}", thread_id);
        if let Ok(messages) = crate::store::get_messages_for_thread(&self.ndb, thread_id) {
            tracing::info!("reload_messages_for_thread: found {} messages for thread {}", messages.len(), thread_id);
            self.messages_by_thread.insert(thread_id.to_string(), messages);
        } else {
            tracing::warn!("reload_messages_for_thread: failed to load messages for thread {}", thread_id);
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

    fn extract_thread_id(note: &Note) -> Option<String> {
        for tag in note.tags() {
            if tag.count() >= 2 {
                let tag_name = tag.get(0).and_then(|t| t.variant().str());
                if tag_name == Some("E") {
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

    fn extract_thread_id_from_metadata(note: &Note) -> Option<String> {
        for tag in note.tags() {
            if tag.count() >= 2 {
                let tag_name = tag.get(0).and_then(|t| t.variant().str());
                if tag_name == Some("e") {
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
        let messages = self.messages_by_thread.get(thread_id)
            .map(|v| v.as_slice())
            .unwrap_or(&[]);
        tracing::debug!("get_messages: returning {} messages for thread {}", messages.len(), thread_id);
        messages
    }

    pub fn get_profile_name(&self, pubkey: &str) -> String {
        self.profiles.get(pubkey)
            .cloned()
            .unwrap_or_else(|| {
                crate::store::get_profile_name(&self.ndb, pubkey)
            })
    }
}
