import XCTest
@testable import TenexMVP

// MARK: - ToolGroup Tests

final class ToolGroupTests: XCTestCase {

    // MARK: buildGroups - MCP prefix parsing

    func testBuildGroupsGroupsMcpToolsByServerName() {
        let tools = [
            "mcp__chrome__click",
            "mcp__chrome__navigate",
            "mcp__chrome__screenshot",
            "mcp__xcode__build",
            "mcp__xcode__test",
        ]

        let groups = ToolGroup.buildGroups(from: tools)

        // Should produce exactly 2 groups: MCP: chrome, MCP: xcode
        XCTAssertEqual(groups.count, 2)
        XCTAssertEqual(groups[0].name, "MCP: chrome")
        XCTAssertEqual(groups[0].tools, ["mcp__chrome__click", "mcp__chrome__navigate", "mcp__chrome__screenshot"])
        XCTAssertEqual(groups[1].name, "MCP: xcode")
        XCTAssertEqual(groups[1].tools, ["mcp__xcode__build", "mcp__xcode__test"])
    }

    func testBuildGroupsMcpGroupsAreCollapsedByDefault() {
        let tools = ["mcp__server__a", "mcp__server__b"]
        let groups = ToolGroup.buildGroups(from: tools)

        XCTAssertEqual(groups.count, 1)
        XCTAssertFalse(groups[0].isExpanded)
    }

    // MARK: buildGroups - underscore prefix fallback

    func testBuildGroupsGroupsByUnderscorePrefixWhenTwoOrMore() {
        let tools = [
            "file_read",
            "file_write",
            "file_delete",
            "bash",
        ]

        let groups = ToolGroup.buildGroups(from: tools)

        // "file_*" tools grouped under "FILE", "bash" is ungrouped single-item
        XCTAssertEqual(groups.count, 2)
        XCTAssertEqual(groups[0].name, "FILE")
        XCTAssertEqual(groups[0].tools, ["file_delete", "file_read", "file_write"])
        XCTAssertEqual(groups[1].name, "bash")
        XCTAssertEqual(groups[1].tools, ["bash"])
    }

    func testBuildGroupsSingleUnderscorePrefixNotGrouped() {
        // Only one tool with "net_" prefix, so it should NOT be grouped
        let tools = ["net_fetch", "bash", "grep"]

        let groups = ToolGroup.buildGroups(from: tools)

        // All three should be ungrouped single-item groups, sorted alphabetically
        XCTAssertEqual(groups.count, 3)
        XCTAssertEqual(groups.map(\.name), ["bash", "grep", "net_fetch"])
    }

    // MARK: buildGroups - single / ungrouped tools

    func testBuildGroupsSingleToolsAreExpandedByDefault() {
        let tools = ["bash"]
        let groups = ToolGroup.buildGroups(from: tools)

        XCTAssertEqual(groups.count, 1)
        XCTAssertTrue(groups[0].isExpanded)
        XCTAssertEqual(groups[0].tools, ["bash"])
    }

    func testBuildGroupsEmptyInput() {
        let groups = ToolGroup.buildGroups(from: [])
        XCTAssertTrue(groups.isEmpty)
    }

    func testBuildGroupsMixedMcpUnderscoreAndSingleTools() {
        let tools = [
            "mcp__db__query",
            "mcp__db__insert",
            "file_read",
            "file_write",
            "grep",
        ]

        let groups = ToolGroup.buildGroups(from: tools)

        // Grouped first (sorted by name): FILE, MCP: db; then ungrouped: grep
        XCTAssertEqual(groups.count, 3)
        XCTAssertEqual(groups[0].name, "FILE")
        XCTAssertEqual(groups[1].name, "MCP: db")
        XCTAssertEqual(groups[2].name, "grep")
    }

    // MARK: isFullySelected / isPartiallySelected

    func testIsFullySelectedReturnsTrueWhenAllToolsSelected() {
        let group = ToolGroup(name: "FILE", tools: ["file_read", "file_write"], isExpanded: false)
        let selected: Set<String> = ["file_read", "file_write", "bash"]

        XCTAssertTrue(group.isFullySelected(selected))
    }

    func testIsFullySelectedReturnsFalseWhenSomeMissing() {
        let group = ToolGroup(name: "FILE", tools: ["file_read", "file_write"], isExpanded: false)
        let selected: Set<String> = ["file_read"]

        XCTAssertFalse(group.isFullySelected(selected))
    }

    func testIsPartiallySelectedReturnsTrueWhenSomeButNotAll() {
        let group = ToolGroup(name: "FILE", tools: ["file_read", "file_write", "file_delete"], isExpanded: false)
        let selected: Set<String> = ["file_read"]

        XCTAssertTrue(group.isPartiallySelected(selected))
    }

    func testIsPartiallySelectedReturnsFalseWhenAllSelected() {
        let group = ToolGroup(name: "FILE", tools: ["file_read", "file_write"], isExpanded: false)
        let selected: Set<String> = ["file_read", "file_write"]

        XCTAssertFalse(group.isPartiallySelected(selected))
    }

    func testIsPartiallySelectedReturnsFalseWhenNoneSelected() {
        let group = ToolGroup(name: "FILE", tools: ["file_read", "file_write"], isExpanded: false)
        let selected: Set<String> = ["bash"]

        XCTAssertFalse(group.isPartiallySelected(selected))
    }
}

// MARK: - AppGlobalFilter Tests

