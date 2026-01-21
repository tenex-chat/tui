use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use uuid::Uuid;

// =============================================================================
// Helper functions (DRY)
// =============================================================================

/// Get current Unix timestamp in seconds
fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Derive a draft name from text (first line, truncated to 50 chars)
fn derive_name(text: &str) -> String {
    let name = text
        .lines()
        .next()
        .unwrap_or("Untitled")
        .chars()
        .take(50)
        .collect::<String>()
        .trim()
        .to_string();

    if name.is_empty() { "Untitled".to_string() } else { name }
}

/// Generate a unique draft ID using UUID v4
fn generate_draft_id() -> String {
    format!("draft-{}", Uuid::new_v4())
}

// =============================================================================
// ChatDraft - per-conversation drafts
// =============================================================================

/// Serializable paste attachment for draft storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftPasteAttachment {
    pub id: usize,
    pub content: String,
}

/// Serializable image attachment for draft storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DraftImageAttachment {
    pub id: usize,
    pub url: String,
}

/// Represents a chat draft for a conversation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatDraft {
    pub conversation_id: String,
    pub text: String,
    #[serde(default)]
    pub attachments: Vec<DraftPasteAttachment>,
    #[serde(default)]
    pub image_attachments: Vec<DraftImageAttachment>,
    pub selected_agent_pubkey: Option<String>,
    pub selected_branch: Option<String>,
    pub last_modified: u64,
}

// =============================================================================
// NamedDraft - user-created project drafts
// =============================================================================

/// Represents a named draft that can be saved and restored later.
/// These are user-created drafts associated with a project.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NamedDraft {
    /// Unique identifier for this draft (UUID-based)
    pub id: String,
    /// User-provided name for the draft (auto-generated from first line if not provided)
    pub name: String,
    /// The draft content
    pub text: String,
    /// Project a-tag this draft belongs to
    pub project_a_tag: String,
    /// Timestamp when draft was created
    pub created_at: u64,
    /// Timestamp when draft was last modified
    pub last_modified: u64,
}

impl NamedDraft {
    /// Create a new named draft with UUID-based unique ID
    pub fn new(text: String, project_a_tag: String) -> Self {
        let now = now_secs();
        let id = generate_draft_id();
        let name = derive_name(&text);

        Self {
            id,
            name,
            text,
            project_a_tag,
            created_at: now,
            last_modified: now,
        }
    }

    /// Get a preview of the draft content (first 100 chars)
    pub fn preview(&self) -> String {
        self.text.chars().take(100).collect::<String>().replace('\n', " ")
    }
}

// =============================================================================
// NamedDraftStorage - persistence with error reporting
// =============================================================================

/// Error type for named draft storage operations
#[derive(Debug)]
pub enum DraftStorageError {
    /// Failed to read drafts file
    ReadError(String),
    /// Failed to parse drafts file
    ParseError(String),
    /// Failed to write drafts file
    WriteError(String),
}

impl std::fmt::Display for DraftStorageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DraftStorageError::ReadError(e) => write!(f, "Failed to read drafts: {}", e),
            DraftStorageError::ParseError(e) => write!(f, "Failed to parse drafts: {}", e),
            DraftStorageError::WriteError(e) => write!(f, "Failed to save drafts: {}", e),
        }
    }
}

impl std::error::Error for DraftStorageError {}

/// Storage for named drafts (persisted to JSON file)
pub struct NamedDraftStorage {
    path: PathBuf,
    drafts: Vec<NamedDraft>,
    /// Last error that occurred (for surfacing to UI)
    last_error: Option<DraftStorageError>,
}

impl NamedDraftStorage {
    /// Create a new storage, loading from disk. Returns the storage even if loading fails
    /// (starts with empty drafts). Check `last_error()` to see if there was a loading issue.
    pub fn new(data_dir: &str) -> Self {
        let path = PathBuf::from(data_dir).join("named_drafts.json");
        let (drafts, last_error) = Self::load_from_file(&path);
        Self { path, drafts, last_error }
    }

