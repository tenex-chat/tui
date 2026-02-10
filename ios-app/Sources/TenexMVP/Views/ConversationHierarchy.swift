import SwiftUI
import CryptoKit
import Kingfisher

// MARK: - Shared UI Constants

/// Maximum number of agent avatars to display before showing "+N" overflow badge
let maxVisibleAvatars = 6

/// Length of pubkey prefix to display in truncated form (e.g., in logs and error messages)
let pubkeyDisplayPrefixLength = 12

// MARK: - Shared Color Utilities

/// Deterministic color selection using SHA-256 hash.
/// Uses consistent color palette across the app for visual coherence.
/// - Parameters:
///   - identifier: The unique identifier to hash (e.g., project ID, pubkey)
///   - colors: Optional custom color palette (defaults to standard palette)
/// - Returns: A Color deterministically selected based on the identifier
func deterministicColor(for identifier: String, from colors: [Color]? = nil) -> Color {
    let palette = colors ?? [.blue, .purple, .orange, .green, .pink, .indigo, .teal, .cyan]
    let data = Data(identifier.utf8)
    let hash = SHA256.hash(data: data)
    let firstByte = hash.withUnsafeBytes { $0[0] }
    return palette[Int(firstByte) % palette.count]
}

// MARK: - Conversation Hierarchy Data Model

/// Precomputed hierarchy data for efficient conversation tree operations.
/// Computes parent→children map and aggregated data once per refresh,
/// avoiding O(n²) BFS traversals per row render.
final class ConversationHierarchy {
    /// Map from conversation ID to its direct children
    let childrenByParentId: [String: [ConversationInfo]]

    /// Map from conversation ID to precomputed aggregated data
    let aggregatedData: [String: AggregatedConversationData]

    /// Root conversations (including orphaned nodes whose parents are missing)
    let rootConversations: [ConversationInfo]

    /// Initialize hierarchy from a flat list of conversations
    /// - Parameter conversations: All conversations to process
    init(conversations: [ConversationInfo]) {
        // Step 1: Build parent→children map (O(n))
        var childrenMap: [String: [ConversationInfo]] = [:]
        let conversationIds = Set(conversations.map { $0.id })

        for conversation in conversations {
            if let parentId = conversation.parentId {
                childrenMap[parentId, default: []].append(conversation)
            }
        }

        // Sort children by lastActivity descending for deterministic ordering
        for (parentId, children) in childrenMap {
            childrenMap[parentId] = children.sorted { $0.lastActivity > $1.lastActivity }
        }

        self.childrenByParentId = childrenMap

        // Step 2: Identify root conversations (no parent OR orphaned)
        // Orphaned = has parentId but that parent doesn't exist in our set
        let roots = conversations.filter { conversation in
            if let parentId = conversation.parentId {
                // Orphaned: parent doesn't exist - treat as root
                return !conversationIds.contains(parentId)
            }
            // No parent - true root
            return true
        }

        // Sort roots by effective last activity (computed below) - we'll re-sort after computing aggregates
        self.rootConversations = roots.sorted { $0.lastActivity > $1.lastActivity }

        // Step 3: Compute aggregated data for each conversation (using safe BFS with cycle detection)
        var aggregated: [String: AggregatedConversationData] = [:]

        for conversation in conversations {
            let descendants = Self.computeDescendants(
                rootId: conversation.id,
                childrenMap: childrenMap
            )

            let allActivities = [conversation.lastActivity] + descendants.map { $0.lastActivity }
            let effectiveLastActivity = allActivities.max() ?? conversation.lastActivity
            let earliestActivity = allActivities.min() ?? conversation.lastActivity
            let activitySpan = TimeInterval(effectiveLastActivity - earliestActivity)

            // Track agent names (for backward compatibility)
            var agentNames = Set<String>()
            agentNames.insert(conversation.author)
            for descendant in descendants {
                agentNames.insert(descendant.author)
            }

            // Track agent info (name + pubkey) for avatar lookups
            // Use pubkey as unique key to avoid duplicates from same agent
            let authorInfo = AgentAvatarInfo(
                name: conversation.author,
                pubkey: conversation.authorPubkey
            )

            // Collect delegation agents (descendants only, excluding the author)
            var delegationAgentsByPubkey: [String: AgentAvatarInfo] = [:]
            for descendant in descendants {
                // Skip if this is the same agent as the author
                if descendant.authorPubkey != conversation.authorPubkey {
                    delegationAgentsByPubkey[descendant.authorPubkey] = AgentAvatarInfo(
                        name: descendant.author,
                        pubkey: descendant.authorPubkey
                    )
                }
            }

            // Sort delegation agents by name for consistent display
            let sortedDelegationAgents = delegationAgentsByPubkey.values.sorted { $0.name < $1.name }

            // All participating agents (for backward compatibility)
            var allAgentsByPubkey = delegationAgentsByPubkey
            allAgentsByPubkey[conversation.authorPubkey] = authorInfo
            let sortedAgentInfos = allAgentsByPubkey.values.sorted { $0.name < $1.name }

            aggregated[conversation.id] = AggregatedConversationData(
                effectiveLastActivity: effectiveLastActivity,
                activitySpan: activitySpan,
                participatingAgents: agentNames.sorted(),
                participatingAgentInfos: sortedAgentInfos,
                authorInfo: authorInfo,
                delegationAgentInfos: sortedDelegationAgents,
                descendantCount: descendants.count
            )
        }

        self.aggregatedData = aggregated
    }

