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
    /// Reference to a source conversation that this draft is created from
    /// (for "Reference conversation" command, results in a "context" tag when sent)
    #[serde(default)]
    pub reference_conversation_id: Option<String>,
    /// Timestamp when message was confirmed published to relay (None = unpublished/pending)
    /// Drafts are ONLY cleaned up after this is set AND after a grace period
    #[serde(default)]
    pub published_at: Option<u64>,
    /// Event ID of the published message (for tracking)
    #[serde(default)]
    pub published_event_id: Option<String>,
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
    /// A draft is considered empty only if it has no text, no attachments, no agent/branch selection,
    /// AND no reference conversation. This ensures all state is persisted even if user hasn't typed anything yet.
    pub fn is_empty(&self) -> bool {
        self.text.trim().is_empty()
            && self.attachments.is_empty()
            && self.image_attachments.is_empty()
            && self.selected_agent_pubkey.is_none()
            && self.selected_branch.is_none()
            && self.reference_conversation_id.is_none()
    }

    /// Check if this draft has been published (confirmed by relay)
    pub fn is_published(&self) -> bool {
        self.published_at.is_some()
    }
}

/// A snapshot of a draft at the time it was sent, with unique tracking ID
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingPublishSnapshot {
    /// Unique ID for this specific send attempt (UUID-based)
    pub publish_id: String,
    /// The content that was actually sent
    pub content: String,
    /// Conversation ID this belongs to
    pub conversation_id: String,
    /// Timestamp when the message was sent
    pub sent_at: u64,
    /// Timestamp when confirmed published (None = still pending)
    pub published_at: Option<u64>,
    /// Event ID from relay (filled in on confirmation)
    pub published_event_id: Option<String>,
}

impl PendingPublishSnapshot {
    /// Create a new pending publish snapshot
    pub fn new(conversation_id: String, content: String) -> Self {
        Self {
            publish_id: format!("pub-{}", Uuid::new_v4()),
            content,
            conversation_id,
            sent_at: now_secs(),
            published_at: None,
            published_event_id: None,
        }
    }

    /// Check if this snapshot has been confirmed as published
    pub fn is_confirmed(&self) -> bool {
        self.published_at.is_some()
    }
}

/// Storage for chat drafts (persisted to JSON file)
///
/// BULLETPROOF PERSISTENCE: This storage is designed to NEVER lose user data.
/// - Drafts are saved on every keystroke (debounced)
/// - Drafts are ONLY removed after confirmed publish to relay AND after grace period
/// - Empty drafts are still persisted to track conversation state
/// - Unpublished drafts can be recovered on startup
/// - Pending publishes are tracked separately as snapshots, so new typing doesn't interfere
pub struct DraftStorage {
    path: PathBuf,
    drafts: HashMap<String, ChatDraft>,
    /// Pending publish snapshots - these track what was actually sent to relays
    /// Key is the publish_id (unique per send attempt)
    pending_publishes: HashMap<String, PendingPublishSnapshot>,
    /// Last error that occurred (for surfacing to UI)
    last_error: Option<DraftStorageError>,
}

/// Grace period in seconds before cleaning up published drafts (24 hours)
/// This ensures we never lose data even if relay confirmation was false positive
const PUBLISHED_DRAFT_GRACE_PERIOD_SECS: u64 = 24 * 60 * 60;

/// Data structure for persisting drafts (includes both current drafts and pending publishes)
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct DraftStorageData {
    drafts: HashMap<String, ChatDraft>,
    #[serde(default)]
    pending_publishes: HashMap<String, PendingPublishSnapshot>,
}

impl DraftStorage {
    /// Create a new storage, loading from disk. Check `last_error()` for load issues.
    pub fn new(data_dir: &str) -> Self {
        let path = PathBuf::from(data_dir).join("drafts.json");
        let (data, last_error) = Self::load_from_file(&path);
        Self {
            path,
            drafts: data.drafts,
            pending_publishes: data.pending_publishes,
            last_error,
        }
    }

