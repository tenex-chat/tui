import SwiftUI
import CryptoKit
import Observation

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

/// Shared TenexCore instance wrapper for environment injection.
/// Uses @Observable for property-level observation tracking,
/// eliminating whole-tree invalidation on any single property change.
@Observable @MainActor
class TenexCoreManager {
    @ObservationIgnored let core: TenexCore
    /// Thread-safe async wrapper for FFI access with proper error handling
    @ObservationIgnored let safeCore: SafeTenexCore
    @ObservationIgnored let profiler = PerformanceProfiler.shared
    var isInitialized = false
    var initializationError: String?

    // MARK: - Centralized Reactive Data Store
    // With @Observable, SwiftUI tracks reads at the property level.
    // Views only re-render when the specific properties they read change.
    var projects: [Project] = []
    var conversations: [ConversationFullInfo] = [] {
        didSet { rebuildConversationById() }
    }
    /// App-filter base conversation scope (project/time constrained) used to derive
    /// status/hashtag facets and apply local refinements without extra FFI calls.
    var appFilterConversationScope: [ConversationFullInfo] = []

    /// O(1) lookup by thread ID. Rebuilt automatically when `conversations` changes.
    @ObservationIgnored private(set) var conversationById: [String: ConversationFullInfo] = [:]

    private func rebuildConversationById() {
        var map = [String: ConversationFullInfo](minimumCapacity: conversations.count)
        for conv in conversations {
            map[conv.thread.id] = conv
        }
        conversationById = map
    }
    var inboxItems: [InboxItem] = []
    var reports: [Report] = []
    var messagesByConversation: [String: [Message]] = [:]
    private(set) var statsVersion: UInt64 = 0
    private(set) var teamsVersion: UInt64 = 0
    private(set) var diagnosticsVersion: UInt64 = 0
    var streamingBuffers: [String: StreamingBuffer] = [:]

    /// Bookmarked nudge/skill IDs (kind:14202). Updated reactively when BookmarkListChanged fires.
    var bookmarkedIds: Set<String> = []

    /// Project online status - updated reactively via event callbacks.
    /// Key: project ID, Value: true if online.
    var projectOnlineStatus: [String: Bool] = [:]

    /// Online agents for each project - updated reactively via event callbacks.
    /// Key: project ID, Value: array of ProjectAgent.
    var onlineAgents: [String: [ProjectAgent]] = [:]

    /// Whether any conversation currently has active agents (24133 events with agents)
    /// Used to highlight the runtime indicator when work is happening
    var hasActiveAgents: Bool = false

    /// Pending NIP-46 bunker signing requests awaiting user approval.
    var pendingBunkerRequests: [FfiBunkerSignRequest] = []

    /// Formatted today's LLM runtime for statusbar display.
    var runtimeText: String = "0m"

    /// Last project ID tombstoned via a push upsert.
    /// Used by view selection state to clear deleted-project detail panes immediately.
    private(set) var lastDeletedProjectId: String?

    @MainActor
    func setLastDeletedProjectId(_ projectId: String?) {
        lastDeletedProjectId = projectId
    }

    // MARK: - Global App Filter

    var appFilterProjectIds: Set<String>
    var appFilterTimeWindow: AppTimeWindow
    var appFilterScheduledEvent: ScheduledEventFilter
    var appFilterStatus: ConversationStatusFilter
    var appFilterHashtags: Set<String>
    var appFilterShowArchived: Bool

    static let appFilterProjectsDefaultsKey = "app.global.filter.projectIds"
    static let appFilterTimeWindowDefaultsKey = "app.global.filter.timeWindow"
    static let appFilterScheduledEventDefaultsKey = "app.global.filter.scheduledEvent"
    static let appFilterStatusDefaultsKey = "app.global.filter.statusLabel"
    static let appFilterHashtagsDefaultsKey = "app.global.filter.hashtags"
    static let appFilterShowArchivedDefaultsKey = "app.global.filter.showArchived"

    // MARK: - Ask Badge Support

    /// Count of unanswered ask events within the selected global filter scope.
    private(set) var unansweredAskCount: Int = 0

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
    @ObservationIgnored var eventHandler: TenexEventHandler?

