import Foundation

extension TenexCoreManager {
    /// Load initial data from the core (local cache).
    /// Real-time updates come via push-based event callbacks, not polling.
    @MainActor
    func fetchData() async {
        let fetchStartedAt = CFAbsoluteTimeGetCurrent()
        profiler.logEvent("fetchData start", category: .general)

        do {
            await refreshWorkspacesFromCore()
            syncActiveWorkspaceFilterFromState()
            let filterSnapshot = appFilterSnapshot
            let loadStartedAt = CFAbsoluteTimeGetCurrent()

            // Fetch all data concurrently
            async let fetchedProjects = core.getProjects()
            async let fetchedConversations = try fetchConversations(for: filterSnapshot)
            async let fetchedInbox = core.getInbox()
            async let fetchedReports = core.getReports(projectId: "")
            async let fetchedHtmlReports = core.getHtmlReports(projectId: "")

            let (p, c, i) = try await (fetchedProjects, fetchedConversations, fetchedInbox)
            let r = await fetchedReports
            let hr = await fetchedHtmlReports
            let loadMs = (CFAbsoluteTimeGetCurrent() - loadStartedAt) * 1000
            profiler.logEvent(
                "fetchData concurrent loads projects=\(p.count) conversations=\(c.count) inbox=\(i.count) reports=\(r.count) htmlReports=\(hr.count) elapsedMs=\(String(format: "%.2f", loadMs))",
                category: .general,
                level: loadMs >= 300 ? .error : .info
            )

            projects = p
            reports = r
            reportsVersion &+= 1
            htmlReports = hr
            htmlReportsVersion &+= 1
            prefetchHtmlReports(hr)
            await reloadPendingBackendApprovalPrompts()
            appFilterConversationScope = sortedConversations(c)
            let now = UInt64(Date().timeIntervalSince1970)
            conversations = sortedConversations(
                conversationsMatchingAppFilter(
                    appFilterConversationScope,
                    now: now,
                    snapshot: filterSnapshot
                )
            )
            inboxItems = i

            let validProjectIds = Set(p.map(\.id))
            let prunedProjectIds = appFilterProjectIds.intersection(validProjectIds)
            if prunedProjectIds != appFilterProjectIds {
                appFilterProjectIds = prunedProjectIds
                persistAppFilter()
                refreshConversationsForActiveFilter()
            }
            refreshUnansweredAskCount(reason: "fetchData")

            // Initialize project liveness and ordered roster projections from core.
            let statusStartedAt = CFAbsoluteTimeGetCurrent()
            await refreshProjectRosterState(for: p)
            let statusMs = (CFAbsoluteTimeGetCurrent() - statusStartedAt) * 1000
            profiler.logEvent(
                "fetchData refreshProjectRosterState projects=\(p.count) elapsedMs=\(String(format: "%.2f", statusMs))",
                category: .general,
                level: statusMs >= 300 ? .error : .info
            )

            updateActiveAgentsState()
            refreshRuntimeText()
            signalStatsUpdate()
            signalDiagnosticsUpdate()
            updateAppBadge()
            let totalMs = (CFAbsoluteTimeGetCurrent() - fetchStartedAt) * 1000
            profiler.logEvent(
                "fetchData complete projects=\(projects.count) conversations=\(conversations.count) inbox=\(inboxItems.count) totalMs=\(String(format: "%.2f", totalMs))",
                category: .general,
                level: totalMs >= 500 ? .error : .info
            )
        } catch {
            // Don't crash - just log and continue with stale data
            let totalMs = (CFAbsoluteTimeGetCurrent() - fetchStartedAt) * 1000
            profiler.logEvent(
                "fetchData failed elapsedMs=\(String(format: "%.2f", totalMs)) error=\(error.localizedDescription)",
                category: .general,
                level: .error
            )
        }
    }

    /// Signal that project-related cached data should be refreshed.
    /// The roster remains core-projected; liveness is updated from project status.
    /// Uses task cancellation to prevent stale overwrites from overlapping refreshes.
    @MainActor
    func signalProjectStatusUpdate() {
        // Cancel any existing refresh task to prevent stale results
        projectStatusUpdateTask?.cancel()

        projectStatusUpdateTask = Task { [weak self] in
            guard let self else { return }

            // Fetch projects
            let projects = await core.getProjects()

            // Check for cancellation before continuing
            if Task.isCancelled { return }

            await MainActor.run {
                self.projects = projects
            }

            await self.refreshProjectRosterState(for: projects)

            // Final diagnostics update on main actor
            if !Task.isCancelled {
                await MainActor.run {
                    self.signalDiagnosticsUpdate()
                }
            }
        }
    }

