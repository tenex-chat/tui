use super::*;

#[uniffi::export]
impl TenexCore {
    /// Set which projects are visible in the Conversations tab.
    /// Pass empty array to show all projects.
    pub fn set_visible_projects(&self, project_a_tags: Vec<String>) {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            prefs.prefs.visible_projects = project_a_tags.into_iter().collect();
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
        }
    }

    /// Archive a conversation (hide from default view).
    pub fn archive_conversation(&self, conversation_id: String) {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            prefs.prefs.archived_thread_ids.insert(conversation_id);
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
        }
    }

    /// Unarchive a conversation (show in default view).
    pub fn unarchive_conversation(&self, conversation_id: String) {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            prefs.prefs.archived_thread_ids.remove(&conversation_id);
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
        }
    }

    /// Toggle archive status for a conversation.
    /// Returns true if the conversation is now archived.
    pub fn toggle_conversation_archived(&self, conversation_id: String) -> bool {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return false,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            let is_now_archived = if prefs.prefs.archived_thread_ids.contains(&conversation_id) {
                prefs.prefs.archived_thread_ids.remove(&conversation_id);
                false
            } else {
                prefs.prefs.archived_thread_ids.insert(conversation_id);
                true
            };
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
            is_now_archived
        } else {
            false
        }
    }

    /// Check if a conversation is archived.
    pub fn is_conversation_archived(&self, conversation_id: String) -> bool {
        let prefs_guard = match self.preferences.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.contains(&conversation_id))
            .unwrap_or(false)
    }

    /// Get all archived conversation IDs.
    /// Returns Result to distinguish "no data" from "lock error".
    pub fn get_archived_conversation_ids(&self) -> Result<Vec<String>, TenexError> {
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;

        Ok(prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.iter().cloned().collect())
            .unwrap_or_default())
    }

    // ===== Collapsed Thread State Methods (Fix #5: Expose via FFI) =====

    /// Get all collapsed thread IDs.
    pub fn get_collapsed_thread_ids(&self) -> Result<Vec<String>, TenexError> {
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;

        Ok(prefs_guard
            .as_ref()
            .map(|p| p.prefs.collapsed_thread_ids.iter().cloned().collect())
            .unwrap_or_default())
    }

    /// Set collapsed thread IDs (replace all).
    pub fn set_collapsed_thread_ids(&self, thread_ids: Vec<String>) {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            prefs.prefs.collapsed_thread_ids = thread_ids.into_iter().collect();
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
        }
    }

    /// Toggle collapsed state for a thread.
    /// Returns true if the thread is now collapsed.
    pub fn toggle_thread_collapsed(&self, thread_id: String) -> bool {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return false,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            let is_now_collapsed = if prefs.prefs.collapsed_thread_ids.contains(&thread_id) {
                prefs.prefs.collapsed_thread_ids.remove(&thread_id);
                false
            } else {
                prefs.prefs.collapsed_thread_ids.insert(thread_id);
                true
            };
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
            is_now_collapsed
        } else {
            false
        }
    }

    /// Check if a thread is collapsed.
    pub fn is_thread_collapsed(&self, thread_id: String) -> bool {
        let prefs_guard = match self.preferences.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        prefs_guard
            .as_ref()
            .map(|p| p.prefs.collapsed_thread_ids.contains(&thread_id))
            .unwrap_or(false)
    }
}
