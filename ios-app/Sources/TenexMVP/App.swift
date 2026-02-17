import SwiftUI
import CryptoKit
import UserNotifications

// MARK: - Auto-Login Result

/// Result of attempting auto-login from stored credentials
enum AutoLoginResult {
    /// No stored credentials found - show login screen
    case noCredentials
    /// Auto-login succeeded with the given npub
    case success(npub: String)
    /// Stored credential was invalid (should be deleted)
    case invalidCredential(error: String)
    /// Transient error (keychain access failed, network issue, etc.) - don't delete credentials
    case transientError(error: String)
}

// MARK: - Streaming Buffer

struct StreamingBuffer {
    let agentPubkey: String
    var text: String
}

// MARK: - Profile Picture Cache

/// Thread-safe cache for profile picture URLs to prevent repeated synchronous FFI calls during scroll.
/// Each pubkey's picture URL is fetched once and cached for the session lifetime.
final class ProfilePictureCache {
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
    @Published private(set) var statsVersion: UInt64 = 0
    @Published private(set) var diagnosticsVersion: UInt64 = 0
    @Published var streamingBuffers: [String: StreamingBuffer] = [:]

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

    // MARK: - Ask Badge Support

    /// Hard cap for inbox items: 48 hours in seconds.
    /// Synchronized with InboxView.inbox48HourCapSeconds and Rust constant.
    private static let inbox48HourCapSeconds: UInt64 = 48 * 60 * 60

    /// Count of unanswered ask events within the 48-hour window.
    /// Computed property that filters inboxItems for badge display.
    var unansweredAskCount: Int {
        let now = UInt64(Date().timeIntervalSince1970)
        let cutoff = now > Self.inbox48HourCapSeconds ? now - Self.inbox48HourCapSeconds : 0
        return inboxItems.filter { item in
            item.eventType == "ask" &&
            item.status == "waiting" &&
            item.createdAt >= cutoff
        }.count
    }

    // MARK: - Event Callback
    /// Event handler for push-based updates from Rust core
    private var eventHandler: TenexEventHandler?

    /// Task reference for project status updates - enables cancellation of stale refreshes
    private var projectStatusUpdateTask: Task<Void, Never>?

    /// Cache for profile picture URLs to prevent repeated FFI calls
    let profilePictureCache = ProfilePictureCache.shared

    // MARK: - Performance Caches

    /// Cache for conversation hierarchy data to prevent N+1 FFI calls in list views
    let hierarchyCache = ConversationHierarchyCache()

