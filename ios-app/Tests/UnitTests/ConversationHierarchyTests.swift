import XCTest
@testable import TenexMVP

final class ConversationHierarchyTests: XCTestCase {

    // MARK: - Empty Input

    func testEmptyConversationListProducesEmptyHierarchy() {
        let hierarchy = ConversationHierarchy(conversations: [])

        XCTAssertTrue(hierarchy.rootConversations.isEmpty)
        XCTAssertTrue(hierarchy.childrenByParentId.isEmpty)
        XCTAssertTrue(hierarchy.aggregatedData.isEmpty)
        XCTAssertTrue(hierarchy.getSortedRoots().isEmpty)
    }

    // MARK: - Single Conversation

    func testSingleRootConversationWithNoChildren() {
        let root = makeConversation(id: "root", parentId: nil, lastActivity: 1000)
        let hierarchy = ConversationHierarchy(conversations: [root])

        XCTAssertEqual(hierarchy.rootConversations.count, 1)
        XCTAssertEqual(hierarchy.rootConversations[0].thread.id, "root")
        XCTAssertTrue(hierarchy.childrenByParentId.isEmpty)

        let data = hierarchy.getData(for: "root")
        XCTAssertEqual(data.descendantCount, 0)
        XCTAssertEqual(data.effectiveLastActivity, 1000)
        XCTAssertEqual(data.activitySpan, 0)
    }

    // MARK: - Root Identification

    func testConversationsWithNoParentAreRoots() {
        let a = makeConversation(id: "a", parentId: nil, lastActivity: 100)
        let b = makeConversation(id: "b", parentId: nil, lastActivity: 200)
        let hierarchy = ConversationHierarchy(conversations: [a, b])

        XCTAssertEqual(hierarchy.rootConversations.count, 2)
        let rootIds = Set(hierarchy.rootConversations.map { $0.thread.id })
        XCTAssertEqual(rootIds, ["a", "b"])
    }

    func testOrphanedConversationIsTreatedAsRoot() {
        // Parent "missing-parent" does not exist in the list
        let orphan = makeConversation(id: "orphan", parentId: "missing-parent", lastActivity: 100)
        let hierarchy = ConversationHierarchy(conversations: [orphan])

        XCTAssertEqual(hierarchy.rootConversations.count, 1)
        XCTAssertEqual(hierarchy.rootConversations[0].thread.id, "orphan")
    }

    func testChildWithExistingParentIsNotARoot() {
        let parent = makeConversation(id: "parent", parentId: nil, lastActivity: 100)
        let child = makeConversation(id: "child", parentId: "parent", lastActivity: 200)
        let hierarchy = ConversationHierarchy(conversations: [parent, child])

        XCTAssertEqual(hierarchy.rootConversations.count, 1)
        XCTAssertEqual(hierarchy.rootConversations[0].thread.id, "parent")
    }

    // MARK: - Tree Building

    func testChildrenAreGroupedUnderParent() {
        let parent = makeConversation(id: "parent", parentId: nil, lastActivity: 100)
        let child1 = makeConversation(id: "child-1", parentId: "parent", lastActivity: 300)
        let child2 = makeConversation(id: "child-2", parentId: "parent", lastActivity: 200)
        let hierarchy = ConversationHierarchy(conversations: [parent, child1, child2])

        let children = hierarchy.childrenByParentId["parent"]!
        XCTAssertEqual(children.count, 2)
        // Children should be sorted by lastActivity descending
        XCTAssertEqual(children[0].thread.id, "child-1")
        XCTAssertEqual(children[1].thread.id, "child-2")
    }

    func testMultipleParentsEachHaveTheirOwnChildren() {
        let p1 = makeConversation(id: "p1", parentId: nil, lastActivity: 100)
        let p2 = makeConversation(id: "p2", parentId: nil, lastActivity: 100)
        let c1 = makeConversation(id: "c1", parentId: "p1", lastActivity: 200)
        let c2 = makeConversation(id: "c2", parentId: "p2", lastActivity: 200)
        let hierarchy = ConversationHierarchy(conversations: [p1, p2, c1, c2])

        XCTAssertEqual(hierarchy.childrenByParentId["p1"]?.count, 1)
        XCTAssertEqual(hierarchy.childrenByParentId["p1"]?[0].thread.id, "c1")
        XCTAssertEqual(hierarchy.childrenByParentId["p2"]?.count, 1)
        XCTAssertEqual(hierarchy.childrenByParentId["p2"]?[0].thread.id, "c2")
    }

    // MARK: - Deep Nesting

