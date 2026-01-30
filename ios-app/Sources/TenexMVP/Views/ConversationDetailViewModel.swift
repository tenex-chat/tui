import SwiftUI

// MARK: - Conversation Detail ViewModel

/// ViewModel for ConversationDetailView that handles data loading, caching derived state,
/// and managing live runtime updates efficiently.
@MainActor
final class ConversationDetailViewModel: ObservableObject {
    // MARK: - Published State

    /// Raw messages for the conversation
    @Published private(set) var messages: [MessageInfo] = []

    /// Child conversations (delegations)
    @Published private(set) var childConversations: [ConversationFullInfo] = []

    /// All descendant conversations (for participant extraction)
    private var allDescendants: [ConversationFullInfo] = []

    /// Loading state
    @Published private(set) var isLoading = false

    /// Error state
    @Published private(set) var error: Error?

    // MARK: - Cached Derived State

    /// Cached delegations extracted from messages
    @Published private(set) var delegations: [DelegationItem] = []

    /// Cached latest reply (most recent non-tool-call message)
    @Published private(set) var latestReply: MessageInfo?

    /// Cached participating agent infos with pubkeys for avatar lookups
    @Published private(set) var participatingAgentInfos: [AgentAvatarInfo] = []

    /// Author info (for avatar group)
    @Published private(set) var authorInfo: AgentAvatarInfo!

    /// P-tagged recipient info (first p-tag from conversation, shown overlapping with author)
    @Published private(set) var pTaggedRecipientInfo: AgentAvatarInfo?

    /// Other participants excluding author (for avatar group overlapping display)
    @Published private(set) var otherParticipantsInfo: [AgentAvatarInfo] = []

    /// Todo list state
    @Published private(set) var todoState: TodoState = TodoState(items: [])

    // MARK: - Refreshable Metadata

    /// Current status (refreshed periodically)
    @Published private(set) var currentStatus: String

    /// Current isActive state (refreshed periodically)
    @Published private(set) var currentIsActive: Bool

    /// Current activity description (refreshed periodically)
    @Published private(set) var currentActivity: String?

    /// Current effectiveLastActivity (refreshed periodically)
    @Published private(set) var currentEffectiveLastActivity: UInt64

    /// Formatted runtime string (computed async and cached)
    @Published private(set) var formattedRuntime: String = ""

    // MARK: - Dependencies

    private let conversation: ConversationFullInfo
    private weak var coreManager: TenexCoreManager?

    /// Timer for periodic metadata refresh
    private var metadataRefreshTask: Task<Void, Never>?

    // MARK: - Initialization

    /// Initialize with conversation only - coreManager is set later via setCoreManager
    init(conversation: ConversationFullInfo) {
        self.conversation = conversation
        // Initialize refreshable metadata from conversation
        self.currentStatus = conversation.status ?? "unknown"
        self.currentIsActive = conversation.isActive
        self.currentActivity = conversation.currentActivity
        self.currentEffectiveLastActivity = conversation.effectiveLastActivity
    }

    deinit {
        metadataRefreshTask?.cancel()
    }

    /// Sets the core manager after initialization (called from view's onAppear/task)
    func setCoreManager(_ coreManager: TenexCoreManager) {
        guard self.coreManager == nil else { return }
        self.coreManager = coreManager
        startMetadataRefresh()
    }

    /// Starts periodic metadata refresh for active conversations
    private func startMetadataRefresh() {
        metadataRefreshTask?.cancel()
        metadataRefreshTask = Task { [weak self] in
            while !Task.isCancelled {
                // Refresh every 30 seconds
                try? await Task.sleep(nanoseconds: 30_000_000_000)
                guard !Task.isCancelled else { break }
                await self?.refreshMetadata()
            }
        }
    }

    /// Refreshes conversation metadata (status, isActive, activity)
    private func refreshMetadata() async {
        guard let coreManager = coreManager else { return }

        // Extract d-tag from a-tag format "kind:pubkey:d-tag" for filter
        let projectId = conversation.projectATag.split(separator: ":").dropFirst(2).joined(separator: ":")

        // Fetch fresh conversation data
        let filter = ConversationFilter(
            projectIds: [projectId],
            showArchived: false,
            hideScheduled: true,
            timeFilter: .all
        )

        let freshConversation: ConversationFullInfo? = await {
            let allConversations = (try? await coreManager.safeCore.getAllConversations(filter: filter)) ?? []
            return allConversations.first { $0.id == conversation.id }
        }()

        if let fresh = freshConversation {
            self.currentStatus = fresh.status ?? "unknown"
            self.currentIsActive = fresh.isActive
            self.currentActivity = fresh.currentActivity
            self.currentEffectiveLastActivity = fresh.effectiveLastActivity
        }
    }

