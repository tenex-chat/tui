import SwiftUI
import CryptoKit
import Combine
import UserNotifications

// MARK: - Streaming Buffer

struct StreamingBuffer {
    let agentPubkey: String
    var text: String
}

struct PendingStreamingDelta {
    let agentPubkey: String
    var text: String
    var chunkCount: Int
    let startedAt: CFAbsoluteTime
}

// MARK: - Profile Picture Cache

/// Thread-safe cache for profile picture URLs to prevent repeated synchronous FFI calls during scroll.
/// Each pubkey's picture URL is fetched once and cached for the session lifetime.
final class ProfilePictureCache: @unchecked Sendable {
    static let shared = ProfilePictureCache()

    private var cache: [String: String?] = [:]
    private let lock = NSLock()

    private init() {}

    /// Get cached profile picture URL for a pubkey.
    /// Returns nil if not cached (call fetch to populate).
    func getCached(_ pubkey: String) -> String?? {
        lock.lock()
        defer { lock.unlock() }

        if cache.keys.contains(pubkey) {
            return cache[pubkey]
        }
        return nil // Not in cache (different from cached nil)
    }

    /// Store a profile picture URL in the cache.
    /// Pass nil to cache "no picture available" for this pubkey.
    func store(_ pubkey: String, pictureUrl: String?) {
        lock.lock()
        defer { lock.unlock() }
        cache[pubkey] = pictureUrl
    }

    /// Clear the entire cache (e.g., on logout)
    func clear() {
        lock.lock()
        defer { lock.unlock() }
        cache.removeAll()
    }

    /// Get the number of cached entries (for debugging)
    var count: Int {
        lock.lock()
        defer { lock.unlock() }
        return cache.count
    }
}

/// Shared TenexCore instance wrapper for environment object
/// Initializes the core OFF the main thread to avoid UI jank
@MainActor
class TenexCoreManager: ObservableObject {
    let core: TenexCore
    /// Thread-safe async wrapper for FFI access with proper error handling
    let safeCore: SafeTenexCore
    let profiler = PerformanceProfiler.shared
    @Published var isInitialized = false
    @Published var initializationError: String?

    // MARK: - Centralized Reactive Data Store
    // These granular @Published properties enable targeted updates without full UI refresh.
    // Views that observe specific properties only re-render when those properties change.
    @Published var projects: [ProjectInfo] = []
    @Published var conversations: [ConversationFullInfo] = []
    @Published var inboxItems: [InboxItem] = []
    @Published var reports: [ReportInfo] = []
    @Published var messagesByConversation: [String: [MessageInfo]] = [:]
    private(set) var statsVersion: UInt64 = 0
    private(set) var teamsVersion: UInt64 = 0
    private(set) var diagnosticsVersion: UInt64 = 0
    private let statsVersionSubject = PassthroughSubject<UInt64, Never>()
    private let teamsVersionSubject = PassthroughSubject<UInt64, Never>()
    private let diagnosticsVersionSubject = PassthroughSubject<UInt64, Never>()
    @Published var streamingBuffers: [String: StreamingBuffer] = [:]

    var statsVersionPublisher: AnyPublisher<UInt64, Never> {
        statsVersionSubject.eraseToAnyPublisher()
    }

    var teamsVersionPublisher: AnyPublisher<UInt64, Never> {
        teamsVersionSubject.eraseToAnyPublisher()
    }

    var diagnosticsVersionPublisher: AnyPublisher<UInt64, Never> {
        diagnosticsVersionSubject.eraseToAnyPublisher()
    }

    /// Project online status - updated reactively via event callbacks.
    /// Key: project ID, Value: true if online.
    /// Subscribe to this instead of polling isProjectOnline().
    @Published var projectOnlineStatus: [String: Bool] = [:]

    /// Online agents for each project - updated reactively via event callbacks.
    /// Key: project ID, Value: array of OnlineAgentInfo.
    /// Subscribe to this instead of fetching agents on-demand via getOnlineAgents().
    /// This eliminates multi-second delays from redundant FFI calls.
    @Published var onlineAgents: [String: [OnlineAgentInfo]] = [:]