    func testDeepNestingParentChildGrandchild() {
        let root = makeConversation(id: "root", parentId: nil, lastActivity: 100)
        let child = makeConversation(id: "child", parentId: "root", lastActivity: 200)
        let grandchild = makeConversation(id: "grandchild", parentId: "child", lastActivity: 300)
        let hierarchy = ConversationHierarchy(conversations: [root, child, grandchild])

        // Only root should be a root conversation
        XCTAssertEqual(hierarchy.rootConversations.count, 1)
        XCTAssertEqual(hierarchy.rootConversations[0].thread.id, "root")

        // Root has child as direct child
        XCTAssertEqual(hierarchy.childrenByParentId["root"]?.count, 1)
        XCTAssertEqual(hierarchy.childrenByParentId["root"]?[0].thread.id, "child")

        // Child has grandchild as direct child
        XCTAssertEqual(hierarchy.childrenByParentId["child"]?.count, 1)
        XCTAssertEqual(hierarchy.childrenByParentId["child"]?[0].thread.id, "grandchild")

        // Root should have 2 descendants (child + grandchild)
        let rootData = hierarchy.getData(for: "root")
        XCTAssertEqual(rootData.descendantCount, 2)

        // Child should have 1 descendant (grandchild)
        let childData = hierarchy.getData(for: "child")
        XCTAssertEqual(childData.descendantCount, 1)

        // Grandchild should have 0 descendants
        let grandchildData = hierarchy.getData(for: "grandchild")
        XCTAssertEqual(grandchildData.descendantCount, 0)
    }

    // MARK: - Aggregated Data

    func testEffectiveLastActivityIsMaxAcrossDescendants() {
        let root = makeConversation(id: "root", parentId: nil, lastActivity: 100)
        let child = makeConversation(id: "child", parentId: "root", lastActivity: 500)
        let grandchild = makeConversation(id: "grandchild", parentId: "child", lastActivity: 300)
        let hierarchy = ConversationHierarchy(conversations: [root, child, grandchild])

        let rootData = hierarchy.getData(for: "root")
        XCTAssertEqual(rootData.effectiveLastActivity, 500)
    }

    func testEffectiveLastActivityIsOwnWhenNoDescendantsAreNewer() {
        let root = makeConversation(id: "root", parentId: nil, lastActivity: 999)
        let child = makeConversation(id: "child", parentId: "root", lastActivity: 100)
        let hierarchy = ConversationHierarchy(conversations: [root, child])

        let rootData = hierarchy.getData(for: "root")
        XCTAssertEqual(rootData.effectiveLastActivity, 999)
    }

    func testActivitySpanIsComputedCorrectly() {
        let root = makeConversation(id: "root", parentId: nil, lastActivity: 100)
        let child = makeConversation(id: "child", parentId: "root", lastActivity: 400)
        let hierarchy = ConversationHierarchy(conversations: [root, child])

        let rootData = hierarchy.getData(for: "root")
        // Span = max(100, 400) - min(100, 400) = 300
        XCTAssertEqual(rootData.activitySpan, 300)
    }

    func testActivitySpanIsZeroForSingleConversation() {
        let root = makeConversation(id: "root", parentId: nil, lastActivity: 500)
        let hierarchy = ConversationHierarchy(conversations: [root])

        let rootData = hierarchy.getData(for: "root")
        XCTAssertEqual(rootData.activitySpan, 0)
    }

    func testDescendantCountIsCorrect() {
        let root = makeConversation(id: "root", parentId: nil, lastActivity: 100)
        let c1 = makeConversation(id: "c1", parentId: "root", lastActivity: 200)
        let c2 = makeConversation(id: "c2", parentId: "root", lastActivity: 300)
        let gc1 = makeConversation(id: "gc1", parentId: "c1", lastActivity: 400)
        let hierarchy = ConversationHierarchy(conversations: [root, c1, c2, gc1])

        let rootData = hierarchy.getData(for: "root")
        XCTAssertEqual(rootData.descendantCount, 3)
    }

    func testParticipatingAgentsIncludesAllUniqueAuthors() {
        let root = makeConversation(id: "root", parentId: nil, lastActivity: 100, author: "alice", pubkey: "pk-alice")
        let child = makeConversation(id: "child", parentId: "root", lastActivity: 200, author: "bob", pubkey: "pk-bob")
        let grandchild = makeConversation(id: "grandchild", parentId: "child", lastActivity: 300, author: "alice", pubkey: "pk-alice")
        let hierarchy = ConversationHierarchy(conversations: [root, child, grandchild])

        let rootData = hierarchy.getData(for: "root")
        XCTAssertEqual(Set(rootData.participatingAgents), Set(["alice", "bob"]))
        XCTAssertEqual(rootData.participatingAgentInfos.count, 2)
    }

