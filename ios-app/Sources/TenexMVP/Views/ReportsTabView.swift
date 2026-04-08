import SwiftUI

// MARK: - ReportEntry

enum ReportEntry: Identifiable {
    case single(Report)
    case group(projectATag: String, docTag: String, reports: [Report])

    var id: String {
        switch self {
        case .single(let r): return r.id
        case .group(let projectATag, let docTag, _): return "group:\(projectATag):\(docTag)"
        }
    }

    var mostRecentCreatedAt: UInt64 {
        switch self {
        case .single(let r): return r.createdAt
        case .group(_, _, let reports): return reports.map(\.createdAt).max() ?? 0
        }
    }
}

// MARK: - ReportsTabView

struct ReportsTabView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @State private var selectedReport: Report?
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    private var useSplitView: Bool { horizontalSizeClass == .regular }
    #else
    private var useSplitView: Bool { true }
    #endif

    var body: some View {
        if useSplitView {
            NavigationSplitView {
                NavigationStack {
                    listContent
                }
            } detail: {
                if let report = selectedReport {
                    ReportDetailView(report: report)
                        .environment(coreManager)
                } else {
                    Text("Select a report to read")
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                }
            }
            .accessibilityIdentifier("reports_tab")
        } else {
            NavigationStack {
                listContent
                    .sheet(isPresented: Binding(get: { selectedReport != nil }, set: { if !$0 { selectedReport = nil } })) {
                        if let report = selectedReport {
                            NavigationStack {
                                ReportDetailView(report: report)
                                    .environment(coreManager)
                                    .toolbar {
                                        ToolbarItem(placement: .confirmationAction) {
                                            Button("Done") { selectedReport = nil }
                                        }
                                    }
                            }
                            .tenexModalPresentation(detents: [.large])
                        }
                    }
            }
            .accessibilityIdentifier("reports_tab")
        }
    }

    private var reportEntries: [ReportEntry] {
        let reports = coreManager.reports

        var groupCounts: [String: Int] = [:]
        for report in reports where !report.document.isEmpty {
            let key = "\(report.projectATag)|\(report.document)"
            groupCounts[key, default: 0] += 1
        }

        var groups: [String: (projectATag: String, docTag: String, reports: [Report])] = [:]
        var singles: [Report] = []

        for report in reports {
            let key = "\(report.projectATag)|\(report.document)"
            if !report.document.isEmpty, let count = groupCounts[key], count > 1 {
                if groups[key] == nil {
                    groups[key] = (report.projectATag, report.document, [])
                }
                groups[key]!.reports.append(report)
            } else {
                singles.append(report)
            }
        }

        var entries: [ReportEntry] = []
        for (_, group) in groups {
            entries.append(.group(projectATag: group.projectATag, docTag: group.docTag, reports: group.reports.sorted { $0.createdAt > $1.createdAt }))
        }
        for report in singles {
            entries.append(.single(report))
        }

        entries.sort { $0.mostRecentCreatedAt > $1.mostRecentCreatedAt }
        return entries
    }

    private func project(for aTag: String) -> Project? {
        let projectId = TenexCoreManager.projectId(fromATag: aTag)
        return coreManager.projects.first(where: { $0.id == projectId })
    }

    private var listContent: some View {
        Group {
            if coreManager.reports.isEmpty {
                emptyStateView
            } else {
                List {
                    ForEach(reportEntries) { entry in
                        switch entry {
                        case .single(let report):
                            Button {
                                selectedReport = report
                            } label: {
                                ReportRowView(report: report, project: project(for: report.projectATag))
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("report_row_\(report.id)")

                        case .group(let projectATag, let docTag, let reports):
                            NavigationLink {
                                ReportGroupView(
                                    docTag: docTag,
                                    reports: reports,
                                    selectedReport: $selectedReport,
                                    project: project(for: projectATag)
                                )
                            } label: {
                                ReportGroupRowView(docTag: docTag, count: reports.count, project: project(for: projectATag))
                            }
                            .accessibilityIdentifier("report_group_\(projectATag)_\(docTag)")
                        }
                    }
                }
                .listStyle(.plain)
                .refreshable {
                    await coreManager.manualRefresh()
                }
            }
        }
        .navigationTitle("Reports")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: "doc.richtext")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("No Reports")
                .font(.title2)
                .fontWeight(.semibold)
            Text("Reports will appear here when they are published.")
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityIdentifier("reports_empty_state")
    }
}

