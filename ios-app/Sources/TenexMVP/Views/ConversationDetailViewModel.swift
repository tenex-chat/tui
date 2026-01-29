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

    /// Loading state
    @Published private(set) var isLoading = false

    /// Error state
    @Published private(set) var error: Error?

    // MARK: - Cached Derived State

    /// Cached delegations extracted from messages
    @Published private(set) var delegations: [DelegationItem] = []

    /// Cached latest reply (most recent non-tool-call message)
    @Published private(set) var latestReply: MessageInfo?

    /// Cached participating agents
    @Published private(set) var participatingAgents: [String] = []

    // MARK: - Refreshable Metadata

    /// Current status (refreshed periodically)
    @Published private(set) var currentStatus: String

    /// Current isActive state (refreshed periodically)
    @Published private(set) var currentIsActive: Bool

    /// Current activity description (refreshed periodically)
    @Published private(set) var currentActivity: String?

    /// Current effectiveLastActivity (refreshed periodically)
    @Published private(set) var currentEffectiveLastActivity: UInt64

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

        // Fetch fresh conversation data
        let filter = ConversationFilter(
            projectIds: [conversation.projectATag],
            showArchived: false,
            hideScheduled: true,
            timeFilter: .all
        )

        let freshConversation: ConversationFullInfo? = await Task {
            let allConversations = (try? coreManager.core.getAllConversations(filter: filter)) ?? []
            return allConversations.first { $0.id == conversation.id }
        }.value

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
            let (fetchedMessages, children) = try await loadDataFromCore(
                coreManager: coreManager,
                conversationId: conversation.id,
                projectATag: conversation.projectATag
            )

            // Update state on main actor
            self.messages = fetchedMessages
            self.childConversations = children

            // Recompute cached derived state
            recomputeDerivedState()

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
    private func loadDataFromCore(coreManager: TenexCoreManager, conversationId: String, projectATag: String) async throws -> ([MessageInfo], [ConversationFullInfo]) {
        try Task.checkCancellation()

        // Use structured Task {} instead of Task.detached to honor parent cancellation
        async let messagesTask: [MessageInfo] = Task {
            coreManager.core.getMessages(conversationId: conversationId)
        }.value

        let filter = ConversationFilter(
            projectIds: [projectATag],
            showArchived: false,
            hideScheduled: true,
            timeFilter: .all
        )

        async let childrenTask: [ConversationFullInfo] = Task {
            let allConversations = (try? coreManager.core.getAllConversations(filter: filter)) ?? []
            return allConversations.filter { $0.parentId == conversationId }
        }.value

        // Await both tasks - cancellation will propagate to both
        let fetchedMessages = await messagesTask
        try Task.checkCancellation()
        let children = await childrenTask

        return (fetchedMessages, children)
    }

    // MARK: - Derived State Computation

    /// Recomputes all cached derived state when messages/children change
    private func recomputeDerivedState() {
        // Compute participating agents
        var agents = Set<String>()
        agents.insert(conversation.author)
        for msg in messages {
            agents.insert(msg.author)
        }
        participatingAgents = agents.sorted()

        // Compute latest reply (last non-tool-call, non-empty message)
        latestReply = messages.last { !$0.isToolCall && !$0.content.isEmpty }

        // Compute delegations
        delegations = extractDelegations()
    }

    /// Extracts delegation items from messages and child conversations
    private func extractDelegations() -> [DelegationItem] {
        var result: [DelegationItem] = []

        for message in messages {
            // Check for delegate tool calls
            if message.toolName == "mcp__tenex__delegate" || message.toolName == "delegate" {
                // qTags contain the conversation IDs of delegated conversations
                for qTag in message.qTags {
                    // Find the child conversation matching this qTag
                    if let childConv = childConversations.first(where: { $0.id == qTag }) {
                        let delegation = DelegationItem(
                            id: qTag,
                            recipient: childConv.author,
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
                let delegation = DelegationItem(
                    id: child.id,
                    recipient: child.author,
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

    /// Computes the effective runtime for the conversation.
    /// Returns the duration between first and last activity, with safe underflow handling.
    /// Uses min(createdAt) instead of assuming messages are sorted.
    func computeEffectiveRuntime(currentTime: Date) -> TimeInterval {
        // Get the earliest activity timestamp using min() - don't assume sorted
        let firstActivity: UInt64
        if let earliestMessage = messages.min(by: { $0.createdAt < $1.createdAt }) {
            firstActivity = earliestMessage.createdAt
        } else {
            // No messages, use lastActivity as both first and last
            firstActivity = conversation.lastActivity
        }

        // Use refreshable metadata instead of stale conversation data
        let lastActivity = currentEffectiveLastActivity

        // If active, use current time as the end point
        let endTimestamp: UInt64
        if currentIsActive {
            endTimestamp = UInt64(currentTime.timeIntervalSince1970)
        } else {
            endTimestamp = lastActivity
        }

        // Safe underflow handling for out-of-order timestamps
        let totalSeconds: UInt64
        if endTimestamp >= firstActivity {
            totalSeconds = endTimestamp - firstActivity
        } else {
            // Out-of-order timestamps, return 0 to avoid underflow
            totalSeconds = 0
        }

        return TimeInterval(totalSeconds)
    }

    /// Formats effective runtime as a human-readable string
    func formatEffectiveRuntime(currentTime: Date) -> String {
        let totalSeconds = computeEffectiveRuntime(currentTime: currentTime)
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
    /// Formats seconds as a human-readable duration string
    static func format(seconds: TimeInterval) -> String {
        let totalSeconds = Int(seconds)
        let hours = totalSeconds / 3600
        let minutes = (totalSeconds % 3600) / 60
        let secs = totalSeconds % 60

        if hours > 0 {
            return String(format: "%dh %02dm", hours, minutes)
        } else if minutes > 0 {
            return String(format: "%dm %02ds", minutes, secs)
        } else {
            return String(format: "%ds", secs)
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