    /// Whether any conversation currently has active agents (24133 events with agents)
    /// Used to highlight the runtime indicator when work is happening
    @Published var hasActiveAgents: Bool = false

    /// Last project ID tombstoned via a push upsert.
    /// Used by view selection state to clear deleted-project detail panes immediately.
    @Published private(set) var lastDeletedProjectId: String?

    // MARK: - Global App Filter

    @Published var appFilterProjectIds: Set<String>
    @Published var appFilterTimeWindow: AppTimeWindow

    static let appFilterProjectsDefaultsKey = "app.global.filter.projectIds"
    static let appFilterTimeWindowDefaultsKey = "app.global.filter.timeWindow"

    // MARK: - Ask Badge Support

    /// Count of unanswered ask events within the selected global filter scope.
    @Published private(set) var unansweredAskCount: Int = 0

    private func computeUnansweredAskCount(now: UInt64? = nil) -> Int {
        let resolvedNow = now ?? UInt64(Date().timeIntervalSince1970)
        let snapshot = appFilterSnapshot
        var count = 0

        for item in inboxItems {
            guard item.eventType == "ask", item.status == "waiting" else { continue }
            if snapshot.includes(projectId: item.projectId, timestamp: item.createdAt, now: resolvedNow) {
                count += 1
            }
        }

        return count
    }

    @MainActor
    func refreshUnansweredAskCount(reason: String) {
        let startedAt = CFAbsoluteTimeGetCurrent()
        let oldCount = unansweredAskCount
        let newCount = computeUnansweredAskCount()

        if newCount != oldCount {
            unansweredAskCount = newCount
            profiler.logEvent(
                "unansweredAskCount updated old=\(oldCount) new=\(newCount) reason=\(reason)",
                category: .general,
                level: .debug
            )
        }

        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        if elapsedMs >= 8 {
            profiler.logEvent(
                "refreshUnansweredAskCount slow elapsedMs=\(String(format: "%.2f", elapsedMs)) reason=\(reason) items=\(inboxItems.count)",
                category: .general,
                level: .error
            )
        }
    }

    // MARK: - Event Callback
    /// Event handler for push-based updates from Rust core
    var eventHandler: TenexEventHandler?

    /// Task reference for project status updates - enables cancellation of stale refreshes
    var projectStatusUpdateTask: Task<Void, Never>?
    /// Task reference for app-filtered conversation refreshes
    var conversationRefreshTask: Task<Void, Never>?
    /// Coalesced streaming chunks pending publish to reduce SwiftUI invalidation storms.
    var pendingStreamingDeltas: [String: PendingStreamingDelta] = [:]
    var streamingFlushTask: Task<Void, Never>?
    /// In-flight message refreshes keyed by conversation to prevent redundant FFI fetch storms.
    var inflightConversationMessageRefreshes: Set<String> = []
    /// Last message refresh timestamp per conversation for lightweight throttling.
    var lastConversationMessageRefreshAt: [String: CFAbsoluteTime] = [:]

    /// Cache for profile picture URLs to prevent repeated FFI calls
    nonisolated let profilePictureCache = ProfilePictureCache.shared

    // MARK: - Performance Caches

    /// Cache for conversation hierarchy data to prevent N+1 FFI calls in list views
    let hierarchyCache = ConversationHierarchyCache()