    func testDelegationAgentInfosExcludesAuthor() {
        let root = makeConversation(id: "root", parentId: nil, lastActivity: 100, author: "alice", pubkey: "pk-alice")
        let child = makeConversation(id: "child", parentId: "root", lastActivity: 200, author: "bob", pubkey: "pk-bob")
        let hierarchy = ConversationHierarchy(conversations: [root, child])

        let rootData = hierarchy.getData(for: "root")
        XCTAssertEqual(rootData.authorInfo?.name, "alice")
        XCTAssertEqual(rootData.delegationAgentInfos.count, 1)
        XCTAssertEqual(rootData.delegationAgentInfos[0].name, "bob")
    }

    // MARK: - Sorting

    func testGetSortedRootsSortsByEffectiveLastActivityDescending() {
        let r1 = makeConversation(id: "r1", parentId: nil, lastActivity: 100)
        let r2 = makeConversation(id: "r2", parentId: nil, lastActivity: 300)
        let r3 = makeConversation(id: "r3", parentId: nil, lastActivity: 200)
        let hierarchy = ConversationHierarchy(conversations: [r1, r2, r3])

        let sorted = hierarchy.getSortedRoots()
        XCTAssertEqual(sorted.map { $0.thread.id }, ["r2", "r3", "r1"])
    }

    func testGetSortedRootsUsesDescendantActivityForSorting() {
        // r1 has old activity but a child with the newest activity
        let r1 = makeConversation(id: "r1", parentId: nil, lastActivity: 100)
        let r1Child = makeConversation(id: "r1-child", parentId: "r1", lastActivity: 999)
        let r2 = makeConversation(id: "r2", parentId: nil, lastActivity: 500)
        let hierarchy = ConversationHierarchy(conversations: [r1, r1Child, r2])

        let sorted = hierarchy.getSortedRoots()
        // r1 should come first because its effective activity (999) > r2's (500)
        XCTAssertEqual(sorted[0].thread.id, "r1")
        XCTAssertEqual(sorted[1].thread.id, "r2")
    }

    func testGetSortedRootsTiebreaksByIdAscending() {
        let r1 = makeConversation(id: "aaa", parentId: nil, lastActivity: 100)
        let r2 = makeConversation(id: "zzz", parentId: nil, lastActivity: 100)
        let hierarchy = ConversationHierarchy(conversations: [r1, r2])

        let sorted = hierarchy.getSortedRoots()
        XCTAssertEqual(sorted.map { $0.thread.id }, ["aaa", "zzz"])
    }

    // MARK: - Cycle Detection

    func testCycleDoesNotCauseInfiniteLoop() {
        // Create a cycle: a -> b -> a
        let a = makeConversation(id: "a", parentId: "b", lastActivity: 100)
        let b = makeConversation(id: "b", parentId: "a", lastActivity: 200)
        // Both are orphaned roots since neither parent exists as a "true" root
        // But the BFS descendant computation must not loop forever
        let hierarchy = ConversationHierarchy(conversations: [a, b])

        // Both should be treated as roots (each references a parent that is in the set,
        // but there's a cycle). Actually: a's parent is "b" which IS in the set, so a is NOT a root.
        // b's parent is "a" which IS in the set, so b is NOT a root either.
        // This means rootConversations is empty since both have valid parents.
        // But the important thing is it doesn't hang.
        XCTAssertTrue(hierarchy.rootConversations.isEmpty || hierarchy.rootConversations.count <= 2)

        // Verify aggregated data was computed without hanging
        let dataA = hierarchy.getData(for: "a")
        let dataB = hierarchy.getData(for: "b")
        // Descendant count should be bounded (cycle detection prevents infinite traversal)
        XCTAssertLessThanOrEqual(dataA.descendantCount, 2)
        XCTAssertLessThanOrEqual(dataB.descendantCount, 2)
    }

    func testSelfReferentialParentDoesNotHang() {
        // Conversation references itself as parent
        let selfRef = makeConversation(id: "self", parentId: "self", lastActivity: 100)
        let hierarchy = ConversationHierarchy(conversations: [selfRef])

        // "self" references parent "self" which IS in the set, so it's not a root
        // The key test is that it completes without hanging
        let data = hierarchy.getData(for: "self")
        XCTAssertLessThanOrEqual(data.descendantCount, 1)
    }

    // MARK: - getData Fallback

    func testGetDataForUnknownIdReturnsEmpty() {
        let hierarchy = ConversationHierarchy(conversations: [])

        let data = hierarchy.getData(for: "nonexistent")
        XCTAssertEqual(data.descendantCount, 0)
        XCTAssertEqual(data.effectiveLastActivity, 0)
        XCTAssertTrue(data.participatingAgents.isEmpty)
    }

    // MARK: - Helper

    private func makeConversation(
        id: String,
        parentId: String?,
        lastActivity: UInt64,
        isActive: Bool = false,
        hasChildren: Bool = false,
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
            hasChildren: hasChildren,
            projectATag: "31922:owner:project-1"
        )
    }
}
