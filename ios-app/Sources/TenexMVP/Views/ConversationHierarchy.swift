import SwiftUI

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
        var roots = conversations.filter { conversation in
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

            var agents = Set<String>()
            agents.insert(conversation.author)
            for descendant in descendants {
                agents.insert(descendant.author)
            }

            aggregated[conversation.id] = AggregatedConversationData(
                effectiveLastActivity: effectiveLastActivity,
                activitySpan: activitySpan,
                participatingAgents: agents.sorted(),
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

    /// Sorted list of unique participating agent names
    let participatingAgents: [String]

    /// Number of descendant conversations
    let descendantCount: Int

    /// Empty/default aggregated data
    static let empty = AggregatedConversationData(
        effectiveLastActivity: 0,
        activitySpan: 0,
        participatingAgents: [],
        descendantCount: 0
    )
}

// MARK: - Shared Agent Avatar View

/// Reusable avatar view for displaying agent initials with consistent colors
struct SharedAgentAvatar: View {
    let agentName: String
    var size: CGFloat = 24
    var fontSize: CGFloat = 10

    /// Generate a consistent color based on agent name
    private var avatarColor: Color {
        let colors: [Color] = [.blue, .purple, .orange, .green, .pink, .indigo, .teal, .cyan, .mint]
        let hash = agentName.hashValue
        return colors[abs(hash) % colors.count]
    }

    /// Get initials from agent name
    private var initials: String {
        let parts = agentName.split(separator: "-")
        if parts.count >= 2 {
            // For names like "claude-code" -> "CC"
            return String(parts.prefix(2).compactMap { $0.first }.map { String($0).uppercased() }.joined())
        } else if let first = agentName.first {
            // Single word -> first two chars
            let chars = agentName.prefix(2)
            return String(chars).uppercased()
        }
        return "?"
    }

    var body: some View {
        Circle()
            .fill(avatarColor.gradient)
            .frame(width: size, height: size)
            .overlay {
                Text(initials)
                    .font(.system(size: fontSize, weight: .semibold))
                    .foregroundStyle(.white)
            }
            .overlay {
                Circle()
                    .stroke(Color(.systemBackground), lineWidth: 2)
            }
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
