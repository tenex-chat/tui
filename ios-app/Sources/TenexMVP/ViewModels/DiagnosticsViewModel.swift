import Foundation
import Combine
import os.log
import SwiftUI

/// ViewModel for the Diagnostics tab
/// Manages diagnostic data fetching from Rust core
///
/// ## Concurrency Strategy (Latest-Wins + Deduplication + Fetch Identity)
/// - If an in-flight request already covers the new request (includeDB=true covers includeDB=false), ignore new call
/// - Otherwise, cancel existing task and start fresh with a new fetchID
/// - All state mutations (isLoading, error, snapshot, cleanup) are gated by fetchID
/// - This prevents stale tasks from clobbering state set by newer tasks
/// - Error state derives from snapshot.sectionErrors (single source of truth)
///
/// ## Race Condition Prevention
/// When Task A is canceled and Task B starts, Task A's completion handlers check
/// the fetchID before mutating state. If fetchID doesn't match (Task B has a new ID),
/// Task A skips all state mutations, preventing stale-task state clobbering.
@MainActor
class DiagnosticsViewModel: ObservableObject {
    // MARK: - Published Properties

    /// Current diagnostics snapshot from Rust core
    /// Note: sectionErrors are accessed directly from snapshot (single source of truth)
    @Published var snapshot: DiagnosticsSnapshot?
    @Published var snapshotCapturedAt = Date()

    /// Loading state
    @Published var isLoading = false

    /// Error state for overall fetch failures (only set when we have NO snapshot)
    @Published var error: Error?

    /// Bunker audit log entries
    @Published var bunkerAuditLog: [FfiBunkerAuditEntry] = []

    /// Selected diagnostics subtab
    ///
    /// **Side Effect:** When switching to the `.database` tab, this property automatically
    /// triggers a lazy load of database statistics if they haven't been loaded yet.
    /// When switching to `.bunker`, triggers lazy load of the audit log.
    @Published var selectedTab: DiagnosticsTab = .overview {
        didSet {
            // Lazy load database stats when user navigates to Database tab
            if selectedTab == .database && snapshot?.database == nil {
                Task {
                    await loadDiagnostics(includeDatabaseStats: true)
                }
            }
            // Load bunker audit log whenever user navigates to the Bunker tab.
            if selectedTab == .bunker {
                Task {
                    await loadBunkerAuditLog()
                }
            }
        }
    }

    // MARK: - Dependencies

    private let coreManager: TenexCoreManager

    // MARK: - Task Management (Concurrency + Cancellation)

    /// Currently running fetch task (for cancellation and in-flight tracking)
    private var currentFetchTask: Task<Void, Never>?

    /// Whether the current in-flight request includes database stats
    /// Used for request deduplication (includeDB=true covers includeDB=false)
    private var currentFetchIncludesDB = false

    /// Unique identifier for the current fetch operation
    /// Used to prevent stale tasks from clobbering state set by newer tasks
    /// When Task A is canceled and Task B starts, Task A's completion handlers
    /// check this ID and skip state mutations if they don't match
    private var currentFetchID: UUID?

    private var subscriptions = Set<AnyCancellable>()

    // MARK: - Initialization

    init(coreManager: TenexCoreManager) {
        self.coreManager = coreManager
        bindToUpdates()
    }

    // MARK: - Public Methods