    /// Load drafts from file, returning (data, optional_error)
    fn load_from_file(path: &PathBuf) -> (DraftStorageData, Option<DraftStorageError>) {
        match fs::read_to_string(path) {
            Ok(contents) => {
                // Try to parse as new format first
                if let Ok(data) = serde_json::from_str::<DraftStorageData>(&contents) {
                    return (data, None);
                }
                // Fall back to old format (just drafts HashMap)
                match serde_json::from_str::<HashMap<String, ChatDraft>>(&contents) {
                    Ok(drafts) => (DraftStorageData { drafts, pending_publishes: HashMap::new() }, None),
                    Err(e) => (DraftStorageData::default(), Some(DraftStorageError::ParseError(e.to_string()))),
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // File doesn't exist yet - that's fine, not an error
                (DraftStorageData::default(), None)
            }
            Err(e) => (DraftStorageData::default(), Some(DraftStorageError::ReadError(e.to_string()))),
        }
    }

    /// Save drafts to file, returning error if it fails
    fn save_to_file(&mut self) -> Result<(), DraftStorageError> {
        let data = DraftStorageData {
            drafts: self.drafts.clone(),
            pending_publishes: self.pending_publishes.clone(),
        };
        let json = serde_json::to_string_pretty(&data)
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

    /// Save a draft for a conversation - ALWAYS persists, never deletes
    ///
    /// BULLETPROOF: Even empty drafts are kept to preserve conversation state.
    /// Only cleanup_published_drafts() removes drafts, and only after confirmation + grace period.
    /// Returns error if persistence fails.
    pub fn save(&mut self, draft: ChatDraft) -> Result<(), DraftStorageError> {
        // CRITICAL: Never auto-delete drafts. User data is precious.
        // Even "empty" drafts may have agent/branch selections we want to preserve.
        // The only deletion path is through cleanup_published_drafts()

        // NOTE: We don't preserve published_at/published_event_id here because:
        // - These fields are for the CURRENT editor draft
        // - Published content is tracked separately in pending_publishes
        // - A new draft save means user is typing something NEW, not the published content
        self.drafts.insert(draft.conversation_id.clone(), draft);
        if let Err(e) = self.save_to_file() {
            self.last_error = Some(DraftStorageError::WriteError(e.to_string()));
            return Err(e);
        }
        Ok(())
    }

    /// Load a draft for a conversation (always returns the current draft if it exists)
    /// The draft represents what's currently in the editor, regardless of pending publishes.
    pub fn load(&self, conversation_id: &str) -> Option<ChatDraft> {
        self.drafts.get(conversation_id).cloned()
    }

    /// Alias for load() - both do the same thing now since we use snapshots for tracking
    pub fn load_any(&self, conversation_id: &str) -> Option<ChatDraft> {
        self.load(conversation_id)
    }

    /// Create a pending publish snapshot and return its unique ID
    /// Call this BEFORE sending to relay - this captures exactly what was sent
    pub fn create_publish_snapshot(&mut self, conversation_id: &str, content: String) -> Result<String, DraftStorageError> {
        let snapshot = PendingPublishSnapshot::new(conversation_id.to_string(), content);
        let publish_id = snapshot.publish_id.clone();
        self.pending_publishes.insert(publish_id.clone(), snapshot);
        if let Err(e) = self.save_to_file() {
            // Rollback: remove the snapshot we just added
            self.pending_publishes.remove(&publish_id);
            self.last_error = Some(DraftStorageError::WriteError(e.to_string()));
            return Err(e);
        }
        Ok(publish_id)
    }

    /// Remove a pending publish snapshot (for rollback when send fails)
    /// Call this when the send to relay fails AFTER snapshot was created.
    /// If disk write fails, the snapshot is reinserted to maintain consistency.
    pub fn remove_publish_snapshot(&mut self, publish_id: &str) -> Result<bool, DraftStorageError> {
        // Remove from HashMap first, but keep the snapshot for potential rollback
        if let Some(removed_snapshot) = self.pending_publishes.remove(publish_id) {
            if let Err(e) = self.save_to_file() {
                // ROLLBACK: Reinsert the snapshot since disk write failed
                // This maintains consistency between memory and disk state
                self.pending_publishes.insert(publish_id.to_string(), removed_snapshot);
                self.last_error = Some(DraftStorageError::WriteError(e.to_string()));
                return Err(e);
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// Mark a pending publish as confirmed (called AFTER relay confirms the message)
    /// Only marks the specific snapshot that matches the publish_id - doesn't affect current draft.
    /// Returns true if the snapshot was found and marked.
    pub fn mark_publish_confirmed(&mut self, publish_id: &str, event_id: Option<String>) -> Result<bool, DraftStorageError> {
        if let Some(snapshot) = self.pending_publishes.get_mut(publish_id) {
            snapshot.published_at = Some(now_secs());
            snapshot.published_event_id = event_id;
            if let Err(e) = self.save_to_file() {
                self.last_error = Some(DraftStorageError::WriteError(e.to_string()));
                return Err(e);
            }
            return Ok(true);
        }
        Ok(false)
    }

    /// Get all unpublished drafts (for recovery on startup)
    /// Returns drafts that have non-trivial content
    pub fn get_unpublished_drafts(&self) -> Vec<&ChatDraft> {
        self.drafts
            .values()
            .filter(|d| !d.is_empty())
            .collect()
    }

    /// Get all pending (unconfirmed) publish snapshots
    /// These are messages that were sent but not yet confirmed by relay
    pub fn get_pending_publishes(&self) -> Vec<&PendingPublishSnapshot> {
        self.pending_publishes
            .values()
            .filter(|s| !s.is_confirmed())
            .collect()
    }

    /// Get all drafts (for debugging/recovery)
    pub fn get_all_drafts(&self) -> Vec<&ChatDraft> {
        self.drafts.values().collect()
    }

    /// Clean up old confirmed publish snapshots (call periodically, e.g., on app startup)
    /// Only removes snapshots that were confirmed more than GRACE_PERIOD ago
    ///
    /// Returns the number of snapshots cleaned up
    pub fn cleanup_confirmed_publishes(&mut self) -> Result<usize, DraftStorageError> {
        let now = now_secs();
        let to_remove: Vec<String> = self.pending_publishes
            .iter()
            .filter_map(|(id, snapshot)| {
                if let Some(published_at) = snapshot.published_at {
                    // Only clean up if grace period has passed
                    if now.saturating_sub(published_at) > PUBLISHED_DRAFT_GRACE_PERIOD_SECS {
                        return Some(id.clone());
                    }
                }
                None
            })
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            self.pending_publishes.remove(&id);
        }

        if count > 0 {
            if let Err(e) = self.save_to_file() {
                self.last_error = Some(DraftStorageError::WriteError(e.to_string()));
                return Err(e);
            }
        }

        Ok(count)
    }

    /// Delete a draft for a conversation
    /// This is for explicit user-initiated deletion only.
    pub fn delete(&mut self, conversation_id: &str) -> Result<(), DraftStorageError> {
        self.drafts.remove(conversation_id);
        if let Err(e) = self.save_to_file() {
            self.last_error = Some(DraftStorageError::WriteError(e.to_string()));
            return Err(e);
        }
        Ok(())
    }

    /// Clear the current draft content for a conversation (after successful send)
    /// This keeps the draft entry but resets its content, preserving agent/branch selection.
    pub fn clear_draft_content(&mut self, conversation_id: &str) -> Result<(), DraftStorageError> {
        if let Some(draft) = self.drafts.get_mut(conversation_id) {
            draft.text.clear();
            draft.attachments.clear();
            draft.image_attachments.clear();
            draft.reference_conversation_id = None;
            draft.last_modified = now_secs();
            if let Err(e) = self.save_to_file() {
                self.last_error = Some(DraftStorageError::WriteError(e.to_string()));
                return Err(e);
            }
        }
        Ok(())
    }
}
