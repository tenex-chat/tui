import XCTest
@testable import TenexMVP

// MARK: - Draft Codable & Migration Tests

/// Tests for Draft encoding/decoding round-trips and migration from older JSON formats.
/// These are NOT covered in DraftModelTests, which only tests struct mutations and computed properties.
final class DraftManagerTests: XCTestCase {

    private let encoder: JSONEncoder = {
        let e = JSONEncoder()
        e.outputFormatting = .prettyPrinted
        return e
    }()
    private let decoder = JSONDecoder()

    // MARK: - Round-Trip Encoding/Decoding

    func testNewConversationDraftRoundTrips() throws {
        var original = Draft(
            projectId: "proj-1",
            title: "My Title",
            content: "Hello world",
            agentPubkey: "agent-abc",
            selectedNudgeIds: ["n1", "n2"],
            selectedSkillIds: ["s1"],
            referenceConversationId: "conv-ref",
            referenceReportATag: "30023:pubkey:slug"
        )
        _ = original.addImageAttachment(url: "https://example.com/img1.png")
        _ = original.addImageAttachment(url: "https://example.com/img2.png")

        let data = try encoder.encode(original)
        let decoded = try decoder.decode(Draft.self, from: data)

        XCTAssertEqual(decoded.id, original.id)
        XCTAssertNil(decoded.conversationId)
        XCTAssertEqual(decoded.projectId, "proj-1")
        XCTAssertEqual(decoded.title, "My Title")
        XCTAssertEqual(decoded.content, "Hello world")
        XCTAssertEqual(decoded.agentPubkey, "agent-abc")
        XCTAssertEqual(decoded.selectedNudgeIds, ["n1", "n2"])
        XCTAssertEqual(decoded.selectedSkillIds, ["s1"])
        XCTAssertTrue(decoded.isNewConversation)
        XCTAssertEqual(decoded.referenceConversationId, "conv-ref")
        XCTAssertEqual(decoded.referenceReportATag, "30023:pubkey:slug")
        XCTAssertEqual(decoded.imageAttachments.count, 2)
        XCTAssertEqual(decoded.imageAttachments[0].url, "https://example.com/img1.png")
        XCTAssertEqual(decoded.imageAttachments[1].url, "https://example.com/img2.png")
    }

    func testExistingConversationDraftRoundTrips() throws {
        let original = Draft(
            conversationId: "conv-42",
            projectId: "proj-2",
            content: "Reply text",
            agentPubkey: nil,
            selectedNudgeIds: [],
            selectedSkillIds: ["s1", "s2"]
        )

        let data = try encoder.encode(original)
        let decoded = try decoder.decode(Draft.self, from: data)

        XCTAssertEqual(decoded.id, original.id)
        XCTAssertEqual(decoded.conversationId, "conv-42")
        XCTAssertEqual(decoded.projectId, "proj-2")
        XCTAssertEqual(decoded.content, "Reply text")
        XCTAssertNil(decoded.agentPubkey)
        XCTAssertFalse(decoded.isNewConversation)
        XCTAssertEqual(decoded.selectedSkillIds, ["s1", "s2"])
    }

    func testDraftDictionaryRoundTrips() throws {
        let draft1 = Draft(projectId: "proj-a", content: "first")
        let draft2 = Draft(conversationId: "conv-1", projectId: "proj-a", content: "second")
        let drafts: [String: Draft] = [
            draft1.storageKey: draft1,
            draft2.storageKey: draft2
        ]

        let data = try encoder.encode(drafts)
        let decoded = try decoder.decode([String: Draft].self, from: data)

        XCTAssertEqual(decoded.count, 2)
        XCTAssertEqual(decoded[draft1.storageKey]?.content, "first")
        XCTAssertEqual(decoded[draft2.storageKey]?.content, "second")
    }

    // MARK: - Migration: Missing Fields

    func testDecodingWithoutProjectIdDefaultsToEmptyString() throws {
        let json = """
        {
            "id": "draft-1",
            "title": "Test",
            "content": "Hello",
            "isNewConversation": true,
            "lastEdited": 0
        }
        """
        let data = Data(json.utf8)
        let draft = try decoder.decode(Draft.self, from: data)

        XCTAssertEqual(draft.projectId, "")
        XCTAssertEqual(draft.content, "Hello")
    }