    init() {
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
            let success = tenexCore.`init`()
            DispatchQueue.main.async {
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
    private(set) var sessionStartTimestamp: UInt64 = 0

    /// Last time the user sent a message per conversation (Unix seconds).
    /// Used to skip TTS when the user was recently active in a conversation.
    private var lastUserActivityByConversation: [String: UInt64] = [:]

    /// Record that the user was active in a conversation (for TTS inactivity gating).
    func recordUserActivity(conversationId: String) {
        lastUserActivityByConversation[conversationId] = UInt64(Date().timeIntervalSince1970)
    }

    /// Register the event callback for push-based updates.
    /// Call this after successful login to enable real-time updates.
    func registerEventCallback() {
        sessionStartTimestamp = UInt64(Date().timeIntervalSince1970)
        let handler = TenexEventHandler(coreManager: self)
        eventHandler = handler
        core.setEventCallback(callback: handler)
    }

    /// Unregister the event callback.
    /// Call this on logout to clean up resources.
    func unregisterEventCallback() {
        core.clearEventCallback()
        eventHandler = nil
    }

    /// Manual refresh for pull-to-refresh gesture.
    ///
    /// This performs a full reconnection to relays to ensure fresh data is fetched.
    /// Unlike the automatic refresh which only drains pending events, this:
    /// 1. Disconnects from all relays
    /// 2. Reconnects with the same credentials
    /// 3. Restarts all subscriptions
    /// 4. Triggers a new negentropy sync to fetch any missed events
    /// 5. Refreshes all data from the store
    func manualRefresh() async {
        _ = await safeCore.refresh()
        await fetchData()
    }

    // MARK: - Push-Based Delta Application
    // These methods update @Published properties directly from Rust callbacks.

    @MainActor
    func applyMessageAppended(conversationId: String, message: MessageInfo) {
        var messages = messagesByConversation[conversationId, default: []]
        if !messages.contains(where: { $0.id == message.id }) {
            messages.append(message)
            messages.sort { $0.createdAt < $1.createdAt }
            setMessagesCache(messages, for: conversationId)
        }
    }

    @MainActor
    func applyConversationUpsert(_ conversation: ConversationFullInfo) {
        var updated = conversations
        if let index = updated.firstIndex(where: { $0.id == conversation.id }) {
            updated[index] = conversation
        } else {
            updated.append(conversation)
        }
        conversations = sortedConversations(updated)
        updateActiveAgentsState()
    }

    @MainActor
    func applyProjectUpsert(_ project: ProjectInfo) {
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
    func applyProjectStatusChanged(projectId: String, projectATag _: String, isOnline: Bool, onlineAgents: [OnlineAgentInfo]) {
        let previousStatus = projectOnlineStatus[projectId]
        let previousAgents = self.onlineAgents[projectId]
        setProjectOnlineStatus(isOnline, for: projectId)
        setOnlineAgentsCache(onlineAgents, for: projectId)
        if previousStatus != isOnline || previousAgents != onlineAgents {
            signalDiagnosticsUpdate()
        }
    }

    @MainActor
    func applyActiveConversationsChanged(projectId _: String, projectATag: String, activeConversationIds: [String]) {
        var updated = conversations
        var didChange = false
        for index in updated.indices {
            if updated[index].projectATag == projectATag {
                let shouldBeActive = activeConversationIds.contains(updated[index].id)
                if updated[index].isActive != shouldBeActive {
                    updated[index].isActive = shouldBeActive
                    didChange = true
                }
            }
        }
        if didChange {
            conversations = sortedConversations(updated)
            updateActiveAgentsState()
        }
    }

    @MainActor
    func handlePendingBackendApproval(backendPubkey: String, projectATag: String) {
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
    }

    @MainActor
    func applyStreamChunk(agentPubkey: String, conversationId: String, textDelta: String?) {
        guard let delta = textDelta, !delta.isEmpty else { return }
        var buffer = streamingBuffers[conversationId] ?? StreamingBuffer(agentPubkey: agentPubkey, text: "")
        buffer.text.append(delta)
        streamingBuffers[conversationId] = buffer
    }

    @MainActor
    func signalStatsUpdate() {
        bumpStatsVersion()
    }

    @MainActor
    func signalDiagnosticsUpdate() {
        bumpDiagnosticsVersion()
    }

    /// Signal that messages for a specific conversation have been updated.
    /// This triggers a refresh of the conversation's messages.
    @MainActor
    func signalConversationUpdate(conversationId: String) {
        streamingBuffers.removeValue(forKey: conversationId)
        Task {
            // Refresh messages for this specific conversation
            let messages = await safeCore.getMessages(conversationId: conversationId)
            await MainActor.run {
                self.setMessagesCache(messages, for: conversationId)
            }
            // Also refresh the conversation list
            // Use showArchived: true to match fetchData() - client-side filtering is applied in views
            let filter = ConversationFilter(
                projectIds: [],
                showArchived: true,
                hideScheduled: false,
                timeFilter: .all
            )
            if let conversations = try? await safeCore.getAllConversations(filter: filter) {
                await MainActor.run {
                    self.conversations = self.sortedConversations(conversations)
                    self.updateActiveAgentsState()
                }
            }
        }
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

            // Fetch projects with proper error handling
            let projects: [ProjectInfo]
            do {
                projects = try await safeCore.getProjects()
            } catch {
                return
            }

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
    private func refreshProjectStatusParallel(for projects: [ProjectInfo]) async {
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
                    return (project.id, isOnline, agents)
                }
            }

            // Merge results on main actor with per-project updates (not whole-dictionary overwrites)
            for await (projectId, isOnline, agents) in group {
                // Check for cancellation before each update to minimize race windows
                if Task.isCancelled { continue }

                await MainActor.run {
                    self.setProjectOnlineStatus(isOnline, for: projectId)
                    self.setOnlineAgentsCache(agents, for: projectId)
                }
            }
        }

        // Re-sort projects: online first, then alphabetical
        if !Task.isCancelled {
            await MainActor.run {
                self.projects.sort { a, b in
                    let aOnline = self.projectOnlineStatus[a.id] ?? false
                    let bOnline = self.projectOnlineStatus[b.id] ?? false
                    if aOnline != bOnline { return aOnline }
                    return a.title.localizedCaseInsensitiveCompare(b.title) == .orderedAscending
                }
            }
        }
    }

    /// Signal a general update - used when the change type is not specific.
    /// This triggers a refresh of core data.
    @MainActor
    func signalGeneralUpdate() {
        bumpDiagnosticsVersion()
    }

    /// Trigger audio notification generation for a p-tag mention.
    /// Runs in background to avoid blocking UI. Audio is played automatically when ready.
    func triggerAudioNotification(
        agentPubkey: String,
        conversationTitle: String,
        messageText: String,
        conversationId: String? = nil
    ) async {
        // Check inactivity threshold: skip TTS if user was recently active in this conversation
        // Using fixed 120 second threshold
        if let convId = conversationId,
           let lastActivity = lastUserActivityByConversation[convId] {
            let threshold: UInt64 = 120
            let now = UInt64(Date().timeIntervalSince1970)
            if now - lastActivity < threshold {
                return
            }
        }

        // Load API keys from iOS Keychain
        let elevenlabsResult = await KeychainService.shared.loadElevenLabsApiKeyAsync()
        let openrouterResult = await KeychainService.shared.loadOpenRouterApiKeyAsync()

        guard case .success(let elevenlabsKey) = elevenlabsResult,
              case .success(let openrouterKey) = openrouterResult else {
            return
        }

        do {
            let notification = try await safeCore.generateAudioNotification(
                agentPubkey: agentPubkey,
                conversationTitle: conversationTitle,
                messageText: messageText,
                elevenlabsApiKey: elevenlabsKey,
                openrouterApiKey: openrouterKey
            )

            await MainActor.run {
                AudioNotificationPlayer.shared.enqueue(notification: notification, conversationId: conversationId)
            }
        } catch {
        }
    }

    /// Update hasActiveAgents based on current conversations
    @MainActor
    private func updateActiveAgentsState() {
        hasActiveAgents = conversations.contains { $0.isActive }
    }

    private func sortedConversations(_ items: [ConversationFullInfo]) -> [ConversationFullInfo] {
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
        // Perform FFI call OFF the MainActor to avoid UI blocking
        let agents: [OnlineAgentInfo]
        do {
            agents = try await safeCore.getOnlineAgents(projectId: projectId)
        } catch {
            // Cache empty array on failure to prevent stale data
            await MainActor.run { self.setOnlineAgentsCache([], for: projectId) }
            return
        }

        // Only hop to main actor to mutate state
        await MainActor.run {
            self.setOnlineAgentsCache(agents, for: projectId)
        }
    }

    @MainActor
    func ensureMessagesLoaded(conversationId: String) async {
        if messagesByConversation[conversationId] != nil {
            return
        }
        let fetched = await safeCore.getMessages(conversationId: conversationId)
        mergeMessagesCache(fetched, for: conversationId)
    }

    @MainActor
    private func setMessagesCache(_ messages: [MessageInfo], for conversationId: String) {
        var updated = messagesByConversation
        updated[conversationId] = messages
        messagesByConversation = updated
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
        var updated = onlineAgents
        updated[projectId] = agents
        onlineAgents = updated
    }

    @MainActor
    private func setProjectOnlineStatus(_ isOnline: Bool, for projectId: String) {
        var updated = projectOnlineStatus
        updated[projectId] = isOnline
        projectOnlineStatus = updated
    }

    @MainActor
    private func bumpStatsVersion() {
        statsVersion &+= 1
    }

    @MainActor
    private func bumpDiagnosticsVersion() {
        diagnosticsVersion &+= 1
    }

    static func projectId(fromATag aTag: String) -> String {
        let parts = aTag.split(separator: ":")
        guard parts.count >= 3 else { return "" }
        return parts.dropFirst(2).joined(separator: ":")
    }

    /// Load initial data from the core (local cache).
    /// Real-time updates come via push-based event callbacks, not polling.
    @MainActor
    func fetchData() async {
        // Auto-approve any pending backends (iOS doesn't have approval UI yet)
        // This allows kind:24010 status events to be processed, enabling online agents
        do {
            let approved = try await safeCore.approveAllPendingBackends()
            if approved > 0 {
            } else {
            }
        } catch {
        }

        do {
            // Always fetch all conversations (including scheduled)
            // Client-side filtering is applied in ConversationsTabView based on user preferences
            let filter = ConversationFilter(
                projectIds: [],
                showArchived: true,
                hideScheduled: false,
                timeFilter: .all
            )

            // Fetch all data concurrently
            async let fetchedProjects = safeCore.getProjects()
            async let fetchedConversations = try safeCore.getAllConversations(filter: filter)
            async let fetchedInbox = safeCore.getInbox()

            let (p, c, i) = try await (fetchedProjects, fetchedConversations, fetchedInbox)

            projects = p
            conversations = sortedConversations(c)
            inboxItems = i

            // Initialize project online status and online agents in parallel, OFF main actor
            // This uses the shared helper to avoid code duplication and ensure consistent behavior
            await refreshProjectStatusParallel(for: p)

            updateActiveAgentsState()
            signalStatsUpdate()
            signalDiagnosticsUpdate()
        } catch {
            // Don't crash - just log and continue with stale data
        }
    }

    // MARK: - Profile Picture API (Cached)

    /// Get profile picture URL for a pubkey, using cache to prevent repeated FFI calls.
    /// This is the primary API for avatar views - always use this instead of core.getProfilePicture directly.
    /// - Parameter pubkey: The hex-encoded public key
    /// - Returns: Profile picture URL if available, nil otherwise
    func getProfilePicture(pubkey: String) -> String? {
        // Check cache first (O(1) lookup)
        if let cached = profilePictureCache.getCached(pubkey) {
            return cached
        }

        // Cache miss - fetch from FFI (synchronous, but only once per pubkey)
        // Handle Result type properly - log errors but return nil for graceful degradation
        do {
            let pictureUrl = try core.getProfilePicture(pubkey: pubkey)
            profilePictureCache.store(pubkey, pictureUrl: pictureUrl)
            return pictureUrl
        } catch {
            // Log error for debugging but don't crash - graceful degradation
            // Cache nil to prevent repeated failed calls
            profilePictureCache.store(pubkey, pictureUrl: nil)
            return nil
        }
    }

    /// Prefetch profile pictures for multiple pubkeys in background.
    /// Call this when loading a list of agents/conversations to warm the cache.
    /// - Parameter pubkeys: Array of hex-encoded public keys to prefetch
    func prefetchProfilePictures(_ pubkeys: [String]) {
        DispatchQueue.global(qos: .utility).async { [weak self] in
            for pubkey in pubkeys {
                // Only fetch if not already cached
                if self?.profilePictureCache.getCached(pubkey) == nil {
                    do {
                        let pictureUrl = try self?.core.getProfilePicture(pubkey: pubkey)
                        self?.profilePictureCache.store(pubkey, pictureUrl: pictureUrl)
                    } catch {
                        // Log but don't crash - cache nil to prevent repeated attempts
                        self?.profilePictureCache.store(pubkey, pictureUrl: nil)
                    }
                }
            }
        }
    }

    // MARK: - Credential Management

    /// Attempts auto-login using stored credentials
    /// - Returns: AutoLoginResult indicating outcome
    /// - Note: Call from background thread
    func attemptAutoLogin() -> AutoLoginResult {
        // Load credential from keychain
        let loadResult = KeychainService.shared.loadNsec()

        switch loadResult {
        case .failure(.itemNotFound):
            return .noCredentials

        case .failure(let error):
            // Keychain access failed - transient error, don't delete credentials
            return .transientError(error: error.localizedDescription)

        case .success(let nsec):
            // Attempt login with stored credential
            do {
                let loginResult = try core.login(nsec: nsec)
                if loginResult.success {
                    return .success(npub: loginResult.npub)
                } else {
                    // Login returned false without throwing - this is ambiguous
                    // Could be network issue, server error, etc. - treat as transient
                    // to avoid deleting potentially valid credentials
                    return .transientError(error: "Login failed - please try again")
                }
            } catch let error as TenexError {
                switch error {
                case .InvalidNsec(let message):
                    // Provably invalid - should delete stored credential
                    return .invalidCredential(error: message)
                case .NotLoggedIn, .Internal, .LogoutFailed, .LockError, .CoreNotInitialized:
                    // These are transient/unexpected - don't delete credentials
                    return .transientError(error: error.localizedDescription)
                }
            } catch {
                // Unknown error - treat as transient
                return .transientError(error: error.localizedDescription)
            }
        }
    }

    /// Saves credentials to keychain after successful login
    /// - Parameter nsec: The nsec to save
    /// - Returns: Optional error message if save failed
    func saveCredential(nsec: String) async -> String? {
        let result = await KeychainService.shared.saveNsecAsync(nsec)
        switch result {
        case .success:
            return nil
        case .failure(let error):
            return error.localizedDescription
        }
    }

    /// Clears stored credentials from keychain
    /// - Returns: Optional error message if clear failed
    func clearCredentials() async -> String? {
        // Clear profile picture cache on logout to prevent stale data
        profilePictureCache.clear()

        let result = await KeychainService.shared.deleteNsecAsync()
        switch result {
        case .success:
            return nil
        case .failure(let error):
            return error.localizedDescription
        }
    }
}

// MARK: - Debug Auto-Login Support
//
// For automated testing (e.g., ios-tester agent), you can bypass the login screen by:
//
// 1. Launch arguments (recommended for xcrun simctl):
//    xcrun simctl launch <UDID> com.tenex.mvp --debug-nsec "nsec1..."
//
// 2. Environment variables:
//    TENEX_DEBUG_NSEC=nsec1...
//
// Example with simctl:
//    xcrun simctl launch 91722A96-628B-49D9-9A07-3E5A2BDEB65D com.tenex.mvp --debug-nsec "nsec1abc..."
//
// The app will auto-login with the provided nsec and skip the login screen.
// This is only intended for DEBUG builds and automated testing.

@main
struct TenexMVPApp: App {
    @StateObject private var coreManager = TenexCoreManager()
    @State private var isLoggedIn = false
    @State private var userNpub = ""
    @State private var isAttemptingAutoLogin = false
    @State private var autoLoginError: String?
    @State private var showNotificationDeniedAlert = false
    @Environment(\.scenePhase) private var scenePhase

