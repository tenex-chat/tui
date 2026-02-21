import Foundation

@MainActor
protocol CoreGateway: AnyObject {
    var projects: [Project] { get }
    var conversations: [ConversationFullInfo] { get }
    var onlineAgents: [String: [ProjectAgent]] { get }

    func getNudges() async throws -> [Nudge]
    func getSkills() async throws -> [Skill]
    func getProfileName(pubkey: String) async -> String
    func sendThread(
        projectId: String,
        title: String,
        content: String,
        agentPubkey: String?,
        nudgeIds: [String],
        skillIds: [String]
    ) async throws -> SendMessageResult
    func sendMessage(
        conversationId: String,
        projectId: String,
        content: String,
        agentPubkey: String?,
        nudgeIds: [String],
        skillIds: [String]
    ) async throws -> SendMessageResult
    func uploadImage(data: Data, mimeType: String) async throws -> String
    func recordUserActivity(conversationId: String)
}

@MainActor
protocol DraftPersisting: AnyObject {
    var loadFailed: Bool { get }

    func getOrCreateDraft(conversationId: String?, projectId: String) async -> Draft
    func updateContent(_ content: String, conversationId: String?, projectId: String) async
    func updateAgent(_ agentPubkey: String?, conversationId: String?, projectId: String) async
    func updateReferenceConversation(_ referenceConversationId: String?, conversationId: String?, projectId: String) async
    func updateReferenceReportATag(_ referenceReportATag: String?, conversationId: String?, projectId: String) async
    func updateNudgeIds(_ nudgeIds: Set<String>, conversationId: String?, projectId: String) async
    func updateSkillIds(_ skillIds: Set<String>, conversationId: String?, projectId: String) async
    func updateImageAttachments(_ imageAttachments: [ImageAttachment], conversationId: String?, projectId: String) async
    func clearDraft(conversationId: String?, projectId: String) async
    func deleteDraft(conversationId: String?, projectId: String) async
    func saveNow() async throws
}

protocol CredentialStoring: AnyObject {
    func loadNsec() -> KeychainResult<String>
    func loadNsecAsync() async -> KeychainResult<String>
    func saveNsecAsync(_ nsec: String) async -> KeychainResult<Void>
    func deleteNsecAsync() async -> KeychainResult<Void>
}

@MainActor
protocol NotificationScheduling: AnyObject {
    func requestAuthorization() async -> NotificationService.AuthorizationResult
    func checkAuthorizationStatus() async
    func updateBadge(count: Int) async
    func clearBadge() async
    func scheduleAskNotification(
        askEventId: String,
        title: String,
        body: String,
        fromAgent: String,
        projectId: String?,
        conversationId: String?
    ) async
}

struct ComposerDependencies {
    let core: CoreGateway
    let drafts: DraftPersisting
    let credentials: CredentialStoring
    let notifications: NotificationScheduling

    @MainActor
    static func live(
        core: CoreGateway,
        drafts: DraftPersisting? = nil,
        credentials: CredentialStoring? = nil,
        notifications: NotificationScheduling? = nil
    ) -> ComposerDependencies {
        let resolvedDrafts = drafts ?? DraftManager.shared
        let resolvedCredentials = credentials ?? KeychainService.shared
        let resolvedNotifications = notifications ?? NotificationService.shared
        return ComposerDependencies(
            core: core,
            drafts: resolvedDrafts,
            credentials: resolvedCredentials,
            notifications: resolvedNotifications
        )
    }
}

extension TenexCoreManager: CoreGateway {
    func getNudges() async throws -> [Nudge] {
        try await safeCore.getNudges()
    }

    func getSkills() async throws -> [Skill] {
        try await safeCore.getSkills()
    }

    func getProfileName(pubkey: String) async -> String {
        await safeCore.getProfileName(pubkey: pubkey)
    }

    func sendThread(
        projectId: String,
        title: String,
        content: String,
        agentPubkey: String?,
        nudgeIds: [String],
        skillIds: [String]
    ) async throws -> SendMessageResult {
        try await safeCore.sendThread(
            projectId: projectId,
            title: title,
            content: content,
            agentPubkey: agentPubkey,
            nudgeIds: nudgeIds,
            skillIds: skillIds
        )
    }

    func sendMessage(
        conversationId: String,
        projectId: String,
        content: String,
        agentPubkey: String?,
        nudgeIds: [String],
        skillIds: [String]
    ) async throws -> SendMessageResult {
        try await safeCore.sendMessage(
            conversationId: conversationId,
            projectId: projectId,
            content: content,
            agentPubkey: agentPubkey,
            nudgeIds: nudgeIds,
            skillIds: skillIds
        )
    }

    func uploadImage(data: Data, mimeType: String) async throws -> String {
        try await safeCore.uploadImage(data: data, mimeType: mimeType)
    }
}

extension DraftManager: DraftPersisting {}
extension KeychainService: CredentialStoring {}
extension NotificationService: NotificationScheduling {}
