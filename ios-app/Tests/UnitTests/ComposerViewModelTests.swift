import XCTest
@testable import TenexMVP

@MainActor
final class ComposerViewModelTests: XCTestCase {
    func testProjectWithMostRecentActivityRespectsScheduledFilter() async {
        let core = MockCoreGateway()
        core.projects = [
            makeProject(id: "project-a"),
            makeProject(id: "project-b")
        ]
        core.conversations = [
            makeConversation(id: "conv-a", projectId: "project-a", lastActivity: 100, isScheduled: false),
            makeConversation(id: "conv-b", projectId: "project-b", lastActivity: 200, isScheduled: true)
        ]

        let viewModel = makeViewModel(core: core, drafts: MockDraftStore())

        // Hide scheduled → prefer non-scheduled project-a
        XCTAssertEqual(viewModel.projectWithMostRecentActivity(scheduledFilter: .hide)?.id, "project-a")
        // Show All → prefer most-recent project-b (regardless of scheduled status)
        XCTAssertEqual(viewModel.projectWithMostRecentActivity(scheduledFilter: .showAll)?.id, "project-b")
        // Show Only scheduled → prefer project-b
        XCTAssertEqual(viewModel.projectWithMostRecentActivity(scheduledFilter: .showOnly)?.id, "project-b")
    }

    func testDetectInlineTriggerParsesAgentAndNudgeTokens() async {
        let viewModel = makeViewModel(core: MockCoreGateway(), drafts: MockDraftStore())

        let agent = viewModel.detectInlineTrigger(in: "please ping @architect")
        if case .agent? = agent?.kind {
        } else {
            XCTFail("Expected @ trigger to resolve as agent")
        }
        XCTAssertEqual(agent?.query, "architect")

        let nudge = viewModel.detectInlineTrigger(in: "use /bugfix")
        if case .nudgeSkill? = nudge?.kind {
        } else {
            XCTFail("Expected / trigger to resolve as nudge/skill")
        }
        XCTAssertEqual(nudge?.query, "bugfix")

        XCTAssertNil(viewModel.detectInlineTrigger(in: "plain text"))
    }

    func testLoadDraftWithInitialContentOverridesStoredDraftAndPersistsReferences() async {
        let draftStore = MockDraftStore()
        let existing = Draft(projectId: "project-a", content: "old content")
        draftStore.seedDraft(existing, conversationId: nil, projectId: "project-a")

        let viewModel = makeViewModel(core: MockCoreGateway(), drafts: draftStore)
        let result = await viewModel.loadDraft(
            projectId: "project-a",
            conversationId: nil,
            initialContent: "new content",
            referenceConversationId: "conv-ref",
            referenceReportATag: "30023:pubkey:slug",
            isDirty: false
        )

        XCTAssertFalse(result.shouldShowLoadFailedAlert)
        XCTAssertEqual(result.draft?.content, "new content")
        XCTAssertEqual(result.localText, "new content")
        XCTAssertTrue(draftStore.updatedContents.contains("new content"))
        XCTAssertEqual(draftStore.updatedReferenceConversationIds.last ?? nil, "conv-ref")
        XCTAssertEqual(draftStore.updatedReferenceReportATags.last ?? nil, "30023:pubkey:slug")
    }

    func testLoadDraftWhenDirtyDoesNotOverwriteAndOnlyPersistsReferences() async {
        let draftStore = MockDraftStore()
        let viewModel = makeViewModel(core: MockCoreGateway(), drafts: draftStore)

        let result = await viewModel.loadDraft(
            projectId: "project-a",
            conversationId: nil,
            initialContent: "ignored",
            referenceConversationId: "conv-ref",
            referenceReportATag: "30023:pubkey:slug",
            isDirty: true
        )

        XCTAssertNil(result.draft)
        XCTAssertNil(result.localText)
        XCTAssertFalse(result.shouldShowLoadFailedAlert)
        XCTAssertTrue(draftStore.updatedContents.isEmpty)
        XCTAssertEqual(draftStore.updatedReferenceConversationIds.last ?? nil, "conv-ref")
        XCTAssertEqual(draftStore.updatedReferenceReportATags.last ?? nil, "30023:pubkey:slug")
    }