    /// Load drafts from file, returning (drafts, optional_error)
    fn load_from_file(path: &PathBuf) -> (Vec<NamedDraft>, Option<DraftStorageError>) {
        match fs::read_to_string(path) {
            Ok(contents) => {
                match serde_json::from_str(&contents) {
                    Ok(drafts) => (drafts, None),
                    Err(e) => (Vec::new(), Some(DraftStorageError::ParseError(e.to_string()))),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File doesn't exist yet - that's fine, not an error
                (Vec::new(), None)
            }
            Err(e) => (Vec::new(), Some(DraftStorageError::ReadError(e.to_string()))),
        }
    }

    /// Save drafts to file, returning error if it fails
    fn save_to_file(&mut self) -> Result<(), DraftStorageError> {
        let json = serde_json::to_string_pretty(&self.drafts)
            .map_err(|e| DraftStorageError::WriteError(e.to_string()))?;

        fs::write(&self.path, json)
            .map_err(|e| DraftStorageError::WriteError(e.to_string()))?;

        self.last_error = None;
        Ok(())
    }

    /// Get the last error that occurred (if any)
    pub fn last_error(&self) -> Option<&DraftStorageError> {
        self.last_error.as_ref()
    }

    /// Clear the last error
    pub fn clear_error(&mut self) {
        self.last_error = None;
    }

    /// Save a new named draft. Returns error if persistence fails.
    pub fn save(&mut self, draft: NamedDraft) -> Result<(), DraftStorageError> {
        self.drafts.push(draft);
        if let Err(e) = self.save_to_file() {
            self.last_error = Some(DraftStorageError::WriteError(e.to_string()));
            // Remove the draft we just added since save failed
            self.drafts.pop();
            return Err(e);
        }
        Ok(())
    }

    /// Get all drafts for a project, sorted by last_modified (newest first)
    pub fn get_for_project(&self, project_a_tag: &str) -> Vec<&NamedDraft> {
        let mut drafts: Vec<_> = self.drafts
            .iter()
            .filter(|d| d.project_a_tag == project_a_tag)
            .collect();
        drafts.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
        drafts
    }

    /// Get all drafts, sorted by last_modified (newest first)
    pub fn get_all(&self) -> Vec<&NamedDraft> {
        let mut drafts: Vec<_> = self.drafts.iter().collect();
        drafts.sort_by(|a, b| b.last_modified.cmp(&a.last_modified));
        drafts
    }

    /// Get a draft by ID
    pub fn get(&self, id: &str) -> Option<&NamedDraft> {
        self.drafts.iter().find(|d| d.id == id)
    }

    /// Delete a draft by ID. Returns error if persistence fails.
    /// Transactional: on write failure, the in-memory list is rolled back.
    pub fn delete(&mut self, id: &str) -> Result<(), DraftStorageError> {
        // Find the index and snapshot the draft before deletion (for rollback)
        let removed_draft = self.drafts.iter()
            .position(|d| d.id == id)
            .map(|idx| self.drafts.remove(idx));

        if let Some(draft) = removed_draft {
            // Attempt to persist
            if let Err(e) = self.save_to_file() {
                // Rollback: re-insert the draft at the same position
                self.drafts.push(draft);
                self.last_error = Some(DraftStorageError::WriteError(e.to_string()));
                return Err(e);
            }
        }
        Ok(())
    }

    /// Update an existing draft. Returns error if persistence fails.
    /// Transactional: on write failure, the original values are restored.
    pub fn update(&mut self, id: &str, text: String) -> Result<(), DraftStorageError> {
        // Find the index of the draft to update
        let idx = match self.drafts.iter().position(|d| d.id == id) {
            Some(idx) => idx,
            None => return Ok(()), // Draft not found, nothing to do
        };

        // Snapshot original values for rollback
        let original_text = self.drafts[idx].text.clone();
        let original_modified = self.drafts[idx].last_modified;
        let original_name = self.drafts[idx].name.clone();

        // Apply changes
        self.drafts[idx].text = text.clone();
        self.drafts[idx].last_modified = now_secs();
        self.drafts[idx].name = derive_name(&text);

        // Attempt to persist
        if let Err(e) = self.save_to_file() {
            // Rollback: restore original values
            self.drafts[idx].text = original_text;
            self.drafts[idx].last_modified = original_modified;
            self.drafts[idx].name = original_name;
            self.last_error = Some(DraftStorageError::WriteError(e.to_string()));
            return Err(e);
        }
        Ok(())
    }
}

impl ChatDraft {
    /// A draft is considered empty only if it has no text, no attachments, AND no agent/branch selection
    /// This ensures agent selection and attachments are persisted even if user hasn't typed anything yet
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && self.attachments.is_empty()
            && self.image_attachments.is_empty()
            && self.selected_agent_pubkey.is_none()
            && self.selected_branch.is_none()
    }
}

/// Storage for chat drafts (persisted to JSON file)
pub struct DraftStorage {
    path: PathBuf,
    drafts: HashMap<String, ChatDraft>,
}

impl DraftStorage {
    pub fn new(data_dir: &str) -> Self {
        let path = PathBuf::from(data_dir).join("drafts.json");
        let drafts = Self::load_from_file(&path).unwrap_or_default();
        Self { path, drafts }
    }

    fn load_from_file(path: &PathBuf) -> Option<HashMap<String, ChatDraft>> {
        let contents = fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    fn save_to_file(&self) {
        if let Ok(json) = serde_json::to_string_pretty(&self.drafts) {
            let _ = fs::write(&self.path, json);
        }
    }

    /// Save a draft for a conversation
    pub fn save(&mut self, draft: ChatDraft) {
        if draft.is_empty() {
            self.drafts.remove(&draft.conversation_id);
        } else {
            self.drafts.insert(draft.conversation_id.clone(), draft);
        }
        self.save_to_file();
    }

    /// Load a draft for a conversation
    pub fn load(&self, conversation_id: &str) -> Option<ChatDraft> {
        self.drafts.get(conversation_id).cloned()
    }

    /// Delete a draft for a conversation
    pub fn delete(&mut self, conversation_id: &str) {
        self.drafts.remove(conversation_id);
        self.save_to_file();
    }
}
