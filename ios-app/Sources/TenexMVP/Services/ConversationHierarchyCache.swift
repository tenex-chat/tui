import Foundation

// MARK: - Conversation Hierarchy Cache

/// Caches precomputed hierarchy data for conversations to avoid N+1 FFI calls.
///
/// ## Problem
/// When rendering a list of conversations, each `ConversationRowFull` calls
/// `getDescendantConversationIds` and `getConversationsByIds` in its `.task` modifier.
/// For N conversations, this results in 2N FFI calls on every scroll.
///
/// ## Solution
/// This cache precomputes all hierarchy data in a single batch operation,
/// making row rendering O(1) instead of O(FFI calls).
///
/// ## Usage
/// ```swift
/// // In ConversationsTabView:
/// .task {
///     await coreManager.hierarchyCache.preloadForConversations(rootConversations)
/// }
///
/// // In ConversationRowFull:
/// let hierarchy = coreManager.hierarchyCache.getHierarchy(for: conversation.id)
/// ```
@MainActor
final class ConversationHierarchyCache: ObservableObject {
    private static let preloadBatchSize = 20

    // MARK: - Types

    /// Cached hierarchy data for a single conversation.
    /// Reference semantics avoid repeated value copies when rows read cached hierarchy.
    final class ConversationHierarchy: @unchecked Sendable {
        /// P-tagged recipient info (first p-tag from conversation root)
        let pTaggedRecipientInfo: AgentAvatarInfo?

        /// Delegation agent infos (unique agents from descendants, excluding author)
        let delegationAgentInfos: [AgentAvatarInfo]

        /// All descendant conversation IDs
        let descendantIds: [String]

        /// Direct child conversation IDs
        let directChildIds: [String]

        init(
            pTaggedRecipientInfo: AgentAvatarInfo?,
            delegationAgentInfos: [AgentAvatarInfo],
            descendantIds: [String],
            directChildIds: [String]
        ) {
            self.pTaggedRecipientInfo = pTaggedRecipientInfo
            self.delegationAgentInfos = delegationAgentInfos
            self.descendantIds = descendantIds
            self.directChildIds = directChildIds
        }
    }

    // MARK: - State

    private var cache: [String: ConversationHierarchy] = [:]
    private var isLoading = false
    private var loadedForConversationIds: Set<String> = []

    // MARK: - Dependencies

    private weak var coreManager: TenexCoreManager?
    private let profiler = PerformanceProfiler.shared

    // MARK: - Initialization

    init() {}

    func setCoreManager(_ coreManager: TenexCoreManager) {
        self.coreManager = coreManager
    }

    // MARK: - Cache Access

    /// Get cached hierarchy for a conversation (O(1))
    func getHierarchy(for conversationId: String) -> ConversationHierarchy? {
        cache[conversationId]
    }

    /// Check if a conversation's hierarchy is cached
    func isCached(_ conversationId: String) -> Bool {
        cache[conversationId] != nil
    }

    // MARK: - Preloading

    /// Preload hierarchy data for a batch of root conversations.
    /// This is the key optimization - batch load instead of per-row load.
    func preloadForConversations(_ conversations: [ConversationFullInfo]) async {
        guard let coreManager = coreManager else { return }
        guard !isLoading else { return }
        let startedAt = CFAbsoluteTimeGetCurrent()

        // Check if we need to load anything
        let newConversationIds = Set(conversations.map(\.id))
        let needsLoading = newConversationIds.subtracting(loadedForConversationIds)
        guard !needsLoading.isEmpty else { return }
        let pendingConversations = conversations.filter { needsLoading.contains($0.id) }
        let batch = Array(pendingConversations.prefix(Self.preloadBatchSize))
        profiler.logEvent(
            "hierarchy preload start roots=\(conversations.count) new=\(needsLoading.count) batch=\(batch.count) cached=\(loadedForConversationIds.count)",
            category: .general,
            level: .debug
        )

        isLoading = true
        defer { isLoading = false }

        // Batch fetch all descendants for all conversations
        // This could potentially be optimized further with a single FFI call
        // that returns descendants for multiple roots

        await withTaskGroup(of: (String, ConversationHierarchy).self) { group in
            for conversation in batch {
                group.addTask { [coreManager] in
                    let hierarchy = await Self.loadHierarchy(
                        for: conversation,
                        using: coreManager
                    )
                    return (conversation.id, hierarchy)
                }
            }

            for await (id, hierarchy) in group {
                cache[id] = hierarchy
                loadedForConversationIds.insert(id)
            }
        }

        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        let remaining = needsLoading.count - batch.count
        profiler.logEvent(
            "hierarchy preload complete roots=\(conversations.count) loadedBatch=\(batch.count) remaining=\(remaining) cacheSize=\(cache.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 200 ? .error : .info
        )

        // Continue in small chunks to avoid monopolizing the core actor during startup.
        if remaining > 0 {
            Task(priority: .utility) { [weak self] in
                try? await Task.sleep(nanoseconds: 150_000_000)
                await self?.preloadForConversations(conversations)
            }
        }
    }