    func testLoadAgentContextUsesInitialAgentAndResolvesName() async {
        let core = MockCoreGateway()
        core.onlineAgents = ["project-a": []]
        core.profileNamesByPubkey["pubkey-author"] = "Author Name"
        let drafts = MockDraftStore()

        let viewModel = makeViewModel(core: core, drafts: drafts)
        let result = await viewModel.loadAgentContext(
            projectId: "project-a",
            conversationId: "conv-1",
            initialAgentPubkey: "pubkey-author",
            currentAgentPubkey: nil
        )

        XCTAssertEqual(result.selectedAgentPubkey, "pubkey-author")
        XCTAssertEqual(result.replyTargetAgentName, "Author Name")
        XCTAssertEqual(drafts.updatedAgentPubkeys.last ?? nil, "pubkey-author")
    }

    func testSendMessageUsesThreadEndpointForNewConversation() async throws {
        let core = MockCoreGateway()
        core.threadSendResult = SendMessageResult(eventId: "evt-thread", success: true)
        let viewModel = makeViewModel(core: core, drafts: MockDraftStore())

        let result = try await viewModel.sendMessage(
            isNewConversation: true,
            conversationId: nil,
            projectId: "project-a",
            content: "hello",
            agentPubkey: "agent",
            nudgeIds: ["n1"],
            skillIds: ["s1"]
        )

        XCTAssertEqual(result.eventId, "evt-thread")
        XCTAssertEqual(core.sendThreadCalls.count, 1)
        XCTAssertEqual(core.sendMessageCalls.count, 0)
    }

    func testSendMessageUsesReplyEndpointForExistingConversation() async throws {
        let core = MockCoreGateway()
        core.replySendResult = SendMessageResult(eventId: "evt-reply", success: true)
        let viewModel = makeViewModel(core: core, drafts: MockDraftStore())

        let result = try await viewModel.sendMessage(
            isNewConversation: false,
            conversationId: "conv-1",
            projectId: "project-a",
            content: "hello",
            agentPubkey: nil,
            nudgeIds: [],
            skillIds: []
        )

        XCTAssertEqual(result.eventId, "evt-reply")
        XCTAssertEqual(core.sendThreadCalls.count, 0)
        XCTAssertEqual(core.sendMessageCalls.count, 1)
    }

    func testValidatedAgentPubkeyClearsInvalidSelectionWhenAgentListIsLoaded() async {
        let drafts = MockDraftStore()
        let viewModel = makeViewModel(core: MockCoreGateway(), drafts: drafts)
        let agents = [ProjectAgent(pubkey: "other", name: "Other", isPm: false, model: nil, tools: [])]

        let validated = await viewModel.validatedAgentPubkey(
            candidate: "missing",
            initialAgentPubkey: nil,
            agentsLoadError: nil,
            availableAgents: agents,
            conversationId: "conv-1",
            projectId: "project-a"
        )

        XCTAssertNil(validated)
        XCTAssertEqual(drafts.updatedAgentPubkeys.last ?? "sentinel", nil)
    }

    // MARK: - Helpers

    private func makeViewModel(
        core: MockCoreGateway,
        drafts: MockDraftStore
    ) -> ComposerViewModel {
        ComposerViewModel(
            dependencies: ComposerDependencies(
                core: core,
                drafts: drafts,
                credentials: MockCredentialStore(),
                notifications: MockNotificationScheduler()
            )
        )
    }

