import SwiftUI

// MARK: - Layout Constants

private let nodeW: CGFloat = 270
private let nodeH: CGFloat = 148
private let hGap: CGFloat = 130
private let vGap: CGFloat = 30
private let padding: CGFloat = 50

// MARK: - Delegation Tree ViewModel

@MainActor
final class DelegationTreeViewModel: ObservableObject {
    @Published var rootNode: DelegationTreeNode?
    @Published var isLoading = true
    @Published var loadError: String?
    @Published var nodePositions: [String: CGPoint] = [:]
    @Published var canvasSize: CGSize = CGSize(width: 800, height: 600)

    var safeCore: SafeTenexCore?

    init() {}

    func loadTree(rootConversationId: String) async {
        guard let safeCore else {
            loadError = "Core not initialized"
            isLoading = false
            return
        }

        isLoading = true
        loadError = nil

        // Step 1: Get all descendant IDs
        let descendantIds = await safeCore.getDescendantConversationIds(conversationId: rootConversationId)

        // Step 2: Load full info for root + all descendants
        let allIds = [rootConversationId] + descendantIds
        let allConversations = await safeCore.getConversationsByIds(conversationIds: allIds)

        guard let rootConversation = allConversations.first(where: { $0.thread.id == rootConversationId }) else {
            loadError = "Root conversation not found"
            isLoading = false
            return
        }

        // Step 3: Load messages for all conversations concurrently
        var messagesByConversation: [String: [Message]] = [:]
        await withTaskGroup(of: (String, [Message]).self) { group in
            for conversation in allConversations {
                group.addTask { [safeCore] in
                    let messages = await safeCore.getMessages(conversationId: conversation.thread.id)
                    return (conversation.thread.id, messages)
                }
            }
            for await (convId, messages) in group {
                messagesByConversation[convId] = messages
            }
        }

        // Step 4: Build conversation lookup map
        let conversationById = Dictionary(uniqueKeysWithValues: allConversations.map { ($0.thread.id, $0) })

        // Step 5: Build tree recursively
        var root = buildNode(
            conversation: rootConversation,
            delegationMessage: nil,
            returnMessage: nil,
            parentConversation: nil,
            conversationById: conversationById,
            messagesByConversation: messagesByConversation,
            depth: 0
        )
        assignDepths(&root, depth: 0)

        // Step 6: Compute layout
        var ySlot = 0
        computeLayout(node: &root, ySlot: &ySlot)

        rootNode = root
        isLoading = false
    }

    // MARK: - Tree Building

    private func buildNode(
        conversation: ConversationFullInfo,
        delegationMessage: Message?,
        returnMessage: Message?,
        parentConversation: ConversationFullInfo?,
        conversationById: [String: ConversationFullInfo],
        messagesByConversation: [String: [Message]],
        depth: Int
    ) -> DelegationTreeNode {
        // Find children: conversations whose parentId == this conversation's id
        let childConversations = conversationById.values.filter { $0.thread.parentConversationId == conversation.thread.id }

        var children: [DelegationTreeNode] = []
        for childConv in childConversations.sorted(by: { $0.thread.lastActivity < $1.thread.lastActivity }) {
            let childMessages = messagesByConversation[childConv.thread.id] ?? []

            // Outgoing arrow: OP of the child conversation (first message = delegation brief)
            let delegMsg = childMessages.first

            // Return arrow: last message from the child's agent (completion)
            // The child conversation's thread pubkey identifies the delegated agent.
            let childReturnMsg = childMessages.last { msg in
                msg.pubkey == childConv.thread.pubkey && msg.toolName == nil
            }

            let childNode = buildNode(
                conversation: childConv,
                delegationMessage: delegMsg,
                returnMessage: childReturnMsg,
                parentConversation: conversation,
                conversationById: conversationById,
                messagesByConversation: messagesByConversation,
                depth: depth + 1
            )
            children.append(childNode)
        }

        return DelegationTreeNode(
            conversation: conversation,
            delegationMessage: delegationMessage,
            returnMessage: returnMessage,
            children: children,
            depth: depth
        )
    }

    private func assignDepths(_ node: inout DelegationTreeNode, depth: Int) {
        node.depth = depth
        for i in node.children.indices {
            assignDepths(&node.children[i], depth: depth + 1)
        }
    }

    // MARK: - Reingold-Tilford Simplified Layout

    /// Post-order: assign each leaf a unique y-slot index; internal nodes get midpoint of first/last child.
    /// Pre-order: assign x from depth.
    private func computeLayout(node: inout DelegationTreeNode, ySlot: inout Int) {
        var positions: [String: CGPoint] = [:]
        var ySlotCounter = 0
        assignPositions(node: &node, positions: &positions, ySlot: &ySlotCounter)

        nodePositions = positions

        // Compute canvas size from max x/y positions
        let maxX = positions.values.map { $0.x }.max() ?? 0
        let maxY = positions.values.map { $0.y }.max() ?? 0
        canvasSize = CGSize(
            width: maxX + nodeW + padding,
            height: maxY + nodeH + padding
        )
    }

    private func assignPositions(
        node: inout DelegationTreeNode,
        positions: inout [String: CGPoint],
        ySlot: inout Int
    ) {
        if node.children.isEmpty {
            // Leaf node: assign a y-slot
            let y = padding + CGFloat(ySlot) * (nodeH + vGap)
            let x = padding + CGFloat(node.depth) * (nodeW + hGap)
            positions[node.id] = CGPoint(x: x, y: y)
            ySlot += 1
        } else {
            // Internal node: first lay out children
            for i in node.children.indices {
                assignPositions(node: &node.children[i], positions: &positions, ySlot: &ySlot)
            }
            // Parent y = midpoint of first and last child y
            let firstChildPos = positions[node.children.first!.id]!
            let lastChildPos = positions[node.children.last!.id]!
            let y = (firstChildPos.y + lastChildPos.y) / 2
            let x = padding + CGFloat(node.depth) * (nodeW + hGap)
            positions[node.id] = CGPoint(x: x, y: y)
        }
    }

    // MARK: - Edge Enumeration

    struct Edge {
        let parentId: String
        let childId: String
        let delegationMessage: Message?
        let returnMessage: Message?
        let childStatus: String?
        let childIsActive: Bool
    }

    var edges: [Edge] {
        guard let root = rootNode else { return [] }
        var result: [Edge] = []
        collectEdges(node: root, into: &result)
        return result
    }

    private func collectEdges(node: DelegationTreeNode, into result: inout [Edge]) {
        for child in node.children {
            result.append(Edge(
                parentId: node.id,
                childId: child.id,
                delegationMessage: child.delegationMessage,
                returnMessage: child.returnMessage,
                childStatus: child.conversation.thread.statusLabel,
                childIsActive: child.conversation.isActive
            ))
            collectEdges(node: child, into: &result)
        }
    }

    // MARK: - Summary

    var totalNodeCount: Int {
        guard let root = rootNode else { return 0 }
        return countNodes(root)
    }

    private func countNodes(_ node: DelegationTreeNode) -> Int {
        1 + node.children.reduce(0) { $0 + countNodes($1) }
    }
}
