import SwiftUI

// MARK: - Reports Tab View

/// Main tab view for Reports - shows reports from all projects with search and filtering.
/// Uses HSplitView on macOS and NavigationSplitView on iPad for master-detail layout,
/// NavigationStack on iPhone for stack-based navigation.
enum ReportsLayoutMode {
    case adaptive
    case shellList
    case shellDetail
}

struct ReportsTabView: View {
    @Environment(TenexCoreManager.self) var coreManager
    let layoutMode: ReportsLayoutMode
    private let selectedReportBindingOverride: Binding<Report?>?
    @StateObject private var viewModel = ReportsViewModel()
    @State private var selectedReportState: Report?
    @State private var hasConfiguredViewModel = false

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    init(
        layoutMode: ReportsLayoutMode = .adaptive,
        selectedReport: Binding<Report?>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedReportBindingOverride = selectedReport
    }

    private var selectedReportBinding: Binding<Report?> {
        selectedReportBindingOverride ?? $selectedReportState
    }

    /// Determine if we should use split view layout (iPad/Mac)
    private var useSplitView: Bool {
        if layoutMode == .shellList || layoutMode == .shellDetail {
            return true
        }
        #if os(macOS)
        return true
        #else
        return horizontalSizeClass == .regular
        #endif
    }

    var body: some View {
        Group {
            switch layoutMode {
            case .shellList:
                shellListLayout
            case .shellDetail:
                shellDetailLayout
            case .adaptive:
                if useSplitView {
                    splitViewLayout
                } else {
                    stackLayout
                }
            }
        }
        .task(id: layoutMode == .shellDetail) {
            guard layoutMode != .shellDetail else { return }
            if !hasConfiguredViewModel {
                viewModel.configure(with: coreManager)
                hasConfiguredViewModel = true
            }
            if viewModel.filteredReports.isEmpty {
                await viewModel.loadReports()
            }
        }
        .onChange(of: viewModel.filteredReports) { _, _ in
            guard let selectedReport = selectedReportBinding.wrappedValue else { return }
            let selectedIdentity = reportIdentity(selectedReport)
            if !viewModel.filteredReports.contains(where: { reportIdentity($0) == selectedIdentity }) {
                selectedReportBinding.wrappedValue = nil
            }
        }
        .onChange(of: coreManager.reports) { _, newReports in
            viewModel.handleReportsChanged(newReports)
        }
    }

    // MARK: - Split View Layout (iPad/Mac)

    private var splitViewLayout: some View {
        #if os(macOS)
        HSplitView {
            reportsListView
                .navigationTitle("Reports")
                .frame(minWidth: 340, idealWidth: 440, maxWidth: 520, maxHeight: .infinity)

            reportDetailContent
                .frame(minWidth: 600, maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #else
        NavigationSplitView {
            reportsListView
                .navigationTitle("Reports")
        } detail: {
            reportDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #endif
    }

    @ViewBuilder
    private var reportDetailContent: some View {
        if let report = selectedReportBinding.wrappedValue {
            ReportsTabDetailView(report: report, project: projectFor(report: report))
        } else {
            ContentUnavailableView(
                "Select a Report",
                systemImage: "doc.richtext",
                description: Text("Choose a report from the list to view its contents")
            )
        }
    }

    // MARK: - Stack Layout (iPhone)

    private var stackLayout: some View {
        NavigationStack {
            reportsListView
                .navigationTitle("Reports")
                .navigationDestination(for: Report.self) { report in
                    ReportsTabDetailView(report: report, project: projectFor(report: report))
                }
        }
    }

    private var shellListLayout: some View {
        reportsListView
            .navigationTitle("Reports")
            .accessibilityIdentifier("section_list_column")
    }

    private var shellDetailLayout: some View {
        reportDetailContent
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .accessibilityIdentifier("detail_column")
    }

    // MARK: - Reports List View

    private var reportsListView: some View {
        Group {
            if viewModel.filteredReports.isEmpty {
                emptyStateView
            } else {
                List(viewModel.filteredReports, id: \.self, selection: useSplitView ? selectedReportBinding : nil) { report in
                    if useSplitView {
                        ReportsTabRowView(
                            report: report,
                            projectTitle: projectFor(report: report)?.title,
                            showsChevron: false
                        )
                        .tag(report)
                    } else {
                        NavigationLink(value: report) {
                            ReportsTabRowView(
                                report: report,
                                projectTitle: projectFor(report: report)?.title,
                                showsChevron: true
                            )
                        }
                    }
                }
                #if os(iOS)
                .listStyle(.plain)
                #else
                .listStyle(.inset)
                #endif
                .refreshable {
                    await viewModel.refresh()
                }
            }
        }
        .searchable(text: $viewModel.searchText, prompt: "Search reports...")
        .toolbar {
            ToolbarItem(placement: .automatic) {
                AppGlobalFilterToolbarButton()
            }
            ToolbarItem(placement: .automatic) {
                if viewModel.isLoading {
                    ProgressView()
                        .scaleEffect(0.8)
                }
            }
        }
    }

    private func projectFor(report: Report) -> Project? {
        if let configured = viewModel.projectFor(report: report) {
            return configured
        }
        let projectId = TenexCoreManager.projectId(fromATag: report.projectATag)
        return coreManager.projects.first { $0.id == projectId }
    }

    private func reportIdentity(_ report: Report) -> String {
        "\(report.projectATag)::\(report.slug)"
    }

    // MARK: - Empty State

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: emptyStateIcon)
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text(emptyStateTitle)
                .font(.title2)
                .fontWeight(.semibold)

            Text(emptyStateMessage)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            if viewModel.isLoading {
                ProgressView()
                    .padding(.top, 8)
            }

            if !coreManager.isAppFilterDefault || !viewModel.searchText.isEmpty {
                Button {
                    viewModel.searchText = ""
                    coreManager.resetAppFilterToDefaults()
                } label: {
                    Label("Clear Filters", systemImage: "xmark.circle")
                }
                .adaptiveGlassButtonStyle()
                .padding(.top, 8)
            }
        }
        .padding()
    }

