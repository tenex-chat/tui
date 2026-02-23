import XCTest
@testable import TenexMVP

@MainActor
final class AppFilterPersistenceTests: XCTestCase {
    private var defaults: UserDefaults!
    private let suiteName = "AppFilterPersistenceTests"

    override func setUp() {
        super.setUp()
        defaults = UserDefaults(suiteName: suiteName)
        defaults.removePersistentDomain(forName: suiteName)
    }

    override func tearDown() {
        defaults.removePersistentDomain(forName: suiteName)
        defaults = nil
        super.tearDown()
    }

    func testLoadPersistedAppFilterReturnsDefaultsWhenUnset() {
        let result = TenexCoreManager.loadPersistedAppFilter(defaults: defaults)

        XCTAssertTrue(result.projectIds.isEmpty)
        XCTAssertEqual(result.timeWindow, .defaultValue)
        XCTAssertEqual(result.scheduledEvent, .defaultValue)
        XCTAssertEqual(result.status, .defaultValue)
        XCTAssertTrue(result.hashtags.isEmpty)
    }

    func testPersistAndLoadRoundTripsValues() {
        TenexCoreManager.persistAppFilter(
            projectIds: ["project-b", "project-a"],
            timeWindow: .days7,
            scheduledEvent: .hide,
            status: .label("In Progress"),
            hashtags: ["feature", "frontend"],
            defaults: defaults
        )

        let loaded = TenexCoreManager.loadPersistedAppFilter(defaults: defaults)

        XCTAssertEqual(loaded.projectIds, Set(["project-a", "project-b"]))
        XCTAssertEqual(loaded.timeWindow, .days7)
        XCTAssertEqual(loaded.scheduledEvent, .hide)
        XCTAssertEqual(loaded.status, .label("In Progress"))
        XCTAssertEqual(loaded.hashtags, Set(["feature", "frontend"]))
        XCTAssertEqual(
            defaults.stringArray(forKey: TenexCoreManager.appFilterProjectsDefaultsKey),
            ["project-a", "project-b"]
        )
        XCTAssertEqual(
            defaults.stringArray(forKey: TenexCoreManager.appFilterHashtagsDefaultsKey),
            ["feature", "frontend"]
        )
    }

    func testLoadPersistedAppFilterFallsBackForInvalidTimeWindowRawValue() {
        defaults.set(
            ["project-a"],
            forKey: TenexCoreManager.appFilterProjectsDefaultsKey
        )
        defaults.set(
            "not-a-valid-window",
            forKey: TenexCoreManager.appFilterTimeWindowDefaultsKey
        )

        let loaded = TenexCoreManager.loadPersistedAppFilter(defaults: defaults)

        XCTAssertEqual(loaded.projectIds, Set(["project-a"]))
        XCTAssertEqual(loaded.timeWindow, .defaultValue)
    }
}
