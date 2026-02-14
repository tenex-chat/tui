import SwiftUI
import Combine

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

    /// Aggregated todo stats (includes current + all descendants)
    @Published private(set) var aggregatedTodoStats: AggregateTodoStats = .empty

    /// Messages for descendant conversations (for todo parsing)
    private var descendantMessages: [String: [MessageInfo]] = [:]

    /// Cached parsed todo states per conversation (avoid re-parsing)
    private var parsedTodoStates: [String: TodoState] = [:]

    /// Children lookup map for efficient subtree traversal
    private var childrenByParentId: [String: [String]] = [:]

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

    private var subscriptions = Set<AnyCancellable>()

    // MARK: - Initialization

    /// Initialize with conversation only - coreManager is set later via setCoreManager
    init(conversation: ConversationFullInfo) {
        self.conversation = conversation
        // Initialize refreshable metadata from conversation
        self.currentStatus = conversation.status ?? "unknown"
        self.currentIsActive = conversation.isActive
        self.currentActivity = conversation.currentActivity
        self.currentEffectiveLastActivity = conversation.effectiveLastActivity

        // Initialize author info immediately from conversation data
        // This allows the header to render instantly without waiting for loadData()
        self.authorInfo = AgentAvatarInfo(
            name: conversation.author,
            pubkey: conversation.authorPubkey
        )
    }

    deinit {
        subscriptions.removeAll()
    }

    /// Sets the core manager after initialization (called from view's onAppear/task)
    func setCoreManager(_ coreManager: TenexCoreManager) {
        guard self.coreManager == nil else { return }
        self.coreManager = coreManager
        bindToCoreManager()
    }
    private func bindToCoreManager() {
        guard let coreManager = coreManager else { return }

        coreManager.$messagesByConversation
            .receive(on: RunLoop.main)
            .sink { [weak self] cache in
                guard let self = self else { return }
                if let updated = cache[self.conversation.id] {
                    self.applyMessages(updated)
                }
                self.applyDescendantMessages(from: cache)
            }
            .store(in: &subscriptions)

        coreManager.$conversations
            .receive(on: RunLoop.main)
            .sink { [weak self] conversations in
                self?.applyConversationUpdates(conversations)
            }
            .store(in: &subscriptions)

        applyConversationUpdates(coreManager.conversations)
        if let cached = coreManager.messagesByConversation[conversation.id] {
            applyMessages(cached)
        }
    }

    // MARK: - Data Loading (Async/Await)

    /// Loads conversation data asynchronously with proper cancellation support.
    /// IMPORTANT: Prioritizes showing the latest reply quickly by loading messages first,
    /// then loading children/descendants in the background.
    func loadData() async {
        guard !isLoading, let coreManager = coreManager else { return }

        isLoading = true
        error = nil

        do {
            // PHASE 1: Load messages first to show latest reply immediately
            await coreManager.ensureMessagesLoaded(conversationId: conversation.id)
            let fetchedMessages = coreManager.messagesByConversation[conversation.id] ?? []
            try Task.checkCancellation()

            // Update messages and compute latest reply immediately
            applyMessages(fetchedMessages)

            // Start runtime loading (non-blocking)
            Task {
                formattedRuntime = await formatEffectiveRuntime()
            }

            // PHASE 2: Load children/descendants for delegations and aggregated stats
            let (directChildren, descendants, descendantMsgs) = try await loadChildrenFromCore(
                coreManager: coreManager,
                conversationId: conversation.id
            )

            // Update state
            self.childConversations = directChildren
            self.allDescendants = descendants
            self.descendantMessages = descendantMsgs

            // Recompute remaining derived state (delegations, participants, aggregated todos)
            await recomputeDerivedState()

            isLoading = false
        } catch is CancellationError {
            isLoading = false
        } catch {
            self.error = error
            isLoading = false
        }
    }

    /// Loads children and descendants - separated from message loading for faster initial render
    private func loadChildrenFromCore(coreManager: TenexCoreManager, conversationId: String) async throws -> ([ConversationFullInfo], [ConversationFullInfo], [String: [MessageInfo]]) {
        try Task.checkCancellation()

        // Get all descendants
        let descendantIds = await coreManager.safeCore.getDescendantConversationIds(conversationId: conversationId)
        let allDescendants = await coreManager.safeCore.getConversationsByIds(conversationIds: descendantIds)

        // Get direct children from descendants
        let directChildren = allDescendants.filter { $0.parentId == conversationId }

        // Fetch messages for all descendants CONCURRENTLY
        let descendantMsgs = await withTaskGroup(
            of: (String, [MessageInfo]).self,
            returning: [String: [MessageInfo]].self
        ) { group in
            for descendant in allDescendants {
                group.addTask {
                    let msgs = await coreManager.safeCore.getMessages(conversationId: descendant.id)
                    return (descendant.id, msgs)
                }
            }

            var results: [String: [MessageInfo]] = [:]
            for await (id, msgs) in group {
                results[id] = msgs
            }
            return results
        }

        return (directChildren, allDescendants, descendantMsgs)
    }

    // MARK: - Reactive Updates

    private func applyMessages(_ fetchedMessages: [MessageInfo]) {
        messages = fetchedMessages
        latestReply = fetchedMessages.last { !$0.isToolCall && !$0.content.isEmpty }
        todoState = TodoParser.parse(messages: fetchedMessages)
        Task { await recomputeDerivedState() }
    }

    private func applyConversationUpdates(_ conversations: [ConversationFullInfo]) {
        if let updated = conversations.first(where: { $0.id == conversation.id }) {
            currentStatus = updated.status ?? "unknown"
            currentIsActive = updated.isActive
            currentActivity = updated.currentActivity
            currentEffectiveLastActivity = updated.effectiveLastActivity
        }

        refreshDescendants(from: conversations)
    }

    private func refreshDescendants(from conversations: [ConversationFullInfo]) {
        var childrenMap: [String: [ConversationFullInfo]] = [:]
        for conv in conversations {
            if let parentId = conv.parentId {
                childrenMap[parentId, default: []].append(conv)
            }
        }

        childrenByParentId = childrenMap.mapValues { $0.map { $0.id } }

        let descendantIds = collectDescendantIds(startId: conversation.id, childrenMap: childrenMap)
        let descendants = conversations.filter { descendantIds.contains($0.id) }
        allDescendants = descendants
        childConversations = descendants
            .filter { $0.parentId == conversation.id }
            .sorted { $0.effectiveLastActivity > $1.effectiveLastActivity }

        loadMissingDescendantMessages(from: descendants)
    }

    private func collectDescendantIds(startId: String, childrenMap: [String: [ConversationFullInfo]]) -> [String] {
        var result: [String] = []
        var stack: [String] = [startId]
        var visited = Set<String>()

        while let current = stack.popLast() {
            guard let children = childrenMap[current] else { continue }
            for child in children {
                if visited.insert(child.id).inserted {
                    result.append(child.id)
                    stack.append(child.id)
                }
            }
        }

        return result
    }

    private func loadMissingDescendantMessages(from descendants: [ConversationFullInfo]) {
        guard let coreManager = coreManager else { return }

        let missing = descendants.filter { descendantMessages[$0.id] == nil }
        guard !missing.isEmpty else { return }

        Task {
            let fetched = await withTaskGroup(
                of: (String, [MessageInfo]).self,
                returning: [String: [MessageInfo]].self
            ) { group in
                for descendant in missing {
                    group.addTask {
                        let msgs = await coreManager.safeCore.getMessages(conversationId: descendant.id)
                        return (descendant.id, msgs)
                    }
                }

                var results: [String: [MessageInfo]] = [:]
                for await (id, msgs) in group {
                    results[id] = msgs
                }
                return results
            }

            await MainActor.run { [weak self] in
                guard let self = self else { return }
                for (id, msgs) in fetched {
                    self.descendantMessages[id] = msgs
                }
                Task { await self.recomputeDerivedState() }
            }
        }
    }

    private func applyDescendantMessages(from cache: [String: [MessageInfo]]) {
        guard !allDescendants.isEmpty else { return }
        var updated = false
        for descendant in allDescendants {
            if let msgs = cache[descendant.id] {
                descendantMessages[descendant.id] = msgs
                updated = true
            }
        }
        if updated {
            Task { await recomputeDerivedState() }
        }
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
            let name = await coreManager.safeCore.getProfileName(pubkey: pTaggedPubkey)
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

        // Build children lookup map for efficient subtree traversal
        childrenByParentId = [:]
        for descendant in allDescendants {
            if let parentId = descendant.parentId {
                childrenByParentId[parentId, default: []].append(descendant.id)
            }
        }

        // Parse and cache todos for all conversations (parse once, use many times)
        parsedTodoStates = [:]
        for (convId, msgs) in descendantMessages {
            parsedTodoStates[convId] = TodoParser.parse(messages: msgs)
        }

        // Parse todos from current conversation's messages
        todoState = TodoParser.parse(messages: messages)

        // Compute aggregated todo stats (current + all descendants)
        var stats = AggregateTodoStats.empty
        stats.add(todoState)
        for (_, todos) in parsedTodoStates {
            stats.add(todos)
        }
        aggregatedTodoStats = stats

        // Compute delegations with their subtree todo stats
        delegations = await extractDelegations()
    }

    /// Computes todo stats for a conversation and all its descendants using cached data
    private func computeSubtreeTodoStats(forConversationId conversationId: String) -> AggregateTodoStats {
        var stats = AggregateTodoStats.empty

        // Add todos from this conversation (from cache)
        if let todos = parsedTodoStates[conversationId] {
            stats.add(todos)
        }

        // Recursively add todos from all children
        collectSubtreeTodos(parentId: conversationId, into: &stats)

        return stats
    }

    /// Recursively collects todos from all descendants of a parent
    private func collectSubtreeTodos(parentId: String, into stats: inout AggregateTodoStats) {
        guard let childIds = childrenByParentId[parentId] else { return }

        for childId in childIds {
            if let todos = parsedTodoStates[childId] {
                stats.add(todos)
            }
            // Recurse into children
            collectSubtreeTodos(parentId: childId, into: &stats)
        }
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

                        // Compute subtree todo stats for this delegation
                        let todoStats = computeSubtreeTodoStats(forConversationId: qTag)

                        var delegation = DelegationItem(
                            id: qTag,
                            recipient: recipient.isEmpty ? childConv.author : recipient,
                            recipientPubkey: recipientPubkey,
                            messagePreview: childConv.title,
                            conversationId: qTag,
                            timestamp: message.createdAt
                        )
                        delegation.todoStats = todoStats.hasTodos ? todoStats : nil
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

                // Compute subtree todo stats for this delegation
                let todoStats = computeSubtreeTodoStats(forConversationId: child.id)

                var delegation = DelegationItem(
                    id: child.id,
                    recipient: recipient.isEmpty ? child.author : recipient,
                    recipientPubkey: recipientPubkey,
                    messagePreview: child.summary ?? child.title,
                    conversationId: child.id,
                    timestamp: child.lastActivity
                )
                delegation.todoStats = todoStats.hasTodos ? todoStats : nil
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

// MARK: - Last Agent Finder

/// Utility for finding the last agent that spoke in a conversation.
/// Used by reply buttons to pre-select the agent to reply to.
enum LastAgentFinder {
    /// Finds the last agent (non-user) that spoke in the conversation.
    /// Only considers agents that are currently available (online).
    ///
    /// - Parameters:
    ///   - messages: The messages in the conversation
    ///   - availableAgents: The currently available/online agents for the project
    ///   - npubToHex: A function to convert npub (bech32) to hex pubkey
    /// - Returns: The hex pubkey of the last agent that spoke, or nil if none found
    static func findLastAgentPubkey(
        messages: [MessageInfo],
        availableAgents: [OnlineAgentInfo],
        npubToHex: (String) -> String?
    ) -> String? {
        // Get set of agent pubkeys (hex format) for quick lookup
        let agentPubkeys = Set(availableAgents.map { $0.pubkey })

        // Find the most recent message from an agent (not the user)
        var latestAgentHexPubkey: String?
        var latestTimestamp: UInt64 = 0

        for msg in messages {
            // Skip user messages
            if msg.role == "user" {
                continue
            }

            // Convert authorNpub (bech32 format) to hex for comparison with agent pubkeys
            guard let hexPubkey = npubToHex(msg.authorNpub) else {
                continue
            }

            // Check if this message is from a known agent
            if agentPubkeys.contains(hexPubkey) && msg.createdAt >= latestTimestamp {
                latestTimestamp = msg.createdAt
                latestAgentHexPubkey = hexPubkey
            }
        }

        return latestAgentHexPubkey
    }
}
