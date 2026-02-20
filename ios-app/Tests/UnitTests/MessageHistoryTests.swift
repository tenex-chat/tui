import XCTest
@testable import TenexMVP

@MainActor
final class MessageHistoryTests: XCTestCase {

    // MARK: - Add

    func testAddStoresMessage() {
        let history = MessageHistory()
        history.add("Hello")
        history.add("World")
        XCTAssertEqual(history.messages, ["Hello", "World"])
    }

    func testAddIgnoresEmptyAndWhitespace() {
        let history = MessageHistory()
        history.add("")
        history.add("   ")
        history.add("\n\t")
        XCTAssertTrue(history.messages.isEmpty)
    }

    func testAddTrimsWhitespace() {
        let history = MessageHistory()
        history.add("  hello  ")
        XCTAssertEqual(history.messages, ["hello"])
    }

    // MARK: - Previous

    func testPreviousEmptyHistoryReturnsNil() {
        let history = MessageHistory()
        XCTAssertNil(history.previous(currentText: "typing"))
        XCTAssertNil(history.currentIndex)
    }

    func testPreviousCyclesBackward() {
        let history = MessageHistory()
        history.add("first")
        history.add("second")
        history.add("third")

        let p1 = history.previous(currentText: "draft")
        XCTAssertEqual(p1, "third")
        XCTAssertEqual(history.currentIndex, 2)

        let p2 = history.previous(currentText: "draft")
        XCTAssertEqual(p2, "second")
        XCTAssertEqual(history.currentIndex, 1)

        let p3 = history.previous(currentText: "draft")
        XCTAssertEqual(p3, "first")
        XCTAssertEqual(history.currentIndex, 0)

        // At oldest, stays there
        let p4 = history.previous(currentText: "draft")
        XCTAssertEqual(p4, "first")
        XCTAssertEqual(history.currentIndex, 0)
    }

    func testPreviousStashesCurrentText() {
        let history = MessageHistory()
        history.add("sent msg")

        _ = history.previous(currentText: "my unsaved typing")
        XCTAssertEqual(history.savedDraft, "my unsaved typing")
    }

    // MARK: - Next

    func testNextWithoutPreviousReturnsNil() {
        let history = MessageHistory()
        history.add("msg")
        XCTAssertNil(history.next())
    }

    func testNextCyclesForward() {
        let history = MessageHistory()
        history.add("first")
        history.add("second")
        history.add("third")

        // Go back to oldest
        _ = history.previous(currentText: "draft")
        _ = history.previous(currentText: "draft")
        _ = history.previous(currentText: "draft")
        XCTAssertEqual(history.currentIndex, 0)

        let n1 = history.next()
        XCTAssertEqual(n1, "second")
        XCTAssertEqual(history.currentIndex, 1)

        let n2 = history.next()
        XCTAssertEqual(n2, "third")
        XCTAssertEqual(history.currentIndex, 2)
    }

    func testNextPastNewestRestoresSavedDraft() {
        let history = MessageHistory()
        history.add("sent")

        _ = history.previous(currentText: "my typing")
        XCTAssertEqual(history.currentIndex, 0)

        let restored = history.next()
        XCTAssertEqual(restored, "my typing")
        XCTAssertNil(history.currentIndex)
        XCTAssertEqual(history.savedDraft, "")
    }

    // MARK: - Reset

    func testResetClearsState() {
        let history = MessageHistory()
        history.add("msg")
        _ = history.previous(currentText: "draft text")
        XCTAssertNotNil(history.currentIndex)
        XCTAssertEqual(history.savedDraft, "draft text")

        history.reset()

        XCTAssertNil(history.currentIndex)
        XCTAssertEqual(history.savedDraft, "")
    }

    // MARK: - Full Cycle

    func testFullUpDownCycle() {
        let history = MessageHistory()
        history.add("alpha")
        history.add("beta")

        // Go up twice (to oldest)
        _ = history.previous(currentText: "current")
        _ = history.previous(currentText: "current")
        XCTAssertEqual(history.currentIndex, 0)

        // Go down twice to restore draft
        let n1 = history.next()
        XCTAssertEqual(n1, "beta")

        let n2 = history.next()
        XCTAssertEqual(n2, "current")
        XCTAssertNil(history.currentIndex)
    }

    func testSingleMessageUpDown() {
        let history = MessageHistory()
        history.add("only one")

        let up = history.previous(currentText: "typing...")
        XCTAssertEqual(up, "only one")

        let down = history.next()
        XCTAssertEqual(down, "typing...")
        XCTAssertNil(history.currentIndex)
    }
}
