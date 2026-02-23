import XCTest
@testable import TenexMVP

final class ConversationActivityMetricsTests: XCTestCase {

    // MARK: - activeConversationCount: Empty input

    func testActiveConversationCountWithEmptyList() {
        let count = ConversationActivityMetrics.activeConversationCount(conversations: [])
        XCTAssertEqual(count, 0)
    }

    // MARK: - activeConversationCount: All active

    func testActiveConversationCountAllActive() {
        let conversations = [
            makeConversation(id: "a", isActive: true),
            makeConversation(id: "b", isActive: true),
            makeConversation(id: "c", isActive: true),
        ]
        let count = ConversationActivityMetrics.activeConversationCount(conversations: conversations)
        XCTAssertEqual(count, 3)
    }

    // MARK: - activeConversationCount: None active

    func testActiveConversationCountNoneActive() {
        let conversations = [
            makeConversation(id: "a", isActive: false),
            makeConversation(id: "b", isActive: false),
        ]
        let count = ConversationActivityMetrics.activeConversationCount(conversations: conversations)
        XCTAssertEqual(count, 0)
    }

    // MARK: - activeConversationCount: Mixed

    func testActiveConversationCountMixed() {
        let conversations = [
            makeConversation(id: "a", isActive: true),
            makeConversation(id: "b", isActive: false),
            makeConversation(id: "c", isActive: true),
            makeConversation(id: "d", isActive: false),
            makeConversation(id: "e", isActive: true),
        ]
        let count = ConversationActivityMetrics.activeConversationCount(conversations: conversations)
        XCTAssertEqual(count, 3)
    }

    func testActiveConversationCountSingleActive() {
        let count = ConversationActivityMetrics.activeConversationCount(
            conversations: [makeConversation(id: "only", isActive: true)]
        )
        XCTAssertEqual(count, 1)
    }

    func testActiveConversationCountSingleInactive() {
        let count = ConversationActivityMetrics.activeConversationCount(
            conversations: [makeConversation(id: "only", isActive: false)]
        )
        XCTAssertEqual(count, 0)
    }

    // MARK: - activeConversationCount: Ignores hierarchy

    func testActiveConversationCountCountsOnlyDirectIsActiveFlag() {
        // Parent-child relationships don't matter; only the isActive flag does
        let conversations = [
            makeConversation(id: "parent", parentId: nil, isActive: true),
            makeConversation(id: "inactive-child", parentId: "parent", isActive: false),
            makeConversation(id: "active-child", parentId: "parent", isActive: true),
        ]
        let count = ConversationActivityMetrics.activeConversationCount(conversations: conversations)
        XCTAssertEqual(count, 2)
    }

    // MARK: - delegationActivityByConversationId: Empty inputs

