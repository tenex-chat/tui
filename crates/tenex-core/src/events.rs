use crate::models::{Message, ProjectStatus};

/// A pending backend approval request - triggered when we receive a status event
/// from an unknown backend that isn't in approved or blocked lists
#[derive(Debug, Clone)]
pub struct PendingBackendApproval {
    pub backend_pubkey: String,
    pub project_a_tag: String,
    pub first_seen: u64,
}

#[derive(Debug)]
pub enum CoreEvent {
    Message(Message),
    ProjectStatus(ProjectStatus),
    /// Backend approval request - UI should show modal to approve/block
    PendingBackendApproval(PendingBackendApproval),
}
