use super::*;

#[uniffi::export]
impl TenexCore {
    // =========================================================================
    // NIP-46 BUNKER (REMOTE SIGNER)
    // =========================================================================

    /// Start the NIP-46 bunker. Returns the bunker:// URI for clients to connect.
    pub fn start_bunker(&self) -> Result<String, TenexError> {
        let handle = self
            .core_handle
            .read()
            .map_err(|_| TenexError::LockError {
                resource: "core_handle".to_string(),
            })?
            .as_ref()
            .ok_or(TenexError::NotLoggedIn)?
            .clone();

        let (resp_tx, resp_rx) = std::sync::mpsc::channel();
        let _ = handle.send(NostrCommand::StartBunker {
            response_tx: resp_tx,
        });

        resp_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| TenexError::Internal {
                message: "Bunker start timed out".to_string(),
            })?
            .map_err(|e| TenexError::Internal { message: e })
    }

    /// Stop the NIP-46 bunker.
    pub fn stop_bunker(&self) -> Result<(), TenexError> {
        let handle = self
            .core_handle
            .read()
            .map_err(|_| TenexError::LockError {
                resource: "core_handle".to_string(),
            })?
            .as_ref()
            .ok_or(TenexError::NotLoggedIn)?
            .clone();

        let (resp_tx, resp_rx) = std::sync::mpsc::channel();
        let _ = handle.send(NostrCommand::StopBunker {
            response_tx: resp_tx,
        });

        resp_rx
            .recv_timeout(Duration::from_secs(5))
            .map_err(|_| TenexError::Internal {
                message: "Bunker stop timed out".to_string(),
            })?
            .map_err(|e| TenexError::Internal { message: e })
    }

    /// Respond to a pending bunker signing request.
    pub fn respond_to_bunker_request(
        &self,
        request_id: String,
        approved: bool,
    ) -> Result<(), TenexError> {
        let handle = self
            .core_handle
            .read()
            .map_err(|_| TenexError::LockError {
                resource: "core_handle".to_string(),
            })?
            .as_ref()
            .ok_or(TenexError::NotLoggedIn)?
            .clone();

        let _ = handle.send(NostrCommand::BunkerResponse {
            request_id,
            approved,
        });

        Ok(())
    }

    /// Get the NIP-46 bunker audit log.
    pub fn get_bunker_audit_log(&self) -> Result<Vec<FfiBunkerAuditEntry>, TenexError> {
        let handle = self
            .core_handle
            .read()
            .map_err(|_| TenexError::LockError {
                resource: "core_handle".to_string(),
            })?
            .as_ref()
            .ok_or(TenexError::NotLoggedIn)?
            .clone();

        let (resp_tx, resp_rx) = std::sync::mpsc::channel();
        let _ = handle.send(NostrCommand::GetBunkerAuditLog {
            response_tx: resp_tx,
        });

        let entries =
            resp_rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| TenexError::Internal {
                    message: "Bunker audit log timed out".to_string(),
                })?;

        Ok(entries
            .into_iter()
            .map(|e| FfiBunkerAuditEntry {
                timestamp_ms: e.timestamp_ms,
                completed_at_ms: e.completed_at_ms,
                request_id: e.request_id,
                source_event_id: e.source_event_id,
                requester_pubkey: e.requester_pubkey,
                request_type: e.request_type,
                event_kind: e.event_kind,
                event_content_preview: e.event_content_preview,
                event_content_full: e.event_content_full,
                event_tags_json: e.event_tags_json,
                request_payload_json: e.request_payload_json,
                response_payload_json: e.response_payload_json,
                decision: e.decision,
                response_time_ms: e.response_time_ms,
            })
            .collect())
    }

    /// Add a bunker auto-approve rule.
    pub fn add_bunker_auto_approve_rule(
        &self,
        requester_pubkey: String,
        event_kind: Option<u16>,
    ) -> Result<(), TenexError> {
        let handle = self
            .core_handle
            .read()
            .map_err(|_| TenexError::LockError {
                resource: "core_handle".to_string(),
            })?
            .as_ref()
            .ok_or(TenexError::NotLoggedIn)?
            .clone();

        let _ = handle.send(NostrCommand::AddBunkerAutoApproveRule {
            requester_pubkey,
            event_kind,
        });

        Ok(())
    }

    /// Remove a bunker auto-approve rule.
    pub fn remove_bunker_auto_approve_rule(
        &self,
        requester_pubkey: String,
        event_kind: Option<u16>,
    ) -> Result<(), TenexError> {
        let handle = self
            .core_handle
            .read()
            .map_err(|_| TenexError::LockError {
                resource: "core_handle".to_string(),
            })?
            .as_ref()
            .ok_or(TenexError::NotLoggedIn)?
            .clone();

        let _ = handle.send(NostrCommand::RemoveBunkerAutoApproveRule {
            requester_pubkey,
            event_kind,
        });

        Ok(())
    }

    /// Get all bunker auto-approve rules.
    pub fn get_bunker_auto_approve_rules(
        &self,
    ) -> Result<Vec<FfiBunkerAutoApproveRule>, TenexError> {
        let handle = self
            .core_handle
            .read()
            .map_err(|_| TenexError::LockError {
                resource: "core_handle".to_string(),
            })?
            .as_ref()
            .ok_or(TenexError::NotLoggedIn)?
            .clone();

        let (resp_tx, resp_rx) = std::sync::mpsc::channel();
        let _ = handle.send(NostrCommand::GetBunkerAutoApproveRules {
            response_tx: resp_tx,
        });

        let rules =
            resp_rx
                .recv_timeout(Duration::from_secs(5))
                .map_err(|_| TenexError::Internal {
                    message: "Bunker auto-approve rules timed out".to_string(),
                })?;

        Ok(rules
            .into_iter()
            .map(|r| FfiBunkerAutoApproveRule {
                requester_pubkey: r.requester_pubkey,
                event_kind: r.event_kind,
            })
            .collect())
    }
}