    private func makeProject(id: String) -> Project {
        Project(
            id: id,
            title: id,
            description: nil,
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

    private func makeConversation(
        id: String,
        projectId: String,
        lastActivity: UInt64,
        isScheduled: Bool
    ) -> ConversationFullInfo {
        let thread = Thread(
            id: id,
            title: id,
            content: "",
            pubkey: "author-pubkey",
            lastActivity: lastActivity,
            effectiveLastActivity: lastActivity,
            statusLabel: nil,
            statusCurrentActivity: nil,
            summary: nil,
            hashtags: [],
            parentConversationId: nil,
            pTags: [],
            askEvent: nil,
            isScheduled: isScheduled
        )
        return ConversationFullInfo(
            thread: thread,
            author: "author",
            messageCount: 1,
            isActive: false,
            isArchived: false,
            hasChildren: false,
            projectATag: "31922:owner:\(projectId)"
        )
    }
}

private struct SendInvocation: Equatable {
    let conversationId: String?
    let projectId: String
    let content: String
    let agentPubkey: String?
    let nudgeIds: [String]
    let skillIds: [String]
}

@MainActor
private final class MockCoreGateway: CoreGateway {
    var projects: [Project] = []
    var conversations: [ConversationFullInfo] = []
    var onlineAgents: [String: [ProjectAgent]] = [:]

    var nudges: [Nudge] = []
    var skills: [Skill] = []
    var profileNamesByPubkey: [String: String] = [:]

    var sendThreadCalls: [SendInvocation] = []
    var sendMessageCalls: [SendInvocation] = []
    var threadSendResult = SendMessageResult(eventId: "thread", success: true)
    var replySendResult = SendMessageResult(eventId: "reply", success: true)

    func getNudges() async throws -> [Nudge] {
        nudges
    }

    func getSkills() async throws -> [Skill] {
        skills
    }

    func getProfileName(pubkey: String) async -> String {
        profileNamesByPubkey[pubkey] ?? ""
    }

    func sendThread(
        projectId: String,
        title _: String,
        content: String,
        agentPubkey: String?,
        nudgeIds: [String],
        skillIds: [String]
    ) async throws -> SendMessageResult {
        sendThreadCalls.append(
            SendInvocation(
                conversationId: nil,
                projectId: projectId,
                content: content,
                agentPubkey: agentPubkey,
                nudgeIds: nudgeIds,
                skillIds: skillIds
            )
        )
        return threadSendResult
    }

    func sendMessage(
        conversationId: String,
        projectId: String,
        content: String,
        agentPubkey: String?,
        nudgeIds: [String],
        skillIds: [String]
    ) async throws -> SendMessageResult {
        sendMessageCalls.append(
            SendInvocation(
                conversationId: conversationId,
                projectId: projectId,
                content: content,
                agentPubkey: agentPubkey,
                nudgeIds: nudgeIds,
                skillIds: skillIds
            )
        )
        return replySendResult
    }

    func uploadImage(data _: Data, mimeType _: String) async throws -> String {
        "https://example.com/image.png"
    }

    func recordUserActivity(conversationId _: String) {}
}

@MainActor
private final class MockDraftStore: DraftPersisting {
    var loadFailed = false
    var shouldSaveFail = false
    var drafts: [String: Draft] = [:]

    var updatedContents: [String] = []
    var updatedAgentPubkeys: [String?] = []
    var updatedReferenceConversationIds: [String?] = []
    var updatedReferenceReportATags: [String?] = []

    func seedDraft(_ draft: Draft, conversationId: String?, projectId: String) {
        drafts[Draft.storageKey(for: conversationId, projectId: projectId)] = draft
    }

    func getOrCreateDraft(conversationId: String?, projectId: String) async -> Draft {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        if let draft = drafts[key] {
            return draft
        }
        let created = conversationId.map { Draft(conversationId: $0, projectId: projectId) } ?? Draft(projectId: projectId)
        drafts[key] = created
        return created
    }

    func updateContent(_ content: String, conversationId: String?, projectId: String) async {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        var draft = drafts[key] ?? (conversationId.map { Draft(conversationId: $0, projectId: projectId) } ?? Draft(projectId: projectId))
        draft.updateContent(content)
        drafts[key] = draft
        updatedContents.append(content)
    }

    func updateAgent(_ agentPubkey: String?, conversationId: String?, projectId: String) async {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        var draft = drafts[key] ?? (conversationId.map { Draft(conversationId: $0, projectId: projectId) } ?? Draft(projectId: projectId))
        draft.setAgent(agentPubkey)
        drafts[key] = draft
        updatedAgentPubkeys.append(agentPubkey)
    }

    func updateReferenceConversation(_ referenceConversationId: String?, conversationId: String?, projectId: String) async {
        updatedReferenceConversationIds.append(referenceConversationId)
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        var draft = drafts[key] ?? (conversationId.map { Draft(conversationId: $0, projectId: projectId) } ?? Draft(projectId: projectId))
        draft.setReferenceConversation(referenceConversationId)
        drafts[key] = draft
    }

    func updateReferenceReportATag(_ referenceReportATag: String?, conversationId: String?, projectId: String) async {
        updatedReferenceReportATags.append(referenceReportATag)
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        var draft = drafts[key] ?? (conversationId.map { Draft(conversationId: $0, projectId: projectId) } ?? Draft(projectId: projectId))
        draft.setReferenceReportATag(referenceReportATag)
        drafts[key] = draft
    }

    func updateNudgeIds(_ nudgeIds: Set<String>, conversationId: String?, projectId: String) async {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        var draft = drafts[key] ?? (conversationId.map { Draft(conversationId: $0, projectId: projectId) } ?? Draft(projectId: projectId))
        draft.selectedNudgeIds = nudgeIds
        drafts[key] = draft
    }

    func updateSkillIds(_ skillIds: Set<String>, conversationId: String?, projectId: String) async {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        var draft = drafts[key] ?? (conversationId.map { Draft(conversationId: $0, projectId: projectId) } ?? Draft(projectId: projectId))
        draft.selectedSkillIds = skillIds
        drafts[key] = draft
    }

    func updateImageAttachments(_ imageAttachments: [ImageAttachment], conversationId: String?, projectId: String) async {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        var draft = drafts[key] ?? (conversationId.map { Draft(conversationId: $0, projectId: projectId) } ?? Draft(projectId: projectId))
        draft.imageAttachments = imageAttachments
        drafts[key] = draft
    }

    func clearDraft(conversationId: String?, projectId: String) async {
        let key = Draft.storageKey(for: conversationId, projectId: projectId)
        var draft = drafts[key] ?? (conversationId.map { Draft(conversationId: $0, projectId: projectId) } ?? Draft(projectId: projectId))
        draft.clear()
        drafts[key] = draft
    }

    func deleteDraft(conversationId: String?, projectId: String) async {
        drafts.removeValue(forKey: Draft.storageKey(for: conversationId, projectId: projectId))
    }

    func saveNow() async throws {
        if shouldSaveFail {
            throw MockDraftError.saveFailed
        }
    }
}

private enum MockDraftError: LocalizedError {
    case saveFailed

    var errorDescription: String? {
        "save failed"
    }
}

private final class MockCredentialStore: CredentialStoring {
    func loadNsec() -> KeychainResult<String> {
        .failure(.itemNotFound)
    }

    func loadNsecAsync() async -> KeychainResult<String> {
        .failure(.itemNotFound)
    }

    func saveNsecAsync(_: String) async -> KeychainResult<Void> {
        .success(())
    }

    func deleteNsecAsync() async -> KeychainResult<Void> {
        .success(())
    }
}

@MainActor
private final class MockNotificationScheduler: NotificationScheduling {
    func requestAuthorization() async -> NotificationService.AuthorizationResult {
        .granted
    }

    func checkAuthorizationStatus() async {}

    func updateBadge(count _: Int) async {}

    func clearBadge() async {}
}
