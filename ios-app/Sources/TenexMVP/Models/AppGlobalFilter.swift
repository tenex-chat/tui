import Foundation

/// Three-state filter for scheduled events in conversation lists.
enum ScheduledEventFilter: String, CaseIterable, Codable {
    /// Show all conversations regardless of scheduled status (default)
    case showAll
    /// Hide scheduled conversations from the list
    case hide
    /// Show only scheduled conversations
    case showOnly

    var label: String {
        switch self {
        case .showAll: return "Show All"
        case .hide: return "Hide"
        case .showOnly: return "Show Only"
        }
    }

    /// Returns true if an item with the given scheduled status passes this filter
    func allows(isScheduled: Bool) -> Bool {
        switch self {
        case .showAll: return true
        case .hide: return !isScheduled
        case .showOnly: return isScheduled
        }
    }
}

extension ScheduledEventFilter {
    static let defaultValue: ScheduledEventFilter = .showAll
}

enum AppFilterMetadataNormalizer {
    static func normalizedStatusLabel(_ value: String?) -> String? {
        guard let value else { return nil }
        let normalized = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return normalized.isEmpty ? nil : normalized
    }

    static func normalizedHashtag(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }
        let withoutPrefix = trimmed.hasPrefix("#") ? String(trimmed.dropFirst()) : trimmed
        let normalized = withoutPrefix.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        return normalized.isEmpty ? nil : normalized
    }

    static func normalizedHashtags<S: Sequence>(_ values: S) -> Set<String> where S.Element == String {
        Set(values.compactMap(normalizedHashtag))
    }

    static func statusLookupKey(_ value: String) -> String {
        value.folding(options: [.caseInsensitive, .diacriticInsensitive], locale: .current)
    }
}

enum ConversationStatusFilter: Equatable {
    case all
    case label(String)

    static let defaultValue: ConversationStatusFilter = .all

    init(persistedRawValue: String?) {
        if let label = AppFilterMetadataNormalizer.normalizedStatusLabel(persistedRawValue) {
            self = .label(label)
        } else {
            self = .all
        }
    }

    var persistedRawValue: String? {
        switch self {
        case .all:
            return nil
        case .label(let label):
            return label
        }
    }

    var isDefault: Bool {
        self == .defaultValue
    }

    var displayLabel: String {
        switch self {
        case .all:
            return "All Statuses"
        case .label(let label):
            return label
        }
    }

    func allows(statusLabel: String?) -> Bool {
        switch self {
        case .all:
            return true
        case .label(let selected):
            guard let status = AppFilterMetadataNormalizer.normalizedStatusLabel(statusLabel) else {
                return false
            }
            return AppFilterMetadataNormalizer.statusLookupKey(status)
                == AppFilterMetadataNormalizer.statusLookupKey(selected)
        }
    }
}

/// App-level time window for global filtering across chats/reports/inbox/search.
enum AppTimeWindow: String, CaseIterable, Codable {
    case hours4
    case hours12
    case hours24
    case days7
    case all

    var label: String {
        switch self {
        case .hours4:
            return "4h"
        case .hours12:
            return "12h"
        case .hours24:
            return "24h"
        case .days7:
            return "7d"
        case .all:
            return "All"
        }
    }

    var coreTimeFilter: TimeFilterOption {
        switch self {
        case .hours4, .hours12, .hours24:
            return .today
        case .days7:
            return .thisWeek
        case .all:
            return .all
        }
    }

    var cutoffSeconds: UInt64? {
        switch self {
        case .hours4:
            return 4 * 60 * 60
        case .hours12:
            return 12 * 60 * 60
        case .hours24:
            return 24 * 60 * 60
        case .days7:
            return 7 * 24 * 60 * 60
        case .all:
            return nil
        }
    }

    func cutoffTimestamp(now: UInt64) -> UInt64? {
        guard let cutoffSeconds else { return nil }
        return now > cutoffSeconds ? now - cutoffSeconds : 0
    }

    func includes(timestamp: UInt64, now: UInt64) -> Bool {
        guard let cutoff = cutoffTimestamp(now: now) else { return true }
        return timestamp >= cutoff
    }
}

extension AppTimeWindow {
    static let defaultValue: AppTimeWindow = .hours24
}

struct AppGlobalFilterSnapshot: Equatable {
    let projectIds: Set<String>
    let timeWindow: AppTimeWindow
    let scheduledEventFilter: ScheduledEventFilter
    let statusFilter: ConversationStatusFilter
    let hashtagFilter: Set<String>
    let showArchived: Bool

    init(
        projectIds: Set<String>,
        timeWindow: AppTimeWindow,
        scheduledEventFilter: ScheduledEventFilter = .defaultValue,
        statusFilter: ConversationStatusFilter = .defaultValue,
        hashtagFilter: Set<String> = [],
        showArchived: Bool = false
    ) {
        self.projectIds = projectIds
        self.timeWindow = timeWindow
        self.scheduledEventFilter = scheduledEventFilter
        self.statusFilter = statusFilter
        self.hashtagFilter = AppFilterMetadataNormalizer.normalizedHashtags(hashtagFilter)
        self.showArchived = showArchived
    }

    var isDefault: Bool {
        projectIds.isEmpty
            && timeWindow == .defaultValue
            && scheduledEventFilter == .defaultValue
            && statusFilter.isDefault
            && hashtagFilter.isEmpty
            && !showArchived
    }

    /// Include check for conversations (applies scheduled/status/hashtag filter).
    func includesConversation(
        projectId: String?,
        timestamp: UInt64,
        now: UInt64,
        isScheduled: Bool,
        statusLabel: String?,
        hashtags: [String]
    ) -> Bool {
        includes(projectId: projectId, timestamp: timestamp, now: now)
            && includesConversationFacets(
                isScheduled: isScheduled,
                statusLabel: statusLabel,
                hashtags: hashtags
            )
    }

    /// Include check for conversation facets only (scheduled/status/hashtags).
    /// `includeStatus` and `includeHashtags` allow facet option lists to
    /// derive choices while excluding the facet currently being edited.
    func includesConversationFacets(
        isScheduled: Bool,
        statusLabel: String?,
        hashtags: [String],
        includeStatus: Bool = true,
        includeHashtags: Bool = true
    ) -> Bool {
        guard scheduledEventFilter.allows(isScheduled: isScheduled) else { return false }
        if includeStatus, !statusFilter.allows(statusLabel: statusLabel) {
            return false
        }
        if includeHashtags, !hashtagFilter.isEmpty {
            let normalizedHashtags = AppFilterMetadataNormalizer.normalizedHashtags(hashtags)
            if normalizedHashtags.isDisjoint(with: hashtagFilter) {
                return false
            }
        }
        return true
    }

    /// Include check for non-conversation items (reports, inbox, search).
    /// Applies project/time only by design.
    func includes(projectId: String?, timestamp: UInt64, now: UInt64) -> Bool {
        let matchesProject: Bool
        if projectIds.isEmpty {
            matchesProject = true
        } else if let projectId {
            matchesProject = projectIds.contains(projectId)
        } else {
            matchesProject = false
        }

        return matchesProject
            && timeWindow.includes(timestamp: timestamp, now: now)
    }
}
