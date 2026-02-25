import Foundation

extension TenexCoreManager {
    var appFilterSnapshot: AppGlobalFilterSnapshot {
        AppGlobalFilterSnapshot(
            projectIds: appFilterProjectIds,
            timeWindow: appFilterTimeWindow,
            scheduledEventFilter: appFilterScheduledEvent,
            statusFilter: appFilterStatus,
            hashtagFilter: appFilterHashtags,
            showArchived: appFilterShowArchived
        )
    }

    var isAppFilterDefault: Bool {
        appFilterSnapshot.isDefault
    }

    var appFilterProjectSummaryLabel: String {
        if appFilterProjectIds.isEmpty {
            return "All Projects"
        }
        if appFilterProjectIds.count == 1,
           let id = appFilterProjectIds.first,
           let title = projects.first(where: { $0.id == id })?.title {
            return title
        }
        return "\(appFilterProjectIds.count) Projects"
    }

    var appFilterSummaryLabel: String {
        var parts = [appFilterTimeWindow.label, appFilterProjectSummaryLabel]
        if appFilterScheduledEvent != .showAll {
            parts.append("Sched: \(appFilterScheduledEvent.label)")
        }
        if case .label(let statusLabel) = appFilterStatus {
            parts.append("Status: \(statusLabel)")
        }
        if !appFilterHashtags.isEmpty {
            if appFilterHashtags.count == 1, let hashtag = appFilterHashtags.first {
                parts.append("#\(hashtag)")
            } else {
                parts.append("\(appFilterHashtags.count) Tags")
            }
        }
        if appFilterShowArchived {
            parts.append("Archived")
        }
        return parts.joined(separator: " · ")
    }

    /// Status options available in the current project/time/scheduled/hashtag scope.
    /// Excludes the status facet itself so users can switch status values without dead-ends.
    var appFilterAvailableStatusLabels: [String] {
        let snapshot = appFilterSnapshot
        let scope = appFilterConversationScope.filter { conversation in
            snapshot.includesConversationFacets(
                isScheduled: conversation.thread.isScheduled,
                statusLabel: conversation.thread.statusLabel,
                hashtags: conversation.thread.hashtags,
                includeStatus: false,
                includeHashtags: true
            )
        }

        var valuesByKey: [String: String] = [:]
        for conversation in scope {
            guard let status = AppFilterMetadataNormalizer.normalizedStatusLabel(conversation.thread.statusLabel) else {
                continue
            }
            let key = AppFilterMetadataNormalizer.statusLookupKey(status)
            if valuesByKey[key] == nil {
                valuesByKey[key] = status
            }
        }

        if case .label(let selected) = appFilterStatus {
            let key = AppFilterMetadataNormalizer.statusLookupKey(selected)
            valuesByKey[key] = valuesByKey[key] ?? selected
        }

        return valuesByKey.values.sorted { lhs, rhs in
            lhs.localizedCaseInsensitiveCompare(rhs) == .orderedAscending
        }
    }

    /// Hashtag options available in the current project/time/scheduled/status scope.
    /// Excludes the hashtag facet itself so multi-tag selection remains discoverable.
    var appFilterAvailableHashtags: [String] {
        let snapshot = appFilterSnapshot
        let scope = appFilterConversationScope.filter { conversation in
            snapshot.includesConversationFacets(
                isScheduled: conversation.thread.isScheduled,
                statusLabel: conversation.thread.statusLabel,
                hashtags: conversation.thread.hashtags,
                includeStatus: true,
                includeHashtags: false
            )
        }

        var hashtags = Set<String>()
        for conversation in scope {
            hashtags.formUnion(AppFilterMetadataNormalizer.normalizedHashtags(conversation.thread.hashtags))
        }

        // Keep selected hashtags visible even if the current scope currently has zero matches.
        hashtags.formUnion(appFilterHashtags)

        return hashtags.sorted { lhs, rhs in
            lhs.localizedCaseInsensitiveCompare(rhs) == .orderedAscending
        }
    }

    @MainActor
    func updateAppFilter(
        projectIds: Set<String>,
        timeWindow: AppTimeWindow,
        scheduledEvent: ScheduledEventFilter? = nil,
        status: ConversationStatusFilter? = nil,
        hashtags: Set<String>? = nil,
        showArchived: Bool? = nil
    ) {
        let newScheduledEvent = scheduledEvent ?? appFilterScheduledEvent
        let newStatus = status ?? appFilterStatus
        let newHashtags = AppFilterMetadataNormalizer.normalizedHashtags(hashtags ?? appFilterHashtags)
        let newShowArchived = showArchived ?? appFilterShowArchived

        guard projectIds != appFilterProjectIds
            || timeWindow != appFilterTimeWindow
            || newScheduledEvent != appFilterScheduledEvent
            || newStatus != appFilterStatus
            || newHashtags != appFilterHashtags
            || newShowArchived != appFilterShowArchived
        else { return }

        appFilterProjectIds = projectIds
        appFilterTimeWindow = timeWindow
        appFilterScheduledEvent = newScheduledEvent
        appFilterStatus = newStatus
        appFilterHashtags = newHashtags
        appFilterShowArchived = newShowArchived
        persistAppFilter()

        // Apply immediately to the cached base scope so selection/UI react instantly.
        let now = UInt64(Date().timeIntervalSince1970)
        let snapshot = appFilterSnapshot
        let source = appFilterConversationScope.isEmpty ? conversations : appFilterConversationScope
        conversations = sortedConversations(
            conversationsMatchingAppFilter(source, now: now, snapshot: snapshot)
        )
        updateActiveAgentsState()
        refreshUnansweredAskCount(reason: "updateAppFilter")
        updateAppBadge()

        refreshConversationsForActiveFilter()
    }