    // MARK: - Data Loading (Async/Await)

    /// Loads conversation data asynchronously with proper cancellation support
    func loadData() async {
        guard !isLoading, let coreManager = coreManager else { return }

        isLoading = true
        error = nil

        do {
            // Perform work off the main actor for better performance
            let (fetchedMessages, directChildren, descendants) = try await loadDataFromCore(
                coreManager: coreManager,
                conversationId: conversation.id,
                projectATag: conversation.projectATag
            )

            // Update state on main actor
            self.messages = fetchedMessages
            self.childConversations = directChildren
            self.allDescendants = descendants

            // Recompute cached derived state
            await recomputeDerivedState()

            // Update runtime
            formattedRuntime = await formatEffectiveRuntime()

            isLoading = false
        } catch is CancellationError {
            // Task was cancelled, don't update state
            isLoading = false
        } catch {
            self.error = error
            isLoading = false
        }
    }

    /// Performs the actual data loading using structured concurrency for proper cancellation
    private func loadDataFromCore(coreManager: TenexCoreManager, conversationId: String, projectATag: String) async throws -> ([MessageInfo], [ConversationFullInfo], [ConversationFullInfo]) {
        try Task.checkCancellation()

        // Fetch messages using safeCore
        async let messagesTask: [MessageInfo] = coreManager.safeCore.getMessages(conversationId: conversationId)

        // Fetch descendants using safeCore
        async let childrenTask: ([ConversationFullInfo], [ConversationFullInfo]) = {
            // Get all descendants (for participants extraction)
            let descendantIds = await coreManager.safeCore.getDescendantConversationIds(conversationId: conversationId)
            let allDescendants = await coreManager.safeCore.getConversationsByIds(conversationIds: descendantIds)

            // Get direct children from descendants (for delegations display)
            let directChildren = allDescendants.filter { $0.parentId == conversationId }

            return (directChildren, allDescendants)
        }()

        // Await both tasks - cancellation will propagate to both
        let fetchedMessages = await messagesTask
        try Task.checkCancellation()
        let (directChildren, allDescendants) = await childrenTask

        return (fetchedMessages, directChildren, allDescendants)
    }

    // MARK: - Derived State Computation

    /// Recomputes all cached derived state when messages/children change
    private func recomputeDerivedState() async {
        // Compute participating agent infos with pubkeys for avatar lookups
        // Use pubkey as unique key to avoid duplicates from same agent
        var agentInfosByPubkey: [String: AgentAvatarInfo] = [:]

        // Store author info separately for avatar group
        authorInfo = AgentAvatarInfo(
            name: conversation.author,
            pubkey: conversation.authorPubkey
        )

        // Add conversation author to full list
        agentInfosByPubkey[conversation.authorPubkey] = authorInfo

        // Add all descendant authors
        for descendant in allDescendants {
            agentInfosByPubkey[descendant.authorPubkey] = AgentAvatarInfo(
                name: descendant.author,
                pubkey: descendant.authorPubkey
            )
        }

        // Sort by name for consistent display
        participatingAgentInfos = agentInfosByPubkey.values.sorted { $0.name < $1.name }

        // Extract p-tagged recipient info (first p-tag from conversation)
        if let pTaggedPubkey = conversation.pTags.first, let coreManager = coreManager {
            let name = coreManager.core.getProfileName(pubkey: pTaggedPubkey)
            pTaggedRecipientInfo = AgentAvatarInfo(name: name, pubkey: pTaggedPubkey)
        } else {
            pTaggedRecipientInfo = nil
        }

        // Other participants excluding author and p-tagged (for avatar group)
        let pTaggedPubkey = conversation.pTags.first
        otherParticipantsInfo = participatingAgentInfos.filter {
            $0.pubkey != conversation.authorPubkey && $0.pubkey != pTaggedPubkey
        }

        // Compute latest reply (last non-tool-call, non-empty message)
        latestReply = messages.last { !$0.isToolCall && !$0.content.isEmpty }

        // Parse todos from messages
        todoState = TodoParser.parse(messages: messages)

        // Compute delegations
        delegations = await extractDelegations()
    }

