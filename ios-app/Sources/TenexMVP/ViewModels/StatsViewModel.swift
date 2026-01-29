import Foundation
import Combine

/// ViewModel for Stats tab with full TUI parity
/// Manages stats data fetching, chart state, and tab selection
@MainActor
class StatsViewModel: ObservableObject {
    // MARK: - Published Properties

    /// Current stats snapshot from Rust core
    @Published var snapshot: StatsSnapshot?

    /// Loading state
    @Published var isLoading = false

    /// Error state
    @Published var error: Error?

    /// Selected stats subtab
    @Published var selectedTab: StatsTab = .chart

    // MARK: - Dependencies

    private let coreManager: TenexCoreManager

    // MARK: - Initialization

    init(coreManager: TenexCoreManager) {
        self.coreManager = coreManager
    }

    // MARK: - Public Methods

    /// Load stats data from Rust core
    func loadStats() async {
        isLoading = true
        error = nil

        do {
            // Move FFI calls off main actor to prevent UI blocking
            let fetchedSnapshot = await Task.detached {
                // Refresh core data first (pull latest from relays)
                _ = self.coreManager.core.refresh()

                // Fetch stats snapshot (single batched call)
                return try self.coreManager.core.getStatsSnapshot()
            }.value

            // Update UI on main actor
            snapshot = fetchedSnapshot
            isLoading = false
        } catch {
            self.error = error
            isLoading = false
        }
    }

    /// Refresh stats data (for pull-to-refresh)
    func refresh() async {
        await loadStats()
    }
}

// MARK: - Stats Tab Enum

enum StatsTab: String, CaseIterable, Identifiable {
    case chart = "Chart"
    case rankings = "Rankings"
    case messages = "Messages"
    case activity = "Activity"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .chart: return "chart.bar.fill"
        case .rankings: return "list.number"
        case .messages: return "bubble.left.and.bubble.right.fill"
        case .activity: return "square.grid.3x3.fill"
        }
    }
}

// MARK: - Helper Extensions

extension StatsSnapshot {
    /// Format runtime in milliseconds to human-readable string
    static func formatRuntime(_ ms: UInt64) -> String {
        let seconds = ms / 1000

        if seconds == 0 && ms > 0 {
            return "\(ms)ms"
        } else if seconds == 0 {
            return "0s"
        } else if seconds < 60 {
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

    /// Format a day_start timestamp as a date label ("Today", "Yest.", "Jan 27")
    static func formatDayLabel(_ dayStart: UInt64, todayStart: UInt64) -> String {
        let secondsPerDay: UInt64 = 86400
        let daysDiff = (todayStart - dayStart) / secondsPerDay

        switch daysDiff {
        case 0:
            return "Today"
        case 1:
            return "Yest."
        default:
            // Format as "Mon DD"
            let date = Date(timeIntervalSince1970: TimeInterval(dayStart))
            let formatter = DateFormatter()
            formatter.dateFormat = "MMM d"
            formatter.timeZone = TimeZone(identifier: "UTC")
            return formatter.string(from: date)
        }
    }

    /// Get current day start timestamp (UTC)
    static var todayStart: UInt64 {
        let now = UInt64(Date().timeIntervalSince1970)
        let secondsPerDay: UInt64 = 86400
        return (now / secondsPerDay) * secondsPerDay
    }
}
