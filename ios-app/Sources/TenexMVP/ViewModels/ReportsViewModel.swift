import SwiftUI
import Combine

/// ViewModel for managing reports data across all projects.
/// Observes reactive reports updates from TenexCoreManager for real-time updates.
@MainActor
final class ReportsViewModel: ObservableObject {
    // MARK: - Published Properties

    /// All reports from all projects, sorted by updated date (newest first)
    @Published private(set) var reports: [ReportInfo] = []

    /// Whether reports are currently being loaded
    @Published private(set) var isLoading = false

    /// Search text for filtering reports
    @Published var searchText = ""

    /// Selected project IDs for filtering (empty means all projects)
    @Published var selectedProjectIds: Set<String> = []

    // MARK: - Private Properties

    private weak var coreManager: TenexCoreManager?
    private var cancellables = Set<AnyCancellable>()

    // MARK: - Computed Properties

    /// Filtered reports based on search text and selected projects
    var filteredReports: [ReportInfo] {
        var result = reports

        // Filter by selected projects
        if !selectedProjectIds.isEmpty {
            result = result.filter { report in
                selectedProjectIds.contains(report.projectId)
            }
        }

        // Filter by search text
        if !searchText.isEmpty {
            let lowercasedSearch = searchText.lowercased()
            result = result.filter { report in
                report.title.lowercased().contains(lowercasedSearch) ||
                (report.summary?.lowercased().contains(lowercasedSearch) ?? false) ||
                report.author.lowercased().contains(lowercasedSearch) ||
                report.tags.contains { $0.lowercased().contains(lowercasedSearch) }
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

        var allReports: [ReportInfo] = []

        // Fetch reports from each project via FFI for initial load
        let projects = coreManager.projects
        for project in projects {
            let projectReports = await coreManager.safeCore.getReports(projectId: project.id)
            allReports.append(contentsOf: projectReports)
        }

        // Sort by updated date (newest first)
        allReports.sort { $0.updatedAt > $1.updatedAt }

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
    func projectFor(report: ReportInfo) -> ProjectInfo? {
        coreManager?.projects.first { $0.id == report.projectId }
    }
}
