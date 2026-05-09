use super::*;

#[uniffi::export]
impl TenexCore {
    /// Return saved workspace/project-scope definitions.
    pub fn get_workspaces(&self) -> Result<Vec<WorkspaceInfo>, TenexError> {
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;

        Ok(prefs_guard
            .as_ref()
            .map(FfiPreferencesStorage::workspaces)
            .unwrap_or_default())
    }

    /// Return the active workspace ID, if any.
    pub fn get_active_workspace_id(&self) -> Result<Option<String>, TenexError> {
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;

        Ok(prefs_guard
            .as_ref()
            .and_then(|prefs| prefs.prefs.active_workspace_id.clone()))
    }

    /// Create a saved workspace. Membership is stored as project a-tags.
    pub fn add_workspace(
        &self,
        name: String,
        project_a_tags: Vec<String>,
    ) -> Result<WorkspaceInfo, TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs
            .add_workspace(name, project_a_tags)
            .map_err(|message| TenexError::Internal { message })
    }

    /// Update an existing workspace's name and project membership.
    pub fn update_workspace(
        &self,
        id: String,
        name: String,
        project_a_tags: Vec<String>,
    ) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs
            .update_workspace(&id, name, project_a_tags)
            .map_err(|message| TenexError::Internal { message })
    }

    /// Delete a saved workspace and clear it if active.
    pub fn delete_workspace(&self, id: String) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs
            .delete_workspace(&id)
            .map_err(|message| TenexError::Internal { message })
    }

    /// Toggle the pinned state for a workspace.
    pub fn toggle_workspace_pinned(&self, id: String) -> Result<bool, TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs
            .toggle_workspace_pinned(&id)
            .map_err(|message| TenexError::Internal { message })
    }

    /// Set the active workspace. Pass nil to return to all-project/manual mode.
    pub fn set_active_workspace(&self, id: Option<String>) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs
            .set_active_workspace(id)
            .map_err(|message| TenexError::Internal { message })
    }
}
