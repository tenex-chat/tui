import Foundation

// MARK: - Parsed Attachments Result

/// Result of parsing message content for text attachments.
struct ParsedAttachments {
    /// Ordered segments of the body — alternating text and attachment references.
    let segments: [MessageSegment]
    /// Extracted attachments keyed by label (e.g., "Text Attachment 1" → content).
    let attachments: [String: String]

    /// Whether any attachments were found.
    var hasAttachments: Bool { !attachments.isEmpty }
}

// MARK: - Message Segment

/// A segment of parsed message content — either plain text or an attachment reference.
enum MessageSegment: Identifiable {
    case text(String)
    case attachmentReference(label: String)

    var id: String {
        switch self {
        case .text(let content):
            return "text-\(content.hashValue)"
        case .attachmentReference(let label):
            return "ref-\(label)"
        }
    }
}

// MARK: - Attachment Parser

/// Parses message content to detect and extract inline text attachments.
///
/// Detects the pattern:
/// ```
/// [Text Attachment 1] some message text
///
/// ----
///
/// -- Text Attachment 1 --
///
/// Actual attachment content here...
/// ```
///
/// Strips the attachment body section (below the `----` separator) and provides
/// segments for rendering — text chunks interspersed with attachment references
/// that can be rendered as collapsible buttons.
enum AttachmentParser {

    // MARK: - Regex Patterns

    /// Matches `[Text Attachment X]` or `[Attachment X]` inline references.
    private static let referencePattern = try! NSRegularExpression(
        pattern: #"\[(Text Attachment|Attachment)\s+(\d+)\]"#,
        options: []
    )

    /// Matches `-- Text Attachment X --` or `-- Attachment X --` section headers.
    private static let headerPattern = try! NSRegularExpression(
        pattern: #"^--\s*(Text Attachment|Attachment)\s+(\d+)\s*--\s*$"#,
        options: [.anchorsMatchLines]
    )

    // MARK: - Public API

    /// Parse message content for text attachments.
    ///
    /// Returns `nil` if no attachment pattern is detected (fast path for normal messages).
    static func parse(_ content: String) -> ParsedAttachments? {
        // Quick check: must contain at least one inline reference
        let nsContent = content as NSString
        let fullRange = NSRange(location: 0, length: nsContent.length)
        guard referencePattern.firstMatch(in: content, options: [], range: fullRange) != nil else {
            return nil
        }

        // Find the separator and split
        guard let (body, attachmentSection) = splitAtSeparator(content) else {
            return nil
        }

        // Extract attachments from the section below the separator
        let attachments = extractAttachments(from: attachmentSection)

        // If no attachments found below the separator, this isn't the pattern we're looking for
        guard !attachments.isEmpty else { return nil }

        // Parse the body into segments
        let segments = segmentize(body, attachments: attachments)

        return ParsedAttachments(
            segments: segments,
            attachments: attachments
        )
    }

    // MARK: - Internal Helpers

    /// Split content at the `----` separator line.
    ///
    /// Returns `(body, attachmentSection)` or `nil` if no valid separator is found.
    private static func splitAtSeparator(_ content: String) -> (String, String)? {
        let lines = content.components(separatedBy: "\n")

        for i in 0..<lines.count {
            let trimmed = lines[i].trimmingCharacters(in: .whitespaces)

            // Match separator: a line that is only dashes (at least 3)
            guard trimmed.count >= 3,
                  trimmed.allSatisfy({ $0 == "-" }) else {
                continue
            }

            // Check if there are attachment headers below this separator
            let below = lines[(i + 1)...].joined(separator: "\n")
            let nsBelow = below as NSString
            let belowRange = NSRange(location: 0, length: nsBelow.length)

            guard headerPattern.firstMatch(in: below, options: [], range: belowRange) != nil else {
                continue
            }

            // Found the separator — split
            let body = lines[0..<i]
                .joined(separator: "\n")
                .trimmingCharacters(in: .whitespacesAndNewlines)
            let attachmentSection = below

            return (body, attachmentSection)
        }

        return nil
    }

    /// Extract attachment label→content pairs from the section below the separator.
    private static func extractAttachments(from section: String) -> [String: String] {
        var attachments: [String: String] = [:]
        let lines = section.components(separatedBy: "\n")
        var currentLabel: String?
        var currentContent: [String] = []

        for line in lines {
            let nsLine = line as NSString
            let lineRange = NSRange(location: 0, length: nsLine.length)

            if let match = headerPattern.firstMatch(in: line, options: [], range: lineRange) {
                // Save previous attachment if any
                if let label = currentLabel {
                    attachments[label] = currentContent
                        .joined(separator: "\n")
                        .trimmingCharacters(in: .whitespacesAndNewlines)
                }

                // Start new attachment
                let type = nsLine.substring(with: match.range(at: 1))
                let num = nsLine.substring(with: match.range(at: 2))
                currentLabel = "\(type) \(num)"
                currentContent = []
            } else if currentLabel != nil {
                currentContent.append(line)
            }
        }

        // Save last attachment
        if let label = currentLabel {
            attachments[label] = currentContent
                .joined(separator: "\n")
                .trimmingCharacters(in: .whitespacesAndNewlines)
        }

        return attachments
    }

    /// Split body text into alternating text and attachment-reference segments.
    private static func segmentize(_ body: String, attachments: [String: String]) -> [MessageSegment] {
        var segments: [MessageSegment] = []
        let nsBody = body as NSString
        let fullRange = NSRange(location: 0, length: nsBody.length)
        let matches = referencePattern.matches(in: body, options: [], range: fullRange)

        var lastEnd = 0

        for match in matches {
            // Text before the reference
            if match.range.location > lastEnd {
                let beforeRange = NSRange(location: lastEnd, length: match.range.location - lastEnd)
                let beforeText = nsBody.substring(with: beforeRange)
                if !beforeText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                    segments.append(.text(beforeText))
                }
            }

            // The reference itself
            let type = nsBody.substring(with: match.range(at: 1))
            let num = nsBody.substring(with: match.range(at: 2))
            let label = "\(type) \(num)"

            // Only render as attachment reference if we have the content
            if attachments[label] != nil {
                segments.append(.attachmentReference(label: label))
            } else {
                // Keep as text if we don't have matching content
                let refText = nsBody.substring(with: match.range)
                segments.append(.text(refText))
            }

            lastEnd = match.range.location + match.range.length
        }

        // Text after the last reference
        if lastEnd < nsBody.length {
            let remainingRange = NSRange(location: lastEnd, length: nsBody.length - lastEnd)
            let remainingText = nsBody.substring(with: remainingRange)
            if !remainingText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                segments.append(.text(remainingText))
            }
        }

        // Fallback: if no segments were created, use the whole body
        if segments.isEmpty && !body.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            segments.append(.text(body))
        }

        return segments
    }
}
