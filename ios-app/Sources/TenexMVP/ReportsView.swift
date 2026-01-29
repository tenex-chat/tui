import SwiftUI

struct ReportsView: View {
    let project: ProjectInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var reports: [ReportInfo] = []
    @State private var isLoading = false
    @State private var selectedReport: ReportInfo?

    var body: some View {
        Group {
            if isLoading {
                ProgressView("Loading reports...")
            } else if reports.isEmpty {
                VStack(spacing: 16) {
                    Image(systemName: "doc.text")
                        .font(.system(size: 60))
                        .foregroundStyle(.secondary)
                    Text("No Reports")
                        .font(.title2)
                        .fontWeight(.semibold)
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
                Button(action: loadReports) {
                    Image(systemName: "arrow.clockwise")
                }
                .disabled(isLoading)
            }
        }
        .onAppear {
            loadReports()
        }
        .sheet(item: $selectedReport) { report in
            ReportDetailView(report: report)
        }
    }

    private func loadReports() {
        isLoading = true
        DispatchQueue.global(qos: .userInitiated).async {
            // Refresh ensures AppDataStore is synced with latest data from nostrdb
            _ = coreManager.core.refresh()
            let fetched = coreManager.core.getReports(projectId: project.id)
            DispatchQueue.main.async {
                self.reports = fetched
                self.isLoading = false
            }
        }
    }
}

// MARK: - Report Row View

struct ReportRowView: View {
    let report: ReportInfo

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
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .none
        return formatter.string(from: date)
    }
}

// MARK: - Report Detail View (Markdown)

struct ReportDetailView: View {
    let report: ReportInfo
    @Environment(\.dismiss) private var dismiss

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
        let formatter = DateFormatter()
        formatter.dateStyle = .long
        formatter.timeStyle = .short
        return formatter.string(from: date)
    }
}

// MARK: - Simple Markdown View

