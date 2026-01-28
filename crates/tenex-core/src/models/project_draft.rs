use super::TimeFilter;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::PathBuf;

/// Represents a draft for creating a new thread in a project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProjectDraft {
    pub project_a_tag: String,
    pub text: String,
    pub selected_agent_pubkey: Option<String>,
    pub last_modified: u64,
}

/// A workspace defines which projects are visible across all views
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Workspace {
    pub id: String,
    pub name: String,
    pub project_ids: Vec<String>,  // Project a-tags
    pub created_at: u64,
    pub pinned: bool,
}

impl ProjectDraft {
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
    }
}

/// Storage for project drafts (persisted to JSON file)
pub struct ProjectDraftStorage {
    path: PathBuf,
    drafts: HashMap<String, ProjectDraft>,
}

impl ProjectDraftStorage {
    pub fn new(data_dir: &str) -> Self {
        let path = PathBuf::from(data_dir).join("project_drafts.json");
        let drafts = Self::load_from_file(&path).unwrap_or_default();
        Self { path, drafts }
    }

    fn load_from_file(path: &PathBuf) -> Option<HashMap<String, ProjectDraft>> {
        let contents = fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    fn save_to_file(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.drafts) {
            let _ = fs::write(&self.path, json);
        }
    }

    pub fn save(&mut self, draft: ProjectDraft) {
        if draft.is_empty() {
            self.drafts.remove(&draft.project_a_tag);
        } else {
            self.drafts.insert(draft.project_a_tag.clone(), draft);
        }
        self.save_to_file();
    }

    pub fn load(&self, project_a_tag: &str) -> Option<ProjectDraft> {
        self.drafts.get(project_a_tag).cloned()
    }

    pub fn delete(&mut self, project_a_tag: &str) {
        self.drafts.remove(project_a_tag);
        self.save_to_file();
    }
}

/// App preferences (persisted to JSON file)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preferences {
    pub last_project_a_tag: Option<String>,
    #[serde(default)]
    pub selected_projects: Vec<String>,
    #[serde(default)]
    pub time_filter: Option<TimeFilter>,
    #[serde(default)]
    pub show_llm_metadata: bool,
    #[serde(default)]
    pub archived_thread_ids: HashSet<String>,
    #[serde(default)]
    pub archived_project_ids: HashSet<String>,
    /// If true, threads with children are collapsed by default in the Conversations tab
    #[serde(default)]
    pub threads_default_collapsed: bool,
    /// Backend pubkeys explicitly approved by the user to receive status updates
    #[serde(default)]
    pub approved_backend_pubkeys: HashSet<String>,
    /// Backend pubkeys blocked by the user (silently ignore their events)
    #[serde(default)]
    pub blocked_backend_pubkeys: HashSet<String>,
    /// Stored credentials (nsec or ncryptsec)
    #[serde(default)]
    pub stored_credentials: Option<String>,
    /// If true, hide scheduled events from conversation list (default: false = show all)
    #[serde(default)]
    pub hide_scheduled: bool,
    /// Saved workspaces (project groups)
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    /// Currently active workspace ID (None = manual project selection mode)
    #[serde(default)]
    pub active_workspace_id: Option<String>,
    /// OpenTelemetry/Jaeger endpoint URL for viewing traces
    #[serde(default = "default_jaeger_endpoint")]
    pub jaeger_endpoint: String,
}

fn default_jaeger_endpoint() -> String {
    "http://localhost:16686".to_string()
}

impl Default for Preferences {
    fn default() -> Self {
        Self {
            last_project_a_tag: None,
            selected_projects: Vec::new(),
            time_filter: None,
            show_llm_metadata: false,
            archived_thread_ids: HashSet::new(),
            archived_project_ids: HashSet::new(),
            threads_default_collapsed: false,
            approved_backend_pubkeys: HashSet::new(),
            blocked_backend_pubkeys: HashSet::new(),
            stored_credentials: None,
            hide_scheduled: false,
            workspaces: Vec::new(),
            active_workspace_id: None,
            jaeger_endpoint: default_jaeger_endpoint(),
        }
    }
}

pub struct PreferencesStorage {
    path: PathBuf,
    pub prefs: Preferences,
}

