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

            let (p, c, i) = try await (fetchedProjects, fetchedConversations, fetchedInbox)
            let loadMs = (CFAbsoluteTimeGetCurrent() - loadStartedAt) * 1000
            profiler.logEvent(
                "fetchData concurrent loads projects=\(p.count) conversations=\(c.count) inbox=\(i.count) elapsedMs=\(String(format: "%.2f", loadMs))",
                category: .general,
                level: loadMs >= 300 ? .error : .info
            )

            projects = p
            conversations = sortedConversations(c)
            inboxItems = i

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
}