    /// Compute all descendants using BFS with cycle detection
    /// - Parameters:
    ///   - rootId: The conversation ID to start from
    ///   - childrenMap: Precomputed parent→children map
    /// - Returns: All descendant conversations (empty if none or cycle detected)
    private static func computeDescendants(
        rootId: String,
        childrenMap: [String: [ConversationInfo]]
    ) -> [ConversationInfo] {
        var descendants: [ConversationInfo] = []
        var visited = Set<String>()

        // Use ArraySlice for O(1) removeFirst via index tracking
        var queue = childrenMap[rootId] ?? []
        var queueIndex = 0

        while queueIndex < queue.count {
            let current = queue[queueIndex]
            queueIndex += 1

            // Cycle detection: skip if already visited
            if visited.contains(current.id) {
                continue
            }
            visited.insert(current.id)

            descendants.append(current)

            // Add children to queue
            if let children = childrenMap[current.id] {
                queue.append(contentsOf: children)
            }
        }

        return descendants
    }

    /// Get aggregated data for a conversation, with fallback defaults
    func getData(for conversationId: String) -> AggregatedConversationData {
        aggregatedData[conversationId] ?? AggregatedConversationData.empty
    }

    /// Get root conversations sorted by effective last activity (stable ordering)
    func getSortedRoots() -> [ConversationInfo] {
        rootConversations.sorted { lhs, rhs in
            let lhsActivity = aggregatedData[lhs.id]?.effectiveLastActivity ?? lhs.lastActivity
            let rhsActivity = aggregatedData[rhs.id]?.effectiveLastActivity ?? rhs.lastActivity

            if lhsActivity != rhsActivity {
                return lhsActivity > rhsActivity // Descending by activity
            }
            // Tiebreaker: sort by ID for deterministic ordering
            return lhs.id < rhs.id
        }
    }
}

// MARK: - Aggregated Conversation Data

/// Precomputed aggregated data for a conversation and its descendants
struct AggregatedConversationData {
    /// Maximum lastActivity across conversation and all descendants
    let effectiveLastActivity: UInt64

    /// Time span from earliest to latest activity (renamed from "total running time")
    let activitySpan: TimeInterval

    /// Sorted list of unique participating agent names (kept for backward compatibility)
    let participatingAgents: [String]

    /// List of participating agents with pubkeys for avatar lookups
    let participatingAgentInfos: [AgentAvatarInfo]

    /// The author who started this conversation (for standalone display)
    let authorInfo: AgentAvatarInfo?

