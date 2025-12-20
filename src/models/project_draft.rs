use serde::{Deserialize, Serialize};
use std::collections::HashMap;
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
}