    init() {
        let persistedFilter = Self.loadPersistedAppFilter()
        _appFilterProjectIds = Published(initialValue: persistedFilter.projectIds)
        _appFilterTimeWindow = Published(initialValue: persistedFilter.timeWindow)

        // Create core immediately (lightweight)
        let tenexCore = TenexCore()
        core = tenexCore
        safeCore = SafeTenexCore(core: tenexCore)

        // Set up hierarchy cache with reference to self
        // Note: This creates a retain cycle that we break on logout
        hierarchyCache.setCoreManager(self)

        // Warm draft storage at app startup so New Chat opens immediately.
        _ = DraftManager.shared

        // Initialize asynchronously off the main thread
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let startedAt = CFAbsoluteTimeGetCurrent()
            let success = tenexCore.`init`()
            let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
            DispatchQueue.main.async {
                self?.profiler.logEvent(
                    "core.init completed success=\(success) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 300 ? .error : .info
                )
                self?.isInitialized = success
                if !success {
                    self?.initializationError = "Failed to initialize TENEX core"
                }
            }
        }
    }

    /// Trigger a manual sync with relays (optional, user-initiated).
    func syncNow() async {
        _ = await safeCore.refresh()
    }

    // MARK: - Event Callback Registration

    /// Timestamp (Unix seconds) when the event callback was registered.
    /// Used to filter out stale inbox items that arrived before this session started.
    var sessionStartTimestamp: UInt64 = 0

    /// Last time the user sent a message per conversation (Unix seconds).
    /// Used to skip TTS when the user was recently active in a conversation.
    var lastUserActivityByConversation: [String: UInt64] = [:]


    // MARK: - Push-Based Delta Application
    // These methods update @Published properties directly from Rust callbacks.

    @MainActor
    func applyMessageAppended(conversationId: String, message: MessageInfo) {
        guard var messages = messagesByConversation[conversationId] else {
            return
        }
        guard !messages.contains(where: { $0.id == message.id }) else {
            return
        }

        if let last = messages.last, last.createdAt <= message.createdAt {
            messages.append(message)
        } else {
            messages.append(message)
            messages.sort { $0.createdAt < $1.createdAt }
        }
        setMessagesCache(messages, for: conversationId)
    }

    @MainActor
    func applyConversationUpsert(_ conversation: ConversationFullInfo) {
        var updated = conversations
        guard conversationMatchesAppFilter(conversation) else {
            let initialCount = updated.count
            updated.removeAll { $0.id == conversation.id }
            guard updated.count != initialCount else {
                return
            }
            conversations = sortedConversations(updated)
            updateActiveAgentsState()
            return
        }
        if let index = updated.firstIndex(where: { $0.id == conversation.id }) {
            if updated[index] == conversation {
                return
            }
            updated[index] = conversation
        } else {
            updated.append(conversation)
        }
        let sorted = sortedConversations(updated)
        if sorted != conversations {
            conversations = sorted
            updateActiveAgentsState()
        }
    }

    /// Apply a conversation upsert from callback without triggering a full conversation-list rebuild.
    /// This path clears streaming state, applies the delta, and only refreshes messages when already cached.
    @MainActor
    func applyConversationUpsertDelta(_ conversation: ConversationFullInfo) {
        let conversationId = conversation.id
        pendingStreamingDeltas.removeValue(forKey: conversationId)
        streamingBuffers.removeValue(forKey: conversationId)
        applyConversationUpsert(conversation)

        // Avoid expensive message fetches for conversations that are not currently loaded.
        guard let cachedMessages = messagesByConversation[conversationId] else {
            profiler.logEvent(
                "applyConversationUpsertDelta conversationId=\(conversationId) messagesCached=false",
                category: .general,
                level: .debug
            )
            return
        }

        let expectedCount = Int(conversation.messageCount)
        if expectedCount > 0, cachedMessages.count == expectedCount {
            profiler.logEvent(
                "applyConversationUpsertDelta skip refresh conversationId=\(conversationId) cachedCount=\(cachedMessages.count) expectedCount=\(expectedCount)",
                category: .general,
                level: .debug
            )
            return
        }

        let now = CFAbsoluteTimeGetCurrent()
        if let lastRefresh = lastConversationMessageRefreshAt[conversationId], now - lastRefresh < 0.75 {
            profiler.logEvent(
                "applyConversationUpsertDelta throttled conversationId=\(conversationId)",
                category: .general,
                level: .debug
            )
            return
        }
        guard !inflightConversationMessageRefreshes.contains(conversationId) else {
            profiler.logEvent(
                "applyConversationUpsertDelta skip inflight conversationId=\(conversationId)",
                category: .general,
                level: .debug
            )
            return
        }

        inflightConversationMessageRefreshes.insert(conversationId)
        lastConversationMessageRefreshAt[conversationId] = now
        Task {
            let startedAt = CFAbsoluteTimeGetCurrent()
            let messages = await safeCore.getMessages(conversationId: conversationId)
            await MainActor.run {
                self.setMessagesCache(messages, for: conversationId)
                self.inflightConversationMessageRefreshes.remove(conversationId)
                let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
                self.profiler.logEvent(
                    "applyConversationUpsertDelta refreshed messages conversationId=\(conversationId) count=\(messages.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 120 ? .error : .info
                )
            }
        }
    }

    @MainActor
    func applyProjectUpsert(_ project: ProjectInfo) {
        if project.isDeleted {
            projects.removeAll { $0.id == project.id }
            projectOnlineStatus.removeValue(forKey: project.id)
            onlineAgents.removeValue(forKey: project.id)
            if appFilterProjectIds.contains(project.id) {
                appFilterProjectIds.remove(project.id)
                persistAppFilter()
                refreshConversationsForActiveFilter()
                updateAppBadge()
            }
            lastDeletedProjectId = project.id
            return
        }

        var updated = projects
        if let index = updated.firstIndex(where: { $0.id == project.id }) {
            updated[index] = project
        } else {
            updated.insert(project, at: 0)
        }
        projects = updated
    }

    @MainActor
    func applyInboxUpsert(_ item: InboxItem) {
        var updated = inboxItems
        if let index = updated.firstIndex(where: { $0.id == item.id }) {
            updated[index] = item
        } else {
            updated.append(item)
        }
        updated.sort { $0.createdAt > $1.createdAt }
        inboxItems = updated

        refreshUnansweredAskCount(reason: "applyInboxUpsert")
        // Update app badge with unanswered ask count
        updateAppBadge()
    }

    /// Update the app icon badge with unanswered ask count.
    @MainActor
    func updateAppBadge() {
        let count = unansweredAskCount
        Task {
            await NotificationService.shared.updateBadge(count: count)
        }
    }

    @MainActor
    func applyReportUpsert(_ report: ReportInfo) {
        var updated = reports
        if let index = updated.firstIndex(where: { $0.id == report.id && $0.projectId == report.projectId }) {
            updated[index] = report
        } else {
            updated.append(report)
        }
        // Sort by updated date (newest first)
        updated.sort { $0.updatedAt > $1.updatedAt }
        reports = updated
    }

    @MainActor
    func applyProjectStatusChanged(projectId: String, projectATag: String, isOnline: Bool, onlineAgents: [OnlineAgentInfo]) {
        let resolvedProjectId: String = {
            if !projectId.isEmpty {
                return projectId
            }
            return Self.projectId(fromATag: projectATag)
        }()

        guard !resolvedProjectId.isEmpty else { return }

        let normalizedAgents = Self.canonicalOnlineAgents(onlineAgents)
        let previousStatus = projectOnlineStatus[resolvedProjectId]
        let previousAgents = self.onlineAgents[resolvedProjectId]
        let statusChanged = previousStatus != isOnline
        let agentsChanged = previousAgents != normalizedAgents

        if statusChanged {
            setProjectOnlineStatus(isOnline, for: resolvedProjectId)
        }
        if agentsChanged {
            setOnlineAgentsCache(normalizedAgents, for: resolvedProjectId)
        }

        if statusChanged || agentsChanged {
            signalDiagnosticsUpdate()
        }

        profiler.logEvent(
            "applyProjectStatusChanged projectId=\(resolvedProjectId) statusChanged=\(statusChanged) agentsChanged=\(agentsChanged) isOnline=\(isOnline) agentCount=\(normalizedAgents.count)",
            category: .general,
            level: (statusChanged || agentsChanged) ? .info : .debug
        )
    }

    @MainActor
    func applyActiveConversationsChanged(projectId: String, projectATag: String, activeConversationIds: [String]) {
        let normalizedProjectId = projectId.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalizedProjectATag = projectATag.trimmingCharacters(in: .whitespacesAndNewlines)
        let resolvedProjectId = !normalizedProjectId.isEmpty ? normalizedProjectId : Self.projectId(fromATag: normalizedProjectATag)
        guard !resolvedProjectId.isEmpty || !normalizedProjectATag.isEmpty else {
            return
        }

        let activeConversationIdSet = Set(activeConversationIds)
        var updated = conversations
        var didChange = false

        for index in updated.indices {
            let conversation = updated[index]
            let conversationProjectId = Self.projectId(fromATag: conversation.projectATag)
            let matchesProjectATag = !normalizedProjectATag.isEmpty && conversation.projectATag == normalizedProjectATag
            let matchesProjectId = !resolvedProjectId.isEmpty && conversationProjectId == resolvedProjectId

            guard matchesProjectATag || matchesProjectId else { continue }

            let shouldBeActive = activeConversationIdSet.contains(conversation.id)
            if conversation.isActive != shouldBeActive {
                updated[index].isActive = shouldBeActive
                didChange = true
            }
        }

        if didChange {
            conversations = sortedConversations(updated)
            updateActiveAgentsState()
        }
    }

    @MainActor
    func handlePendingBackendApproval(backendPubkey: String, projectATag: String) {
        #if os(macOS)
        // Manual approval on macOS: keep backend pending and surface it in Settings > Backends.
        signalDiagnosticsUpdate()
        return
        #else
        Task {
            do {
                try await safeCore.approveBackend(pubkey: backendPubkey)
            } catch {
                return
            }

            let projectId = Self.projectId(fromATag: projectATag)
            guard !projectId.isEmpty else { return }

            let isOnline = await safeCore.isProjectOnline(projectId: projectId)
            let agents = (try? await safeCore.getOnlineAgents(projectId: projectId)) ?? []
            await MainActor.run {
                self.applyProjectStatusChanged(projectId: projectId, projectATag: projectATag, isOnline: isOnline, onlineAgents: agents)
            }
        }
        #endif
    }

    @MainActor
    func applyStreamChunk(agentPubkey: String, conversationId: String, textDelta: String?) {
        guard let delta = textDelta, !delta.isEmpty else { return }

        if var pending = pendingStreamingDeltas[conversationId] {
            pending.text.append(delta)
            pending.chunkCount += 1
            pendingStreamingDeltas[conversationId] = pending
        } else {
            pendingStreamingDeltas[conversationId] = PendingStreamingDelta(
                agentPubkey: agentPubkey,
                text: delta,
                chunkCount: 1,
                startedAt: CFAbsoluteTimeGetCurrent()
            )
        }

        scheduleStreamingFlushIfNeeded()
    }

    @MainActor
    func signalStatsUpdate() {
        bumpStatsVersion()
    }

    @MainActor
    func signalDiagnosticsUpdate() {
        bumpDiagnosticsVersion()
    }

    @MainActor
    func signalTeamsUpdate() {
        bumpTeamsVersion()
    }

    /// Signal that messages for a specific conversation have been updated.
    /// This triggers a refresh of the conversation's messages.
    @MainActor
    func signalConversationUpdate(conversationId: String) {
        pendingStreamingDeltas.removeValue(forKey: conversationId)
        streamingBuffers.removeValue(forKey: conversationId)
        profiler.logEvent(
            "signalConversationUpdate conversationId=\(conversationId)",
            category: .general,
            level: .debug
        )
        Task {
            let startedAt = CFAbsoluteTimeGetCurrent()
            // Refresh messages for this specific conversation
            let messages = await safeCore.getMessages(conversationId: conversationId)
            let refreshedConversation = await safeCore.getConversationsByIds(conversationIds: [conversationId]).first
            await MainActor.run {
                self.setMessagesCache(messages, for: conversationId)
                if let refreshedConversation {
                    self.applyConversationUpsert(refreshedConversation)
                } else {
                    self.conversations.removeAll { $0.id == conversationId }
                    self.updateActiveAgentsState()
                }
                self.inflightConversationMessageRefreshes.remove(conversationId)
                let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
                self.profiler.logEvent(
                    "signalConversationUpdate complete conversationId=\(conversationId) messages=\(messages.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 150 ? .error : .info
                )
            }
        }
    }

    @MainActor
    private func scheduleStreamingFlushIfNeeded() {
        guard streamingFlushTask == nil else { return }

        streamingFlushTask = Task { @MainActor [weak self] in
            guard let self else { return }
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 80_000_000) // ~12.5 FPS publish cap
                guard !self.pendingStreamingDeltas.isEmpty else { break }
                self.flushPendingStreamingDeltas()
            }
            self.streamingFlushTask = nil
        }
    }

    @MainActor
    private func flushPendingStreamingDeltas() {
        guard !pendingStreamingDeltas.isEmpty else { return }

        let flushStartedAt = CFAbsoluteTimeGetCurrent()
        let pendingConversationCount = pendingStreamingDeltas.count
        var updatedBuffers = streamingBuffers
        var totalChars = 0
        var totalChunks = 0
        var maxQueuedMs: Double = 0

        for (conversationId, pending) in pendingStreamingDeltas {
            var buffer = updatedBuffers[conversationId] ?? StreamingBuffer(agentPubkey: pending.agentPubkey, text: "")
            buffer.text.append(pending.text)
            updatedBuffers[conversationId] = buffer
            totalChars += pending.text.count
            totalChunks += pending.chunkCount
            let queuedMs = (flushStartedAt - pending.startedAt) * 1000
            if queuedMs > maxQueuedMs {
                maxQueuedMs = queuedMs
            }
        }

        pendingStreamingDeltas.removeAll(keepingCapacity: true)
        streamingBuffers = updatedBuffers

        let elapsedMs = (CFAbsoluteTimeGetCurrent() - flushStartedAt) * 1000
        profiler.logEvent(
            "flushPendingStreamingDeltas conversations=\(pendingConversationCount) chunks=\(totalChunks) chars=\(totalChars) maxQueuedMs=\(String(format: "%.2f", maxQueuedMs)) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .swiftUI,
            level: totalChunks >= 64 ? .debug : .info
        )
    }

    /// Signal that project status has changed (kind:24010 events).
    /// This triggers a refresh of project status data, online status, and agents cache.
    /// Uses task cancellation to prevent stale overwrites from overlapping refreshes.
    @MainActor
    func signalProjectStatusUpdate() {
        // Cancel any existing refresh task to prevent stale results
        projectStatusUpdateTask?.cancel()

        projectStatusUpdateTask = Task { [weak self] in
            guard let self else { return }

            // Fetch projects
            let projects = await safeCore.getProjects()

            // Check for cancellation before continuing
            if Task.isCancelled { return }

            await MainActor.run {
                self.projects = projects
            }

            // Compute status and agents OFF main actor using shared helper
            await self.refreshProjectStatusParallel(for: projects)

            // Final diagnostics update on main actor
            if !Task.isCancelled {
                await MainActor.run {
                    self.signalDiagnosticsUpdate()
                }
            }
        }
    }

    /// Refresh project online status and agents in parallel, OFF the main actor.
    /// This shared helper is used by both signalProjectStatusUpdate() and fetchData()
    /// to eliminate code duplication and ensure consistent behavior.
    /// - Parameter projects: Array of projects to check status for
    func refreshProjectStatusParallel(for projects: [ProjectInfo]) async {
        let startedAt = CFAbsoluteTimeGetCurrent()
        var statusUpdates: [String: Bool] = [:]
        var agentsUpdates: [String: [OnlineAgentInfo]] = [:]

        // Use withTaskGroup for concurrent project status checks (runs OFF MainActor)
        await withTaskGroup(of: (String, Bool, [OnlineAgentInfo]).self) { group in
            for project in projects {
                group.addTask {
                    // Check cancellation inside each task
                    if Task.isCancelled {
                        return (project.id, false, [])
                    }

                    let isOnline = await self.safeCore.isProjectOnline(projectId: project.id)
                    let agents: [OnlineAgentInfo]
                    if isOnline {
                        agents = (try? await self.safeCore.getOnlineAgents(projectId: project.id)) ?? []
                    } else {
                        agents = []
                    }
                    return (project.id, isOnline, Self.canonicalOnlineAgents(agents))
                }
            }

            // Collect results off-main, then publish once to avoid N UI invalidations.
            for await (projectId, isOnline, agents) in group {
                if Task.isCancelled { continue }
                statusUpdates[projectId] = isOnline
                agentsUpdates[projectId] = agents
            }
        }

        if !Task.isCancelled {
            await MainActor.run {
                var nextProjectOnlineStatus = self.projectOnlineStatus
                nextProjectOnlineStatus.merge(statusUpdates, uniquingKeysWith: { _, new in new })
                if nextProjectOnlineStatus != self.projectOnlineStatus {
                    self.projectOnlineStatus = nextProjectOnlineStatus
                }

                var nextOnlineAgents = self.onlineAgents
                nextOnlineAgents.merge(agentsUpdates, uniquingKeysWith: { _, new in new })
                if nextOnlineAgents != self.onlineAgents {
                    self.onlineAgents = nextOnlineAgents
                }

                // Re-sort projects: online first, then alphabetical
                self.projects.sort { a, b in
                    let aOnline = nextProjectOnlineStatus[a.id] ?? false
                    let bOnline = nextProjectOnlineStatus[b.id] ?? false
                    if aOnline != bOnline { return aOnline }
                    return a.title.localizedCaseInsensitiveCompare(b.title) == .orderedAscending
                }
            }
        }
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "refreshProjectStatusParallel projects=\(projects.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 200 ? .error : .info
        )
    }


    /// Update hasActiveAgents based on current conversations
    @MainActor
    func updateActiveAgentsState() {
        hasActiveAgents = conversations.contains { $0.isActive }
    }

    func sortedConversations(_ items: [ConversationFullInfo]) -> [ConversationFullInfo] {
        var updated = items
        updated.sort { lhs, rhs in
            switch (lhs.isActive, rhs.isActive) {
            case (true, false):
                return true
            case (false, true):
                return false
            default:
                return lhs.effectiveLastActivity > rhs.effectiveLastActivity
            }
        }
        return updated
    }

    /// Fetch and cache online agents for a specific project.
    /// This shared method eliminates code duplication and ensures consistent agent caching.
    /// FFI work runs off the main thread; only state mutation hops to MainActor.
    /// - Parameter projectId: The ID of the project to fetch agents for
    func fetchAndCacheAgents(for projectId: String) async {
        let startedAt = CFAbsoluteTimeGetCurrent()
        // Perform FFI call OFF the MainActor to avoid UI blocking
        let agents: [OnlineAgentInfo]
        do {
            agents = try await safeCore.getOnlineAgents(projectId: projectId)
        } catch {
            // Cache empty array on failure to prevent stale data
            await MainActor.run { self.setOnlineAgentsCache([], for: projectId) }
            let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
            profiler.logEvent(
                "fetchAndCacheAgents failed projectId=\(projectId) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                category: .general,
                level: .error
            )
            return
        }

        // Only hop to main actor to mutate state
        await MainActor.run {
            self.setOnlineAgentsCache(agents, for: projectId)
        }
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "fetchAndCacheAgents projectId=\(projectId) agents=\(agents.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 120 ? .error : .info
        )
    }

    @MainActor
    func ensureMessagesLoaded(conversationId: String) async {
        if messagesByConversation[conversationId] != nil {
            profiler.logEvent(
                "ensureMessagesLoaded cache-hit conversationId=\(conversationId)",
                category: .general,
                level: .debug
            )
            return
        }
        let startedAt = CFAbsoluteTimeGetCurrent()
        let fetched = await safeCore.getMessages(conversationId: conversationId)
        mergeMessagesCache(fetched, for: conversationId)
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "ensureMessagesLoaded cache-miss conversationId=\(conversationId) fetched=\(fetched.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 120 ? .error : .info
        )
    }

    @MainActor
    private func setMessagesCache(_ messages: [MessageInfo], for conversationId: String) {
        if messagesByConversation[conversationId] == messages {
            return
        }
        messagesByConversation[conversationId] = messages
    }

    @MainActor
    private func mergeMessagesCache(_ messages: [MessageInfo], for conversationId: String) {
        var combined = messagesByConversation[conversationId] ?? []
        if combined.isEmpty {
            combined = messages
        } else {
            let existingIds = Set(combined.map { $0.id })
            combined.append(contentsOf: messages.filter { !existingIds.contains($0.id) })
        }
        combined.sort { $0.createdAt < $1.createdAt }
        setMessagesCache(combined, for: conversationId)
    }

    @MainActor
    private func setOnlineAgentsCache(_ agents: [OnlineAgentInfo], for projectId: String) {
        let normalizedAgents = Self.canonicalOnlineAgents(agents)
        if onlineAgents[projectId] == normalizedAgents {
            return
        }
        var updated = onlineAgents
        updated[projectId] = normalizedAgents
        onlineAgents = updated
    }

    @MainActor
    private func setProjectOnlineStatus(_ isOnline: Bool, for projectId: String) {
        if projectOnlineStatus[projectId] == isOnline {
            return
        }
        var updated = projectOnlineStatus
        updated[projectId] = isOnline
        projectOnlineStatus = updated
    }

    nonisolated private static func canonicalOnlineAgents(_ agents: [OnlineAgentInfo]) -> [OnlineAgentInfo] {
        agents
            .map { agent in
                var normalized = agent
                normalized.tools = agent.tools.sorted()
                return normalized
            }
            .sorted { lhs, rhs in
                if lhs.isPm != rhs.isPm {
                    return lhs.isPm && !rhs.isPm
                }
                if lhs.name != rhs.name {
                    return lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
                }
                if lhs.pubkey != rhs.pubkey {
                    return lhs.pubkey < rhs.pubkey
                }
                let lhsModel = lhs.model ?? ""
                let rhsModel = rhs.model ?? ""
                if lhsModel != rhsModel {
                    return lhsModel < rhsModel
                }
                return lhs.tools.lexicographicallyPrecedes(rhs.tools)
            }
    }

    @MainActor
    func bumpStatsVersion() {
        statsVersion &+= 1
        statsVersionSubject.send(statsVersion)
    }

    @MainActor
    func bumpTeamsVersion() {
        teamsVersion &+= 1
        teamsVersionSubject.send(teamsVersion)
    }

    @MainActor
    func bumpDiagnosticsVersion() {
        diagnosticsVersion &+= 1
        diagnosticsVersionSubject.send(diagnosticsVersion)
    }

    @MainActor
    func refreshConversationsForActiveFilter() {
        let snapshot = appFilterSnapshot

        conversationRefreshTask?.cancel()
        profiler.logEvent(
            "refreshConversationsForActiveFilter start projectIds=\(snapshot.projectIds.count) timeWindow=\(snapshot.timeWindow.rawValue)",
            category: .general,
            level: .debug
        )
        conversationRefreshTask = Task { [weak self] in
            guard let self else { return }
            let startedAt = CFAbsoluteTimeGetCurrent()

            guard let refreshed = try? await self.fetchConversations(for: snapshot) else { return }
            guard !Task.isCancelled else { return }

            await MainActor.run {
                guard self.appFilterSnapshot == snapshot else { return }
                self.conversations = self.sortedConversations(refreshed)
                self.updateActiveAgentsState()
                let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
                self.profiler.logEvent(
                    "refreshConversationsForActiveFilter complete results=\(refreshed.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 180 ? .error : .info
                )
            }
        }
    }

    func fetchConversations(for snapshot: AppGlobalFilterSnapshot) async throws -> [ConversationFullInfo] {
        let startedAt = CFAbsoluteTimeGetCurrent()
        let filter = ConversationFilter(
            projectIds: Array(snapshot.projectIds),
            showArchived: true,
            hideScheduled: false,
            timeFilter: snapshot.timeWindow.coreTimeFilter
        )
        let fetched = try await safeCore.getAllConversations(filter: filter)
        guard !Task.isCancelled else { return [] }
        let now = UInt64(Date().timeIntervalSince1970)
        let filtered = fetched.filter { conversation in
            let projectId = Self.projectId(fromATag: conversation.projectATag)
            return snapshot.includes(projectId: projectId, timestamp: conversation.effectiveLastActivity, now: now)
        }
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "fetchConversations projectIds=\(snapshot.projectIds.count) fetched=\(fetched.count) filtered=\(filtered.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 180 ? .error : .info
        )
        return filtered
    }

    static func projectId(fromATag aTag: String) -> String {
        let parts = aTag.split(separator: ":")
        guard parts.count >= 3 else { return "" }
        return parts.dropFirst(2).joined(separator: ":")
    }

}
