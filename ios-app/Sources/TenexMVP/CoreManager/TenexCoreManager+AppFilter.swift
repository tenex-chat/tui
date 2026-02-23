import Foundation

extension TenexCoreManager {
    var appFilterSnapshot: AppGlobalFilterSnapshot {
        AppGlobalFilterSnapshot(
            projectIds: appFilterProjectIds,
            timeWindow: appFilterTimeWindow,
            scheduledEventFilter: appFilterScheduledEvent,
            statusFilter: appFilterStatus,
            hashtagFilter: appFilterHashtags
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
        hashtags: Set<String>? = nil
    ) {
        let newScheduledEvent = scheduledEvent ?? appFilterScheduledEvent
        let newStatus = status ?? appFilterStatus
        let newHashtags = AppFilterMetadataNormalizer.normalizedHashtags(hashtags ?? appFilterHashtags)

        guard projectIds != appFilterProjectIds
            || timeWindow != appFilterTimeWindow
            || newScheduledEvent != appFilterScheduledEvent
            || newStatus != appFilterStatus
            || newHashtags != appFilterHashtags
        else { return }

        appFilterProjectIds = projectIds
        appFilterTimeWindow = timeWindow
        appFilterScheduledEvent = newScheduledEvent
        appFilterStatus = newStatus
        appFilterHashtags = newHashtags
        persistAppFilter()

        // Apply immediately to the cached base scope so selection/UI react instantly.
        let now = UInt64(Date().timeIntervalSince1970)
        let snapshot = appFilterSnapshot
        let source = appFilterConversationScope.isEmpty ? conversations : appFilterConversationScope
        conversations = sortedConversations(
            source.filter { conversation in
                conversationMatchesAppFilter(conversation, now: now, snapshot: snapshot)
            }
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
            hashtags: Set<String>()
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
        let projectId = Self.projectId(fromATag: conversation.projectATag)
        let resolvedNow = now ?? UInt64(Date().timeIntervalSince1970)
        let resolvedSnapshot = snapshot ?? appFilterSnapshot
        return resolvedSnapshot.includesConversation(
            projectId: projectId,
            timestamp: conversation.thread.effectiveLastActivity,
            now: resolvedNow,
            isScheduled: conversation.thread.isScheduled,
            statusLabel: conversation.thread.statusLabel,
            hashtags: conversation.thread.hashtags
        )
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
        hashtags: Set<String>
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

        return (
            persistedProjectIds,
            persistedTimeWindow,
            persistedScheduledEvent,
            persistedStatus,
            persistedHashtags
        )
    }

    static func persistAppFilter(
        projectIds: Set<String>,
        timeWindow: AppTimeWindow,
        scheduledEvent: ScheduledEventFilter = .defaultValue,
        status: ConversationStatusFilter = .defaultValue,
        hashtags: Set<String> = [],
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
    }

    func persistAppFilter(defaults: UserDefaults = .standard) {
        Self.persistAppFilter(
            projectIds: appFilterProjectIds,
            timeWindow: appFilterTimeWindow,
            scheduledEvent: appFilterScheduledEvent,
            status: appFilterStatus,
            hashtags: appFilterHashtags,
            defaults: defaults
        )
    }
}
