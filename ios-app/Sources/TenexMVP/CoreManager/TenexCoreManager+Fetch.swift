import Foundation

extension TenexCoreManager {
    /// Load initial data from the core (local cache).
    /// Real-time updates come via push-based event callbacks, not polling.
    @MainActor
    func fetchData() async {
        let fetchStartedAt = CFAbsoluteTimeGetCurrent()
        profiler.logEvent("fetchData start", category: .general)
        #if !os(macOS)
        // Keep current iOS/iPadOS behavior: auto-approve pending backends.
        // macOS uses manual approval from Settings > Backends.
        let approveStartedAt = CFAbsoluteTimeGetCurrent()
        do {
            _ = try await safeCore.approveAllPendingBackends()
        } catch {
        }
        let approveMs = (CFAbsoluteTimeGetCurrent() - approveStartedAt) * 1000
        profiler.logEvent(
            "fetchData approveAllPendingBackends elapsedMs=\(String(format: "%.2f", approveMs))",
            category: .general,
            level: approveMs >= 120 ? .error : .info
        )
        #endif

        do {
            let filterSnapshot = appFilterSnapshot
            let loadStartedAt = CFAbsoluteTimeGetCurrent()

            // Fetch all data concurrently
            async let fetchedProjects = safeCore.getProjects()
            async let fetchedConversations = try fetchConversations(for: filterSnapshot)
            async let fetchedInbox = safeCore.getInbox()
            async let fetchedBookmarkedIds = try? safeCore.getBookmarkedIds()

            let (p, c, i) = try await (fetchedProjects, fetchedConversations, fetchedInbox)
            let bIds = await fetchedBookmarkedIds
            let loadMs = (CFAbsoluteTimeGetCurrent() - loadStartedAt) * 1000
            profiler.logEvent(
                "fetchData concurrent loads projects=\(p.count) conversations=\(c.count) inbox=\(i.count) elapsedMs=\(String(format: "%.2f", loadMs))",
                category: .general,
                level: loadMs >= 300 ? .error : .info
            )

            projects = p
            appFilterConversationScope = sortedConversations(c)
            let now = UInt64(Date().timeIntervalSince1970)
            conversations = sortedConversations(
                appFilterConversationScope.filter { conversation in
                    conversationMatchesAppFilter(
                        conversation,
                        now: now,
                        snapshot: filterSnapshot
                    )
                }
            )
            inboxItems = i
            bookmarkedIds = Set(bIds ?? [])

            let validProjectIds = Set(p.map(\.id))
            let prunedProjectIds = appFilterProjectIds.intersection(validProjectIds)
            if prunedProjectIds != appFilterProjectIds {
                appFilterProjectIds = prunedProjectIds
                persistAppFilter()
                refreshConversationsForActiveFilter()
            }
            refreshUnansweredAskCount(reason: "fetchData")

            // Initialize project online status and online agents in parallel, OFF main actor
            // This uses the shared helper to avoid code duplication and ensure consistent behavior
            let statusStartedAt = CFAbsoluteTimeGetCurrent()
            await refreshProjectStatusParallel(for: p)
            let statusMs = (CFAbsoluteTimeGetCurrent() - statusStartedAt) * 1000
            profiler.logEvent(
                "fetchData refreshProjectStatusParallel projects=\(p.count) elapsedMs=\(String(format: "%.2f", statusMs))",
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

    /// Signal that project status has changed (kind:24010 events).
    /// This triggers a refresh of project status data, online status, and agents cache.
    /// Uses task cancellation to prevent stale overwrites from overlapping refreshes.
    @MainActor
    func signalProjectStatusUpdate() {
        // Cancel any existing refresh task to prevent stale results
        projectStatusUpdateTask?.cancel()

        projectStatusUpdateTask = Task { [weak self] in
            guard let self else { return }

            // Fetch projects
            let projects = await safeCore.getProjects()

            // Check for cancellation before continuing
            if Task.isCancelled { return }

            await MainActor.run {
                self.projects = projects
            }

            // Compute status and agents OFF main actor using shared helper
            await self.refreshProjectStatusParallel(for: projects)

            // Final diagnostics update on main actor
            if !Task.isCancelled {
                await MainActor.run {
                    self.signalDiagnosticsUpdate()
                }
            }
        }
    }

    /// Refresh project online status and agents in parallel, OFF the main actor.
    /// This shared helper is used by both signalProjectStatusUpdate() and fetchData()
    /// to eliminate code duplication and ensure consistent behavior.
    /// - Parameter projects: Array of projects to check status for
    func refreshProjectStatusParallel(for projects: [Project]) async {
        let startedAt = CFAbsoluteTimeGetCurrent()
        var statusUpdates: [String: Bool] = [:]
        var agentsUpdates: [String: [ProjectAgent]] = [:]

        // Use withTaskGroup for concurrent project status checks (runs OFF MainActor)
        await withTaskGroup(of: (String, Bool, [ProjectAgent]).self) { group in
            for project in projects {
                group.addTask {
                    // Check cancellation inside each task
                    if Task.isCancelled {
                        return (project.id, false, [])
                    }

                    let isOnline = await self.safeCore.isProjectOnline(projectId: project.id)
                    let agents: [ProjectAgent]
                    if isOnline {
                        agents = (try? await self.safeCore.getOnlineAgents(projectId: project.id)) ?? []
                    } else {
                        agents = []
                    }
                    return (project.id, isOnline, Self.canonicalOnlineAgents(agents))
                }
            }

            // Collect results off-main, then publish once to avoid N UI invalidations.
            for await (projectId, isOnline, agents) in group {
                if Task.isCancelled { continue }
                statusUpdates[projectId] = isOnline
                agentsUpdates[projectId] = agents
            }
        }

        if !Task.isCancelled {
            await MainActor.run {
                var nextProjectOnlineStatus = self.projectOnlineStatus
                nextProjectOnlineStatus.merge(statusUpdates, uniquingKeysWith: { _, new in new })
                if nextProjectOnlineStatus != self.projectOnlineStatus {
                    self.projectOnlineStatus = nextProjectOnlineStatus
                }

                var nextOnlineAgents = self.onlineAgents
                nextOnlineAgents.merge(agentsUpdates, uniquingKeysWith: { _, new in new })
                if nextOnlineAgents != self.onlineAgents {
                    self.onlineAgents = nextOnlineAgents
                }

                // Re-sort projects: online first, then alphabetical
                self.projects.sort { a, b in
                    let aOnline = nextProjectOnlineStatus[a.id] ?? false
                    let bOnline = nextProjectOnlineStatus[b.id] ?? false
                    if aOnline != bOnline { return aOnline }
                    return a.title.localizedCaseInsensitiveCompare(b.title) == .orderedAscending
                }
            }
        }
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "refreshProjectStatusParallel projects=\(projects.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
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
                return lhs.thread.effectiveLastActivity > rhs.thread.effectiveLastActivity
            }
        }
        return updated
    }

    /// Fetch and cache online agents for a specific project.
    /// This shared method eliminates code duplication and ensures consistent agent caching.
    /// FFI work runs off the main thread; only state mutation hops to MainActor.
    /// - Parameter projectId: The ID of the project to fetch agents for
    func fetchAndCacheAgents(for projectId: String) async {
        let startedAt = CFAbsoluteTimeGetCurrent()
        // Perform FFI call OFF the MainActor to avoid UI blocking
        let agents: [ProjectAgent]
        do {
            agents = try await safeCore.getOnlineAgents(projectId: projectId)
        } catch {
            // Cache empty array on failure to prevent stale data
            await MainActor.run { self.setOnlineAgentsCache([], for: projectId) }
            let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
            profiler.logEvent(
                "fetchAndCacheAgents failed projectId=\(projectId) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                category: .general,
                level: .error
            )
            return
        }

        // Only hop to main actor to mutate state
        await MainActor.run {
            self.setOnlineAgentsCache(agents, for: projectId)
        }
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "fetchAndCacheAgents projectId=\(projectId) agents=\(agents.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
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
        let fetched = await safeCore.getMessages(conversationId: conversationId)
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
    func setOnlineAgentsCache(_ agents: [ProjectAgent], for projectId: String) {
        let normalizedAgents = Self.canonicalOnlineAgents(agents)
        if onlineAgents[projectId] == normalizedAgents {
            return
        }
        var updated = onlineAgents
        updated[projectId] = normalizedAgents
        onlineAgents = updated
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

    nonisolated static func canonicalOnlineAgents(_ agents: [ProjectAgent]) -> [ProjectAgent] {
        agents
            .map { agent in
                var normalized = agent
                normalized.tools = agent.tools.sorted()
                return normalized
            }
            .sorted { lhs, rhs in
                if lhs.isPm != rhs.isPm {
                    return lhs.isPm && !rhs.isPm
                }
                if lhs.name != rhs.name {
                    return lhs.name.localizedCaseInsensitiveCompare(rhs.name) == .orderedAscending
                }
                if lhs.pubkey != rhs.pubkey {
                    return lhs.pubkey < rhs.pubkey
                }
                let lhsModel = lhs.model ?? ""
                let rhsModel = rhs.model ?? ""
                if lhsModel != rhsModel {
                    return lhsModel < rhsModel
                }
                return lhs.tools.lexicographicallyPrecedes(rhs.tools)
            }
    }

    @MainActor
    func refreshConversationsForActiveFilter() {
        let snapshot = appFilterSnapshot

        conversationRefreshTask?.cancel()
        profiler.logEvent(
            "refreshConversationsForActiveFilter start projectIds=\(snapshot.projectIds.count) timeWindow=\(snapshot.timeWindow.rawValue) scheduled=\(snapshot.scheduledEventFilter.rawValue)",
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
                let filtered = self.appFilterConversationScope.filter { conversation in
                    self.conversationMatchesAppFilter(
                        conversation,
                        now: now,
                        snapshot: snapshot
                    )
                }
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
            timeFilter: snapshot.timeWindow.coreTimeFilter
        )
        let fetched = try await safeCore.getAllConversations(filter: filter)
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