    /// Task reference for project status updates - enables cancellation of stale refreshes
    @ObservationIgnored var projectStatusUpdateTask: Task<Void, Never>?
    /// Task reference for app-filtered conversation refreshes
    @ObservationIgnored var conversationRefreshTask: Task<Void, Never>?
    /// Coalesced streaming chunks pending publish to reduce SwiftUI invalidation storms.
    @ObservationIgnored var pendingStreamingDeltas: [String: PendingStreamingDelta] = [:]
    @ObservationIgnored var streamingFlushTask: Task<Void, Never>?
    /// In-flight message refreshes keyed by conversation to prevent redundant FFI fetch storms.
    @ObservationIgnored var inflightConversationMessageRefreshes: Set<String> = []
    /// Last message refresh timestamp per conversation for lightweight throttling.
    @ObservationIgnored var lastConversationMessageRefreshAt: [String: CFAbsoluteTime] = [:]

    /// Cache for profile picture URLs to prevent repeated FFI calls
    @ObservationIgnored nonisolated let profilePictureCache = ProfilePictureCache.shared

    // MARK: - Performance Caches

    /// Cache for conversation hierarchy data to prevent N+1 FFI calls in list views
    @ObservationIgnored let hierarchyCache = ConversationHierarchyCache()

    // MARK: - Profile Name Resolution

    @ObservationIgnored private var profileNameCache: [String: String] = [:]

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
        appFilterProjectIds = persistedFilter.projectIds
        appFilterTimeWindow = persistedFilter.timeWindow
        appFilterScheduledEvent = persistedFilter.scheduledEvent
        appFilterStatus = persistedFilter.status
        appFilterHashtags = persistedFilter.hashtags
        appFilterShowArchived = persistedFilter.showArchived

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
            print("[TENEX PERF] core.init starting")
            let startedAt = CFAbsoluteTimeGetCurrent()
            let success = tenexCore.`init`()
            let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
            print("[TENEX PERF] core.init finished success=\(success) totalMs=\(String(format: "%.0f", elapsedMs))")

            // Read the Rust sub-step timings from tenex.log and forward them so they
            // appear in the Xcode console and Swift perf log alongside the Swift timings.
            let rustLines = Self.readRustInitTimings()

            DispatchQueue.main.async {
                for line in rustLines {
                    print("[TENEX PERF] rust: \(line)")
                    self?.profiler.logEvent("rust-init: \(line)", category: .general)
                }
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
    @ObservationIgnored var sessionStartTimestamp: UInt64 = 0

    /// Last time the user sent a message per conversation (Unix seconds).
    /// Used to skip TTS when the user was recently active in a conversation.
    @ObservationIgnored var lastUserActivityByConversation: [String: UInt64] = [:]

    // NOTE: Push-based delta application and event reaction methods live in
    // `TenexCoreManager+Callbacks.swift` to keep this root type focused on state + wiring.

    @MainActor
    func bumpStatsVersion() {
        statsVersion &+= 1
    }

    @MainActor
    func bumpTeamsVersion() {
        teamsVersion &+= 1
    }

    @MainActor
    func bumpDiagnosticsVersion() {
        diagnosticsVersion &+= 1
    }

    static func projectId(fromATag aTag: String) -> String {
        let parts = aTag.split(separator: ":")
        guard parts.count >= 3 else { return "" }
        return parts.dropFirst(2).joined(separator: ":")
    }

    /// Reads the last ffi.init timing block from the Rust log file and returns the lines.
    /// Rust writes detailed per-step PERF logs to tenex.log during init; this surfaces them
    /// to the Swift profiler so they appear in the Xcode console and perf log.
    private nonisolated static func readRustInitTimings() -> [String] {
        guard let base = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first else {
            return ["error: no Application Support directory found"]
        }
        let logURL = base.appendingPathComponent("tenex/nostrdb/tenex.log")
        guard let handle = try? FileHandle(forReadingFrom: logURL) else {
            return ["tenex.log not found at \(logURL.path)"]
        }
        defer { handle.closeFile() }

        // Read only the last 8 KB — ffi.init logs are small and always at the tail.
        let fileSize = handle.seekToEndOfFile()
        let offset = fileSize > 8192 ? fileSize - 8192 : 0
        handle.seek(toFileOffset: offset)
        let data = handle.readDataToEndOfFile()
        guard let tail = String(data: data, encoding: .utf8) else {
            return ["failed to decode tenex.log"]
        }

        // Extract the most recent ffi.init block (start → complete).
        // If we see a second "ffi.init start" we reset, keeping only the latest session.
        var capturing = false
        var block: [String] = []
        for line in tail.components(separatedBy: "\n") {
            if line.contains("ffi.init start") {
                capturing = true
                block = [line]
            } else if capturing {
                block.append(line)
                if line.contains("ffi.init complete") {
                    capturing = false
                }
            }
        }
        return block.filter { !$0.isEmpty }
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
