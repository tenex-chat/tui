import XCTest
@testable import TenexMVP

final class ConversationActivityMetricsTests: XCTestCase {
    func testActiveConversationCountCountsOnlyDirectActiveConversations() {
        let conversations = [
            makeConversation(id: "conv-1", parentId: nil, isActive: true),
            makeConversation(id: "conv-2", parentId: nil, isActive: false),
            makeConversation(id: "conv-3", parentId: "conv-1", isActive: true)
        ]

        let activeCount = ConversationActivityMetrics.activeConversationCount(conversations: conversations)

        XCTAssertEqual(activeCount, 2)
    }

    func testActiveConversationCountIsZeroWhenNoConversationsAreActive() {
        let conversations = [
            makeConversation(id: "conv-1", parentId: nil, isActive: false),
            makeConversation(id: "conv-2", parentId: "conv-1", isActive: false)
        ]

        let activeCount = ConversationActivityMetrics.activeConversationCount(conversations: conversations)

        XCTAssertEqual(activeCount, 0)
    }

    func testDelegationActivityMarksDirectChildActiveWhenDescendantActive() {
        let childA = makeConversation(id: "child-a", parentId: "root", isActive: false)
        let childB = makeConversation(id: "child-b", parentId: "root", isActive: false)
        let grandchildOfA = makeConversation(id: "grandchild-a", parentId: "child-a", isActive: true)

        let activityMap = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [childA, childB],
            allDescendants: [childA, childB, grandchildOfA]
        )

        XCTAssertEqual(activityMap["child-a"], true)
        XCTAssertEqual(activityMap["child-b"], false)
    }

    func testDelegationActivityIsFalseWhenEntireSubtreeIsInactive() {
        let child = makeConversation(id: "child", parentId: "root", isActive: false)
        let grandchild = makeConversation(id: "grandchild", parentId: "child", isActive: false)

        let activityMap = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [child],
            allDescendants: [child, grandchild]
        )

        XCTAssertEqual(activityMap["child"], false)
    }

    private func makeConversation(
        id: String,
        parentId: String?,
        isActive: Bool
    ) -> ConversationFullInfo {
        ConversationFullInfo(
            id: id,
            title: id,
            author: "author",
            authorPubkey: "author-pubkey",
            summary: nil,
            messageCount: 1,
            lastActivity: 100,
            effectiveLastActivity: 100,
            parentId: parentId,
            status: nil,
            currentActivity: nil,
            isActive: isActive,
            isArchived: false,
            hasChildren: false,
            projectATag: "31922:owner:project-1",
            isScheduled: false,
            pTags: []
        )
    }
}
