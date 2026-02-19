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

    /// Clears stored credentials.
    /// - Returns: Optional error message if clear failed
    func clearCredentials() async -> String? {
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
