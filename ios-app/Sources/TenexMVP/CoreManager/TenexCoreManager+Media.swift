import Foundation

extension TenexCoreManager {
    /// Trigger audio notification generation for a p-tag mention.
    /// Runs in background to avoid blocking UI. Audio is played automatically when ready.
    func triggerAudioNotification(
        agentPubkey: String,
        conversationTitle: String,
        messageText: String,
        conversationId: String? = nil
    ) async {
        // Check inactivity threshold: skip TTS if user was recently active in this conversation
        // Using fixed 120 second threshold
        if let convId = conversationId,
           let lastActivity = lastUserActivityByConversation[convId] {
            let threshold: UInt64 = 120
            let now = UInt64(Date().timeIntervalSince1970)
            if now - lastActivity < threshold {
                return
            }
        }

        // Load API keys from the configured credential storage backend.
        let elevenlabsResult = await KeychainService.shared.loadElevenLabsApiKeyAsync()
        let openrouterResult = await KeychainService.shared.loadOpenRouterApiKeyAsync()

        guard case .success(let elevenlabsKey) = elevenlabsResult,
              case .success(let openrouterKey) = openrouterResult else {
            return
        }

        do {
            let notification = try await safeCore.generateAudioNotification(
                agentPubkey: agentPubkey,
                conversationTitle: conversationTitle,
                messageText: messageText,
                elevenlabsApiKey: elevenlabsKey,
                openrouterApiKey: openrouterKey
            )

            await MainActor.run {
                AudioNotificationPlayer.shared.enqueue(notification: notification, conversationId: conversationId)
            }
        } catch {
        }
    }

    /// Get profile picture URL for a pubkey, using cache to prevent repeated FFI calls.
    /// This is the primary API for avatar views - always use this instead of core.getProfilePicture directly.
    /// - Parameter pubkey: The hex-encoded public key
    /// - Returns: Profile picture URL if available, nil otherwise
    nonisolated func getProfilePicture(pubkey: String) -> String? {
        // Check cache first (O(1) lookup)
        if let cached = profilePictureCache.getCached(pubkey) {
            return cached
        }

        // Cache miss - fetch from FFI (synchronous, but only once per pubkey)
        // Handle Result type properly - log errors but return nil for graceful degradation
        let startedAt = CFAbsoluteTimeGetCurrent()
        do {
            let pictureUrl = try core.getProfilePicture(pubkey: pubkey)
            profilePictureCache.store(pubkey, pictureUrl: pictureUrl)
            let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
            PerformanceProfiler.shared.logEvent(
                "getProfilePicture cache-miss pubkey=\(pubkey.prefix(12)) elapsedMs=\(String(format: "%.2f", elapsedMs)) hit=\(pictureUrl != nil)",
                category: .ffi,
                level: elapsedMs >= 50 ? .error : .info
            )
            return pictureUrl
        } catch {
            // Log error for debugging but don't crash - graceful degradation
            // Cache nil to prevent repeated failed calls
            profilePictureCache.store(pubkey, pictureUrl: nil)
            let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
            PerformanceProfiler.shared.logEvent(
                "getProfilePicture failed pubkey=\(pubkey.prefix(12)) elapsedMs=\(String(format: "%.2f", elapsedMs)) error=\(error.localizedDescription)",
                category: .ffi,
                level: .error
            )
            return nil
        }
    }

    /// Prefetch profile pictures for multiple pubkeys in background.
    /// Call this when loading a list of agents/conversations to warm the cache.
    /// - Parameter pubkeys: Array of hex-encoded public keys to prefetch
    nonisolated func prefetchProfilePictures(_ pubkeys: [String]) {
        let cache = profilePictureCache
        let core = core
        PerformanceProfiler.shared.logEvent(
            "prefetchProfilePictures start requested=\(pubkeys.count)",
            category: .ffi,
            level: .debug
        )
        DispatchQueue.global(qos: .utility).async {
            let startedAt = CFAbsoluteTimeGetCurrent()
            var fetchedCount = 0
            var cacheMisses = 0
            for pubkey in pubkeys {
                // Only fetch if not already cached
                if cache.getCached(pubkey) == nil {
                    cacheMisses += 1
                    do {
                        let pictureUrl = try core.getProfilePicture(pubkey: pubkey)
                        cache.store(pubkey, pictureUrl: pictureUrl)
                        fetchedCount += 1
                    } catch {
                        // Log but don't crash - cache nil to prevent repeated attempts
                        cache.store(pubkey, pictureUrl: nil)
                    }
                }
            }
            let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
            PerformanceProfiler.shared.logEvent(
                "prefetchProfilePictures complete requested=\(pubkeys.count) cacheMisses=\(cacheMisses) fetched=\(fetchedCount) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                category: .ffi,
                level: elapsedMs >= 150 ? .error : .info
            )
        }
    }
}
