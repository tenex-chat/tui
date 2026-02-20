import Foundation

// MARK: - Delegation Tree Node

/// Pure data model for a node in the delegation tree.
/// No ViewModel logic — this is only data.
struct DelegationTreeNode: Identifiable {
    /// The conversation this node represents
    let conversation: ConversationFullInfo
    /// The delegate tool-call message in the *parent's* conversation (outgoing arrow content)
    let delegationMessage: Message?
    /// The kind:1 in *this* conversation where this agent p-tags the parent's author (return arrow)
    let returnMessage: Message?
    var children: [DelegationTreeNode]

    var id: String { conversation.thread.id }
    /// Set during layout computation
    var depth: Int = 0
}
