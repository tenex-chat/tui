import XCTest
@testable import TenexMVP

final class ComposerPinControlPolicyTests: XCTestCase {

    func testPinControlModeHiddenWhenInputIsEmptyAndNoPinnedPrompts() {
        let mode = MessageComposerView.pinControlMode(forInputText: "", pinnedPromptCount: 0)
        XCTAssertEqual(mode, .hidden)
    }

    func testPinControlModeMenuWhenInputIsEmptyAndPinnedPromptsExist() {
        let mode = MessageComposerView.pinControlMode(forInputText: "", pinnedPromptCount: 2)
        XCTAssertEqual(mode, .menu)
    }

    func testPinControlModePinActionWhenInputHasText() {
        let modeWithoutPinnedPrompts = MessageComposerView.pinControlMode(
            forInputText: "Ship this",
            pinnedPromptCount: 0
        )
        let modeWithPinnedPrompts = MessageComposerView.pinControlMode(
            forInputText: "Ship this",
            pinnedPromptCount: 8
        )

        XCTAssertEqual(modeWithoutPinnedPrompts, .pinAction)
        XCTAssertEqual(modeWithPinnedPrompts, .pinAction)
    }

    func testPinControlModeTreatsWhitespaceInputAsEmpty() {
        let mode = MessageComposerView.pinControlMode(forInputText: "  \n\t  ", pinnedPromptCount: 1)
        XCTAssertEqual(mode, .menu)
    }

    func testCanPinCurrentPromptForNewConversationsAndReplies() {
        let project = makeProject()

        let newConversationComposer = MessageComposerView(
            project: project,
            conversationId: nil,
            initialContent: "Pin me",
            displayStyle: .inline,
            inlineLayoutStyle: .workspace
        )
        let replyComposer = MessageComposerView(
            project: project,
            conversationId: "thread-123",
            initialContent: "Pin me",
            displayStyle: .inline,
            inlineLayoutStyle: .workspace
        )

        XCTAssertTrue(newConversationComposer.canPinCurrentPrompt)
        XCTAssertTrue(replyComposer.canPinCurrentPrompt)
    }

    private func makeProject() -> Project {
        Project(
            id: "project-1",
            title: "Test Project",
            description: "Test",
            repoUrl: nil,
            pictureUrl: nil,
            isDeleted: false,
            pubkey: "",
            participants: [],
            agentDefinitionIds: [],
            mcpToolIds: [],
            createdAt: 0
        )
    }
}
