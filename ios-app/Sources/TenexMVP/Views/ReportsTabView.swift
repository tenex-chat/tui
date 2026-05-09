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

enum ReportListEntry: Identifiable {
    case html(HtmlReportVersionEntry)
    case markdown(ReportEntry)

    var id: String {
        switch self {
        case .html(let entry): return "html:\(entry.id)"
        case .markdown(let entry): return "markdown:\(entry.id)"
        }
    }

    var createdAt: UInt64 {
        switch self {
        case .html(let entry): return entry.latest.createdAt
        case .markdown(let entry): return entry.mostRecentCreatedAt
        }
    }
}

struct HtmlReportVersionEntry: Identifiable {
    let latest: HtmlReport
    let versions: [HtmlReport]

    var id: String {
        let slug = latest.slug.trimmingCharacters(in: .whitespacesAndNewlines)
        if slug.isEmpty {
            return latest.eventId
        }
        return "\(latest.projectATag):\(slug)"
    }

    static func grouped(from reports: [HtmlReport]) -> [HtmlReportVersionEntry] {
        var groups: [String: [HtmlReport]] = [:]
        var entries: [HtmlReportVersionEntry] = []

        for report in reports {
            let slug = report.slug.trimmingCharacters(in: .whitespacesAndNewlines)
            if slug.isEmpty {
                entries.append(HtmlReportVersionEntry(latest: report, versions: [report]))
            } else {
                let key = "\(report.projectATag)|\(slug)"
                groups[key, default: []].append(report)
            }
        }

        for versions in groups.values {
            let sorted = sortedVersions(versions)
            guard let latest = sorted.first else { continue }
            entries.append(HtmlReportVersionEntry(latest: latest, versions: sorted))
        }

        return entries.sorted { lhs, rhs in
            if lhs.latest.createdAt != rhs.latest.createdAt {
                return lhs.latest.createdAt > rhs.latest.createdAt
            }
            return lhs.id < rhs.id
        }
    }

    private static func sortedVersions(_ reports: [HtmlReport]) -> [HtmlReport] {
        reports.sorted {
            if $0.createdAt != $1.createdAt {
                return $0.createdAt > $1.createdAt
            }
            return $0.eventId < $1.eventId
        }
    }
}

// MARK: - ReportsTabView

