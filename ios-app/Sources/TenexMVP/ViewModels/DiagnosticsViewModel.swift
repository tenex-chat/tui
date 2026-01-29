import Foundation

/// ViewModel for the Diagnostics tab
/// Manages diagnostic data fetching from Rust core
///
/// ## Concurrency Strategy (Latest-Wins + Deduplication)
/// - If an in-flight request already covers the new request (includeDB=true covers includeDB=false), ignore new call
/// - Otherwise, cancel existing task and start fresh
/// - Error state derives from snapshot.sectionErrors (single source of truth)
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
    @Published var selectedTab: DiagnosticsTab = .overview {
        didSet {
            // When switching to Database tab, reload with DB stats if not already loaded
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

    // MARK: - Initialization

    init(coreManager: TenexCoreManager) {
        self.coreManager = coreManager
    }

    // MARK: - Public Methods

    /// Load diagnostics data from Rust core
    ///
    /// Implements latest-wins + deduplication concurrency strategy:
    /// - If in-flight request already covers new request (includeDB=true covers false), ignore new call
    /// - Otherwise, cancel existing task and start fresh
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
        // but prevents stale results from being applied
        currentFetchTask?.cancel()

        isLoading = true
        currentFetchIncludesDB = includeDB

        // Capture core before creating task to avoid actor isolation issues
        let core = self.coreManager.core

        // Create a new cancellable task
        let task = Task { [weak self] in
            // Use defer to ensure state cleanup on any exit path (prevents state races)
            defer {
                Task { @MainActor [weak self] in
                    self?.currentFetchTask = nil
                    self?.currentFetchIncludesDB = false
                    // Note: isLoading is set to false in the result handlers below
                }
            }

            guard let self = self else { return }

            do {
                // Move FFI calls off main actor to prevent UI blocking
                // Note: Task.detached is required for FFI - synchronous calls can't be preempted
                let fetchedSnapshot = try await Task.detached { [core, includeDB] in
                    // Check for cancellation before starting
                    try Task.checkCancellation()

                    // Refresh core data first and handle errors
                    let refreshSuccess = core.refresh()
                    if !refreshSuccess {
                        // Log but continue - refresh failure shouldn't block diagnostics
                        print("[DiagnosticsViewModel] Core refresh returned false, continuing with diagnostics fetch")
                    }

                    // Check for cancellation after refresh
                    try Task.checkCancellation()

                    // Fetch diagnostics snapshot (single batched call, now infallible)
                    return core.getDiagnosticsSnapshot(includeDatabaseStats: includeDB)
                }.value

                // CANCELLATION: Guard UI updates - don't apply results if cancelled
                // (The underlying Rust work completed, but we shouldn't show stale data)
                guard !Task.isCancelled else {
                    await MainActor.run { [weak self] in
                        self?.isLoading = false
                    }
                    return
                }

                // Update UI on main actor
                // Note: sectionErrors derive from snapshot (single source of truth)
                await MainActor.run { [weak self] in
                    self?.snapshot = fetchedSnapshot
                    self?.error = nil  // Clear any previous error since we have data
                    self?.isLoading = false
                }
            } catch is CancellationError {
                // Task was cancelled, reset loading state but don't touch data
                // Keep last known snapshot (with its errors) until new snapshot succeeds
                await MainActor.run { [weak self] in
                    self?.isLoading = false
                }
            } catch {
                // Only set error if we have no snapshot at all
                // If we have existing data, keep showing it with its errors
                await MainActor.run { [weak self] in
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
    func cancelFetch() {
        currentFetchTask?.cancel()
        currentFetchTask = nil
        currentFetchIncludesDB = false
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