    /// Extracts delegation items from messages and child conversations
    private func extractDelegations() async -> [DelegationItem] {
        guard let coreManager = coreManager else { return [] }

        var result: [DelegationItem] = []

        for message in messages {
            // Check for delegate tool calls
            if message.toolName == "mcp__tenex__delegate" || message.toolName == "delegate" {
                // qTags contain the conversation IDs of delegated conversations
                for qTag in message.qTags {
                    // Find the child conversation matching this qTag
                    if let childConv = childConversations.first(where: { $0.id == qTag }) {
                        // Get recipient from p-tag of the child conversation (who was delegated TO)
                        let recipientPubkey = childConv.pTags.first ?? childConv.authorPubkey
                        let recipient = await coreManager.safeCore.getProfileName(pubkey: recipientPubkey)

                        let delegation = DelegationItem(
                            id: qTag,
                            recipient: recipient.isEmpty ? childConv.author : recipient,
                            recipientPubkey: recipientPubkey,
                            messagePreview: childConv.title,
                            conversationId: qTag,
                            timestamp: message.createdAt
                        )
                        result.append(delegation)
                    }
                }
            }
        }

        // Also add child conversations that might not have tool call references
        for child in childConversations {
            if !result.contains(where: { $0.conversationId == child.id }) {
                // Get recipient from child's p-tag if available
                let recipientPubkey = child.pTags.first ?? child.authorPubkey
                let recipient = await coreManager.safeCore.getProfileName(pubkey: recipientPubkey)

                let delegation = DelegationItem(
                    id: child.id,
                    recipient: recipient.isEmpty ? child.author : recipient,
                    recipientPubkey: recipientPubkey,
                    messagePreview: child.summary ?? child.title,
                    conversationId: child.id,
                    timestamp: child.lastActivity
                )
                result.append(delegation)
            }
        }

        return result.sorted { $0.timestamp > $1.timestamp }
    }

    // MARK: - Runtime Calculation

    /// Gets the hierarchical LLM runtime for this conversation (includes all descendants).
    /// Returns the total runtime in seconds by converting from milliseconds.
    func getHierarchicalRuntime() async -> TimeInterval {
        guard let coreManager = coreManager else { return 0 }

        // Get runtime in milliseconds from the FFI
        let runtimeMs = await coreManager.safeCore.getConversationRuntimeMs(conversationId: conversation.id)

        // Convert milliseconds to seconds
        return TimeInterval(runtimeMs) / 1000.0
    }

    /// Formats hierarchical runtime as a human-readable string
    func formatEffectiveRuntime() async -> String {
        let totalSeconds = await getHierarchicalRuntime()
        return RuntimeFormatter.format(seconds: totalSeconds)
    }

    // MARK: - Child Conversation Lookup

    /// Finds a child conversation by ID for delegation navigation
    func childConversation(for delegationId: String) -> ConversationFullInfo? {
        childConversations.first { $0.id == delegationId }
    }
}

// MARK: - Runtime Formatter

/// Utility for formatting runtime durations consistently
enum RuntimeFormatter {
    /// Formats seconds as a human-readable duration string (matches TUI logic)
    static func format(seconds: TimeInterval) -> String {
        if seconds >= 3600.0 {
            // Show as hours and minutes for longer runtimes
            let hours = floor(seconds / 3600.0)
            let mins = floor((seconds.truncatingRemainder(dividingBy: 3600.0)) / 60.0)
            return String(format: "%.0fh%.0fm", hours, mins)
        } else if seconds >= 60.0 {
            // Show as minutes and seconds
            let mins = floor(seconds / 60.0)
            let secs = seconds.truncatingRemainder(dividingBy: 60.0)
            return String(format: "%.0fm%.0fs", mins, secs)
        } else {
            // Show seconds with one decimal place
            return String(format: "%.1fs", seconds)
        }
    }
}

// MARK: - Agent Name Formatter

/// Utility for formatting agent names consistently across the app
enum AgentNameFormatter {
    /// Formats an agent name from kebab-case to Title Case
    /// e.g., "claude-code" -> "Claude Code"
    static func format(_ name: String) -> String {
        name.split(separator: "-")
            .map { $0.capitalized }
            .joined(separator: " ")
    }
}

// MARK: - Text Utilities

/// Utility for text truncation
enum TextUtilities {
    /// Truncates text to a maximum length with ellipsis
    static func truncate(_ text: String, maxLength: Int) -> String {
        if text.count <= maxLength { return text }
        return String(text.prefix(maxLength)) + "..."
    }
}
