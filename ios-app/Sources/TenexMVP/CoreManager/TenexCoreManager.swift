import SwiftUI
import CryptoKit
import Combine

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
    @Published var projects: [Project] = []
    @Published var conversations: [ConversationFullInfo] = []
    @Published var inboxItems: [InboxItem] = []
    @Published var reports: [Report] = []
    @Published var messagesByConversation: [String: [Message]] = [:]
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
    /// Key: project ID, Value: array of ProjectAgent.
    /// Subscribe to this instead of fetching agents on-demand via getOnlineAgents().
    /// This eliminates multi-second delays from redundant FFI calls.
    @Published var onlineAgents: [String: [ProjectAgent]] = [:]

    /// Whether any conversation currently has active agents (24133 events with agents)
    /// Used to highlight the runtime indicator when work is happening
    @Published var hasActiveAgents: Bool = false

    /// Formatted today's LLM runtime for statusbar display.
    /// Centralized here to avoid duplicate updateRuntime() in multiple views.
    /// Reads directly from TenexCore (lock-free AtomicU64) without actor serialization.
    @Published var runtimeText: String = "0m"

    /// Last project ID tombstoned via a push upsert.
    /// Used by view selection state to clear deleted-project detail panes immediately.
    @Published private(set) var lastDeletedProjectId: String?

    @MainActor
    func setLastDeletedProjectId(_ projectId: String?) {
        lastDeletedProjectId = projectId
    }

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
            guard item.eventType == .ask, !item.isRead else { continue }
            if snapshot.includes(projectId: item.resolvedProjectId, timestamp: item.createdAt, now: resolvedNow) {
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

    // MARK: - Profile Name Resolution

    private var profileNameCache: [String: String] = [:]

    /// Resolves a display name for a pubkey via kind:0 profile metadata (cached).
    func displayName(for pubkey: String) -> String {
        if let cached = profileNameCache[pubkey] {
            return cached
        }
        let name = core.getProfileName(pubkey: pubkey)
        profileNameCache[pubkey] = name
        return name
    }

    /// Invalidates the profile name cache so next access re-fetches from core.
    func invalidateProfileNameCache() {
        profileNameCache.removeAll()
    }

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
    /// Runs refresh() via Task.detached to avoid blocking the SafeTenexCore actor queue,
    /// which would cause priority inversion for lightweight reads like getTodayRuntimeMs().
    func syncNow() async {
        let core = self.core
        await Task.detached {
            _ = core.refresh()
        }.value
    }

    // MARK: - Event Callback Registration

    /// Timestamp (Unix seconds) when the event callback was registered.
    /// Used to filter out stale inbox items that arrived before this session started.
    var sessionStartTimestamp: UInt64 = 0

    /// Last time the user sent a message per conversation (Unix seconds).
    /// Used to skip TTS when the user was recently active in a conversation.
    var lastUserActivityByConversation: [String: UInt64] = [:]

    // NOTE: Push-based delta application and event reaction methods live in
    // `TenexCoreManager+Callbacks.swift` to keep this root type focused on state + wiring.

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

    static func projectId(fromATag aTag: String) -> String {
        let parts = aTag.split(separator: ":")
        guard parts.count >= 3 else { return "" }
        return parts.dropFirst(2).joined(separator: ":")
    }

    /// Refresh `runtimeText` from the Rust FFI.
    /// The Rust side holds an RwLock (not lock-free despite the atomic cache),
    /// so the FFI call runs on a detached task to avoid blocking the main thread.
    @MainActor
    func refreshRuntimeText() {
        let core = self.core
        Task.detached {
            let totalMs = core.getTodayRuntimeMs()
            let totalSeconds = totalMs / 1000
            let text: String
            if totalSeconds < 60 {
                text = "\(totalSeconds)s"
            } else if totalSeconds < 3600 {
                text = "\(totalSeconds / 60)m"
            } else {
                let hours = totalSeconds / 3600
                let minutes = (totalSeconds % 3600) / 60
                text = minutes > 0 ? "\(hours)h \(minutes)m" : "\(hours)h"
            }
            await MainActor.run { [weak self] in
                self?.runtimeText = text
            }
        }
    }

}
