import Foundation

/// Protocol matching SafeTenexCore's interface for mock testing.
/// All methods are async to match the actor-based implementation.
protocol SafeTenexCoreProtocol: Actor {
    // MARK: - Core Lifecycle
    func refresh() -> Bool
    func forceReconnect() throws

    // MARK: - Projects
    func getProjects() -> [Project]
    func getProjectFilters() throws -> [ProjectFilterInfo]

    // MARK: - Conversations
    func getAllConversations(filter: ConversationFilter) throws -> [ConversationFullInfo]
    func getConversationsByIds(conversationIds: [String]) -> [ConversationFullInfo]
    func getConversations(projectId: String) -> [ConversationFullInfo]
    func getDescendantConversationIds(conversationId: String) -> [String]
    func getArchivedConversationIds() throws -> [String]
    func isConversationArchived(conversationId: String) -> Bool
    func archiveConversation(conversationId: String)
    func unarchiveConversation(conversationId: String)
    func toggleConversationArchived(conversationId: String) -> Bool

    // MARK: - Messages
    func getMessages(conversationId: String) -> [Message]
    func getAskEventById(eventId: String) -> AskEventLookupInfo?
    func sendMessage(conversationId: String, projectId: String, content: String, agentPubkey: String?, nudgeIds: [String], skillIds: [String]) throws -> SendMessageResult
    func sendThread(projectId: String, title: String, content: String, agentPubkey: String?, nudgeIds: [String], skillIds: [String]) throws -> SendMessageResult
    func answerAsk(askEventId: String, askAuthorPubkey: String, conversationId: String, projectId: String, answers: [AskAnswer]) throws -> SendMessageResult

    // MARK: - Inbox
    func getInbox() -> [InboxItem]

    // MARK: - Search
    func search(query: String, limit: Int32) -> [SearchResult]

    // MARK: - Agents
    func getAgents(projectId: String) throws -> [AgentDefinition]
    func getAllAgents() throws -> [AgentDefinition]
    func getAllMcpTools() throws -> [McpTool]
    func getOnlineAgents(projectId: String) throws -> [ProjectAgent]
    func getProjectConfigOptions(projectId: String) throws -> ProjectConfigOptions
    func updateAgentConfig(projectId: String, agentPubkey: String, model: String?, tools: [String], tags: [String]) throws
    func updateGlobalAgentConfig(agentPubkey: String, model: String?, tools: [String], tags: [String]) throws
    func createAgentDefinition(
        name: String,
        description: String,
        role: String,
        instructions: String,
        version: String,
        sourceId: String?,
        isFork: Bool
    ) throws
    func deleteAgentDefinition(agentId: String) throws
    func updateProject(
        projectId: String,
        title: String,
        description: String,
        repoUrl: String?,
        pictureUrl: String?,
        agentDefinitionIds: [String],
        mcpToolIds: [String]
    ) throws
    func deleteProject(projectId: String) throws

    // MARK: - Teams
    func getAllTeams() throws -> [TeamInfo]
    func getTeamComments(teamCoordinate: String, teamEventId: String) throws -> [TeamCommentInfo]
    func reactToTeam(teamCoordinate: String, teamEventId: String, teamPubkey: String, isLike: Bool) throws -> String
    func postTeamComment(
        teamCoordinate: String,
        teamEventId: String,
        teamPubkey: String,
        content: String,
        parentCommentId: String?,
        parentCommentPubkey: String?
    ) throws -> String

    // MARK: - Project Status
    func isProjectOnline(projectId: String) -> Bool
    func bootProject(projectId: String) throws

    // MARK: - Nudges
    func getNudges() throws -> [Nudge]
    func getSkills() throws -> [Skill]


    // MARK: - Backend Trust
    func setTrustedBackends(approved: [String], blocked: [String]) throws
    func approveBackend(pubkey: String) throws
    func blockBackend(pubkey: String) throws
    func approveAllPendingBackends() throws -> UInt32
    func getBackendTrustSnapshot() throws -> BackendTrustSnapshot
    func getConfiguredRelays() -> [String]

    // MARK: - Stats & Diagnostics
    func getStatsSnapshot() throws -> StatsSnapshot
    func getDiagnosticsSnapshot(includeDatabaseStats: Bool) -> DiagnosticsSnapshot
    func getConversationRuntimeMs(conversationId: String) -> UInt64
    func getTodayRuntimeMs() -> UInt64

    // MARK: - Authentication
    func login(nsec: String) throws -> LoginResult
    func logout() throws
    func isLoggedIn() -> Bool
    func getCurrentUser() -> UserInfo?

    // MARK: - Profile
    func getProfileName(pubkey: String) -> String
    func getProfilePicture(pubkey: String) throws -> String?

    // MARK: - Thread Collapse State
    func getCollapsedThreadIds() throws -> [String]
    func isThreadCollapsed(threadId: String) -> Bool
    func toggleThreadCollapsed(threadId: String) -> Bool
    func setCollapsedThreadIds(threadIds: [String])

    // MARK: - Project Visibility
    func setVisibleProjects(projectATags: [String])

    // MARK: - Reports
    func getReports(projectId: String) -> [Report]

    // MARK: - AI Audio Settings
    func getAiAudioSettings() throws -> AiAudioSettings
    func setAudioNotificationsEnabled(enabled: Bool) throws
    func setTtsInactivityThreshold(secs: UInt64) throws
    func setAudioPrompt(prompt: String) throws
    func setOpenRouterModel(model: String?) throws
    func setSelectedVoiceIds(voiceIds: [String]) throws
    func generateAudioNotification(agentPubkey: String, conversationTitle: String, messageText: String, elevenlabsApiKey: String, openrouterApiKey: String) throws -> AudioNotificationInfo

    // MARK: - Misc
    func version() -> String

    // MARK: - Image Upload
    func uploadImage(data: Data, mimeType: String) throws -> String
}