    /// Check for debug nsec from launch arguments or environment variables.
    /// Returns the nsec if found, nil otherwise.
    private func getDebugNsec() -> String? {
        #if DEBUG
        // Check launch arguments first: --debug-nsec "nsec1..."
        let args = ProcessInfo.processInfo.arguments
        if let index = args.firstIndex(of: "--debug-nsec"), index + 1 < args.count {
            let nsec = args[index + 1]
            if nsec.hasPrefix("nsec1") {
                return nsec
            }
        }

        // Check environment variable: TENEX_DEBUG_NSEC=nsec1...
        if let nsec = ProcessInfo.processInfo.environment["TENEX_DEBUG_NSEC"],
           nsec.hasPrefix("nsec1") {
            return nsec
        }
        #endif

        return nil
    }

    var body: some Scene {
        WindowGroup {
            Group {
                if !coreManager.isInitialized {
                    // Show loading while initializing
                    VStack(spacing: 16) {
                        ProgressView()
                            .scaleEffect(1.5)
                        Text("Initializing TENEX...")
                            .foregroundStyle(.secondary)

                        if let error = coreManager.initializationError {
                            Text(error)
                                .foregroundStyle(Color.healthError)
                                .font(.caption)
                        }
                    }
                } else if isAttemptingAutoLogin {
                    // Show loading while attempting auto-login
                    VStack(spacing: 16) {
                        ProgressView()
                            .scaleEffect(1.5)
                        Text("Logging in...")
                            .foregroundStyle(.secondary)
                    }
                } else if isLoggedIn {
                    MainTabView(userNpub: $userNpub, isLoggedIn: $isLoggedIn)
                        .environmentObject(coreManager)
                } else {
                    LoginView(
                        isLoggedIn: $isLoggedIn,
                        userNpub: $userNpub,
                        autoLoginError: autoLoginError
                    )
                    .environmentObject(coreManager)
                }
            }
            .onChange(of: coreManager.isInitialized) { _, isInitialized in
                if isInitialized {
                    attemptAutoLogin()
                }
            }
            .onChange(of: isLoggedIn) { _, loggedIn in
                // Register/unregister event callback based on login state
                if loggedIn {
                    coreManager.registerEventCallback()
                    // Initial data fetch on login with proper authorization sequencing
                    Task { @MainActor in
                        // Request authorization FIRST so badge can be set after data load
                        // This checks status first - only shows dialog if status is .notDetermined
                        let result = await NotificationService.shared.requestAuthorization()

                        // Handle the authorization result
                        switch result {
                        case .granted:
                            break
                        case .denied, .previouslyDenied:
                            // User denied notifications - show alert directing them to Settings.
                            showNotificationDeniedAlert = true
                        case .error(let error):
                            _ = error
                            break
                        }

                        await coreManager.fetchData()
                        // Update badge after both authorization and data load complete
                        coreManager.updateAppBadge()
                    }
                } else {
                    coreManager.unregisterEventCallback()
                    // Clear badge on logout
                    Task {
                        await NotificationService.shared.clearBadge()
                    }
                }
            }
            .onChange(of: scenePhase) { _, newPhase in
                // Handle app becoming active
                if newPhase == .active && isLoggedIn {
                    Task {
                        // Refresh authorization status (user may have changed permissions in Settings)
                        await NotificationService.shared.checkAuthorizationStatus()
                        // Recalculate badge (may be stale after 48-hour window changes)
                        await MainActor.run {
                            coreManager.updateAppBadge()
                        }
                    }
                }
            }
            #if os(iOS)
            .alert("Notifications Disabled", isPresented: $showNotificationDeniedAlert) {
                Button("Open Settings") {
                    NotificationService.shared.openNotificationSettings()
                }
                Button("Not Now", role: .cancel) { }
            } message: {
                Text("To receive notifications when agents need your input, please enable notifications in Settings.")
            }
            #endif
        }
        #if os(macOS)
        .defaultSize(width: 1200, height: 800)
        #endif

        #if os(macOS)
        WindowGroup(id: "full-conversation", for: String.self) { $conversationId in
            if let conversationId {
                FullConversationWindow(conversationId: conversationId)
                    .environmentObject(coreManager)
            }
        }
        .defaultSize(width: 800, height: 700)

        WindowGroup(id: "delegation-tree", for: String.self) { $conversationId in
            if let id = conversationId {
                DelegationTreeView(rootConversationId: id)
                    .environmentObject(coreManager)
            }
        }
        .defaultSize(width: 1300, height: 820)
        #endif
    }