    /// Load diagnostics data from Rust core using SafeTenexCore
    ///
    /// Implements latest-wins + deduplication concurrency strategy with fetch identity:
    /// - If in-flight request already covers new request (includeDB=true covers false), ignore new call
    /// - Otherwise, cancel existing task and start fresh with a new fetchID
    /// - All state mutations are gated by fetchID to prevent stale tasks from clobbering newer state
    ///
    /// - Parameter includeDatabaseStats: Whether to include expensive DB stats (default: based on tab)
    func loadDiagnostics(includeDatabaseStats: Bool? = nil) async {
        // Determine whether to include database stats
        // Default: only include if Database tab is active (lazy loading optimization)
        let includeDB = includeDatabaseStats ?? (selectedTab == .database)

        // CONCURRENCY: Deduplication check
        // If there's an in-flight request that already includes DB stats (or we don't need them),
        // the current request is covered - skip this call
        if currentFetchTask != nil && (currentFetchIncludesDB || !includeDB) {
            return
        }

        // CONCURRENCY: Latest-wins - cancel existing task if new request needs more data
        // Note: This doesn't stop the underlying Rust FFI work (no token support),
        // but the fetchID check prevents stale results from being applied
        currentFetchTask?.cancel()

        // Generate unique identifier for this fetch operation
        // This gates ALL state mutations to prevent stale tasks from clobbering state
        let fetchID = UUID()
        currentFetchID = fetchID

        isLoading = true
        currentFetchIncludesDB = includeDB

        // Capture safeCore for actor-isolated access
        let safeCore = self.coreManager.safeCore

        // Create a new cancellable task
        let task = Task { [weak self] in
            // Use defer to ensure state cleanup on any exit path
            // CRITICAL: Only cleanup if this task is still the current one (fetchID matches)
            defer {
                Task { @MainActor [weak self] in
                    guard self?.currentFetchID == fetchID else { return }
                    self?.currentFetchTask = nil
                    self?.currentFetchIncludesDB = false
                    self?.isLoading = false
                }
            }

            guard let self = self else { return }

            do {
                // Check for cancellation before starting
                try Task.checkCancellation()

                // PHASE 1: Show cached data immediately (no blocking on network)
                let cachedSnapshot = await safeCore.getDiagnosticsSnapshot(includeDatabaseStats: includeDB)
                await MainActor.run { [weak self] in
                    guard self?.currentFetchID == fetchID else { return }
                    self?.snapshot = cachedSnapshot
                    self?.snapshotCapturedAt = Date()
                    self?.error = nil
                }

                // Check for cancellation before reload
                try Task.checkCancellation()

                // PHASE 2: Get fresh data from local store
                let freshSnapshot = await safeCore.getDiagnosticsSnapshot(includeDatabaseStats: includeDB)

                // IDENTITY CHECK: Guard UI updates - only apply if this is still the current fetch
                await MainActor.run { [weak self] in
                    guard self?.currentFetchID == fetchID else { return }
                    self?.snapshot = freshSnapshot
                    self?.snapshotCapturedAt = Date()
                    self?.error = nil
                }
            } catch is CancellationError {
                // Task was cancelled, keep existing data
            } catch {
                // Keep showing cached data if fetch fails
                await MainActor.run { [weak self] in
                    guard self?.currentFetchID == fetchID else { return }
                    if self?.snapshot == nil {
                        self?.error = error
                    }
                }
            }
        }

        currentFetchTask = task

        // Wait for the task to complete
        await task.value
    }

    /// Load bunker audit log from Rust core
    func loadBunkerAuditLog() async {
        let safeCore = self.coreManager.safeCore
        do {
            let entries = try await safeCore.getBunkerAuditLog()
            self.bunkerAuditLog = entries
        } catch {
            // Silently fail - empty log is fine
        }
    }

    /// Handle Diagnostics-version bumps from the core.
    /// When Bunker is active we refresh bunker audit log directly; other tabs refresh diagnostics snapshot.
    func handleDiagnosticsVersionUpdate() async {
        if selectedTab == .bunker {
            await loadBunkerAuditLog()
        } else {
            await loadDiagnostics()
        }
    }

    /// Refresh diagnostics data (for pull-to-refresh)
    /// Always includes database stats since user is explicitly refreshing
    func refresh() async {
        await coreManager.syncNow()
        await loadDiagnostics(includeDatabaseStats: true)
    }