    @MainActor
    func resetAppFilterToDefaults() {
        updateAppFilter(
            projectIds: [],
            timeWindow: .defaultValue,
            scheduledEvent: .defaultValue,
            status: .defaultValue,
            hashtags: Set<String>(),
            showArchived: false
        )
    }

    func matchesAppFilter(projectId: String?, timestamp: UInt64, now: UInt64? = nil) -> Bool {
        let resolvedNow = now ?? UInt64(Date().timeIntervalSince1970)
        return appFilterSnapshot.includes(projectId: projectId, timestamp: timestamp, now: resolvedNow)
    }

    func conversationMatchesAppFilter(
        _ conversation: ConversationFullInfo,
        now: UInt64? = nil,
        snapshot: AppGlobalFilterSnapshot? = nil
    ) -> Bool {
        let resolvedNow = now ?? UInt64(Date().timeIntervalSince1970)
        let resolvedSnapshot = snapshot ?? appFilterSnapshot
        return Self.matchesConversationAppFilter(
            conversation,
            now: resolvedNow,
            snapshot: resolvedSnapshot
        )
    }

    /// Filters conversations by app filter facets and enforces root visibility:
    /// descendants are hidden whenever their root conversation is hidden.
    func conversationsMatchingAppFilter(
        _ conversations: [ConversationFullInfo],
        now: UInt64? = nil,
        snapshot: AppGlobalFilterSnapshot? = nil
    ) -> [ConversationFullInfo] {
        let resolvedNow = now ?? UInt64(Date().timeIntervalSince1970)
        let resolvedSnapshot = snapshot ?? appFilterSnapshot
        return Self.filterConversationsByRootVisibility(
            conversations,
            now: resolvedNow,
            snapshot: resolvedSnapshot
        )
    }

    nonisolated static func matchesConversationAppFilter(
        _ conversation: ConversationFullInfo,
        now: UInt64,
        snapshot: AppGlobalFilterSnapshot
    ) -> Bool {
        let projectId = projectId(fromATag: conversation.projectATag)
        guard snapshot.includes(
            projectId: projectId,
            timestamp: conversation.thread.effectiveLastActivity,
            now: now
        ) else {
            return false
        }

        guard snapshot.includesConversationFacets(
            isScheduled: conversation.thread.isScheduled,
            statusLabel: conversation.thread.statusLabel,
            hashtags: conversation.thread.hashtags
        ) else {
            return false
        }

        if !snapshot.showArchived && conversation.isArchived {
            return false
        }

        return true
    }

    nonisolated static func filterConversationsByRootVisibility(
        _ conversations: [ConversationFullInfo],
        now: UInt64,
        snapshot: AppGlobalFilterSnapshot
    ) -> [ConversationFullInfo] {
        guard !conversations.isEmpty else { return [] }

        let conversationsById = Dictionary(
            uniqueKeysWithValues: conversations.map { ($0.thread.id, $0) }
        )
        var rootIdByConversationId: [String: String] = [:]
        var rootVisibilityById: [String: Bool] = [:]

        func resolveRootId(for conversationId: String) -> String {
            if let cached = rootIdByConversationId[conversationId] {
                return cached
            }

            var currentId = conversationId
            var path: [String] = []
            var seenIds: Set<String> = []

            while true {
                path.append(currentId)
                guard
                    let conversation = conversationsById[currentId],
                    let rawParentId = conversation.thread.parentConversationId
                else {
                    break
                }

                let parentId = rawParentId.trimmingCharacters(in: .whitespacesAndNewlines)
                guard !parentId.isEmpty, conversationsById[parentId] != nil else {
                    break
                }

                if seenIds.contains(parentId) {
                    break
                }

                seenIds.insert(currentId)
                currentId = parentId
            }

            for id in path {
                rootIdByConversationId[id] = currentId
            }
            return currentId
        }

        return conversations.filter { conversation in
            guard matchesConversationAppFilter(conversation, now: now, snapshot: snapshot) else {
                return false
            }

            let rootId = resolveRootId(for: conversation.thread.id)
            if let rootVisible = rootVisibilityById[rootId] {
                return rootVisible
            }

            let rootVisible: Bool
            if let rootConversation = conversationsById[rootId] {
                rootVisible = matchesConversationAppFilter(
                    rootConversation,
                    now: now,
                    snapshot: snapshot
                )
            } else {
                rootVisible = true
            }

            rootVisibilityById[rootId] = rootVisible
            return rootVisible
        }
    }

