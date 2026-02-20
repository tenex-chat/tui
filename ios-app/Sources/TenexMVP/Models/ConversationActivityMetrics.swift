import Foundation

enum ConversationActivityMetrics {
    static func activeConversationCount(conversations: [ConversationFullInfo]) -> Int {
        conversations.reduce(0) { count, conversation in
            count + (conversation.isActive ? 1 : 0)
        }
    }

    static func delegationActivityByConversationId(
        directChildren: [ConversationFullInfo],
        allDescendants: [ConversationFullInfo]
    ) -> [String: Bool] {
        guard !directChildren.isEmpty else {
            return [:]
        }

        var conversationsById: [String: ConversationFullInfo] = [:]
        conversationsById.reserveCapacity(allDescendants.count + directChildren.count)

        for descendant in allDescendants {
            conversationsById[descendant.thread.id] = descendant
        }
        for child in directChildren {
            conversationsById[child.thread.id] = child
        }

        let hierarchy = ConversationFullHierarchy(conversations: Array(conversationsById.values))
        return Dictionary(uniqueKeysWithValues: directChildren.map { child in
            (child.thread.id, hierarchy.isHierarchicallyActive(child.thread.id))
        })
    }
}