    /// Refresh the ordered project roster cache from the core projection.
    /// Core owns the merge of kind:31933 membership/order, kind:24011 per-agent availability,
    /// and kind:0 config/display metadata. Swift only caches the projected rows and the separate
    /// project liveness bit from kind:24010.
    /// - Parameter projects: Array of projects to rebuild roster rows for.
    func refreshProjectRosterState(for projects: [Project]? = nil) async {
        let startedAt = CFAbsoluteTimeGetCurrent()
        let projects = projects ?? self.projects
        let inventory = (try? await core.getAgentInventory()) ?? []
        var statusUpdates: [String: Bool] = [:]
        var rosterUpdates: [String: [ProjectAgent]] = [:]

        // Fetch one projected roster per project. The returned row order is exactly the 31933 p-tag order.
        for project in projects {
            if Task.isCancelled { break }
            if let roster = try? await core.getProjectRoster(projectId: project.id) {
                rosterUpdates[project.id] = Self.canonicalRosterAgents(roster)
            }
            // Project online ⇔ a backend has sent a fresh kind:24010 heartbeat. Do not use the
            // per-agent isOnline flag; that reflects approved 24011 inventory availability.
            statusUpdates[project.id] = await core.isProjectOnline(projectId: project.id)
        }

        if !Task.isCancelled {
            await MainActor.run {
                let sortedInventory = self.sortedAgentInventory(inventory)
                if sortedInventory != self.agentInventory {
                    self.agentInventory = sortedInventory
                }

                var nextProjectOnlineStatus = self.projectOnlineStatus
                nextProjectOnlineStatus.merge(statusUpdates, uniquingKeysWith: { _, new in new })
                if nextProjectOnlineStatus != self.projectOnlineStatus {
                    self.projectOnlineStatus = nextProjectOnlineStatus
                }

                var nextProjectRosterAgents = self.projectRosterAgents
                nextProjectRosterAgents.merge(rosterUpdates, uniquingKeysWith: { _, new in new })
                if nextProjectRosterAgents != self.projectRosterAgents {
                    self.projectRosterAgents = nextProjectRosterAgents
                }

                // Re-sort projects: available first, then alphabetical.
                self.projects.sort { a, b in
                    Self.projectSortPrecedes(a, b, onlineStatus: nextProjectOnlineStatus)
                }
            }
        }
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "refreshProjectRosterState projects=\(projects.count) inventory=\(inventory.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 200 ? .error : .info
        )
    }

    /// Update hasActiveAgents based on current conversations
    @MainActor
    func updateActiveAgentsState() {
        hasActiveAgents = conversations.contains { $0.isActive }
    }

    func sortedConversations(_ items: [ConversationFullInfo]) -> [ConversationFullInfo] {
        var updated = items
        updated.sort { lhs, rhs in
            switch (lhs.isActive, rhs.isActive) {
            case (true, false):
                return true
            case (false, true):
                return false
            default:
                // Bucket by 60-second windows to prevent near-simultaneous activity from
                // causing conversations to jump positions. Within the same bucket, use
                // alphabetical event ID for stable, deterministic ordering.
                let lhsBucket = lhs.thread.effectiveLastActivity / 60
                let rhsBucket = rhs.thread.effectiveLastActivity / 60
                if lhsBucket != rhsBucket {
                    return lhsBucket > rhsBucket
                }
                return lhs.thread.id < rhs.thread.id
            }
        }
        return updated
    }

    /// Refresh and cache the 31933 roster for a specific project.
    /// - Parameter projectId: The ID of the project to rebuild.
    func fetchAndCacheAgents(for projectId: String) async {
        let startedAt = CFAbsoluteTimeGetCurrent()
        guard let project = projects.first(where: { $0.id == projectId }) else {
            setProjectRosterCache([], for: projectId)
            setProjectOnlineStatus(false, for: projectId)
            return
        }
        await refreshProjectRosterState(for: [project])
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "fetchAndCacheAgents projectId=\(projectId) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 120 ? .error : .info
        )
    }

    @MainActor
    func ensureMessagesLoaded(conversationId: String) async {
        if messagesByConversation[conversationId] != nil {
            profiler.logEvent(
                "ensureMessagesLoaded cache-hit conversationId=\(conversationId)",
                category: .general,
                level: .debug
            )
            return
        }
        let startedAt = CFAbsoluteTimeGetCurrent()
        let fetched = await core.getMessages(conversationId: conversationId)
        mergeMessagesCache(fetched, for: conversationId)
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "ensureMessagesLoaded cache-miss conversationId=\(conversationId) fetched=\(fetched.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 120 ? .error : .info
        )
    }

    @MainActor
    func setMessagesCache(_ messages: [Message], for conversationId: String) {
        if messagesByConversation[conversationId] == messages {
            return
        }
        messagesByConversation[conversationId] = messages
    }

    @MainActor
    private func mergeMessagesCache(_ messages: [Message], for conversationId: String) {
        var combined = messagesByConversation[conversationId] ?? []
        if combined.isEmpty {
            combined = messages
        } else {
            let existingIds = Set(combined.map { $0.id })
            combined.append(contentsOf: messages.filter { !existingIds.contains($0.id) })
        }
        combined.sort { $0.createdAt < $1.createdAt }
        setMessagesCache(combined, for: conversationId)
    }

    @MainActor
    func setProjectRosterCache(_ agents: [ProjectAgent], for projectId: String) {
        let normalizedAgents = Self.canonicalRosterAgents(agents)
        if projectRosterAgents[projectId] == normalizedAgents {
            return
        }
        var updated = projectRosterAgents
        updated[projectId] = normalizedAgents
        projectRosterAgents = updated
    }

    @MainActor
    func setProjectOnlineStatus(_ isOnline: Bool, for projectId: String) {
        if projectOnlineStatus[projectId] == isOnline {
            return
        }
        var updated = projectOnlineStatus
        updated[projectId] = isOnline
        projectOnlineStatus = updated
    }

    nonisolated static func canonicalRosterAgents(_ agents: [ProjectAgent]) -> [ProjectAgent] {
        agents.map { agent in
            var normalized = agent
            normalized.tools = agent.tools.sorted()
            normalized.skills = agent.skills.sorted()
            normalized.mcpServers = agent.mcpServers.sorted()
            return normalized
        }
    }

    nonisolated static func projectSortPrecedes(
        _ lhs: Project,
        _ rhs: Project,
        onlineStatus: [String: Bool]
    ) -> Bool {
        let lhsOnline = onlineStatus[lhs.id] ?? false
        let rhsOnline = onlineStatus[rhs.id] ?? false
        if lhsOnline != rhsOnline { return lhsOnline }
        return lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
    }

    @MainActor
    func sortProjectsByAvailability() {
        projects.sort { lhs, rhs in
            Self.projectSortPrecedes(lhs, rhs, onlineStatus: projectOnlineStatus)
        }
    }

    private func sortedAgentInventory(_ inventory: [AgentInventoryItem]) -> [AgentInventoryItem] {
        inventory.sorted {
            let lhsName = AgentDisplayName.resolve(pubkey: $0.pubkey, coreManager: self)
            let rhsName = AgentDisplayName.resolve(pubkey: $1.pubkey, coreManager: self)
            let comparison = lhsName.localizedCaseInsensitiveCompare(rhsName)
            if comparison != .orderedSame {
                return comparison == .orderedAscending
            }
            return $0.pubkey < $1.pubkey
        }
    }

    @MainActor
    func refreshConversationsForActiveFilter() {
        let snapshot = appFilterSnapshot

        conversationRefreshTask?.cancel()
        profiler.logEvent(
            "refreshConversationsForActiveFilter start projectIds=\(snapshot.projectIds.count) timeWindow=\(snapshot.timeWindow.rawValue) scheduled=\(snapshot.scheduledEventFilter.rawValue) intervention=\(snapshot.interventionReviewFilter.rawValue)",
            category: .general,
            level: .debug
        )
        conversationRefreshTask = Task { [weak self] in
            guard let self else { return }
            let startedAt = CFAbsoluteTimeGetCurrent()

            guard let refreshed = try? await self.fetchConversations(for: snapshot) else { return }
            guard !Task.isCancelled else { return }

            await MainActor.run {
                guard self.appFilterSnapshot == snapshot else { return }
                self.appFilterConversationScope = self.sortedConversations(refreshed)
                let now = UInt64(Date().timeIntervalSince1970)
                let filtered = self.conversationsMatchingAppFilter(
                    self.appFilterConversationScope,
                    now: now,
                    snapshot: snapshot
                )
                self.conversations = self.sortedConversations(filtered)
                self.updateActiveAgentsState()
                let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
                self.profiler.logEvent(
                    "refreshConversationsForActiveFilter complete base=\(refreshed.count) filtered=\(filtered.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 180 ? .error : .info
                )
            }
        }
    }

    func fetchConversations(for snapshot: AppGlobalFilterSnapshot) async throws -> [ConversationFullInfo] {
        let startedAt = CFAbsoluteTimeGetCurrent()
        let filter = ConversationFilter(
            projectIds: Array(snapshot.projectIds),
            showArchived: true,
            hideScheduled: false,
            hideInterventionReview: snapshot.interventionReviewFilter == .hide,
            timeFilter: snapshot.timeWindow.coreTimeFilter
        )
        let fetched = try await core.getAllConversations(filter: filter)
        guard !Task.isCancelled else { return [] }
        let now = UInt64(Date().timeIntervalSince1970)
        let baseFiltered = fetched.filter { conversation in
            let projectId = Self.projectId(fromATag: conversation.projectATag)
            return snapshot.includes(projectId: projectId, timestamp: conversation.thread.effectiveLastActivity, now: now)
        }
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "fetchConversations projectIds=\(snapshot.projectIds.count) fetched=\(fetched.count) baseFiltered=\(baseFiltered.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 180 ? .error : .info
        )
        return baseFiltered
    }
}
