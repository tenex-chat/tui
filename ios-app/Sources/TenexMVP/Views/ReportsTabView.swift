import SwiftUI

// MARK: - ReportsTabView

struct ReportsTabView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @State private var selectedReport: Report?

    var body: some View {
        NavigationSplitView {
            reportsListView
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
    }

    private var groupedReports: [(String, [Report])] {
        let reports = coreManager.reports
        var grouped: [String: [Report]] = [:]

        for report in reports {
            let planTag = report.hashtags.first(where: { !$0.isEmpty }) ?? "Other"
            grouped[planTag, default: []].append(report)
        }

        var result = grouped.map { ($0.key, $0.value) }
        result.sort { lhs, rhs in
            if lhs.0 == "Other" { return false }
            if rhs.0 == "Other" { return true }
            return lhs.0 < rhs.0
        }
        return result
    }

    @ViewBuilder
    private var reportsListView: some View {
        Group {
            if coreManager.reports.isEmpty {
                emptyStateView
            } else {
                List(selection: $selectedReport) {
                    ForEach(groupedReports, id: \.0) { sectionTitle, reports in
                        Section(sectionTitle) {
                            ForEach(reports, id: \.id) { report in
                                ReportRowView(report: report)
                                    .tag(report)
                                    .accessibilityIdentifier("report_row_\(report.id)")
                            }
                        }
                    }
                }
                .listStyle(.plain)
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

// MARK: - ReportRowView

struct ReportRowView: View {
    let report: Report

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
            Text(readingTimeText)
                .font(.caption2)
                .foregroundStyle(.tertiary)
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
                // Header
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

                // Content
                if !report.content.isEmpty {
                    MarkdownView(content: report.content)
                        .accessibilityIdentifier("report_detail_content")
                } else {
                    Text("No content available.")
                        .foregroundStyle(.secondary)
                }

                Divider()

                // Open Chat button — opens as a sheet, consistent with "New conversation"
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
            MessageComposerView(
                project: project,
                conversationId: nil,
                conversationTitle: nil,
                initialAgentPubkey: nil,
                initialContent: nil,
                initialTextAttachments: [],
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
