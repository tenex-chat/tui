import SwiftUI

struct ReportsView: View {
    let project: ProjectInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var reports: [ReportInfo] = []
    @State private var isLoading = false
    @State private var selectedReport: ReportInfo?
    @State private var dataChangedObserver: NSObjectProtocol?

    var body: some View {
        Group {
            if reports.isEmpty {
                VStack(spacing: 16) {
                    Image(systemName: "doc.text")
                        .font(.system(size: 60))
                        .foregroundStyle(.secondary)
                    Text("No Reports")
                        .font(.title2)
                        .fontWeight(.semibold)
                    if isLoading {
                        ProgressView()
                            .padding(.top, 8)
                    }
                }
            } else {
                List {
                    ForEach(reports, id: \.id) { report in
                        ReportRowView(report: report)
                            .contentShape(Rectangle())
                            .onTapGesture {
                                selectedReport = report
                            }
                    }
                }
                .listStyle(.plain)
            }
        }
        .navigationTitle("Reports")
        .navigationBarTitleDisplayMode(.large)
        .toolbar {
            ToolbarItem(placement: .topBarTrailing) {
                Button(action: { Task { await loadReports() } }) {
                    Image(systemName: "arrow.clockwise")
                }
                .disabled(isLoading)
            }
        }
        .task {
            await loadReports()
            subscribeToDataChanges()
        }
        .onDisappear {
            if let observer = dataChangedObserver {
                NotificationCenter.default.removeObserver(observer)
                dataChangedObserver = nil
            }
        }
        .sheet(item: $selectedReport) { report in
            ReportDetailView(report: report)
        }
    }

    private func subscribeToDataChanges() {
        // Subscribe to general data change notifications for reactive report updates
        dataChangedObserver = NotificationCenter.default.addObserver(
            forName: .tenexDataChanged,
            object: nil,
            queue: .main
        ) { [project] _ in
            Task {
                await loadReports()
            }
        }
    }

    private func loadReports() async {
        isLoading = true
        // Refresh ensures AppDataStore is synced with latest data from nostrdb
        _ = await coreManager.safeCore.refresh()
        reports = await coreManager.safeCore.getReports(projectId: project.id)
        isLoading = false
    }
}

// MARK: - Report Row View

struct ReportRowView: View {
    let report: ReportInfo

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .none
        return formatter
    }()

    var body: some View {
        HStack(spacing: 12) {
            // Report icon
            RoundedRectangle(cornerRadius: 10)
                .fill(Color.orange.gradient)
                .frame(width: 44, height: 44)
                .overlay {
                    Image(systemName: "doc.richtext")
                        .foregroundStyle(.white)
                        .font(.title3)
                }

            VStack(alignment: .leading, spacing: 4) {
                Text(report.title)
                    .font(.headline)
                    .lineLimit(1)

                if let summary = report.summary {
                    Text(summary)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }

                HStack(spacing: 8) {
                    Text(report.author)
                        .font(.caption)
                        .foregroundStyle(.tertiary)

                    Text("•")
                        .foregroundStyle(.tertiary)

                    Text(formatDate(report.updatedAt))
                        .font(.caption)
                        .foregroundStyle(.tertiary)

                    // Tags
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

// MARK: - Report Detail View (Markdown)

struct ReportDetailView: View {
    let report: ReportInfo
    @Environment(\.dismiss) private var dismiss

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .long
        formatter.timeStyle = .short
        return formatter
    }()

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 16) {
                    // Header
                    VStack(alignment: .leading, spacing: 8) {
                        // Tags
                        if !report.tags.isEmpty {
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

                        // Title
                        Text(report.title)
                            .font(.largeTitle)
                            .fontWeight(.bold)

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
                        }
                    }
                    .padding(.bottom, 8)

                    Divider()

                    // Markdown content
                    MarkdownView(content: report.content)
                }
                .padding()
            }
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }

    private func formatDate(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return Self.dateFormatter.string(from: date)
    }
}

// MARK: - ReportInfo Identifiable

extension ReportInfo: Identifiable {}
