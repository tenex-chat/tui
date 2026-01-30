import SwiftUI
import CryptoKit

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

// MARK: - Profile Picture Cache

/// Thread-safe cache for profile picture URLs to prevent repeated synchronous FFI calls during scroll.
/// Each pubkey's picture URL is fetched once and cached for the session lifetime.
final class ProfilePictureCache {
    static let shared = ProfilePictureCache()

    private var cache: [String: String?] = [:]
    private let lock = NSLock()

    private init() {}

    /// Get cached profile picture URL for a pubkey.
    /// Returns nil if not cached (call fetch to populate).
    func getCached(_ pubkey: String) -> String?? {
        lock.lock()
        defer { lock.unlock() }

        if cache.keys.contains(pubkey) {
            return cache[pubkey]
        }
        return nil // Not in cache (different from cached nil)
    }

    /// Store a profile picture URL in the cache.
    /// Pass nil to cache "no picture available" for this pubkey.
    func store(_ pubkey: String, pictureUrl: String?) {
        lock.lock()
        defer { lock.unlock() }
        cache[pubkey] = pictureUrl
    }

    /// Clear the entire cache (e.g., on logout)
    func clear() {
        lock.lock()
        defer { lock.unlock() }
        cache.removeAll()
    }

    /// Get the number of cached entries (for debugging)
    var count: Int {
        lock.lock()
        defer { lock.unlock() }
        return cache.count
    }
}

/// Shared TenexCore instance wrapper for environment object
/// Initializes the core OFF the main thread to avoid UI jank
class TenexCoreManager: ObservableObject {
    let core: TenexCore
    /// Thread-safe async wrapper for FFI access with proper error handling
    let safeCore: SafeTenexCore
    @Published var isInitialized = false
    @Published var initializationError: String?

    // MARK: - Centralized Reactive Data Store
    @Published var projects: [ProjectInfo] = []
    @Published var conversations: [ConversationFullInfo] = []
    @Published var inboxItems: [InboxItem] = []

    // MARK: - Polling Infrastructure
    private var pollingTimer: Timer?
    private let pollingInterval: TimeInterval = 2.5
    private var isPolling = false

    /// Cache for profile picture URLs to prevent repeated FFI calls
    let profilePictureCache = ProfilePictureCache.shared