    func testDecodingWithoutSelectedNudgeIdsDefaultsToEmptySet() throws {
        let json = """
        {
            "id": "draft-2",
            "projectId": "proj-1",
            "title": "Test",
            "content": "",
            "isNewConversation": true,
            "lastEdited": 0
        }
        """
        let data = Data(json.utf8)
        let draft = try decoder.decode(Draft.self, from: data)

        XCTAssertTrue(draft.selectedNudgeIds.isEmpty)
    }

    func testDecodingWithoutSelectedSkillIdsDefaultsToEmptySet() throws {
        let json = """
        {
            "id": "draft-3",
            "projectId": "proj-1",
            "title": "Test",
            "content": "",
            "isNewConversation": true,
            "lastEdited": 0
        }
        """
        let data = Data(json.utf8)
        let draft = try decoder.decode(Draft.self, from: data)

        XCTAssertTrue(draft.selectedSkillIds.isEmpty)
    }

    func testDecodingWithoutReferenceFieldsDefaultsToNil() throws {
        let json = """
        {
            "id": "draft-4",
            "projectId": "proj-1",
            "title": "",
            "content": "Hello",
            "isNewConversation": false,
            "lastEdited": 0
        }
        """
        let data = Data(json.utf8)
        let draft = try decoder.decode(Draft.self, from: data)

        XCTAssertNil(draft.referenceConversationId)
        XCTAssertNil(draft.referenceReportATag)
    }

    func testDecodingWithoutImageAttachmentsDefaultsToEmptyArray() throws {
        let json = """
        {
            "id": "draft-5",
            "projectId": "proj-1",
            "title": "",
            "content": "",
            "isNewConversation": true,
            "lastEdited": 0
        }
        """
        let data = Data(json.utf8)
        let draft = try decoder.decode(Draft.self, from: data)

        XCTAssertTrue(draft.imageAttachments.isEmpty)
        XCTAssertFalse(draft.hasImages)
    }

    func testDecodingPreV2JsonWithAllFieldsMissing() throws {
        // Simulates a draft from the earliest version with only core fields
        let json = """
        {
            "id": "ancient-draft",
            "title": "Old Title",
            "content": "Old Content",
            "isNewConversation": true,
            "lastEdited": 1000000
        }
        """
        let data = Data(json.utf8)
        let draft = try decoder.decode(Draft.self, from: data)

        XCTAssertEqual(draft.id, "ancient-draft")
        XCTAssertEqual(draft.projectId, "")
        XCTAssertNil(draft.conversationId)
        XCTAssertNil(draft.agentPubkey)
        XCTAssertTrue(draft.selectedNudgeIds.isEmpty)
        XCTAssertTrue(draft.selectedSkillIds.isEmpty)
        XCTAssertNil(draft.referenceConversationId)
        XCTAssertNil(draft.referenceReportATag)
        XCTAssertTrue(draft.imageAttachments.isEmpty)
    }

    func testDecodingWithImageAttachmentsRestoresNextImageId() throws {
        // Encode a draft with images, then decode -- nextImageId should be restored
        // so that adding a new image doesn't collide with existing IDs
        var original = Draft(projectId: "proj-1")
        _ = original.addImageAttachment(url: "https://example.com/1.png")
        _ = original.addImageAttachment(url: "https://example.com/2.png")
        _ = original.addImageAttachment(url: "https://example.com/3.png")

        let data = try encoder.encode(original)
        var decoded = try decoder.decode(Draft.self, from: data)

        // The next image ID should be 4 (max existing ID 3 + 1)
        let nextId = decoded.addImageAttachment(url: "https://example.com/4.png")
        XCTAssertEqual(nextId, 4)
    }

    func testDecodingWithExistingConversationIdSetsCorrectFields() throws {
        let json = """
        {
            "id": "draft-reply",
            "conversationId": "conv-99",
            "projectId": "proj-1",
            "title": "",
            "content": "Reply content",
            "isNewConversation": false,
            "lastEdited": 0
        }
        """
        let data = Data(json.utf8)
        let draft = try decoder.decode(Draft.self, from: data)

        XCTAssertEqual(draft.conversationId, "conv-99")
        XCTAssertFalse(draft.isNewConversation)
    }

    // MARK: - Draft Mutations Not Covered in DraftModelTests

    func testClearAgentResetsAgentAndUpdatesTimestamp() {
        var draft = Draft(projectId: "p", agentPubkey: "agent-1")
        let before = draft.lastEdited

        Thread.sleep(forTimeInterval: 0.01)
        draft.clearAgent()

        XCTAssertNil(draft.agentPubkey)
        XCTAssertGreaterThan(draft.lastEdited, before)
    }

