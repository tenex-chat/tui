import SwiftUI

// MARK: - Reports Tab View

/// Main tab view for Reports - shows reports from all projects with search and filtering.
/// Uses NavigationSplitView on iPad/Mac for master-detail layout,
/// NavigationStack on iPhone for stack-based navigation.
struct ReportsTabView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @StateObject private var viewModel = ReportsViewModel()
    @State private var selectedReport: ReportInfo?
    @State private var showProjectFilter = false

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    /// Determine if we should use split view layout (iPad/Mac)
    private var useSplitView: Bool {
        #if os(macOS)
        return true
        #else
        return horizontalSizeClass == .regular
        #endif
    }

    var body: some View {
        Group {
            if useSplitView {
                splitViewLayout
            } else {
                stackLayout
            }
        }
        .task {
            viewModel.configure(with: coreManager)
            await viewModel.loadReports()
        }
        // Reports now update reactively via TenexEventHandler -> coreManager.reports -> viewModel
        // No need to observe coreManager.projects for report updates
    }

    // MARK: - Split View Layout (iPad/Mac)

    private var splitViewLayout: some View {
        NavigationSplitView {
            reportsListView
                .navigationTitle("Reports")
        } detail: {
            if let report = selectedReport {
                ReportsTabDetailView(report: report, project: viewModel.projectFor(report: report))
            } else {
                ContentUnavailableView(
                    "Select a Report",
                    systemImage: "doc.richtext",
                    description: Text("Choose a report from the list to view its contents")
                )
            }
        }
    }

    // MARK: - Stack Layout (iPhone)

    private var stackLayout: some View {
        NavigationStack {
            reportsListView
                .navigationTitle("Reports")
                .navigationDestination(for: ReportInfo.self) { report in
                    ReportsTabDetailView(report: report, project: viewModel.projectFor(report: report))
                }
        }
    }

    // MARK: - Reports List View

    private var reportsListView: some View {
        Group {
            if viewModel.filteredReports.isEmpty {
                emptyStateView
            } else {
                List(viewModel.filteredReports, id: \.self, selection: useSplitView ? $selectedReport : nil) { report in
                    if useSplitView {
                        ReportsTabRowView(
                            report: report,
                            projectTitle: viewModel.projectFor(report: report)?.title
                        )
                        .tag(report)
                    } else {
                        NavigationLink(value: report) {
                            ReportsTabRowView(
                                report: report,
                                projectTitle: viewModel.projectFor(report: report)?.title
                            )
                        }
                    }
                }
                .listStyle(.plain)
                .refreshable {
                    await viewModel.refresh()
                }
            }
        }
        .searchable(text: $viewModel.searchText, prompt: "Search reports...")
        .toolbar {
            ToolbarItem(placement: .topBarLeading) {
                filterButton
            }
            ToolbarItem(placement: .topBarTrailing) {
                if viewModel.isLoading {
                    ProgressView()
                        .scaleEffect(0.8)
                }
            }
        }
        .sheet(isPresented: $showProjectFilter) {
            ReportsProjectFilterSheet(
                projects: coreManager.projects,
                projectOnlineStatus: coreManager.projectOnlineStatus,
                selectedProjectIds: $viewModel.selectedProjectIds
            )
        }
    }

    // MARK: - Filter Button

    private var filterButton: some View {
        Button {
            showProjectFilter = true
        } label: {
            Label(filterButtonLabel, systemImage: filterButtonIcon)
        }
    }

    private var filterButtonLabel: String {
        if viewModel.selectedProjectIds.isEmpty {
            return "All Projects"
        } else if viewModel.selectedProjectIds.count == 1 {
            return coreManager.projects.first { $0.id == viewModel.selectedProjectIds.first }?.title ?? "1 Project"
        } else {
            return "\(viewModel.selectedProjectIds.count) Projects"
        }
    }

    private var filterButtonIcon: String {
        viewModel.selectedProjectIds.isEmpty
            ? "line.3.horizontal.decrease.circle"
            : "line.3.horizontal.decrease.circle.fill"
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

            if !viewModel.selectedProjectIds.isEmpty || !viewModel.searchText.isEmpty {
                Button {
                    viewModel.selectedProjectIds.removeAll()
                    viewModel.searchText = ""
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
        } else if !viewModel.selectedProjectIds.isEmpty {
            return "line.3.horizontal.decrease.circle"
        } else {
            return "doc.richtext"
        }
    }

    private var emptyStateTitle: String {
        if !viewModel.searchText.isEmpty {
            return "No Matching Reports"
        } else if !viewModel.selectedProjectIds.isEmpty {
            return "No Reports in Selected Projects"
        } else {
            return "No Reports"
        }
    }

    private var emptyStateMessage: String {
        if !viewModel.searchText.isEmpty {
            return "Try adjusting your search terms"
        } else if !viewModel.selectedProjectIds.isEmpty {
            return "Try selecting different projects"
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
                            .background(Color.blue.opacity(0.15))
                            .foregroundStyle(.blue)
                            .clipShape(Capsule())
                    }

                    // Tags (show first 2)
                    if !report.tags.isEmpty {
                        HStack(spacing: 4) {
                            ForEach(report.tags.prefix(2), id: \.self) { tag in
                                Text("#\(tag)")
                                    .font(.caption2)
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(Color.orange.opacity(0.15))
                                    .foregroundStyle(.orange)
                                    .clipShape(Capsule())
                            }
                        }
                    }
                }
            }

            Spacer()

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
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
        }
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
                                .background(Color.orange.opacity(0.15))
                                .foregroundStyle(.orange)
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

