import XCTest
@testable import TenexMVP

final class DraftModelTests: XCTestCase {

    // MARK: - Initialization

    func testNewConversationDraftInitializesCorrectly() {
        let draft = Draft(projectId: "proj-1", title: "My Title", content: "Hello")

        XCTAssertEqual(draft.projectId, "proj-1")
        XCTAssertEqual(draft.title, "My Title")
        XCTAssertEqual(draft.content, "Hello")
        XCTAssertNil(draft.conversationId)
        XCTAssertTrue(draft.isNewConversation)
        XCTAssertNil(draft.agentPubkey)
        XCTAssertTrue(draft.selectedNudgeIds.isEmpty)
        XCTAssertTrue(draft.selectedSkillIds.isEmpty)
        XCTAssertTrue(draft.imageAttachments.isEmpty)
        XCTAssertTrue(draft.textAttachments.isEmpty)
        XCTAssertNil(draft.referenceConversationId)
        XCTAssertNil(draft.referenceReportATag)
    }

    func testExistingConversationDraftInitializesCorrectly() {
        let draft = Draft(conversationId: "conv-42", projectId: "proj-2", content: "Reply text")

        XCTAssertEqual(draft.conversationId, "conv-42")
        XCTAssertEqual(draft.projectId, "proj-2")
        XCTAssertEqual(draft.content, "Reply text")
        XCTAssertEqual(draft.title, "")
        XCTAssertFalse(draft.isNewConversation)
    }

    // MARK: - updateContent

    func testUpdateContentChangesContentAndUpdatesTimestamp() {
        var draft = Draft(projectId: "p")
        let before = draft.lastEdited

        Thread.sleep(forTimeInterval: 0.01)
        draft.updateContent("new text")

        XCTAssertEqual(draft.content, "new text")
        XCTAssertGreaterThan(draft.lastEdited, before)
    }

    // MARK: - updateTitle

    func testUpdateTitleChangesTitleAndUpdatesTimestamp() {
        var draft = Draft(projectId: "p", title: "old")
        let before = draft.lastEdited

        Thread.sleep(forTimeInterval: 0.01)
        draft.updateTitle("new title")

        XCTAssertEqual(draft.title, "new title")
        XCTAssertGreaterThan(draft.lastEdited, before)
    }

    // MARK: - setAgent

    func testSetAgentAssignsPubkey() {
        var draft = Draft(projectId: "p")
        XCTAssertNil(draft.agentPubkey)

        draft.setAgent("pubkey-abc")
        XCTAssertEqual(draft.agentPubkey, "pubkey-abc")
    }

    func testSetAgentToNilClearsAgent() {
        var draft = Draft(projectId: "p", agentPubkey: "initial")
        XCTAssertEqual(draft.agentPubkey, "initial")

        draft.setAgent(nil)
        XCTAssertNil(draft.agentPubkey)
    }

    // MARK: - addNudge / removeNudge

    func testAddNudgeInsertsId() {
        var draft = Draft(projectId: "p")
        draft.addNudge("nudge-1")
        draft.addNudge("nudge-2")

        XCTAssertEqual(draft.selectedNudgeIds, ["nudge-1", "nudge-2"])
    }

    func testAddNudgeIsIdempotent() {
        var draft = Draft(projectId: "p")
        draft.addNudge("nudge-1")
        draft.addNudge("nudge-1")

        XCTAssertEqual(draft.selectedNudgeIds.count, 1)
    }

    func testRemoveNudgeRemovesId() {
        var draft = Draft(projectId: "p", selectedNudgeIds: ["nudge-1", "nudge-2"])
        draft.removeNudge("nudge-1")

        XCTAssertEqual(draft.selectedNudgeIds, ["nudge-2"])
    }

    func testRemoveNudgeForNonexistentIdIsNoOp() {
        var draft = Draft(projectId: "p", selectedNudgeIds: ["nudge-1"])
        let before = draft.selectedNudgeIds
        draft.removeNudge("nonexistent")

        XCTAssertEqual(draft.selectedNudgeIds, before)
    }

    // MARK: - addSkill / removeSkill

    func testAddSkillInsertsId() {
        var draft = Draft(projectId: "p")
        draft.addSkill("skill-a")
        draft.addSkill("skill-b")

        XCTAssertEqual(draft.selectedSkillIds, ["skill-a", "skill-b"])
    }

    func testAddSkillIsIdempotent() {
        var draft = Draft(projectId: "p")
        draft.addSkill("skill-a")
        draft.addSkill("skill-a")

        XCTAssertEqual(draft.selectedSkillIds.count, 1)
    }

    func testRemoveSkillRemovesId() {
        var draft = Draft(projectId: "p", selectedSkillIds: ["skill-a", "skill-b"])
        draft.removeSkill("skill-a")

        XCTAssertEqual(draft.selectedSkillIds, ["skill-b"])
    }

