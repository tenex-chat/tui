import SwiftUI

/// ViewModel for managing reports data across all projects.
/// Handles fetching, filtering, and caching reports with reactive updates.
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

    /// Cache mapping report ID to project info for display
    private var reportProjectMap: [String: ProjectInfo] = [:]

    // MARK: - Computed Properties

    /// Filtered reports based on search text and selected projects
    var filteredReports: [ReportInfo] {
        var result = reports

        // Filter by selected projects
        if !selectedProjectIds.isEmpty {
            result = result.filter { report in
                if let project = projectFor(report: report) {
                    return selectedProjectIds.contains(project.id)
                }
                return false
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

    /// Configure the ViewModel with a core manager reference
    func configure(with coreManager: TenexCoreManager) {
        self.coreManager = coreManager
    }

    // MARK: - Public Methods

    /// Load reports from all projects
    func loadReports() async {
        guard let coreManager = coreManager else { return }

        isLoading = true
        defer { isLoading = false }

        var allReports: [ReportInfo] = []
        var projectMap: [String: ProjectInfo] = [:]

        // Fetch reports from each project
        let projects = coreManager.projects
        for project in projects {
            let projectReports = await coreManager.safeCore.getReports(projectId: project.id)
            for report in projectReports {
                allReports.append(report)
                projectMap[report.id] = project
            }
        }

        // Sort by updated date (newest first)
        allReports.sort { $0.updatedAt > $1.updatedAt }

        self.reports = allReports
        self.reportProjectMap = projectMap
    }

    /// Refresh reports (sync first, then reload)
    func refresh() async {
        guard let coreManager = coreManager else { return }
        await coreManager.syncNow()
        await loadReports()
    }

    /// Get the project associated with a report
    func projectFor(report: ReportInfo) -> ProjectInfo? {
        reportProjectMap[report.id]
    }
}