    func testClearNudgesResetsAllNudgesAndUpdatesTimestamp() {
        var draft = Draft(projectId: "p", selectedNudgeIds: ["n1", "n2", "n3"])
        let before = draft.lastEdited

        Thread.sleep(forTimeInterval: 0.01)
        draft.clearNudges()

        XCTAssertTrue(draft.selectedNudgeIds.isEmpty)
        XCTAssertGreaterThan(draft.lastEdited, before)
    }

    func testClearSkillsResetsAllSkillsAndUpdatesTimestamp() {
        var draft = Draft(projectId: "p", selectedSkillIds: ["s1", "s2"])
        let before = draft.lastEdited

        Thread.sleep(forTimeInterval: 0.01)
        draft.clearSkills()

        XCTAssertTrue(draft.selectedSkillIds.isEmpty)
        XCTAssertGreaterThan(draft.lastEdited, before)
    }

    func testClearReferenceConversationResetsAndUpdatesTimestamp() {
        var draft = Draft(projectId: "p", referenceConversationId: "conv-ref")
        let before = draft.lastEdited

        Thread.sleep(forTimeInterval: 0.01)
        draft.clearReferenceConversation()

        XCTAssertNil(draft.referenceConversationId)
        XCTAssertGreaterThan(draft.lastEdited, before)
    }

    func testClearReferenceReportATagResetsAndUpdatesTimestamp() {
        var draft = Draft(projectId: "p", referenceReportATag: "30023:pub:slug")
        let before = draft.lastEdited

        Thread.sleep(forTimeInterval: 0.01)
        draft.clearReferenceReportATag()

        XCTAssertNil(draft.referenceReportATag)
        XCTAssertGreaterThan(draft.lastEdited, before)
    }

    func testClearImageAttachmentsResetsAndNextIdRestartsAt1() {
        var draft = Draft(projectId: "p")
        _ = draft.addImageAttachment(url: "https://example.com/1.png")
        _ = draft.addImageAttachment(url: "https://example.com/2.png")
        XCTAssertEqual(draft.imageAttachments.count, 2)

        draft.clearImageAttachments()

        XCTAssertTrue(draft.imageAttachments.isEmpty)
        let nextId = draft.addImageAttachment(url: "https://example.com/new.png")
        XCTAssertEqual(nextId, 1)
    }

    func testUpdateProjectIdChangesProjectAndUpdatesTimestamp() {
        var draft = Draft(projectId: "old-proj")
        let before = draft.lastEdited

        Thread.sleep(forTimeInterval: 0.01)
        draft.updateProjectId("new-proj")

        XCTAssertEqual(draft.projectId, "new-proj")
        XCTAssertGreaterThan(draft.lastEdited, before)
    }

    // MARK: - DraftStore Actor Tests

    func testDraftStoreLoadFromEmptyDirectoryReturnsEmptyDrafts() async {
        let store = DraftStore()
        // Clean up any existing file first
        let documentsDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let fileURL = documentsDir.appendingPathComponent("message_drafts.json")
        try? FileManager.default.removeItem(at: fileURL)

        let result = await store.loadDrafts()

        XCTAssertTrue(result.drafts.isEmpty)
        XCTAssertFalse(result.loadFailed)
    }

    func testDraftStoreSaveAndLoadRoundTrips() async throws {
        let store = DraftStore()
        let draft1 = Draft(projectId: "proj-a", content: "first draft")
        let draft2 = Draft(conversationId: "conv-1", projectId: "proj-a", content: "reply draft")
        let drafts: [String: Draft] = [
            draft1.storageKey: draft1,
            draft2.storageKey: draft2
        ]

        try await store.saveDrafts(drafts, allowSave: true)
        let result = await store.loadDrafts()

        XCTAssertFalse(result.loadFailed)
        XCTAssertEqual(result.drafts.count, 2)
        XCTAssertEqual(result.drafts[draft1.storageKey]?.content, "first draft")
        XCTAssertEqual(result.drafts[draft2.storageKey]?.content, "reply draft")

        // Clean up
        let documentsDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let fileURL = documentsDir.appendingPathComponent("message_drafts.json")
        try? FileManager.default.removeItem(at: fileURL)
    }

    func testDraftStoreSaveForbiddenWhenAllowSaveIsFalse() async {
        let store = DraftStore()
        let drafts: [String: Draft] = ["key": Draft(projectId: "p")]

        do {
            try await store.saveDrafts(drafts, allowSave: false)
            XCTFail("Expected saveForbidden error")
        } catch {
            guard let storeError = error as? DraftStore.DraftStoreError else {
                XCTFail("Expected DraftStoreError, got \(type(of: error))")
                return
            }
            if case .saveForbidden(let reason) = storeError {
                XCTAssertTrue(reason.contains("quarantined"))
            } else {
                XCTFail("Expected saveForbidden case")
            }
        }
    }