    func testDelegationActivityEmptyDirectChildren() {
        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [],
            allDescendants: []
        )
        XCTAssertTrue(result.isEmpty)
    }

    func testDelegationActivityEmptyDirectChildrenWithDescendants() {
        let descendants = [makeConversation(id: "d1", isActive: true)]
        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [],
            allDescendants: descendants
        )
        XCTAssertTrue(result.isEmpty)
    }

    // MARK: - delegationActivityByConversationId: Direct activity (no descendants)

    func testDelegationActivityDirectChildActive() {
        let child = makeConversation(id: "child-1", isActive: true)
        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [child],
            allDescendants: []
        )
        XCTAssertEqual(result.count, 1)
        XCTAssertEqual(result["child-1"], true)
    }

    func testDelegationActivityDirectChildInactive() {
        let child = makeConversation(id: "child-1", isActive: false)
        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [child],
            allDescendants: []
        )
        XCTAssertEqual(result.count, 1)
        XCTAssertEqual(result["child-1"], false)
    }

    func testDelegationActivityMultipleDirectChildrenMixed() {
        let active = makeConversation(id: "active-child", isActive: true)
        let inactive = makeConversation(id: "inactive-child", isActive: false)
        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [active, inactive],
            allDescendants: []
        )
        XCTAssertEqual(result.count, 2)
        XCTAssertEqual(result["active-child"], true)
        XCTAssertEqual(result["inactive-child"], false)
    }

    // MARK: - delegationActivityByConversationId: Hierarchical activity propagation

    func testDelegationActivityChildInactiveButGrandchildActive() {
        let child = makeConversation(id: "child", parentId: nil, isActive: false)
        let grandchild = makeConversation(id: "grandchild", parentId: "child", isActive: true)

        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [child],
            allDescendants: [grandchild]
        )
        XCTAssertEqual(result.count, 1)
        XCTAssertEqual(result["child"], true, "Child should be hierarchically active via active grandchild")
    }

    func testDelegationActivityDeepHierarchyLeafActive() {
        // rootChild -> mid -> leaf (active)
        let rootChild = makeConversation(id: "root-child", parentId: nil, isActive: false)
        let mid = makeConversation(id: "mid", parentId: "root-child", isActive: false)
        let leaf = makeConversation(id: "leaf", parentId: "mid", isActive: true)

        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [rootChild],
            allDescendants: [mid, leaf]
        )
        XCTAssertEqual(result["root-child"], true, "Root child should be active due to active leaf descendant")
    }

    func testDelegationActivityDeepHierarchyAllInactive() {
        let rootChild = makeConversation(id: "root-child", parentId: nil, isActive: false)
        let mid = makeConversation(id: "mid", parentId: "root-child", isActive: false)
        let leaf = makeConversation(id: "leaf", parentId: "mid", isActive: false)

        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [rootChild],
            allDescendants: [mid, leaf]
        )
        XCTAssertEqual(result["root-child"], false, "Root child should be inactive when entire subtree is inactive")
    }

    // MARK: - delegationActivityByConversationId: Multiple branches

    func testDelegationActivityMultipleBranchesOnlyOneActive() {
        let childA = makeConversation(id: "child-a", parentId: nil, isActive: false)
        let childB = makeConversation(id: "child-b", parentId: nil, isActive: false)
        let descendantA = makeConversation(id: "desc-a", parentId: "child-a", isActive: false)
        let descendantB = makeConversation(id: "desc-b", parentId: "child-b", isActive: true)

        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [childA, childB],
            allDescendants: [descendantA, descendantB]
        )
        XCTAssertEqual(result["child-a"], false)
        XCTAssertEqual(result["child-b"], true)
    }

    func testDelegationActivityMultipleBranchesBothActive() {
        let childA = makeConversation(id: "child-a", parentId: nil, isActive: true)
        let childB = makeConversation(id: "child-b", parentId: nil, isActive: false)
        let descendantB = makeConversation(id: "desc-b", parentId: "child-b", isActive: true)

        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [childA, childB],
            allDescendants: [descendantB]
        )
        XCTAssertEqual(result["child-a"], true)
        XCTAssertEqual(result["child-b"], true)
    }

    // MARK: - delegationActivityByConversationId: Deduplication

    func testDelegationActivityDescendantsOverlapWithDirectChildren() {
        // When directChildren also appear in allDescendants, deduplication via
        // dictionary keying should produce correct results
        let child = makeConversation(id: "child", parentId: nil, isActive: true)
        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [child],
            allDescendants: [child]
        )
        XCTAssertEqual(result["child"], true)
    }

    // MARK: - delegationActivityByConversationId: Result keys

    func testDelegationActivityOnlyReturnsKeysForDirectChildren() {
        let child = makeConversation(id: "child", parentId: nil, isActive: false)
        let descendant = makeConversation(id: "descendant", parentId: "child", isActive: true)

        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [child],
            allDescendants: [descendant]
        )
        XCTAssertEqual(result.count, 1)
        XCTAssertNotNil(result["child"])
        XCTAssertNil(result["descendant"], "Descendants should not appear as keys in the result")
    }

    // MARK: - delegationActivityByConversationId: Large flat list

    func testDelegationActivityLargeFlatList() {
        let children = (0..<20).map { i in
            makeConversation(id: "child-\(i)", isActive: i % 3 == 0)
        }
        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: children,
            allDescendants: []
        )
        XCTAssertEqual(result.count, 20)
        for i in 0..<20 {
            XCTAssertEqual(result["child-\(i)"], i % 3 == 0)
        }
    }

    // MARK: - delegationActivityByConversationId: Complex nested hierarchy

    func testDelegationActivityComplexHierarchyWithMultipleDepths() {
        // child-a -> desc-a1 -> desc-a1a (active)
        // child-a -> desc-a2 (inactive)
        // child-b -> desc-b1 (inactive)
        let childA = makeConversation(id: "child-a", parentId: nil, isActive: false)
        let childB = makeConversation(id: "child-b", parentId: nil, isActive: false)
        let descA1 = makeConversation(id: "desc-a1", parentId: "child-a", isActive: false)
        let descA2 = makeConversation(id: "desc-a2", parentId: "child-a", isActive: false)
        let descA1a = makeConversation(id: "desc-a1a", parentId: "desc-a1", isActive: true)
        let descB1 = makeConversation(id: "desc-b1", parentId: "child-b", isActive: false)

        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [childA, childB],
            allDescendants: [descA1, descA2, descA1a, descB1]
        )
        XCTAssertEqual(result["child-a"], true, "child-a has a deeply nested active descendant")
        XCTAssertEqual(result["child-b"], false, "child-b subtree is entirely inactive")
    }

    func testDelegationActivityDirectChildActiveOverridesInactiveDescendants() {
        // Child is active itself, even if its descendants are inactive
        let child = makeConversation(id: "child", parentId: nil, isActive: true)
        let descendant = makeConversation(id: "desc", parentId: "child", isActive: false)

        let result = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: [child],
            allDescendants: [descendant]
        )
        XCTAssertEqual(result["child"], true)
    }

    // MARK: - Helper

    private func makeConversation(
        id: String,
        parentId: String? = nil,
        lastActivity: UInt64 = 1000,
        isActive: Bool = false,
        author: String = "author",
        pubkey: String = "author-pubkey"
    ) -> ConversationFullInfo {
        let thread = Thread(
            id: id,
            title: id,
            content: "",
            pubkey: pubkey,
            lastActivity: lastActivity,
            effectiveLastActivity: lastActivity,
            statusLabel: nil,
            statusCurrentActivity: nil,
            summary: nil,
            hashtags: [],
            parentConversationId: parentId,
            pTags: [],
            askEvent: nil,
            isScheduled: false
        )
        return ConversationFullInfo(
            thread: thread,
            author: author,
            messageCount: 0,
            isActive: isActive,
            isArchived: false,
            hasChildren: false,
            projectATag: "31922:owner:project-1"
        )
    }
}
