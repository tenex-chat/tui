import Foundation

/// An uploaded image attachment with its Blossom URL
struct ImageAttachment: Codable, Identifiable, Equatable {
    /// Unique identifier for the attachment
    let id: Int
    /// Blossom URL where the image is stored
    let url: String
}

/// A draft message for composition.
/// Can be for a new conversation (thread) or an existing conversation.
struct Draft: Codable, Identifiable, Equatable {
    // MARK: - Codable Keys

    enum CodingKeys: String, CodingKey {
        case id
        case conversationId
        case projectId
        case title
        case content
        case agentPubkey
        case selectedNudgeIds
        case selectedSkillIds
        case isNewConversation
        case lastEdited
        case referenceConversationId
        case referenceReportATag
        case imageAttachments
    }
    /// Unique identifier for the draft
    var id: String

    /// The conversation ID if adding to existing conversation, nil for new thread
    var conversationId: String?

    /// The project ID this draft belongs to
    var projectId: String

    /// Title of the conversation (kept for compatibility but auto-generated)
    var title: String

    /// The message content being composed
    var content: String

    /// Pubkey of agent to p-tag in the message (single-select)
    var agentPubkey: String?

    /// Selected nudge IDs for this conversation (multi-select)
    var selectedNudgeIds: Set<String>

    /// Selected skill IDs for this conversation (multi-select)
    var selectedSkillIds: Set<String>

    /// Whether this is for a new conversation (thread)
    var isNewConversation: Bool

    /// Timestamp of last edit
    var lastEdited: Date

    /// Reference conversation ID for context tagging (used by "Reference this conversation" feature)
    /// When set, adds a ["context", "<conversation-id>"] tag to the sent event
    var referenceConversationId: String?

    /// Reference report a-tag for context tagging (used by "Chat with Author" feature)
    /// Format: "30023:<pubkey>:<slug>" - the standard Nostr a-tag for addressable events
    /// When set, adds a ["context", "<a-tag>"] tag to the sent event
    var referenceReportATag: String?

    /// Uploaded image attachments with their Blossom URLs
    var imageAttachments: [ImageAttachment]

    /// Next image attachment ID (for generating unique IDs)
    private var nextImageId: Int = 1

    // MARK: - Initialization

    /// Create a new draft for a new conversation
    init(projectId: String, title: String = "", content: String = "", agentPubkey: String? = nil, selectedNudgeIds: Set<String> = [], selectedSkillIds: Set<String> = [], referenceConversationId: String? = nil, referenceReportATag: String? = nil) {
        self.id = UUID().uuidString
        self.conversationId = nil
        self.projectId = projectId
        self.title = title
        self.content = content
        self.agentPubkey = agentPubkey
        self.selectedNudgeIds = selectedNudgeIds
        self.selectedSkillIds = selectedSkillIds
        self.isNewConversation = true
        self.lastEdited = Date()
        self.referenceConversationId = referenceConversationId
        self.referenceReportATag = referenceReportATag
        self.imageAttachments = []
    }

    /// Create a new draft for an existing conversation
    init(conversationId: String, projectId: String, content: String = "", agentPubkey: String? = nil, selectedNudgeIds: Set<String> = [], selectedSkillIds: Set<String> = [], referenceConversationId: String? = nil, referenceReportATag: String? = nil) {
        self.id = UUID().uuidString
        self.conversationId = conversationId
        self.projectId = projectId
        self.title = "" // Not used for existing conversations
        self.content = content
        self.agentPubkey = agentPubkey
        self.selectedNudgeIds = selectedNudgeIds
        self.selectedSkillIds = selectedSkillIds
        self.isNewConversation = false
        self.lastEdited = Date()
        self.referenceConversationId = referenceConversationId
        self.referenceReportATag = referenceReportATag
        self.imageAttachments = []
    }

    // MARK: - Migration Support

    /// Custom decoder for backward compatibility
    /// Handles drafts from before projectId, selectedNudgeIds, referenceConversationId, referenceReportATag, and imageAttachments were added
    init(from decoder: Decoder) throws {
        let container = try decoder.container(keyedBy: CodingKeys.self)

        self.id = try container.decode(String.self, forKey: .id)
        self.conversationId = try container.decodeIfPresent(String.self, forKey: .conversationId)

        // Migration: projectId is now required, but old drafts didn't have it
        // Use empty string as placeholder - these drafts will be orphaned but won't crash
        self.projectId = try container.decodeIfPresent(String.self, forKey: .projectId) ?? ""

        self.title = try container.decode(String.self, forKey: .title)
        self.content = try container.decode(String.self, forKey: .content)
        self.agentPubkey = try container.decodeIfPresent(String.self, forKey: .agentPubkey)
        // Migration: selectedNudgeIds is new, default to empty set
        self.selectedNudgeIds = try container.decodeIfPresent(Set<String>.self, forKey: .selectedNudgeIds) ?? []
        // Migration: selectedSkillIds is new, default to empty set
        self.selectedSkillIds = try container.decodeIfPresent(Set<String>.self, forKey: .selectedSkillIds) ?? []
        self.isNewConversation = try container.decode(Bool.self, forKey: .isNewConversation)
        self.lastEdited = try container.decode(Date.self, forKey: .lastEdited)
        // Migration: referenceConversationId is new, default to nil
        self.referenceConversationId = try container.decodeIfPresent(String.self, forKey: .referenceConversationId)
        // Migration: referenceReportATag is new, default to nil
        self.referenceReportATag = try container.decodeIfPresent(String.self, forKey: .referenceReportATag)
        // Migration: imageAttachments is new, default to empty array
        self.imageAttachments = try container.decodeIfPresent([ImageAttachment].self, forKey: .imageAttachments) ?? []

        // Restore nextImageId from existing attachments
        if let maxId = self.imageAttachments.map(\.id).max() {
            self.nextImageId = maxId + 1
        }
    }

