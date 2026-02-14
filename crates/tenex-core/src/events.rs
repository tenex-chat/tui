use crate::models::{Message, ProjectStatus, Report};

/// A pending backend approval request - triggered when we receive a status event
/// from an unknown backend that isn't in approved or blocked lists.
/// Stores the full ProjectStatus so it can be applied when the backend is approved.
#[derive(Debug, Clone)]
pub struct PendingBackendApproval {
    pub backend_pubkey: String,
    pub project_a_tag: String,
    pub first_seen: u64,
    /// The project status to apply when this backend is approved
    pub status: ProjectStatus,
}

#[derive(Debug)]
pub enum CoreEvent {
    Message(Message),
    ProjectStatus(ProjectStatus),
    /// Backend approval request - UI should show modal to approve/block
    PendingBackendApproval(PendingBackendApproval),
    /// Report created or updated (kind:30023)
    ReportUpsert(Report),
}
