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
    private var subscriptions = Set<AnyCancellable>()
    private var refreshTask: Task<Void, Never>?
    private var currentFetchID: UUID?

    // MARK: - Initialization

    init(coreManager: TenexCoreManager) {
        self.coreManager = coreManager
        bindToUpdates()
    }

    // MARK: - Public Methods

    /// Load stats data from Rust core using SafeTenexCore
    /// Shows cached data immediately using the local store
    func loadStats() async {
        await reloadSnapshot()
    }

    /// Refresh stats data (for pull-to-refresh)
    func refresh() async {
        await coreManager.syncNow()
        await reloadSnapshot()
    }

    private func bindToUpdates() {
        coreManager.$statsVersion
            .removeDuplicates()
            .debounce(for: .milliseconds(250), scheduler: RunLoop.main)
            .sink { [weak self] _ in
                guard let self = self else { return }
                Task { await self.reloadSnapshot() }
            }
            .store(in: &subscriptions)
    }

    private func reloadSnapshot() async {
        refreshTask?.cancel()
        let fetchID = UUID()
        currentFetchID = fetchID
        await MainActor.run {
            isLoading = true
        }

        let task = Task { [weak self] in
            defer {
                Task { @MainActor [weak self] in
                    guard self?.currentFetchID == fetchID else { return }
                    self?.isLoading = false
                }
            }

            guard let self = self else { return }

            do {
                try Task.checkCancellation()
                let freshSnapshot = try await self.coreManager.safeCore.getStatsSnapshot()
                await MainActor.run { [weak self] in
                    guard self?.currentFetchID == fetchID else { return }
                    self?.snapshot = freshSnapshot
                    self?.error = nil
                }
            } catch is CancellationError {
                // Task canceled; keep current snapshot
            } catch {
                await MainActor.run { [weak self] in
                    guard self?.currentFetchID == fetchID else { return }
                    if self?.snapshot == nil {
                        self?.error = error
                    }
                }
            }
        }
        refreshTask = task
        await task.value
    }

    deinit {
        refreshTask?.cancel()
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

    /// Cached date formatter for day labels (performance optimization)
    private static let dayLabelFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateFormat = "MMM d"
        formatter.timeZone = TimeZone(identifier: "UTC")
        return formatter
    }()

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
            // Format as "MMM d" using cached formatter
            let date = Date(timeIntervalSince1970: TimeInterval(dayStart))
            return dayLabelFormatter.string(from: date)
        }
    }

    /// Get current day start timestamp (UTC)
    static var todayStart: UInt64 {
        let now = UInt64(Date().timeIntervalSince1970)
        let secondsPerDay: UInt64 = 86400
        return (now / secondsPerDay) * secondsPerDay
    }
}