    private func attemptAutoLogin() {
        isAttemptingAutoLogin = true
        autoLoginError = nil

        // Check for debug nsec first (only in DEBUG builds)
        let debugNsec = getDebugNsec()

        DispatchQueue.global(qos: .userInitiated).async {
            // If debug nsec provided, attempt login with it directly
            if let nsec = debugNsec {
                do {
                    let loginResult = try coreManager.core.login(nsec: nsec)
                    DispatchQueue.main.async {
                        isAttemptingAutoLogin = false
                        if loginResult.success {
                            userNpub = loginResult.npub
                            isLoggedIn = true
                        } else {
                            autoLoginError = "Debug nsec login failed"
                        }
                    }
                    return
                } catch {
                    DispatchQueue.main.async {
                        isAttemptingAutoLogin = false
                        autoLoginError = "Debug nsec invalid: \(error.localizedDescription)"
                    }
                    return
                }
            }

            // Normal auto-login flow using stored credentials
            let result = coreManager.attemptAutoLogin()

            DispatchQueue.main.async {
                isAttemptingAutoLogin = false

                switch result {
                case .noCredentials:
                    // No stored credentials - show login screen
                    break

                case .success(let npub):
                    // Auto-login succeeded
                    userNpub = npub
                    isLoggedIn = true

                case .invalidCredential(let error):
                    // Credential was provably invalid - delete it and show login
                    Task {
                        _ = await coreManager.clearCredentials()
                    }
                    autoLoginError = "Stored credential was invalid. Please log in again."

                case .transientError(let error):
                    // Transient error - don't delete credentials, show login with warning
                    autoLoginError = "Could not auto-login: \(error)"
                }
            }
        }
    }
}

// MARK: - Main Tab View

enum AppSection: String, CaseIterable, Identifiable {
    case chats
    case projects
    case reports
    case inbox
    case search

