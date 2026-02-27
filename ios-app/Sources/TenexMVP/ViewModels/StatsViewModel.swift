import Foundation

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
    @Published var selectedTab: StatsTab = .rankings

    // MARK: - Dependencies

    private weak var coreManager: TenexCoreManager?
    private var refreshTask: Task<Void, Never>?
    private var currentFetchID: UUID?

    // MARK: - Configuration

    func configure(with coreManager: TenexCoreManager) {
        guard self.coreManager == nil else { return }
        self.coreManager = coreManager
    }

    // MARK: - Public Methods

    /// Load stats data from Rust core using SafeTenexCore
    /// Shows cached data immediately using the local store
    func loadStats() async {
        await reloadSnapshot()
    }

    /// Refresh stats data (for pull-to-refresh)
    func refresh() async {
        guard let coreManager else { return }
        await coreManager.syncNow()
        await reloadSnapshot()
    }

    private func reloadSnapshot() async {
        guard let coreManager else { return }
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
                let freshSnapshot = try await coreManager.safeCore.getStatsSnapshot()
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
    case rankings = "Rankings"
    case runtime = "Runtime"
    case messages = "Messages"
    case activity = "Activity"

    var id: String { rawValue }

    var icon: String {
        switch self {
        case .rankings: return "list.number"
        case .runtime: return "clock.badge.checkmark"
        case .messages: return "bubble.left.and.bubble.right.fill"
        case .activity: return "square.grid.3x3.fill"
        }
    }
}