    // MARK: - Computed Properties

    /// Whether the draft has meaningful content
    var hasContent: Bool {
        // Check content OR images - either is valid
        !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !imageAttachments.isEmpty
    }

    /// Whether the draft is valid for sending
    var isValid: Bool {
        // Both new conversations and replies need content OR images
        !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty || !imageAttachments.isEmpty
    }

    /// Whether the draft has any image attachments
    var hasImages: Bool {
        !imageAttachments.isEmpty
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

    /// Update the project ID (useful when switching projects in composer)
    mutating func updateProjectId(_ newProjectId: String) {
        projectId = newProjectId
        lastEdited = Date()
    }

    /// Clear all content from the draft
    mutating func clear() {
        title = ""
        content = ""
        agentPubkey = nil
        selectedNudgeIds = []
        selectedSkillIds = []
        referenceConversationId = nil
        referenceReportATag = nil
        imageAttachments = []
        nextImageId = 1
        lastEdited = Date()
    }

    /// Add a nudge to the selection
    mutating func addNudge(_ nudgeId: String) {
        selectedNudgeIds.insert(nudgeId)
        lastEdited = Date()
    }

    /// Remove a nudge from the selection
    mutating func removeNudge(_ nudgeId: String) {
        selectedNudgeIds.remove(nudgeId)
        lastEdited = Date()
    }

    /// Clear all selected nudges
    mutating func clearNudges() {
        selectedNudgeIds = []
        lastEdited = Date()
    }

    /// Add a skill to the selection
    mutating func addSkill(_ skillId: String) {
        selectedSkillIds.insert(skillId)
        lastEdited = Date()
    }

    /// Remove a skill from the selection
    mutating func removeSkill(_ skillId: String) {
        selectedSkillIds.remove(skillId)
        lastEdited = Date()
    }

    /// Clear all selected skills
    mutating func clearSkills() {
        selectedSkillIds = []
        lastEdited = Date()
    }

    /// Set the reference conversation ID for context tagging
    mutating func setReferenceConversation(_ conversationId: String?) {
        referenceConversationId = conversationId
        lastEdited = Date()
    }

    /// Clear the reference conversation
    mutating func clearReferenceConversation() {
        referenceConversationId = nil
        lastEdited = Date()
    }

    /// Set the reference report a-tag for context tagging (used by "Chat with Author" feature)
    mutating func setReferenceReportATag(_ aTag: String?) {
        referenceReportATag = aTag
        lastEdited = Date()
    }

    /// Clear the reference report a-tag
    mutating func clearReferenceReportATag() {
        referenceReportATag = nil
        lastEdited = Date()
    }

    // MARK: - Image Attachments

    /// Add an image attachment and return its ID
    mutating func addImageAttachment(url: String) -> Int {
        let id = nextImageId
        nextImageId += 1
        imageAttachments.append(ImageAttachment(id: id, url: url))
        lastEdited = Date()
        return id
    }

    /// Remove an image attachment by ID
    mutating func removeImageAttachment(id: Int) {
        imageAttachments.removeAll { $0.id == id }
        lastEdited = Date()
    }

    /// Clear all image attachments
    mutating func clearImageAttachments() {
        imageAttachments = []
        nextImageId = 1
        lastEdited = Date()
    }

    /// Build the full message content including image URLs
    /// Replaces [Image #N] markers with actual URLs (matching TUI behavior)
    func buildFullContent() -> String {
        var fullContent = content

        // Replace [Image #N] markers with actual URLs
        for attachment in imageAttachments {
            let marker = "[Image #\(attachment.id)]"
            fullContent = fullContent.replacingOccurrences(of: marker, with: attachment.url)
        }

        return fullContent
    }
}

// MARK: - Draft Key

extension Draft {
    /// Create a unique key for storing drafts
    /// For new conversations: "new-{projectId}"
    /// For existing conversations: "reply-{projectId}-{conversationId}"
    ///
    /// Note: Conversation IDs are Nostr event IDs (SHA-256 hashes) which are globally unique
    /// across the entire Nostr network. However, we include projectId in the reply key for
    /// defense-in-depth and to support potential future scenarios where the same conversation
    /// might be accessed from different projects.
    static func storageKey(for conversationId: String?, projectId: String) -> String {
        if let conversationId = conversationId {
            // Include projectId to avoid collisions if conversation IDs are not globally unique
            // or if the same conversation is accessed from multiple projects
            return "reply-\(projectId)-\(conversationId)"
        } else {
            return "new-\(projectId)"
        }
    }

    var storageKey: String {
        Draft.storageKey(for: conversationId, projectId: projectId)
    }
}