impl PreferencesStorage {
    pub fn new(data_dir: &str) -> Self {
        let path = PathBuf::from(data_dir).join("preferences.json");
        let prefs = Self::load_from_file(&path).unwrap_or_default();
        Self { path, prefs }
    }

    fn load_from_file(path: &PathBuf) -> Option<Preferences> {
        let contents = fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    fn save_to_file(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.prefs) {
            let _ = fs::write(&self.path, json);
        }
    }

    pub fn set_last_project(&mut self, a_tag: &str) {
        self.prefs.last_project_a_tag = Some(a_tag.to_string());
        self.save_to_file();
    }

    pub fn last_project(&self) -> Option<&str> {
        self.prefs.last_project_a_tag.as_deref()
    }

    pub fn selected_projects(&self) -> &[String] {
        &self.prefs.selected_projects
    }

    pub fn set_selected_projects(&mut self, projects: Vec<String>) {
        self.prefs.selected_projects = projects;
        self.save_to_file();
    }

    pub fn time_filter(&self) -> Option<TimeFilter> {
        self.prefs.time_filter
    }

    pub fn set_time_filter(&mut self, filter: Option<TimeFilter>) {
        self.prefs.time_filter = filter;
        self.save_to_file();
    }

    pub fn show_llm_metadata(&self) -> bool {
        self.prefs.show_llm_metadata
    }

    pub fn set_show_llm_metadata(&mut self, value: bool) {
        self.prefs.show_llm_metadata = value;
        self.save_to_file();
    }

    pub fn archived_thread_ids(&self) -> &HashSet<String> {
        &self.prefs.archived_thread_ids
    }

    pub fn is_thread_archived(&self, thread_id: &str) -> bool {
        self.prefs.archived_thread_ids.contains(thread_id)
    }

    pub fn toggle_thread_archived(&mut self, thread_id: &str) -> bool {
        let is_now_archived = if self.prefs.archived_thread_ids.contains(thread_id) {
            self.prefs.archived_thread_ids.remove(thread_id);
            false
        } else {
            self.prefs.archived_thread_ids.insert(thread_id.to_string());
            true
        };
        self.save_to_file();
        is_now_archived
    }

    pub fn set_thread_archived(&mut self, thread_id: &str, archived: bool) {
        if archived {
            self.prefs.archived_thread_ids.insert(thread_id.to_string());
        } else {
            self.prefs.archived_thread_ids.remove(thread_id);
        }
        self.save_to_file();
    }

    pub fn archived_project_ids(&self) -> &HashSet<String> {
        &self.prefs.archived_project_ids
    }

    pub fn is_project_archived(&self, project_a_tag: &str) -> bool {
        self.prefs.archived_project_ids.contains(project_a_tag)
    }

    pub fn toggle_project_archived(&mut self, project_a_tag: &str) -> bool {
        let is_now_archived = if self.prefs.archived_project_ids.contains(project_a_tag) {
            self.prefs.archived_project_ids.remove(project_a_tag);
            false
        } else {
            self.prefs.archived_project_ids.insert(project_a_tag.to_string());
            true
        };
        self.save_to_file();
        is_now_archived
    }

    pub fn threads_default_collapsed(&self) -> bool {
        self.prefs.threads_default_collapsed
    }

    pub fn set_threads_default_collapsed(&mut self, value: bool) {
        self.prefs.threads_default_collapsed = value;
        self.save_to_file();
    }

    pub fn toggle_threads_default_collapsed(&mut self) -> bool {
        self.prefs.threads_default_collapsed = !self.prefs.threads_default_collapsed;
        self.save_to_file();
        self.prefs.threads_default_collapsed
    }

    // ===== Backend Trust Methods =====

    pub fn is_backend_approved(&self, pubkey: &str) -> bool {
        self.prefs.approved_backend_pubkeys.contains(pubkey)
    }

    pub fn is_backend_blocked(&self, pubkey: &str) -> bool {
        self.prefs.blocked_backend_pubkeys.contains(pubkey)
    }

    pub fn approved_backend_pubkeys(&self) -> &HashSet<String> {
        &self.prefs.approved_backend_pubkeys
    }

    pub fn blocked_backend_pubkeys(&self) -> &HashSet<String> {
        &self.prefs.blocked_backend_pubkeys
    }

