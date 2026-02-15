import SwiftUI

// MARK: - Markdown View

/// A reusable view that renders markdown content with support for headers, lists, code blocks, tables, etc.
///
/// ## Performance Optimizations
/// - Uses Equatable conformance to prevent unnecessary re-renders
/// - Caches parsed elements to avoid re-parsing on every body evaluation
/// - Uses identifiable wrapper for stable ForEach identity
///
/// ## Features
/// - Clickable links (URLs are tappable and open in browser)
/// - Inline images (image URLs are rendered as embedded images)
/// - Markdown formatting (headers, code blocks, tables, lists, bold, inline code)
struct MarkdownView: View, Equatable {
    let content: String

    /// Cache for parsed markdown elements, keyed by content hash
    /// This prevents re-parsing the same content multiple times
    private static var parseCache: [Int: [MarkdownElement]] = [:]
    private static let cacheLock = NSLock()
    private static let maxCacheSize = 100

    /// Image file extensions that should be rendered inline
    private static let imageExtensions = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tiff", "tif", "heic", "heif", "avif"]

    /// URL regex pattern - matches http/https URLs
    private static let urlPattern = try! NSRegularExpression(
        pattern: #"https?://[^\s\)\]\>\"\']+[^\s\.\,\;\:\!\?\)\]\>\"\'…]"#,
        options: [.caseInsensitive]
    )

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

