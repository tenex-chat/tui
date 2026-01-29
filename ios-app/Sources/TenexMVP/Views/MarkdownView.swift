import SwiftUI

// MARK: - Markdown View

/// A reusable view that renders markdown content with support for headers, lists, code blocks, tables, etc.
struct MarkdownView: View {
    let content: String

    var body: some View {
        VStack(alignment: .leading, spacing: 12) {
            ForEach(Array(parseMarkdown().enumerated()), id: \.offset) { _, element in
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
                    ForEach(Array(row.enumerated()), id: \.offset) { _, cell in
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
