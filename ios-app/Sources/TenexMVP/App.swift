import SwiftUI
import CryptoKit

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
    @Published var messagesByConversation: [String: [MessageInfo]] = [:]
    @Published private(set) var statsVersion: UInt64 = 0
    @Published private(set) var diagnosticsVersion: UInt64 = 0
    @Published private(set) var liveFeed: [LiveFeedItem] = []
    @Published private(set) var liveFeedLastReceivedAt: Date?
    @Published var streamingBuffers: [String: StreamingBuffer] = [:]

    private let liveFeedMaxItems = 400

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

    /// Register the event callback for push-based updates.
    /// Call this after successful login to enable real-time updates.
    func registerEventCallback() {
        let handler = TenexEventHandler(coreManager: self)
        eventHandler = handler
        core.setEventCallback(callback: handler)
        print("[TenexCoreManager] Event callback registered")
    }

    /// Unregister the event callback.
    /// Call this on logout to clean up resources.
    func unregisterEventCallback() {
        core.clearEventCallback()
        eventHandler = nil
        print("[TenexCoreManager] Event callback unregistered")
    }

    /// Manual refresh for pull-to-refresh gesture (optional)
    func manualRefresh() async {
        await syncNow()
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
        recordLiveFeedItem(conversationId: conversationId, message: message)
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
                print("[TenexCoreManager] Failed to approve backend '\(backendPubkey)': \(error)")
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
            let filter = ConversationFilter(
                projectIds: [],
                showArchived: false,
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
                print("[TenexCoreManager] getProjects failed: \(error)")
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
        messageText: String
    ) async {
        // Load API keys from iOS Keychain
        let elevenlabsResult = await KeychainService.shared.loadElevenLabsApiKeyAsync()
        let openrouterResult = await KeychainService.shared.loadOpenRouterApiKeyAsync()

        guard case .success(let elevenlabsKey) = elevenlabsResult,
              case .success(let openrouterKey) = openrouterResult else {
            print("[TenexCoreManager] Audio notification skipped: API keys not configured in keychain")
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
                do {
                    try AudioNotificationPlayer.shared.play(path: notification.audioFilePath)
                } catch {
                    print("[TenexCoreManager] Failed to play audio notification: \(error)")
                }
            }
        } catch {
            print("[TenexCoreManager] Audio notification skipped: \(error)")
        }
    }

    @MainActor
    func recordLiveFeedItem(conversationId: String, message: MessageInfo) {
        if liveFeed.contains(where: { $0.id == message.id }) {
            return
        }

        liveFeed.insert(LiveFeedItem(conversationId: conversationId, message: message), at: 0)
        if liveFeed.count > liveFeedMaxItems {
            liveFeed.removeLast(liveFeed.count - liveFeedMaxItems)
        }
        liveFeedLastReceivedAt = liveFeed.first?.receivedAt
    }

    @MainActor
    func clearLiveFeed() {
        liveFeed.removeAll()
        liveFeedLastReceivedAt = nil
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
            print("[TenexCoreManager] Fetched \(agents.count) agents for project '\(projectId)'")
        } catch {
            print("[TenexCoreManager] Failed to fetch agents for project '\(projectId)': \(error)")
            // Cache empty array on failure to prevent stale data
            await MainActor.run { self.setOnlineAgentsCache([], for: projectId) }
            return
        }

        // Only hop to main actor to mutate state
        await MainActor.run {
            self.setOnlineAgentsCache(agents, for: projectId)
            print("[TenexCoreManager] Cached agents, onlineAgents['\(projectId)'] now has \(self.onlineAgents[projectId]?.count ?? 0) agents")
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
                print("[TenexCoreManager] Auto-approved \(approved) backend(s)")
            } else {
                print("[TenexCoreManager] No pending backends to approve")
            }
        } catch {
            print("[TenexCoreManager] Failed to approve pending backends: \(error)")
        }

        do {
            let filter = ConversationFilter(
                projectIds: [],
                showArchived: true,
                hideScheduled: true,
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
            print("[TenexCoreManager] Fetch failed: \(error)")
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
            print("[TenexCoreManager] Failed to get profile picture for pubkey '\(pubkey.prefix(pubkeyDisplayPrefixLength))...': \(error)")
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
                        print("[TenexCoreManager] Prefetch failed for pubkey '\(pubkey.prefix(pubkeyDisplayPrefixLength))...': \(error)")
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

@main
struct TenexMVPApp: App {
    @StateObject private var coreManager = TenexCoreManager()
    @State private var isLoggedIn = false
    @State private var userNpub = ""
    @State private var isAttemptingAutoLogin = false
    @State private var autoLoginError: String?
    @Environment(\.scenePhase) private var scenePhase

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
                                .foregroundStyle(.red)
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
                    coreManager.clearLiveFeed()
                    coreManager.registerEventCallback()
                    // Initial data fetch on login
                    Task { @MainActor in
                        await coreManager.fetchData()
                    }
                } else {
                    coreManager.unregisterEventCallback()
                    coreManager.clearLiveFeed()
                }
            }
        }
        #if os(macOS)
        .defaultSize(width: 1200, height: 800)
        #endif

        #if os(macOS)
        WindowGroup(id: "conversation-summary", for: String.self) { $conversationId in
            if let conversationId {
                ConversationSummaryWindow(conversationId: conversationId)
                    .environmentObject(coreManager)
            }
        }
        .defaultSize(width: 700, height: 600)

        WindowGroup(id: "full-conversation", for: String.self) { $conversationId in
            if let conversationId {
                FullConversationWindow(conversationId: conversationId)
                    .environmentObject(coreManager)
            }
        }
        .defaultSize(width: 800, height: 700)
        #endif
    }

    private func attemptAutoLogin() {
        isAttemptingAutoLogin = true
        autoLoginError = nil

        DispatchQueue.global(qos: .userInitiated).async {
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
                    print("[TENEX] Stored credential invalid: \(error)")
                    Task {
                        _ = await coreManager.clearCredentials()
                    }
                    autoLoginError = "Stored credential was invalid. Please log in again."

                case .transientError(let error):
                    // Transient error - don't delete credentials, show login with warning
                    print("[TENEX] Auto-login transient error: \(error)")
                    autoLoginError = "Could not auto-login: \(error)"
                }
            }
        }
    }
}

// MARK: - Main Tab View

struct MainTabView: View {
    @Binding var userNpub: String
    @Binding var isLoggedIn: Bool
    @EnvironmentObject var coreManager: TenexCoreManager

    @State private var selectedTab = 0

    var body: some View {
        TabView(selection: $selectedTab) {
            Tab("Conversations", systemImage: "bubble.left.and.bubble.right", value: 0) {
                NavigationStack {
                    ConversationsTabView()
                        .environmentObject(coreManager)
                }
            }
            Tab("Feed", systemImage: "dot.radiowaves.left.and.right", value: 1) {
                NavigationStack {
                    FeedView()
                        .environmentObject(coreManager)
                }
            }
            Tab("Projects", systemImage: "folder", value: 2) {
                ContentView(userNpub: $userNpub, isLoggedIn: $isLoggedIn)
                    .environmentObject(coreManager)
            }
            Tab("Inbox", systemImage: "tray", value: 3) {
                InboxView()
                    .environmentObject(coreManager)
            }
            Tab("Search", systemImage: "magnifyingglass", value: 10) {
                NavigationStack {
                    SearchView()
                        .environmentObject(coreManager)
                }
            }
        }
        .overlay(alignment: .topTrailing) {
            AudioPlayingIndicator()
                .padding(.top, 50)
                .padding(.trailing, 8)
        }
    }
}
