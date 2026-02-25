import XCTest
@testable import TenexMVP

final class PinnedPromptTests: XCTestCase {

    // MARK: - PinnedPrompt Model

    func testPinnedPromptCodableRoundTrip() throws {
        let original = PinnedPrompt(title: "Release Notes", text: "Summarize changes from today.")

        let encoder = JSONEncoder()
        encoder.outputFormatting = .prettyPrinted
        let data = try encoder.encode(original)
        let decoded = try JSONDecoder().decode(PinnedPrompt.self, from: data)

        XCTAssertEqual(decoded.id, original.id)
        XCTAssertEqual(decoded.title, original.title)
        XCTAssertEqual(decoded.text, original.text)
        XCTAssertEqual(decoded.createdAt, original.createdAt)
        XCTAssertEqual(decoded.lastModified, original.lastModified)
        XCTAssertEqual(decoded.lastUsedAt, original.lastUsedAt)
    }

    func testPinnedPromptValidationTrimsInputs() {
        let normalized = PinnedPrompt.normalized(
            title: "  Weekly planning  ",
            text: "\n  Build a weekly project plan.  \n"
        )

        XCTAssertEqual(normalized?.title, "Weekly planning")
        XCTAssertEqual(normalized?.text, "Build a weekly project plan.")
    }

    func testPinnedPromptValidationRejectsEmptyValues() {
        XCTAssertNil(PinnedPrompt.normalized(title: "   ", text: "content"))
        XCTAssertNil(PinnedPrompt.normalized(title: "Title", text: "  \n  "))
    }

    func testPinnedPromptSortComparatorUsesLastUsedThenLastModified() {
        let base = Date(timeIntervalSince1970: 1_000)
        let oldest = PinnedPrompt(
            id: "old",
            title: "Old",
            text: "text",
            createdAt: base,
            lastModified: base.addingTimeInterval(1),
            lastUsedAt: base.addingTimeInterval(1)
        )
        let newerModified = PinnedPrompt(
            id: "mod",
            title: "Mod",
            text: "text",
            createdAt: base,
            lastModified: base.addingTimeInterval(20),
            lastUsedAt: base.addingTimeInterval(5)
        )
        let newestUsed = PinnedPrompt(
            id: "new",
            title: "New",
            text: "text",
            createdAt: base,
            lastModified: base.addingTimeInterval(2),
            lastUsedAt: base.addingTimeInterval(50)
        )

        let sorted = [oldest, newestUsed, newerModified].sorted(by: PinnedPrompt.sortComparator(_:_:))
        XCTAssertEqual(sorted.map(\.id), ["new", "mod", "old"])
    }

    // MARK: - Store

    func testStoreLoadFromEmptyDirectoryReturnsEmptyPrompts() async {
        await cleanPinnedPromptArtifacts()
        let store = PinnedPromptStore()

        let result = await store.load()

        XCTAssertTrue(result.prompts.isEmpty)
        XCTAssertFalse(result.loadFailed)
    }

    func testStoreSaveAndLoadRoundTrips() async throws {
        await cleanPinnedPromptArtifacts()
        let store = PinnedPromptStore()
        let prompts = [
            PinnedPrompt(title: "Prompt A", text: "A text"),
            PinnedPrompt(title: "Prompt B", text: "B text"),
        ]

        try await store.save(prompts, allowSave: true)
        let result = await store.load()

        XCTAssertFalse(result.loadFailed)
        XCTAssertEqual(result.prompts.count, 2)
        XCTAssertEqual(result.prompts[0].title, "Prompt A")
        XCTAssertEqual(result.prompts[1].title, "Prompt B")
    }

    func testStoreLoadCorruptedFileQuarantines() async {
        await cleanPinnedPromptArtifacts()
        let documentsDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let fileURL = documentsDir.appendingPathComponent("pinned_prompts.json")
        let corruptedData = Data("not valid json{{{".utf8)
        try? corruptedData.write(to: fileURL, options: .atomic)

        let store = PinnedPromptStore()
        let result = await store.load()

        XCTAssertTrue(result.loadFailed)
        XCTAssertTrue(result.prompts.isEmpty)
        XCTAssertFalse(FileManager.default.fileExists(atPath: fileURL.path))
    }

    // MARK: - Manager

    @MainActor
    func testManagerDeleteRemovesPrompt() async {
        await cleanPinnedPromptArtifacts()
        let manager = PinnedPromptManager.shared
        await clearManager(manager)

        let a = await manager.pin(title: "Prompt A", text: "First")
        _ = await manager.pin(title: "Prompt B", text: "Second")
        await manager.delete(a.id)

        XCTAssertFalse(manager.all().contains(where: { $0.id == a.id }))
    }

    @MainActor
    func testManagerMarkUsedMovesPromptToTop() async {
        await cleanPinnedPromptArtifacts()
        let manager = PinnedPromptManager.shared
        await clearManager(manager)

        let first = await manager.pin(title: "First", text: "First text")
        try? await Task.sleep(for: .milliseconds(10))
        let second = await manager.pin(title: "Second", text: "Second text")

        XCTAssertEqual(manager.all().first?.id, second.id)

        await manager.markUsed(first.id)
        XCTAssertEqual(manager.all().first?.id, first.id)
    }

    // MARK: - Helpers

    @MainActor
    private func clearManager(_ manager: PinnedPromptManager) async {
        let ids = manager.all().map(\.id)
        for id in ids {
            await manager.delete(id)
        }
    }

    private func cleanPinnedPromptArtifacts() async {
        let documentsDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let fileURL = documentsDir.appendingPathComponent("pinned_prompts.json")
        try? FileManager.default.removeItem(at: fileURL)

        let contents = try? FileManager.default.contentsOfDirectory(at: documentsDir, includingPropertiesForKeys: nil)
        for url in contents ?? [] where url.lastPathComponent.contains("pinned_prompts.corrupted-") {
            try? FileManager.default.removeItem(at: url)
        }
    }
}
