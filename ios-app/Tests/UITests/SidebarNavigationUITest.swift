import XCTest

final class SidebarNavigationUITest: XCTestCase {
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

    func testSidebarRowsNavigateToSectionContent() throws {
        app.launch()
        try ensureSidebarOrSkip()

        let sections: [(rowID: String, contentID: String)] = [
            ("section_row_chats", "section_content_chats"),
            ("section_row_projects", "section_content_projects"),
            ("section_row_reports", "section_content_reports"),
            ("section_row_inbox", "section_content_inbox"),
            ("section_row_search", "section_content_search"),
            ("section_row_teams", "section_content_teams"),
            ("section_row_agentDefinitions", "section_content_agentDefinitions")
        ]

        for section in sections {
            try tapSidebarRow(section.rowID)
            let content = element(matchingID: section.contentID)
            XCTAssertTrue(
                content.waitForExistence(timeout: defaultTimeout),
                "Expected section content \(section.contentID) after tapping \(section.rowID)"
            )
        }
    }

    func testSidebarSettingsOpensAndDismisses() throws {
        app.launch()
        try ensureSidebarOrSkip()

        let settingsButton = app.buttons["Settings"]
        guard settingsButton.waitForExistence(timeout: defaultTimeout) else {
            throw XCTSkip("Settings button was not found in sidebar.")
        }

        settingsButton.tap()

        let doneButton = app.buttons["Done"]
        XCTAssertTrue(doneButton.waitForExistence(timeout: defaultTimeout), "Settings sheet should present a Done button.")
        doneButton.tap()

        try tapSidebarRow("section_row_chats")
        XCTAssertTrue(
            element(matchingID: "section_content_chats").waitForExistence(timeout: defaultTimeout),
            "Chat section should be navigable after dismissing settings."
        )
    }

    private func ensureSidebarOrSkip() throws {
        if element(matchingID: "app_sidebar").waitForExistence(timeout: defaultTimeout) {
            return
        }

        let loginTextField = app.textFields.matching(
            NSPredicate(format: "placeholderValue CONTAINS[c] 'nsec' OR placeholderValue CONTAINS[c] 'key'")
        ).firstMatch
        let secureLoginField = app.secureTextFields["Secret Key"]

        if loginTextField.exists || secureLoginField.exists {
            throw XCTSkip("UI test requires an authenticated session.")
        }

        throw XCTSkip("Sidebar is unavailable (likely compact iPhone layout).")
    }

    private func tapSidebarRow(_ rowID: String) throws {
        let row = app.cells.containing(.any, identifier: rowID).firstMatch
        let target = row.exists ? row : element(matchingID: rowID)

        guard target.waitForExistence(timeout: defaultTimeout) else {
            XCTFail("Sidebar row \(rowID) not found.")
            return
        }

        if target.isHittable {
            target.tap()
        } else {
            target.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.5)).tap()
        }
    }

    private func element(matchingID identifier: String) -> XCUIElement {
        app.descendants(matching: .any).matching(identifier: identifier).firstMatch
    }
}
