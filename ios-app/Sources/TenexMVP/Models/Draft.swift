import Foundation

/// A draft message for composition.
/// Can be for a new conversation (thread) or an existing conversation.
struct Draft: Codable, Identifiable, Equatable {
    /// Unique identifier for the draft
    var id: String

    /// The conversation ID if adding to existing conversation, nil for new thread
    var conversationId: String?

    /// The project ID this draft belongs to
    var projectId: String

    /// Title of the conversation (required for new threads)
    var title: String

    /// The message content being composed
    var content: String

    /// Pubkey of agent to p-tag in the message (single-select)
    var agentPubkey: String?

    /// Whether this is for a new conversation (thread)
    var isNewConversation: Bool

    /// Timestamp of last edit
    var lastEdited: Date

    // MARK: - Initialization

    /// Create a new draft for a new conversation
    init(projectId: String, title: String = "", content: String = "", agentPubkey: String? = nil) {
        self.id = UUID().uuidString
        self.conversationId = nil
        self.projectId = projectId
        self.title = title
        self.content = content
        self.agentPubkey = agentPubkey
        self.isNewConversation = true
        self.lastEdited = Date()
    }

    /// Create a new draft for an existing conversation
    init(conversationId: String, projectId: String, content: String = "", agentPubkey: String? = nil) {
        self.id = UUID().uuidString
        self.conversationId = conversationId
        self.projectId = projectId
        self.title = "" // Not used for existing conversations
        self.content = content
        self.agentPubkey = agentPubkey
        self.isNewConversation = false
        self.lastEdited = Date()
    }

    // MARK: - Computed Properties

    /// Whether the draft has meaningful content
    var hasContent: Bool {
        if isNewConversation {
            return !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ||
                   !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        } else {
            return !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        }
    }

    /// Whether the draft is valid for sending
    var isValid: Bool {
        if isNewConversation {
            // New conversation needs at least a title
            return !title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        } else {
            // Reply needs content
            return !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
        }
    }

    // MARK: - Mutation

    /// Update the draft content and timestamp
    mutating func updateContent(_ newContent: String) {
        content = newContent
        lastEdited = Date()
    }

    /// Update the draft title and timestamp
    mutating func updateTitle(_ newTitle: String) {
        title = newTitle
        lastEdited = Date()
    }

    /// Set the agent pubkey for the draft (single-select)
    mutating func setAgent(_ pubkey: String?) {
        agentPubkey = pubkey
        lastEdited = Date()
    }

    /// Clear the selected agent
    mutating func clearAgent() {
        agentPubkey = nil
        lastEdited = Date()
    }

    /// Clear all content from the draft
    mutating func clear() {
        title = ""
        content = ""
        agentPubkey = nil
        lastEdited = Date()
    }
}

// MARK: - Draft Key

extension Draft {
    /// Create a unique key for storing drafts
    /// For new conversations: "new-{projectId}"
    /// For existing conversations: "reply-{conversationId}"
    static func storageKey(for conversationId: String?, projectId: String) -> String {
        if let conversationId = conversationId {
            return "reply-\(conversationId)"
        } else {
            return "new-\(projectId)"
        }
    }

    var storageKey: String {
        Draft.storageKey(for: conversationId, projectId: projectId)
    }
}
