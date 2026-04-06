import XCTest

final class ToolbarDiagnosticTest: XCTestCase {
    let app = XCUIApplication()
    
    override func setUpWithError() throws {
        continueAfterFailure = false
        app.launch()
    }
    
    override func tearDownWithError() throws {
        app.terminate()
    }
    
    func testDiagnoseToolbarElements() throws {
        // App should already be logged in from keychain
        sleep(3)
        
        // Navigate to Chats tab
        let chatsTab = app.tabBars.buttons["Chats"]
        if chatsTab.waitForExistence(timeout: 10) { chatsTab.tap() }
        
        let firstCell = app.cells.firstMatch
        guard firstCell.waitForExistence(timeout: 10) else {
            XCTFail("No conversations found")
            return
        }
        firstCell.tap()
        sleep(3)
        
        // Take full screenshot
        let shot1 = XCUIScreen.main.screenshot()
        let att1 = XCTAttachment(screenshot: shot1)
        att1.name = "01_conversation_view"
        att1.lifetime = .keepAlways
        add(att1)
        
        // Dump ALL elements with labels, identifiers, and types
        var report = "=== ALL ELEMENTS IN CONVERSATION VIEW ===\n"
        
        // All buttons
        report += "\n--- BUTTONS (\(app.buttons.count)) ---\n"
        for btn in app.buttons.allElementsBoundByIndex {
            report += "  Label: '\(btn.label)' | ID: '\(btn.identifier)' | Frame: \(btn.frame)\n"
        }
        
        // All static texts
        report += "\n--- STATIC TEXTS (first 20) ---\n"
        var count = 0
        for txt in app.staticTexts.allElementsBoundByIndex {
            if count > 20 { break }
            report += "  '\(txt.label)'\n"
            count += 1
        }
        
        // All images (SF Symbols show as images)
        report += "\n--- IMAGES (\(app.images.count)) ---\n"
        for img in app.images.allElementsBoundByIndex {
            report += "  Label: '\(img.label)' | ID: '\(img.identifier)'\n"
        }
        
        // Any element with slash, nudge, or skill
        report += "\n--- SEARCHING FOR slash/nudge/skill ---\n"
        let slashPred = NSPredicate(format: "label CONTAINS[cd] 'slash' OR identifier CONTAINS[cd] 'slash' OR label == '/' OR label CONTAINS[cd] 'nudge' OR label CONTAINS[cd] 'skill'")
        let found = app.descendants(matching: .any).matching(slashPred)
        report += "  Count: \(found.count)\n"
        for el in found.allElementsBoundByIndex {
            report += "  Type: \(el.elementType) | Label: '\(el.label)' | ID: '\(el.identifier)'\n"
        }
        
        print(report)
        
        // Save report as attachment
        let reportData = report.data(using: .utf8)!
        let reportAtt = XCTAttachment(data: reportData, uniformTypeIdentifier: "public.plain-text")
        reportAtt.name = "element_report"
        reportAtt.lifetime = .keepAlways
        add(reportAtt)
        
        // Scroll down to see if toolbar is lower
        app.swipeUp()
        sleep(1)
        let shot2 = XCUIScreen.main.screenshot()
        let att2 = XCTAttachment(screenshot: shot2)
        att2.name = "02_after_swipe_up"
        att2.lifetime = .keepAlways
        add(att2)
        
        // Dump buttons again after scroll
        var report2 = "=== BUTTONS AFTER SWIPE UP ===\n"
        for btn in app.buttons.allElementsBoundByIndex {
            report2 += "  Label: '\(btn.label)' | ID: '\(btn.identifier)' | Frame: \(btn.frame)\n"
        }
        print(report2)
        
        let report2Att = XCTAttachment(data: report2.data(using: .utf8)!, uniformTypeIdentifier: "public.plain-text")
        report2Att.name = "buttons_after_swipe"
        report2Att.lifetime = .keepAlways
        add(report2Att)
        
        // Always pass - this is just diagnostic
        XCTAssertTrue(true, "Diagnostic complete")
    }
}
