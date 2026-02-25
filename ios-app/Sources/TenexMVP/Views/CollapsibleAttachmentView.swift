import SwiftUI

// MARK: - Collapsible Attachment View

/// A tappable button that reveals attachment content when expanded.
///
/// Renders as a compact pill-shaped button showing the attachment label
/// (e.g., "Text Attachment 1"). Tapping toggles inline expansion of the
/// full attachment content rendered through `MarkdownView`.
struct CollapsibleAttachmentView: View {
    let label: String
    let content: String

    @State private var isExpanded = false

    var body: some View {
        VStack(alignment: .leading, spacing: 6) {
            // Tappable attachment button
            Button {
                withAnimation(.easeInOut(duration: 0.2)) {
                    isExpanded.toggle()
                }
            } label: {
                HStack(spacing: 5) {
                    Image(systemName: isExpanded ? "chevron.down" : "chevron.right")
                        .font(.caption2)
                        .fontWeight(.semibold)
                    Image(systemName: "doc.text")
                        .font(.caption2)
                    Text(label)
                        .font(.caption)
                        .fontWeight(.medium)
                }
                .foregroundStyle(Color.composerAction)
                .padding(.horizontal, 10)
                .padding(.vertical, 6)
                .background(Color.composerAction.opacity(0.1))
                .clipShape(RoundedRectangle(cornerRadius: 6))
            }
            .buttonStyle(.borderless)

            // Expanded attachment content
            if isExpanded {
                MarkdownView(content: content)
                    .font(.callout)
                    .foregroundStyle(.primary.opacity(0.85))
                    .padding(12)
                    .frame(maxWidth: .infinity, alignment: .leading)
                    .background(Color.systemGray6)
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .overlay(
                        RoundedRectangle(cornerRadius: 8)
                            .stroke(Color.systemGray4.opacity(0.5), lineWidth: 0.5)
                    )
                    .transition(.opacity.combined(with: .scale(scale: 0.98, anchor: .top)))
            }
        }
    }
}

// MARK: - Message Content View

/// Renders message content with attachment-aware processing.
///
/// When the content contains the text-attachment pattern (inline references like
/// `[Text Attachment 1]` paired with attachment bodies below a `----` separator),
/// this view strips the raw attachment section and renders the body text with
/// interactive collapsible buttons in place of the attachment references.
///
/// For messages without attachments, this view transparently delegates to `MarkdownView`.
struct MessageContentView: View, Equatable {
    let content: String

    static func == (lhs: MessageContentView, rhs: MessageContentView) -> Bool {
        lhs.content == rhs.content
    }

    /// Cached parse result to avoid re-parsing on every body evaluation.
    private static var parseCache: [Int: ParsedAttachments?] = [:]
    private static let cacheLock = NSLock()
    private static let maxCacheSize = 80

    var body: some View {
        let parsed = cachedParse()

        if let parsed, parsed.hasAttachments {
            VStack(alignment: .leading, spacing: 8) {
                ForEach(parsed.segments) { segment in
                    segmentView(segment, attachments: parsed.attachments)
                }
            }
        } else {
            MarkdownView(content: content)
        }
    }

    @ViewBuilder
    private func segmentView(_ segment: MessageSegment, attachments: [String: String]) -> some View {
        switch segment {
        case .text(let text):
            MarkdownView(content: text)
        case .attachmentReference(let label):
            if let attachmentContent = attachments[label] {
                CollapsibleAttachmentView(label: label, content: attachmentContent)
            }
        }
    }

    // MARK: - Parse Cache

    private func cachedParse() -> ParsedAttachments? {
        let contentHash = content.hashValue

        Self.cacheLock.lock()
        defer { Self.cacheLock.unlock() }

        if let cached = Self.parseCache[contentHash] {
            return cached
        }

        let result = AttachmentParser.parse(content)

        // Evict oldest entries if cache is too large
        if Self.parseCache.count >= Self.maxCacheSize {
            let keysToRemove = Array(Self.parseCache.keys.prefix(Self.maxCacheSize / 2))
            for key in keysToRemove {
                Self.parseCache.removeValue(forKey: key)
            }
        }

        Self.parseCache[contentHash] = result
        return result
    }

    /// Clears the parse cache (call on memory warning).
    static func clearCache() {
        cacheLock.lock()
        parseCache.removeAll()
        cacheLock.unlock()
    }
}

// MARK: - Preview

#Preview("Collapsible Attachment") {
    VStack(alignment: .leading, spacing: 20) {
        CollapsibleAttachmentView(
            label: "Text Attachment 1",
            content: "This is the content of the first attachment.\n\nIt can contain **markdown** and `code blocks`."
        )

        CollapsibleAttachmentView(
            label: "Attachment 2",
            content: "Another attachment with different content."
        )
    }
    .padding()
}

#Preview("Message with Attachments") {
    MessageContentView(content: """
    [Text Attachment 1] tell me more about the "Steinberger → OpenAI" thing

    ----

    -- Text Attachment 1 --

    This is the full attachment content that was previously displayed inline.
    It can contain **bold text**, `code`, and other markdown formatting.

    - List item 1
    - List item 2
    """)
    .padding()
}