    private var emptyStateIcon: String {
        if !viewModel.searchText.isEmpty {
            return "magnifyingglass"
        } else if !coreManager.isAppFilterDefault {
            return "line.3.horizontal.decrease.circle"
        } else {
            return "doc.richtext"
        }
    }

    private var emptyStateTitle: String {
        if !viewModel.searchText.isEmpty {
            return "No Matching Reports"
        } else if !coreManager.isAppFilterDefault {
            return "No Reports in Current Filter"
        } else {
            return "No Reports"
        }
    }

    private var emptyStateMessage: String {
        if !viewModel.searchText.isEmpty {
            return "Try adjusting your search terms"
        } else if !coreManager.isAppFilterDefault {
            return "Try adjusting your project/time filter"
        } else {
            return "Reports from your projects will appear here"
        }
    }
}

// MARK: - Reports Tab Row View

/// Row view for displaying a report in the list
struct ReportsTabRowView: View {
    let report: Report
    let projectTitle: String?
    var showsChevron: Bool = true

    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                // Title
                Text(report.title)
                    .font(.headline)
                    .lineLimit(1)

                // Summary
                if !report.summary.isEmpty {
                    Text(report.summary)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }

                // Metadata row
                HStack(spacing: 8) {
                    // Author
                    ReportAuthorName(pubkey: report.author)
                        .font(.caption)
                        .foregroundStyle(.tertiary)

                    Text("•")
                        .foregroundStyle(.tertiary)

                    // Date
                    Text(TimestampTextFormatter.string(from: report.createdAt, style: .mediumDate))
                        .font(.caption)
                        .foregroundStyle(.tertiary)

                    // Project badge
                    if let projectTitle = projectTitle {
                        Text(projectTitle)
                            .font(.caption2)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(Color.projectBrandBackground)
                            .foregroundStyle(Color.projectBrand)
                            .clipShape(Capsule())
                    }
                }

            }

            Spacer()

            if showsChevron {
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 8)
    }

}

// MARK: - Reports Tab Detail View

/// Detail view for displaying a single report's content
struct ReportsTabDetailView: View {
    let report: Report
    let project: Project?

    @Environment(TenexCoreManager.self) private var coreManager
    @State private var showChatWithAuthor = false

    /// The report's a-tag for reference (format: 30023:pubkey:slug)
    private var reportATag: String {
        "30023:\(report.author):\(report.slug)"
    }

    /// Whether the "Chat with Author" button should be enabled
    private var canChatWithAuthor: Bool {
        project != nil && !report.author.isEmpty
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                // Header
                headerSection

                Divider()

                // Markdown content
                MarkdownView(content: report.content)
            }
            .padding()
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .navigationTitle(report.title)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .automatic) {
                Button {
                    showChatWithAuthor = true
                } label: {
                    Label("Chat with Author", systemImage: "bubble.left.fill")
                }
                .disabled(!canChatWithAuthor)
            }
        }
        .sheet(isPresented: $showChatWithAuthor) {
            if let project = project {
                // TODO(#modal-composer-deprecation): migrate this modal composer entry point to inline flow.
                MessageComposerView(
                    project: project,
                    initialAgentPubkey: report.author,
                    initialContent: ConversationFormatters.generateReportContextMessage(report: report),
                    referenceReportATag: reportATag
                )
                .environment(coreManager)
                .tenexModalPresentation(detents: [.large])
            }
        }
    }

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Tags
            if !report.hashtags.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(report.hashtags, id: \.self) { tag in
                            Text("#\(tag)")
                                .font(.caption)
                                .padding(.horizontal, 10)
                                .padding(.vertical, 4)
                                .background(Color.skillBrandBackground)
                                .foregroundStyle(Color.skillBrand)
                                .clipShape(Capsule())
                        }
                    }
                }
            }

            // Title
            Text(report.title)
                .font(.largeTitle)
                .fontWeight(.bold)

            // Summary
            if !report.summary.isEmpty {
                Text(report.summary)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            // Metadata
            HStack(spacing: 12) {
                HStack(spacing: 4) {
                    Image(systemName: "person.circle")
                    ReportAuthorName(pubkey: report.author)
                }
                .font(.subheadline)
                .foregroundStyle(.secondary)

                Text("•")
                    .foregroundStyle(.secondary)

                HStack(spacing: 4) {
                    Image(systemName: "clock")
                    Text(TimestampTextFormatter.string(from: report.createdAt, style: .longDateShortTime))
                }
                .font(.subheadline)
                .foregroundStyle(.secondary)

                if let project = project {
                    Text("•")
                        .foregroundStyle(.secondary)

                    HStack(spacing: 4) {
                        Image(systemName: "folder")
                        Text(project.title)
                    }
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                }
            }
        }
    }

}

// MARK: - Report Author Name Helper

private struct ReportAuthorName: View {
    let pubkey: String
    @Environment(TenexCoreManager.self) private var coreManager

    var body: some View {
        Text(coreManager.displayName(for: pubkey))
    }
}

#Preview {
    ReportsTabView()
        .environment(TenexCoreManager())
}
