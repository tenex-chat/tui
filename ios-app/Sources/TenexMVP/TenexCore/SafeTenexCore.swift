import Foundation

/// Thread-safe actor wrapper around TenexCore that provides:
/// - Serialized FFI access (thread safety via actor isolation)
/// - Proper error handling (no force unwraps exposed to callers)
/// - Async interface for modern Swift concurrency
/// - **FFI Performance Profiling** - All calls are timed and logged via PerformanceProfiler
///
/// ## Usage
/// ```swift
/// let safeCore = SafeTenexCore(core: tenexCore)
/// let projects = try await safeCore.getProjects()
/// ```
///
/// ## Profiling
/// All FFI calls are instrumented with `PerformanceProfiler.shared.measureFFI()`.
/// View aggregate metrics in the in-app ProfilingView or via Console.app:
/// ```bash
/// log stream --predicate 'subsystem == "com.tenex.app" AND category == "FFI"'
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
    private let profiler = PerformanceProfiler.shared

    init(core: TenexCore) {
        self.core = core
    }

    // MARK: - Core Lifecycle

    /// Refresh data from relays.
    /// Returns true if refresh was performed, false if throttled.
    /// Note: Internal `try!` - can crash on FFI error.
    func refresh() -> Bool {
        profiler.measureFFI("refresh") {
            core.refresh()
        }
    }

    // MARK: - Projects

    /// Get all projects.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getProjects() -> [ProjectInfo] {
        profiler.measureFFI("getProjects") {
            core.getProjects()
        }
    }

    /// Get project filters with visibility and counts.
    func getProjectFilters() throws -> [ProjectFilterInfo] {
        try profiler.measureFFI("getProjectFilters") {
            do {
                return try core.getProjectFilters()
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    // MARK: - Conversations

    /// Get all conversations with filtering.
    func getAllConversations(filter: ConversationFilter) throws -> [ConversationFullInfo] {
        try profiler.measureFFI("getAllConversations") {
            do {
                return try core.getAllConversations(filter: filter)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Get conversations by their IDs.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getConversationsByIds(conversationIds: [String]) -> [ConversationFullInfo] {
        profiler.measureFFI("getConversationsByIds(\(conversationIds.count))") {
            core.getConversationsByIds(conversationIds: conversationIds)
        }
    }

    /// Get conversations for a project.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getConversations(projectId: String) -> [ConversationInfo] {
        profiler.measureFFI("getConversations") {
            core.getConversations(projectId: projectId)
        }
    }

    /// Get all descendant conversation IDs for a conversation.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getDescendantConversationIds(conversationId: String) -> [String] {
        profiler.measureFFI("getDescendantConversationIds") {
            core.getDescendantConversationIds(conversationId: conversationId)
        }
    }

    /// Get archived conversation IDs.
    func getArchivedConversationIds() throws -> [String] {
        try profiler.measureFFI("getArchivedConversationIds") {
            do {
                return try core.getArchivedConversationIds()
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Check if a conversation is archived.
    /// Note: Internal `try!` in FFI - can crash on error.
    func isConversationArchived(conversationId: String) -> Bool {
        profiler.measureFFI("isConversationArchived") {
            core.isConversationArchived(conversationId: conversationId)
        }
    }

    /// Archive a conversation.
    /// Note: Internal `try!` in FFI - can crash on error.
    func archiveConversation(conversationId: String) {
        profiler.measureFFI("archiveConversation") {
            core.archiveConversation(conversationId: conversationId)
        }
    }

    /// Unarchive a conversation.
    /// Note: Internal `try!` in FFI - can crash on error.
    func unarchiveConversation(conversationId: String) {
        profiler.measureFFI("unarchiveConversation") {
            core.unarchiveConversation(conversationId: conversationId)
        }
    }

    /// Toggle conversation archived status.
    /// Note: Internal `try!` in FFI - can crash on error.
    func toggleConversationArchived(conversationId: String) -> Bool {
        profiler.measureFFI("toggleConversationArchived") {
            core.toggleConversationArchived(conversationId: conversationId)
        }
    }

    // MARK: - Messages

    /// Get messages for a conversation.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getMessages(conversationId: String) -> [MessageInfo] {
        profiler.measureFFI("getMessages") {
            core.getMessages(conversationId: conversationId)
        }
    }

    /// Send a message to an existing conversation.
    func sendMessage(conversationId: String, projectId: String, content: String, agentPubkey: String?) throws -> SendMessageResult {
        try profiler.measureFFI("sendMessage") {
            do {
                return try core.sendMessage(conversationId: conversationId, projectId: projectId, content: content, agentPubkey: agentPubkey)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Send a new conversation thread.
    func sendThread(projectId: String, title: String, content: String, agentPubkey: String?, nudgeIds: [String]) throws -> SendMessageResult {
        try profiler.measureFFI("sendThread") {
            do {
                return try core.sendThread(projectId: projectId, title: title, content: content, agentPubkey: agentPubkey, nudgeIds: nudgeIds)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Answer an ask event with formatted responses.
    func answerAsk(askEventId: String, askAuthorPubkey: String, conversationId: String, projectId: String, answers: [AskAnswer]) throws -> SendMessageResult {
        try profiler.measureFFI("answerAsk") {
            do {
                return try core.answerAsk(askEventId: askEventId, askAuthorPubkey: askAuthorPubkey, conversationId: conversationId, projectId: projectId, answers: answers)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    // MARK: - Inbox

    /// Get inbox items.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getInbox() -> [InboxItem] {
        profiler.measureFFI("getInbox") {
            core.getInbox()
        }
    }

    // MARK: - Search

    /// Full-text search across all events.
    /// Note: Internal `try!` in FFI - can crash on error.
    func search(query: String, limit: Int32) -> [SearchResult] {
        profiler.measureFFI("search") {
            core.search(query: query, limit: limit)
        }
    }

    // MARK: - Agents

    /// Get agents for a project.
    func getAgents(projectId: String) throws -> [AgentInfo] {
        try profiler.measureFFI("getAgents") {
            do {
                return try core.getAgents(projectId: projectId)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Get all agents.
    func getAllAgents() throws -> [AgentInfo] {
        try profiler.measureFFI("getAllAgents") {
            do {
                return try core.getAllAgents()
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Get online agents from project status (kind:24010).
    /// These are actual agent instances with their own Nostr keypairs.
    func getOnlineAgents(projectId: String) throws -> [OnlineAgentInfo] {
        try profiler.measureFFI("getOnlineAgents") {
            do {
                return try core.getOnlineAgents(projectId: projectId)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Get all nudges (kind:4201 events).
    /// Returns nudges sorted by created_at descending (most recent first).
    func getNudges() throws -> [NudgeInfo] {
        try profiler.measureFFI("getNudges") {
            do {
                return try core.getNudges()
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Get available configuration options for a project.
    /// Returns all available models and tools from the project status.
    func getProjectConfigOptions(projectId: String) throws -> ProjectConfigOptions {
        try profiler.measureFFI("getProjectConfigOptions") {
            do {
                return try core.getProjectConfigOptions(projectId: projectId)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Update an agent's configuration (model and tools).
    /// Publishes a kind:24020 event to update the agent's config.
    func updateAgentConfig(projectId: String, agentPubkey: String, model: String?, tools: [String]) throws {
        try profiler.measureFFI("updateAgentConfig") {
            do {
                try core.updateAgentConfig(projectId: projectId, agentPubkey: agentPubkey, model: model, tools: tools)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    // MARK: - Project Status

    /// Check if a project is online (has a recent kind:24010 status event).
    /// A project is considered online if it has a non-stale status from an approved backend.
    /// Note: Internal `try!` in FFI - can crash on error.
    func isProjectOnline(projectId: String) -> Bool {
        profiler.measureFFI("isProjectOnline") {
            core.isProjectOnline(projectId: projectId)
        }
    }

    /// Boot/start a project (sends kind:24000 event).
    /// This sends a boot request to wake up the project's backend.
    func bootProject(projectId: String) throws {
        try profiler.measureFFI("bootProject") {
            do {
                try core.bootProject(projectId: projectId)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    // MARK: - Backend Trust

    /// Set the trusted backends from preferences.
    /// Must be called after init to enable processing of kind:24010 events.
    func setTrustedBackends(approved: [String], blocked: [String]) throws {
        try profiler.measureFFI("setTrustedBackends") {
            do {
                try core.setTrustedBackends(approved: approved, blocked: blocked)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Add a backend to the approved list.
    func approveBackend(pubkey: String) throws {
        try profiler.measureFFI("approveBackend") {
            do {
                try core.approveBackend(pubkey: pubkey)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Add a backend to the blocked list.
    func blockBackend(pubkey: String) throws {
        try profiler.measureFFI("blockBackend") {
            do {
                try core.blockBackend(pubkey: pubkey)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Approve all pending backends.
    /// Useful for mobile apps without a backend approval UI.
    func approveAllPendingBackends() throws -> UInt32 {
        try profiler.measureFFI("approveAllPendingBackends") {
            do {
                return try core.approveAllPendingBackends()
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    // MARK: - Stats & Diagnostics

    /// Get stats snapshot.
    func getStatsSnapshot() throws -> StatsSnapshot {
        try profiler.measureFFI("getStatsSnapshot") {
            do {
                return try core.getStatsSnapshot()
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Get diagnostics snapshot.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getDiagnosticsSnapshot(includeDatabaseStats: Bool) -> DiagnosticsSnapshot {
        profiler.measureFFI("getDiagnosticsSnapshot") {
            core.getDiagnosticsSnapshot(includeDatabaseStats: includeDatabaseStats)
        }
    }

    /// Get conversation runtime in milliseconds.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getConversationRuntimeMs(conversationId: String) -> UInt64 {
        profiler.measureFFI("getConversationRuntimeMs") {
            core.getConversationRuntimeMs(conversationId: conversationId)
        }
    }

    /// Get today's runtime in milliseconds.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getTodayRuntimeMs() -> UInt64 {
        profiler.measureFFI("getTodayRuntimeMs") {
            core.getTodayRuntimeMs()
        }
    }

    // MARK: - Authentication

    /// Login with nsec.
    func login(nsec: String) throws -> LoginResult {
        try profiler.measureFFI("login") {
            do {
                return try core.login(nsec: nsec)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Logout the current user.
    func logout() throws {
        try profiler.measureFFI("logout") {
            do {
                try core.logout()
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Check if logged in.
    /// Note: Internal `try!` in FFI - can crash on error.
    func isLoggedIn() -> Bool {
        profiler.measureFFI("isLoggedIn") {
            core.isLoggedIn()
        }
    }

    /// Get current user info.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getCurrentUser() -> UserInfo? {
        profiler.measureFFI("getCurrentUser") {
            core.getCurrentUser()
        }
    }

    // MARK: - Profile

    /// Get profile name for a pubkey.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getProfileName(pubkey: String) -> String {
        profiler.measureFFI("getProfileName") {
            core.getProfileName(pubkey: pubkey)
        }
    }

    /// Get profile picture URL for a pubkey.
    func getProfilePicture(pubkey: String) throws -> String? {
        try profiler.measureFFI("getProfilePicture") {
            do {
                return try core.getProfilePicture(pubkey: pubkey)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    // MARK: - Thread Collapse State

    /// Get collapsed thread IDs.
    func getCollapsedThreadIds() throws -> [String] {
        try profiler.measureFFI("getCollapsedThreadIds") {
            do {
                return try core.getCollapsedThreadIds()
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Check if a thread is collapsed.
    /// Note: Internal `try!` in FFI - can crash on error.
    func isThreadCollapsed(threadId: String) -> Bool {
        profiler.measureFFI("isThreadCollapsed") {
            core.isThreadCollapsed(threadId: threadId)
        }
    }

    /// Toggle thread collapsed state.
    /// Note: Internal `try!` in FFI - can crash on error.
    func toggleThreadCollapsed(threadId: String) -> Bool {
        profiler.measureFFI("toggleThreadCollapsed") {
            core.toggleThreadCollapsed(threadId: threadId)
        }
    }

    /// Set collapsed thread IDs.
    /// Note: Internal `try!` in FFI - can crash on error.
    func setCollapsedThreadIds(threadIds: [String]) {
        profiler.measureFFI("setCollapsedThreadIds") {
            core.setCollapsedThreadIds(threadIds: threadIds)
        }
    }

    // MARK: - Project Visibility

    /// Set visible projects.
    /// Note: Internal `try!` in FFI - can crash on error.
    func setVisibleProjects(projectATags: [String]) {
        profiler.measureFFI("setVisibleProjects") {
            core.setVisibleProjects(projectATags: projectATags)
        }
    }

    // MARK: - Reports

    /// Get reports for a project.
    /// Note: Internal `try!` in FFI - can crash on error.
    func getReports(projectId: String) -> [ReportInfo] {
        profiler.measureFFI("getReports") {
            core.getReports(projectId: projectId)
        }
    }

    // MARK: - AI Audio Settings

    /// Get AI audio settings (API keys never exposed - only configuration status)
    func getAiAudioSettings() throws -> AiAudioSettings {
        try profiler.measureFFI("getAiAudioSettings") {
            do {
                let settings = try core.getAiAudioSettings()
                return AiAudioSettings(
                    elevenlabsApiKeyConfigured: settings.elevenlabsApiKeyConfigured,
                    openrouterApiKeyConfigured: settings.openrouterApiKeyConfigured,
                    selectedVoiceIds: settings.selectedVoiceIds,
                    openrouterModel: settings.openrouterModel,
                    audioPrompt: settings.audioPrompt,
                    enabled: settings.enabled
                )
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Set ElevenLabs API key (stored in OS secure storage)
    func setElevenLabsApiKey(key: String?) throws {
        try profiler.measureFFI("setElevenLabsApiKey") {
            do {
                try core.setElevenLabsApiKey(key: key)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Set OpenRouter API key (stored in OS secure storage)
    func setOpenRouterApiKey(key: String?) throws {
        try profiler.measureFFI("setOpenRouterApiKey") {
            do {
                try core.setOpenRouterApiKey(key: key)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Set whether audio notifications are enabled
    func setAiAudioEnabled(enabled: Bool) throws {
        try profiler.measureFFI("setAiAudioEnabled") {
            do {
                try core.setAiAudioEnabled(enabled: enabled)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Set audio prompt template
    func setAiAudioPrompt(prompt: String) throws {
        try profiler.measureFFI("setAiAudioPrompt") {
            do {
                try core.setAiAudioPrompt(prompt: prompt)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Set OpenRouter model to use for text massage
    func setOpenRouterModel(model: String?) throws {
        try profiler.measureFFI("setOpenRouterModel") {
            do {
                try core.setOpenRouterModel(model: model)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Set selected voice IDs whitelist
    func setSelectedVoiceIds(voiceIds: [String]) throws {
        try profiler.measureFFI("setSelectedVoiceIds") {
            do {
                try core.setSelectedVoiceIds(voiceIds: voiceIds)
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Fetch available voices from ElevenLabs
    /// Note: This is a blocking call that makes a network request
    func fetchElevenLabsVoices() throws -> [VoiceInfo] {
        try profiler.measureFFI("fetchElevenLabsVoices") {
            do {
                let voices = try core.fetchElevenLabsVoices()
                return voices.map { voice in
                    VoiceInfo(
                        voiceId: voice.voiceId,
                        name: voice.name,
                        category: voice.category,
                        description: voice.description
                    )
                }
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    /// Fetch available models from OpenRouter
    /// Note: This is a blocking call that makes a network request
    func fetchOpenRouterModels() throws -> [ModelInfo] {
        try profiler.measureFFI("fetchOpenRouterModels") {
            do {
                let models = try core.fetchOpenRouterModels()
                return models.map { model in
                    ModelInfo(
                        modelId: model.id,
                        name: model.name ?? model.id,
                        description: model.description,
                        contextLength: model.contextLength
                    )
                }
            } catch let error as TenexError {
                throw CoreError.tenex(error)
            }
        }
    }

    // MARK: - Misc

    /// Get version string.
    /// Note: Internal `try!` in FFI - can crash on error.
    func version() -> String {
        profiler.measureFFI("version") {
            core.version()
        }
    }
}