    func reportMatchesAppFilter(_ report: Report, now: UInt64? = nil) -> Bool {
        let projectId = Self.projectId(fromATag: report.projectATag)
        return matchesAppFilter(projectId: projectId, timestamp: report.createdAt, now: now)
    }

    func inboxItemMatchesAppFilter(_ item: InboxItem, now: UInt64? = nil) -> Bool {
        matchesAppFilter(projectId: item.resolvedProjectId, timestamp: item.createdAt, now: now)
    }

    func searchResultMatchesAppFilter(_ result: SearchResult, now: UInt64? = nil) -> Bool {
        let projectId = result.projectATag.map(Self.projectId(fromATag:))
        return matchesAppFilter(projectId: projectId, timestamp: result.createdAt, now: now)
    }

    static func loadPersistedAppFilter(
        defaults: UserDefaults = .standard
    ) -> (
        projectIds: Set<String>,
        timeWindow: AppTimeWindow,
        scheduledEvent: ScheduledEventFilter,
        status: ConversationStatusFilter,
        hashtags: Set<String>,
        showArchived: Bool
    ) {
        let persistedProjectIds = Set(defaults.stringArray(forKey: Self.appFilterProjectsDefaultsKey) ?? [])
        let persistedTimeWindow = defaults.string(forKey: Self.appFilterTimeWindowDefaultsKey)
            .flatMap(AppTimeWindow.init(rawValue:))
            ?? .defaultValue

        // Load scheduled event filter with migration from legacy "hideScheduled" AppStorage key
        let persistedScheduledEvent: ScheduledEventFilter
        if let rawValue = defaults.string(forKey: Self.appFilterScheduledEventDefaultsKey),
           let filter = ScheduledEventFilter(rawValue: rawValue) {
            persistedScheduledEvent = filter
        } else {
            // Migrate from legacy boolean AppStorage key "hideScheduled"
            let legacyHideScheduled = defaults.bool(forKey: "hideScheduled")
            persistedScheduledEvent = legacyHideScheduled ? .hide : .showAll
        }

        let persistedStatus = ConversationStatusFilter(
            persistedRawValue: defaults.string(forKey: Self.appFilterStatusDefaultsKey)
        )
        let persistedHashtags = AppFilterMetadataNormalizer.normalizedHashtags(
            defaults.stringArray(forKey: Self.appFilterHashtagsDefaultsKey) ?? []
        )
        let persistedShowArchived = defaults.bool(forKey: Self.appFilterShowArchivedDefaultsKey)

        return (
            persistedProjectIds,
            persistedTimeWindow,
            persistedScheduledEvent,
            persistedStatus,
            persistedHashtags,
            persistedShowArchived
        )
    }

    static func persistAppFilter(
        projectIds: Set<String>,
        timeWindow: AppTimeWindow,
        scheduledEvent: ScheduledEventFilter = .defaultValue,
        status: ConversationStatusFilter = .defaultValue,
        hashtags: Set<String> = [],
        showArchived: Bool = false,
        defaults: UserDefaults = .standard
    ) {
        defaults.set(Array(projectIds).sorted(), forKey: Self.appFilterProjectsDefaultsKey)
        defaults.set(timeWindow.rawValue, forKey: Self.appFilterTimeWindowDefaultsKey)
        defaults.set(scheduledEvent.rawValue, forKey: Self.appFilterScheduledEventDefaultsKey)

        if let persistedStatus = status.persistedRawValue {
            defaults.set(persistedStatus, forKey: Self.appFilterStatusDefaultsKey)
        } else {
            defaults.removeObject(forKey: Self.appFilterStatusDefaultsKey)
        }

        let normalizedHashtags = AppFilterMetadataNormalizer.normalizedHashtags(hashtags)
        if normalizedHashtags.isEmpty {
            defaults.removeObject(forKey: Self.appFilterHashtagsDefaultsKey)
        } else {
            defaults.set(normalizedHashtags.sorted(), forKey: Self.appFilterHashtagsDefaultsKey)
        }

        defaults.set(showArchived, forKey: Self.appFilterShowArchivedDefaultsKey)
    }

    func persistAppFilter(defaults: UserDefaults = .standard) {
        Self.persistAppFilter(
            projectIds: appFilterProjectIds,
            timeWindow: appFilterTimeWindow,
            scheduledEvent: appFilterScheduledEvent,
            status: appFilterStatus,
            hashtags: appFilterHashtags,
            showArchived: appFilterShowArchived,
            defaults: defaults
        )
    }
}
