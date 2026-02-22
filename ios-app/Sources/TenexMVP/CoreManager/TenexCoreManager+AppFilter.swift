import Foundation

extension TenexCoreManager {
    var appFilterSnapshot: AppGlobalFilterSnapshot {
        AppGlobalFilterSnapshot(
            projectIds: appFilterProjectIds,
            timeWindow: appFilterTimeWindow,
            scheduledEventFilter: appFilterScheduledEvent
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
        return parts.joined(separator: " · ")
    }

    @MainActor
    func updateAppFilter(
        projectIds: Set<String>,
        timeWindow: AppTimeWindow,
        scheduledEvent: ScheduledEventFilter? = nil
    ) {
        let newScheduledEvent = scheduledEvent ?? appFilterScheduledEvent
        guard projectIds != appFilterProjectIds
            || timeWindow != appFilterTimeWindow
            || newScheduledEvent != appFilterScheduledEvent
        else { return }

        appFilterProjectIds = projectIds
        appFilterTimeWindow = timeWindow
        appFilterScheduledEvent = newScheduledEvent
        persistAppFilter()

        // Apply immediately to current in-memory list so selection/UI react instantly.
        let now = UInt64(Date().timeIntervalSince1970)
        conversations = sortedConversations(
            conversations.filter { conversationMatchesAppFilter($0, now: now) }
        )
        updateActiveAgentsState()
        refreshUnansweredAskCount(reason: "updateAppFilter")
        updateAppBadge()

        refreshConversationsForActiveFilter()
    }

    @MainActor
    func resetAppFilterToDefaults() {
        updateAppFilter(projectIds: [], timeWindow: .defaultValue, scheduledEvent: .showAll)
    }

    func matchesAppFilter(projectId: String?, timestamp: UInt64, now: UInt64? = nil) -> Bool {
        let resolvedNow = now ?? UInt64(Date().timeIntervalSince1970)
        return appFilterSnapshot.includes(projectId: projectId, timestamp: timestamp, now: resolvedNow)
    }

    func conversationMatchesAppFilter(_ conversation: ConversationFullInfo, now: UInt64? = nil) -> Bool {
        let projectId = Self.projectId(fromATag: conversation.projectATag)
        let resolvedNow = now ?? UInt64(Date().timeIntervalSince1970)
        return appFilterSnapshot.includes(
            projectId: projectId,
            timestamp: conversation.thread.effectiveLastActivity,
            now: resolvedNow,
            isScheduled: conversation.thread.isScheduled
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
    ) -> (projectIds: Set<String>, timeWindow: AppTimeWindow, scheduledEvent: ScheduledEventFilter) {
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

        return (persistedProjectIds, persistedTimeWindow, persistedScheduledEvent)
    }

    static func persistAppFilter(
        projectIds: Set<String>,
        timeWindow: AppTimeWindow,
        scheduledEvent: ScheduledEventFilter,
        defaults: UserDefaults = .standard
    ) {
        defaults.set(Array(projectIds).sorted(), forKey: Self.appFilterProjectsDefaultsKey)
        defaults.set(timeWindow.rawValue, forKey: Self.appFilterTimeWindowDefaultsKey)
        defaults.set(scheduledEvent.rawValue, forKey: Self.appFilterScheduledEventDefaultsKey)
    }

    func persistAppFilter(defaults: UserDefaults = .standard) {
        Self.persistAppFilter(
            projectIds: appFilterProjectIds,
            timeWindow: appFilterTimeWindow,
            scheduledEvent: appFilterScheduledEvent,
            defaults: defaults
        )
    }
}
