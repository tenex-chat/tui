import Foundation

/// A named, reusable draft that users explicitly save for later use.
/// Unlike auto-saved chat drafts (Draft), named drafts are user-initiated:
/// save, browse, restore, delete. They persist across sessions.
struct NamedDraft: Codable, Identifiable, Equatable {
    let id: String
    var name: String
    var text: String
    let projectId: String
    let createdAt: Date
    var lastModified: Date

    /// First 100 chars of text with newlines replaced by spaces
    var preview: String {
        let cleaned = text.replacingOccurrences(of: "\n", with: " ")
        if cleaned.count <= 100 { return cleaned }
        return String(cleaned.prefix(100)) + "..."
    }

    init(text: String, projectId: String) {
        self.id = UUID().uuidString
        self.name = Self.deriveName(from: text)
        self.text = text
        self.projectId = projectId
        self.createdAt = Date()
        self.lastModified = Date()
    }

    mutating func updateText(_ newText: String) {
        text = newText
        name = Self.deriveName(from: newText)
        lastModified = Date()
    }

    /// Derives a name from the first line of text, trimmed, capped at 50 chars.
    /// Falls back to "Untitled" if empty.
    static func deriveName(from text: String) -> String {
        let firstLine = text.components(separatedBy: .newlines).first ?? ""
        let trimmed = firstLine.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return "Untitled" }
        if trimmed.count <= 50 { return trimmed }
        return String(trimmed.prefix(50)) + "..."
    }
}
