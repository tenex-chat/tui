use crate::events::PendingBackendApproval;
use crate::models::ProjectStatus;
use std::collections::HashSet;

/// Sub-store for backend trust state: approved/blocked backends and pending approvals.
pub struct TrustStore {
    pub approved_backends: HashSet<String>,
    pub blocked_backends: HashSet<String>,
    pub pending_backend_approvals: Vec<PendingBackendApproval>,
}

impl TrustStore {
    pub fn new() -> Self {
        Self {
            approved_backends: HashSet::new(),
            blocked_backends: HashSet::new(),
            pending_backend_approvals: Vec::new(),
        }
    }

    pub fn clear(&mut self) {
        self.approved_backends.clear();
        self.blocked_backends.clear();
        self.pending_backend_approvals.clear();
    }

    // ===== Query Methods =====

    pub fn is_approved(&self, pubkey: &str) -> bool {
        self.approved_backends.contains(pubkey)
    }

    pub fn is_blocked(&self, pubkey: &str) -> bool {
        self.blocked_backends.contains(pubkey)
    }

    pub fn has_pending_approval(&self, backend_pubkey: &str, project_a_tag: &str) -> bool {
        self.pending_backend_approvals
            .iter()
            .any(|p| p.backend_pubkey == backend_pubkey && p.project_a_tag == project_a_tag)
    }

    pub fn has_pending_approvals(&self) -> bool {
        !self.pending_backend_approvals.is_empty()
    }

    // ===== Mutation Methods =====

    pub fn set_trusted_backends(&mut self, approved: HashSet<String>, blocked: HashSet<String>) {
        self.approved_backends = approved;
        self.blocked_backends = blocked;
    }

    /// Add a backend to the approved list. Removes from blocked and pending.
    /// Returns pending statuses that were waiting for this backend (for cross-cutting application).
    pub fn add_approved(&mut self, pubkey: &str) -> Vec<ProjectStatus> {
        self.blocked_backends.remove(pubkey);
        self.approved_backends.insert(pubkey.to_string());

        // Extract pending statuses for this backend
        let pending_statuses: Vec<ProjectStatus> = self
            .pending_backend_approvals
            .iter()
            .filter(|p| p.backend_pubkey == pubkey)
            .map(|p| p.status.clone())
            .collect();

        self.pending_backend_approvals
            .retain(|p| p.backend_pubkey != pubkey);

        pending_statuses
    }

    /// Add a backend to the blocked list. Removes from approved and pending.
    pub fn add_blocked(&mut self, pubkey: &str) {
        self.approved_backends.remove(pubkey);
        self.blocked_backends.insert(pubkey.to_string());
        self.pending_backend_approvals
            .retain(|p| p.backend_pubkey != pubkey);
    }

    pub fn drain_pending(&mut self) -> Vec<PendingBackendApproval> {
        std::mem::take(&mut self.pending_backend_approvals)
    }

    /// Queue a new pending approval or update an existing one with a newer status.
    pub fn queue_or_update_pending(
        &mut self,
        backend_pubkey: &str,
        project_a_tag: &str,
        status: ProjectStatus,
    ) {
        if let Some(existing) = self
            .pending_backend_approvals
            .iter_mut()
            .find(|p| p.backend_pubkey == backend_pubkey && p.project_a_tag == project_a_tag)
        {
            if status.created_at >= existing.status.created_at {
                existing.status = status;
            }
        } else {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);

            self.pending_backend_approvals.push(PendingBackendApproval {
                backend_pubkey: backend_pubkey.to_string(),
                project_a_tag: project_a_tag.to_string(),
                first_seen: now,
                status,
            });
        }
    }
}
