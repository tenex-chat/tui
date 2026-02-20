import XCTest
@testable import TenexMVP

final class FormattingHelperTests: XCTestCase {

    // MARK: - StatsSnapshot.formatRuntime

    func testFormatRuntimeZeroMilliseconds() {
        XCTAssertEqual(StatsSnapshot.formatRuntime(0), "0s")
    }

    func testFormatRuntimeSubSecondMilliseconds() {
        XCTAssertEqual(StatsSnapshot.formatRuntime(1), "1ms")
        XCTAssertEqual(StatsSnapshot.formatRuntime(500), "500ms")
        XCTAssertEqual(StatsSnapshot.formatRuntime(999), "999ms")
    }

    func testFormatRuntimeExactlyOneSecond() {
        XCTAssertEqual(StatsSnapshot.formatRuntime(1000), "1s")
    }

    func testFormatRuntimeSeconds() {
        XCTAssertEqual(StatsSnapshot.formatRuntime(5000), "5s")
        XCTAssertEqual(StatsSnapshot.formatRuntime(45_000), "45s")
        XCTAssertEqual(StatsSnapshot.formatRuntime(59_000), "59s")
    }

    func testFormatRuntimeMinutesOnly() {
        XCTAssertEqual(StatsSnapshot.formatRuntime(60_000), "1m")
        XCTAssertEqual(StatsSnapshot.formatRuntime(120_000), "2m")
        XCTAssertEqual(StatsSnapshot.formatRuntime(300_000), "5m")
    }

    func testFormatRuntimeMinutesAndSeconds() {
        XCTAssertEqual(StatsSnapshot.formatRuntime(90_000), "1m 30s")
        XCTAssertEqual(StatsSnapshot.formatRuntime(150_000), "2m 30s")
        XCTAssertEqual(StatsSnapshot.formatRuntime(3599_000), "59m 59s")
    }

    func testFormatRuntimeHoursOnly() {
        XCTAssertEqual(StatsSnapshot.formatRuntime(3_600_000), "1h")
        XCTAssertEqual(StatsSnapshot.formatRuntime(7_200_000), "2h")
    }

    func testFormatRuntimeHoursAndMinutes() {
        XCTAssertEqual(StatsSnapshot.formatRuntime(6_300_000), "1h 45m")
        XCTAssertEqual(StatsSnapshot.formatRuntime(5_400_000), "1h 30m")
        // 2h 30m = 9000s = 9_000_000ms
        XCTAssertEqual(StatsSnapshot.formatRuntime(9_000_000), "2h 30m")
    }

    func testFormatRuntimeHoursDropsSeconds() {
        // 1h 0m 30s should show as "1h" (seconds are dropped in the hour range)
        XCTAssertEqual(StatsSnapshot.formatRuntime(3_630_000), "1h")
    }

    // MARK: - StatsSnapshot.formatDayLabel

    func testFormatDayLabelToday() {
        let todayStart: UInt64 = 1_700_000_000
        XCTAssertEqual(StatsSnapshot.formatDayLabel(todayStart, todayStart: todayStart), "Today")
    }

    func testFormatDayLabelYesterday() {
        let todayStart: UInt64 = 1_700_000_000
        let yesterdayStart = todayStart - 86400
        XCTAssertEqual(StatsSnapshot.formatDayLabel(yesterdayStart, todayStart: todayStart), "Yest.")
    }

    func testFormatDayLabelOlderDates() {
        let todayStart: UInt64 = 1_700_000_000
        let twoDaysAgo = todayStart - (86400 * 2)
        let label = StatsSnapshot.formatDayLabel(twoDaysAgo, todayStart: todayStart)
        // Should be a "MMM d" formatted date, not "Today" or "Yest."
        XCTAssertNotEqual(label, "Today")
        XCTAssertNotEqual(label, "Yest.")
        XCTAssertFalse(label.isEmpty)
    }

    func testFormatDayLabelSpecificDate() {
        // 2024-01-15 00:00:00 UTC = 1705276800
        let dayStart: UInt64 = 1_705_276_800
        let todayStart: UInt64 = dayStart + (86400 * 5) // 5 days later
        let label = StatsSnapshot.formatDayLabel(dayStart, todayStart: todayStart)
        XCTAssertEqual(label, "Jan 15")
    }

    func testFormatDayLabelSevenDaysAgo() {
        let todayStart: UInt64 = 1_700_000_000
        let sevenDaysAgo = todayStart - (86400 * 7)
        let label = StatsSnapshot.formatDayLabel(sevenDaysAgo, todayStart: todayStart)
        XCTAssertNotEqual(label, "Today")
        XCTAssertNotEqual(label, "Yest.")
    }

