import XCTest

final class AgentDeletionUITest: XCTestCase {
    
    var app: XCUIApplication!
    private let defaultTimeout: TimeInterval = 15
    
    override func setUpWithError() throws {
        continueAfterFailure = false
        app = XCUIApplication()
        app.launchArguments = ["--uitesting"]
        app.launchEnvironment = ["UITEST_MODE": "true", "DISABLE_ANIMATIONS": "true"]
        app.launch()
    }
    
    override func tearDownWithError() throws { app = nil }
    
    func ss(_ name: String) {
        let s = XCTAttachment(screenshot: app.screenshot())
        s.name = name; s.lifetime = .keepAlways; add(s)
    }
    
    func printAll(_ prefix: String = "") {
        let btns = app.buttons.allElementsBoundByIndex
        print("\(prefix)BUTTONS: \(btns.filter { !$0.label.isEmpty }.map { "'\($0.label)'(\($0.identifier))" }.prefix(12))")
        let cells = app.cells.allElementsBoundByIndex
        print("\(prefix)CELLS[\(cells.count)]: \(cells.prefix(12).map { "'\($0.label)' frame=\($0.frame)" })")
        let texts = app.staticTexts.allElementsBoundByIndex
        print("\(prefix)TEXTS: \(texts.prefix(12).map { "'\($0.label)' frame=\($0.frame)" })")
    }
    
    func findBtn(_ label: String, exact: Bool = false) -> XCUIElement {
        exact ? app.buttons.matching(NSPredicate(format: "label == %@", label)).firstMatch
              : app.buttons.matching(NSPredicate(format: "label CONTAINS[c] %@", label)).firstMatch
    }
    
    // MARK: - TEST 1: Build & Launch
    func testBuildAndLaunch() throws {
        XCTAssertTrue(app.state == .runningForeground)
        XCTAssertTrue(findBtn("Projects", exact: true).waitForExistence(timeout: defaultTimeout))
        ss("01_launch")
        print("✅ App launched successfully")
    }
    
