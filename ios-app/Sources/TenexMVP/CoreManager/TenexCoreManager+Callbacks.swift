import Foundation

extension TenexCoreManager {
    /// Record that the user was active in a conversation (for TTS inactivity gating).
    func recordUserActivity(conversationId: String) {
        lastUserActivityByConversation[conversationId] = UInt64(Date().timeIntervalSince1970)
    }

    /// Register the event callback for push-based updates.
    /// Call this after successful login to enable real-time updates.
    func registerEventCallback() async {
        sessionStartTimestamp = UInt64(Date().timeIntervalSince1970)
        let handler = TenexEventHandler(coreManager: self)
        eventHandler = handler
        await core.setEventCallback(callback: handler)
        profiler.logEvent(
            "event callback registered sessionStart=\(sessionStartTimestamp)",
            category: .general
        )
        // Start observing APNs token delivery so we can publish kind:25000 events.
        registerApnsObserver()
        // Watch for network path changes so we can reconnect when connectivity returns.
        startNetworkMonitoring()
    }

    /// Unregister the event callback.
    /// Call this on logout to clean up resources.
    func unregisterEventCallback() async {
        // Stop APNs observer and publish deregistration event before clearing core state.
        unregisterApnsObserver()
        stopNetworkMonitoring()
        await core.clearEventCallback()
        eventHandler = nil
        projectStatusUpdateTask?.cancel()
        conversationRefreshTask?.cancel()
        streamingFlushTask?.cancel()
        streamingFlushTask = nil
        pendingStreamingDeltas.removeAll(keepingCapacity: true)
        profiler.logEvent("event callback unregistered", category: .general)
    }

