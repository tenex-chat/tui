import SwiftUI

// MARK: - Auto-Login Result

/// Result of attempting auto-login from stored credentials
enum AutoLoginResult {
    /// No stored credentials found - show login screen
    case noCredentials
    /// Auto-login succeeded with the given npub
    case success(npub: String)
    /// Stored credential was invalid (should be deleted)
    case invalidCredential(error: String)
    /// Transient error (keychain access failed, network issue, etc.) - don't delete credentials
    case transientError(error: String)
}

/// Shared TenexCore instance wrapper for environment object
/// Initializes the core OFF the main thread to avoid UI jank
class TenexCoreManager: ObservableObject {
    let core: TenexCore
    @Published var isInitialized = false
    @Published var initializationError: String?

    init() {
        // Create core immediately (lightweight)
        core = TenexCore()

        // Initialize asynchronously off the main thread
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let success = self?.core.`init`() ?? false
            DispatchQueue.main.async {
                self?.isInitialized = success
                if !success {
                    self?.initializationError = "Failed to initialize TENEX core"
                }
            }
        }
    }

    func refresh() {
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            _ = self?.core.refresh()
        }
    }

    // MARK: - Credential Management

    /// Attempts auto-login using stored credentials
    /// - Returns: AutoLoginResult indicating outcome
    /// - Note: Call from background thread
    func attemptAutoLogin() -> AutoLoginResult {
        // Load credential from keychain
        let loadResult = KeychainService.shared.loadNsec()

        switch loadResult {
        case .failure(.itemNotFound):
            return .noCredentials

        case .failure(let error):
            // Keychain access failed - transient error, don't delete credentials
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

    /// Saves credentials to keychain after successful login
    /// - Parameter nsec: The nsec to save
    /// - Returns: Optional error message if save failed
    /// - Note: Call from background thread
    func saveCredential(nsec: String) -> String? {
        let result = KeychainService.shared.saveNsec(nsec)
        switch result {
        case .success:
            return nil
        case .failure(let error):
            return error.localizedDescription
        }
    }

    /// Clears stored credentials from keychain
    /// - Returns: Optional error message if clear failed
    /// - Note: Call from background thread
    func clearCredentials() -> String? {
        let result = KeychainService.shared.deleteNsec()
        switch result {
        case .success:
            return nil
        case .failure(let error):
            return error.localizedDescription
        }
    }
}

@main
struct TenexMVPApp: App {
    @StateObject private var coreManager = TenexCoreManager()
    @State private var isLoggedIn = false
    @State private var userNpub = ""
    @State private var isAttemptingAutoLogin = false
    @State private var autoLoginError: String?

    var body: some Scene {
        WindowGroup {
            Group {
                if !coreManager.isInitialized {
                    // Show loading while initializing
                    VStack(spacing: 16) {
                        ProgressView()
                            .scaleEffect(1.5)
                        Text("Initializing TENEX...")
                            .foregroundStyle(.secondary)

                        if let error = coreManager.initializationError {
                            Text(error)
                                .foregroundStyle(.red)
                                .font(.caption)
                        }
                    }
                } else if isAttemptingAutoLogin {
                    // Show loading while attempting auto-login
                    VStack(spacing: 16) {
                        ProgressView()
                            .scaleEffect(1.5)
                        Text("Logging in...")
                            .foregroundStyle(.secondary)
                    }
                } else if isLoggedIn {
                    MainTabView(userNpub: $userNpub, isLoggedIn: $isLoggedIn)
                        .environmentObject(coreManager)
                } else {
                    LoginView(
                        isLoggedIn: $isLoggedIn,
                        userNpub: $userNpub,
                        autoLoginError: autoLoginError
                    )
                    .environmentObject(coreManager)
                }
            }
            .onChange(of: coreManager.isInitialized) { _, isInitialized in
                if isInitialized {
                    attemptAutoLogin()
                }
            }
        }
    }

    private func attemptAutoLogin() {
        isAttemptingAutoLogin = true
        autoLoginError = nil

        DispatchQueue.global(qos: .userInitiated).async {
            let result = coreManager.attemptAutoLogin()

            DispatchQueue.main.async {
                isAttemptingAutoLogin = false

                switch result {
                case .noCredentials:
                    // No stored credentials - show login screen
                    break

                case .success(let npub):
                    // Auto-login succeeded
                    userNpub = npub
                    isLoggedIn = true

                case .invalidCredential(let error):
                    // Credential was provably invalid - delete it and show login
                    print("[TENEX] Stored credential invalid: \(error)")
                    DispatchQueue.global(qos: .userInitiated).async {
                        _ = coreManager.clearCredentials()
                    }
                    autoLoginError = "Stored credential was invalid. Please log in again."

                case .transientError(let error):
                    // Transient error - don't delete credentials, show login with warning
                    print("[TENEX] Auto-login transient error: \(error)")
                    autoLoginError = "Could not auto-login: \(error)"
                }
            }
        }
    }
}

// MARK: - Main Tab View

struct MainTabView: View {
    @Binding var userNpub: String
    @Binding var isLoggedIn: Bool
    @EnvironmentObject var coreManager: TenexCoreManager

    var body: some View {
        TabView {
            ConversationsTabView()
                .tabItem {
                    Label("Conversations", systemImage: "bubble.left.and.bubble.right.fill")
                }
                .environmentObject(coreManager)

            ContentView(userNpub: $userNpub, isLoggedIn: $isLoggedIn)
                .tabItem {
                    Label("Projects", systemImage: "folder.fill")
                }
                .environmentObject(coreManager)

            // TODO: Enable Stats tab once StatsSnapshot FFI bindings are generated
            // StatsView(coreManager: coreManager)
            //     .tabItem {
            //         Label("Stats", systemImage: "chart.bar.fill")
            //     }

            InboxView()
                .tabItem {
                    Label("Inbox", systemImage: "tray.fill")
                }
                .environmentObject(coreManager)
        }
    }
}