// MARK: - ReportGroupRowView

struct ReportGroupRowView: View {
    let docTag: String
    let count: Int
    let project: Project?

    var body: some View {
        HStack {
            Image(systemName: "folder")
                .foregroundStyle(.secondary)
            VStack(alignment: .leading, spacing: 2) {
                Text(docTag)
                    .font(.body)
                    .fontWeight(.medium)
                Text("\(count) documents")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            Spacer()
            if let project {
                ProjectBadge(projectTitle: project.title, projectId: project.id)
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: - ReportGroupView

struct ReportGroupView: View {
    let docTag: String
    let reports: [Report]
    @Binding var selectedReport: Report?
    let project: Project?

    var body: some View {
        List {
            ForEach(reports, id: \.id) { report in
                Button {
                    selectedReport = report
                } label: {
                    ReportRowView(report: report, project: project)
                }
                .buttonStyle(.plain)
                .accessibilityIdentifier("report_row_\(report.id)")
            }
        }
        .listStyle(.plain)
        .navigationTitle(docTag)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
    }
}

// MARK: - ReportRowView

struct ReportRowView: View {
    let report: Report
    let project: Project?

    private var readingTimeText: String {
        report.readingTimeMins == 1 ? "1 min read" : "\(report.readingTimeMins) min read"
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(report.title.isEmpty ? "Untitled" : report.title)
                .font(.body)
                .fontWeight(.medium)
                .lineLimit(2)
            if !report.summary.isEmpty {
                Text(report.summary)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .lineLimit(2)
            }
            HStack(spacing: 8) {
                if let project {
                    ProjectBadge(projectTitle: project.title, projectId: project.id)
                }
                Text(readingTimeText)
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
                Text(Date(timeIntervalSince1970: TimeInterval(report.createdAt)), style: .relative)
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: - ReportDetailView

struct ReportDetailView: View {
    let report: Report
    @Environment(TenexCoreManager.self) private var coreManager
    @State private var showingMessageComposer = false

    private var referenceATag: String {
        "30023:\(report.author):\(report.slug)"
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(report.title.isEmpty ? "Untitled" : report.title)
                        .font(.title)
                        .fontWeight(.bold)
                    if !report.summary.isEmpty {
                        Text(report.summary)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                    HStack {
                        Label(
                            report.readingTimeMins == 1 ? "1 min read" : "\(report.readingTimeMins) min read",
                            systemImage: "clock"
                        )
                        .font(.caption)
                        .foregroundStyle(.secondary)
                    }
                    .accessibilityIdentifier("report_detail_meta")
                }

                Divider()

                if !report.content.isEmpty {
                    MarkdownView(content: report.content)
                        .accessibilityIdentifier("report_detail_content")
                } else {
                    Text("No content available.")
                        .foregroundStyle(.secondary)
                }

                Divider()

                Button {
                    showingMessageComposer = true
                } label: {
                    Label("Open Chat", systemImage: "bubble.left.and.bubble.right")
                        .frame(maxWidth: .infinity)
                }
                .buttonStyle(.borderedProminent)
                .accessibilityIdentifier("report_open_chat_button")
            }
            .padding()
        }
        .navigationTitle(report.title.isEmpty ? "Report" : report.title)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .sheet(isPresented: $showingMessageComposer) {
            let projectId = TenexCoreManager.projectId(fromATag: report.projectATag)
            let project = coreManager.projects.first(where: { $0.id == projectId })
            let contextMessage = ConversationFormatters.generateContextMessage(report: report)
            MessageComposerView(
                project: project,
                conversationId: nil,
                conversationTitle: nil,
                initialAgentPubkey: nil,
                initialContent: "[Text Attachment 1]",
                initialTextAttachments: [TextAttachment(id: 1, content: contextMessage)],
                referenceConversationId: nil,
                referenceReportATag: referenceATag,
                displayStyle: .modal,
                inlineLayoutStyle: .standard
            )
            .environment(coreManager)
            .tenexModalPresentation(detents: [.large])
        }
    }
}
