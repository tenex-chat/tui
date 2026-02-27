import Foundation

// MARK: - Delegation Tree Node

enum DelegationTreeNodeRole {
    case rootAuthor
    case recipient
}

/// Pure data model for a node in the delegation tree.
/// No ViewModel logic — this is only data.
struct DelegationTreeNode: Identifiable {
    /// Stable node identifier.
    let id: String
    /// Conversation opened when this node is selected.
    let conversation: ConversationFullInfo
    /// Participant shown in this node.
    let participantPubkey: String
    /// Role of this participant card in the rendered tree.
    let role: DelegationTreeNodeRole
    /// Last non-tool message from the conversation author, used as completion signal.
    let returnMessage: Message?
    /// Most recent non-tool, non-reasoning message in the conversation (any author).
    let lastMessage: Message?
    var children: [DelegationTreeNode]

    /// Set during layout computation
    var depth: Int = 0
}
