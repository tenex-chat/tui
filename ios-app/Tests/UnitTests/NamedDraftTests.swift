import XCTest
@testable import TenexMVP

final class NamedDraftTests: XCTestCase {

    // MARK: - deriveName

    func testDeriveNameFromNormalText() {
        XCTAssertEqual(NamedDraft.deriveName(from: "Hello world"), "Hello world")
    }

    func testDeriveNameTruncatesAt50Chars() {
        let longText = String(repeating: "a", count: 60)
        let result = NamedDraft.deriveName(from: longText)
        XCTAssertTrue(result.hasPrefix(String(repeating: "a", count: 50)))
        XCTAssertTrue(result.hasSuffix("..."))
    }

    func testDeriveNameExactly50CharsNotTruncated() {
        let text = String(repeating: "b", count: 50)
        XCTAssertEqual(NamedDraft.deriveName(from: text), text)
    }

    func testDeriveNameUsesFirstLineOnly() {
        let text = "First line\nSecond line\nThird line"
        XCTAssertEqual(NamedDraft.deriveName(from: text), "First line")
    }

    func testDeriveNameTrimsWhitespace() {
        XCTAssertEqual(NamedDraft.deriveName(from: "  spaced  "), "spaced")
    }

    func testDeriveNameEmptyTextReturnsUntitled() {
        XCTAssertEqual(NamedDraft.deriveName(from: ""), "Untitled")
    }

    func testDeriveNameWhitespaceOnlyReturnsUntitled() {
        XCTAssertEqual(NamedDraft.deriveName(from: "   \n\n  "), "Untitled")
    }

    // MARK: - preview

    func testPreviewShortText() {
        let draft = NamedDraft(text: "Short text", projectId: "p1")
        XCTAssertEqual(draft.preview, "Short text")
    }

    func testPreviewReplacesNewlines() {
        let draft = NamedDraft(text: "Line1\nLine2\nLine3", projectId: "p1")
        XCTAssertEqual(draft.preview, "Line1 Line2 Line3")
    }

    func testPreviewTruncatesAt100Chars() {
        let longText = String(repeating: "x", count: 120)
        let draft = NamedDraft(text: longText, projectId: "p1")
        XCTAssertEqual(draft.preview.count, 103) // 100 + "..."
        XCTAssertTrue(draft.preview.hasSuffix("..."))
    }

    func testPreviewExactly100CharsNotTruncated() {
        let text = String(repeating: "y", count: 100)
        let draft = NamedDraft(text: text, projectId: "p1")
        XCTAssertEqual(draft.preview, text)
    }

    // MARK: - updateText

    func testUpdateTextChangesTextNameAndTimestamp() {
        var draft = NamedDraft(text: "Original", projectId: "p1")
        let originalModified = draft.lastModified

        Thread.sleep(forTimeInterval: 0.01)
        draft.updateText("New first line\nMore content")

        XCTAssertEqual(draft.text, "New first line\nMore content")
        XCTAssertEqual(draft.name, "New first line")
        XCTAssertGreaterThan(draft.lastModified, originalModified)
    }

    // MARK: - Initialization

    func testInitSetsAllFields() {
        let draft = NamedDraft(text: "Test content", projectId: "proj-abc")

        XCTAssertFalse(draft.id.isEmpty)
        XCTAssertEqual(draft.name, "Test content")
        XCTAssertEqual(draft.text, "Test content")
        XCTAssertEqual(draft.projectId, "proj-abc")
        XCTAssertNotNil(draft.createdAt)
        XCTAssertNotNil(draft.lastModified)
    }

    // MARK: - Codable Round-Trip

    func testCodableRoundTrip() throws {
        let original = NamedDraft(text: "Hello world\nSecond line", projectId: "proj-1")

        let encoder = JSONEncoder()
        encoder.outputFormatting = .prettyPrinted
        let data = try encoder.encode(original)
        let decoded = try JSONDecoder().decode(NamedDraft.self, from: data)

        XCTAssertEqual(decoded.id, original.id)
        XCTAssertEqual(decoded.name, original.name)
        XCTAssertEqual(decoded.text, original.text)
        XCTAssertEqual(decoded.projectId, original.projectId)
    }

    func testArrayCodableRoundTrip() throws {
        let drafts = [
            NamedDraft(text: "First", projectId: "p1"),
            NamedDraft(text: "Second", projectId: "p2"),
        ]

        let encoder = JSONEncoder()
        let data = try encoder.encode(drafts)
        let decoded = try JSONDecoder().decode([NamedDraft].self, from: data)

        XCTAssertEqual(decoded.count, 2)
        XCTAssertEqual(decoded[0].text, "First")
        XCTAssertEqual(decoded[1].text, "Second")
    }

    // MARK: - Equatable

    func testEquatableMatchesSameInstance() {
        let draft = NamedDraft(text: "Test", projectId: "p1")
        XCTAssertEqual(draft, draft)
    }

    func testEquatableDifferentInstancesNotEqual() {
        let a = NamedDraft(text: "Test", projectId: "p1")
        let b = NamedDraft(text: "Test", projectId: "p1")
        // Different UUIDs
        XCTAssertNotEqual(a, b)
    }

    // MARK: - NamedDraftStore

    func testStoreLoadFromEmptyDirectoryReturnsEmptyDrafts() async {
        let store = NamedDraftStore()
        let documentsDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let fileURL = documentsDir.appendingPathComponent("named_drafts.json")
        try? FileManager.default.removeItem(at: fileURL)

        let result = await store.load()

        XCTAssertTrue(result.drafts.isEmpty)
        XCTAssertFalse(result.loadFailed)
    }

    func testStoreSaveAndLoadRoundTrips() async throws {
        let store = NamedDraftStore()
        let drafts = [
            NamedDraft(text: "Draft one", projectId: "proj-a"),
            NamedDraft(text: "Draft two", projectId: "proj-b"),
        ]

        try await store.save(drafts, allowSave: true)
        let result = await store.load()

        XCTAssertFalse(result.loadFailed)
        XCTAssertEqual(result.drafts.count, 2)
        XCTAssertEqual(result.drafts[0].text, "Draft one")
        XCTAssertEqual(result.drafts[1].text, "Draft two")

        // Clean up
        let documentsDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let fileURL = documentsDir.appendingPathComponent("named_drafts.json")
        try? FileManager.default.removeItem(at: fileURL)
    }

    func testStoreSaveForbiddenWhenNotAllowed() async {
        let store = NamedDraftStore()
        do {
            try await store.save([], allowSave: false)
            XCTFail("Expected error")
        } catch {
            XCTAssertTrue(error.localizedDescription.contains("Save forbidden"))
        }
    }

    func testStoreLoadCorruptedFileQuarantines() async {
        let documentsDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let fileURL = documentsDir.appendingPathComponent("named_drafts.json")

        let corruptedData = Data("not valid json{{{".utf8)
        try? corruptedData.write(to: fileURL, options: .atomic)

        let store = NamedDraftStore()
        let result = await store.load()

        XCTAssertTrue(result.loadFailed)
        XCTAssertTrue(result.drafts.isEmpty)
        XCTAssertFalse(FileManager.default.fileExists(atPath: fileURL.path))

        // Clean up quarantined files
        let contents = try? FileManager.default.contentsOfDirectory(at: documentsDir, includingPropertiesForKeys: nil)
        for url in contents ?? [] {
            if url.lastPathComponent.contains("named_drafts.corrupted-") {
                try? FileManager.default.removeItem(at: url)
            }
        }
    }
}