    var id: String { rawValue }

    var title: String {
        switch self {
        case .chats: return "Chats"
        case .projects: return "Projects"
        case .reports: return "Reports"
        case .inbox: return "Inbox"
        case .search: return "Search"
        }
    }

    var systemImage: String {
        switch self {
        case .chats: return "bubble.left.and.bubble.right"
        case .projects: return "folder"
        case .reports: return "doc.richtext"
        case .inbox: return "tray"
        case .search: return "magnifyingglass"
        }
    }

    var accessibilityRowID: String {
        "section_row_\(rawValue)"
    }
}

struct MainTabView: View {
    @Binding var userNpub: String
    @Binding var isLoggedIn: Bool
    @EnvironmentObject var coreManager: TenexCoreManager

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    @State private var selectedTab = 0
    @State private var showAISettings = false
    @State private var showDiagnostics = false
    @State private var showStats = false
    @State private var runtimeText: String = "0m"

    private var useMailShellLayout: Bool {
        #if os(macOS)
        true
        #else
        horizontalSizeClass == .regular
        #endif
    }

    private func updateRuntime() async {
        let totalMs = await coreManager.safeCore.getTodayRuntimeMs()
        let totalSeconds = totalMs / 1000

        if totalSeconds < 60 {
            runtimeText = "\(totalSeconds)s"
        } else if totalSeconds < 3600 {
            runtimeText = "\(totalSeconds / 60)m"
        } else {
            let hours = totalSeconds / 3600
            let minutes = (totalSeconds % 3600) / 60
            runtimeText = minutes > 0 ? "\(hours)h \(minutes)m" : "\(hours)h"
        }
    }