    /// Check if a URL points to an image based on file extension
    private static func isImageURL(_ urlString: String) -> Bool {
        // Remove query parameters and fragments for extension check
        let cleanURL = urlString.components(separatedBy: "?").first ?? urlString
        let cleanURLWithoutFragment = cleanURL.components(separatedBy: "#").first ?? cleanURL
        let lowercased = cleanURLWithoutFragment.lowercased()
        return imageExtensions.contains { lowercased.hasSuffix(".\($0)") }
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

    /// Parse inline markdown and return a view that can contain text, links, and images
    @ViewBuilder
    private func parseInlineMarkdown(_ text: String) -> some View {
        InlineMarkdownView(text: text)
    }

    /// Parse inline markdown and return a Text view (for use in headers and simple contexts)
    private func parseInlineMarkdownText(_ text: String) -> Text {
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

// MARK: - Inline Markdown View (with Links and Images)

/// A view that renders inline markdown content with support for:
/// - Clickable links (URLs open in browser)
/// - Inline images (image URLs rendered as embedded images)
/// - Bold text (**text**)
/// - Inline code (`code`)
struct InlineMarkdownView: View {
    let text: String

    /// Image file extensions that should be rendered inline
    private static let imageExtensions = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tiff", "tif", "heic", "heif", "avif"]

    /// URL regex pattern - matches http/https URLs
    private static let urlPattern = try! NSRegularExpression(
        pattern: #"https?://[^\s\)\]\>\"\']+[^\s\.\,\;\:\!\?\)\]\>\"\'…]"#,
        options: [.caseInsensitive]
    )

    var body: some View {
        let segments = parseTextWithURLs(text)

        // Use FlowLayout for wrapping content
        FlowLayout(spacing: 0) {
            ForEach(segments) { segment in
                segment.view
            }
        }
    }

    /// Check if a URL points to an image based on file extension
    private static func isImageURL(_ urlString: String) -> Bool {
        // Remove query parameters and fragments for extension check
        let cleanURL = urlString.components(separatedBy: "?").first ?? urlString
        let cleanURLWithoutFragment = cleanURL.components(separatedBy: "#").first ?? cleanURL
        let lowercased = cleanURLWithoutFragment.lowercased()
        return imageExtensions.contains { lowercased.hasSuffix(".\($0)") }
    }

    /// Parse text into segments, separating URLs from regular text
    private func parseTextWithURLs(_ text: String) -> [InlineSegment] {
        var segments: [InlineSegment] = []
        let nsText = text as NSString
        let range = NSRange(location: 0, length: nsText.length)

        var lastEnd = 0
        let matches = Self.urlPattern.matches(in: text, options: [], range: range)

        for match in matches {
            // Add text before the URL
            if match.range.location > lastEnd {
                let beforeRange = NSRange(location: lastEnd, length: match.range.location - lastEnd)
                let beforeText = nsText.substring(with: beforeRange)
                if !beforeText.isEmpty {
                    segments.append(InlineSegment(view: AnyView(parseFormattedText(beforeText))))
                }
            }

            // Add the URL (as image or link)
            let urlString = nsText.substring(with: match.range)
            if Self.isImageURL(urlString), let url = URL(string: urlString) {
                segments.append(InlineSegment(view: AnyView(InlineImageView(url: url, urlString: urlString))))
            } else if let url = URL(string: urlString) {
                segments.append(InlineSegment(view: AnyView(InlineLinkView(url: url, displayText: urlString))))
            } else {
                // Fallback: render as plain text if URL parsing fails
                segments.append(InlineSegment(view: AnyView(Text(urlString))))
            }

            lastEnd = match.range.location + match.range.length
        }

        // Add remaining text after the last URL
        if lastEnd < nsText.length {
            let remainingRange = NSRange(location: lastEnd, length: nsText.length - lastEnd)
            let remainingText = nsText.substring(with: remainingRange)
            if !remainingText.isEmpty {
                segments.append(InlineSegment(view: AnyView(parseFormattedText(remainingText))))
            }
        }

        // If no URLs found, just parse the whole text for formatting
        if segments.isEmpty {
            segments.append(InlineSegment(view: AnyView(parseFormattedText(text))))
        }

        return segments
    }

    /// Parse text for bold and inline code formatting
    private func parseFormattedText(_ text: String) -> Text {
        var result = AttributedString()
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
}

// MARK: - Inline Segment (Identifiable wrapper)

/// Wrapper for inline content segments
private struct InlineSegment: Identifiable {
    let id = UUID()
    let view: AnyView
}

// MARK: - Inline Link View

/// A clickable link that opens in the browser
struct InlineLinkView: View {
    let url: URL
    let displayText: String

    var body: some View {
        Link(destination: url) {
            Text(displayText)
                .foregroundStyle(Color.accentColor)
                .underline()
        }
    }
}

// MARK: - Inline Image View

/// An embedded image loaded from a URL
struct InlineImageView: View {
    let url: URL
    let urlString: String

    /// Maximum width for inline images
    private let maxImageWidth: CGFloat = 300

    /// Maximum height for inline images
    private let maxImageHeight: CGFloat = 200

    var body: some View {
        VStack(alignment: .leading, spacing: 4) {
            AsyncImage(url: url) { phase in
                switch phase {
                case .empty:
                    ProgressView()
                        .frame(width: 100, height: 60)
                case .success(let image):
                    Link(destination: url) {
                        image
                            .resizable()
                            .aspectRatio(contentMode: .fit)
                            .frame(maxWidth: maxImageWidth, maxHeight: maxImageHeight)
                            .clipShape(RoundedRectangle(cornerRadius: 8))
                    }
                case .failure:
                    // On failure, show the URL as a clickable link instead
                    InlineLinkView(url: url, displayText: urlString)
                @unknown default:
                    InlineLinkView(url: url, displayText: urlString)
                }
            }
        }
        .padding(.vertical, 4)
    }
}

// MARK: - Flow Layout

/// A layout that wraps content horizontally like text
struct FlowLayout: Layout {
    let spacing: CGFloat

    init(spacing: CGFloat = 4) {
        self.spacing = spacing
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let result = FlowResult(in: proposal.width ?? .infinity, subviews: subviews, spacing: spacing)
        return result.size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let result = FlowResult(in: bounds.width, subviews: subviews, spacing: spacing)

        for (index, subview) in subviews.enumerated() {
            let point = result.positions[index]
            subview.place(at: CGPoint(x: bounds.minX + point.x, y: bounds.minY + point.y), proposal: .unspecified)
        }
    }

    private struct FlowResult {
        var size: CGSize = .zero
        var positions: [CGPoint] = []

        init(in maxWidth: CGFloat, subviews: Subviews, spacing: CGFloat) {
            var currentX: CGFloat = 0
            var currentY: CGFloat = 0
            var lineHeight: CGFloat = 0

            for subview in subviews {
                let viewSize = subview.sizeThatFits(.unspecified)

                // If this view doesn't fit on current line, move to next line
                if currentX + viewSize.width > maxWidth && currentX > 0 {
                    currentX = 0
                    currentY += lineHeight + spacing
                    lineHeight = 0
                }

                positions.append(CGPoint(x: currentX, y: currentY))

                currentX += viewSize.width + spacing
                lineHeight = max(lineHeight, viewSize.height)

                size.width = max(size.width, currentX)
            }

            size.height = currentY + lineHeight
        }
    }
}