    /// Agents from delegations only (excludes author, for overlapping display)
    let delegationAgentInfos: [AgentAvatarInfo]

    /// Number of descendant conversations
    let descendantCount: Int

    /// Empty/default aggregated data
    static let empty = AggregatedConversationData(
        effectiveLastActivity: 0,
        activitySpan: 0,
        participatingAgents: [],
        participatingAgentInfos: [],
        authorInfo: nil,
        delegationAgentInfos: [],
        descendantCount: 0
    )
}

// MARK: - Agent Avatar Info

/// Information needed to display an agent's avatar
struct AgentAvatarInfo: Hashable, Identifiable {
    let name: String
    let pubkey: String

    var id: String { pubkey }
}

// MARK: - Unified Agent Avatar View

/// Unified reusable avatar view for displaying agent avatars throughout the app.
///
/// Features:
/// - Deterministic SHA-256 hash for stable avatar colors across app launches
/// - Cached profile picture lookups to prevent repeated FFI calls during scroll
/// - Consistent fallback strategy: kind:0 profile picture → agent-definition picture → initials
/// - Configurable size and border options
///
/// Usage:
/// ```swift
/// // With pubkey (recommended - enables caching and kind:0 lookup)
/// AgentAvatarView(agentName: "claude-code", pubkey: "abc123...")
///
/// // With explicit fallback URL (agent-definition picture)
/// AgentAvatarView(agentName: "claude-code", pubkey: "abc123...", fallbackPictureUrl: agent.picture)
///
/// // Without pubkey (uses name-based initials only)
/// AgentAvatarView(agentName: "claude-code")
/// ```
struct AgentAvatarView: View {
    @EnvironmentObject var coreManager: TenexCoreManager

    /// Agent's display name (used for initials and color if no pubkey)
    let agentName: String

    /// Agent's public key in hex format (enables profile picture lookup and deterministic color)
    var pubkey: String? = nil

    /// Fallback picture URL from agent-definition (kind:4199), used if kind:0 has no picture
    var fallbackPictureUrl: String? = nil

    /// Avatar size in points
    var size: CGFloat = 24

    /// Font size for initials (auto-scaled if not specified)
    var fontSize: CGFloat? = nil

    /// Whether to show the border stroke (for overlapping avatars)
    var showBorder: Bool = true

    /// Whether this avatar is selected (shows blue border)
    var isSelected: Bool = false

    /// Profile picture URL fetched from kind:0 or cache
    @State private var kind0PictureUrl: String?

    /// Computed font size based on avatar size if not explicitly set
    private var effectiveFontSize: CGFloat {
        fontSize ?? (size * 0.42)
    }

    /// The best available picture URL using consistent fallback strategy:
    /// 1. kind:0 profile picture (from Nostr metadata)
    /// 2. Agent-definition picture (from kind:4199)
    /// 3. nil (will show initials)
    private var effectivePictureUrl: String? {
        kind0PictureUrl ?? fallbackPictureUrl
    }

    /// Generate a deterministic color using SHA-256 hash.
    /// Uses pubkey if available (most stable), falls back to agent name.
    /// This ensures consistent colors across app launches, unlike Swift's hashValue.
    private var avatarColor: Color {
        let colors: [Color] = [.blue, .purple, .orange, .green, .pink, .indigo, .teal, .cyan, .mint]
        let hashInput = pubkey ?? agentName

        // SHA-256 produces deterministic output for the same input
        let data = Data(hashInput.utf8)
        let hash = SHA256.hash(data: data)

        // Use first byte of hash for color selection
        let firstByte = hash.withUnsafeBytes { $0[0] }
        return colors[Int(firstByte) % colors.count]
    }

    /// Get initials from agent name
    private var initials: String {
        let parts = agentName.split(separator: "-")
        if parts.count >= 2 {
            // For names like "claude-code" -> "CC"
            return String(parts.prefix(2).compactMap { $0.first }.map { String($0).uppercased() }.joined())
        } else if !agentName.isEmpty {
            // Single word -> first two chars
            let chars = agentName.prefix(2)
            return String(chars).uppercased()
        }
        return "?"
    }

