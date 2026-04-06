import XCTest

final class ToolbarSlashButtonUITest: XCTestCase {
    let app = XCUIApplication()
    let testNSEC = "nsec1qxwkwn5fueyzf0htkdktfr46y6hvjvdzxzrzxkjtzyk6uf23gxxq3jxs5k"

    override func setUpWithError() throws {
        continueAfterFailure = false
        app.launch()
    }

    override func tearDownWithError() throws {
        app.terminate()
    }

    func loginIfNeeded() {
        let nsecField = app.secureTextFields.firstMatch
        if nsecField.waitForExistence(timeout: 5) {
            nsecField.tap()
            nsecField.typeText(testNSEC)
            let loginBtn = app.buttons["Login"]
            if loginBtn.waitForExistence(timeout: 3) && loginBtn.isEnabled {
                loginBtn.tap()
            }
            sleep(5)
        }
    }

    func dumpButtons(label: String) {
        var lines = ["=== BUTTONS [\(label)] ==="]
        for btn in app.buttons.allElementsBoundByIndex {
            lines.append("  Label: '\(btn.label)' | ID: '\(btn.identifier)' | Frame: \(btn.frame)")
        }
        let report = lines.joined(separator: "\n")
        print(report)
        let att = XCTAttachment(data: report.data(using: .utf8)!, uniformTypeIdentifier: "public.plain-text")
        att.name = "buttons_\(label)"
        att.lifetime = .keepAlways
        add(att)
    }

    func addScreenshot(name: String) {
        let shot = XCUIScreen.main.screenshot()
        let att = XCTAttachment(screenshot: shot)
        att.name = name
        att.lifetime = .keepAlways
        add(att)
    }

    func testToolbarSlashButtonExists() throws {
        loginIfNeeded()

        // Navigate to Chats tab
        let chatsTab = app.tabBars.buttons["Chats"]
        if chatsTab.waitForExistence(timeout: 10) { chatsTab.tap() }
        sleep(1)

        // Tap first conversation cell → preview sheet
        let firstCell = app.cells.firstMatch
        guard firstCell.waitForExistence(timeout: 10) else {
            XCTFail("No conversations found")
            return
        }
        firstCell.tap()
        sleep(2)
        addScreenshot(name: "01_preview_sheet")

        // Tap "View Full Conversation" → full conversation
        let viewFullBtn = app.buttons["View Full Conversation"]
        if viewFullBtn.waitForExistence(timeout: 5) {
            viewFullBtn.tap()
            sleep(3)
        }
        addScreenshot(name: "02_full_conversation")
        dumpButtons(label: "after_view_full")

        // === VERIFY: Old buttons REMOVED ===
        let addNudgeButton = app.buttons.matching(
            NSPredicate(format: "label CONTAINS[cd] 'nudge' AND label CONTAINS[cd] 'add'")
        ).firstMatch
        let addSkillButton = app.buttons.matching(
            NSPredicate(format: "label CONTAINS[cd] 'skill' AND label CONTAINS[cd] 'add'")
        ).firstMatch

        XCTAssertFalse(addNudgeButton.exists, "❌ 'Add Nudge' button should be REMOVED but still exists!")
        XCTAssertFalse(addSkillButton.exists, "❌ 'Add Skill' button should be REMOVED but still exists!")
        print("✅ 'Add Nudge' button: ABSENT (correct)")
        print("✅ 'Add Skill' button: ABSENT (correct)")

        // Try to activate compose mode by tapping "Reply" button
        let replyBtn = app.buttons["Reply"]
        if replyBtn.waitForExistence(timeout: 5) {
            print("Found 'Reply' button - tapping to activate compose mode")
            replyBtn.tap()
            sleep(2)
            addScreenshot(name: "03_after_reply_tap")
            dumpButtons(label: "after_reply_tap")
        } else {
            print("No 'Reply' button found - trying text editor")
            // Try tapping on a text field / text editor
            let textEditor = app.textViews.firstMatch
            if textEditor.waitForExistence(timeout: 3) {
                textEditor.tap()
                sleep(2)
                addScreenshot(name: "03_after_textview_tap")
                dumpButtons(label: "after_textview_tap")
            }
        }

        // === VERIFY: [/] button EXISTS in toolbar ===
        // Check by label, identifier, or accessible name
        let slashByLabel = app.buttons.matching(
            NSPredicate(format: "label CONTAINS[cd] '/' OR label CONTAINS[cd] 'slash' OR label == '/'")
        ).firstMatch
        let slashByIdentifier = app.buttons.matching(
            NSPredicate(format: "identifier CONTAINS[cd] 'slash' OR identifier CONTAINS[cd] 'nudge' OR identifier CONTAINS[cd] 'skill' OR identifier CONTAINS[cd] 'commands'")
        ).firstMatch

        let slashExists = slashByLabel.waitForExistence(timeout: 3) || slashByIdentifier.waitForExistence(timeout: 1)

        addScreenshot(name: "04_toolbar_state")

        if slashExists {
            let slashButton = slashByLabel.exists ? slashByLabel : slashByIdentifier
            print("✅ [/] button found! Label: '\(slashButton.label)' ID: '\(slashButton.identifier)'")

            // Tap [/] and verify sheet opens
            slashButton.tap()
            sleep(2)
            addScreenshot(name: "05_nudge_skills_sheet")

            let sheetAppeared = app.sheets.firstMatch.waitForExistence(timeout: 3) ||
                                app.otherElements["nudge-skill-sheet"].waitForExistence(timeout: 1) ||
                                app.scrollViews.count > 0
            XCTAssertTrue(sheetAppeared, "❌ Nudge-Skills sheet should appear after tapping [/]")
            if sheetAppeared {
                print("✅ Nudge-Skills sheet appeared after tapping [/]")
            }
        } else {
            // Final diagnostic: collect all buttons for report
            var buttonLabels: [String] = []
            for btn in app.buttons.allElementsBoundByIndex {
                buttonLabels.append("'\(btn.label)'(id:\(btn.identifier))")
            }
            XCTFail("❌ [/] button not found in toolbar after activating compose mode.\n\nAll buttons:\n\(buttonLabels.joined(separator: "\n"))")
        }
    }
}