    /// Load hierarchy for a single conversation (used by TaskGroup)
    /// This method runs off the MainActor to avoid blocking the UI during heavy FFI calls
    private static func loadHierarchy(
        for conversation: ConversationFullInfo,
        using coreManager: TenexCoreManager
    ) async -> ConversationHierarchy {
        // Run all heavy FFI work off the MainActor in a detached task
        // This prevents blocking the UI during profile lookups and hierarchy traversal
        return await Task.detached {
            let startedAt = CFAbsoluteTimeGetCurrent()
            // Load p-tagged recipient
            var pTaggedRecipientInfo: AgentAvatarInfo?
            if let pTaggedPubkey = conversation.pTags.first {
                let name = await coreManager.safeCore.getProfileName(pubkey: pTaggedPubkey)
                pTaggedRecipientInfo = AgentAvatarInfo(name: name, pubkey: pTaggedPubkey)
            }

            // Get all descendants
            let descendantIds = await coreManager.safeCore.getDescendantConversationIds(
                conversationId: conversation.id
            )
            let descendants = await coreManager.safeCore.getConversationsByIds(
                conversationIds: descendantIds
            )

            // Direct children
            let directChildIds = descendants
                .filter { $0.parentId == conversation.id }
                .map(\.id)

            // Collect unique agents from descendants
            let pTaggedPubkey = conversation.pTags.first
            var agentsByPubkey: [String: AgentAvatarInfo] = [:]
            for descendant in descendants {
                if descendant.authorPubkey != conversation.authorPubkey &&
                    descendant.authorPubkey != pTaggedPubkey {
                    agentsByPubkey[descendant.authorPubkey] = AgentAvatarInfo(
                        name: descendant.author,
                        pubkey: descendant.authorPubkey
                    )
                }
            }

            let delegationAgentInfos = agentsByPubkey.values.sorted { $0.name < $1.name }
            let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
            if elapsedMs >= 20 || !descendantIds.isEmpty || !delegationAgentInfos.isEmpty {
                PerformanceProfiler.shared.logEvent(
                    "hierarchy load root=\(conversation.id) descendants=\(descendantIds.count) directChildren=\(directChildIds.count) delegationAgents=\(delegationAgentInfos.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 120 ? .error : .info
                )
            }

            return ConversationHierarchy(
                pTaggedRecipientInfo: pTaggedRecipientInfo,
                delegationAgentInfos: delegationAgentInfos,
                descendantIds: descendantIds,
                directChildIds: directChildIds
            )
        }.value
    }

    // MARK: - Cache Management

    /// Clear the entire cache (call on logout or memory warning)
    func clearCache() {
        cache.removeAll()
        loadedForConversationIds.removeAll()
    }

    /// Invalidate cache for specific conversations (call when data changes)
    func invalidate(_ conversationIds: [String]) {
        for id in conversationIds {
            cache.removeValue(forKey: id)
            loadedForConversationIds.remove(id)
        }
    }

    /// Get cache statistics for debugging
    var stats: (cachedCount: Int, loadedCount: Int) {
        (cache.count, loadedForConversationIds.count)
    }
}

// MARK: - AgentAvatarInfo (if not defined elsewhere)

// Note: AgentAvatarInfo should already be defined in ConversationHierarchy.swift
// If not, uncomment this:
//
// struct AgentAvatarInfo: Identifiable, Hashable {
//     let id = UUID()
//     let name: String
//     let pubkey: String
// }
