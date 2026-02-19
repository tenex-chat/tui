import Foundation

extension TenexCoreManager {
    /// Record that the user was active in a conversation (for TTS inactivity gating).
    func recordUserActivity(conversationId: String) {
        lastUserActivityByConversation[conversationId] = UInt64(Date().timeIntervalSince1970)
    }

    /// Register the event callback for push-based updates.
    /// Call this after successful login to enable real-time updates.
    func registerEventCallback() {
        sessionStartTimestamp = UInt64(Date().timeIntervalSince1970)
        let handler = TenexEventHandler(coreManager: self)
        eventHandler = handler
        core.setEventCallback(callback: handler)
        profiler.logEvent(
            "event callback registered sessionStart=\(sessionStartTimestamp)",
            category: .general
        )
    }

    /// Unregister the event callback.
    /// Call this on logout to clean up resources.
    func unregisterEventCallback() {
        core.clearEventCallback()
        eventHandler = nil
        projectStatusUpdateTask?.cancel()
        conversationRefreshTask?.cancel()
        streamingFlushTask?.cancel()
        streamingFlushTask = nil
        pendingStreamingDeltas.removeAll(keepingCapacity: true)
        profiler.logEvent("event callback unregistered", category: .general)
    }

    /// Manual refresh for pull-to-refresh gesture.
    ///
    /// This performs a full reconnection to relays to ensure fresh data is fetched.
    /// Unlike the automatic refresh which only drains pending events, this:
    /// 1. Disconnects from all relays
    /// 2. Reconnects with the same credentials
    /// 3. Restarts all subscriptions
    /// 4. Triggers a new negentropy sync to fetch any missed events
    /// 5. Refreshes all data from the store
    func manualRefresh() async {
        let startedAt = CFAbsoluteTimeGetCurrent()
        _ = await safeCore.refresh()
        await fetchData()
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "manualRefresh complete elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 500 ? .error : .info
        )
    }

    /// Signal a general update - used when the change type is not specific.
    /// This triggers a refresh of core data.
    @MainActor
    func signalGeneralUpdate() {
        bumpDiagnosticsVersion()
    }
}
