import SwiftUI
import Combine

// MARK: - Conversation Detail ViewModel

/// A report reference extracted from message a-tags in a conversation.
struct ReferencedReportItem: Identifiable, Hashable {
    /// Full report coordinate: 30023:pubkey:slug
    let aTag: String
    /// Human-readable title (resolved from Report when available, else slug fallback)
    let title: String
    /// Report slug parsed from a-tag coordinate
    let slug: String
    /// Matching report object from the local report cache when available
    let report: Report?

    var id: String { aTag }
}

private struct ReportATagCoordinate {
    let kind: Int
    let pubkey: String
    let slug: String

    static func parse(_ aTag: String) -> ReportATagCoordinate? {
        let parts = aTag.split(separator: ":", omittingEmptySubsequences: false)
        guard parts.count >= 3, let kind = Int(parts[0]) else { return nil }
        let pubkey = String(parts[1])
        let slug = parts.dropFirst(2).joined(separator: ":")
        return ReportATagCoordinate(kind: kind, pubkey: pubkey, slug: slug)
    }
}

/// ViewModel for ConversationDetailView that handles data loading, caching derived state,
/// and managing live runtime updates efficiently.
@MainActor
final class ConversationDetailViewModel: ObservableObject {
    // MARK: - Published State

    /// Raw messages for the conversation
    @Published private(set) var messages: [Message] = []

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

    /// Cached activity status for each direct delegation conversation
    @Published private(set) var delegationActivityByConversationId: [String: Bool] = [:]

    /// Cached latest reply (most recent non-tool-call message)
    @Published private(set) var latestReply: Message?

    /// Reports referenced in this conversation via message a-tags (deduped by a-tag)
    @Published private(set) var referencedReports: [ReferencedReportItem] = []

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
    private var descendantMessages: [String: [Message]] = [:]

    /// Cached parsed todo states per descendant conversation.
    private var parsedTodoStates: [String: TodoState] = [:]
    /// Descendants whose message snapshots changed and need todo re-parse.
    private var dirtyDescendantTodoConversationIds: Set<String> = []
    /// Profile-name cache to avoid repeated FFI lookups during recompute.
    private var profileNameCache: [String: String] = [:]

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
    private let profiler = PerformanceProfiler.shared
    private var recomputeTask: Task<Void, Never>?
    private var recomputePending = false
    private var didCompleteInitialLoad = false

    // MARK: - Initialization

    /// Initialize with conversation only - coreManager is set later via setCoreManager
    init(conversation: ConversationFullInfo) {
        self.conversation = conversation
        // Initialize refreshable metadata from conversation
        self.currentStatus = conversation.thread.statusLabel ?? "unknown"
        self.currentIsActive = conversation.isActive
        self.currentActivity = conversation.thread.statusCurrentActivity
        self.currentEffectiveLastActivity = conversation.thread.effectiveLastActivity

        // Initialize author info immediately from conversation data
        // This allows the header to render instantly without waiting for loadData()
        self.authorInfo = AgentAvatarInfo(
            name: conversation.author,
            pubkey: conversation.thread.pubkey
        )
    }

    deinit {
        recomputeTask?.cancel()
        subscriptions.removeAll()
    }

