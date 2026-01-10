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
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Preferences {
    pub last_project_a_tag: Option<String>,
    #[serde(default)]
    pub selected_projects: Vec<String>,
    #[serde(default)]
    pub only_by_me: bool,
    #[serde(default)]
    pub time_filter: Option<TimeFilter>,
    #[serde(default)]
    pub show_llm_metadata: bool,
    #[serde(default)]
    pub archived_thread_ids: HashSet<String>,
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

    pub fn only_by_me(&self) -> bool {
        self.prefs.only_by_me
    }

    pub fn set_only_by_me(&mut self, value: bool) {
        self.prefs.only_by_me = value;
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
}
