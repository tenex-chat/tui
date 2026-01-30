import Foundation
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

    /// Loading state
    @Published var isLoading = false

    /// Error state for overall fetch failures (only set when we have NO snapshot)
    @Published var error: Error?

    /// Selected diagnostics subtab
    ///
    /// **Side Effect:** When switching to the `.database` tab, this property automatically
    /// triggers a lazy load of database statistics if they haven't been loaded yet.
    /// This optimization avoids loading expensive DB stats until the user explicitly
    /// navigates to the Database tab, improving initial load performance.
    ///
    /// The side effect is intentional for UX - database stats require a full LMDB scan
    /// which can be slow on large databases, so we defer this work until needed.
    @Published var selectedTab: DiagnosticsTab = .overview {
        didSet {
            // Lazy load database stats when user navigates to Database tab
            if selectedTab == .database && snapshot?.database == nil {
                Task {
                    await loadDiagnostics(includeDatabaseStats: true)
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

    // MARK: - Initialization

    init(coreManager: TenexCoreManager) {
        self.coreManager = coreManager
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
                    // Note: isLoading is set to false in the result handlers below
                }
            }

            guard let self = self else { return }

            do {
                // Check for cancellation before starting
                try Task.checkCancellation()

                // Refresh core data first using SafeTenexCore
                let refreshSuccess = await safeCore.refresh()
                if !refreshSuccess {
                    // Log but continue - refresh failure shouldn't block diagnostics
                    Logger.diagnostics.warning("Core refresh returned false, continuing with diagnostics fetch")
                }

                // Check for cancellation after refresh
                try Task.checkCancellation()

                // Fetch diagnostics snapshot using SafeTenexCore
                let fetchedSnapshot = await safeCore.getDiagnosticsSnapshot(includeDatabaseStats: includeDB)

                // IDENTITY CHECK: Guard UI updates - only apply if this is still the current fetch
                // (The underlying Rust work completed, but we shouldn't show stale data)
                await MainActor.run { [weak self] in
                    guard self?.currentFetchID == fetchID else { return }
                    self?.snapshot = fetchedSnapshot
                    self?.error = nil  // Clear any previous error since we have data
                    self?.isLoading = false
                }
            } catch is CancellationError {
                // Task was cancelled, reset loading state but don't touch data
                // Keep last known snapshot (with its errors) until new snapshot succeeds
                // CRITICAL: Only update state if this task is still the current one
                await MainActor.run { [weak self] in
                    guard self?.currentFetchID == fetchID else { return }
                    self?.isLoading = false
                }
            } catch {
                // Only set error if we have no snapshot at all
                // If we have existing data, keep showing it with its errors
                // CRITICAL: Only update state if this task is still the current one
                await MainActor.run { [weak self] in
                    guard self?.currentFetchID == fetchID else { return }
                    if self?.snapshot == nil {
                        self?.error = error
                    }
                    self?.isLoading = false
                }
            }
        }

        currentFetchTask = task

        // Wait for the task to complete
        await task.value
    }

    /// Refresh diagnostics data (for pull-to-refresh)
    /// Always includes database stats since user is explicitly refreshing
    func refresh() async {
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
}

// MARK: - Diagnostics Tab Enum

enum DiagnosticsTab: String, CaseIterable, Identifiable {
    case overview = "Overview"
    case sync = "Sync"
    case subscriptions = "Subscriptions"
    case database = "Database"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .overview: return "gauge.with.needle"
        case .sync: return "arrow.triangle.2.circlepath"
        case .subscriptions: return "antenna.radiowaves.left.and.right"
        case .database: return "cylinder"
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
}

// MARK: - Helper Extensions

extension DiagnosticsSnapshot {
    /// Format uptime in milliseconds to human-readable string
    static func formatUptime(_ ms: UInt64) -> String {
        let seconds = ms / 1000

        if seconds < 60 {
            return "\(seconds)s"
        } else if seconds < 3600 {
            let mins = seconds / 60
            let secs = seconds % 60
            return secs > 0 ? "\(mins)m \(secs)s" : "\(mins)m"
        } else {
            let hours = seconds / 3600
            let mins = (seconds % 3600) / 60
            return mins > 0 ? "\(hours)h \(mins)m" : "\(hours)h"
        }
    }

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

    /// Format seconds since last sync
    static func formatTimeSince(_ seconds: UInt64?) -> String {
        guard let seconds = seconds else {
            return "Never"
        }

        if seconds < 60 {
            return "\(seconds)s ago"
        } else if seconds < 3600 {
            let mins = seconds / 60
            return "\(mins)m ago"
        } else {
            let hours = seconds / 3600
            return "\(hours)h ago"
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
