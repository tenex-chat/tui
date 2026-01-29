import Foundation

// MARK: - Delegation Item Model

/// Represents a delegation from one agent to another within a conversation
/// This is a view-model friendly representation of delegation data
struct DelegationItem: Identifiable {
    /// Unique identifier for the delegation
    let id: String

    /// The recipient agent of the delegation
    let recipient: String

    /// Preview of the delegation message/task
    let messagePreview: String

    /// The conversation ID associated with this delegation
    let conversationId: String

    /// Timestamp of the delegation (Unix seconds)
    let timestamp: UInt64
}
