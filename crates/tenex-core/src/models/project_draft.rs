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
    pub project_ids: Vec<String>, // Project a-tags
    pub created_at: u64,
    pub pinned: bool,
}

/// Persisted NIP-46 bunker auto-approve rule.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BunkerAutoApproveRulePref {
    pub requester_pubkey: String,
    pub event_kind: Option<u16>, // None = any event kind
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

/// Three-state filter for scheduled events in conversation lists.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ScheduledFilter {
    /// Show all conversations regardless of scheduled status (default)
    #[default]
    ShowAll,
    /// Hide scheduled conversations from the list
    Hide,
    /// Show only scheduled conversations
    ShowOnly,
}

impl ScheduledFilter {
    /// Cycle to the next state: ShowAll → Hide → ShowOnly → ShowAll
    pub fn next(self) -> Self {
        match self {
            Self::ShowAll => Self::Hide,
            Self::Hide => Self::ShowOnly,
            Self::ShowOnly => Self::ShowAll,
        }
    }

    /// Human-readable label for display
    pub fn label(self) -> &'static str {
        match self {
            Self::ShowAll => "Show All",
            Self::Hide => "Hide",
            Self::ShowOnly => "Show Only",
        }
    }

    /// Returns true if an item with the given scheduled status passes this filter
    pub fn allows(self, is_scheduled: bool) -> bool {
        match self {
            Self::ShowAll => true,
            Self::Hide => !is_scheduled,
            Self::ShowOnly => is_scheduled,
        }
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
    /// Legacy: if true, hide scheduled events (migrated to scheduled_filter on load)
    #[serde(default, skip_serializing)]
    pub hide_scheduled: bool,
    /// Three-state filter for scheduled events (default: ShowAll)
    #[serde(default)]
    pub scheduled_filter: ScheduledFilter,
    /// Saved workspaces (project groups)
    #[serde(default)]
    pub workspaces: Vec<Workspace>,
    /// Currently active workspace ID (None = manual project selection mode)
    #[serde(default)]
    pub active_workspace_id: Option<String>,
    /// OpenTelemetry/Jaeger endpoint URL for viewing traces
    #[serde(default = "default_jaeger_endpoint")]
    pub jaeger_endpoint: String,
    /// AI Audio Notifications settings
    #[serde(default)]
    pub ai_audio_settings: AiAudioSettings,
    /// Enable/disable auto-starting the NIP-46 bunker signer.
    #[serde(default = "default_bunker_enabled")]
    pub bunker_enabled: bool,
    /// Persisted NIP-46 bunker auto-approve rules.
    #[serde(default)]
    pub bunker_auto_approve_rules: Vec<BunkerAutoApproveRulePref>,
}

/// Settings for AI-powered audio notifications
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AiAudioSettings {
    /// Whitelisted voice IDs from ElevenLabs
    /// (API keys stored in OS secure storage, not in JSON)
    #[serde(default)]
    pub selected_voice_ids: Vec<String>,
    /// OpenRouter model selection for text massaging.
    /// Legacy format stores a single model ID.
    /// Multi-select format is encoded as:
    /// `tenex:openrouter_models:v1:[\"model/id\",\"model/id2\"]`
    #[serde(default)]
    pub openrouter_model: Option<String>,
    /// Custom prompt for making text audio-friendly
    #[serde(default = "default_audio_prompt")]
    pub audio_prompt: String,
    /// Enable/disable audio notifications
    #[serde(default)]
    pub enabled: bool,
    /// Seconds of inactivity before TTS fires (skip if user was recently active)
    #[serde(default = "default_tts_inactivity_threshold")]
    pub tts_inactivity_threshold_secs: u64,
    /// Legacy fields for migration (ignored, will be removed on next save)
    /// These are only used during one-time migration from JSON to secure storage
    #[serde(default, skip_serializing)]
    pub(crate) elevenlabs_api_key: Option<String>,
    #[serde(default, skip_serializing)]
    pub(crate) openrouter_api_key: Option<String>,
}

pub fn default_tts_inactivity_threshold() -> u64 {
    120
}

