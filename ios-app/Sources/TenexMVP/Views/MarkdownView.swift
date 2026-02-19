import SwiftUI
#if os(iOS)
import UIKit
#endif

// MARK: - URL Detection Utilities (Shared)

/// Shared utilities for URL detection in markdown content
enum MarkdownURLUtilities {
    /// Image file extensions that should be rendered inline
    static let imageExtensions = ["png", "jpg", "jpeg", "gif", "webp", "svg", "bmp", "ico", "tiff", "tif", "heic", "heif", "avif"]

    /// URL regex pattern - matches http/https URLs
    static let urlPattern = try! NSRegularExpression(
        pattern: #"https?://[^\s\)\]\>\"\']+[^\s\.\,\;\:\!\?\)\]\>\"\'…]"#,
        options: [.caseInsensitive]
    )

    /// Check if a URL points to an image based on file extension
    static func isImageURL(_ urlString: String) -> Bool {
        // Remove query parameters and fragments for extension check
        let cleanURL = urlString.components(separatedBy: "?").first ?? urlString
        let cleanURLWithoutFragment = cleanURL.components(separatedBy: "#").first ?? cleanURL
        let lowercased = cleanURLWithoutFragment.lowercased()
        return imageExtensions.contains { lowercased.hasSuffix(".\($0)") }
    }

    /// Check if text contains any URLs
    static func containsURL(_ text: String) -> Bool {
        let nsText = text as NSString
        let range = NSRange(location: 0, length: nsText.length)
        return urlPattern.firstMatch(in: text, options: [], range: range) != nil
    }
}

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

        let parseStartedAt = CFAbsoluteTimeGetCurrent()
        let parsed = parseMarkdown()
        let parseMs = (CFAbsoluteTimeGetCurrent() - parseStartedAt) * 1000
        PerformanceProfiler.shared.logEvent(
            "markdown parse cache-miss chars=\(content.count) lines=\(content.split(separator: "\n", omittingEmptySubsequences: false).count) elements=\(parsed.count) parseMs=\(String(format: "%.2f", parseMs))",
            category: .swiftUI,
            level: parseMs >= 25 ? .error : .debug
        )

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
                            .foregroundStyle(Color.todoDone)
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
///
/// ## Performance Optimizations
/// - Segments are parsed once in init and stored with stable IDs
/// - Plain text without URLs bypasses FlowLayout for proper wrapping
/// - Segments have stable identity based on content hash + position
struct InlineMarkdownView: View {
    let text: String

    /// Parsed segments, computed once at init for stable identity
    private let segments: [InlineSegment]

    /// Whether this content contains any URLs (determines layout strategy)
    private let containsURLs: Bool

    init(text: String) {
        self.text = text
        self.containsURLs = MarkdownURLUtilities.containsURL(text)
        self.segments = Self.parseTextWithURLs(text)
    }

    var body: some View {
        if containsURLs {
            // Use FlowLayout only when we have mixed content (text + links/images)
            FlowLayout(spacing: 0) {
                ForEach(segments) { segment in
                    segmentView(for: segment)
                }
            }
        } else {
            // For plain text without URLs, use regular Text for proper wrapping
            if let firstSegment = segments.first, case .text(let attributed) = firstSegment.content {
                Text(attributed)
            } else {
                Text(text)
            }
        }
    }

    @ViewBuilder
    private func segmentView(for segment: InlineSegment) -> some View {
        switch segment.content {
        case .text(let attributed):
            Text(attributed)
                .fixedSize(horizontal: false, vertical: true)
        case .link(let url, let displayText):
            InlineLinkView(url: url, displayText: displayText)
        case .image(let url, let urlString):
            InlineImageView(url: url, urlString: urlString)
        }
    }

    /// Parse text into segments, separating URLs from regular text
    /// This is a static function to enable calling from init
    private static func parseTextWithURLs(_ text: String) -> [InlineSegment] {
        var segments: [InlineSegment] = []
        let nsText = text as NSString
        let range = NSRange(location: 0, length: nsText.length)

        var lastEnd = 0
        let matches = MarkdownURLUtilities.urlPattern.matches(in: text, options: [], range: range)

        for match in matches {
            // Add text before the URL
            if match.range.location > lastEnd {
                let beforeRange = NSRange(location: lastEnd, length: match.range.location - lastEnd)
                let beforeText = nsText.substring(with: beforeRange)
                if !beforeText.isEmpty {
                    let attributed = parseFormattedText(beforeText)
                    segments.append(InlineSegment(
                        content: .text(attributed),
                        rangeStart: lastEnd
                    ))
                }
            }

            // Add the URL (as image or link)
            let urlString = nsText.substring(with: match.range)
            if MarkdownURLUtilities.isImageURL(urlString), let url = URL(string: urlString) {
                segments.append(InlineSegment(
                    content: .image(url: url, urlString: urlString),
                    rangeStart: match.range.location
                ))
            } else if let url = URL(string: urlString) {
                segments.append(InlineSegment(
                    content: .link(url: url, displayText: urlString),
                    rangeStart: match.range.location
                ))
            } else {
                // Fallback: render as plain text if URL parsing fails
                segments.append(InlineSegment(
                    content: .text(AttributedString(urlString)),
                    rangeStart: match.range.location
                ))
            }

            lastEnd = match.range.location + match.range.length
        }

        // Add remaining text after the last URL
        if lastEnd < nsText.length {
            let remainingRange = NSRange(location: lastEnd, length: nsText.length - lastEnd)
            let remainingText = nsText.substring(with: remainingRange)
            if !remainingText.isEmpty {
                let attributed = parseFormattedText(remainingText)
                segments.append(InlineSegment(
                    content: .text(attributed),
                    rangeStart: lastEnd
                ))
            }
        }

        // If no URLs found, just parse the whole text for formatting
        if segments.isEmpty {
            let attributed = parseFormattedText(text)
            segments.append(InlineSegment(
                content: .text(attributed),
                rangeStart: 0
            ))
        }

        return segments
    }

