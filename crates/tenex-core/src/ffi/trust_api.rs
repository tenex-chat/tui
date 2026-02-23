use super::*;

#[uniffi::export]
impl TenexCore {
    // =========================================================================
    // Backend Trust Management
    // =========================================================================

    /// Set the trusted backends from preferences.
    ///
    /// This must be called after login to enable processing of kind:24010 (project status)
    /// events. Status events from approved backends will populate project_statuses,
    /// enabling get_online_agents() to return online agents.
    ///
    /// Call this on app startup with stored approved/blocked backend pubkeys.
    pub fn set_trusted_backends(
        &self,
        approved: Vec<String>,
        blocked: Vec<String>,
    ) -> Result<(), TenexError> {
        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let approved_set: std::collections::HashSet<String> = approved.into_iter().collect();
        let blocked_set: std::collections::HashSet<String> = blocked.into_iter().collect();
        store
            .trust
            .set_trusted_backends(approved_set.clone(), blocked_set.clone());

        drop(store_guard);
        self.persist_trusted_backends_to_preferences(approved_set, blocked_set)?;

        Ok(())
    }

    /// Add a backend to the approved list.
    ///
    /// Once approved, kind:24010 events from this backend will be processed,
    /// populating project_statuses and enabling get_online_agents().
    pub fn approve_backend(&self, pubkey: String) -> Result<(), TenexError> {
        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        store.add_approved_backend(&pubkey);
        drop(store_guard);
        self.persist_current_trusted_backends()?;
        Ok(())
    }

    /// Add a backend to the blocked list.
    ///
    /// Status events from blocked backends will be silently ignored.
    pub fn block_backend(&self, pubkey: String) -> Result<(), TenexError> {
        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        store.add_blocked_backend(&pubkey);
        drop(store_guard);
        self.persist_current_trusted_backends()?;
        Ok(())
    }

    /// Approve all pending backends.
    ///
    /// This is useful for mobile apps that don't have a UI for backend approval modals.
    /// Approves any backends that sent kind:24010 events but weren't in the approved list.
    /// Returns the number of backends that were approved.
    pub fn approve_all_pending_backends(&self) -> Result<u32, TenexError> {
        use crate::tlog;
        tlog!("FFI", "approve_all_pending_backends called");
        eprintln!("[DEBUG] approve_all_pending_backends called");

        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let pending = store.drain_pending_backend_approvals();
        tlog!("FFI", "Found {} pending backend approvals", pending.len());
        eprintln!("[DEBUG] Found {} pending backend approvals", pending.len());
        for approval in &pending {
            tlog!(
                "FFI",
                "  - Backend: {} for project: {}",
                approval.backend_pubkey,
                approval.project_a_tag
            );
            eprintln!(
                "[DEBUG]   - Backend: {} for project: {}",
                approval.backend_pubkey, approval.project_a_tag
            );
        }

        let count = store.approve_pending_backends(pending);
        tlog!(
            "FFI",
            "Approved {} backends, project_statuses now has {} entries",
            count,
            store.project_statuses.len()
        );
        eprintln!("[DEBUG] Approved {} backends", count);
        eprintln!(
            "[DEBUG] project_statuses HashMap now has {} entries",
            store.project_statuses.len()
        );
        drop(store_guard);
        self.persist_current_trusted_backends()?;

        Ok(count)
    }

    /// Get approved/blocked/pending backend trust state for settings UI.
    pub fn get_backend_trust_snapshot(&self) -> Result<BackendTrustSnapshot, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let mut approved: Vec<String> = store.trust.approved_backends.iter().cloned().collect();
        let mut blocked: Vec<String> = store.trust.blocked_backends.iter().cloned().collect();
        approved.sort();
        blocked.sort();

        let mut pending: Vec<PendingBackendInfo> = store
            .trust
            .pending_backend_approvals
            .iter()
            .map(|p| PendingBackendInfo {
                backend_pubkey: p.backend_pubkey.clone(),
                project_a_tag: p.project_a_tag.clone(),
                first_seen: p.first_seen,
                status_created_at: p.status.created_at,
            })
            .collect();
        pending.sort_by(|a, b| b.first_seen.cmp(&a.first_seen));

        Ok(BackendTrustSnapshot {
            approved,
            blocked,
            pending,
        })
    }

    /// Return currently configured relay URLs (read-only in this phase).
    pub fn get_configured_relays(&self) -> Vec<String> {
        vec![crate::constants::RELAY_URL.to_string()]
    }

    /// Get diagnostics about backend approvals and project statuses.
    /// Returns JSON with project statuses keys.
    pub fn get_backend_diagnostics(&self) -> Result<String, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let diagnostic = serde_json::json!({
            "has_pending_backend_approvals": store.trust.has_pending_approvals(),
            "project_statuses_count": store.project_statuses.len(),
            "project_statuses_keys": store.project_statuses.keys().collect::<Vec<_>>(),
            "projects_count": store.get_projects().len(),
            "projects": store.get_projects().iter().map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "name": p.title,
                    "a_tag": p.a_tag(),
                })
            }).collect::<Vec<_>>(),
        });

        Ok(diagnostic.to_string())
    }
}
