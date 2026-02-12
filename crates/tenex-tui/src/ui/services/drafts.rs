//! Draft service for unified draft storage management.
//!
//! Consolidates `DraftStorage` and `NamedDraftStorage` behind a unified service
//! that owns the `RefCell` internally, preventing scattered `borrow_mut()` calls
//! throughout the codebase.
//!
//! BULLETPROOF PERSISTENCE: This service is designed to NEVER lose user data.
//! - Every keystroke is saved
//! - Versioned drafts prevent message overwrites
//! - State machine prevents premature clearing
//! - Backup rotation protects against file corruption

use std::cell::RefCell;
use tenex_core::models::draft::{
    ChatDraft, DraftStorage, DraftStorageError, NamedDraft, NamedDraftStorage,
    PendingPublishSnapshot, SendState,
};

/// Unified service for draft persistence.
/// Owns RefCell internally to prevent scattered borrow_mut calls.
pub struct DraftService {
    draft_storage: RefCell<DraftStorage>,
    named_draft_storage: RefCell<NamedDraftStorage>,
}

impl DraftService {
    /// Create a new DraftService with the given data directory.
    pub fn new(data_dir: &str) -> Self {
        Self {
            draft_storage: RefCell::new(DraftStorage::new(data_dir)),
            named_draft_storage: RefCell::new(NamedDraftStorage::new(data_dir)),
        }
    }

    // =========================================================================
    // Chat Draft Operations
    // =========================================================================

    /// Save a chat draft for a conversation
    pub fn save_chat_draft(&self, draft: ChatDraft) -> Result<(), DraftStorageError> {
        self.draft_storage.borrow_mut().save(draft)
    }

    /// Load a chat draft for a conversation
    pub fn load_chat_draft(&self, conversation_id: &str) -> Option<ChatDraft> {
        self.draft_storage.borrow().load(conversation_id)
    }

    /// Delete a chat draft for a conversation
    pub fn delete_chat_draft(&self, conversation_id: &str) -> Result<(), DraftStorageError> {
        self.draft_storage.borrow_mut().delete(conversation_id)
    }

    /// Clear the draft content but preserve agent/branch selections
    pub fn clear_draft_content(&self, conversation_id: &str) -> Result<(), DraftStorageError> {
        self.draft_storage.borrow_mut().clear_draft_content(conversation_id)
    }

    // =========================================================================
    // Publish Snapshot Operations (Bulletproof persistence)
    // =========================================================================

    /// Create a publish snapshot for a message about to be sent.
    /// Returns the unique publish_id for tracking confirmation.
    pub fn create_publish_snapshot(
        &self,
        conversation_id: &str,
        content: String,
    ) -> Result<String, DraftStorageError> {
        self.draft_storage
            .borrow_mut()
            .create_publish_snapshot(conversation_id, content)
    }

    /// Mark a publish snapshot as confirmed (call after relay confirmation)
    pub fn mark_publish_confirmed(
        &self,
        publish_id: &str,
        event_id: Option<String>,
    ) -> Result<bool, DraftStorageError> {
        self.draft_storage
            .borrow_mut()
            .mark_publish_confirmed(publish_id, event_id)
    }

    /// Remove a publish snapshot (for rollback when send fails)
    pub fn remove_publish_snapshot(&self, publish_id: &str) -> Result<bool, DraftStorageError> {
        self.draft_storage
            .borrow_mut()
            .remove_publish_snapshot(publish_id)
    }

    /// Clean up old confirmed publish snapshots (call on app startup)
    /// Returns the number of snapshots cleaned up
    pub fn cleanup_confirmed_publishes(&self) -> Result<usize, DraftStorageError> {
        self.draft_storage.borrow_mut().cleanup_confirmed_publishes()
    }

