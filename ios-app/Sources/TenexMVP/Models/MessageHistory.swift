import Foundation

/// Session-only ring buffer of sent message texts for up-arrow recall in the composer.
/// Not persisted — sent messages already exist in Nostr DB.
@Observable
@MainActor
final class MessageHistory {
    private(set) var messages: [String] = []
    private(set) var currentIndex: Int?
    private(set) var savedDraft: String = ""

    /// Record a sent message. Called after successful send.
    func add(_ text: String) {
        let trimmed = text.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }
        messages.append(trimmed)
    }

    /// Move backward through history. On first call, stashes currentText.
    /// Returns the previous message text, or nil if history is empty.
    func previous(currentText: String) -> String? {
        guard !messages.isEmpty else { return nil }

        if currentIndex == nil {
            // Entering history mode — stash whatever user was typing
            savedDraft = currentText
            currentIndex = messages.count - 1
        } else if let idx = currentIndex, idx > 0 {
            currentIndex = idx - 1
        } else {
            // Already at oldest message
            return messages[0]
        }

        return messages[currentIndex!]
    }

    /// Move forward through history.
    /// Returns the next message text, or savedDraft when past the newest.
    func next() -> String? {
        guard let idx = currentIndex else { return nil }

        if idx < messages.count - 1 {
            currentIndex = idx + 1
            return messages[currentIndex!]
        } else {
            // Past newest — restore saved draft and exit history mode
            let draft = savedDraft
            reset()
            return draft
        }
    }

    /// Exit history browsing mode.
    func reset() {
        currentIndex = nil
        savedDraft = ""
    }
}
