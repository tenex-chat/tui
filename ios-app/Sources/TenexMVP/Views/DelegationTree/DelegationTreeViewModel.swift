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

        // Step 1: Get known descendants from runtime hierarchy
        let descendantIds = await safeCore.getDescendantConversationIds(conversationId: rootConversationId)

        // Step 1b: Build a lightweight parent->children index from all visible conversations.
        // This backfills delegations that may be missing from runtime descendant traversal.
        let allConversationChildrenByParent: [String: [String]]
        do {
            var allConversationsById: [String: ConversationFullInfo] = [:]

            let allConversations = try await safeCore.getAllConversations(
                filter: ConversationFilter(
                    projectIds: [],
                    showArchived: true,
                    hideScheduled: false,
                    timeFilter: .all
                )
            )
            for conversation in allConversations {
                allConversationsById[conversation.thread.id] = conversation
            }

            let projects = await safeCore.getProjects()
            let perProjectConversations = await withTaskGroup(
                of: [ConversationFullInfo].self,
                returning: [ConversationFullInfo].self
            ) { group in
                for project in projects {
                    group.addTask { [safeCore] in
                        await safeCore.getConversations(projectId: project.id)
                    }
                }

                var merged: [ConversationFullInfo] = []
                for await conversations in group {
                    merged.append(contentsOf: conversations)
                }
                return merged
            }

            for conversation in perProjectConversations {
                allConversationsById[conversation.thread.id] = conversation
            }

            var childrenByParent: [String: [String]] = [:]
            for conversation in allConversationsById.values {
                guard let parentId = conversation.thread.parentConversationId,
                      !parentId.isEmpty else { continue }
                childrenByParent[parentId, default: []].append(conversation.thread.id)
            }
            allConversationChildrenByParent = childrenByParent
        } catch {
            allConversationChildrenByParent = [:]
        }

        // Step 2: Expand graph by following q-tags so missing leaves still render
        // even when runtime hierarchy does not yet include them.
        var requestedConversationIds = Set([rootConversationId])
        requestedConversationIds.formUnion(descendantIds)
        var attemptedConversationIds = Set<String>()
        var expandedParentConversationIds = Set<String>()
        var conversationById: [String: ConversationFullInfo] = [:]
        var messagesByConversation: [String: [Message]] = [:]

        while true {
            let parentsToExpand = requestedConversationIds.subtracting(expandedParentConversationIds)
            for parentId in parentsToExpand {
                expandedParentConversationIds.insert(parentId)
                for childId in allConversationChildrenByParent[parentId] ?? [] where !childId.isEmpty {
                    requestedConversationIds.insert(childId)
                }
            }

            let idsToFetch = requestedConversationIds.subtracting(attemptedConversationIds)
            if idsToFetch.isEmpty {
                break
            }
            attemptedConversationIds.formUnion(idsToFetch)

            let fetchedConversations = await safeCore.getConversationsByIds(
                conversationIds: Array(idsToFetch).sorted()
            )
            let newConversations = fetchedConversations.filter { conversationById[$0.thread.id] == nil }
            if newConversations.isEmpty {
                continue
            }

            for conversation in newConversations {
                conversationById[conversation.thread.id] = conversation
            }

            await withTaskGroup(of: (String, [Message]).self) { group in
                for conversation in newConversations {
                    group.addTask { [safeCore] in
                        let messages = await safeCore.getMessages(conversationId: conversation.thread.id)
                        return (conversation.thread.id, messages)
                    }
                }
                for await (conversationId, messages) in group {
                    messagesByConversation[conversationId] = messages
                    for message in messages {
                        guard shouldTreatQTagsAsDelegationReference(message) else { continue }
                        for qTag in message.qTags where !qTag.isEmpty {
                            requestedConversationIds.insert(qTag)
                        }
                    }
                }
            }
        }

        guard let rootConversation = conversationById[rootConversationId] else {
            loadError = "Root conversation not found"
            isLoading = false
            return
        }

        // Step 3: Build parent->children relationships.
        // We combine explicit parent tags with q-tag links so delegations still render
        // when child threads are missing parentConversationId.
        let childrenByParentId = buildChildrenByParent(
            rootConversationId: rootConversationId,
            conversationById: conversationById,
            messagesByConversation: messagesByConversation
        )

        // Step 4: Build participant tree:
        // root author node -> root recipient node -> delegated conversation recipients.
        var visitedConversationIds = Set<String>()
        let rootRecipient = buildRecipientSubtree(
            conversation: rootConversation,
            childrenByParentId: childrenByParentId,
            conversationById: conversationById,
            messagesByConversation: messagesByConversation,
            depth: 1,
            visitedConversationIds: &visitedConversationIds
        )
        var root = DelegationTreeNode(
            id: rootAuthorNodeId(conversationId: rootConversation.thread.id),
            conversation: rootConversation,
            participantPubkey: rootConversation.thread.pubkey,
            role: .rootAuthor,
            returnMessage: nil,
            lastMessage: lastVisibleMessage(
                for: rootConversation,
                messagesByConversation: messagesByConversation
            ),
            children: [rootRecipient],
            depth: 0
        )
        assignDepths(&root, depth: 0)

        // Step 5: Compute layout
        computeLayout(node: &root)

        rootNode = root
        isLoading = false
    }

    // MARK: - Tree Building

    private func buildRecipientSubtree(
        conversation: ConversationFullInfo,
        childrenByParentId: [String: [String]],
        conversationById: [String: ConversationFullInfo],
        messagesByConversation: [String: [Message]],
        depth: Int,
        visitedConversationIds: inout Set<String>
    ) -> DelegationTreeNode {
        // Guard against cycles in malformed hierarchy data.
        guard visitedConversationIds.insert(conversation.thread.id).inserted else {
            return DelegationTreeNode(
                id: recipientNodeId(conversationId: conversation.thread.id),
                conversation: conversation,
                participantPubkey: recipientPubkey(for: conversation),
                role: .recipient,
                returnMessage: completionMessage(
                    for: conversation,
                    messagesByConversation: messagesByConversation
                ),
                lastMessage: lastVisibleMessage(
                    for: conversation,
                    messagesByConversation: messagesByConversation
                ),
                children: [],
                depth: depth
            )
        }

        let childIds = childrenByParentId[conversation.thread.id] ?? []
        let childConversations = childIds.compactMap { conversationById[$0] }

        var children: [DelegationTreeNode] = []
        for childConv in childConversations {
            let childNode = buildRecipientSubtree(
                conversation: childConv,
                childrenByParentId: childrenByParentId,
                conversationById: conversationById,
                messagesByConversation: messagesByConversation,
                depth: depth + 1,
                visitedConversationIds: &visitedConversationIds
            )
            children.append(childNode)
        }

        return DelegationTreeNode(
            id: recipientNodeId(conversationId: conversation.thread.id),
            conversation: conversation,
            participantPubkey: recipientPubkey(for: conversation),
            role: .recipient,
            returnMessage: completionMessage(
                for: conversation,
                messagesByConversation: messagesByConversation
            ),
            lastMessage: lastVisibleMessage(
                for: conversation,
                messagesByConversation: messagesByConversation
            ),
            children: children,
            depth: depth
        )
    }

    private func completionMessage(
        for conversation: ConversationFullInfo,
        messagesByConversation: [String: [Message]]
    ) -> Message? {
        let messages = messagesByConversation[conversation.thread.id] ?? []
        return messages.last { msg in
            msg.pubkey == conversation.thread.pubkey && msg.toolName == nil
        }
    }

    private func lastVisibleMessage(
        for conversation: ConversationFullInfo,
        messagesByConversation: [String: [Message]]
    ) -> Message? {
        let messages = messagesByConversation[conversation.thread.id] ?? []
        return messages.last { msg in
            msg.toolName == nil && !msg.isReasoning
        }
    }

    private func buildChildrenByParent(
        rootConversationId: String,
        conversationById: [String: ConversationFullInfo],
        messagesByConversation: [String: [Message]]
    ) -> [String: [String]] {
        let knownConversationIds = Set(conversationById.keys)
        var parentByChild: [String: (parentId: String, priority: Int)] = [:]

        func registerParent(childId: String, parentId: String, priority: Int) {
            guard childId != parentId else { return }
            guard knownConversationIds.contains(childId), knownConversationIds.contains(parentId) else { return }

            if let existing = parentByChild[childId] {
                // Keep stronger parent source. If same priority, keep deterministic ordering.
                if existing.priority > priority ||
                    (existing.priority == priority && existing.parentId <= parentId) {
                    return
                }
            }
            parentByChild[childId] = (parentId: parentId, priority: priority)
        }

        // Priority 1: explicit thread parent
        for conversation in conversationById.values {
            let childId = conversation.thread.id
            guard childId != rootConversationId else { continue }
            if let parentId = conversation.thread.parentConversationId {
                registerParent(childId: childId, parentId: parentId, priority: 1)
            }
        }

        // Priority 3 (strongest): inferred parent from child's delegation tags.
        // This mirrors runtime_hierarchy where message-level parent info can override thread-level tags.
        for conversation in conversationById.values {
            let childId = conversation.thread.id
            guard childId != rootConversationId else { continue }
            let messages = messagesByConversation[childId] ?? []
            if let inferredParentId = messages
                .compactMap(\.delegationTag)
                .first(where: { $0 != childId }) {
                registerParent(childId: childId, parentId: inferredParentId, priority: 3)
            }
        }

        // Priority 2: q-tag edges from parent messages to child conversation ids.
        for (parentId, messages) in messagesByConversation {
            guard knownConversationIds.contains(parentId) else { continue }
            for message in messages {
                guard shouldTreatQTagsAsDelegationReference(message) else { continue }
                for childId in message.qTags {
                    registerParent(childId: childId, parentId: parentId, priority: 2)
                }
            }
        }

        var childrenByParentId: [String: [String]] = [:]
        for (childId, relation) in parentByChild {
            childrenByParentId[relation.parentId, default: []].append(childId)
        }

        for parentId in Array(childrenByParentId.keys) {
            let deduped = Array(Set(childrenByParentId[parentId] ?? []))
            childrenByParentId[parentId] = deduped.sorted { lhs, rhs in
                let lhsActivity = conversationById[lhs]?.thread.lastActivity ?? 0
                let rhsActivity = conversationById[rhs]?.thread.lastActivity ?? 0
                if lhsActivity != rhsActivity {
                    return lhsActivity < rhsActivity
                }
                return lhs < rhs
            }
        }

        return childrenByParentId
    }

    private func assignDepths(_ node: inout DelegationTreeNode, depth: Int) {
        node.depth = depth
        for i in node.children.indices {
            assignDepths(&node.children[i], depth: depth + 1)
        }
    }

    // MARK: - Reingold-Tilford Simplified Layout

    /// Compact layered layout:
    /// 1) Seed each depth column in DFS order with fixed row spacing.
    /// 2) Iteratively pull parents toward children.
    /// 3) Re-compact each depth column to remove oversized vertical gaps.
    private func computeLayout(node: inout DelegationTreeNode) {
        var nodesByDepth: [Int: [DelegationTreeNode]] = [:]
        collectNodesByDepth(node: node, into: &nodesByDepth)

        var positions: [String: CGPoint] = [:]
        let depthLevels = nodesByDepth.keys.sorted()
        let rowStep = nodeH + max(16, vGap * 0.35)

        // Pass 1: seed compact columns by DFS order to avoid oversized gaps between sibling branches.
        for depth in depthLevels {
            guard let nodesAtDepth = nodesByDepth[depth] else { continue }
            let x = padding + CGFloat(depth) * (nodeW + hGap)
            for (index, depthNode) in nodesAtDepth.enumerated() {
                let y = padding + CGFloat(index) * rowStep
                positions[depthNode.id] = CGPoint(x: x, y: y)
            }
        }

        // Pass 2: iteratively pull parents toward children, then re-compact each depth column.
        for _ in 0..<4 {
            relaxParentAnchors(node: node, positions: &positions)
            for depth in depthLevels {
                guard let nodesAtDepth = nodesByDepth[depth] else { continue }
                compactDepthColumn(nodesAtDepth, positions: &positions, rowStep: rowStep)
            }
        }

        nodePositions = positions

        // Compute canvas size from max x/y positions
        let maxX = positions.values.map { $0.x }.max() ?? 0
        let maxY = positions.values.map { $0.y }.max() ?? 0
        canvasSize = CGSize(
            width: maxX + nodeW + padding,
            height: maxY + nodeH + padding
        )
    }

    private func collectNodesByDepth(
        node: DelegationTreeNode,
        into nodesByDepth: inout [Int: [DelegationTreeNode]]
    ) {
        nodesByDepth[node.depth, default: []].append(node)
        for child in node.children {
            collectNodesByDepth(node: child, into: &nodesByDepth)
        }
    }

    private func relaxParentAnchors(
        node: DelegationTreeNode,
        positions: inout [String: CGPoint]
    ) {
        for child in node.children {
            relaxParentAnchors(node: child, positions: &positions)
        }

        guard !node.children.isEmpty,
              let firstChildPos = positions[node.children.first!.id],
              let lastChildPos = positions[node.children.last!.id],
              var parentPos = positions[node.id] else {
            return
        }

        let targetY: CGFloat
        if node.depth == 0 {
            targetY = (firstChildPos.y + lastChildPos.y) / 2
        } else {
            let childSpan = lastChildPos.y - firstChildPos.y
            targetY = firstChildPos.y + childSpan * parentAnchorBias(forChildCount: node.children.count)
        }

        parentPos.y = targetY
        positions[node.id] = parentPos
    }

    private func compactDepthColumn(
        _ nodesAtDepth: [DelegationTreeNode],
        positions: inout [String: CGPoint],
        rowStep: CGFloat
    ) {
        guard !nodesAtDepth.isEmpty else { return }

        var startAccumulator: CGFloat = 0
        var sampleCount: CGFloat = 0

        for (index, node) in nodesAtDepth.enumerated() {
            guard let pos = positions[node.id] else { continue }
            startAccumulator += pos.y - CGFloat(index) * rowStep
            sampleCount += 1
        }

        guard sampleCount > 0 else { return }
        let baseline = max(padding, startAccumulator / sampleCount)

        for (index, node) in nodesAtDepth.enumerated() {
            guard var pos = positions[node.id] else { continue }
            pos.y = baseline + CGFloat(index) * rowStep
            positions[node.id] = pos
        }
    }

    private func parentAnchorBias(forChildCount childCount: Int) -> CGFloat {
        switch childCount {
        case 0, 1:
            return 0
        case 2:
            return 0.45
        case 3:
            return 0.4
        case 4...6:
            return 0.34
        default:
            return 0.28
        }
    }

    // MARK: - Edge Enumeration

    struct Edge {
        let parentId: String
        let childId: String
        let isComplete: Bool
        let crossProjectTargetLabel: String?
        let childStatus: String?
        let childIsActive: Bool
    }

    var edges: [Edge] {
        guard let root = rootNode else { return [] }
        var raw: [Edge] = []
        collectEdges(node: root, into: &raw)

        var byPair: [String: Edge] = [:]
        for edge in raw {
            let key = "\(edge.parentId)->\(edge.childId)"
            if let existing = byPair[key] {
                byPair[key] = mergeEdges(existing, edge)
            } else {
                byPair[key] = edge
            }
        }

        return byPair.values.sorted {
            if $0.parentId != $1.parentId {
                return $0.parentId < $1.parentId
            }
            return $0.childId < $1.childId
        }
    }

    private func collectEdges(node: DelegationTreeNode, into result: inout [Edge]) {
        for child in node.children {
            let isConversationStartEdge = node.role == .rootAuthor &&
                child.role == .recipient &&
                node.conversation.thread.id == child.conversation.thread.id
            let parentProjectTag = node.conversation.projectATag.trimmingCharacters(in: .whitespacesAndNewlines)
            let childProjectTag = child.conversation.projectATag.trimmingCharacters(in: .whitespacesAndNewlines)
            let isCrossProject = !isConversationStartEdge &&
                !childProjectTag.isEmpty &&
                parentProjectTag != childProjectTag

            result.append(Edge(
                parentId: node.id,
                childId: child.id,
                isComplete: isConversationStartEdge || child.returnMessage != nil,
                crossProjectTargetLabel: isCrossProject ? projectLabel(fromATag: childProjectTag) : nil,
                childStatus: child.conversation.thread.statusLabel,
                childIsActive: child.conversation.isActive
            ))
            collectEdges(node: child, into: &result)
        }
    }

    private func projectLabel(fromATag aTag: String) -> String {
        let parts = aTag.split(separator: ":")
        if let slug = parts.last, !slug.isEmpty {
            return String(slug)
        }
        return aTag
    }

    private func rootAuthorNodeId(conversationId: String) -> String {
        "author:\(conversationId)"
    }

    private func recipientNodeId(conversationId: String) -> String {
        "recipient:\(conversationId)"
    }

    private func recipientPubkey(for conversation: ConversationFullInfo) -> String {
        let candidate = conversation.thread.pTags
            .first?
            .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
        if !candidate.isEmpty {
            return candidate
        }
        return conversation.thread.pubkey
    }

    private func shouldTreatQTagsAsDelegationReference(_ message: Message) -> Bool {
        guard let toolName = message.toolName?.trimmingCharacters(in: .whitespacesAndNewlines),
              !toolName.isEmpty else {
            return false
        }
        let lowercased = toolName.lowercased()
        if lowercased == "delegate" || lowercased == "mcp__tenex__delegate" {
            return true
        }
        return lowercased.contains("__delegate") ||
            lowercased.hasPrefix("delegate_") ||
            lowercased.hasSuffix("_delegate")
    }

    private func mergeEdges(_ lhs: Edge, _ rhs: Edge) -> Edge {
        Edge(
            parentId: lhs.parentId,
            childId: lhs.childId,
            isComplete: lhs.isComplete || rhs.isComplete,
            crossProjectTargetLabel: lhs.crossProjectTargetLabel ?? rhs.crossProjectTargetLabel,
            childStatus: lhs.childStatus ?? rhs.childStatus,
            childIsActive: lhs.childIsActive || rhs.childIsActive
        )
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
