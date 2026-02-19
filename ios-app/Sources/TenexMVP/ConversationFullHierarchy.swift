import Foundation

// MARK: - Conversation Full Hierarchy

/// Precomputed hierarchy data for ConversationFullInfo with activity tracking.
/// Computes parent→children map and hierarchical activity status once per refresh,
/// avoiding O(n²) traversals during sorting and rendering.
final class ConversationFullHierarchy {
    /// Map from conversation ID to its direct children
    let childrenByParentId: [String: [ConversationFullInfo]]

    /// Map from conversation ID to conversation for O(1) lookups
    let conversationById: [String: ConversationFullInfo]

    /// Precomputed hierarchical activity status: true if conversation or any descendant is active
    let hierarchicallyActiveById: [String: Bool]

    /// Root conversations (no parent or parent doesn't exist in the set)
    let rootConversations: [ConversationFullInfo]

    /// Root conversations sorted by: hierarchically active first, then by effective last activity
    let sortedRootConversations: [ConversationFullInfo]

    /// Initialize hierarchy from a flat list of conversations
    /// - Parameter conversations: All conversations to process
    init(conversations: [ConversationFullInfo]) {
        // Step 1: Build O(1) lookup maps
        let byId = Dictionary(uniqueKeysWithValues: conversations.map { ($0.id, $0) })
        self.conversationById = byId

        // Step 2: Build parent→children map (O(n))
        var childrenMap: [String: [ConversationFullInfo]] = [:]
        for conversation in conversations {
            if let parentId = conversation.parentId {
                childrenMap[parentId, default: []].append(conversation)
            }
        }
        self.childrenByParentId = childrenMap

        // Step 3: Identify root conversations (no parent OR orphaned)
        let allIds = Set(conversations.map { $0.id })
        let roots = conversations.filter { conv in
            if let parentId = conv.parentId {
                return !allIds.contains(parentId) // Orphaned: parent doesn't exist
            }
            return true // No parent - true root
        }
        self.rootConversations = roots

        // Step 4: Compute hierarchical activity status using bottom-up BFS
        // We process in reverse topological order (leaves first)
        var activityMap: [String: Bool] = [:]
        Self.computeHierarchicalActivity(
            conversations: conversations,
            childrenMap: childrenMap,
            activityMap: &activityMap
        )
        self.hierarchicallyActiveById = activityMap

        // Step 5: Sort roots by hierarchical activity first, then by effective last activity
        self.sortedRootConversations = roots.sorted { a, b in
            let aActive = activityMap[a.id] ?? a.isActive
            let bActive = activityMap[b.id] ?? b.isActive

            // Active conversations come first
            if aActive && !bActive { return true }
            if !aActive && bActive { return false }

            // Within same activity status, sort by effective last activity (newest first)
            return a.effectiveLastActivity > b.effectiveLastActivity
        }
    }

    /// Compute hierarchical activity for all conversations in O(n) time.
    /// Uses DFS with memoization - each conversation is processed exactly once.
    private static func computeHierarchicalActivity(
        conversations: [ConversationFullInfo],
        childrenMap: [String: [ConversationFullInfo]],
        activityMap: inout [String: Bool]
    ) {
        let conversationsById = Dictionary(uniqueKeysWithValues: conversations.map { ($0.id, $0) })
        var visited = Set<String>()

        // Process all conversations using DFS with memoization
        for conversation in conversations {
            if activityMap[conversation.id] == nil {
                _ = computeActivityRecursive(
                    conversationId: conversation.id,
                    conversations: conversationsById,
                    childrenMap: childrenMap,
                    activityMap: &activityMap,
                    visited: &visited
                )
            }
        }
    }

    /// Recursively compute activity with memoization.
    /// Uses inout visited set to prevent cycles without copying.
    private static func computeActivityRecursive(
        conversationId: String,
        conversations: [String: ConversationFullInfo],
        childrenMap: [String: [ConversationFullInfo]],
        activityMap: inout [String: Bool],
        visited: inout Set<String>
    ) -> Bool {
        // Return cached result if available
        if let cached = activityMap[conversationId] {
            return cached
        }

        // Cycle detection
        if visited.contains(conversationId) {
            return false
        }
        visited.insert(conversationId)

        // Get the conversation
        guard let conversation = conversations[conversationId] else {
            activityMap[conversationId] = false
            visited.remove(conversationId)
            return false
        }

        // Check if directly active
        if conversation.isActive {
            activityMap[conversationId] = true
            visited.remove(conversationId)
            return true
        }

        // Check children recursively
        let children = childrenMap[conversationId] ?? []
        for child in children {
            if computeActivityRecursive(
                conversationId: child.id,
                conversations: conversations,
                childrenMap: childrenMap,
                activityMap: &activityMap,
                visited: &visited
            ) {
                activityMap[conversationId] = true
                visited.remove(conversationId)
                return true
            }
        }

        // Not active
        activityMap[conversationId] = false
        visited.remove(conversationId)
        return false
    }

    /// Check if a conversation is hierarchically active (O(1) lookup)
    func isHierarchicallyActive(_ conversationId: String) -> Bool {
        hierarchicallyActiveById[conversationId] ?? false
    }
}

/// Main tab view for Conversations - uses sidebar-first controls on iPad/macOS
/// and compact toolbar controls on iPhone.
enum ConversationsLayoutMode {
    case adaptive
    case shellList
    case shellDetail
    case shellComposite
}
