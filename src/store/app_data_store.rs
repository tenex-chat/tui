use crate::models::{Message, Project, ProjectStatus, Thread};
use nostrdb::Ndb;
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
}