struct ReportsTabView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @State private var selectedReport: Report?
    @State private var selectedHtmlReportEntry: HtmlReportVersionEntry?
    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    private var useSplitView: Bool { horizontalSizeClass == .regular }
    #else
    private var useSplitView: Bool { true }
    #endif

    var body: some View {
        if useSplitView {
            NavigationSplitView {
                NavigationStack { listContent }
            } detail: {
                if let htmlReportEntry = selectedHtmlReportEntry {
                    HtmlReportDetailView(report: htmlReportEntry.latest, versions: htmlReportEntry.versions)
                        .environment(coreManager)
                        .id(htmlReportEntry.id)
                } else if let report = selectedReport {
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
                    .sheet(isPresented: Binding(get: { selectedHtmlReportEntry != nil }, set: { if !$0 { selectedHtmlReportEntry = nil } })) {
                        if let htmlReportEntry = selectedHtmlReportEntry {
                            NavigationStack {
                                HtmlReportDetailView(report: htmlReportEntry.latest, versions: htmlReportEntry.versions)
                                    .environment(coreManager)
                                    .toolbar {
                                        ToolbarItem(placement: .confirmationAction) {
                                            Button("Done") { selectedHtmlReportEntry = nil }
                                        }
                                    }
                            }
                        }
                    }
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
        let reports = scopedReports

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

    private var htmlReportEntries: [HtmlReportVersionEntry] {
        HtmlReportVersionEntry.grouped(from: scopedHtmlReports)
    }

    private var scopedReports: [Report] {
        coreManager.reports.filter { report in
            let projectId = TenexCoreManager.projectId(fromATag: report.projectATag)
            return coreManager.includesProjectInCurrentScope(projectId)
        }
    }

    private var scopedHtmlReports: [HtmlReport] {
        coreManager.htmlReports.filter { report in
            let projectId = TenexCoreManager.projectId(fromATag: report.projectATag)
            return coreManager.includesProjectInCurrentScope(projectId)
        }
    }

    private var chronologicalEntries: [ReportListEntry] {
        let entries = htmlReportEntries.map(ReportListEntry.html)
            + reportEntries.map(ReportListEntry.markdown)

        return entries.sorted {
            if $0.createdAt != $1.createdAt {
                return $0.createdAt > $1.createdAt
            }
            return $0.id < $1.id
        }
    }

    private var listContent: some View {
        Group {
            if scopedReports.isEmpty && scopedHtmlReports.isEmpty {
                emptyStateView
            } else {
                List {
                    ForEach(chronologicalEntries) { entry in
                        switch entry {
                        case .html(let htmlReportEntry):
                            Button {
                                selectedReport = nil
                                selectedHtmlReportEntry = htmlReportEntry
                            } label: {
                                HtmlReportRowView(
                                    report: htmlReportEntry.latest,
                                    project: project(for: htmlReportEntry.latest.projectATag),
                                    versionCount: htmlReportEntry.versions.count
                                )
                            }
                            .buttonStyle(.plain)
                            .accessibilityIdentifier("html_report_row_\(htmlReportEntry.latest.eventId)")

                        case .markdown(let markdownEntry):
                            switch markdownEntry {
                            case .single(let report):
                                Button {
                                    selectedHtmlReportEntry = nil
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
                }
                .listStyle(.plain)
                .refreshable {
                    await coreManager.manualRefresh()
                }
            }
        }
        .navigationTitle("Reports")
        .toolbar {
            ToolbarItem(placement: .automatic) {
                WorkspaceScopeButton(style: .toolbar)
            }
            ToolbarItem(placement: .automatic) {
                AppGlobalFilterToolbarButton()
            }
        }
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
            Text(emptyStateMessage)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)
                .padding(.horizontal, 32)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity)
        .accessibilityIdentifier("reports_empty_state")
    }

    private var emptyStateMessage: String {
        if !coreManager.isAppFilterDefault {
            return "No reports match the current workspace or filter."
        }
        return "Reports will appear here when they are published."
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

// MARK: - HtmlReportRowView

struct HtmlReportRowView: View {
    let report: HtmlReport
    let project: Project?
    let versionCount: Int

    private var subtitle: String {
        let trimmed = report.description.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty, trimmed != report.title else { return "" }
        return trimmed
    }

    var body: some View {
        HStack(alignment: .top, spacing: 10) {
            Image(systemName: report.isZip ? "archivebox" : "doc.richtext.fill")
                .foregroundStyle(.tint)
                .font(.title3)
                .frame(width: 28)
            VStack(alignment: .leading, spacing: 4) {
                Text(report.title.isEmpty ? "Untitled" : report.title)
                    .font(.body)
                    .fontWeight(.medium)
                    .lineLimit(2)
                if !subtitle.isEmpty {
                    Text(subtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                HStack(spacing: 8) {
                    if let project {
                        ProjectBadge(projectTitle: project.title, projectId: project.id)
                    }
                    if report.isZip {
                        Text("Bundle")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    } else {
                        Text("HTML")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }
                    if versionCount > 1 {
                        Text("\(versionCount) versions")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }
                    Text(Date(timeIntervalSince1970: TimeInterval(report.createdAt)), style: .relative)
                        .font(.caption2)
                        .foregroundStyle(.tertiary)
                }
            }
        }
        .padding(.vertical, 2)
    }
}

// MARK: - ReportRowView

struct ReportRowView: View {
    let report: Report
    let project: Project?

    private var readingTimeText: String {
        report.readingTimeMins == 1 ? "1 min read" : "\(report.readingTimeMins) min read"
    }

    private var displaySummary: String {
        ReportDisplayContent.summary(for: report)
    }

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(report.title.isEmpty ? "Untitled" : report.title)
                .font(.body)
                .fontWeight(.medium)
                .lineLimit(2)
            if !displaySummary.isEmpty {
                Text(displaySummary)
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

    private var project: Project? {
        let projectId = TenexCoreManager.projectId(fromATag: report.projectATag)
        return coreManager.projects.first(where: { $0.id == projectId })
    }

    private var displaySummary: String {
        ReportDisplayContent.summary(for: report)
    }

    private var displayContent: String {
        ReportDisplayContent.body(for: report)
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 16) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(report.title.isEmpty ? "Untitled" : report.title)
                        .font(.title)
                        .fontWeight(.bold)
                    if !displaySummary.isEmpty {
                        Text(displaySummary)
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

                if !displayContent.isEmpty {
                    MarkdownView(content: displayContent)
                        .accessibilityIdentifier("report_detail_content")
                } else {
                    Text("No content available.")
                        .foregroundStyle(.secondary)
                }
            }
            .padding()
        }
        .navigationTitle(report.title.isEmpty ? "Report" : report.title)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .primaryAction) {
                openChatButton
            }
        }
        .sheet(isPresented: $showingMessageComposer) {
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

    private var openChatButton: some View {
        Button {
            showingMessageComposer = true
        } label: {
            Label("Open Chat", systemImage: "bubble.left.and.bubble.right")
        }
        .adaptiveProminentGlassButtonStyle()
        .accessibilityIdentifier("report_open_chat_button")
    }
}

private enum ReportDisplayContent {
    static func body(for report: Report) -> String {
        let body = bodyWithoutFrontmatter(from: report.content)
        return bodyWithoutDuplicateTitleHeading(body, title: report.title)
    }

    static func summary(for report: Report) -> String {
        let rawSummary = report.summary.trimmingCharacters(in: .whitespacesAndNewlines)
        let fallback = bodySummary(for: report)

        if rawSummary.isEmpty || looksLikeFrontmatterSummary(rawSummary) {
            return fallback
        }

        if normalized(rawSummary) == normalized(report.title) {
            return fallback
        }

        return rawSummary
    }

    private static func bodySummary(for report: Report) -> String {
        if let frontmatterSummary = frontmatterScalar(in: report.content, key: "summary")
            ?? frontmatterScalar(in: report.content, key: "scope") {
            return frontmatterSummary
        }

        let body = body(for: report)
        let flattened = body
            .components(separatedBy: .newlines)
            .map { strippedMarkdownSummaryLine($0) }
            .filter { !$0.isEmpty }
            .joined(separator: " ")
            .split(whereSeparator: { $0.isWhitespace })
            .joined(separator: " ")

        guard !flattened.isEmpty else { return "" }
        return String(flattened.prefix(180))
    }

    private static func bodyWithoutFrontmatter(from content: String) -> String {
        let normalizedContent = content
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")

        guard normalizedContent.hasPrefix("---\n") else { return content }
        let rest = String(normalizedContent.dropFirst(4))
        var consumedCharacterCount = 0

        for line in rest.components(separatedBy: "\n") {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            let lineCharacterCount = line.count + 1

            if trimmed == "---" || trimmed == "..." {
                let bodyStart = rest.index(rest.startIndex, offsetBy: min(consumedCharacterCount + lineCharacterCount, rest.count))
                return String(rest[bodyStart...]).trimmingCharacters(in: .newlines)
            }

            consumedCharacterCount += lineCharacterCount
        }

        return content
    }

    private static func bodyWithoutDuplicateTitleHeading(_ content: String, title: String) -> String {
        var lines = content.components(separatedBy: "\n")

        while let first = lines.first, first.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            lines.removeFirst()
        }

        if let first = lines.first, heading(first, matchesTitle: title) {
            lines.removeFirst()
            while let first = lines.first, first.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                lines.removeFirst()
            }
        }

        return lines.joined(separator: "\n")
    }

    private static func heading(_ line: String, matchesTitle title: String) -> Bool {
        let trimmed = line.trimmingCharacters(in: .whitespaces)
        guard trimmed.hasPrefix("#") else { return false }

        let markerCount = trimmed.prefix { $0 == "#" }.count
        guard markerCount > 0, markerCount <= 6 else { return false }

        let headingText = trimmed
            .dropFirst(markerCount)
        let heading = String(headingText)
            .trimmingCharacters(in: .whitespaces)
            .trimmingCharacters(in: CharacterSet(charactersIn: "#").union(.whitespaces))
        return normalized(heading) == normalized(title)
    }

    private static func frontmatterScalar(in content: String, key: String) -> String? {
        let normalizedContent = content
            .replacingOccurrences(of: "\r\n", with: "\n")
            .replacingOccurrences(of: "\r", with: "\n")

        guard normalizedContent.hasPrefix("---\n") else { return nil }
        let rest = String(normalizedContent.dropFirst(4))

        for line in rest.components(separatedBy: "\n") {
            let trimmed = line.trimmingCharacters(in: .whitespaces)
            if trimmed == "---" || trimmed == "..." {
                return nil
            }

            let parts = trimmed.split(separator: ":", maxSplits: 1, omittingEmptySubsequences: false)
            guard parts.count == 2, String(parts[0]).trimmingCharacters(in: .whitespaces) == key else {
                continue
            }

            let value = unquotedScalar(String(parts[1]).trimmingCharacters(in: .whitespaces))
            return value.isEmpty ? nil : value
        }

        return nil
    }

    private static func unquotedScalar(_ value: String) -> String {
        guard value.count >= 2,
              let first = value.first,
              let last = value.last,
              (first == "\"" && last == "\"") || (first == "'" && last == "'")
        else {
            return value.trimmingCharacters(in: .whitespacesAndNewlines)
        }

        return String(value.dropFirst().dropLast()).trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func strippedMarkdownSummaryLine(_ line: String) -> String {
        var text = line.trimmingCharacters(in: .whitespacesAndNewlines)

        while text.hasPrefix(">") {
            text = String(text.dropFirst()).trimmingCharacters(in: .whitespaces)
        }

        if text.hasPrefix("#") {
            let markerCount = text.prefix { $0 == "#" }.count
            text = String(text.dropFirst(markerCount)).trimmingCharacters(in: .whitespaces)
        }

        if text.hasPrefix("- [x] ") || text.hasPrefix("- [X] ") || text.hasPrefix("- [ ] ") {
            text = String(text.dropFirst(6))
        } else if text.hasPrefix("- ") || text.hasPrefix("* ") {
            text = String(text.dropFirst(2))
        }

        return text
            .replacingOccurrences(of: "**", with: "")
            .replacingOccurrences(of: "__", with: "")
            .replacingOccurrences(of: "`", with: "")
            .trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private static func looksLikeFrontmatterSummary(_ summary: String) -> Bool {
        let trimmed = summary.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.hasPrefix("---") ||
            (trimmed.contains("title:") && (trimmed.contains("date:") || trimmed.contains("scope:")))
    }

    private static func normalized(_ value: String) -> String {
        value
            .trimmingCharacters(in: .whitespacesAndNewlines)
            .lowercased()
    }
}
