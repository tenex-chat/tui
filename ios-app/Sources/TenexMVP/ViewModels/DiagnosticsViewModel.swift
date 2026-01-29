import Foundation

/// ViewModel for the Diagnostics tab
/// Manages diagnostic data fetching from Rust core
@MainActor
class DiagnosticsViewModel: ObservableObject {
    // MARK: - Published Properties

    /// Current diagnostics snapshot from Rust core
    @Published var snapshot: DiagnosticsSnapshot?

    /// Loading state
    @Published var isLoading = false

    /// Error state for overall fetch failures
    @Published var error: Error?

    /// Per-section error messages from Rust (for partial failures)
    @Published var sectionErrors: [String] = []

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

    // MARK: - Task Management (Fix race conditions)

    /// Currently running fetch task (for cancellation and in-flight tracking)
    private var currentFetchTask: Task<Void, Never>?

    // MARK: - Initialization

    init(coreManager: TenexCoreManager) {
        self.coreManager = coreManager
    }

    // MARK: - Public Methods

    /// Load diagnostics data from Rust core
    /// - Parameter includeDatabaseStats: Whether to include expensive DB stats (default: based on tab)
    func loadDiagnostics(includeDatabaseStats: Bool? = nil) async {
        // Cancel any in-flight task to prevent race conditions
        currentFetchTask?.cancel()

        // Don't start a new fetch if one is already running (prevents concurrent refresh calls)
        guard !isLoading else { return }

        isLoading = true
        error = nil
        sectionErrors = []

        // Determine whether to include database stats
        // Default: only include if Database tab is active (lazy loading optimization)
        let includeDB = includeDatabaseStats ?? (selectedTab == .database)

        // Create a new cancellable task
        let task = Task { [weak self] in
            guard let self = self else { return }

            // Capture core before detaching to avoid actor isolation violation
            let core = self.coreManager.core

            do {
                // Move FFI calls off main actor to prevent UI blocking
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

                // Check for cancellation before updating UI
                try Task.checkCancellation()

                // Update UI on main actor
                await MainActor.run {
                    self.snapshot = fetchedSnapshot
                    self.sectionErrors = fetchedSnapshot.sectionErrors
                    self.isLoading = false
                }
            } catch is CancellationError {
                // Task was cancelled, don't update state
                await MainActor.run {
                    self.isLoading = false
                }
            } catch {
                await MainActor.run {
                    self.error = error
                    self.isLoading = false
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
