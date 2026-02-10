import SwiftUI

// MARK: - Markdown View

/// A reusable view that renders markdown content with support for headers, lists, code blocks, tables, etc.
///
/// ## Performance Optimizations
/// - Uses Equatable conformance to prevent unnecessary re-renders
/// - Caches parsed elements to avoid re-parsing on every body evaluation
/// - Uses identifiable wrapper for stable ForEach identity
struct MarkdownView: View, Equatable {
    let content: String

    /// Cache for parsed markdown elements, keyed by content hash
    /// This prevents re-parsing the same content multiple times
    private static var parseCache: [Int: [MarkdownElement]] = [:]
    private static let cacheLock = NSLock()
    private static let maxCacheSize = 100

    static func == (lhs: MarkdownView, rhs: MarkdownView) -> Bool {
        lhs.content == rhs.content
    }

    var body: some View {
        let elements = cachedParse()
        VStack(alignment: .leading, spacing: 12) {
            ForEach(elements) { element in
                element.view
            }
        }
    }

    /// Returns cached parsed elements or parses and caches if needed
    private func cachedParse() -> [MarkdownElement] {
        let contentHash = content.hashValue

        Self.cacheLock.lock()
        defer { Self.cacheLock.unlock() }

        if let cached = Self.parseCache[contentHash] {
            return cached
        }

        let parsed = parseMarkdown()

        // Evict oldest entries if cache is too large
        if Self.parseCache.count >= Self.maxCacheSize {
            // Simple eviction: remove half the cache
            let keysToRemove = Array(Self.parseCache.keys.prefix(Self.maxCacheSize / 2))
            for key in keysToRemove {
                Self.parseCache.removeValue(forKey: key)
            }
        }

        Self.parseCache[contentHash] = parsed
        return parsed
    }

    private func parseMarkdown() -> [MarkdownElement] {
        var elements: [MarkdownElement] = []
        let lines = content.components(separatedBy: "\n")
        var inCodeBlock = false
        var codeBlockContent = ""
        var inTable = false
        var tableRows: [[String]] = []

        for line in lines {
            // Code blocks
            if line.hasPrefix("```") {
                if inCodeBlock {
                    elements.append(MarkdownElement(view: AnyView(CodeBlockView(content: codeBlockContent))))
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
                elements.append(MarkdownElement(view: AnyView(TableView(rows: tableRows))))
                tableRows = []
                inTable = false
            }

            // Headers
            if line.hasPrefix("# ") {
                elements.append(MarkdownElement(view: AnyView(
                    Text(line.dropFirst(2))
                        .font(.title)
                        .fontWeight(.bold)
                        .padding(.top, 8)
                )))
            } else if line.hasPrefix("## ") {
                elements.append(MarkdownElement(view: AnyView(
                    Text(line.dropFirst(3))
                        .font(.title2)
                        .fontWeight(.semibold)
                        .padding(.top, 6)
                )))
            } else if line.hasPrefix("### ") {
                elements.append(MarkdownElement(view: AnyView(
                    Text(line.dropFirst(4))
                        .font(.title3)
                        .fontWeight(.medium)
                        .padding(.top, 4)
                )))
            }
            // Horizontal rule
            else if line == "---" || line == "***" {
                elements.append(MarkdownElement(view: AnyView(Divider().padding(.vertical, 8))))
            }
            // Checkbox lists (must check before bullet lists due to prefix overlap)
            else if line.hasPrefix("- [x] ") {
                elements.append(MarkdownElement(view: AnyView(
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: "checkmark.square.fill")
                            .foregroundStyle(.green)
                        parseInlineMarkdown(String(line.dropFirst(6)))
                    }
                )))
            }
            else if line.hasPrefix("- [ ] ") {
                elements.append(MarkdownElement(view: AnyView(
                    HStack(alignment: .top, spacing: 8) {
                        Image(systemName: "square")
                            .foregroundStyle(.secondary)
                        parseInlineMarkdown(String(line.dropFirst(6)))
                    }
                )))
            }
            // Bullet lists
            else if line.hasPrefix("- ") || line.hasPrefix("* ") {
                elements.append(MarkdownElement(view: AnyView(
                    HStack(alignment: .top, spacing: 8) {
                        Text("•")
                            .foregroundStyle(.secondary)
                        parseInlineMarkdown(String(line.dropFirst(2)))
                    }
                )))
            }
            // Empty lines
            else if line.trimmingCharacters(in: .whitespaces).isEmpty {
                elements.append(MarkdownElement(view: AnyView(Spacer().frame(height: 8))))
            }
            // Regular text
            else {
                elements.append(MarkdownElement(view: AnyView(parseInlineMarkdown(line))))
            }
        }

        // Handle any remaining table
        if inTable && !tableRows.isEmpty {
            elements.append(MarkdownElement(view: AnyView(TableView(rows: tableRows))))
        }

        return elements
    }

    private func parseInlineMarkdown(_ text: String) -> Text {
        var result = AttributedString()

        // Simple parsing for bold and inline code
        var current = text
        while !current.isEmpty {
            if let boldRange = current.range(of: "\\*\\*(.+?)\\*\\*", options: .regularExpression) {
                let before = String(current[..<boldRange.lowerBound])
                let match = String(current[boldRange])
                let inner = String(match.dropFirst(2).dropLast(2))

                result.append(AttributedString(before))
                var boldPart = AttributedString(inner)
                boldPart.font = .body.bold()
                result.append(boldPart)
                current = String(current[boldRange.upperBound...])
            } else if let codeRange = current.range(of: "`(.+?)`", options: .regularExpression) {
                let before = String(current[..<codeRange.lowerBound])
                let match = String(current[codeRange])
                let inner = String(match.dropFirst(1).dropLast(1))

                result.append(AttributedString(before))
                var codePart = AttributedString(inner)
                codePart.font = .system(.body, design: .monospaced)
                codePart.foregroundColor = .orange
                result.append(codePart)
                current = String(current[codeRange.upperBound...])
            } else {
                result.append(AttributedString(current))
                break
            }
        }

        return Text(result)
    }

    /// Clears the parse cache (call on memory warning)
    static func clearCache() {
        cacheLock.lock()
        parseCache.removeAll()
        cacheLock.unlock()
    }
}

// MARK: - Markdown Element (Identifiable wrapper)

/// Wrapper to give AnyView a stable identity for ForEach
private struct MarkdownElement: Identifiable {
    let id = UUID()
    let view: AnyView
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
        .background(Color.systemGray6)
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
                            .background(rowIndex == 0 ? Color.systemGray5 : Color.systemGray6.opacity(0.5))
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
                .stroke(Color.systemGray4, lineWidth: 1)
        )
    }
}