    pub fn approve_backend(&mut self, pubkey: &str) {
        // Remove from blocked if present
        self.prefs.blocked_backend_pubkeys.remove(pubkey);
        // Add to approved
        self.prefs.approved_backend_pubkeys.insert(pubkey.to_string());
        self.save_to_file();
    }

    pub fn block_backend(&mut self, pubkey: &str) {
        // Remove from approved if present
        self.prefs.approved_backend_pubkeys.remove(pubkey);
        // Add to blocked
        self.prefs.blocked_backend_pubkeys.insert(pubkey.to_string());
        self.save_to_file();
    }

    // ===== Credentials Methods =====

    pub fn has_stored_credentials(&self) -> bool {
        self.prefs.stored_credentials.is_some()
    }

    pub fn get_stored_credentials(&self) -> Option<&str> {
        self.prefs.stored_credentials.as_deref()
    }

    pub fn store_credentials(&mut self, credentials: &str) {
        self.prefs.stored_credentials = Some(credentials.to_string());
        self.save_to_file();
    }

    pub fn clear_credentials(&mut self) {
        self.prefs.stored_credentials = None;
        self.save_to_file();
    }

    pub fn credentials_need_password(&self) -> bool {
        self.prefs
            .stored_credentials
            .as_ref()
            .map(|c| c.starts_with("ncryptsec"))
            .unwrap_or(false)
    }

    // ===== Scheduled Events Filter Methods =====

    pub fn hide_scheduled(&self) -> bool {
        self.prefs.hide_scheduled
    }

    pub fn set_hide_scheduled(&mut self, value: bool) {
        self.prefs.hide_scheduled = value;
        self.save_to_file();
    }

    pub fn toggle_hide_scheduled(&mut self) -> bool {
        self.prefs.hide_scheduled = !self.prefs.hide_scheduled;
        self.save_to_file();
        self.prefs.hide_scheduled
    }

    // ===== Workspace Methods =====

    pub fn workspaces(&self) -> &[Workspace] {
        &self.prefs.workspaces
    }

    pub fn add_workspace(&mut self, name: String, project_ids: Vec<String>) -> Workspace {
        let id = format!("ws_{}", std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis());
        let workspace = Workspace {
            id: id.clone(),
            name,
            project_ids,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            pinned: false,
        };
        self.prefs.workspaces.push(workspace.clone());
        self.save_to_file();
        workspace
    }

    pub fn update_workspace(&mut self, id: &str, name: String, project_ids: Vec<String>) {
        if let Some(ws) = self.prefs.workspaces.iter_mut().find(|w| w.id == id) {
            ws.name = name;
            ws.project_ids = project_ids;
            self.save_to_file();
        }
    }

    pub fn delete_workspace(&mut self, id: &str) {
        self.prefs.workspaces.retain(|w| w.id != id);
        // Clear active workspace if we deleted the active one
        if self.prefs.active_workspace_id.as_deref() == Some(id) {
            self.prefs.active_workspace_id = None;
        }
        self.save_to_file();
    }

    pub fn toggle_workspace_pinned(&mut self, id: &str) -> bool {
        if let Some(ws) = self.prefs.workspaces.iter_mut().find(|w| w.id == id) {
            ws.pinned = !ws.pinned;
            let result = ws.pinned;
            self.save_to_file();
            result
        } else {
            false
        }
    }

    pub fn set_active_workspace(&mut self, id: Option<&str>) {
        self.prefs.active_workspace_id = id.map(String::from);
        self.save_to_file();
    }

    pub fn active_workspace(&self) -> Option<&Workspace> {
        self.prefs.active_workspace_id.as_ref().and_then(|id| {
            self.prefs.workspaces.iter().find(|w| w.id == *id)
        })
    }

    pub fn active_workspace_id(&self) -> Option<&str> {
        self.prefs.active_workspace_id.as_deref()
    }

    // ===== Jaeger/OTL Endpoint Methods =====

    pub fn jaeger_endpoint(&self) -> &str {
        &self.prefs.jaeger_endpoint
    }

    pub fn set_jaeger_endpoint(&mut self, endpoint: String) {
        self.prefs.jaeger_endpoint = endpoint;
        self.save_to_file();
    }
}
