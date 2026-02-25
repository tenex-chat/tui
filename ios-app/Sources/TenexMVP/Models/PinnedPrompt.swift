import Foundation

/// A locally persisted reusable prompt that can be quickly applied in the composer.
struct PinnedPrompt: Codable, Identifiable, Equatable {
    let id: String
    var title: String
    var text: String
    let createdAt: Date
    var lastModified: Date
    var lastUsedAt: Date

    init(
        id: String = UUID().uuidString,
        title: String,
        text: String,
        createdAt: Date = Date(),
        lastModified: Date = Date(),
        lastUsedAt: Date = Date()
    ) {
        self.id = id
        self.title = title
        self.text = text
        self.createdAt = createdAt
        self.lastModified = lastModified
        self.lastUsedAt = lastUsedAt
    }

    var preview: String {
        let cleaned = text.replacingOccurrences(of: "\n", with: " ")
        if cleaned.count <= 100 { return cleaned }
        return String(cleaned.prefix(100)) + "..."
    }

    mutating func markUsed(_ date: Date = Date()) {
        lastUsedAt = date
        lastModified = date
    }

    static func normalized(title: String, text: String) -> (title: String, text: String)? {
        let normalizedTitle = title.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalizedText = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !normalizedTitle.isEmpty, !normalizedText.isEmpty else { return nil }
        return (normalizedTitle, normalizedText)
    }

    static func sortComparator(_ lhs: PinnedPrompt, _ rhs: PinnedPrompt) -> Bool {
        if lhs.lastUsedAt != rhs.lastUsedAt {
            return lhs.lastUsedAt > rhs.lastUsedAt
        }
        if lhs.lastModified != rhs.lastModified {
            return lhs.lastModified > rhs.lastModified
        }
        let titleComparison = lhs.title.localizedCaseInsensitiveCompare(rhs.title)
        if titleComparison != .orderedSame {
            return titleComparison == .orderedAscending
        }
        return lhs.id < rhs.id
    }
}
