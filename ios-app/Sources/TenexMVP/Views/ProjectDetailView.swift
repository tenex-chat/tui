import SwiftUI

// MARK: - Feed Item

private enum ProjectFeedItem: Identifiable {
    case conversation(ConversationFullInfo)
    case htmlReport(HtmlReportVersionEntry)
    case markdownReport(Report)

    var id: String {
        switch self {
        case .conversation(let c): return "conv:\(c.thread.id)"
        case .htmlReport(let e): return "html:\(e.id)"
        case .markdownReport(let r): return "md:\(r.id)"
        }
    }

    var timestamp: UInt64 {
        switch self {
        case .conversation(let c): return c.thread.effectiveLastActivity
        case .htmlReport(let e): return e.latest.createdAt
        case .markdownReport(let r): return r.createdAt
        }
    }
}

// MARK: - ProjectDetailView

struct ProjectDetailView: View {
    @Environment(TenexCoreManager.self) private var coreManager

    let projectId: String
    @Binding var selectedProjectId: String?

    private var project: Project? {
        coreManager.projects.first { $0.id == projectId }
    }

    private var projectConversations: [ConversationFullInfo] {
        coreManager.conversations.filter { conv in
            TenexCoreManager.projectId(fromATag: conv.projectATag) == projectId && !conv.isArchived
        }
    }

    private var projectReports: [Report] {
        coreManager.reports.filter { report in
            TenexCoreManager.projectId(fromATag: report.projectATag) == projectId
        }
    }

    private var projectHtmlReports: [HtmlReport] {
        coreManager.htmlReports.filter { report in
            TenexCoreManager.projectId(fromATag: report.projectATag) == projectId
        }
    }

    private var feedItems: [ProjectFeedItem] {
        var items: [ProjectFeedItem] = []

        for conv in projectConversations {
            items.append(.conversation(conv))
        }
        for entry in HtmlReportVersionEntry.grouped(from: projectHtmlReports) {
            items.append(.htmlReport(entry))
        }
        for report in projectReports {
            items.append(.markdownReport(report))
        }

        return items.sorted { $0.timestamp > $1.timestamp }
    }

    var body: some View {
        Group {
            if feedItems.isEmpty {
                emptyStateView
            } else {
                List {
                    ForEach(feedItems) { item in
                        itemRow(for: item)
                    }
                }
                #if os(iOS)
                .listStyle(.plain)
                #else
                .listStyle(.inset)
                #endif
                .refreshable {
                    await coreManager.manualRefresh()
                }
            }
        }
        .navigationTitle(project?.title ?? "Project")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                NavigationLink {
                    ProjectSettingsView(
                        projectId: projectId,
                        selectedProjectId: $selectedProjectId
                    )
                } label: {
                    Image(systemName: "gearshape")
                }
            }
        }
    }

    @ViewBuilder
    private func itemRow(for item: ProjectFeedItem) -> some View {
        switch item {
        case .conversation(let conversation):
            NavigationLink {
                ConversationAdaptiveDetailView(conversation: conversation)
                    .environment(coreManager)
                    #if os(iOS)
                    .toolbar(.hidden, for: .tabBar)
                    #endif
            } label: {
                ConversationFeedRow(conversation: conversation)
            }

        case .htmlReport(let entry):
            NavigationLink {
                HtmlReportDetailView(report: entry.latest, versions: entry.versions)
                    .environment(coreManager)
            } label: {
                HtmlReportRowView(report: entry.latest, project: nil, versionCount: entry.versions.count)
            }

        case .markdownReport(let report):
            NavigationLink {
                ReportDetailView(report: report)
                    .environment(coreManager)
            } label: {
                ReportRowView(report: report, project: nil)
            }
        }
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: "tray")
                .font(.system(size: 48))
                .foregroundStyle(.secondary)
            Text("Nothing Here Yet")
                .font(.title2)
                .fontWeight(.semibold)
            Text("Chats and reports for this project will appear here.")
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
    }
}

// MARK: - Conversation Feed Row

private struct ConversationFeedRow: View {
    let conversation: ConversationFullInfo

    private var statusColor: Color {
        Color.conversationStatus(for: conversation.thread.statusLabel, isActive: conversation.isActive)
    }

    var body: some View {
        HStack(spacing: 12) {
            ZStack {
                Circle()
                    .fill(statusColor)
                    .frame(width: 10, height: 10)
                if conversation.isActive {
                    Circle()
                        .stroke(statusColor.opacity(0.5), lineWidth: 2)
                        .frame(width: 16, height: 16)
                }
            }

            VStack(alignment: .leading, spacing: 4) {
                HStack(alignment: .top) {
                    Text(conversation.thread.title)
                        .font(.headline)
                        .lineLimit(2)
                    Spacer()
                    RelativeTimeText(
                        timestamp: conversation.thread.effectiveLastActivity,
                        style: .localizedAbbreviated
                    )
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                }

                if let summary = conversation.thread.summary, !summary.isEmpty {
                    Text(summary)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                } else if let status = conversation.thread.statusCurrentActivity, !status.isEmpty {
                    Text(status)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                HStack(spacing: 6) {
                    Image(systemName: "bubble.left")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                    Text("\(conversation.messageCount)")
                        .font(.caption2)
                        .foregroundStyle(.tertiary)

                    if let label = conversation.thread.statusLabel, !label.isEmpty {
                        Text("·")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                        Text(label)
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }
                }
            }
        }
        .padding(.vertical, 4)
    }
}