    func testRemoveSkillForNonexistentIdIsNoOp() {
        var draft = Draft(projectId: "p", selectedSkillIds: ["skill-a"])
        draft.removeSkill("nonexistent")

        XCTAssertEqual(draft.selectedSkillIds, ["skill-a"])
    }

    // MARK: - setReferenceConversation / setReferenceReportATag

    func testSetReferenceConversation() {
        var draft = Draft(projectId: "p")
        draft.setReferenceConversation("conv-ref-1")

        XCTAssertEqual(draft.referenceConversationId, "conv-ref-1")

        draft.setReferenceConversation(nil)
        XCTAssertNil(draft.referenceConversationId)
    }

    func testSetReferenceReportATag() {
        var draft = Draft(projectId: "p")
        draft.setReferenceReportATag("30023:pubkey:slug")

        XCTAssertEqual(draft.referenceReportATag, "30023:pubkey:slug")

        draft.setReferenceReportATag(nil)
        XCTAssertNil(draft.referenceReportATag)
    }

    // MARK: - Image Attachments

    func testAddImageAttachmentReturnsIncrementingIds() {
        var draft = Draft(projectId: "p")

        let id1 = draft.addImageAttachment(url: "https://example.com/a.png")
        let id2 = draft.addImageAttachment(url: "https://example.com/b.png")

        XCTAssertEqual(id1, 1)
        XCTAssertEqual(id2, 2)
        XCTAssertEqual(draft.imageAttachments.count, 2)
        XCTAssertEqual(draft.imageAttachments[0].url, "https://example.com/a.png")
        XCTAssertEqual(draft.imageAttachments[1].url, "https://example.com/b.png")
    }

    func testRemoveImageAttachmentById() {
        var draft = Draft(projectId: "p")
        let id1 = draft.addImageAttachment(url: "https://example.com/a.png")
        _ = draft.addImageAttachment(url: "https://example.com/b.png")

        draft.removeImageAttachment(id: id1)

        XCTAssertEqual(draft.imageAttachments.count, 1)
        XCTAssertEqual(draft.imageAttachments[0].url, "https://example.com/b.png")
    }

    func testRemoveImageAttachmentWithInvalidIdIsNoOp() {
        var draft = Draft(projectId: "p")
        _ = draft.addImageAttachment(url: "https://example.com/a.png")

        draft.removeImageAttachment(id: 999)
        XCTAssertEqual(draft.imageAttachments.count, 1)
    }

    // MARK: - buildFullContent

    func testBuildFullContentReplacesImageMarkers() {
        var draft = Draft(projectId: "p")
        let id1 = draft.addImageAttachment(url: "https://cdn.example.com/img1.png")
        let id2 = draft.addImageAttachment(url: "https://cdn.example.com/img2.jpg")
        draft.updateContent("Check this [Image #\(id1)] and also [Image #\(id2)] done")

        let result = draft.buildFullContent()

        XCTAssertEqual(result, "Check this https://cdn.example.com/img1.png and also https://cdn.example.com/img2.jpg done")
    }

    func testBuildFullContentWithNoImagesReturnsContentUnchanged() {
        var draft = Draft(projectId: "p")
        draft.updateContent("plain text without images")

        XCTAssertEqual(draft.buildFullContent(), "plain text without images")
    }

    func testBuildFullContentWithTextAttachmentsAppendsSections() {
        var draft = Draft(projectId: "p")
        let textAttachmentId = draft.addTextAttachment(content: "alpha\\nbeta")
        draft.updateContent("Please review [Text Attachment \(textAttachmentId)]")

        let result = draft.buildFullContent()

        XCTAssertTrue(result.contains("Please review [Text Attachment 1]"))
        XCTAssertTrue(result.contains("----"))
        XCTAssertTrue(result.contains("-- Text Attachment 1 --"))
        XCTAssertTrue(result.contains("alpha\\nbeta"))
    }

    func testBuildFullContentWithMixedImageAndTextAttachments() {
        var draft = Draft(projectId: "p")
        let imageId = draft.addImageAttachment(url: "https://cdn.example.com/img1.png")
        let textId = draft.addTextAttachment(content: "large paste payload")
        draft.updateContent("img [Image #\(imageId)] text [Text Attachment \(textId)]")

        let result = draft.buildFullContent()

        XCTAssertTrue(result.contains("img https://cdn.example.com/img1.png text [Text Attachment 1]"))
        XCTAssertTrue(result.contains("-- Text Attachment 1 --"))
        XCTAssertTrue(result.contains("large paste payload"))
    }

    // MARK: - storageKey

    func testStorageKeyForNewConversation() {
        let key = Draft.storageKey(for: nil, projectId: "proj-abc")
        XCTAssertEqual(key, "new-proj-abc")
    }

