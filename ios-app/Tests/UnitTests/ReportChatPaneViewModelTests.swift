import XCTest
@testable import TenexMVP

@MainActor
final class ReportChatPaneViewModelTests: XCTestCase {
    func testLoadSortsThreadsByLastActivityDescending() async {
        let viewModel = ReportChatPaneViewModel()

        await viewModel.load(reportATag: "30023:pubkey:weekly") { _ in
            [
                self.makeThread(id: "older", title: "Older", lastActivity: 100),
                self.makeThread(id: "newer", title: "Newer", lastActivity: 400)
            ]
        }

        XCTAssertEqual(viewModel.threads.map(\.id), ["newer", "older"])
        XCTAssertFalse(viewModel.isLoading)
        XCTAssertNil(viewModel.errorMessage)
    }

    func testLoadSetsErrorStateWhenLoaderThrows() async {
        enum LoaderError: LocalizedError {
            case failed

            var errorDescription: String? { "load failed" }
        }

        let viewModel = ReportChatPaneViewModel()

        await viewModel.load(reportATag: "30023:pubkey:weekly") { _ in
            throw LoaderError.failed
        }

        XCTAssertTrue(viewModel.threads.isEmpty)
        XCTAssertEqual(viewModel.errorMessage, "load failed")
        XCTAssertFalse(viewModel.isLoading)
    }

    func testRefreshDebouncedCoalescesRapidRefreshes() async {
        let viewModel = ReportChatPaneViewModel()
        var loadCallCount = 0

        viewModel.refreshDebounced(
            reportATag: "30023:pubkey:weekly",
            delayNanoseconds: 50_000_000
        ) { _ in
            loadCallCount += 1
            return [self.makeThread(id: "first", title: "First", lastActivity: 100)]
        }

        viewModel.refreshDebounced(
            reportATag: "30023:pubkey:weekly",
            delayNanoseconds: 50_000_000
        ) { _ in
            loadCallCount += 1
            return [self.makeThread(id: "second", title: "Second", lastActivity: 200)]
        }

        try? await Task.sleep(nanoseconds: 150_000_000)

        XCTAssertEqual(loadCallCount, 1)
        XCTAssertEqual(viewModel.threads.map(\.id), ["second"])
        XCTAssertNil(viewModel.errorMessage)
    }

    func testPreferredEntryModeIsListWhenReportThreadsExist() async {
        let viewModel = ReportChatPaneViewModel()

        await viewModel.load(reportATag: "30023:pubkey:weekly") { _ in
            [self.makeThread(id: "thread-1", title: "Thread", lastActivity: 100)]
        }

        XCTAssertEqual(viewModel.preferredEntryMode(), .list)
    }

    func testPreferredEntryModeIsNewConversationWhenReportThreadsAreEmpty() async {
        let viewModel = ReportChatPaneViewModel()

        await viewModel.load(reportATag: "30023:pubkey:weekly") { _ in [] }

        XCTAssertEqual(viewModel.preferredEntryMode(), .newConversation)
        XCTAssertTrue(viewModel.orderedConversationIds.isEmpty)
    }

    func testNewThreadComposerSeedCarriesReportReferenceAndNoAttachments() {
        let seed = NewThreadComposerSeed(
            projectId: "project-1",
            agentPubkey: "author-pubkey",
            initialContent: "",
            textAttachments: [],
            referenceConversationId: nil,
            referenceReportATag: "30023:author-pubkey:weekly-report"
        )

        XCTAssertEqual(seed.projectId, "project-1")
        XCTAssertEqual(seed.agentPubkey, "author-pubkey")
        XCTAssertEqual(seed.initialContent, "")
        XCTAssertEqual(seed.textAttachments, [])
        XCTAssertNil(seed.referenceConversationId)
        XCTAssertEqual(seed.referenceReportATag, "30023:author-pubkey:weekly-report")
    }

    private func makeThread(id: String, title: String, lastActivity: UInt64) -> TenexMVP.Thread {
        TenexMVP.Thread(
            id: id,
            title: title,
            content: "",
            pubkey: "pubkey-\(id)",
            lastActivity: lastActivity,
            effectiveLastActivity: lastActivity,
            statusLabel: nil,
            statusCurrentActivity: nil,
            summary: nil,
            hashtags: [],
            parentConversationId: nil,
            pTags: [],
            askEvent: nil,
            isScheduled: false
        )
    }
}
