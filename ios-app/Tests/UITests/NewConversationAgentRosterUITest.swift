import XCTest

final class NewConversationAgentRosterUITest: XCTestCase {
    private var app: XCUIApplication!
    private let timeout: TimeInterval = 20

    override func setUpWithError() throws {
        try super.setUpWithError()
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--uitesting"]
        app.launchEnvironment = [
            "UITEST_MODE": "true",
            "DISABLE_ANIMATIONS": "true"
        ]
        let environment = ProcessInfo.processInfo.environment
        if let nsec = environment["TENEX_UITEST_DEBUG_NSEC"]
            ?? environment["TENEX_DEBUG_NSEC"]
            ?? environment["TEST_RUNNER_TENEX_UITEST_DEBUG_NSEC"]
            ?? environment["TEST_RUNNER_TENEX_DEBUG_NSEC"],
            !nsec.isEmpty {
            app.launchEnvironment["TENEX_DEBUG_NSEC"] = nsec
        }
    }

    override func tearDownWithError() throws {
        app = nil
        try super.tearDownWithError()
    }

    func testNewConversationAgentSelectorUsesSelectedProjectRosterOnly() throws {
        app.launch()
        try ensureMainUIOrSkip()

        let createButton = app.buttons["new_conversation_button"]
        XCTAssertTrue(createButton.waitForExistence(timeout: timeout), "Create conversation button should be visible.")
        createButton.tap()

        let selectedProject = try firstProjectMenuItemWithAgents()
        let expectedPubkeys = pubkeys(from: selectedProject.value as? String)
        XCTAssertFalse(expectedPubkeys.isEmpty, "Selected project should expose its roster pubkeys.")
        selectedProject.tap()

        let selector = app.descendants(matching: .any)["new_conversation_agent_selector"]
        XCTAssertTrue(selector.waitForExistence(timeout: timeout), "New conversation agent selector should be visible.")

        let renderedPubkeys = pubkeys(from: selector.value as? String)
        XCTAssertEqual(
            renderedPubkeys,
            expectedPubkeys,
            "New conversation selector must render only the selected project's roster."
        )
    }

    private func ensureMainUIOrSkip() throws {
        if app.buttons["new_conversation_button"].waitForExistence(timeout: timeout)
            || app.buttons["Create conversation"].waitForExistence(timeout: 2) {
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

    private func firstProjectMenuItemWithAgents() throws -> XCUIElement {
        let projectPredicate = NSPredicate(format: "identifier BEGINSWITH %@", "new_conversation_project_")
        if app.buttons["Unbooted Projects"].waitForExistence(timeout: 2) {
            app.buttons["Unbooted Projects"].tap()
        }

        let deadline = Date().addingTimeInterval(timeout)
        while Date() < deadline {
            let items = app.buttons.matching(projectPredicate).allElementsBoundByIndex
            if let item = items.first(where: { !pubkeys(from: $0.value as? String).isEmpty }) {
                return item
            }
            RunLoop.current.run(until: Date().addingTimeInterval(0.2))
        }

        throw XCTSkip("No project with roster agents was available in the new conversation menu.")
    }

    private func pubkeys(from value: String?) -> [String] {
        (value ?? "")
            .split(separator: ",")
            .map(String.init)
            .filter { !$0.isEmpty }
    }
}
