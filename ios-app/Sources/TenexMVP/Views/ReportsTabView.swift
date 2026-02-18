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
    @EnvironmentObject var coreManager: TenexCoreManager
    let layoutMode: ReportsLayoutMode
    private let selectedReportBindingOverride: Binding<ReportInfo?>?
    @StateObject private var viewModel = ReportsViewModel()
    @State private var selectedReportState: ReportInfo?
    @State private var hasConfiguredViewModel = false

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    init(
        layoutMode: ReportsLayoutMode = .adaptive,
        selectedReport: Binding<ReportInfo?>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedReportBindingOverride = selectedReport
    }

    private var selectedReportBinding: Binding<ReportInfo?> {
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
        .onChange(of: viewModel.filteredReports.map(reportIdentity)) { _, visibleReportIds in
            guard let selectedReport = selectedReportBinding.wrappedValue else { return }
            let selectedIdentity = reportIdentity(selectedReport)
            if !visibleReportIds.contains(selectedIdentity) {
                selectedReportBinding.wrappedValue = nil
            }
        }
        // Reports now update reactively via TenexEventHandler -> coreManager.reports -> viewModel
        // No need to observe coreManager.projects for report updates
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
                .navigationDestination(for: ReportInfo.self) { report in
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
            ToolbarItem(placement: .topBarLeading) {
                AppGlobalFilterToolbarButton()
            }
            ToolbarItem(placement: .topBarTrailing) {
                if viewModel.isLoading {
                    ProgressView()
                        .scaleEffect(0.8)
                }
            }
        }
    }

    private func projectFor(report: ReportInfo) -> ProjectInfo? {
        if let configured = viewModel.projectFor(report: report) {
            return configured
        }
        return coreManager.projects.first { $0.id == report.projectId }
    }

    private func reportIdentity(_ report: ReportInfo) -> String {
        "\(report.projectId)::\(report.id)"
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
                .buttonStyle(.bordered)
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
    let report: ReportInfo
    let projectTitle: String?
    var showsChevron: Bool = true

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .none
        return formatter
    }()

    var body: some View {
        HStack(spacing: 12) {
            VStack(alignment: .leading, spacing: 4) {
                // Title
                Text(report.title)
                    .font(.headline)
                    .lineLimit(1)

                // Summary
                if let summary = report.summary {
                    Text(summary)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }

                // Metadata row
                HStack(spacing: 8) {
                    // Author
                    Text(report.author)
                        .font(.caption)
                        .foregroundStyle(.tertiary)

                    Text("•")
                        .foregroundStyle(.tertiary)

                    // Date
                    Text(formatDate(report.updatedAt))
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

    private func formatDate(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return Self.dateFormatter.string(from: date)
    }
}

// MARK: - Reports Tab Detail View

/// Detail view for displaying a single report's content
struct ReportsTabDetailView: View {
    let report: ReportInfo
    let project: ProjectInfo?

    @EnvironmentObject private var coreManager: TenexCoreManager
    @State private var showChatWithAuthor = false

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .long
        formatter.timeStyle = .short
        return formatter
    }()

    /// Convert the report author's npub to hex pubkey for MessageComposerView
    private var authorHexPubkey: String? {
        Bech32.npubToHex(report.authorNpub)
    }

    /// Generate the report's a-tag for reference (format: 30023:pubkey:slug)
    /// Returns nil if the author's npub cannot be converted to hex (invalid npub)
    private var reportATag: String? {
        guard let authorHex = authorHexPubkey else {
            // Invalid npub - cannot generate valid a-tag
            return nil
        }
        return "30023:\(authorHex):\(report.id)"
    }

    /// Whether the "Chat with Author" button should be enabled
    /// Requires both a valid project and a valid author hex pubkey
    private var canChatWithAuthor: Bool {
        project != nil && authorHexPubkey != nil
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
        .navigationBarTitleDisplayMode(.inline)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button {
                    showChatWithAuthor = true
                } label: {
                    Label("Chat with Author", systemImage: "bubble.left.fill")
                }
                .disabled(!canChatWithAuthor)
            }
        }
        .sheet(isPresented: $showChatWithAuthor) {
            if let project = project, let authorPubkey = authorHexPubkey {
                MessageComposerView(
                    project: project,
                    initialAgentPubkey: authorPubkey,
                    initialContent: ConversationFormatters.generateReportContextMessage(report: report),
                    referenceReportATag: reportATag
                )
                .environmentObject(coreManager)
                .tenexModalPresentation(detents: [.large])
            }
        }
    }

    private var headerSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            // Tags
            if !report.tags.isEmpty {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 8) {
                        ForEach(report.tags, id: \.self) { tag in
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
            if let summary = report.summary {
                Text(summary)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }

            // Metadata
            HStack(spacing: 12) {
                HStack(spacing: 4) {
                    Image(systemName: "person.circle")
                    Text(report.author)
                }
                .font(.subheadline)
                .foregroundStyle(.secondary)

                Text("•")
                    .foregroundStyle(.secondary)

                HStack(spacing: 4) {
                    Image(systemName: "clock")
                    Text(formatDate(report.updatedAt))
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

    private func formatDate(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return Self.dateFormatter.string(from: date)
    }
}

#Preview {
    ReportsTabView()
        .environmentObject(TenexCoreManager())
}
