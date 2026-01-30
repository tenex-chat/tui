import Foundation

/// Thread-safe actor wrapper around TenexCore that provides:
/// - Serialized FFI access (thread safety via actor isolation)
/// - Proper error handling (no force unwraps exposed to callers)
/// - Async interface for modern Swift concurrency
///
/// ## Usage
/// ```swift
/// let safeCore = SafeTenexCore(core: tenexCore)
/// let projects = try await safeCore.getProjects()
/// ```
///
/// ## Known Limitations
/// Some methods still have internal `try!` in the auto-generated FFI code.
/// These can still crash on FFI errors. The only fix would be modifying
/// the Rust FFI to return Result types. Methods with internal force unwraps:
/// - `getProjects()`, `getMessages()`, `getConversations()`, `getInbox()`
/// - `getConversationRuntimeMs()`, `getTodayRuntimeMs()`
/// - `getDiagnosticsSnapshot()`, `refresh()`, `init()`
actor SafeTenexCore: SafeTenexCoreProtocol {
    private let core: TenexCore

    init(core: TenexCore) {
        self.core = core
    }

    // MARK: - Core Lifecycle

    /// Refresh data from relays.
    /// Returns true if refresh was performed, false if throttled.
    /// Note: Internal `try!` - can crash on FFI error.
    func refresh() -> Bool {
        core.refresh()
    }

    // MARK: - Projects

    /// Get all projects.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getProjects() -> [ProjectInfo] {
        core.getProjects()
    }

    /// Get project filters with visibility and counts.
    func getProjectFilters() throws -> [ProjectFilterInfo] {
        do {
            return try core.getProjectFilters()
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    // MARK: - Conversations

    /// Get all conversations with filtering.
    func getAllConversations(filter: ConversationFilter) throws -> [ConversationFullInfo] {
        do {
            return try core.getAllConversations(filter: filter)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Get conversations by their IDs.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getConversationsByIds(conversationIds: [String]) -> [ConversationFullInfo] {
        core.getConversationsByIds(conversationIds: conversationIds)
    }

    /// Get conversations for a project.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getConversations(projectId: String) -> [ConversationInfo] {
        core.getConversations(projectId: projectId)
    }

    /// Get all descendant conversation IDs for a conversation.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getDescendantConversationIds(conversationId: String) -> [String] {
        core.getDescendantConversationIds(conversationId: conversationId)
    }

    /// Get archived conversation IDs.
    func getArchivedConversationIds() throws -> [String] {
        do {
            return try core.getArchivedConversationIds()
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Check if a conversation is archived.
    /// Note: Internal `try!` in FFI - can crash on error.
    func isConversationArchived(conversationId: String) -> Bool {
        core.isConversationArchived(conversationId: conversationId)
    }

    /// Archive a conversation.
    /// Note: Internal `try!` in FFI - can crash on error.
    func archiveConversation(conversationId: String) {
        core.archiveConversation(conversationId: conversationId)
    }

    /// Unarchive a conversation.
    /// Note: Internal `try!` in FFI - can crash on error.
    func unarchiveConversation(conversationId: String) {
        core.unarchiveConversation(conversationId: conversationId)
    }

    /// Toggle conversation archived status.
    /// Note: Internal `try!` in FFI - can crash on error.
    func toggleConversationArchived(conversationId: String) -> Bool {
        core.toggleConversationArchived(conversationId: conversationId)
    }

    // MARK: - Messages

    /// Get messages for a conversation.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getMessages(conversationId: String) -> [MessageInfo] {
        core.getMessages(conversationId: conversationId)
    }

    /// Send a message to an existing conversation.
    func sendMessage(conversationId: String, projectId: String, content: String, agentPubkey: String?) throws -> SendMessageResult {
        do {
            return try core.sendMessage(conversationId: conversationId, projectId: projectId, content: content, agentPubkey: agentPubkey)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Send a new conversation thread.
    func sendThread(projectId: String, title: String, content: String, agentPubkey: String?, nudgeIds: [String]) throws -> SendMessageResult {
        do {
            return try core.sendThread(projectId: projectId, title: title, content: content, agentPubkey: agentPubkey, nudgeIds: nudgeIds)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Answer an ask event with formatted responses.
    func answerAsk(askEventId: String, askAuthorPubkey: String, conversationId: String, projectId: String, answers: [AskAnswer]) throws -> SendMessageResult {
        do {
            return try core.answerAsk(askEventId: askEventId, askAuthorPubkey: askAuthorPubkey, conversationId: conversationId, projectId: projectId, answers: answers)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    // MARK: - Inbox

    /// Get inbox items.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getInbox() -> [InboxItem] {
        core.getInbox()
    }

    // MARK: - Search

    /// Full-text search across all events.
    /// Note: Internal `try!` in FFI - can crash on error.
    func search(query: String, limit: Int32) -> [SearchResult] {
        core.search(query: query, limit: limit)
    }

    // MARK: - Agents

    /// Get agents for a project.
    func getAgents(projectId: String) throws -> [AgentInfo] {
        do {
            return try core.getAgents(projectId: projectId)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Get all agents.
    func getAllAgents() throws -> [AgentInfo] {
        do {
            return try core.getAllAgents()
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Get online agents from project status (kind:24010).
    /// These are actual agent instances with their own Nostr keypairs.
    func getOnlineAgents(projectId: String) throws -> [OnlineAgentInfo] {
        do {
            return try core.getOnlineAgents(projectId: projectId)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Get all nudges (kind:4201 events).
    /// Returns nudges sorted by created_at descending (most recent first).
    func getNudges() throws -> [NudgeInfo] {
        do {
            return try core.getNudges()
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Get available configuration options for a project.
    /// Returns all available models and tools from the project status.
    func getProjectConfigOptions(projectId: String) throws -> ProjectConfigOptions {
        do {
            return try core.getProjectConfigOptions(projectId: projectId)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Update an agent's configuration (model and tools).
    /// Publishes a kind:24020 event to update the agent's config.
    func updateAgentConfig(projectId: String, agentPubkey: String, model: String?, tools: [String]) throws {
        do {
            try core.updateAgentConfig(projectId: projectId, agentPubkey: agentPubkey, model: model, tools: tools)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    // MARK: - Backend Trust

    /// Set the trusted backends from preferences.
    /// Must be called after init to enable processing of kind:24010 events.
    func setTrustedBackends(approved: [String], blocked: [String]) throws {
        do {
            try core.setTrustedBackends(approved: approved, blocked: blocked)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Add a backend to the approved list.
    func approveBackend(pubkey: String) throws {
        do {
            try core.approveBackend(pubkey: pubkey)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Add a backend to the blocked list.
    func blockBackend(pubkey: String) throws {
        do {
            try core.blockBackend(pubkey: pubkey)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Approve all pending backends.
    /// Useful for mobile apps without a backend approval UI.
    func approveAllPendingBackends() throws -> UInt32 {
        do {
            return try core.approveAllPendingBackends()
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    // MARK: - Stats & Diagnostics

    /// Get stats snapshot.
    func getStatsSnapshot() throws -> StatsSnapshot {
        do {
            return try core.getStatsSnapshot()
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Get diagnostics snapshot.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getDiagnosticsSnapshot(includeDatabaseStats: Bool) -> DiagnosticsSnapshot {
        core.getDiagnosticsSnapshot(includeDatabaseStats: includeDatabaseStats)
    }

    /// Get conversation runtime in milliseconds.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getConversationRuntimeMs(conversationId: String) -> UInt64 {
        core.getConversationRuntimeMs(conversationId: conversationId)
    }

    /// Get today's runtime in milliseconds.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getTodayRuntimeMs() -> UInt64 {
        core.getTodayRuntimeMs()
    }

    // MARK: - Authentication

    /// Login with nsec.
    func login(nsec: String) throws -> LoginResult {
        do {
            return try core.login(nsec: nsec)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Logout the current user.
    func logout() throws {
        do {
            try core.logout()
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Check if logged in.
    /// Note: Internal `try!` in FFI - can crash on error.
    func isLoggedIn() -> Bool {
        core.isLoggedIn()
    }

    /// Get current user info.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getCurrentUser() -> UserInfo? {
        core.getCurrentUser()
    }

    // MARK: - Profile

    /// Get profile name for a pubkey.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getProfileName(pubkey: String) -> String {
        core.getProfileName(pubkey: pubkey)
    }

    /// Get profile picture URL for a pubkey.
    func getProfilePicture(pubkey: String) throws -> String? {
        do {
            return try core.getProfilePicture(pubkey: pubkey)
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    // MARK: - Thread Collapse State

    /// Get collapsed thread IDs.
    func getCollapsedThreadIds() throws -> [String] {
        do {
            return try core.getCollapsedThreadIds()
        } catch let error as TenexError {
            throw CoreError.tenex(error)
        }
    }

    /// Check if a thread is collapsed.
    /// Note: Internal `try!` in FFI - can crash on error.
    func isThreadCollapsed(threadId: String) -> Bool {
        core.isThreadCollapsed(threadId: threadId)
    }

    /// Toggle thread collapsed state.
    /// Note: Internal `try!` in FFI - can crash on error.
    func toggleThreadCollapsed(threadId: String) -> Bool {
        core.toggleThreadCollapsed(threadId: threadId)
    }

    /// Set collapsed thread IDs.
    /// Note: Internal `try!` in FFI - can crash on error.
    func setCollapsedThreadIds(threadIds: [String]) {
        core.setCollapsedThreadIds(threadIds: threadIds)
    }

    // MARK: - Project Visibility

    /// Set visible projects.
    /// Note: Internal `try!` in FFI - can crash on error.
    func setVisibleProjects(projectATags: [String]) {
        core.setVisibleProjects(projectATags: projectATags)
    }

    // MARK: - Reports

    /// Get reports for a project.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getReports(projectId: String) -> [ReportInfo] {
        core.getReports(projectId: projectId)
    }

    // MARK: - Misc

    /// Get version string.
    /// Note: Internal `try!` in FFI - can crash on error.
    func version() -> String {
        core.version()
    }
}
