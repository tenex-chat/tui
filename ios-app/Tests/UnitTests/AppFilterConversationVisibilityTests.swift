import XCTest
@testable import TenexMVP

final class AppFilterConversationVisibilityTests: XCTestCase {
    func testScheduledHiddenRootHidesDelegatedConversation() {
        let root = makeConversation(
            id: "root",
            parentId: nil,
            isScheduled: true,
            isArchived: false
        )
        let child = makeConversation(
            id: "child",
            parentId: "root",
            isScheduled: false,
            isArchived: false
        )
        let snapshot = AppGlobalFilterSnapshot(
            projectIds: [],
            timeWindow: .all,
            scheduledEventFilter: .hide,
            statusFilter: .all,
            hashtagFilter: [],
            showArchived: true
        )

        let filtered = TenexCoreManager.filterConversationsByRootVisibility(
            [root, child],
            now: 1_000,
            snapshot: snapshot
        )

        XCTAssertTrue(filtered.isEmpty)
    }

    func testArchivedHiddenRootHidesDelegatedConversation() {
        let root = makeConversation(
            id: "root",
            parentId: nil,
            isScheduled: false,
            isArchived: true
        )
        let child = makeConversation(
            id: "child",
            parentId: "root",
            isScheduled: false,
            isArchived: false
        )
        let snapshot = AppGlobalFilterSnapshot(
            projectIds: [],
            timeWindow: .all,
            scheduledEventFilter: .showAll,
            statusFilter: .all,
            hashtagFilter: [],
            showArchived: false
        )

        let filtered = TenexCoreManager.filterConversationsByRootVisibility(
            [root, child],
            now: 1_000,
            snapshot: snapshot
        )

        XCTAssertTrue(filtered.isEmpty)
    }

    func testVisibleRootKeepsVisibleDelegatedConversation() {
        let root = makeConversation(
            id: "root",
            parentId: nil,
            isScheduled: false,
            isArchived: false
        )
        let child = makeConversation(
            id: "child",
            parentId: "root",
            isScheduled: false,
            isArchived: false
        )
        let snapshot = AppGlobalFilterSnapshot(
            projectIds: [],
            timeWindow: .all,
            scheduledEventFilter: .showAll,
            statusFilter: .all,
            hashtagFilter: [],
            showArchived: false
        )

        let filtered = TenexCoreManager.filterConversationsByRootVisibility(
            [root, child],
            now: 1_000,
            snapshot: snapshot
        )
        let filteredIds = Set(filtered.map(\.thread.id))

        XCTAssertEqual(filteredIds, Set(["root", "child"]))
    }

    private func makeConversation(
        id: String,
        parentId: String?,
        isScheduled: Bool,
        isArchived: Bool
    ) -> ConversationFullInfo {
        let thread = Thread(
            id: id,
            title: id,
            content: "",
            pubkey: "author-pubkey",
            lastActivity: 100,
            effectiveLastActivity: 100,
            statusLabel: nil,
            statusCurrentActivity: nil,
            summary: nil,
            hashtags: [],
            parentConversationId: parentId,
            pTags: [],
            askEvent: nil,
            isScheduled: isScheduled
        )

        return ConversationFullInfo(
            thread: thread,
            author: "author",
            messageCount: 1,
            isActive: false,
            isArchived: isArchived,
            hasChildren: parentId == nil,
            projectATag: "31922:owner:project-1"
        )
    }
}