pub fn default_audio_prompt() -> String {
    "Rephrase the message for an audio listener. Output ONLY the rephrased text — no preamble, no commentary, no meta-text. Capture the meaning and context concisely as natural speech. Reference the conversation title naturally if provided. Use ALL CAPS for words the original emphasized with bold or italic. Omit code blocks, URLs, pubkeys, and other visual-only content — summarize their intent instead.".to_string()
}

impl Default for AiAudioSettings {
    fn default() -> Self {
        Self {
            selected_voice_ids: Vec::new(),
            openrouter_model: None,
            audio_prompt: default_audio_prompt(),
            enabled: false,
            tts_inactivity_threshold_secs: default_tts_inactivity_threshold(),
            elevenlabs_api_key: None,
            openrouter_api_key: None,
        }
    }
}

fn default_jaeger_endpoint() -> String {
    "http://localhost:16686".to_string()
}

fn default_bunker_enabled() -> bool {
    true
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
            scheduled_filter: ScheduledFilter::ShowAll,
            workspaces: Vec::new(),
            active_workspace_id: None,
            jaeger_endpoint: default_jaeger_endpoint(),
            ai_audio_settings: AiAudioSettings::default(),
            bunker_enabled: default_bunker_enabled(),
            bunker_auto_approve_rules: Vec::new(),
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
        let mut prefs = Self::load_from_file(&path).unwrap_or_default();

        // Migrate any existing API keys from JSON to secure storage
        Self::migrate_api_keys(&mut prefs.ai_audio_settings);

        // Migrate legacy hide_scheduled boolean to the new three-state scheduled_filter
        if prefs.hide_scheduled && prefs.scheduled_filter == ScheduledFilter::ShowAll {
            prefs.scheduled_filter = ScheduledFilter::Hide;
        }

        Self { path, prefs }
    }

    /// Migrate API keys from JSON to OS secure storage (one-time migration)
    fn migrate_api_keys(settings: &mut AiAudioSettings) {
        use crate::secure_storage::{SecureKey, SecureStorage};

        // Migrate ElevenLabs API key if present in JSON
        if let Some(key) = settings.elevenlabs_api_key.take() {
            if !key.is_empty() {
                let _ = SecureStorage::set(SecureKey::ElevenLabsApiKey, &key);
                tracing::info!("Migrated ElevenLabs API key to secure storage");
            }
        }

        // Migrate OpenRouter API key if present in JSON
        if let Some(key) = settings.openrouter_api_key.take() {
            if !key.is_empty() {
                let _ = SecureStorage::set(SecureKey::OpenRouterApiKey, &key);
                tracing::info!("Migrated OpenRouter API key to secure storage");
            }
        }
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
            self.prefs
                .archived_project_ids
                .insert(project_a_tag.to_string());
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
        self.prefs
            .approved_backend_pubkeys
            .insert(pubkey.to_string());
        self.save_to_file();
    }

    pub fn block_backend(&mut self, pubkey: &str) {
        // Remove from approved if present
        self.prefs.approved_backend_pubkeys.remove(pubkey);
        // Add to blocked
        self.prefs
            .blocked_backend_pubkeys
            .insert(pubkey.to_string());
        self.save_to_file();
    }

    // ===== Bunker (NIP-46) Methods =====

    pub fn bunker_enabled(&self) -> bool {
        self.prefs.bunker_enabled
    }

    pub fn set_bunker_enabled(&mut self, enabled: bool) -> Result<(), String> {
        self.prefs.bunker_enabled = enabled;
        self.save_to_file_with_result()
    }

    pub fn bunker_auto_approve_rules(&self) -> &[BunkerAutoApproveRulePref] {
        &self.prefs.bunker_auto_approve_rules
    }

    pub fn add_bunker_auto_approve_rule(
        &mut self,
        requester_pubkey: String,
        event_kind: Option<u16>,
    ) -> Result<(), String> {
        let exists = self
            .prefs
            .bunker_auto_approve_rules
            .iter()
            .any(|r| r.requester_pubkey == requester_pubkey && r.event_kind == event_kind);

        if !exists {
            self.prefs
                .bunker_auto_approve_rules
                .push(BunkerAutoApproveRulePref {
                    requester_pubkey,
                    event_kind,
                });
            self.save_to_file_with_result()?;
        }

        Ok(())
    }

    pub fn remove_bunker_auto_approve_rule(
        &mut self,
        requester_pubkey: &str,
        event_kind: Option<u16>,
    ) -> Result<(), String> {
        self.prefs
            .bunker_auto_approve_rules
            .retain(|r| !(r.requester_pubkey == requester_pubkey && r.event_kind == event_kind));
        self.save_to_file_with_result()
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

    pub fn scheduled_filter(&self) -> ScheduledFilter {
        self.prefs.scheduled_filter
    }

    pub fn set_scheduled_filter(&mut self, filter: ScheduledFilter) {
        self.prefs.scheduled_filter = filter;
        self.save_to_file();
    }

    pub fn cycle_scheduled_filter(&mut self) -> ScheduledFilter {
        self.prefs.scheduled_filter = self.prefs.scheduled_filter.next();
        self.save_to_file();
        self.prefs.scheduled_filter
    }

    // ===== Workspace Methods =====

    pub fn workspaces(&self) -> &[Workspace] {
        &self.prefs.workspaces
    }

    pub fn add_workspace(&mut self, name: String, project_ids: Vec<String>) -> Workspace {
        let id = format!(
            "ws_{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        );
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
        self.prefs
            .active_workspace_id
            .as_ref()
            .and_then(|id| self.prefs.workspaces.iter().find(|w| w.id == *id))
    }

    pub fn active_workspace_id(&self) -> Option<&str> {
        self.prefs.active_workspace_id.as_deref()
    }

    // ===== Jaeger/OTL Endpoint Methods =====

    pub fn jaeger_endpoint(&self) -> &str {
        &self.prefs.jaeger_endpoint
    }

    /// Sets the Jaeger endpoint and persists to disk.
    /// Returns an error if serialization or file writing fails.
    pub fn set_jaeger_endpoint(&mut self, endpoint: String) -> Result<(), String> {
        self.prefs.jaeger_endpoint = endpoint;
        self.save_to_file_with_result()
    }

    /// Saves preferences to disk, returning an error if it fails.
    fn save_to_file_with_result(&self) -> Result<(), String> {
        let json = serde_json::to_string_pretty(&self.prefs)
            .map_err(|e| format!("Failed to serialize preferences: {}", e))?;
        fs::write(&self.path, json)
            .map_err(|e| format!("Failed to write preferences file: {}", e))?;
        Ok(())
    }

    // ===== AI Audio Settings Methods =====

    pub fn ai_audio_settings(&self) -> &AiAudioSettings {
        &self.prefs.ai_audio_settings
    }

    /// Set ElevenLabs API key (stored in OS secure storage, not JSON)
    pub fn set_elevenlabs_api_key(&mut self, key: Option<String>) -> Result<(), String> {
        use crate::secure_storage::{SecureKey, SecureStorage};

        match key {
            Some(k) if !k.is_empty() => {
                SecureStorage::set(SecureKey::ElevenLabsApiKey, &k)
                    .map_err(|e| format!("Failed to store API key: {}", e))?;
            }
            _ => {
                // Empty or None means delete
                SecureStorage::delete(SecureKey::ElevenLabsApiKey)
                    .map_err(|e| format!("Failed to delete API key: {}", e))?;
            }
        }
        Ok(())
    }

    /// Set OpenRouter API key (stored in OS secure storage, not JSON)
    pub fn set_openrouter_api_key(&mut self, key: Option<String>) -> Result<(), String> {
        use crate::secure_storage::{SecureKey, SecureStorage};

        match key {
            Some(k) if !k.is_empty() => {
                SecureStorage::set(SecureKey::OpenRouterApiKey, &k)
                    .map_err(|e| format!("Failed to store API key: {}", e))?;
            }
            _ => {
                // Empty or None means delete
                SecureStorage::delete(SecureKey::OpenRouterApiKey)
                    .map_err(|e| format!("Failed to delete API key: {}", e))?;
            }
        }
        Ok(())
    }

    /// Get ElevenLabs API key from secure storage
    pub fn get_elevenlabs_api_key(&self) -> Option<String> {
        use crate::secure_storage::{SecureKey, SecureStorage};
        SecureStorage::get(SecureKey::ElevenLabsApiKey).ok()
    }

    /// Get OpenRouter API key from secure storage
    pub fn get_openrouter_api_key(&self) -> Option<String> {
        use crate::secure_storage::{SecureKey, SecureStorage};
        SecureStorage::get(SecureKey::OpenRouterApiKey).ok()
    }

    pub fn set_selected_voice_ids(&mut self, voice_ids: Vec<String>) -> Result<(), String> {
        self.prefs.ai_audio_settings.selected_voice_ids = voice_ids;
        self.save_to_file_with_result()
    }

    pub fn set_openrouter_model(&mut self, model: Option<String>) -> Result<(), String> {
        self.prefs.ai_audio_settings.openrouter_model = model;
        self.save_to_file_with_result()
    }

    pub fn set_audio_prompt(&mut self, prompt: String) -> Result<(), String> {
        self.prefs.ai_audio_settings.audio_prompt = prompt;
        self.save_to_file_with_result()
    }

    pub fn set_audio_notifications_enabled(&mut self, enabled: bool) -> Result<(), String> {
        self.prefs.ai_audio_settings.enabled = enabled;
        self.save_to_file_with_result()
    }

    pub fn set_tts_inactivity_threshold(&mut self, secs: u64) -> Result<(), String> {
        self.prefs.ai_audio_settings.tts_inactivity_threshold_secs = secs;
        self.save_to_file_with_result()
    }

    pub fn toggle_audio_notifications(&mut self) -> Result<bool, String> {
        self.prefs.ai_audio_settings.enabled = !self.prefs.ai_audio_settings.enabled;
        self.save_to_file_with_result()?;
        Ok(self.prefs.ai_audio_settings.enabled)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn bunker_enabled_defaults_to_true() {
        let dir = tempdir().expect("tempdir");
        let storage = PreferencesStorage::new(dir.path().to_str().expect("utf8 path"));
        assert!(storage.bunker_enabled());
    }

    #[test]
    fn bunker_auto_approve_rules_add_remove_and_deduplicate() {
        let dir = tempdir().expect("tempdir");
        let mut storage = PreferencesStorage::new(dir.path().to_str().expect("utf8 path"));

        storage
            .add_bunker_auto_approve_rule("pubkey-a".to_string(), Some(1))
            .expect("add rule");
        storage
            .add_bunker_auto_approve_rule("pubkey-a".to_string(), Some(1))
            .expect("dedupe add");
        storage
            .add_bunker_auto_approve_rule("pubkey-a".to_string(), None)
            .expect("add any-kind rule");

        assert_eq!(storage.bunker_auto_approve_rules().len(), 2);

        storage
            .remove_bunker_auto_approve_rule("pubkey-a", Some(1))
            .expect("remove specific rule");
        assert_eq!(storage.bunker_auto_approve_rules().len(), 1);
        assert_eq!(
            storage.bunker_auto_approve_rules()[0],
            BunkerAutoApproveRulePref {
                requester_pubkey: "pubkey-a".to_string(),
                event_kind: None,
            }
        );
    }

    #[test]
    fn missing_bunker_fields_migrate_to_defaults() {
        let dir = tempdir().expect("tempdir");
        let prefs_path = dir.path().join("preferences.json");
        let mut value = serde_json::to_value(Preferences::default()).expect("serialize prefs");

        let map = value.as_object_mut().expect("object");
        map.remove("bunker_enabled");
        map.remove("bunker_auto_approve_rules");

        fs::write(
            &prefs_path,
            serde_json::to_string_pretty(&value).expect("json"),
        )
        .expect("write prefs");

        let storage = PreferencesStorage::new(dir.path().to_str().expect("utf8 path"));
        assert!(storage.bunker_enabled());
        assert!(storage.bunker_auto_approve_rules().is_empty());
    }
}