    init() {
        // Create core immediately (lightweight)
        let tenexCore = TenexCore()
        core = tenexCore
        safeCore = SafeTenexCore(core: tenexCore)

        // Initialize asynchronously off the main thread
        DispatchQueue.global(qos: .userInitiated).async { [weak self] in
            let success = tenexCore.`init`()
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

    // MARK: - Polling Control

    /// Starts background polling for data updates
    func startPolling() {
        guard !isPolling else { return }
        isPolling = true

        // Initial fetch immediately using async version
        Task { @MainActor in
            await pollForUpdatesAsync()
        }

        // Start timer on main thread
        DispatchQueue.main.async { [weak self] in
            guard let self = self else { return }
            self.pollingTimer = Timer.scheduledTimer(withTimeInterval: self.pollingInterval, repeats: true) { [weak self] _ in
                Task { @MainActor in
                    await self?.pollForUpdatesAsync()
                }
            }
        }
    }

    /// Stops background polling
    func stopPolling() {
        isPolling = false
        pollingTimer?.invalidate()
        pollingTimer = nil
    }

    /// Manual refresh for pull-to-refresh gesture
    func manualRefresh() async {
        await pollForUpdatesAsync()
    }

    /// Async polling using SafeTenexCore with proper error handling.
    /// Unlike pollForUpdates(), this version won't crash on FFI errors.
    @MainActor
    func pollForUpdatesAsync() async {
        _ = await safeCore.refresh()

        do {
            let filter = ConversationFilter(
                projectIds: [],
                showArchived: true,
                hideScheduled: true,
                timeFilter: .all
            )

            // Fetch all data concurrently
            async let fetchedProjects = safeCore.getProjects()
            async let fetchedConversations = try safeCore.getAllConversations(filter: filter)
            async let fetchedInbox = safeCore.getInbox()

            let (p, c, i) = try await (fetchedProjects, fetchedConversations, fetchedInbox)

            projects = p
            conversations = c
            inboxItems = i
        } catch {
            print("[TenexCoreManager] Poll failed: \(error)")
            // Don't crash - just log and continue with stale data
        }
    }

    // MARK: - Profile Picture API (Cached)

    /// Get profile picture URL for a pubkey, using cache to prevent repeated FFI calls.
    /// This is the primary API for avatar views - always use this instead of core.getProfilePicture directly.
    /// - Parameter pubkey: The hex-encoded public key
    /// - Returns: Profile picture URL if available, nil otherwise
    func getProfilePicture(pubkey: String) -> String? {
        // Check cache first (O(1) lookup)
        if let cached = profilePictureCache.getCached(pubkey) {
            return cached
        }

        // Cache miss - fetch from FFI (synchronous, but only once per pubkey)
        // Handle Result type properly - log errors but return nil for graceful degradation
        do {
            let pictureUrl = try core.getProfilePicture(pubkey: pubkey)
            profilePictureCache.store(pubkey, pictureUrl: pictureUrl)
            return pictureUrl
        } catch {
            // Log error for debugging but don't crash - graceful degradation
            print("[TenexCoreManager] Failed to get profile picture for pubkey '\(pubkey.prefix(pubkeyDisplayPrefixLength))...': \(error)")
            // Cache nil to prevent repeated failed calls
            profilePictureCache.store(pubkey, pictureUrl: nil)
            return nil
        }
    }

    /// Prefetch profile pictures for multiple pubkeys in background.
    /// Call this when loading a list of agents/conversations to warm the cache.
    /// - Parameter pubkeys: Array of hex-encoded public keys to prefetch
    func prefetchProfilePictures(_ pubkeys: [String]) {
        DispatchQueue.global(qos: .utility).async { [weak self] in
            for pubkey in pubkeys {
                // Only fetch if not already cached
                if self?.profilePictureCache.getCached(pubkey) == nil {
                    do {
                        let pictureUrl = try self?.core.getProfilePicture(pubkey: pubkey)
                        self?.profilePictureCache.store(pubkey, pictureUrl: pictureUrl)
                    } catch {
                        // Log but don't crash - cache nil to prevent repeated attempts
                        print("[TenexCoreManager] Prefetch failed for pubkey '\(pubkey.prefix(pubkeyDisplayPrefixLength))...': \(error)")
                        self?.profilePictureCache.store(pubkey, pictureUrl: nil)
                    }
                }
            }
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
        // Clear profile picture cache on logout to prevent stale data
        profilePictureCache.clear()

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
    @Environment(\.scenePhase) private var scenePhase

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
            .onChange(of: scenePhase) { _, newPhase in
                // Pause/resume polling based on app lifecycle
                if isLoggedIn {
                    switch newPhase {
                    case .active:
                        coreManager.startPolling()
                    case .background, .inactive:
                        coreManager.stopPolling()
                    @unknown default:
                        break
                    }
                }
            }
            .onChange(of: isLoggedIn) { _, loggedIn in
                // Start/stop polling based on login state
                if loggedIn {
                    coreManager.startPolling()
                } else {
                    coreManager.stopPolling()
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

    @State private var selectedTab = 0
    @State private var showNewConversation = false

    var body: some View {
        NavigationStack {
            Group {
                switch selectedTab {
                case 0:
                    ConversationsTabView()
                case 1:
                    ContentView(userNpub: $userNpub, isLoggedIn: $isLoggedIn)
                case 2:
                    InboxView()
                default:
                    ConversationsTabView()
                }
            }
            .environmentObject(coreManager)
            .toolbar {
                // Left glass segment: Tab buttons
                ToolbarItemGroup(placement: .bottomBar) {
                    Button {
                        selectedTab = 0
                    } label: {
                        Image(systemName: selectedTab == 0 ? "bubble.left.and.bubble.right.fill" : "bubble.left.and.bubble.right")
                    }
                    Button {
                        selectedTab = 1
                    } label: {
                        Image(systemName: selectedTab == 1 ? "folder.fill" : "folder")
                    }
                    Button {
                        selectedTab = 2
                    } label: {
                        Image(systemName: selectedTab == 2 ? "tray.fill" : "tray")
                    }
                }

                // Pushes segments to opposite sides
                if #available(iOS 26.0, *) {
                    ToolbarSpacer(.flexible, placement: .bottomBar)
                } else {
                    ToolbarItem(placement: .bottomBar) {
                        Spacer()
                    }
                }

                // Right glass segment: New conversation
                ToolbarItem(placement: .bottomBar) {
                    Button {
                        showNewConversation = true
                    } label: {
                        Image(systemName: "plus")
                    }
                }
            }
        }
        .sheet(isPresented: $showNewConversation) {
            MessageComposerView(
                project: nil,
                conversationId: nil,
                conversationTitle: nil
            )
            .environmentObject(coreManager)
        }
    }
}
