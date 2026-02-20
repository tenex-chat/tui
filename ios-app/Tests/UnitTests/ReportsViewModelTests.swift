import XCTest
@testable import TenexMVP

@MainActor
final class ReportsViewModelTests: XCTestCase {

    // MARK: - Search Filtering

    func testFilteredReportsReturnsAllWhenSearchTextIsEmpty() {
        let vm = ReportsViewModel()
        vm.reports = [
            makeReport(id: "1", title: "Alpha Report"),
            makeReport(id: "2", title: "Beta Report"),
            makeReport(id: "3", title: "Gamma Report"),
        ]

        XCTAssertEqual(vm.filteredReports.count, 3)
    }

    func testFilteredReportsFiltersByTitle() {
        let vm = ReportsViewModel()
        vm.reports = [
            makeReport(id: "1", title: "Performance Analysis"),
            makeReport(id: "2", title: "Security Audit"),
            makeReport(id: "3", title: "Performance Tuning"),
        ]
        vm.searchText = "Performance"

        let filtered = vm.filteredReports
        XCTAssertEqual(filtered.count, 2)
        XCTAssertTrue(filtered.allSatisfy { $0.title.contains("Performance") })
    }

    func testFilteredReportsFiltersBySummary() {
        let vm = ReportsViewModel()
        vm.reports = [
            makeReport(id: "1", title: "Report A", summary: "Contains important metrics"),
            makeReport(id: "2", title: "Report B", summary: "Nothing relevant here"),
        ]
        vm.searchText = "metrics"

        let filtered = vm.filteredReports
        XCTAssertEqual(filtered.count, 1)
        XCTAssertEqual(filtered.first?.id, "1")
    }

    func testFilteredReportsFiltersByHashtags() {
        let vm = ReportsViewModel()
        vm.reports = [
            makeReport(id: "1", title: "Report A", hashtags: ["rust", "backend"]),
            makeReport(id: "2", title: "Report B", hashtags: ["swift", "ios"]),
            makeReport(id: "3", title: "Report C", hashtags: ["rust", "wasm"]),
        ]
        vm.searchText = "rust"

        let filtered = vm.filteredReports
        XCTAssertEqual(filtered.count, 2)
        XCTAssertEqual(Set(filtered.map(\.id)), Set(["1", "3"]))
    }

    func testFilteredReportsIsCaseInsensitive() {
        let vm = ReportsViewModel()
        vm.reports = [
            makeReport(id: "1", title: "Performance Report"),
            makeReport(id: "2", title: "PERFORMANCE METRICS"),
            makeReport(id: "3", title: "Other"),
        ]
        vm.searchText = "performance"

        XCTAssertEqual(vm.filteredReports.count, 2)

        vm.searchText = "PERFORMANCE"
        XCTAssertEqual(vm.filteredReports.count, 2)

        vm.searchText = "PeRfOrMaNcE"
        XCTAssertEqual(vm.filteredReports.count, 2)
    }

    func testFilteredReportsReturnsEmptyWhenNothingMatches() {
        let vm = ReportsViewModel()
        vm.reports = [
            makeReport(id: "1", title: "Alpha"),
            makeReport(id: "2", title: "Beta"),
        ]
        vm.searchText = "zzz_nonexistent_zzz"

        XCTAssertTrue(vm.filteredReports.isEmpty)
    }

    func testFilteredReportsMatchesPartialStrings() {
        let vm = ReportsViewModel()
        vm.reports = [
            makeReport(id: "1", title: "Infrastructure Overview"),
            makeReport(id: "2", title: "API Documentation"),
        ]
        vm.searchText = "struct"

        let filtered = vm.filteredReports
        XCTAssertEqual(filtered.count, 1)
        XCTAssertEqual(filtered.first?.id, "1")
    }

    func testFilteredReportsMatchesAcrossFields() {
        let vm = ReportsViewModel()
        vm.reports = [
            makeReport(id: "1", title: "Report A", summary: "nothing", hashtags: ["deploy"]),
            makeReport(id: "2", title: "Deploy Guide", summary: "nothing", hashtags: []),
            makeReport(id: "3", title: "Report C", summary: "deploy process", hashtags: []),
        ]
        vm.searchText = "deploy"

        let filtered = vm.filteredReports
        XCTAssertEqual(filtered.count, 3)
    }

    // MARK: - Sorting

    func testReportsPreserveSortOrderInFilteredResults() {
        let vm = ReportsViewModel()
        vm.reports = [
            makeReport(id: "newest", title: "Newest", createdAt: 3000),
            makeReport(id: "middle", title: "Middle", createdAt: 2000),
            makeReport(id: "oldest", title: "Oldest", createdAt: 1000),
        ]

        let ids = vm.filteredReports.map(\.id)
        XCTAssertEqual(ids, ["newest", "middle", "oldest"])
    }

    // MARK: - projectFor(report:) Without CoreManager

    func testProjectForReportReturnsNilWithoutCoreManager() {
        let vm = ReportsViewModel()
        let report = makeReport(id: "1", projectATag: "30023:pubkey:project-id")

        XCTAssertNil(vm.projectFor(report: report))
    }

    // MARK: - Edge Cases

    func testFilteredReportsWithEmptyReportsArray() {
        let vm = ReportsViewModel()
        vm.searchText = "anything"

        XCTAssertTrue(vm.filteredReports.isEmpty)
    }

    func testFilteredReportsWithWhitespaceOnlySearchText() {
        let vm = ReportsViewModel()
        vm.reports = [
            makeReport(id: "1", title: "Report"),
        ]
        vm.searchText = "   "

        // Non-empty searchText "   " won't match title "Report"
        XCTAssertTrue(vm.filteredReports.isEmpty)
    }

    // MARK: - Helpers

    private func makeReport(
        id: String,
        title: String = "Title",
        summary: String = "Summary",
        hashtags: [String] = [],
        createdAt: UInt64 = 1000,
        projectATag: String = "30023:pubkey:default-project"
    ) -> Report {
        Report(
            id: id,
            slug: "slug-\(id)",
            projectATag: projectATag,
            author: "author-pubkey",
            title: title,
            summary: summary,
            content: "Full content for \(id)",
            hashtags: hashtags,
            createdAt: createdAt,
            readingTimeMins: 5
        )
    }
}
