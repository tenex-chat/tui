import Foundation

extension TenexCoreManager {
    var appFilterSnapshot: AppGlobalFilterSnapshot {
        AppGlobalFilterSnapshot(projectIds: appFilterProjectIds, timeWindow: appFilterTimeWindow)
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
        "\(appFilterTimeWindow.label) · \(appFilterProjectSummaryLabel)"
    }

    @MainActor
    func updateAppFilter(projectIds: Set<String>, timeWindow: AppTimeWindow) {
        guard projectIds != appFilterProjectIds || timeWindow != appFilterTimeWindow else { return }

        appFilterProjectIds = projectIds
        appFilterTimeWindow = timeWindow
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
        updateAppFilter(projectIds: [], timeWindow: .defaultValue)
    }

    func matchesAppFilter(projectId: String?, timestamp: UInt64, now: UInt64? = nil) -> Bool {
        let resolvedNow = now ?? UInt64(Date().timeIntervalSince1970)
        return appFilterSnapshot.includes(projectId: projectId, timestamp: timestamp, now: resolvedNow)
    }

    func conversationMatchesAppFilter(_ conversation: ConversationFullInfo, now: UInt64? = nil) -> Bool {
        let projectId = Self.projectId(fromATag: conversation.projectATag)
        return matchesAppFilter(projectId: projectId, timestamp: conversation.thread.effectiveLastActivity, now: now)
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

    static func loadPersistedAppFilter() -> (projectIds: Set<String>, timeWindow: AppTimeWindow) {
        let defaults = UserDefaults.standard
        let persistedProjectIds = Set(defaults.stringArray(forKey: Self.appFilterProjectsDefaultsKey) ?? [])
        let persistedTimeWindow = defaults.string(forKey: Self.appFilterTimeWindowDefaultsKey)
            .flatMap(AppTimeWindow.init(rawValue:))
            ?? .defaultValue
        return (persistedProjectIds, persistedTimeWindow)
    }

    func persistAppFilter() {
        let defaults = UserDefaults.standard
        defaults.set(Array(appFilterProjectIds).sorted(), forKey: Self.appFilterProjectsDefaultsKey)
        defaults.set(appFilterTimeWindow.rawValue, forKey: Self.appFilterTimeWindowDefaultsKey)
    }
}