final class AppGlobalFilterTests: XCTestCase {

    // MARK: AppTimeWindow.cutoffTimestamp

    func testCutoffTimestamp4Hours() {
        let now: UInt64 = 1_000_000
        let cutoff = AppTimeWindow.hours4.cutoffTimestamp(now: now)
        XCTAssertEqual(cutoff, now - 4 * 60 * 60)
    }

    func testCutoffTimestamp12Hours() {
        let now: UInt64 = 1_000_000
        let cutoff = AppTimeWindow.hours12.cutoffTimestamp(now: now)
        XCTAssertEqual(cutoff, now - 12 * 60 * 60)
    }

    func testCutoffTimestamp24Hours() {
        let now: UInt64 = 1_000_000
        let cutoff = AppTimeWindow.hours24.cutoffTimestamp(now: now)
        XCTAssertEqual(cutoff, now - 24 * 60 * 60)
    }

    func testCutoffTimestamp7Days() {
        let now: UInt64 = 1_000_000
        let cutoff = AppTimeWindow.days7.cutoffTimestamp(now: now)
        XCTAssertEqual(cutoff, now - 7 * 24 * 60 * 60)
    }

    func testCutoffTimestampAllReturnsNil() {
        let cutoff = AppTimeWindow.all.cutoffTimestamp(now: 1_000_000)
        XCTAssertNil(cutoff)
    }

    func testCutoffTimestampClampsToZeroWhenNowIsSmall() {
        // now < cutoffSeconds should clamp to 0, not underflow
        let cutoff = AppTimeWindow.days7.cutoffTimestamp(now: 100)
        XCTAssertEqual(cutoff, 0)
    }

    // MARK: AppTimeWindow.includes

    func testIncludesReturnsTrueForTimestampWithinWindow() {
        let now: UInt64 = 1_000_000
        let oneHourAgo = now - 3600
        XCTAssertTrue(AppTimeWindow.hours4.includes(timestamp: oneHourAgo, now: now))
    }

    func testIncludesReturnsFalseForTimestampOutsideWindow() {
        let now: UInt64 = 1_000_000
        let fiveHoursAgo = now - 5 * 3600
        XCTAssertFalse(AppTimeWindow.hours4.includes(timestamp: fiveHoursAgo, now: now))
    }

    func testIncludesReturnsTrueForExactBoundary() {
        let now: UInt64 = 1_000_000
        let exactCutoff = now - 4 * 3600
        XCTAssertTrue(AppTimeWindow.hours4.includes(timestamp: exactCutoff, now: now))
    }

    func testIncludesAllWindowAlwaysReturnsTrue() {
        XCTAssertTrue(AppTimeWindow.all.includes(timestamp: 0, now: 1_000_000))
        XCTAssertTrue(AppTimeWindow.all.includes(timestamp: 1_000_000, now: 1_000_000))
    }

    // MARK: AppGlobalFilterSnapshot.includes

    func testSnapshotIncludesMatchesProjectAndTime() {
        let snapshot = AppGlobalFilterSnapshot(
            projectIds: Set(["proj-a", "proj-b"]),
            timeWindow: .hours4
        )
        let now: UInt64 = 1_000_000
        let recent = now - 3600

        XCTAssertTrue(snapshot.includes(projectId: "proj-a", timestamp: recent, now: now))
        XCTAssertFalse(snapshot.includes(projectId: "proj-c", timestamp: recent, now: now))
    }

    func testSnapshotIncludesRejectsOldTimestampEvenIfProjectMatches() {
        let snapshot = AppGlobalFilterSnapshot(
            projectIds: Set(["proj-a"]),
            timeWindow: .hours4
        )
        let now: UInt64 = 1_000_000
        let old = now - 5 * 3600

        XCTAssertFalse(snapshot.includes(projectId: "proj-a", timestamp: old, now: now))
    }

    func testSnapshotIncludesEmptyProjectIdsMatchesAll() {
        let snapshot = AppGlobalFilterSnapshot(
            projectIds: Set(),
            timeWindow: .hours4
        )
        let now: UInt64 = 1_000_000
        let recent = now - 3600

        XCTAssertTrue(snapshot.includes(projectId: "any-project", timestamp: recent, now: now))
        XCTAssertTrue(snapshot.includes(projectId: nil, timestamp: recent, now: now))
    }

    func testSnapshotIncludesNilProjectIdRejectedWhenFilterHasProjects() {
        let snapshot = AppGlobalFilterSnapshot(
            projectIds: Set(["proj-a"]),
            timeWindow: .all
        )

        XCTAssertFalse(snapshot.includes(projectId: nil, timestamp: 500, now: 1_000_000))
    }

    func testSnapshotIsDefaultWhenEmptyProjectsAndDefaultTimeWindow() {
        let snapshot = AppGlobalFilterSnapshot(projectIds: Set(), timeWindow: .defaultValue)
        XCTAssertTrue(snapshot.isDefault)
    }

    func testSnapshotIsNotDefaultWhenProjectsSet() {
        let snapshot = AppGlobalFilterSnapshot(projectIds: Set(["proj-a"]), timeWindow: .defaultValue)
        XCTAssertFalse(snapshot.isDefault)
    }

    func testSnapshotIsNotDefaultWhenTimeWindowChanged() {
        let snapshot = AppGlobalFilterSnapshot(projectIds: Set(), timeWindow: .days7)
        XCTAssertFalse(snapshot.isDefault)
    }
}
