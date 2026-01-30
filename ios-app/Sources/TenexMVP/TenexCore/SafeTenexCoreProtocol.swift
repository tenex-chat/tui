import Foundation

/// Protocol matching SafeTenexCore's interface for mock testing.
/// All methods are async to match the actor-based implementation.
protocol SafeTenexCoreProtocol: Actor {
    // MARK: - Core Lifecycle
    func refresh() -> Bool

    // MARK: - Projects
    func getProjects() -> [ProjectInfo]
    func getProjectFilters() throws -> [ProjectFilterInfo]

    // MARK: - Conversations
    func getAllConversations(filter: ConversationFilter) throws -> [ConversationFullInfo]
    func getConversationsByIds(conversationIds: [String]) -> [ConversationFullInfo]
    func getConversations(projectId: String) -> [ConversationInfo]
    func getDescendantConversationIds(conversationId: String) -> [String]
    func getArchivedConversationIds() throws -> [String]
    func isConversationArchived(conversationId: String) -> Bool
    func archiveConversation(conversationId: String)
    func unarchiveConversation(conversationId: String)
    func toggleConversationArchived(conversationId: String) -> Bool

    // MARK: - Messages
    func getMessages(conversationId: String) -> [MessageInfo]
    func sendMessage(conversationId: String, projectId: String, content: String, agentPubkey: String?) throws -> SendMessageResult
    func sendThread(projectId: String, title: String, content: String, agentPubkey: String?) throws -> SendMessageResult

    // MARK: - Inbox
    func getInbox() -> [InboxItem]

    // MARK: - Agents
    func getAgents(projectId: String) throws -> [AgentInfo]
    func getAllAgents() throws -> [AgentInfo]

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
    func getReports(projectId: String) -> [ReportInfo]

    // MARK: - Misc
    func version() -> String
}