    /// Placeholder avatar with initials
    private var placeholderAvatar: some View {
        Circle()
            .fill(avatarColor.gradient)
            .frame(width: size, height: size)
            .overlay {
                Text(initials)
                    .font(.system(size: effectiveFontSize, weight: .semibold))
                    .foregroundStyle(.white)
            }
    }

    var body: some View {
        Group {
            if let pictureUrl = effectivePictureUrl, let url = URL(string: pictureUrl) {
                KFImage(url)
                    .placeholder {
                        placeholderAvatar
                    }
                    .retry(maxCount: 2, interval: .seconds(1))
                    .fade(duration: 0.2)
                    .resizable()
                    .aspectRatio(contentMode: .fill)
                    .frame(width: size, height: size)
                    .clipShape(Circle())
            } else {
                placeholderAvatar
            }
        }
        .overlay {
            if showBorder {
                Circle()
                    .stroke(Color.systemBackground, lineWidth: 2)
            }
        }
        .overlay {
            if isSelected {
                Circle()
                    .strokeBorder(Color.blue, lineWidth: 2)
            }
        }
        .onAppear {
            // Fetch profile picture asynchronously to avoid blocking UI thread during scroll.
            // Uses cached API to prevent repeated FFI calls for the same pubkey.
            if let pubkey = pubkey {
                Task {
                    // Perform FFI call on background thread
                    let pictureUrl = await fetchProfilePictureAsync(pubkey: pubkey)
                    // Update UI on main thread
                    await MainActor.run {
                        kind0PictureUrl = pictureUrl
                    }
                }
            }
        }
    }

    /// Fetch profile picture asynchronously off the main thread.
    /// This prevents FFI calls from blocking the UI during scroll.
    private func fetchProfilePictureAsync(pubkey: String) async -> String? {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = coreManager.getProfilePicture(pubkey: pubkey)
                continuation.resume(returning: result)
            }
        }
    }
}

// MARK: - Conversation Avatar Group

/// Reusable avatar group component that displays:
/// - Author avatar (standalone on left)
/// - Gap (12pt)
/// - Other participants (overlapping group)
///
/// Used in both conversation list and conversation detail views for consistent avatar display.
/// Shows author + p-tagged recipient overlapping, then a gap, then other participants overlapping.
struct ConversationAvatarGroup: View {
    @EnvironmentObject var coreManager: TenexCoreManager

    /// Author who started the conversation
    let authorInfo: AgentAvatarInfo

    /// P-tagged recipient info (shown overlapping with author)
    let pTaggedRecipientInfo: AgentAvatarInfo?

    /// Other participants (shown overlapping after gap, excluding author and p-tagged)
    let otherParticipants: [AgentAvatarInfo]

    /// Avatar size (default 24 for conversation list, smaller for detail header)
    var avatarSize: CGFloat = 24

    /// Font size for initials (auto-scaled if not specified)
    var fontSize: CGFloat? = nil

    /// Maximum visible avatars in the other participants group
    var maxVisibleAvatars: Int = 8

    /// Overlap offset for avatars (default: avatarSize * 0.67)
    private var overlapOffset: CGFloat { avatarSize * 0.67 }

