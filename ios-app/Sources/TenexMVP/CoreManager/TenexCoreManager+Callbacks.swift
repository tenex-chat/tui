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
        // Run refresh() via Task.detached to avoid blocking the SafeTenexCore actor queue,
        // which would cause priority inversion for lightweight reads.
        let core = self.core
        await Task.detached {
            _ = core.refresh()
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
        var updated = conversations
        guard conversationMatchesAppFilter(conversation) else {
            let initialCount = updated.count
            updated.removeAll { $0.thread.id == conversation.thread.id }
            guard updated.count != initialCount else {
                return
            }
            conversations = sortedConversations(updated)
            updateActiveAgentsState()
            return
        }
        if let index = updated.firstIndex(where: { $0.thread.id == conversation.thread.id }) {
            if updated[index] == conversation {
                return
            }
            updated[index] = conversation
        } else {
            updated.append(conversation)
        }
        let sorted = sortedConversations(updated)
        if sorted != conversations {
            conversations = sorted
            updateActiveAgentsState()
        }
    }

    /// Apply a conversation upsert from callback without triggering a full conversation-list rebuild.
    /// This path clears streaming state, applies the delta, and only refreshes messages when already cached.
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
            let messages = await safeCore.getMessages(conversationId: conversationId)
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
            onlineAgents.removeValue(forKey: project.id)
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
    func applyReportUpsert(_ report: Report) {
        var updated = reports
        if let index = updated.firstIndex(where: { $0.slug == report.slug && $0.projectATag == report.projectATag }) {
            updated[index] = report
        } else {
            updated.append(report)
        }
        // Sort by created date (newest first)
        updated.sort { $0.createdAt > $1.createdAt }
        reports = updated
    }

    @MainActor
    func applyProjectStatusChanged(projectId: String, projectATag: String, isOnline: Bool, onlineAgents: [ProjectAgent]) {
        let resolvedProjectId: String = {
            if !projectId.isEmpty {
                return projectId
            }
            return Self.projectId(fromATag: projectATag)
        }()

        guard !resolvedProjectId.isEmpty else { return }

        let normalizedAgents = Self.canonicalOnlineAgents(onlineAgents)
        let previousStatus = projectOnlineStatus[resolvedProjectId]
        let previousAgents = self.onlineAgents[resolvedProjectId]
        let statusChanged = previousStatus != isOnline
        let agentsChanged = previousAgents != normalizedAgents

        if statusChanged {
            setProjectOnlineStatus(isOnline, for: resolvedProjectId)
        }
        if agentsChanged {
            setOnlineAgentsCache(normalizedAgents, for: resolvedProjectId)
        }

        if statusChanged || agentsChanged {
            signalDiagnosticsUpdate()
        }

        profiler.logEvent(
            "applyProjectStatusChanged projectId=\(resolvedProjectId) statusChanged=\(statusChanged) agentsChanged=\(agentsChanged) isOnline=\(isOnline) agentCount=\(normalizedAgents.count)",
            category: .general,
            level: (statusChanged || agentsChanged) ? .info : .debug
        )
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
        var updated = conversations
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
            conversations = sortedConversations(updated)
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
            do {
                try await safeCore.approveBackend(pubkey: backendPubkey)
            } catch {
                return
            }

            let projectId = Self.projectId(fromATag: projectATag)
            guard !projectId.isEmpty else { return }

            let isOnline = await safeCore.isProjectOnline(projectId: projectId)
            let agents = (try? await safeCore.getOnlineAgents(projectId: projectId)) ?? []
            await MainActor.run {
                self.applyProjectStatusChanged(projectId: projectId, projectATag: projectATag, isOnline: isOnline, onlineAgents: agents)
            }
        }
        #endif
    }

    @MainActor
    func applyStreamChunk(agentPubkey: String, conversationId: String, textDelta: String?) {
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
            let messages = await safeCore.getMessages(conversationId: conversationId)
            let refreshedConversation = await safeCore.getConversationsByIds(conversationIds: [conversationId]).first
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
}
