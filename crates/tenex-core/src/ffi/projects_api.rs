use super::*;

#[uniffi::export]
impl TenexCore {
    /// Create a new project (kind:31933 replaceable event).
    pub fn create_project(
        &self,
        name: String,
        description: String,
        agent_pubkeys: Vec<String>,
        mcp_tool_ids: Vec<String>,
        is_private: bool,
    ) -> Result<(), TenexError> {
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::SaveProject {
                slug: None,
                name,
                description,
                agent_pubkeys,
                mcp_tool_ids,
                client: Some("tenex-ios".to_string()),
                is_private,
                repo_url: None,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send create project command: {}", e),
            })?;

        Ok(())
    }

    /// Update an existing project (kind:31933 replaceable event).
    ///
    /// Republish the same d-tag with updated metadata, agents, and MCP tool assignments.
    pub fn update_project(
        &self,
        project_id: String,
        title: String,
        description: String,
        repo_url: Option<String>,
        picture_url: Option<String>,
        agent_pubkeys: Vec<String>,
        mcp_tool_ids: Vec<String>,
        is_private: bool,
    ) -> Result<(), TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::UpdateProject {
                project_a_tag,
                title,
                description,
                repo_url,
                picture_url,
                agent_pubkeys,
                mcp_tool_ids,
                client: Some("tenex-ios".to_string()),
                is_private,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send update project command: {}", e),
            })?;

        Ok(())
    }

    /// Tombstone-delete a project by republishing it with ["deleted"] tag.
    pub fn delete_project(&self, project_id: String) -> Result<(), TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::DeleteProject {
                project_a_tag,
                client: Some("tenex-ios".to_string()),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send delete project command: {}", e),
            })?;

        Ok(())
    }

    /// Whether the project is currently online — i.e. has a fresh kind:24010
    /// heartbeat from any approved backend (within the 45-second staleness
    /// window).
    ///
    /// Note: kind:24011 (installed-agent inventory) is *not* a liveness signal.
    /// A backend can publish 24011 once and then never run the project, so
    /// inventory presence does not imply the project is running.
    pub fn is_project_online(&self, project_id: String) -> bool {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return false,
        };

        // Find the project by ID
        let project = match store.get_projects().iter().find(|p| p.id == project_id) {
            Some(p) => p,
            None => return false,
        };

        store.is_project_online(&project.a_tag())
    }

    /// Return the pubkey of a backend currently running the project — i.e.
    /// one whose kind:24010 heartbeat is fresh. Use this when sending a
    /// command (e.g. chat message) that should reach an actively running
    /// backend. Returns `None` if no backend is currently running the project.
    pub fn get_project_backend_pubkey(&self, project_id: String) -> Option<String> {
        let store_guard = self.store.read().ok()?;
        let store = store_guard.as_ref()?;
        let project = store.get_projects().iter().find(|p| p.id == project_id)?;
        store.first_online_backend_for_project(&project.a_tag())
    }

    /// Boot/start a project (sends kind:24000 event).
    ///
    /// This sends a boot request to wake up the project's backend.
    /// The backend should publish kind:24011 inventory for available agents.
    ///
    /// Use this when a project is offline and you want to start it.
    pub fn boot_project(&self, project_id: String) -> Result<(), TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        // Find the project by ID
        let project = store
            .get_projects()
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or_else(|| TenexError::Internal {
                message: format!("Project not found: {}", project_id),
            })?;

        let core_handle = get_core_handle(&self.core_handle)?;

        // Send the boot project command
        core_handle
            .send(NostrCommand::BootProject {
                project_a_tag: project.a_tag(),
                project_pubkey: Some(project.pubkey.clone()),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send boot project command: {}", e),
            })?;

        Ok(())
    }
}