    var body: some View {
        HStack(spacing: 0) {
            // Author + p-tagged recipient overlapping
            ZStack(alignment: .leading) {
                // Author avatar
                AgentAvatarView(
                    agentName: authorInfo.name,
                    pubkey: authorInfo.pubkey,
                    size: avatarSize,
                    fontSize: fontSize
                )
                .environmentObject(coreManager)
                .zIndex(1)

                // P-tagged recipient overlapping with author
                if let pTagged = pTaggedRecipientInfo {
                    AgentAvatarView(
                        agentName: pTagged.name,
                        pubkey: pTagged.pubkey,
                        size: avatarSize,
                        fontSize: fontSize
                    )
                    .environmentObject(coreManager)
                    .offset(x: overlapOffset)
                    .zIndex(0)
                }
            }
            .frame(width: pTaggedRecipientInfo != nil ? avatarSize + overlapOffset : avatarSize, height: avatarSize)

            // Gap and other participants
            if !otherParticipants.isEmpty {
                Spacer()
                    .frame(width: 12)

                // Overlapping other participant avatars
                ZStack(alignment: .leading) {
                    ForEach(Array(otherParticipants.prefix(maxVisibleAvatars).enumerated()), id: \.element.id) { index, agentInfo in
                        AgentAvatarView(
                            agentName: agentInfo.name,
                            pubkey: agentInfo.pubkey,
                            size: avatarSize,
                            fontSize: fontSize
                        )
                        .environmentObject(coreManager)
                        .offset(x: CGFloat(index) * (avatarSize - 8))
                        .zIndex(Double(maxVisibleAvatars - index))
                    }

                    // +N indicator
                    if otherParticipants.count > maxVisibleAvatars {
                        Circle()
                            .fill(Color.systemGray4)
                            .frame(width: avatarSize, height: avatarSize)
                            .overlay {
                                Text("+\(otherParticipants.count - maxVisibleAvatars)")
                                    .font(fontSize != nil ? .system(size: fontSize! * 0.8) : .caption2)
                                    .fontWeight(.medium)
                                    .foregroundStyle(.secondary)
                            }
                            .offset(x: CGFloat(maxVisibleAvatars) * (avatarSize - 8))
                    }
                }
                .frame(height: avatarSize)
            }
        }
    }
}

// MARK: - Legacy Compatibility Aliases

/// Legacy SharedAgentAvatar - now a thin wrapper around AgentAvatarView.
/// Prefer using AgentAvatarView directly for new code.
struct SharedAgentAvatar: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let agentName: String
    var pictureUrl: String? = nil
    var size: CGFloat = 24
    var fontSize: CGFloat = 10

    var body: some View {
        AgentAvatarView(
            agentName: agentName,
            fallbackPictureUrl: pictureUrl,
            size: size,
            fontSize: fontSize
        )
        .environmentObject(coreManager)
    }
}

/// Legacy AgentAvatarWithPicture - now a thin wrapper around AgentAvatarView.
/// Prefer using AgentAvatarView directly for new code.
struct AgentAvatarWithPicture: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let agentInfo: AgentAvatarInfo
    var size: CGFloat = 24
    var fontSize: CGFloat = 10

    var body: some View {
        AgentAvatarView(
            agentName: agentInfo.name,
            pubkey: agentInfo.pubkey,
            size: size,
            fontSize: fontSize
        )
        .environmentObject(coreManager)
    }
}

// MARK: - Shared Formatters

/// Utility for shared date/time formatting across conversation views
enum ConversationFormatters {
    /// Shared RelativeDateTimeFormatter instance (expensive to create)
    private static let relativeFormatter: RelativeDateTimeFormatter = {
        let formatter = RelativeDateTimeFormatter()
        formatter.unitsStyle = .abbreviated
        return formatter
    }()

    /// Format a timestamp as relative time (e.g., "5m ago")
    static func formatRelativeTime(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return relativeFormatter.localizedString(for: date, relativeTo: Date())
    }

    /// Format a duration in seconds as human-readable (e.g., "2h 30m")
    static func formatDuration(_ seconds: TimeInterval) -> String {
        let hours = Int(seconds) / 3600
        let minutes = (Int(seconds) % 3600) / 60

        if hours > 0 {
            return "\(hours)h \(minutes)m"
        } else if minutes > 0 {
            return "\(minutes)m"
        } else {
            return "<1m"
        }
    }
}

// MARK: - Status Color Helper

/// Get status color for a conversation status string
func conversationStatusColor(for status: String) -> Color {
    switch status {
    case "active": return .green
    case "waiting": return .orange
    case "completed": return .gray
    default: return .blue
    }
}