    func testDraftStoreLoadCorruptedFileQuarantinesAndReturnsFailed() async {
        let documentsDir = FileManager.default.urls(for: .documentDirectory, in: .userDomainMask)[0]
        let fileURL = documentsDir.appendingPathComponent("message_drafts.json")

        // Write corrupted JSON
        let corruptedData = Data("this is not valid json{{{".utf8)
        try? corruptedData.write(to: fileURL, options: .atomic)

        let store = DraftStore()
        let result = await store.loadDrafts()

        XCTAssertTrue(result.loadFailed)
        XCTAssertTrue(result.drafts.isEmpty)

        // Original file should have been quarantined (moved)
        XCTAssertFalse(FileManager.default.fileExists(atPath: fileURL.path))

        // Clean up quarantined files
        let documentsContents = try? FileManager.default.contentsOfDirectory(at: documentsDir, includingPropertiesForKeys: nil)
        for url in documentsContents ?? [] {
            if url.lastPathComponent.contains("message_drafts.corrupted-") {
                try? FileManager.default.removeItem(at: url)
            }
        }
    }

    // MARK: - DraftStoreError

    func testDraftStoreErrorLocalizedDescription() {
        let error = DraftStore.DraftStoreError.saveForbidden(reason: "test reason")
        XCTAssertEqual(error.errorDescription, "Save forbidden: test reason")
    }

    // MARK: - ImageAttachment

    func testImageAttachmentEquality() {
        let a = ImageAttachment(id: 1, url: "https://example.com/a.png")
        let b = ImageAttachment(id: 1, url: "https://example.com/a.png")
        let c = ImageAttachment(id: 2, url: "https://example.com/a.png")

        XCTAssertEqual(a, b)
        XCTAssertNotEqual(a, c)
    }

    func testImageAttachmentCodableRoundTrip() throws {
        let original = ImageAttachment(id: 42, url: "https://cdn.example.com/test.jpg")
        let data = try encoder.encode(original)
        let decoded = try decoder.decode(ImageAttachment.self, from: data)

        XCTAssertEqual(decoded.id, 42)
        XCTAssertEqual(decoded.url, "https://cdn.example.com/test.jpg")
    }

    // MARK: - Storage Key Edge Cases

    func testStorageKeyWithEmptyProjectId() {
        let key = Draft.storageKey(for: nil, projectId: "")
        XCTAssertEqual(key, "new-")

        let replyKey = Draft.storageKey(for: "conv-1", projectId: "")
        XCTAssertEqual(replyKey, "reply--conv-1")
    }

    func testStorageKeyWithSpecialCharacters() {
        let key = Draft.storageKey(for: nil, projectId: "proj/with:special")
        XCTAssertEqual(key, "new-proj/with:special")
    }

    func testStorageKeyConsistencyAfterUpdateProjectId() {
        var draft = Draft(projectId: "proj-1")
        let originalKey = draft.storageKey
        XCTAssertEqual(originalKey, "new-proj-1")

        draft.updateProjectId("proj-2")
        // After changing projectId, storageKey changes
        XCTAssertEqual(draft.storageKey, "new-proj-2")
        XCTAssertNotEqual(draft.storageKey, originalKey)
    }

    // MARK: - Draft Equatable

    func testDraftEqualityComparesAllFields() {
        let draft1 = Draft(projectId: "p", content: "hello")
        let draft2 = Draft(projectId: "p", content: "hello")

        // Different drafts have different UUIDs, so they should NOT be equal
        XCTAssertNotEqual(draft1, draft2)

        // Same draft should equal itself
        XCTAssertEqual(draft1, draft1)
    }

    func testDraftCopyMutationDoesNotAffectOriginal() {
        let original = Draft(projectId: "p", content: "original")
        var copy = original

        copy.updateContent("modified")

        XCTAssertEqual(original.content, "original")
        XCTAssertEqual(copy.content, "modified")
    }

    // MARK: - buildFullContent Edge Cases

    func testBuildFullContentWithUnmatchedMarkersLeavesThemIntact() {
        var draft = Draft(projectId: "p")
        draft.updateContent("Check [Image #99] here")
        // No image with ID 99 was added

        XCTAssertEqual(draft.buildFullContent(), "Check [Image #99] here")
    }

