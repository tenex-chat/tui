import SwiftUI
import Combine

/// ViewModel for managing reports data across all projects.
/// Observes reactive reports updates from TenexCoreManager for real-time updates.
@MainActor
final class ReportsViewModel: ObservableObject {
    // MARK: - Published Properties

    /// All reports from all projects, sorted by created date (newest first)
    @Published var reports: [Report] = []

    /// Whether reports are currently being loaded
    @Published private(set) var isLoading = false

    /// Search text for filtering reports
    @Published var searchText = ""

    // MARK: - Private Properties

    private weak var coreManager: TenexCoreManager?
    private var cancellables = Set<AnyCancellable>()

    // MARK: - Computed Properties

    /// Filtered reports based on global project/time filters and search text.
    var filteredReports: [Report] {
        var result = reports

        // Filter by global app filter first
        if let coreManager {
            let now = UInt64(Date().timeIntervalSince1970)
            result = result.filter { report in
                let projectId = TenexCoreManager.projectId(fromATag: report.projectATag)
                return coreManager.matchesAppFilter(
                    projectId: projectId,
                    timestamp: report.createdAt,
                    now: now
                )
            }
        }

        // Filter by search text
        if !searchText.isEmpty {
            let lowercasedSearch = searchText.lowercased()
            result = result.filter { report in
                report.title.lowercased().contains(lowercasedSearch) ||
                report.summary.lowercased().contains(lowercasedSearch) ||
                report.hashtags.contains { $0.lowercased().contains(lowercasedSearch) }
            }
        }

        return result
    }

    // MARK: - Initialization

    init(coreManager: TenexCoreManager? = nil) {
        self.coreManager = coreManager
    }

    /// Configure the ViewModel with a core manager reference and set up reactive bindings
    func configure(with coreManager: TenexCoreManager) {
        self.coreManager = coreManager

        // Observe reactive reports updates from coreManager
        coreManager.$reports
            .receive(on: DispatchQueue.main)
            .sink { [weak self] newReports in
                self?.reports = newReports
            }
            .store(in: &cancellables)
    }

    // MARK: - Public Methods

    /// Load reports from all projects (initial load)
    func loadReports() async {
        guard let coreManager = coreManager else { return }

        isLoading = true
        defer { isLoading = false }

        var allReports: [Report] = []

        // Fetch reports from each project via FFI for initial load
        let projects = coreManager.projects
        for project in projects {
            let projectReports = await coreManager.safeCore.getReports(projectId: project.id)
            allReports.append(contentsOf: projectReports)
        }

        // Sort by created date (newest first)
        allReports.sort { $0.createdAt > $1.createdAt }

        // Update both local state and coreManager's reactive property
        self.reports = allReports
        coreManager.reports = allReports
    }

    /// Refresh reports (sync first, then reload)
    func refresh() async {
        guard let coreManager = coreManager else { return }
        await coreManager.syncNow()
        await loadReports()
    }

    /// Get the project associated with a report
    func projectFor(report: Report) -> Project? {
        let projectId = TenexCoreManager.projectId(fromATag: report.projectATag)
        return coreManager?.projects.first { $0.id == projectId }
    }
}