    /// Get all unpublished drafts (for recovery on startup)
    pub fn get_unpublished_drafts(&self) -> Vec<ChatDraft> {
        self.draft_storage
            .borrow()
            .get_unpublished_drafts()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get pending (unconfirmed) publish snapshots
    pub fn get_pending_publishes(&self) -> Vec<PendingPublishSnapshot> {
        self.draft_storage
            .borrow()
            .get_pending_publishes()
            .into_iter()
            .cloned()
            .collect()
    }

    // =========================================================================
    // Error Handling
    // =========================================================================

    /// Get the last draft storage error (if any)
    pub fn chat_draft_last_error(&self) -> Option<String> {
        self.draft_storage.borrow().last_error().map(|e| e.to_string())
    }

    /// Clear the last draft storage error
    pub fn chat_draft_clear_error(&self) {
        self.draft_storage.borrow_mut().clear_error();
    }

    /// Get the last named draft storage error (if any)
    pub fn named_draft_last_error(&self) -> Option<String> {
        self.named_draft_storage
            .borrow()
            .last_error()
            .map(|e| e.to_string())
    }

    /// Clear the last named draft storage error
    pub fn named_draft_clear_error(&self) {
        self.named_draft_storage.borrow_mut().clear_error();
    }

    // =========================================================================
    // Named Draft Operations
    // =========================================================================

    /// Save a named draft
    pub fn save_named_draft(&self, draft: NamedDraft) -> Result<(), DraftStorageError> {
        self.named_draft_storage.borrow_mut().save(draft)
    }

    /// Get all named drafts for a project
    pub fn get_named_drafts_for_project(&self, project_a_tag: &str) -> Vec<NamedDraft> {
        self.named_draft_storage
            .borrow()
            .get_for_project(project_a_tag)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get all named drafts
    pub fn get_all_named_drafts(&self) -> Vec<NamedDraft> {
        self.named_draft_storage
            .borrow()
            .get_all()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Delete a named draft by ID
    pub fn delete_named_draft(&self, id: &str) -> Result<(), DraftStorageError> {
        self.named_draft_storage.borrow_mut().delete(id)
    }

    /// Update a named draft's text
    pub fn update_named_draft(&self, id: &str, text: String) -> Result<(), DraftStorageError> {
        self.named_draft_storage.borrow_mut().update(id, text)
    }

    // =========================================================================
    // Bulletproof Draft Operations (New)
    // =========================================================================

    /// Check if storage was recovered from backup (show notification to user once)
    pub fn was_recovered_from_backup(&self) -> bool {
        self.draft_storage.borrow().recovered_from_backup
    }

    /// Force flush to disk immediately (call after critical operations)
    pub fn flush(&self) -> Result<(), DraftStorageError> {
        self.draft_storage.borrow_mut().flush()
    }

    // =========================================================================
    // Versioned Draft Operations (Multi-message support)
    // =========================================================================

    /// Save a versioned draft (for multi-message tracking)
    pub fn save_versioned_draft(&self, draft: ChatDraft) -> Result<(), DraftStorageError> {
        self.draft_storage.borrow_mut().save_versioned(draft)
    }

    /// Load a specific versioned draft
    pub fn load_versioned_draft(&self, conversation_id: &str, message_sequence: u32) -> Option<ChatDraft> {
        self.draft_storage.borrow().load_versioned(conversation_id, message_sequence)
    }

    /// Get all versioned drafts for a conversation
    pub fn get_versioned_drafts_for_conversation(&self, conversation_id: &str) -> Vec<ChatDraft> {
        self.draft_storage
            .borrow()
            .get_versioned_drafts_for_conversation(conversation_id)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get all versioned drafts (for recovery/debugging)
    pub fn get_all_versioned_drafts(&self) -> Vec<ChatDraft> {
        self.draft_storage
            .borrow()
            .get_all_versioned_drafts()
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get current message sequence for a conversation
    pub fn get_current_sequence(&self, conversation_id: &str) -> u32 {
        self.draft_storage.borrow().get_current_sequence(conversation_id)
    }

    // =========================================================================
    // State Machine Operations (Bulletproof clearing)
    // =========================================================================

    /// Mark draft as pending send (when user hits send button)
    /// NEVER deletes - only transitions state
    pub fn mark_draft_pending_send(&self, conversation_id: &str) -> Result<(), DraftStorageError> {
        self.draft_storage.borrow_mut().mark_draft_pending_send(conversation_id)
    }

    /// Mark draft as sent awaiting confirmation
    pub fn mark_draft_sent_awaiting(&self, conversation_id: &str) -> Result<(), DraftStorageError> {
        self.draft_storage.borrow_mut().mark_draft_sent_awaiting(conversation_id)
    }

    /// Mark draft as confirmed (after relay confirms)
    /// STILL doesn't delete - cleanup happens separately after grace period
    pub fn mark_draft_confirmed(&self, conversation_id: &str, event_id: Option<String>) -> Result<(), DraftStorageError> {
        self.draft_storage.borrow_mut().mark_draft_confirmed(conversation_id, event_id)
    }

    // =========================================================================
    // Archive Operations
    // =========================================================================

    /// Archive old confirmed drafts (move to drafts_archive.json)
    /// Returns number of drafts archived
    pub fn archive_old_confirmed_drafts(&self) -> Result<usize, DraftStorageError> {
        self.draft_storage.borrow_mut().archive_old_confirmed_drafts()
    }

    /// Get all archived drafts
    pub fn get_archived_drafts(&self) -> Vec<ChatDraft> {
        self.draft_storage.borrow().get_archived_drafts()
    }

    // =========================================================================
    // Pre-conversation Draft Operations (New conversation handling)
    // =========================================================================

    /// Get or create a draft for a new conversation in a project
    /// Uses session ID for tracking before real conversation_id exists
    pub fn get_or_create_project_draft(&self, project_a_tag: &str) -> Result<ChatDraft, DraftStorageError> {
        self.draft_storage.borrow_mut().get_or_create_project_draft(project_a_tag)
    }

    /// Migrate a pre-conversation draft to a real conversation ID
    /// Call after first message is sent and we have a thread ID
    pub fn migrate_draft_to_conversation(
        &self,
        old_draft_key: &str,
        new_conversation_id: &str,
    ) -> Result<(), DraftStorageError> {
        self.draft_storage.borrow_mut().migrate_draft_to_conversation(old_draft_key, new_conversation_id)
    }

    /// Get all drafts for a specific project
    pub fn get_drafts_for_project(&self, project_a_tag: &str) -> Vec<ChatDraft> {
        self.draft_storage
            .borrow()
            .get_drafts_for_project(project_a_tag)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get all pre-conversation drafts (typing in new conversation)
    pub fn get_pre_conversation_drafts(&self) -> Vec<ChatDraft> {
        self.draft_storage
            .borrow()
            .get_pre_conversation_drafts()
            .into_iter()
            .cloned()
            .collect()
    }

    // =========================================================================
    // Comprehensive Search (for Ctrl+R integration)
    // =========================================================================

    /// Get ALL drafts from all sources for search/recovery
    /// Includes: current drafts, versioned drafts, archived drafts, named drafts
    pub fn get_all_searchable_drafts(&self) -> AllDrafts {
        let draft_storage = self.draft_storage.borrow();
        let named_storage = self.named_draft_storage.borrow();

        AllDrafts {
            chat_drafts: draft_storage.get_all_drafts().into_iter().cloned().collect(),
            versioned_drafts: draft_storage.get_all_versioned_drafts().into_iter().cloned().collect(),
            archived_drafts: draft_storage.get_archived_drafts(),
            named_drafts: named_storage.get_all().into_iter().cloned().collect(),
            pending_publishes: draft_storage.get_pending_publishes().into_iter().cloned().collect(),
        }
    }
}

/// Container for all draft types (for comprehensive search)
#[derive(Debug, Clone, Default)]
pub struct AllDrafts {
    pub chat_drafts: Vec<ChatDraft>,
    pub versioned_drafts: Vec<ChatDraft>,
    pub archived_drafts: Vec<ChatDraft>,
    pub named_drafts: Vec<NamedDraft>,
    pub pending_publishes: Vec<PendingPublishSnapshot>,
}

impl AllDrafts {
    /// Check if there are any drafts at all
    pub fn is_empty(&self) -> bool {
        self.chat_drafts.is_empty()
            && self.versioned_drafts.is_empty()
            && self.archived_drafts.is_empty()
            && self.named_drafts.is_empty()
    }

    /// Get total count of all drafts
    pub fn total_count(&self) -> usize {
        self.chat_drafts.len()
            + self.versioned_drafts.len()
            + self.archived_drafts.len()
            + self.named_drafts.len()
    }

    /// Search all drafts by text content (case-insensitive)
    pub fn search(&self, query: &str) -> Vec<SearchableDraft> {
        let query_lower = query.to_lowercase();
        let mut results = Vec::new();

        // Search chat drafts
        for draft in &self.chat_drafts {
            if draft.text.to_lowercase().contains(&query_lower) {
                results.push(SearchableDraft::Chat(draft.clone()));
            }
        }

        // Search versioned drafts
        for draft in &self.versioned_drafts {
            if draft.text.to_lowercase().contains(&query_lower) {
                results.push(SearchableDraft::Versioned(draft.clone()));
            }
        }

        // Search archived drafts
        for draft in &self.archived_drafts {
            if draft.text.to_lowercase().contains(&query_lower) {
                results.push(SearchableDraft::Archived(draft.clone()));
            }
        }

        // Search named drafts
        for draft in &self.named_drafts {
            if draft.text.to_lowercase().contains(&query_lower)
                || draft.name.to_lowercase().contains(&query_lower)
            {
                results.push(SearchableDraft::Named(draft.clone()));
            }
        }

        // Sort by last modified (most recent first)
        results.sort_by(|a, b| b.last_modified().cmp(&a.last_modified()));

        results
    }
}

/// A draft that can be searched (wraps different draft types)
#[derive(Debug, Clone)]
pub enum SearchableDraft {
    Chat(ChatDraft),
    Versioned(ChatDraft),
    Archived(ChatDraft),
    Named(NamedDraft),
}

impl SearchableDraft {
    /// Get the text content of this draft
    pub fn text(&self) -> &str {
        match self {
            SearchableDraft::Chat(d) => &d.text,
            SearchableDraft::Versioned(d) => &d.text,
            SearchableDraft::Archived(d) => &d.text,
            SearchableDraft::Named(d) => &d.text,
        }
    }

    /// Get the last modified timestamp
    pub fn last_modified(&self) -> u64 {
        match self {
            SearchableDraft::Chat(d) => d.last_modified,
            SearchableDraft::Versioned(d) => d.last_modified,
            SearchableDraft::Archived(d) => d.last_modified,
            SearchableDraft::Named(d) => d.last_modified,
        }
    }

    /// Get the conversation/project ID
    pub fn context_id(&self) -> &str {
        match self {
            SearchableDraft::Chat(d) => &d.conversation_id,
            SearchableDraft::Versioned(d) => &d.conversation_id,
            SearchableDraft::Archived(d) => &d.conversation_id,
            SearchableDraft::Named(d) => &d.project_a_tag,
        }
    }

    /// Get the draft type as a string for display
    pub fn draft_type(&self) -> &'static str {
        match self {
            SearchableDraft::Chat(_) => "Chat",
            SearchableDraft::Versioned(_) => "Versioned",
            SearchableDraft::Archived(_) => "Archived",
            SearchableDraft::Named(_) => "Named",
        }
    }

    /// Get the send state (for chat drafts)
    pub fn send_state(&self) -> Option<SendState> {
        match self {
            SearchableDraft::Chat(d) => Some(d.send_state),
            SearchableDraft::Versioned(d) => Some(d.send_state),
            SearchableDraft::Archived(d) => Some(d.send_state),
            SearchableDraft::Named(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_service() -> (DraftService, TempDir) {
        let temp_dir = TempDir::new().expect("Failed to create temp dir");
        let service = DraftService::new(temp_dir.path().to_str().unwrap());
        (service, temp_dir)
    }

    #[test]
    fn test_draft_service_new() {
        let (service, _temp_dir) = create_test_service();
        // Service should start with no errors
        assert!(service.chat_draft_last_error().is_none());
        assert!(service.named_draft_last_error().is_none());
    }

    /// Helper to create a test ChatDraft with all required fields
    fn create_test_chat_draft(conversation_id: &str, text: &str) -> ChatDraft {
        ChatDraft {
            conversation_id: conversation_id.to_string(),
            session_id: None,
            project_a_tag: None,
            message_sequence: 0,
            send_state: SendState::Typing,
            text: text.to_string(),
            attachments: vec![],
            image_attachments: vec![],
            selected_agent_pubkey: None,
            last_modified: 1234567890,
            reference_conversation_id: None,
            fork_message_id: None,
            published_at: None,
            published_event_id: None,
            confirmed_at: None,
        }
    }

    #[test]
    fn test_chat_draft_save_load_roundtrip() {
        let (service, _temp_dir) = create_test_service();

        let mut draft = create_test_chat_draft("test-conv-123", "Hello, this is a test draft");
        draft.selected_agent_pubkey = Some("agent-pubkey".to_string());

        // Save the draft
        let save_result = service.save_chat_draft(draft.clone());
        assert!(save_result.is_ok());

        // Load it back
        let loaded = service.load_chat_draft("test-conv-123");
        assert!(loaded.is_some());
        let loaded = loaded.unwrap();
        assert_eq!(loaded.conversation_id, "test-conv-123");
        assert_eq!(loaded.text, "Hello, this is a test draft");
        assert_eq!(loaded.selected_agent_pubkey, Some("agent-pubkey".to_string()));
    }

    #[test]
    fn test_chat_draft_delete() {
        let (service, _temp_dir) = create_test_service();

        let draft = create_test_chat_draft("to-delete", "Will be deleted");

        service.save_chat_draft(draft).unwrap();
        assert!(service.load_chat_draft("to-delete").is_some());

        // Delete and verify
        service.delete_chat_draft("to-delete").unwrap();
        assert!(service.load_chat_draft("to-delete").is_none());
    }

    #[test]
    fn test_named_draft_operations() {
        let (service, _temp_dir) = create_test_service();

        // Create and save multiple named drafts
        let draft1 = NamedDraft::new("First draft content".to_string(), "project-a".to_string());
        let draft2 = NamedDraft::new("Second draft content".to_string(), "project-a".to_string());
        let draft3 = NamedDraft::new("Third draft for project B".to_string(), "project-b".to_string());

        service.save_named_draft(draft1.clone()).unwrap();
        service.save_named_draft(draft2.clone()).unwrap();
        service.save_named_draft(draft3.clone()).unwrap();

        // Verify project filtering
        let project_a_drafts = service.get_named_drafts_for_project("project-a");
        assert_eq!(project_a_drafts.len(), 2);

        let project_b_drafts = service.get_named_drafts_for_project("project-b");
        assert_eq!(project_b_drafts.len(), 1);
        assert_eq!(project_b_drafts[0].text, "Third draft for project B");

        // Verify get all
        let all_drafts = service.get_all_named_drafts();
        assert_eq!(all_drafts.len(), 3);
    }

    #[test]
    fn test_named_draft_delete() {
        let (service, _temp_dir) = create_test_service();

        let draft = NamedDraft::new("To be deleted".to_string(), "project-x".to_string());
        let draft_id = draft.id.clone();

        service.save_named_draft(draft).unwrap();
        assert_eq!(service.get_all_named_drafts().len(), 1);

        // Delete by ID
        service.delete_named_draft(&draft_id).unwrap();
        assert_eq!(service.get_all_named_drafts().len(), 0);
    }

    #[test]
    fn test_error_clearing_with_forced_error() {
        // Create service with invalid path to force error on save
        let service = DraftService::new("/nonexistent/path/that/should/fail");

        // Initially no errors (load doesn't fail, file just doesn't exist)
        assert!(service.chat_draft_last_error().is_none());
        assert!(service.named_draft_last_error().is_none());

        // Try to save a draft - this should fail and set an error
        let draft = ChatDraft {
            conversation_id: "test-conv".to_string(),
            session_id: None,
            project_a_tag: None,
            message_sequence: 0,
            send_state: SendState::Typing,
            text: "Test".to_string(),
            attachments: vec![],
            image_attachments: vec![],
            selected_agent_pubkey: None,
            last_modified: 1234567890,
            reference_conversation_id: None,
            fork_message_id: None,
            published_at: None,
            published_event_id: None,
            confirmed_at: None,
        };

        // This should fail due to invalid path
        let save_result = service.save_chat_draft(draft);
        assert!(save_result.is_err());

        // Now there should be an error
        assert!(service.chat_draft_last_error().is_some());

        // Clear the error
        service.chat_draft_clear_error();

        // Error should be cleared
        assert!(service.chat_draft_last_error().is_none());

        // Similarly test named draft error clearing
        let named_draft = NamedDraft::new("Test".to_string(), "project".to_string());
        let named_save_result = service.save_named_draft(named_draft);
        assert!(named_save_result.is_err());
        assert!(service.named_draft_last_error().is_some());
        service.named_draft_clear_error();
        assert!(service.named_draft_last_error().is_none());
    }

    // =========================================================================
    // Publish Snapshot Tests
    // =========================================================================

    #[test]
    fn test_publish_snapshot_create_and_confirm() {
        let (service, _temp_dir) = create_test_service();

        // Create a snapshot
        let publish_id = service
            .create_publish_snapshot("conv-123", "Hello, world!".to_string())
            .expect("create_publish_snapshot should succeed");

        // Verify snapshot is pending (unconfirmed)
        let pending = service.get_pending_publishes();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].publish_id, publish_id);
        assert_eq!(pending[0].content, "Hello, world!");
        assert_eq!(pending[0].conversation_id, "conv-123");
        assert!(pending[0].published_at.is_none()); // Not yet confirmed

        // Mark as confirmed
        let confirmed = service
            .mark_publish_confirmed(&publish_id, Some("event-abc".to_string()))
            .expect("mark_publish_confirmed should succeed");
        assert!(confirmed);

        // After confirmation, snapshot is no longer "pending" (it's confirmed)
        // get_pending_publishes only returns unconfirmed snapshots
        let pending_after = service.get_pending_publishes();
        assert_eq!(pending_after.len(), 0); // No longer pending
    }

    #[test]
    fn test_publish_snapshot_remove_rollback() {
        let (service, _temp_dir) = create_test_service();

        // Create a snapshot
        let publish_id = service
            .create_publish_snapshot("conv-456", "Message content".to_string())
            .expect("create_publish_snapshot should succeed");

        // Verify it exists
        assert_eq!(service.get_pending_publishes().len(), 1);

        // Remove it (rollback scenario)
        let removed = service
            .remove_publish_snapshot(&publish_id)
            .expect("remove_publish_snapshot should succeed");
        assert!(removed);

        // Verify it's gone
        assert_eq!(service.get_pending_publishes().len(), 0);
    }

    #[test]
    fn test_cleanup_confirmed_publishes() {
        let (service, _temp_dir) = create_test_service();

        // Initially no pending publishes
        let cleanup_count = service
            .cleanup_confirmed_publishes()
            .expect("cleanup should succeed");
        assert_eq!(cleanup_count, 0);

        // Create and confirm a snapshot (won't be cleaned up due to grace period)
        let publish_id = service
            .create_publish_snapshot("conv-789", "Test message".to_string())
            .expect("create_publish_snapshot should succeed");

        service
            .mark_publish_confirmed(&publish_id, Some("event-xyz".to_string()))
            .expect("mark_publish_confirmed should succeed");

        // Cleanup won't remove recently confirmed (due to 24h grace period)
        let cleanup_count = service
            .cleanup_confirmed_publishes()
            .expect("cleanup should succeed");
        assert_eq!(cleanup_count, 0); // Not old enough to clean up
    }

    #[test]
    fn test_get_unpublished_drafts() {
        let (service, _temp_dir) = create_test_service();

        // Create a draft without publishing
        let draft = create_test_chat_draft("unpublished-conv", "Draft content");

        service.save_chat_draft(draft).unwrap();

        // Get unpublished drafts
        let unpublished = service.get_unpublished_drafts();
        assert_eq!(unpublished.len(), 1);
        assert_eq!(unpublished[0].conversation_id, "unpublished-conv");
    }

    // =========================================================================
    // Content/Update Method Tests
    // =========================================================================

    #[test]
    fn test_clear_draft_content() {
        let (service, _temp_dir) = create_test_service();

        // Create a draft with content and selections
        let mut draft = create_test_chat_draft("clear-test", "Some content to clear");
        draft.selected_agent_pubkey = Some("agent-123".to_string());
        draft.reference_conversation_id = Some("ref-conv".to_string());
        draft.fork_message_id = Some("fork-msg".to_string());

        service.save_chat_draft(draft).unwrap();

        // Clear the content
        service.clear_draft_content("clear-test").unwrap();

        // Load and verify content is cleared but selections preserved
        let loaded = service.load_chat_draft("clear-test").unwrap();
        assert!(loaded.text.is_empty()); // Content cleared
        assert!(loaded.attachments.is_empty()); // Attachments cleared
        assert!(loaded.reference_conversation_id.is_none()); // Reference cleared
        assert!(loaded.fork_message_id.is_none()); // Fork message cleared
        // Note: The underlying implementation preserves agent but clears text
        assert_eq!(loaded.selected_agent_pubkey, Some("agent-123".to_string())); // Preserved
    }

    #[test]
    fn test_update_named_draft() {
        let (service, _temp_dir) = create_test_service();

        // Create a named draft
        let draft = NamedDraft::new("Original content".to_string(), "project-update".to_string());
        let draft_id = draft.id.clone();

        service.save_named_draft(draft).unwrap();

        // Verify original content
        let drafts = service.get_all_named_drafts();
        assert_eq!(drafts.len(), 1);
        assert_eq!(drafts[0].text, "Original content");

        // Update the draft
        service
            .update_named_draft(&draft_id, "Updated content".to_string())
            .unwrap();

        // Verify updated content
        let drafts_after = service.get_all_named_drafts();
        assert_eq!(drafts_after.len(), 1);
        assert_eq!(drafts_after[0].text, "Updated content");
        assert_eq!(drafts_after[0].id, draft_id); // Same ID
    }

    #[test]
    fn test_fork_message_id_round_trip_persistence() {
        let (service, _temp_dir) = create_test_service();

        // Create a draft with fork metadata (simulating a forked conversation)
        let mut draft = create_test_chat_draft("fork-persist-test", "Forked conversation content");
        draft.reference_conversation_id = Some("source-conv-123".to_string());
        draft.fork_message_id = Some("fork-msg-456".to_string());
        draft.selected_agent_pubkey = Some("agent-xyz".to_string());

        // Save the draft
        service.save_chat_draft(draft).unwrap();

        // Load the draft and verify all fork metadata persists
        let loaded = service.load_chat_draft("fork-persist-test").unwrap();
        assert_eq!(loaded.text, "Forked conversation content");
        assert_eq!(loaded.reference_conversation_id, Some("source-conv-123".to_string()));
        assert_eq!(loaded.fork_message_id, Some("fork-msg-456".to_string()));
        assert_eq!(loaded.selected_agent_pubkey, Some("agent-xyz".to_string()));

        // Update the draft text and save again (simulating user typing)
        let mut updated_draft = loaded;
        updated_draft.text = "Updated forked conversation content".to_string();
        service.save_chat_draft(updated_draft).unwrap();

        // Load again and verify fork metadata is still preserved after update
        let reloaded = service.load_chat_draft("fork-persist-test").unwrap();
        assert_eq!(reloaded.text, "Updated forked conversation content");
        assert_eq!(reloaded.reference_conversation_id, Some("source-conv-123".to_string()));
        assert_eq!(reloaded.fork_message_id, Some("fork-msg-456".to_string()));
        assert_eq!(reloaded.selected_agent_pubkey, Some("agent-xyz".to_string()));
    }
}
