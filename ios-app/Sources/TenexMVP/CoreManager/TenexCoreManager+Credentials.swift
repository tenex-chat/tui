import Foundation

extension TenexCoreManager {
    /// Attempts auto-login using stored credentials
    /// - Returns: AutoLoginResult indicating outcome
    /// - Note: Call from background thread
    nonisolated func attemptAutoLogin() -> AutoLoginResult {
        // Load credential from configured storage backend.
        let loadResult = KeychainService.shared.loadNsec()

        switch loadResult {
        case .failure(.itemNotFound):
            return .noCredentials

        case .failure(let error):
            // Storage access failed - transient error, don't delete credentials.
            return .transientError(error: error.localizedDescription)

        case .success(let nsec):
            // Attempt login with stored credential
            do {
                let loginResult = try core.login(nsec: nsec)
                if loginResult.success {
                    return .success(npub: loginResult.npub)
                } else {
                    // Login returned false without throwing - this is ambiguous
                    // Could be network issue, server error, etc. - treat as transient
                    // to avoid deleting potentially valid credentials
                    return .transientError(error: "Login failed - please try again")
                }
            } catch let error as TenexError {
                switch error {
                case .InvalidNsec(let message):
                    // Provably invalid - should delete stored credential
                    return .invalidCredential(error: message)
                case .NotLoggedIn, .Internal, .LogoutFailed, .LockError, .CoreNotInitialized:
                    // These are transient/unexpected - don't delete credentials
                    return .transientError(error: error.localizedDescription)
                }
            } catch {
                // Unknown error - treat as transient
                return .transientError(error: error.localizedDescription)
            }
        }
    }

    /// Saves credentials after successful login.
    /// - Parameter nsec: The nsec to save
    /// - Returns: Optional error message if save failed
    func saveCredential(nsec: String) async -> String? {
        let result = await KeychainService.shared.saveNsecAsync(nsec)
        switch result {
        case .success:
            return nil
        case .failure(let error):
            return error.localizedDescription
        }
    }

    /// Clears stored credentials and resets all session state.
    /// - Returns: Optional error message if clear failed
    func clearCredentials() async -> String? {
        // Reset Rust core state first — this stops NDK subscriptions and clears
        // internal caches so stale data from the old account cannot leak into
        // the next login session.
        // Run logout at utility priority: the Rust path blocks on worker-thread
        // disconnect confirmation, which can trigger QoS inversion warnings when
        // called from user-initiated UI tasks.
        do {
            try await withCheckedThrowingContinuation {
                (continuation: CheckedContinuation<Void, Error>) in
                Task.detached(priority: .utility) { [safeCore] in
                    do {
                        try await safeCore.logout()
                        continuation.resume(returning: ())
                    } catch {
                        continuation.resume(throwing: error)
                    }
                }
            }
        } catch {
            profiler.logEvent(
                "clearCredentials logout failed error=\(error.localizedDescription)",
                category: .general,
                level: .error
            )
        }

        // Clear Swift-side session data immediately so the UI shows nothing
        // while the next fetchData() runs.
        projects = []
        conversations = []
        appFilterConversationScope = []
        inboxItems = []
        reports = []
        messagesByConversation = [:]
        projectOnlineStatus = [:]
        onlineAgents = [:]

        // Clear profile picture cache on logout to prevent stale data
        profilePictureCache.clear()

        let result = await KeychainService.shared.deleteNsecAsync()
        switch result {
        case .success:
            return nil
        case .failure(let error):
            return error.localizedDescription
        }
    }
}