// MARK: - Reports Project Filter Sheet

/// Sheet for filtering reports by project
private struct ReportsProjectFilterSheet: View {
    let projects: [ProjectInfo]
    let projectOnlineStatus: [String: Bool]
    @Binding var selectedProjectIds: Set<String>
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                // "All Projects" option
                Button {
                    selectedProjectIds.removeAll()
                } label: {
                    HStack {
                        Image(systemName: "square.grid.2x2")
                            .foregroundStyle(.blue)
                            .frame(width: 24)
                        Text("All Projects")
                            .foregroundStyle(.primary)
                        Spacer()
                        if selectedProjectIds.isEmpty {
                            Image(systemName: "checkmark")
                                .foregroundStyle(.blue)
                        }
                    }
                }

                Divider()

                // Individual projects
                ForEach(projects, id: \.id) { project in
                    Button {
                        toggleProject(project.id)
                    } label: {
                        HStack {
                            RoundedRectangle(cornerRadius: 6)
                                .fill(deterministicColor(for: project.id).gradient)
                                .frame(width: 24, height: 24)
                                .overlay {
                                    Image(systemName: "folder.fill")
                                        .foregroundStyle(.white)
                                        .font(.caption)
                                }

                            Text(project.title)
                                .foregroundStyle(.primary)
                                .lineLimit(1)

                            if projectOnlineStatus[project.id] == true {
                                Circle()
                                    .fill(.green)
                                    .frame(width: 8, height: 8)
                            }

                            Spacer()

                            if selectedProjectIds.contains(project.id) {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(.blue)
                            }
                        }
                    }
                }
            }
            #if os(iOS)
            .listStyle(.insetGrouped)
            #else
            .listStyle(.inset)
            #endif
            .navigationTitle("Filter by Project")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                    .fontWeight(.semibold)
                }
            }
        }
        .presentationDetents([.medium, .large])
    }

    private func toggleProject(_ id: String) {
        if selectedProjectIds.contains(id) {
            selectedProjectIds.remove(id)
        } else {
            selectedProjectIds.insert(id)
        }
    }
}

#Preview {
    ReportsTabView()
        .environmentObject(TenexCoreManager())
}