    func testStorageKeyForReply() {
        let key = Draft.storageKey(for: "conv-123", projectId: "proj-abc")
        XCTAssertEqual(key, "reply-proj-abc-conv-123")
    }

    func testInstanceStorageKeyMatchesStaticMethod() {
        let newDraft = Draft(projectId: "proj-1")
        XCTAssertEqual(newDraft.storageKey, Draft.storageKey(for: nil, projectId: "proj-1"))

        let replyDraft = Draft(conversationId: "conv-1", projectId: "proj-1")
        XCTAssertEqual(replyDraft.storageKey, Draft.storageKey(for: "conv-1", projectId: "proj-1"))
    }

    // MARK: - Computed Properties

    func testHasContentIsFalseForEmptyAndWhitespace() {
        let empty = Draft(projectId: "p")
        XCTAssertFalse(empty.hasContent)

        var whitespace = Draft(projectId: "p")
        whitespace.updateContent("   \n\t  ")
        XCTAssertFalse(whitespace.hasContent)
    }

    func testHasContentIsTrueWithTextOrImages() {
        var withText = Draft(projectId: "p")
        withText.updateContent("hello")
        XCTAssertTrue(withText.hasContent)

        var withImage = Draft(projectId: "p")
        _ = withImage.addImageAttachment(url: "https://example.com/img.png")
        XCTAssertTrue(withImage.hasContent)

        var withTextAttachment = Draft(projectId: "p")
        _ = withTextAttachment.addTextAttachment(content: "context payload")
        XCTAssertTrue(withTextAttachment.hasContent)
    }

    func testIsValidMatchesHasContentBehavior() {
        let empty = Draft(projectId: "p")
        XCTAssertFalse(empty.isValid)

        var withText = Draft(projectId: "p")
        withText.updateContent("valid")
        XCTAssertTrue(withText.isValid)

        var imageOnly = Draft(projectId: "p")
        _ = imageOnly.addImageAttachment(url: "https://example.com/img.png")
        XCTAssertTrue(imageOnly.isValid)

        var textAttachmentOnly = Draft(projectId: "p")
        _ = textAttachmentOnly.addTextAttachment(content: "context payload")
        XCTAssertTrue(textAttachmentOnly.isValid)
    }

    func testHasImages() {
        var draft = Draft(projectId: "p")
        XCTAssertFalse(draft.hasImages)

        _ = draft.addImageAttachment(url: "https://example.com/img.png")
        XCTAssertTrue(draft.hasImages)
    }

    // MARK: - clear

    func testClearResetsAllFields() {
        var draft = Draft(projectId: "p", title: "T", content: "C", agentPubkey: "agent",
                          selectedNudgeIds: ["n"], selectedSkillIds: ["s"],
                          referenceConversationId: "ref", referenceReportATag: "atag")
        _ = draft.addImageAttachment(url: "https://example.com/img.png")
        _ = draft.addTextAttachment(content: "context payload")

        draft.clear()

        XCTAssertEqual(draft.title, "")
        XCTAssertEqual(draft.content, "")
        XCTAssertNil(draft.agentPubkey)
        XCTAssertTrue(draft.selectedNudgeIds.isEmpty)
        XCTAssertTrue(draft.selectedSkillIds.isEmpty)
        XCTAssertNil(draft.referenceConversationId)
        XCTAssertNil(draft.referenceReportATag)
        XCTAssertTrue(draft.imageAttachments.isEmpty)
        XCTAssertTrue(draft.textAttachments.isEmpty)
        // nextImageId resets to 1, so next add should return 1
        let nextId = draft.addImageAttachment(url: "https://example.com/new.png")
        XCTAssertEqual(nextId, 1)
        let nextTextAttachmentId = draft.addTextAttachment(content: "new payload")
        XCTAssertEqual(nextTextAttachmentId, 1)
    }

    // MARK: - Text Attachments

    func testAddTextAttachmentReturnsIncrementingIds() {
        var draft = Draft(projectId: "p")

        let id1 = draft.addTextAttachment(content: "one")
        let id2 = draft.addTextAttachment(content: "two")

        XCTAssertEqual(id1, 1)
        XCTAssertEqual(id2, 2)
        XCTAssertEqual(draft.textAttachments.count, 2)
        XCTAssertEqual(draft.textAttachments[0].content, "one")
        XCTAssertEqual(draft.textAttachments[1].content, "two")
    }

    func testRemoveTextAttachmentById() {
        var draft = Draft(projectId: "p")
        let id1 = draft.addTextAttachment(content: "one")
        _ = draft.addTextAttachment(content: "two")

        draft.removeTextAttachment(id: id1)

        XCTAssertEqual(draft.textAttachments.count, 1)
        XCTAssertEqual(draft.textAttachments[0].content, "two")
    }
}