    /// Reconnect all relays, restart subscriptions, and refresh UI data.
    /// Called on foreground return and when network reachability is restored.
    /// Debounced to 5 seconds to prevent double-reconnects from overlapping triggers.
    func reconnectAndRefresh() async {
        let now = CFAbsoluteTimeGetCurrent()
        guard now - lastReconnectAt >= 5.0 else {
            profiler.logEvent("reconnectAndRefresh throttled", category: .general, level: .debug)
            return
        }
        lastReconnectAt = now

        let startedAt = now
        let core = self.core
        await Task.detached {
            try? await core.forceReconnect()
        }.value
        await fetchData()
        refreshRuntimeText()
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "reconnectAndRefresh complete elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 500 ? .error : .info
        )
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
        // Run refresh() via Task.detached to avoid blocking the TenexCoreActor actor queue,
        // which would cause priority inversion for lightweight reads.
        let core = self.core
        await Task.detached {
            _ = await core.refresh()
        }.value
        await fetchData()
        refreshRuntimeText()
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
        profiler.logEvent(
            "manualRefresh complete elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .general,
            level: elapsedMs >= 500 ? .error : .info
        )
    }

    // MARK: - Push-Based Delta Application
    // These methods update @Published properties directly from Rust callbacks.

    @MainActor
    func applyMessageAppended(conversationId: String, message: Message) {
        guard var messages = messagesByConversation[conversationId] else {
            return
        }
        guard !messages.contains(where: { $0.id == message.id }) else {
            return
        }

        if let last = messages.last, last.createdAt <= message.createdAt {
            messages.append(message)
        } else {
            messages.append(message)
            messages.sort { $0.createdAt < $1.createdAt }
        }
        setMessagesCache(messages, for: conversationId)
    }

    @MainActor
    func applyConversationUpsert(_ conversation: ConversationFullInfo) {
        let snapshot = appFilterSnapshot
        let now = UInt64(Date().timeIntervalSince1970)
        let projectId = Self.projectId(fromATag: conversation.projectATag)
        let isInBaseScope = snapshot.includes(
            projectId: projectId,
            timestamp: conversation.thread.effectiveLastActivity,
            now: now
        )

        var updatedScope = appFilterConversationScope
        if let index = updatedScope.firstIndex(where: { $0.thread.id == conversation.thread.id }) {
            if !isInBaseScope {
                updatedScope.remove(at: index)
            } else if updatedScope[index] == conversation {
                return
            } else {
                updatedScope[index] = conversation
            }
        } else if isInBaseScope {
            updatedScope.append(conversation)
        } else {
            return
        }

        appFilterConversationScope = sortedConversations(updatedScope)
        let filtered = sortedConversations(
            conversationsMatchingAppFilter(
                appFilterConversationScope,
                now: now,
                snapshot: snapshot
            )
        )
        if filtered != conversations {
            conversations = filtered
            updateActiveAgentsState()
        }
    }

    /// Apply a conversation upsert from callback without triggering a full conversation-list rebuild.
    /// This path clears ephemeral stream-delta state (kind:24135), applies the authoritative
    /// kind:1 update, and only refreshes messages when already cached.
    @MainActor
    func applyConversationUpsertDelta(_ conversation: ConversationFullInfo) {
        let conversationId = conversation.thread.id
        pendingStreamingDeltas.removeValue(forKey: conversationId)
        streamingBuffers.removeValue(forKey: conversationId)
        applyConversationUpsert(conversation)
        refreshRuntimeText()

        // Avoid expensive message fetches for conversations that are not currently loaded.
        guard let cachedMessages = messagesByConversation[conversationId] else {
            profiler.logEvent(
                "applyConversationUpsertDelta conversationId=\(conversationId) messagesCached=false",
                category: .general,
                level: .debug
            )
            return
        }

        let expectedCount = Int(conversation.messageCount)
        if expectedCount > 0, cachedMessages.count == expectedCount {
            profiler.logEvent(
                "applyConversationUpsertDelta skip refresh conversationId=\(conversationId) cachedCount=\(cachedMessages.count) expectedCount=\(expectedCount)",
                category: .general,
                level: .debug
            )
            return
        }

        let now = CFAbsoluteTimeGetCurrent()
        if let lastRefresh = lastConversationMessageRefreshAt[conversationId], now - lastRefresh < 0.75 {
            profiler.logEvent(
                "applyConversationUpsertDelta throttled conversationId=\(conversationId)",
                category: .general,
                level: .debug
            )
            return
        }
        guard !inflightConversationMessageRefreshes.contains(conversationId) else {
            profiler.logEvent(
                "applyConversationUpsertDelta skip inflight conversationId=\(conversationId)",
                category: .general,
                level: .debug
            )
            return
        }

        inflightConversationMessageRefreshes.insert(conversationId)
        lastConversationMessageRefreshAt[conversationId] = now
        Task {
            let startedAt = CFAbsoluteTimeGetCurrent()
            let messages = await core.getMessages(conversationId: conversationId)
            await MainActor.run {
                self.setMessagesCache(messages, for: conversationId)
                self.inflightConversationMessageRefreshes.remove(conversationId)
                let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
                self.profiler.logEvent(
                    "applyConversationUpsertDelta refreshed messages conversationId=\(conversationId) count=\(messages.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 120 ? .error : .info
                )
            }
        }
    }

    @MainActor
    func applyProjectUpsert(_ project: Project) {
        if project.isDeleted {
            projects.removeAll { $0.id == project.id }
            projectOnlineStatus.removeValue(forKey: project.id)
            projectRosterAgents.removeValue(forKey: project.id)
            if appFilterProjectIds.contains(project.id) {
                appFilterProjectIds.remove(project.id)
                persistAppFilter()
                refreshConversationsForActiveFilter()
                updateAppBadge()
            }
            setLastDeletedProjectId(project.id)
            return
        }

        var updated = projects
        if let index = updated.firstIndex(where: { $0.id == project.id }) {
            updated[index] = project
        } else {
            updated.insert(project, at: 0)
        }
        projects = updated
        Task {
            await refreshProjectRosterState(for: updated)
        }
    }

    @MainActor
    func applyInboxUpsert(_ item: InboxItem) {
        var updated = inboxItems
        if let index = updated.firstIndex(where: { $0.id == item.id }) {
            updated[index] = item
        } else {
            updated.append(item)
        }
        updated.sort { $0.createdAt > $1.createdAt }
        inboxItems = updated

        refreshUnansweredAskCount(reason: "applyInboxUpsert")
        // Update app badge with unanswered ask count
        updateAppBadge()
    }

    /// Update the app icon badge with unanswered ask count.
    @MainActor
    func updateAppBadge() {
        let count = unansweredAskCount
        Task {
            await NotificationService.shared.updateBadge(count: count)
        }
    }

    @MainActor
    func applyProjectStatusChanged(projectId: String, projectATag: String, isOnline: Bool) {
        let resolvedProjectId: String = {
            if !projectId.isEmpty {
                return projectId
            }
            return Self.projectId(fromATag: projectATag)
        }()

        guard !resolvedProjectId.isEmpty else { return }

        setProjectOnlineStatus(isOnline, for: resolvedProjectId)

        sortProjectsByAvailability()
        profiler.logEvent(
            "applyProjectStatusChanged projectId=\(resolvedProjectId) isOnline=\(isOnline)",
            category: .general,
            level: .debug
        )
        signalDiagnosticsUpdate()
    }

    @MainActor
    func applyProjectRosterChanged(projectId: String, projectATag: String, agents: [ProjectAgent]) {
        let resolvedProjectId: String = {
            if !projectId.isEmpty {
                return projectId
            }
            return Self.projectId(fromATag: projectATag)
        }()

        guard !resolvedProjectId.isEmpty else { return }

        setProjectRosterCache(agents, for: resolvedProjectId)
        signalDiagnosticsUpdate()
    }

    @MainActor
    func applyActiveConversationsChanged(projectId: String, projectATag: String, activeConversationIds: [String]) {
        let normalizedProjectId = projectId.trimmingCharacters(in: .whitespacesAndNewlines)
        let normalizedProjectATag = projectATag.trimmingCharacters(in: .whitespacesAndNewlines)
        let resolvedProjectId = !normalizedProjectId.isEmpty ? normalizedProjectId : Self.projectId(fromATag: normalizedProjectATag)
        guard !resolvedProjectId.isEmpty || !normalizedProjectATag.isEmpty else {
            return
        }

        let activeConversationIdSet = Set(activeConversationIds)
        var updated = appFilterConversationScope
        var didChange = false

        for index in updated.indices {
            let conversation = updated[index]
            let conversationProjectId = Self.projectId(fromATag: conversation.projectATag)
            let matchesProjectATag = !normalizedProjectATag.isEmpty && conversation.projectATag == normalizedProjectATag
            let matchesProjectId = !resolvedProjectId.isEmpty && conversationProjectId == resolvedProjectId

            guard matchesProjectATag || matchesProjectId else { continue }

            let shouldBeActive = activeConversationIdSet.contains(conversation.thread.id)
            if conversation.isActive != shouldBeActive {
                updated[index].isActive = shouldBeActive
                didChange = true
            }
        }

        if didChange {
            appFilterConversationScope = sortedConversations(updated)
            let now = UInt64(Date().timeIntervalSince1970)
            let snapshot = appFilterSnapshot
            conversations = sortedConversations(
                conversationsMatchingAppFilter(
                    appFilterConversationScope,
                    now: now,
                    snapshot: snapshot
                )
            )
            updateActiveAgentsState()
            refreshRuntimeText()
        }
    }

    @MainActor
    func handlePendingBackendApproval(backendPubkey: String, projectATag: String) {
        #if os(macOS)
        // Manual approval on macOS: keep backend pending and surface it in Settings > Backends.
        signalDiagnosticsUpdate()
        return
        #else
        Task {
            await reloadPendingBackendApprovalPrompts()
        }
        #endif
    }

    @MainActor
    func applyStreamChunk(agentPubkey: String, conversationId: String, textDelta: String?) {
        // Stream chunks are ordered/coalesced in Rust from Nostr ephemeral kind:24135 events.
        guard let delta = textDelta, !delta.isEmpty else { return }

        if var pending = pendingStreamingDeltas[conversationId] {
            pending.text.append(delta)
            pending.chunkCount += 1
            pendingStreamingDeltas[conversationId] = pending
        } else {
            pendingStreamingDeltas[conversationId] = PendingStreamingDelta(
                agentPubkey: agentPubkey,
                text: delta,
                chunkCount: 1,
                startedAt: CFAbsoluteTimeGetCurrent()
            )
        }

        scheduleStreamingFlushIfNeeded()
    }

    @MainActor
    func signalStatsUpdate() {
        bumpStatsVersion()
    }

    @MainActor
    func signalDiagnosticsUpdate() {
        bumpDiagnosticsVersion()
    }

    @MainActor
    func signalTeamsUpdate() {
        bumpTeamsVersion()
    }

    @MainActor
    func signalContentCatalogUpdate() {
        bumpContentCatalogVersion()
    }

    @MainActor
    func applyInstalledAgentsChanged() {
        bumpAgentInventoryVersion()
        // Clear cached profile names so that kind:0 events arriving via the
        // subscriptions set up after inventory changes are picked up on next render.
        invalidateProfileNameCache()
        Task {
            await refreshProjectRosterState()
        }
        signalDiagnosticsUpdate()
    }

    /// Signal that messages for a specific conversation have been updated.
    /// This triggers a refresh of the conversation's messages.
    @MainActor
    func signalConversationUpdate(conversationId: String) {
        pendingStreamingDeltas.removeValue(forKey: conversationId)
        streamingBuffers.removeValue(forKey: conversationId)
        profiler.logEvent(
            "signalConversationUpdate conversationId=\(conversationId)",
            category: .general,
            level: .debug
        )
        Task {
            let startedAt = CFAbsoluteTimeGetCurrent()
            // Refresh messages for this specific conversation
            let messages = await core.getMessages(conversationId: conversationId)
            let refreshedConversation = await core.getConversationsByIds(conversationIds: [conversationId]).first
            await MainActor.run {
                self.setMessagesCache(messages, for: conversationId)
                if let refreshedConversation {
                    self.applyConversationUpsert(refreshedConversation)
                } else {
                    self.conversations.removeAll { $0.thread.id == conversationId }
                    self.updateActiveAgentsState()
                }
                self.inflightConversationMessageRefreshes.remove(conversationId)
                let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
                self.profiler.logEvent(
                    "signalConversationUpdate complete conversationId=\(conversationId) messages=\(messages.count) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                    category: .general,
                    level: elapsedMs >= 150 ? .error : .info
                )
            }
        }
    }

    @MainActor
    private func scheduleStreamingFlushIfNeeded() {
        guard streamingFlushTask == nil else { return }

        streamingFlushTask = Task { @MainActor [weak self] in
            guard let self else { return }
            while !Task.isCancelled {
                try? await Task.sleep(nanoseconds: 80_000_000) // ~12.5 FPS publish cap
                guard !self.pendingStreamingDeltas.isEmpty else { break }
                self.flushPendingStreamingDeltas()
            }
            self.streamingFlushTask = nil
        }
    }

    @MainActor
    private func flushPendingStreamingDeltas() {
        guard !pendingStreamingDeltas.isEmpty else { return }

        let flushStartedAt = CFAbsoluteTimeGetCurrent()
        let pendingConversationCount = pendingStreamingDeltas.count
        var updatedBuffers = streamingBuffers
        var totalChars = 0
        var totalChunks = 0
        var maxQueuedMs: Double = 0

        for (conversationId, pending) in pendingStreamingDeltas {
            var buffer = updatedBuffers[conversationId] ?? StreamingBuffer(agentPubkey: pending.agentPubkey, text: "")
            buffer.text.append(pending.text)
            updatedBuffers[conversationId] = buffer
            totalChars += pending.text.count
            totalChunks += pending.chunkCount
            let queuedMs = (flushStartedAt - pending.startedAt) * 1000
            if queuedMs > maxQueuedMs {
                maxQueuedMs = queuedMs
            }
        }

        pendingStreamingDeltas.removeAll(keepingCapacity: true)
        streamingBuffers = updatedBuffers

        let elapsedMs = (CFAbsoluteTimeGetCurrent() - flushStartedAt) * 1000
        profiler.logEvent(
            "flushPendingStreamingDeltas conversations=\(pendingConversationCount) chunks=\(totalChunks) chars=\(totalChars) maxQueuedMs=\(String(format: "%.2f", maxQueuedMs)) elapsedMs=\(String(format: "%.2f", elapsedMs))",
            category: .swiftUI,
            level: totalChunks >= 64 ? .debug : .info
        )
    }

    /// Signal a general update - used when the change type is not specific.
    /// This triggers a refresh of core data.
    @MainActor
    func signalGeneralUpdate() {
        bumpDiagnosticsVersion()
    }

    // MARK: - Bunker (NIP-46)

    @MainActor
    func handleBunkerSignRequest(_ request: FfiBunkerSignRequest) {
        pendingBunkerRequests.append(request)
        signalDiagnosticsUpdate()
    }

    @MainActor
    func approveBunkerRequest(requestId: String) {
        pendingBunkerRequests.removeAll { $0.requestId == requestId }
        signalDiagnosticsUpdate()
        Task {
            try? await core.respondToBunkerRequest(requestId: requestId, approved: true)
            await MainActor.run {
                self.signalDiagnosticsUpdate()
            }
        }
    }

    @MainActor
    func rejectBunkerRequest(requestId: String) {
        pendingBunkerRequests.removeAll { $0.requestId == requestId }
        signalDiagnosticsUpdate()
        Task {
            try? await core.respondToBunkerRequest(requestId: requestId, approved: false)
            await MainActor.run {
                self.signalDiagnosticsUpdate()
            }
        }
    }
}