    // MARK: - DiagnosticsFormatters.formatNumber

    func testFormatNumberSmallValues() {
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(0), "0")
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(1), "1")
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(42), "42")
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(999), "999")
    }

    func testFormatNumberThousands() {
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(1_000), "1.0K")
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(1_500), "1.5K")
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(10_000), "10.0K")
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(999_999), "1000.0K")
    }

    func testFormatNumberMillions() {
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(1_000_000), "1.0M")
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(2_500_000), "2.5M")
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(100_000_000), "100.0M")
    }

    func testFormatNumberBoundaryBetweenKAndM() {
        // 999_999 is < 1_000_000, so should be K
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(999_999), "1000.0K")
        // 1_000_000 is >= 1_000_000, so should be M
        XCTAssertEqual(DiagnosticsFormatters.formatNumber(1_000_000), "1.0M")
    }

    // MARK: - DiagnosticsFormatters.formatDuration

    func testFormatDurationZero() {
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(0), "0s")
    }

    func testFormatDurationSubSecond() {
        // Sub-second ms values still compute to 0 total seconds
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(500), "0s")
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(999), "0s")
    }

    func testFormatDurationSeconds() {
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(1_000), "1s")
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(30_000), "30s")
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(59_000), "59s")
    }

    func testFormatDurationMinutesAndSeconds() {
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(60_000), "1m 0s")
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(90_000), "1m 30s")
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(150_000), "2m 30s")
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(3_599_000), "59m 59s")
    }

    func testFormatDurationHoursAndMinutes() {
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(3_600_000), "1h 0m")
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(5_400_000), "1h 30m")
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(7_200_000), "2h 0m")
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(9_000_000), "2h 30m")
    }

    func testFormatDurationHoursDropsSeconds() {
        // 1h 0m 30s (3630 seconds = 3_630_000 ms) should show "1h 0m" (seconds dropped)
        XCTAssertEqual(DiagnosticsFormatters.formatDuration(3_630_000), "1h 0m")
    }

    // MARK: - DiagnosticsSnapshot.formatBytes

    func testFormatBytesBytes() {
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(0), "0 B")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(1), "1 B")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(512), "512 B")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(1023), "1023 B")
    }

    func testFormatBytesKilobytes() {
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(1024), "1.0 KB")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(1536), "1.5 KB")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(10_240), "10.0 KB")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(512_000), "500.0 KB")
    }

    func testFormatBytesMegabytes() {
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(1_048_576), "1.0 MB")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(1_572_864), "1.5 MB")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(10_485_760), "10.0 MB")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(524_288_000), "500.0 MB")
    }

    func testFormatBytesGigabytes() {
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(1_073_741_824), "1.0 GB")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(1_610_612_736), "1.5 GB")
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(10_737_418_240), "10.0 GB")
    }

    func testFormatBytesBoundaryKBtoMB() {
        // 1023 KB = 1047552 bytes -> should still be KB
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(1_047_552), "1023.0 KB")
        // 1024 KB = 1048576 bytes -> should be MB
        XCTAssertEqual(DiagnosticsSnapshot.formatBytes(1_048_576), "1.0 MB")
    }

    // MARK: - DiagnosticsSnapshot.formatTimeSince

    func testFormatTimeSinceNil() {
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(nil), "Never")
    }

    func testFormatTimeSinceSeconds() {
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(0), "0s ago")
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(1), "1s ago")
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(30), "30s ago")
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(59), "59s ago")
    }

    func testFormatTimeSinceMinutes() {
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(60), "1m ago")
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(120), "2m ago")
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(300), "5m ago")
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(3599), "59m ago")
    }

    func testFormatTimeSinceHours() {
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(3600), "1h ago")
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(7200), "2h ago")
        XCTAssertEqual(DiagnosticsSnapshot.formatTimeSince(86400), "24h ago")
    }

    // MARK: - NegentropySyncDiagnostics.successRate

    func testSuccessRateNoSyncs() {
        let diag = makeSyncDiagnostics(successful: 0, failed: 0)
        XCTAssertEqual(diag.successRate, 100.0)
    }

    func testSuccessRateAllSuccessful() {
        let diag = makeSyncDiagnostics(successful: 10, failed: 0)
        XCTAssertEqual(diag.successRate, 100.0)
    }

    func testSuccessRateAllFailed() {
        let diag = makeSyncDiagnostics(successful: 0, failed: 10)
        XCTAssertEqual(diag.successRate, 0.0)
    }

    func testSuccessRateMixed() {
        let diag = makeSyncDiagnostics(successful: 7, failed: 3)
        XCTAssertEqual(diag.successRate, 70.0)
    }

    func testSuccessRateHalf() {
        let diag = makeSyncDiagnostics(successful: 5, failed: 5)
        XCTAssertEqual(diag.successRate, 50.0)
    }

    func testSuccessRateHighPrecision() {
        let diag = makeSyncDiagnostics(successful: 99, failed: 1)
        XCTAssertEqual(diag.successRate, 99.0)
    }

    func testSuccessRateOneOfEach() {
        let diag = makeSyncDiagnostics(successful: 1, failed: 1)
        XCTAssertEqual(diag.successRate, 50.0)
    }

    // MARK: - NegentropySyncDiagnostics.successRateColor

    func testSuccessRateColorGreen() {
        let diag90 = makeSyncDiagnostics(successful: 90, failed: 10)
        XCTAssertEqual(diag90.successRateColor, .green)

        let diag100 = makeSyncDiagnostics(successful: 100, failed: 0)
        XCTAssertEqual(diag100.successRateColor, .green)

        let diagNone = makeSyncDiagnostics(successful: 0, failed: 0)
        XCTAssertEqual(diagNone.successRateColor, .green, "No syncs defaults to 100% = green")
    }

    func testSuccessRateColorOrange() {
        let diag70 = makeSyncDiagnostics(successful: 70, failed: 30)
        XCTAssertEqual(diag70.successRateColor, .orange)

        let diag89 = makeSyncDiagnostics(successful: 89, failed: 11)
        XCTAssertEqual(diag89.successRateColor, .orange)
    }

    func testSuccessRateColorRed() {
        let diag69 = makeSyncDiagnostics(successful: 69, failed: 31)
        XCTAssertEqual(diag69.successRateColor, .red)

        let diag0 = makeSyncDiagnostics(successful: 0, failed: 100)
        XCTAssertEqual(diag0.successRateColor, .red)
    }

    // MARK: - DiagnosticsSnapshot.sortedSubscriptions

    func testSortedSubscriptionsNil() {
        let snapshot = DiagnosticsSnapshot(
            system: nil,
            sync: nil,
            subscriptions: nil,
            totalSubscriptionEvents: 0,
            database: nil,
            sectionErrors: []
        )
        XCTAssertEqual(snapshot.sortedSubscriptions.count, 0)
    }

    func testSortedSubscriptionsEmpty() {
        let snapshot = DiagnosticsSnapshot(
            system: nil,
            sync: nil,
            subscriptions: [],
            totalSubscriptionEvents: 0,
            database: nil,
            sectionErrors: []
        )
        XCTAssertEqual(snapshot.sortedSubscriptions.count, 0)
    }

    func testSortedSubscriptionsOrderedByEventsReceived() {
        let sub1 = SubscriptionDiagnostics(
            subId: "sub-1", description: "First", kinds: [], rawFilter: nil, eventsReceived: 10, ageSecs: 100
        )
        let sub2 = SubscriptionDiagnostics(
            subId: "sub-2", description: "Second", kinds: [], rawFilter: nil, eventsReceived: 500, ageSecs: 200
        )
        let sub3 = SubscriptionDiagnostics(
            subId: "sub-3", description: "Third", kinds: [], rawFilter: nil, eventsReceived: 50, ageSecs: 50
        )
        let snapshot = DiagnosticsSnapshot(
            system: nil,
            sync: nil,
            subscriptions: [sub1, sub2, sub3],
            totalSubscriptionEvents: 560,
            database: nil,
            sectionErrors: []
        )
        let sorted = snapshot.sortedSubscriptions
        XCTAssertEqual(sorted.count, 3)
        XCTAssertEqual(sorted[0].subId, "sub-2")
        XCTAssertEqual(sorted[1].subId, "sub-3")
        XCTAssertEqual(sorted[2].subId, "sub-1")
    }

    // MARK: - Helpers

    private func makeSyncDiagnostics(
        successful: UInt64,
        failed: UInt64
    ) -> NegentropySyncDiagnostics {
        NegentropySyncDiagnostics(
            enabled: true,
            currentIntervalSecs: 60,
            secondsSinceLastCycle: nil,
            syncInProgress: false,
            successfulSyncs: successful,
            failedSyncs: failed,
            unsupportedSyncs: 0,
            totalEventsReconciled: 0,
            recentResults: []
        )
    }
}