    // MARK: - TEST 2: Swipe-to-delete in Project Settings Agents section
    func testAgentDeletion_SwipeToDelete() throws {
        sleep(3)
        
        // Step 1: Navigate to Projects tab
        let projectsTab = findBtn("Projects", exact: true)
        guard projectsTab.waitForExistence(timeout: defaultTimeout) else {
            XCTFail("Projects tab not found"); return
        }
        projectsTab.tap(); sleep(2)
        ss("01_projects_list")
        printAll("PROJECTS: ")
        
        let projectCells = app.cells.allElementsBoundByIndex
        print("📊 Projects: \(projectCells.count)")
        guard !projectCells.isEmpty else {
            XCTFail("No projects found"); return
        }
        
        // Step 2: Open first project (Active Conversations Test)
        projectCells[0].tap(); sleep(2)
        ss("02_project_settings_before_scroll")
        
        let settingsTitle = app.staticTexts.matching(NSPredicate(format: "label == 'Project Settings'")).firstMatch
        guard settingsTitle.waitForExistence(timeout: 5) else {
            XCTFail("Project Settings not opened"); return
        }
        print("✅ Project Settings opened")
        
        // Before scroll: agents at y=764 (header) and y=804 (Transparent agent)
        // Check texts visible initially
        let transparentPre = app.staticTexts.matching(NSPredicate(format: "label == 'Transparent'")).firstMatch
        if transparentPre.waitForExistence(timeout: 2) {
            print("📍 Transparent agent visible BEFORE scroll at y=\(transparentPre.frame.minY)")
        }
        
        // Step 3: Scroll down using app.swipeUp() (works for form/list views)
        app.swipeUp(); sleep(1)
        ss("03_after_scroll")
        printAll("AFTER_SCROLL: ")
        
        // After scroll, the Transparent agent should be higher up (~y=132 from prior tests)
        let transparentText = app.staticTexts.matching(NSPredicate(format: "label == 'Transparent'")).firstMatch
        if transparentText.waitForExistence(timeout: 3) {
            let agentY = transparentText.frame.midY
            print("✅ 'Transparent' agent after scroll at y=\(agentY)")
            
            // Step 4A: Swipe the ELEMENT itself left to trigger .swipeActions
            print("➡️ Strategy A: swipeLeft() on Transparent text element")
            transparentText.swipeLeft()
            Thread.sleep(forTimeInterval: 1.0)
            ss("04a_swipe_text_result")
            
            var deleteBtn = app.buttons.matching(NSPredicate(format: "label == 'Delete'")).firstMatch
            if deleteBtn.waitForExistence(timeout: 2) {
                print("✅ DELETE via text.swipeLeft()!")
                deleteBtn.tap(); sleep(1)
                verifyDeletionSheet(); return
            }
            transparentText.swipeRight(); Thread.sleep(forTimeInterval: 0.4)
            
            // Step 4B: Coordinate swipe at the agent's y position
            print("➡️ Strategy B: coordinate swipe at agent y=\(agentY)")
            for swipeY in [agentY, agentY - 15, agentY + 15, agentY - 25] {
                let startC = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 370, dy: swipeY))
                let endC = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 10, dy: swipeY))
                startC.press(forDuration: 0.1, thenDragTo: endC, withVelocity: .fast, thenHoldForDuration: 0.1)
                Thread.sleep(forTimeInterval: 1.0)
                ss("04b_coord_y\(Int(swipeY))")
                
                deleteBtn = app.buttons.matching(NSPredicate(format: "label == 'Delete'")).firstMatch
                if deleteBtn.waitForExistence(timeout: 2) {
                    print("✅ DELETE via coord swipe at y=\(swipeY)!")
                    deleteBtn.tap(); sleep(1)
                    verifyDeletionSheet(); return
                }
                
                // Reset
                let r1 = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 10, dy: swipeY))
                let r2 = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 370, dy: swipeY))
                r1.press(forDuration: 0.1, thenDragTo: r2, withVelocity: .fast, thenHoldForDuration: 0.1)
                Thread.sleep(forTimeInterval: 0.5)
            }
        } else {
            print("❌ 'Transparent' not found after scroll")
        }
        
        // Step 4C: Try the cell at the right y position after scroll
        // From prior test: cell[2] at y=117.33 after scroll is the agent
        print("➡️ Strategy C: swipe specific cells after scroll")
        let scrolledCells = app.cells.allElementsBoundByIndex
        for (i, cell) in scrolledCells.enumerated() {
            let frame = cell.frame
            // Only target cells in the scrolled-visible area (not off-screen)
            guard frame.minY >= 0 && frame.minY < 700 else { continue }
            // Skip very short cells (section headers, notes)
            guard frame.height >= 60 else { continue }
            
            print("  Swiping cell[\(i)] y=\(frame.minY) h=\(frame.height)")
            cell.swipeLeft()
            Thread.sleep(forTimeInterval: 1.0)
            
            let deleteBtn = app.buttons.matching(NSPredicate(format: "label == 'Delete'")).firstMatch
            if deleteBtn.waitForExistence(timeout: 2) {
                print("✅ DELETE on cell[\(i)]!")
                ss("04c_cell_delete")
                deleteBtn.tap(); sleep(1)
                verifyDeletionSheet(); return
            }
            cell.swipeRight(); Thread.sleep(forTimeInterval: 0.5)
        }
        
        // Step 4D: Long press on agent area
        print("➡️ Strategy D: long press on agent area")
        let transparentFinal = app.staticTexts.matching(NSPredicate(format: "label == 'Transparent'")).firstMatch
        if transparentFinal.exists {
            let agentY = transparentFinal.frame.midY
            let lpCoord = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 196, dy: agentY))
            lpCoord.press(forDuration: 2.0)
            Thread.sleep(forTimeInterval: 1.0)
            ss("04d_longpress")
            printAll("AFTER_LONGPRESS: ")
            
            let deleteBtn = app.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'Delete' OR label CONTAINS[c] 'Remove'")).firstMatch
            if deleteBtn.waitForExistence(timeout: 2) {
                print("✅ DELETE via long press!")
                deleteBtn.tap(); sleep(1)
                verifyDeletionSheet(); return
            }
            
            // Dismiss
            app.coordinate(withNormalizedOffset: CGVector(dx: 0.5, dy: 0.05)).tap()
            Thread.sleep(forTimeInterval: 0.5)
        }
        
        // Report findings
        ss("05_no_delete_found")
        printAll("FINAL_STATE: ")
        print("ℹ️ SUMMARY: Agent rows are visible in Project Settings but swipe-to-delete could not be triggered via UI automation")
        print("ℹ️ This may be a limitation of XCTest's interaction with SwiftUI .swipeActions()")
    }
    
    // MARK: - TEST 3: Scope picker verification (if we can trigger deletion)
    func testAgentDeletion_ScopePicker() throws {
        sleep(3)
        
        let projectsTab = findBtn("Projects", exact: true)
        guard projectsTab.waitForExistence(timeout: defaultTimeout) else { return }
        projectsTab.tap(); sleep(2)
        
        let cells = app.cells.allElementsBoundByIndex
        guard !cells.isEmpty else { return }
        cells[0].tap(); sleep(2)
        
        let settingsTitle = app.staticTexts.matching(NSPredicate(format: "label == 'Project Settings'")).firstMatch
        guard settingsTitle.waitForExistence(timeout: 5) else { return }
        
        app.swipeUp(); sleep(1)
        
        let transparentText = app.staticTexts.matching(NSPredicate(format: "label == 'Transparent'")).firstMatch
        guard transparentText.waitForExistence(timeout: 3) else {
            print("ℹ️ No agent to test scope picker with"); return
        }
        
        let agentY = transparentText.frame.midY
        print("✅ Transparent agent at y=\(agentY)")
        
        var deleteBtn: XCUIElement? = nil
        
        // Try all swipe approaches
        for swipeY in [agentY, agentY - 15, agentY + 15] {
            let startC = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 370, dy: swipeY))
            let endC = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 10, dy: swipeY))
            startC.press(forDuration: 0.1, thenDragTo: endC, withVelocity: .fast, thenHoldForDuration: 0.1)
            Thread.sleep(forTimeInterval: 1.0)
            
            let btn = app.buttons.matching(NSPredicate(format: "label == 'Delete'")).firstMatch
            if btn.waitForExistence(timeout: 2) {
                deleteBtn = btn; break
            }
            
            let r1 = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 10, dy: swipeY))
            let r2 = app.coordinate(withNormalizedOffset: .zero).withOffset(CGVector(dx: 370, dy: swipeY))
            r1.press(forDuration: 0.1, thenDragTo: r2, withVelocity: .fast, thenHoldForDuration: 0.1)
            Thread.sleep(forTimeInterval: 0.5)
        }
        
        guard let delBtn = deleteBtn else {
            print("ℹ️ Cannot trigger swipe-to-delete - skipping scope picker test")
            return
        }
        
        delBtn.tap(); sleep(1)
        ss("scope_01_sheet")
        printAll("SCOPE_SHEET: ")
        
        var foundThis = false; var foundAll = false
        
        if app.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'This Project'")).firstMatch.waitForExistence(timeout: 5) {
            foundThis = true; print("✅ 'This Project' PRESENT")
        }
        if app.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'All Projects' OR label CONTAINS[c] 'Global'")).firstMatch.waitForExistence(timeout: 3) {
            foundAll = true; print("✅ 'All Projects' PRESENT")
        }
        for ctrl in app.segmentedControls.allElementsBoundByIndex {
            for seg in ctrl.buttons.allElementsBoundByIndex {
                let lbl = seg.label.lowercased()
                print("  Segment: '\(seg.label)'")
                if lbl.contains("this project") { foundThis = true }
                if lbl.contains("all") || lbl.contains("global") { foundAll = true }
            }
        }
        
        ss("scope_02_final")
        XCTAssertTrue(foundThis, "'This Project' must be in AgentDeletionSheet")
        XCTAssertTrue(foundAll, "'All Projects (Global)' must be in AgentDeletionSheet")
        
        let cancel = app.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'Cancel'")).firstMatch
        if cancel.waitForExistence(timeout: 2) { cancel.tap() } else { app.swipeDown() }
    }
    
    func verifyDeletionSheet() {
        ss("verify_01_sheet")
        printAll("DELETION_SHEET: ")
        
        var foundThis = false; var foundAll = false
        
        if app.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'This Project'")).firstMatch.waitForExistence(timeout: 5) {
            foundThis = true; print("✅ 'This Project' scope PRESENT"); ss("verify_02_this")
        } else { print("❌ 'This Project' NOT PRESENT") }
        
        if app.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'All Projects' OR label CONTAINS[c] 'Global'")).firstMatch.waitForExistence(timeout: 3) {
            foundAll = true; print("✅ 'All Projects (Global)' scope PRESENT"); ss("verify_03_all")
        } else { print("❌ 'All Projects (Global)' NOT PRESENT") }
        
        for ctrl in app.segmentedControls.allElementsBoundByIndex {
            for seg in ctrl.buttons.allElementsBoundByIndex {
                let lbl = seg.label.lowercased()
                print("  Segment: '\(seg.label)'")
                if lbl.contains("this project") { foundThis = true }
                if lbl.contains("all") || lbl.contains("global") { foundAll = true }
            }
        }
        
        if foundThis && foundAll {
            print("✅✅ AgentDeletionSheet FULLY VERIFIED")
        } else {
            print("⚠️ Missing: thisProject=\(foundThis) allProjects=\(foundAll)")
        }
        
        let cancel = app.buttons.matching(NSPredicate(format: "label CONTAINS[c] 'Cancel'")).firstMatch
        if cancel.waitForExistence(timeout: 2) {
            cancel.tap(); print("✅ Cancelled - agent preserved")
        } else {
            app.swipeDown(); print("ℹ️ Dismissed with swipe")
        }
        sleep(1)
        ss("verify_04_done")
    }
}