    var body: some View {
        Group {
            if useMailShellLayout {
                MainShellView(
                    userNpub: $userNpub,
                    isLoggedIn: $isLoggedIn,
                    runtimeText: runtimeText,
                    onShowSettings: { showAISettings = true },
                    onShowDiagnostics: { showDiagnostics = true },
                    onShowStats: { showStats = true }
                )
                .environmentObject(coreManager)
                .nowPlayingInset(coreManager: coreManager)
            } else {
                compactTabView
            }
        }
        .task {
            await updateRuntime()
        }
        .onChange(of: coreManager.conversations) { _, _ in
            Task { await updateRuntime() }
        }
        .sheet(isPresented: $showAISettings) {
            AISettingsView()
                .tenexModalPresentation(detents: [.large])
                #if os(macOS)
                .frame(minWidth: 500, idealWidth: 520, minHeight: 500, idealHeight: 600)
                #endif
        }
        .sheet(isPresented: $showDiagnostics) {
            NavigationStack {
                DiagnosticsView(coreManager: coreManager)
                    .toolbar {
                        ToolbarItem(placement: .topBarLeading) {
                            Button("Done") { showDiagnostics = false }
                        }
                    }
            }
            .tenexModalPresentation(detents: [.large])
        }
        .sheet(isPresented: $showStats) {
            NavigationStack {
                StatsView(coreManager: coreManager)
                    .toolbar {
                        ToolbarItem(placement: .topBarLeading) {
                            Button("Done") { showStats = false }
                        }
                    }
            }
            .tenexModalPresentation(detents: [.large])
        }
        .ignoresSafeArea(.keyboard)
    }