    /// Cancel any in-flight fetch operation
    /// Resets fetchID to prevent any stale task completion handlers from updating state
    func cancelFetch() {
        currentFetchTask?.cancel()
        currentFetchTask = nil
        currentFetchIncludesDB = false
        currentFetchID = nil  // Invalidate any in-flight fetch identity
        isLoading = false
    }

    deinit {
        currentFetchTask?.cancel()
    }

    private func bindToUpdates() {
        $selectedTab
            .removeDuplicates()
            .sink { [weak self] _ in
                guard let self = self else { return }
                Task { await self.loadDiagnostics() }
            }
            .store(in: &subscriptions)
    }
}

// MARK: - Diagnostics Tab Enum

enum DiagnosticsTab: String, CaseIterable, Identifiable {
    case overview = "Overview"
    case sync = "Sync"
    case subscriptions = "Subscriptions"
    case database = "Database"
    case bunker = "Bunker"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .overview: return "gauge.with.needle"
        case .sync: return "arrow.triangle.2.circlepath"
        case .subscriptions: return "antenna.radiowaves.left.and.right"
        case .database: return "cylinder"
        case .bunker: return "lock.shield"
        }
    }
}

// MARK: - Shared Formatting Helpers

/// Centralized formatting helpers for Diagnostics views
enum DiagnosticsFormatters {
    /// Format large numbers with K/M suffixes for readability
    static func formatNumber(_ value: UInt64) -> String {
        if value >= 1_000_000 {
            return String(format: "%.1fM", Double(value) / 1_000_000)
        } else if value >= 1_000 {
            return String(format: "%.1fK", Double(value) / 1_000)
        } else {
            return "\(value)"
        }
    }

    /// Format duration in milliseconds to human-readable string
    static func formatDuration(_ ms: UInt64) -> String {
        let totalSeconds = ms / 1000
        let hours = totalSeconds / 3600
        let minutes = (totalSeconds % 3600) / 60
        let seconds = totalSeconds % 60

        if hours > 0 {
            return "\(hours)h \(minutes)m"
        } else if minutes > 0 {
            return "\(minutes)m \(seconds)s"
        } else {
            return "\(seconds)s"
        }
    }
}

// MARK: - Helper Extensions

extension DiagnosticsSnapshot {
    /// Format bytes to human-readable string (KB, MB, GB)
    static func formatBytes(_ bytes: UInt64) -> String {
        let kb = Double(bytes) / 1024
        let mb = kb / 1024
        let gb = mb / 1024

        if gb >= 1 {
            return String(format: "%.1f GB", gb)
        } else if mb >= 1 {
            return String(format: "%.1f MB", mb)
        } else if kb >= 1 {
            return String(format: "%.1f KB", kb)
        } else {
            return "\(bytes) B"
        }
    }

}

// MARK: - Business Logic Extensions

extension NegentropySyncDiagnostics {
    /// Calculated success rate for sync operations
    /// Returns 100% if no syncs have been attempted yet
    var successRate: Double {
        let totalSyncs = successfulSyncs + failedSyncs
        guard totalSyncs > 0 else { return 100.0 }
        return Double(successfulSyncs) / Double(totalSyncs) * 100
    }

    /// Color based on success rate thresholds
    /// - >= 90%: green
    /// - >= 70%: orange
    /// - < 70%: red
    var successRateColor: SwiftUI.Color {
        if successRate >= 90 {
            return .green
        } else if successRate >= 70 {
            return .orange
        } else {
            return .red
        }
    }
}

extension DiagnosticsSnapshot {
    /// Subscriptions sorted by events received (highest first)
    /// Returns empty array if subscriptions data is unavailable
    var sortedSubscriptions: [SubscriptionDiagnostics] {
        guard let subs = subscriptions else { return [] }
        return subs.sorted { $0.eventsReceived > $1.eventsReceived }
    }
}

// MARK: - Logging

extension Logger {
    /// Logger for diagnostics-related operations
    static let diagnostics = Logger(subsystem: "com.tenex.app", category: "Diagnostics")
}