struct MarkdownView: View {
    let content: String

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(parseMarkdown().enumerated()), id: \.offset) { index, element in
                element
            }
        }
    }

    private func parseMarkdown() -> [AnyView] {
        var views: [AnyView] = []
        let lines = content.components(separatedBy: "\n")
        var inCodeBlock = false
        var codeBlockContent = ""
        var inTable = false
        var tableRows: [[String]] = []

        for line in lines {
            // Code blocks
            if line.hasPrefix("```") {
                if inCodeBlock {
                    views.append(AnyView(CodeBlockView(content: codeBlockContent)))
                    codeBlockContent = ""
                }
                inCodeBlock.toggle()
                continue
            }

            if inCodeBlock {
                codeBlockContent += (codeBlockContent.isEmpty ? "" : "\n") + line
                continue
            }

            // Tables
            if line.contains("|") && !line.trimmingCharacters(in: .whitespaces).isEmpty {
                if !inTable {
                    inTable = true
                    tableRows = []
                }

                // Skip separator lines (|---|---|)
                if line.contains("---") {
                    continue
                }

                let cells = line.components(separatedBy: "|")
                    .map { $0.trimmingCharacters(in: .whitespaces) }
                    .filter { !$0.isEmpty }

                if !cells.isEmpty {
                    tableRows.append(cells)
                }
                continue
            } else if inTable {
                views.append(AnyView(TableView(rows: tableRows)))
                tableRows = []
                inTable = false
            }

            // Headers
            if line.hasPrefix("# ") {
                views.append(AnyView(
                    Text(line.dropFirst(2))
                        .font(.title)
                        .fontWeight(.bold)
                        .padding(.top, 8)
                ))
            } else if line.hasPrefix("## ") {
                views.append(AnyView(
                    Text(line.dropFirst(3))
                        .font(.title2)
                        .fontWeight(.semibold)
                        .padding(.top, 6)
                ))
            } else if line.hasPrefix("### ") {
                views.append(AnyView(
                    Text(line.dropFirst(4))
                        .font(.title3)
                        .fontWeight(.medium)
                        .padding(.top, 4)
                ))
            }
            // Horizontal rule
            else if line == "---" || line == "***" {
                views.append(AnyView(Divider().padding(.vertical, 8)))
            }
            // Bullet lists
            else if line.hasPrefix("- ") || line.hasPrefix("* ") {
                views.append(AnyView(
                    HStack(alignment: .top, spacing: 8) {
                        Text("•")
                            .foregroundStyle(.secondary)
                        parseInlineMarkdown(String(line.dropFirst(2)))
                    }
                ))
            }
            // Checkbox lists
            else if line.hasPrefix("- [x] ") {
                views.append(AnyView(
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: "checkmark.square.fill")
                            .foregroundStyle(.green)
                        parseInlineMarkdown(String(line.dropFirst(6)))
                    }
                ))
            }
            else if line.hasPrefix("- [ ] ") {
                views.append(AnyView(
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: "square")
                            .foregroundStyle(.secondary)
                        parseInlineMarkdown(String(line.dropFirst(6)))
                    }
                ))
            }
            // Empty lines
            else if line.trimmingCharacters(in: .whitespaces).isEmpty {
                views.append(AnyView(Spacer().frame(height: 8)))
            }
            // Regular text
            else {
                views.append(AnyView(parseInlineMarkdown(line)))
            }
        }

        // Handle any remaining table
        if inTable && !tableRows.isEmpty {
            views.append(AnyView(TableView(rows: tableRows)))
        }

        return views
    }

    private func parseInlineMarkdown(_ text: String) -> Text {
        var result = Text("")

        // Simple parsing for bold and inline code
        var current = text
        while !current.isEmpty {
            if let boldRange = current.range(of: "\\*\\*(.+?)\\*\\*", options: .regularExpression) {
                let before = String(current[..<boldRange.lowerBound])
                let match = String(current[boldRange])
                let inner = String(match.dropFirst(2).dropLast(2))

                result = result + Text(before) + Text(inner).bold()
                current = String(current[boldRange.upperBound...])
            } else if let codeRange = current.range(of: "`(.+?)`", options: .regularExpression) {
                let before = String(current[..<codeRange.lowerBound])
                let match = String(current[codeRange])
                let inner = String(match.dropFirst(1).dropLast(1))

                result = result + Text(before) + Text(inner)
                    .font(.system(.body, design: .monospaced))
                    .foregroundStyle(.orange)
                current = String(current[codeRange.upperBound...])
            } else {
                result = result + Text(current)
                break
            }
        }

        return result
    }
}

// MARK: - Code Block View

struct CodeBlockView: View {
    let content: String

    var body: some View {
        ScrollView(.horizontal, showsIndicators: false) {
            Text(content)
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(.primary)
                .padding(12)
        }
        .background(Color(.systemGray6))
        .clipShape(RoundedRectangle(cornerRadius: 8))
    }
}

// MARK: - Table View

struct TableView: View {
    let rows: [[String]]

    var body: some View {
        VStack(spacing: 0) {
            ForEach(Array(rows.enumerated()), id: \.offset) { rowIndex, row in
                HStack(spacing: 0) {
                    ForEach(Array(row.enumerated()), id: \.offset) { colIndex, cell in
                        Text(cell)
                            .font(rowIndex == 0 ? .caption.bold() : .caption)
                            .padding(8)
                            .frame(maxWidth: .infinity, alignment: .leading)
                            .background(rowIndex == 0 ? Color(.systemGray5) : Color(.systemGray6).opacity(0.5))
                    }
                }

                if rowIndex == 0 {
                    Divider()
                }
            }
        }
        .clipShape(RoundedRectangle(cornerRadius: 8))
        .overlay(
            RoundedRectangle(cornerRadius: 8)
                .stroke(Color(.systemGray4), lineWidth: 1)
        )
    }
}

// MARK: - ReportInfo Identifiable

extension ReportInfo: Identifiable {}
