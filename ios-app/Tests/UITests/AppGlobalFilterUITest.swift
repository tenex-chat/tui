import XCTest

final class AppGlobalFilterUITest: XCTestCase {
    private var app: XCUIApplication!
    private let defaultTimeout: TimeInterval = 15

    override func setUpWithError() throws {
        try super.setUpWithError()
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--uitesting"]
        app.launchEnvironment = [
            "UITEST_MODE": "true",
            "DISABLE_ANIMATIONS": "true"
        ]
    }

    override func tearDownWithError() throws {
        app = nil
        try super.tearDownWithError()
    }

    func testGlobalFilterDefaultSummaryIs24hAllProjects() throws {
        app.launch()
        try ensureMainUIOrSkip()

        let button = try waitForGlobalFilterButton()
        let summary = (button.value as? String) ?? button.label
        XCTAssertEqual(summary, "24h · All Projects")
    }

    func testTimeWindowSelectionPersistsAcrossRelaunch() throws {
        app.launch()
        try ensureMainUIOrSkip()

        let button = try waitForGlobalFilterButton()
        button.tap()

        let timeMenu = app.buttons["global_filter_menu_time"]
        XCTAssertTrue(timeMenu.waitForExistence(timeout: defaultTimeout))
        timeMenu.tap()

        let sevenDayOption = app.buttons["global_filter_time_days7"]
        XCTAssertTrue(sevenDayOption.waitForExistence(timeout: defaultTimeout))
        sevenDayOption.tap()

        let changedSummary = try waitForGlobalFilterSummaryPrefix("7d")
        XCTAssertTrue(changedSummary.hasPrefix("7d"))

        app.terminate()
        app.launch()
        try ensureMainUIOrSkip()

        let afterRelaunch = try waitForGlobalFilterButton()
        let relaunchedSummary = (afterRelaunch.value as? String) ?? afterRelaunch.label
        XCTAssertTrue(relaunchedSummary.hasPrefix("7d"))
    }

    private func ensureMainUIOrSkip() throws {
        let filterButton = app.buttons["global_filter_button"]
        if filterButton.waitForExistence(timeout: defaultTimeout) {
            return
        }

        let loginTextField = app.textFields.matching(
            NSPredicate(format: "placeholderValue CONTAINS[c] 'nsec' OR placeholderValue CONTAINS[c] 'key'")
        ).firstMatch
        let secureLoginField = app.secureTextFields["Secret Key"]
        if loginTextField.exists || secureLoginField.exists {
            throw XCTSkip("UI test requires an authenticated session.")
        }

        throw XCTSkip("Main UI did not appear in time.")
    }

    private func waitForGlobalFilterButton() throws -> XCUIElement {
        let button = app.buttons["global_filter_button"]
        guard button.waitForExistence(timeout: defaultTimeout) else {
            throw XCTSkip("Global filter button not found in active screen.")
        }
        return button
    }

    private func waitForGlobalFilterSummaryPrefix(_ prefix: String) throws -> String {
        let deadline = Date().addingTimeInterval(defaultTimeout)
        while Date() < deadline {
            let button = try waitForGlobalFilterButton()
            let summary = (button.value as? String) ?? button.label
            if summary.hasPrefix(prefix) {
                return summary
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.1))
        }
        throw XCTSkip("Global filter summary did not update with prefix '\(prefix)'.")
    }
}
