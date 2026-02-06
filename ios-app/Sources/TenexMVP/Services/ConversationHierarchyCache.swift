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
    // MARK: - Types

    /// Cached hierarchy data for a single conversation
    struct ConversationHierarchy {
        /// P-tagged recipient info (first p-tag from conversation root)
        let pTaggedRecipientInfo: AgentAvatarInfo?

        /// Delegation agent infos (unique agents from descendants, excluding author)
        let delegationAgentInfos: [AgentAvatarInfo]

        /// All descendant conversation IDs
        let descendantIds: [String]

        /// Direct child conversation IDs
        let directChildIds: [String]
    }

    // MARK: - State

    private var cache: [String: ConversationHierarchy] = [:]
    private var isLoading = false
    private var loadedForConversationIds: Set<String> = []

    // MARK: - Dependencies

    private weak var coreManager: TenexCoreManager?

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

        // Check if we need to load anything
        let newConversationIds = Set(conversations.map(\.id))
        let needsLoading = newConversationIds.subtracting(loadedForConversationIds)
        guard !needsLoading.isEmpty else { return }

        isLoading = true
        defer { isLoading = false }

        // Batch fetch all descendants for all conversations
        // This could potentially be optimized further with a single FFI call
        // that returns descendants for multiple roots

        await withTaskGroup(of: (String, ConversationHierarchy).self) { group in
            for conversation in conversations where needsLoading.contains(conversation.id) {
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