    /// Parse text for bold and inline code formatting
    private static func parseFormattedText(_ text: String) -> AttributedString {
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

        return result
    }
}

// MARK: - Inline Segment (with Stable Identity)

/// Content types for inline segments
private enum InlineSegmentContent: Hashable {
    case text(AttributedString)
    case link(url: URL, displayText: String)
    case image(url: URL, urlString: String)
}

/// Wrapper for inline content segments with stable identity
/// Identity is derived from content type + position, not random UUID
private struct InlineSegment: Identifiable {
    let content: InlineSegmentContent
    let rangeStart: Int

    /// Stable ID derived from content hash and position
    var id: Int {
        var hasher = Hasher()
        hasher.combine(rangeStart)
        hasher.combine(content)
        return hasher.finalize()
    }
}

// MARK: - Inline Link View

/// A clickable link that opens in the browser
struct InlineLinkView: View {
    let url: URL
    let displayText: String

    var body: some View {
        Link(destination: url) {
            Text(displayText)
                .foregroundStyle(Color.composerAction)
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
///
/// ## Width Handling
/// - Uses a reasonable default width (active screen width - padding) when parent doesn't specify
/// - Passes width constraints to text subviews so they can wrap properly
/// - Only wraps to next line when a subview cannot fit
struct FlowLayout: Layout {
    let spacing: CGFloat

    /// Default width to use when parent doesn't specify one
    /// This prevents infinite width scenarios
    private static var defaultMaxWidth: CGFloat {
        #if os(iOS)
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        let activeScene = scenes.first { $0.activationState == .foregroundActive } ?? scenes.first
        if let screenWidth = activeScene?.screen.bounds.width, screenWidth.isFinite, screenWidth > 0 {
            return max(320, screenWidth - 32) // Account for typical horizontal padding.
        }
        return 360
        #else
        return 600 // Reasonable default for macOS
        #endif
    }

    init(spacing: CGFloat = 4) {
        self.spacing = spacing
    }

    func sizeThatFits(proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) -> CGSize {
        let maxWidth = resolveWidth(proposal.width)
        let result = FlowResult(in: maxWidth, subviews: subviews, spacing: spacing)
        return result.size
    }

    func placeSubviews(in bounds: CGRect, proposal: ProposedViewSize, subviews: Subviews, cache: inout ()) {
        let maxWidth = bounds.width > 0 ? bounds.width : resolveWidth(proposal.width)
        let result = FlowResult(in: maxWidth, subviews: subviews, spacing: spacing)

        for (index, subview) in subviews.enumerated() {
            let point = result.positions[index]
            let size = result.sizes[index]
            // Pass width constraint to subview so Text can wrap properly
            subview.place(
                at: CGPoint(x: bounds.minX + point.x, y: bounds.minY + point.y),
                proposal: ProposedViewSize(width: size.width, height: size.height)
            )
        }
    }

    /// Resolve width, using a reasonable default when not specified
    private func resolveWidth(_ proposedWidth: CGFloat?) -> CGFloat {
        guard let width = proposedWidth, width.isFinite, width > 0 else {
            return Self.defaultMaxWidth
        }
        return width
    }

    private struct FlowResult {
        var size: CGSize = .zero
        var positions: [CGPoint] = []
        var sizes: [CGSize] = []

        init(in maxWidth: CGFloat, subviews: Subviews, spacing: CGFloat) {
            var currentX: CGFloat = 0
            var currentY: CGFloat = 0
            var lineHeight: CGFloat = 0

            for subview in subviews {
                // Calculate remaining width on current line
                let remainingWidth = max(0, maxWidth - currentX)

                // First, try to fit with remaining width constraint
                let constrainedSize = subview.sizeThatFits(ProposedViewSize(width: remainingWidth, height: nil))

                // If the constrained size doesn't fit and we're not at line start, try full width on new line
                if constrainedSize.width > remainingWidth && currentX > 0 {
                    // Move to next line
                    currentX = 0
                    currentY += lineHeight + spacing
                    lineHeight = 0

                    // Re-measure with full width available
                    let fullWidthSize = subview.sizeThatFits(ProposedViewSize(width: maxWidth, height: nil))
                    let viewSize = CGSize(
                        width: min(fullWidthSize.width, maxWidth),
                        height: fullWidthSize.height
                    )

                    positions.append(CGPoint(x: currentX, y: currentY))
                    sizes.append(viewSize)

                    currentX += viewSize.width + spacing
                    lineHeight = max(lineHeight, viewSize.height)
                } else {
                    // Fits on current line (or we're at line start, so must place here)
                    let viewSize = CGSize(
                        width: min(constrainedSize.width, remainingWidth),
                        height: constrainedSize.height
                    )

                    positions.append(CGPoint(x: currentX, y: currentY))
                    sizes.append(viewSize)

                    currentX += viewSize.width + spacing
                    lineHeight = max(lineHeight, viewSize.height)
                }

                size.width = max(size.width, currentX - spacing)
            }

            size.height = currentY + lineHeight
        }
    }
}