    /// Sets the core manager after initialization (called from view's onAppear/task)
    func setCoreManager(_ coreManager: TenexCoreManager) {
        guard self.coreManager == nil else { return }
        self.coreManager = coreManager
        bindToCoreManager()
        profiler.logEvent(
            "ConversationDetailViewModel bound coreManager conversationId=\(conversation.thread.id)",
            category: .general,
            level: .debug
        )
    }
    private func bindToCoreManager() {
        guard let coreManager = coreManager else { return }

        coreManager.$messagesByConversation
            .receive(on: RunLoop.main)
            .sink { [weak self] cache in
                guard let self = self else { return }
                if let updated = cache[self.conversation.thread.id] {
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

        coreManager.$reports
            .receive(on: RunLoop.main)
            .sink { [weak self] _ in
                guard let self = self else { return }
                self.scheduleDerivedStateRecompute()
            }
            .store(in: &subscriptions)

        applyConversationUpdates(coreManager.conversations)
        if let cached = coreManager.messagesByConversation[conversation.thread.id] {
            applyMessages(cached)
        }
    }

    // MARK: - Data Loading (Async/Await)

    /// Loads conversation data asynchronously with proper cancellation support.
    /// IMPORTANT: Prioritizes showing the latest reply quickly by loading messages first,
    /// then loading children/descendants in the background.
    func loadData() async {
        guard !isLoading, let coreManager = coreManager else { return }
        let loadStartedAt = CFAbsoluteTimeGetCurrent()
        profiler.logEvent(
            "loadData start conversationId=\(conversation.thread.id)",
            category: .general
        )

        isLoading = true
        error = nil

        do {
            // PHASE 1: Load messages first to show latest reply immediately
            let phase1StartedAt = CFAbsoluteTimeGetCurrent()
            await coreManager.ensureMessagesLoaded(conversationId: conversation.thread.id)
            let fetchedMessages = coreManager.messagesByConversation[conversation.thread.id] ?? []
            try Task.checkCancellation()
            let phase1Ms = (CFAbsoluteTimeGetCurrent() - phase1StartedAt) * 1000
            profiler.logEvent(
                "loadData phase=messages conversationId=\(conversation.thread.id) count=\(fetchedMessages.count) elapsedMs=\(String(format: "%.2f", phase1Ms))",
                category: .general,
                level: phase1Ms >= 120 ? .error : .info
            )

            // Update messages and compute latest reply immediately
            applyMessages(fetchedMessages, scheduleRecompute: false)

            // Start runtime loading (non-blocking)
            Task {
                formattedRuntime = await formatEffectiveRuntime()
            }

            // PHASE 2: Load children/descendants for delegations and aggregated stats
            let phase2StartedAt = CFAbsoluteTimeGetCurrent()
            let (directChildren, descendants, descendantMsgs) = try await loadChildrenFromCore(
                coreManager: coreManager,
                conversationId: conversation.thread.id
            )
            let phase2Ms = (CFAbsoluteTimeGetCurrent() - phase2StartedAt) * 1000
            profiler.logEvent(
                "loadData phase=children conversationId=\(conversation.thread.id) directChildren=\(directChildren.count) descendants=\(descendants.count) descendantMsgMaps=\(descendantMsgs.count) elapsedMs=\(String(format: "%.2f", phase2Ms))",
                category: .general,
                level: phase2Ms >= 250 ? .error : .info
            )

            // Update state
            self.childConversations = directChildren
            self.allDescendants = descendants
            self.descendantMessages = descendantMsgs
            self.dirtyDescendantTodoConversationIds.formUnion(descendantMsgs.keys)

            // Recompute remaining derived state (delegations, participants, aggregated todos)
            let phase3StartedAt = CFAbsoluteTimeGetCurrent()
            await recomputeDerivedStateNow()
            let phase3Ms = (CFAbsoluteTimeGetCurrent() - phase3StartedAt) * 1000
            profiler.logEvent(
                "loadData phase=derived-state conversationId=\(conversation.thread.id) elapsedMs=\(String(format: "%.2f", phase3Ms))",
                category: .general,
                level: phase3Ms >= 120 ? .error : .info
            )

            didCompleteInitialLoad = true
            loadMissingDescendantMessages(from: allDescendants)
            isLoading = false
            let totalMs = (CFAbsoluteTimeGetCurrent() - loadStartedAt) * 1000
            profiler.logEvent(
                "loadData complete conversationId=\(conversation.thread.id) totalMs=\(String(format: "%.2f", totalMs)) messages=\(messages.count) children=\(childConversations.count)",
                category: .general,
                level: totalMs >= 350 ? .error : .info
            )
        } catch is CancellationError {
            isLoading = false
            let totalMs = (CFAbsoluteTimeGetCurrent() - loadStartedAt) * 1000
            profiler.logEvent(
                "loadData cancelled conversationId=\(conversation.thread.id) elapsedMs=\(String(format: "%.2f", totalMs))",
                category: .general,
                level: .info
            )
        } catch {
            self.error = error
            isLoading = false
            let totalMs = (CFAbsoluteTimeGetCurrent() - loadStartedAt) * 1000
            profiler.logEvent(
                "loadData failed conversationId=\(conversation.thread.id) elapsedMs=\(String(format: "%.2f", totalMs)) error=\(error.localizedDescription)",
                category: .general,
                level: .error
            )
        }
    }

    /// Loads children and descendants - separated from message loading for faster initial render
    private func loadChildrenFromCore(coreManager: TenexCoreManager, conversationId: String) async throws -> ([ConversationFullInfo], [ConversationFullInfo], [String: [Message]]) {
        try Task.checkCancellation()
        let startedAt = CFAbsoluteTimeGetCurrent()

        // Get all descendants
        let descendantsLookupStartedAt = CFAbsoluteTimeGetCurrent()
        let descendantIds = await coreManager.safeCore.getDescendantConversationIds(conversationId: conversationId)
        let allDescendants = await coreManager.safeCore.getConversationsByIds(conversationIds: descendantIds)
        let descendantsLookupMs = (CFAbsoluteTimeGetCurrent() - descendantsLookupStartedAt) * 1000

        // Get direct children from descendants
        let directChildren = allDescendants.filter { $0.thread.parentConversationId == conversationId }

        // Fetch messages for all descendants CONCURRENTLY
        let descendantMessagesStartedAt = CFAbsoluteTimeGetCurrent()
        let descendantMsgs = await withTaskGroup(
            of: (String, [Message]).self,
            returning: [String: [Message]].self
        ) { group in
            for descendant in allDescendants {
                group.addTask {
                    let msgs = await coreManager.safeCore.getMessages(conversationId: descendant.thread.id)
                    return (descendant.thread.id, msgs)
                }
            }

            var results: [String: [Message]] = [:]
            for await (id, msgs) in group {
                results[id] = msgs
            }
            return results
        }
        let descendantMessagesMs = (CFAbsoluteTimeGetCurrent() - descendantMessagesStartedAt) * 1000
        let totalMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "loadChildrenFromCore conversationId=\(conversationId) descendantIds=\(descendantIds.count) descendants=\(allDescendants.count) directChildren=\(directChildren.count) descendantMessageMaps=\(descendantMsgs.count) lookupMs=\(String(format: "%.2f", descendantsLookupMs)) messageFetchMs=\(String(format: "%.2f", descendantMessagesMs)) totalMs=\(String(format: "%.2f", totalMs))",
            category: .general,
            level: totalMs >= 250 ? .error : .info
        )

        return (directChildren, allDescendants, descendantMsgs)
    }

    // MARK: - Reactive Updates

    private func scheduleDerivedStateRecompute() {
        recomputePending = true
        guard recomputeTask == nil else { return }

        recomputeTask = Task { [weak self] in
            guard let self = self else { return }
            while self.recomputePending, !Task.isCancelled {
                self.recomputePending = false
                await self.recomputeDerivedState()
            }
            self.recomputeTask = nil
        }
    }

    private func recomputeDerivedStateNow() async {
        scheduleDerivedStateRecompute()
        await recomputeTask?.value
    }

    private func applyMessages(_ fetchedMessages: [Message], scheduleRecompute: Bool = true) {
        guard messages != fetchedMessages else { return }
        messages = fetchedMessages
        latestReply = fetchedMessages.last { $0.toolName == nil && !$0.content.isEmpty }
        todoState = TodoParser.parse(messages: fetchedMessages)
        if scheduleRecompute {
            scheduleDerivedStateRecompute()
        }
    }

    private func applyConversationUpdates(_ conversations: [ConversationFullInfo]) {
        if let updated = conversations.first(where: { $0.thread.id == conversation.thread.id }) {
            currentStatus = updated.thread.statusLabel ?? "unknown"
            currentIsActive = updated.isActive
            currentActivity = updated.thread.statusCurrentActivity
            currentEffectiveLastActivity = updated.thread.effectiveLastActivity
        }

        refreshDescendants(from: conversations)
    }

    private func refreshDescendants(from conversations: [ConversationFullInfo]) {
        var childrenMap: [String: [ConversationFullInfo]] = [:]
        for conv in conversations {
            if let parentId = conv.thread.parentConversationId {
                childrenMap[parentId, default: []].append(conv)
            }
        }

        childrenByParentId = childrenMap.mapValues { $0.map { $0.thread.id } }

        let descendantIds = collectDescendantIds(startId: conversation.thread.id, childrenMap: childrenMap)
        let descendants = conversations.filter { descendantIds.contains($0.thread.id) }
        let nextChildConversations = descendants
            .filter { $0.thread.parentConversationId == conversation.thread.id }
            .sorted { $0.thread.effectiveLastActivity > $1.thread.effectiveLastActivity }
        let descendantsChanged = allDescendants != descendants
        let childrenChanged = childConversations != nextChildConversations
        allDescendants = descendants
        childConversations = nextChildConversations
        pruneDescendantCaches(keeping: Set(descendantIds))

        if didCompleteInitialLoad {
            loadMissingDescendantMessages(from: descendants)
        }

        if descendantsChanged || childrenChanged {
            scheduleDerivedStateRecompute()
        }
    }

    private func collectDescendantIds(startId: String, childrenMap: [String: [ConversationFullInfo]]) -> [String] {
        var result: [String] = []
        var stack: [String] = [startId]
        var visited = Set<String>()

        while let current = stack.popLast() {
            guard let children = childrenMap[current] else { continue }
            for child in children {
                if visited.insert(child.thread.id).inserted {
                    result.append(child.thread.id)
                    stack.append(child.thread.id)
                }
            }
        }

        return result
    }

    private func loadMissingDescendantMessages(from descendants: [ConversationFullInfo]) {
        guard let coreManager = coreManager else { return }

        let missing = descendants.filter { descendantMessages[$0.thread.id] == nil }
        guard !missing.isEmpty else { return }
        profiler.logEvent(
            "loadMissingDescendantMessages conversationId=\(conversation.thread.id) missingCount=\(missing.count)",
            category: .general,
            level: .debug
        )

        Task {
            let startedAt = CFAbsoluteTimeGetCurrent()
            let fetched = await withTaskGroup(
                of: (String, [Message]).self,
                returning: [String: [Message]].self
            ) { group in
                for descendant in missing {
                    group.addTask {
                        let msgs = await coreManager.safeCore.getMessages(conversationId: descendant.thread.id)
                        return (descendant.thread.id, msgs)
                    }
                }

                var results: [String: [Message]] = [:]
                for await (id, msgs) in group {
                    results[id] = msgs
                }
                return results
            }

            await MainActor.run { [weak self] in
                guard let self = self else { return }
                for (id, msgs) in fetched {
                    self.descendantMessages[id] = msgs
                    self.dirtyDescendantTodoConversationIds.insert(id)
                }
                self.scheduleDerivedStateRecompute()
                let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
                self.profiler.logEvent(
                    "loadMissingDescendantMessages complete conversationId=\(self.conversation.thread.id) fetchedConversations=\(fetched.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 150 ? .error : .info
                )
            }
        }
    }

    private func applyDescendantMessages(from cache: [String: [Message]]) {
        guard !allDescendants.isEmpty else { return }
        var updated = false
        for descendant in allDescendants {
            if let msgs = cache[descendant.thread.id], descendantMessages[descendant.thread.id] != msgs {
                descendantMessages[descendant.thread.id] = msgs
                dirtyDescendantTodoConversationIds.insert(descendant.thread.id)
                updated = true
            }
        }
        if updated {
            scheduleDerivedStateRecompute()
        }
    }

    private func pruneDescendantCaches(keeping descendantIds: Set<String>) {
        descendantMessages = descendantMessages.filter { descendantIds.contains($0.key) }
        parsedTodoStates = parsedTodoStates.filter { descendantIds.contains($0.key) }
        dirtyDescendantTodoConversationIds = dirtyDescendantTodoConversationIds.filter { descendantIds.contains($0) }
    }

    private func profileName(for pubkey: String, coreManager: TenexCoreManager) async -> String {
        if let cached = profileNameCache[pubkey] {
            return cached
        }
        let name = await coreManager.safeCore.getProfileName(pubkey: pubkey)
        profileNameCache[pubkey] = name
        return name
    }

    // MARK: - Derived State Computation

    /// Recomputes all cached derived state when messages/children change
    private func recomputeDerivedState() async {
        let startedAt = CFAbsoluteTimeGetCurrent()
        let currentDescendantIds = Set(allDescendants.map { $0.thread.id })
        pruneDescendantCaches(keeping: currentDescendantIds)

        let participantsStartedAt = CFAbsoluteTimeGetCurrent()
        // Compute participating agent infos with pubkeys for avatar lookups.
        var agentInfosByPubkey: [String: AgentAvatarInfo] = [:]

        authorInfo = AgentAvatarInfo(
            name: conversation.author,
            pubkey: conversation.thread.pubkey
        )
        agentInfosByPubkey[conversation.thread.pubkey] = authorInfo

        for descendant in allDescendants {
            agentInfosByPubkey[descendant.thread.pubkey] = AgentAvatarInfo(
                name: descendant.author,
                pubkey: descendant.thread.pubkey
            )
        }

        participatingAgentInfos = agentInfosByPubkey.values.sorted { $0.name < $1.name }

        if let pTaggedPubkey = conversation.thread.pTags.first, let coreManager = coreManager {
            let name = await profileName(for: pTaggedPubkey, coreManager: coreManager)
            pTaggedRecipientInfo = AgentAvatarInfo(name: name, pubkey: pTaggedPubkey)
        } else {
            pTaggedRecipientInfo = nil
        }

        let pTaggedPubkey = conversation.thread.pTags.first
        otherParticipantsInfo = participatingAgentInfos.filter {
            $0.pubkey != conversation.thread.pubkey && $0.pubkey != pTaggedPubkey
        }

        latestReply = messages.last { $0.toolName == nil && !$0.content.isEmpty }
        let participantsMs = (CFAbsoluteTimeGetCurrent() - participantsStartedAt) * 1000

        let todoStartedAt = CFAbsoluteTimeGetCurrent()
        childrenByParentId = [:]
        for descendant in allDescendants {
            if let parentId = descendant.thread.parentConversationId {
                childrenByParentId[parentId, default: []].append(descendant.thread.id)
            }
        }

        if dirtyDescendantTodoConversationIds.isEmpty && parsedTodoStates.count != descendantMessages.count {
            dirtyDescendantTodoConversationIds.formUnion(descendantMessages.keys)
        }

        for conversationId in dirtyDescendantTodoConversationIds {
            guard let descendantMessagesForId = descendantMessages[conversationId] else {
                parsedTodoStates.removeValue(forKey: conversationId)
                continue
            }
            parsedTodoStates[conversationId] = TodoParser.parse(messages: descendantMessagesForId)
        }
        dirtyDescendantTodoConversationIds.removeAll()

        var stats = AggregateTodoStats.empty
        stats.add(todoState)
        for descendant in allDescendants {
            if let todos = parsedTodoStates[descendant.thread.id] {
                stats.add(todos)
            }
        }
        aggregatedTodoStats = stats
        let todoMs = (CFAbsoluteTimeGetCurrent() - todoStartedAt) * 1000

        delegationActivityByConversationId = ConversationActivityMetrics.delegationActivityByConversationId(
            directChildren: childConversations,
            allDescendants: allDescendants
        )

        let delegationsStartedAt = CFAbsoluteTimeGetCurrent()
        delegations = await extractDelegations()
        let delegationsMs = (CFAbsoluteTimeGetCurrent() - delegationsStartedAt) * 1000

        let reportsStartedAt = CFAbsoluteTimeGetCurrent()
        referencedReports = extractReferencedReports()
        let reportsMs = (CFAbsoluteTimeGetCurrent() - reportsStartedAt) * 1000

        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "recomputeDerivedState conversationId=\(conversation.thread.id) messages=\(messages.count) descendants=\(allDescendants.count) delegations=\(delegations.count) referencedReports=\(referencedReports.count) participantsMs=\(String(format: "%.2f", participantsMs)) todoMs=\(String(format: "%.2f", todoMs)) delegationsMs=\(String(format: "%.2f", delegationsMs)) reportsMs=\(String(format: "%.2f", reportsMs)) totalMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 100 ? .error : .info
        )
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
        let startedAt = CFAbsoluteTimeGetCurrent()

        var result: [DelegationItem] = []

        for message in messages {
            // Check for delegate tool calls
            if message.toolName == "mcp__tenex__delegate" || message.toolName == "delegate" {
                // qTags contain the conversation IDs of delegated conversations
                for qTag in message.qTags {
                    // Find the child conversation matching this qTag
                    if let childConv = childConversations.first(where: { $0.thread.id == qTag }) {
                        // Get recipient from p-tag of the child conversation (who was delegated TO)
                        let recipientPubkey = childConv.thread.pTags.first ?? childConv.thread.pubkey
                        let recipient = await profileName(for: recipientPubkey, coreManager: coreManager)

                        // Compute subtree todo stats for this delegation
                        let todoStats = computeSubtreeTodoStats(forConversationId: qTag)

                        var delegation = DelegationItem(
                            id: qTag,
                            recipient: recipient.isEmpty ? childConv.author : recipient,
                            recipientPubkey: recipientPubkey,
                            messagePreview: childConv.thread.title,
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
            if !result.contains(where: { $0.conversationId == child.thread.id }) {
                // Get recipient from child's p-tag if available
                let recipientPubkey = child.thread.pTags.first ?? child.thread.pubkey
                let recipient = await profileName(for: recipientPubkey, coreManager: coreManager)

                // Compute subtree todo stats for this delegation
                let todoStats = computeSubtreeTodoStats(forConversationId: child.thread.id)

                var delegation = DelegationItem(
                    id: child.thread.id,
                    recipient: recipient.isEmpty ? child.author : recipient,
                    recipientPubkey: recipientPubkey,
                    messagePreview: child.thread.summary ?? child.thread.title,
                    conversationId: child.thread.id,
                    timestamp: child.thread.lastActivity
                )
                delegation.todoStats = todoStats.hasTodos ? todoStats : nil
                result.append(delegation)
            }
        }

        let sorted = result.sorted { $0.timestamp > $1.timestamp }
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "extractDelegations conversationId=\(conversation.thread.id) messages=\(messages.count) children=\(childConversations.count) result=\(sorted.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 120 ? .error : .info
        )
        return sorted
    }

    /// Extracts report references from message a-tags, deduplicated by full a-tag coordinate.
    /// Matches TUI behavior: consume only explicit a-tags, never infer from message content.
    private func extractReferencedReports() -> [ReferencedReportItem] {
        guard let coreManager = coreManager else { return [] }

        // Build lookup from canonical report a-tag to Report for fast resolution.
        var reportsByATag: [String: Report] = [:]
        for report in coreManager.reports {
            let aTag = "30023:\(report.author):\(report.slug)"
            if reportsByATag[aTag] == nil {
                reportsByATag[aTag] = report
            }
        }

        var seen = Set<String>()
        var referenced: [ReferencedReportItem] = []

        for message in messages {
            for aTag in message.aTags {
                // Only keep report references (kind 30023) and dedupe.
                guard let coordinate = ReportATagCoordinate.parse(aTag), coordinate.kind == 30023 else {
                    continue
                }
                guard seen.insert(aTag).inserted else { continue }

                if let report = reportsByATag[aTag] {
                    referenced.append(
                        ReferencedReportItem(
                            aTag: aTag,
                            title: report.title,
                            slug: report.slug,
                            report: report
                        )
                    )
                } else {
                    referenced.append(
                        ReferencedReportItem(
                            aTag: aTag,
                            title: coordinate.slug,
                            slug: coordinate.slug,
                            report: nil
                        )
                    )
                }
            }
        }

        return referenced
    }

    // MARK: - Runtime Calculation

    /// Gets the hierarchical LLM runtime for this conversation (includes all descendants).
    /// Returns the total runtime in seconds by converting from milliseconds.
    func getHierarchicalRuntime() async -> TimeInterval {
        guard let coreManager = coreManager else { return 0 }

        // Get runtime in milliseconds from the FFI
        let runtimeMs = await coreManager.safeCore.getConversationRuntimeMs(conversationId: conversation.thread.id)

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
        childConversations.first { $0.thread.id == delegationId }
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
    /// - Returns: The hex pubkey of the last agent that spoke, or nil if none found
    static func findLastAgentPubkey(
        messages: [Message],
        availableAgents: [ProjectAgent]
    ) -> String? {
        // Get set of agent pubkeys (hex format) for quick lookup
        let agentPubkeys = Set(availableAgents.map { $0.pubkey })

        // Find the most recent message from a known agent
        var latestAgentPubkey: String?
        var latestTimestamp: UInt64 = 0

        for msg in messages {
            // Message pubkey is already hex -- check directly against known agents
            if agentPubkeys.contains(msg.pubkey) && msg.createdAt >= latestTimestamp {
                latestTimestamp = msg.createdAt
                latestAgentPubkey = msg.pubkey
            }
        }

        return latestAgentPubkey
    }
}