    private var compactTabView: some View {
        TabView(selection: $selectedTab) {
            Tab("Chats", systemImage: "bubble.left.and.bubble.right", value: 0) {
                ConversationsTabView()
                    .environmentObject(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Projects", systemImage: "folder", value: 1) {
                ProjectsTabView()
                    .environmentObject(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Reports", systemImage: "doc.richtext", value: 4) {
                ReportsTabView()
                    .environmentObject(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }

            Tab("Inbox", systemImage: "tray", value: 3) {
                InboxView()
                    .environmentObject(coreManager)
                    .nowPlayingInset(coreManager: coreManager)
            }
            .badge(coreManager.unansweredAskCount)

            Tab(value: 10, role: .search) {
                SearchView()
                    .environmentObject(coreManager)
                .nowPlayingInset(coreManager: coreManager)
            } label: {
                Label("Search", systemImage: "magnifyingglass")
            }
        }
        #if os(iOS)
        .tabBarMinimizeBehavior(.onScrollDown)
        #endif
    }
}

struct MainShellView: View {
    @Binding var userNpub: String
    @Binding var isLoggedIn: Bool
    let runtimeText: String
    let onShowSettings: () -> Void
    let onShowDiagnostics: () -> Void
    let onShowStats: () -> Void

    @EnvironmentObject private var coreManager: TenexCoreManager

    @State private var selectedSection: AppSection? = .chats
    @State private var selectedConversation: ConversationFullInfo?
    @State private var selectedProjectId: String?
    @State private var selectedReport: ReportInfo?
    @State private var selectedInboxFilter: InboxFilter = .all
    @State private var selectedInboxItemId: String?
    @State private var activeInboxConversationId: String?
    @State private var selectedSearchConversation: ConversationFullInfo?

    private var currentSection: AppSection {
        selectedSection ?? .chats
    }

    var body: some View {
        NavigationSplitView {
            VStack(spacing: 0) {
                List(selection: $selectedSection) {
                    ForEach(AppSection.allCases) { section in
                        shellSidebarRow(for: section)
                            .tag(Optional(section))
                            .accessibilityIdentifier(section.accessibilityRowID)
                    }
                }
                .listStyle(.sidebar)
                .accessibilityIdentifier("app_sidebar")

                Divider()
                shellSidebarBottomBar
            }
            #if os(macOS)
            .navigationSplitViewColumnWidth(min: 210, ideal: 250, max: 300)
            #endif
        } content: {
            sectionListColumn
                .accessibilityIdentifier("section_list_column")
                #if os(macOS)
                .navigationSplitViewColumnWidth(min: 320, ideal: 420, max: 520)
                #endif
        } detail: {
            sectionDetailColumn
                .accessibilityIdentifier("detail_column")
        }
        .onChange(of: coreManager.projects.map(\.id)) { _, ids in
            if let selectedProjectId, !ids.contains(selectedProjectId) {
                self.selectedProjectId = nil
            }
        }
    }

    @ViewBuilder
    private func shellSidebarRow(for section: AppSection) -> some View {
        HStack(spacing: 10) {
            Label(section.title, systemImage: section.systemImage)

            Spacer(minLength: 8)

            if section == .inbox, coreManager.unansweredAskCount > 0 {
                Text("\(coreManager.unansweredAskCount)")
                    .font(.caption2.weight(.semibold))
                    .padding(.horizontal, 6)
                    .padding(.vertical, 2)
                    .background(Color.askBrandBackground)
                    .foregroundStyle(Color.askBrand)
                    .clipShape(Capsule())
            }
        }
    }

    private var shellSidebarBottomBar: some View {
        VStack(spacing: 10) {
            Button(action: onShowSettings) {
                Label("Settings", systemImage: "gearshape")
                    .frame(maxWidth: .infinity, alignment: .leading)
            }
            .buttonStyle(.plain)

            HStack(spacing: 8) {
                Menu {
                    if !userNpub.isEmpty {
                        Text(userNpub)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Divider()

                    Button(action: onShowDiagnostics) {
                        Label("Diagnostics", systemImage: "gauge.with.needle")
                    }

                    Button(action: onShowStats) {
                        Label("LLM Runtime", systemImage: "clock")
                    }

                    Divider()

                    Button(role: .destructive) {
                        Task {
                            _ = await coreManager.clearCredentials()
                            await MainActor.run {
                                userNpub = ""
                                isLoggedIn = false
                            }
                        }
                    } label: {
                        Label("Log Out", systemImage: "rectangle.portrait.and.arrow.right")
                    }
                } label: {
                    Label("You", systemImage: "person.crop.circle")
                }

                Spacer(minLength: 0)

                Button(action: onShowStats) {
                    Text(runtimeText)
                        .font(.subheadline)
                        .fontWeight(.medium)
                        .foregroundStyle(coreManager.hasActiveAgents ? Color.presenceOnline : .secondary)
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.horizontal, 12)
        .padding(.vertical, 10)
        .background(.bar)
    }

    @ViewBuilder
    private var sectionListColumn: some View {
        switch currentSection {
        case .chats:
            ConversationsTabView(layoutMode: .shellList, selectedConversation: $selectedConversation)
        case .projects:
            ProjectsSectionListColumn(selectedProjectId: $selectedProjectId)
        case .reports:
            ReportsTabView(layoutMode: .shellList, selectedReport: $selectedReport)
        case .inbox:
            InboxView(
                layoutMode: .shellList,
                selectedFilter: $selectedInboxFilter,
                selectedItemId: $selectedInboxItemId,
                activeConversationId: $activeInboxConversationId
            )
        case .search:
            SearchView(layoutMode: .shellList, selectedConversation: $selectedSearchConversation)
        }
    }

    @ViewBuilder
    private var sectionDetailColumn: some View {
        switch currentSection {
        case .chats:
            ConversationsTabView(layoutMode: .shellDetail, selectedConversation: $selectedConversation)
        case .projects:
            ProjectsSectionDetailColumn(selectedProjectId: $selectedProjectId)
        case .reports:
            ReportsTabView(layoutMode: .shellDetail, selectedReport: $selectedReport)
        case .inbox:
            InboxView(
                layoutMode: .shellDetail,
                selectedFilter: $selectedInboxFilter,
                selectedItemId: $selectedInboxItemId,
                activeConversationId: $activeInboxConversationId
            )
        case .search:
            SearchView(layoutMode: .shellDetail, selectedConversation: $selectedSearchConversation)
        }
    }
}

private struct ProjectsSectionListColumn: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
    @Binding var selectedProjectId: String?

    private var sortedProjects: [ProjectInfo] {
        coreManager.projects.sorted { a, b in
            let aOnline = coreManager.projectOnlineStatus[a.id] ?? false
            let bOnline = coreManager.projectOnlineStatus[b.id] ?? false
            if aOnline != bOnline { return aOnline }
            return a.title.localizedCaseInsensitiveCompare(b.title) == .orderedAscending
        }
    }

    var body: some View {
        List(selection: $selectedProjectId) {
            ForEach(sortedProjects, id: \.id) { project in
                HStack(spacing: 10) {
                    RoundedRectangle(cornerRadius: 7)
                        .fill(deterministicColor(for: project.id).gradient)
                        .frame(width: 28, height: 28)
                        .overlay {
                            Image(systemName: "folder.fill")
                                .font(.caption)
                                .foregroundStyle(.white)
                        }

                    VStack(alignment: .leading, spacing: 2) {
                        Text(project.title)
                            .font(.headline)
                            .lineLimit(1)

                        Text((coreManager.projectOnlineStatus[project.id] ?? false) ? "Online" : "Offline")
                            .font(.caption)
                            .foregroundStyle((coreManager.projectOnlineStatus[project.id] ?? false) ? Color.presenceOnline : .secondary)
                    }

                    Spacer()
                }
                .tag(Optional(project.id))
            }
        }
        #if os(macOS)
        .listStyle(.inset)
        #else
        .listStyle(.plain)
        #endif
        .navigationTitle("Projects")
    }
}

private struct ProjectsSectionDetailColumn: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
    @Binding var selectedProjectId: String?

    @State private var isBooting = false
    @State private var showBootError = false
    @State private var bootErrorMessage: String?

    private var selectedProject: ProjectInfo? {
        guard let selectedProjectId else { return nil }
        return coreManager.projects.first { $0.id == selectedProjectId }
    }

    var body: some View {
        Group {
            if let project = selectedProject {
                ScrollView {
                    VStack(alignment: .leading, spacing: 18) {
                        HStack(spacing: 12) {
                            RoundedRectangle(cornerRadius: 10)
                                .fill(deterministicColor(for: project.id).gradient)
                                .frame(width: 44, height: 44)
                                .overlay {
                                    Image(systemName: "folder.fill")
                                        .foregroundStyle(.white)
                                }

                            VStack(alignment: .leading, spacing: 4) {
                                Text(project.title)
                                    .font(.title2.weight(.semibold))
                                Text(project.id)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }

                        let isOnline = coreManager.projectOnlineStatus[project.id] ?? false
                        let onlineAgents = coreManager.onlineAgents[project.id]?.count ?? 0

                        HStack(spacing: 8) {
                            Circle()
                                .fill(isOnline ? Color.presenceOnline : Color.secondary)
                                .frame(width: 10, height: 10)
                            Text(isOnline ? "Online" : "Offline")
                                .font(.headline)
                            if isOnline {
                                Text("• \(onlineAgents) agent\(onlineAgents == 1 ? "" : "s")")
                                    .foregroundStyle(.secondary)
                            }
                        }

                        if !isOnline {
                            Button {
                                bootProject(project.id)
                            } label: {
                                if isBooting {
                                    ProgressView()
                                } else {
                                    Label("Boot Project", systemImage: "power")
                                }
                            }
                            .buttonStyle(.borderedProminent)
                            .disabled(isBooting)
                        }

                        Spacer(minLength: 0)
                    }
                    .padding(24)
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
            } else {
                ContentUnavailableView(
                    "Select a Project",
                    systemImage: "folder",
                    description: Text("Choose a project from the list")
                )
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .alert("Boot Failed", isPresented: $showBootError) {
            Button("OK") { bootErrorMessage = nil }
        } message: {
            if let bootErrorMessage {
                Text(bootErrorMessage)
            }
        }
    }

    private func bootProject(_ projectId: String) {
        isBooting = true
        bootErrorMessage = nil

        Task {
            do {
                try await coreManager.safeCore.bootProject(projectId: projectId)
            } catch {
                await MainActor.run {
                    bootErrorMessage = error.localizedDescription
                    showBootError = true
                }
            }
            await MainActor.run {
                isBooting = false
            }
        }
    }
}

struct ProjectsTabView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var selectedProjectIds: Set<String> = []

    var body: some View {
        NavigationStack {
            ProjectsContentView(selectedProjectIds: $selectedProjectIds)
                .environmentObject(coreManager)
        }
    }
}