    func testBuildFullContentWithMultipleSameMarkers() {
        var draft = Draft(projectId: "p")
        let id = draft.addImageAttachment(url: "https://example.com/img.png")
        draft.updateContent("See [Image #\(id)] and again [Image #\(id)]")

        let result = draft.buildFullContent()
        XCTAssertEqual(result, "See https://example.com/img.png and again https://example.com/img.png")
    }

    func testBuildFullContentEmptyContentWithImages() {
        var draft = Draft(projectId: "p")
        _ = draft.addImageAttachment(url: "https://example.com/img.png")
        // Content is empty, no markers to replace

        XCTAssertEqual(draft.buildFullContent(), "")
    }

    // MARK: - hasContent / isValid with Images Only

    func testHasContentTrueWithOnlyImagesNoText() {
        var draft = Draft(projectId: "p")
        XCTAssertFalse(draft.hasContent)

        _ = draft.addImageAttachment(url: "https://example.com/img.png")
        XCTAssertTrue(draft.hasContent)
        XCTAssertTrue(draft.isValid)
    }

    func testHasContentAfterClearImageAttachments() {
        var draft = Draft(projectId: "p")
        _ = draft.addImageAttachment(url: "https://example.com/img.png")
        XCTAssertTrue(draft.hasContent)

        draft.clearImageAttachments()
        XCTAssertFalse(draft.hasContent)
    }

    // MARK: - Complex Mutation Sequences

    func testMultipleMutationsPreserveUnrelatedFields() {
        var draft = Draft(
            projectId: "proj-1",
            title: "Title",
            content: "Content",
            agentPubkey: "agent",
            selectedNudgeIds: ["n1"],
            selectedSkillIds: ["s1"],
            referenceConversationId: "ref-conv",
            referenceReportATag: "30023:pub:slug"
        )

        // Update content should NOT affect other fields
        draft.updateContent("New content")
        XCTAssertEqual(draft.title, "Title")
        XCTAssertEqual(draft.agentPubkey, "agent")
        XCTAssertEqual(draft.selectedNudgeIds, ["n1"])
        XCTAssertEqual(draft.selectedSkillIds, ["s1"])
        XCTAssertEqual(draft.referenceConversationId, "ref-conv")
        XCTAssertEqual(draft.referenceReportATag, "30023:pub:slug")

        // Clear agent should NOT affect other fields
        draft.clearAgent()
        XCTAssertEqual(draft.content, "New content")
        XCTAssertEqual(draft.selectedNudgeIds, ["n1"])
        XCTAssertEqual(draft.referenceConversationId, "ref-conv")
    }

    func testClearResetsEverythingThenRebuildable() {
        var draft = Draft(
            projectId: "proj-1",
            title: "T",
            content: "C",
            agentPubkey: "a",
            selectedNudgeIds: ["n"],
            selectedSkillIds: ["s"],
            referenceConversationId: "ref",
            referenceReportATag: "tag"
        )
        _ = draft.addImageAttachment(url: "https://example.com/img.png")

        draft.clear()

        // All should be reset
        XCTAssertEqual(draft.title, "")
        XCTAssertEqual(draft.content, "")
        XCTAssertNil(draft.agentPubkey)
        XCTAssertTrue(draft.selectedNudgeIds.isEmpty)
        XCTAssertTrue(draft.selectedSkillIds.isEmpty)
        XCTAssertNil(draft.referenceConversationId)
        XCTAssertNil(draft.referenceReportATag)
        XCTAssertTrue(draft.imageAttachments.isEmpty)

        // Should be rebuildable from scratch
        draft.updateContent("Rebuilt")
        draft.setAgent("new-agent")
        draft.addNudge("new-nudge")
        draft.addSkill("new-skill")
        draft.setReferenceConversation("new-ref")
        draft.setReferenceReportATag("new-tag")
        _ = draft.addImageAttachment(url: "https://example.com/rebuilt.png")

        XCTAssertEqual(draft.content, "Rebuilt")
        XCTAssertEqual(draft.agentPubkey, "new-agent")
        XCTAssertEqual(draft.selectedNudgeIds, ["new-nudge"])
        XCTAssertEqual(draft.selectedSkillIds, ["new-skill"])
        XCTAssertEqual(draft.referenceConversationId, "new-ref")
        XCTAssertEqual(draft.referenceReportATag, "new-tag")
        XCTAssertEqual(draft.imageAttachments.count, 1)
    }
}
