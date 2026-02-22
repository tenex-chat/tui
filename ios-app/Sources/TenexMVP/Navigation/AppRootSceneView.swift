import SwiftUI

struct AppRootSceneView: View {
    var coreManager: TenexCoreManager
    @ObservedObject var sessionStore: AppSessionStore
    let notificationScheduler: NotificationScheduling

    @State private var showNotificationDeniedAlert = false
    @State private var previousInitializedState: Bool?
    @State private var previousLoginState: Bool?
    @State private var previousScenePhase: ScenePhase?
    @Environment(\.scenePhase) private var scenePhase

    var body: some View {
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
                            .foregroundStyle(Color.healthError)
                            .font(.caption)
                    }
                }
            } else if sessionStore.isAttemptingAutoLogin {
                // Show loading while attempting auto-login
                VStack(spacing: 16) {
                    ProgressView()
                        .scaleEffect(1.5)
                    Text("Logging in...")
                        .foregroundStyle(.secondary)
                }
            } else if sessionStore.isLoggedIn {
                MainTabView(userNpub: $sessionStore.userNpub, isLoggedIn: $sessionStore.isLoggedIn)
                    .environment(coreManager)
            } else {
                LoginView(
                    isLoggedIn: $sessionStore.isLoggedIn,
                    userNpub: $sessionStore.userNpub,
                    autoLoginError: sessionStore.autoLoginError
                )
                .environment(coreManager)
            }
        }
        .onAppear {
            PerformanceProfiler.shared.startRuntimeMonitorsIfNeeded()
            PerformanceProfiler.shared.logEvent("App root appeared", category: .general)
            #if os(macOS)
            MacWindowAuthSizing.updateMainWindowForAuthState(isLoggedIn: sessionStore.isLoggedIn)
            #endif
        }
        .task(id: coreManager.isInitialized) {
            handleInitializationTransition(coreManager.isInitialized)
        }
        .task(id: sessionStore.isLoggedIn) {
            await handleLoginTransition(sessionStore.isLoggedIn)
        }
        .task(id: scenePhase) {
            await handleScenePhaseTransition(scenePhase)
        }
        #if os(iOS)
        .alert("Notifications Disabled", isPresented: $showNotificationDeniedAlert) {
            Button("Open Settings") {
                NotificationService.shared.openNotificationSettings()
            }
            Button("Not Now", role: .cancel) { }
        } message: {
            Text("To receive notifications when agents need your input, please enable notifications in Settings.")
        }
        #endif
    }

    private func handleInitializationTransition(_ isInitialized: Bool) {
        defer {
            previousInitializedState = isInitialized
        }

        guard previousInitializedState != nil else {
            return
        }

        PerformanceProfiler.shared.logEvent(
            "coreManager.isInitialized changed value=\(isInitialized)",
            category: .general
        )

        if isInitialized {
            sessionStore.attemptAutoLogin(coreManager: coreManager)
        }
    }

    private func handleLoginTransition(_ loggedIn: Bool) async {
        defer {
            previousLoginState = loggedIn
        }

        guard previousLoginState != nil else {
            return
        }

        PerformanceProfiler.shared.logEvent(
            "isLoggedIn changed value=\(loggedIn)",
            category: .general
        )
        #if os(macOS)
        MacWindowAuthSizing.updateMainWindowForAuthState(isLoggedIn: loggedIn)
        #endif

        if loggedIn {
            coreManager.registerEventCallback()
            await performInitialLoginBootstrap()
        } else {
            coreManager.unregisterEventCallback()
            await notificationScheduler.clearBadge()
        }
    }

    private func performInitialLoginBootstrap() async {
        // Request authorization first so badge updates are ready after data load.
        let result = await notificationScheduler.requestAuthorization()
        guard !Task.isCancelled else { return }

        switch result {
        case .granted:
            break
        case .denied, .previouslyDenied:
            showNotificationDeniedAlert = true
        case .error:
            break
        }

        await coreManager.fetchData()
        guard !Task.isCancelled else { return }
        coreManager.updateAppBadge()

        // Auto-start bunker if previously enabled
        await autoStartBunkerIfEnabled()
    }

    private func autoStartBunkerIfEnabled() async {
        let defaults = UserDefaults.standard
        // Match AppSettingsViewModel.loadPersistedBunkerEnabled() logic:
        // defaults to true when key doesn't exist
        let enabled: Bool
        if defaults.object(forKey: "settings.bunker.enabled") != nil {
            enabled = defaults.bool(forKey: "settings.bunker.enabled")
        } else {
            enabled = true
        }

        guard enabled else { return }

        do {
            let _ = try await coreManager.safeCore.startBunker()

            // Restore persisted auto-approve rules into the Rust core
            let persistedRules = BunkerAutoApproveStorage.loadRules()
            for rule in persistedRules {
                try? await coreManager.safeCore.addBunkerAutoApproveRule(
                    requesterPubkey: rule.requesterPubkey,
                    eventKind: rule.eventKind
                )
            }
        } catch {
            // Silently fail - bunker can be started manually from Settings
        }
    }

    private func handleScenePhaseTransition(_ phase: ScenePhase) async {
        defer {
            previousScenePhase = phase
        }

        guard previousScenePhase != nil else {
            return
        }

        guard phase == .active, sessionStore.isLoggedIn else {
            return
        }

        // User may have changed notification permissions while inactive.
        await notificationScheduler.checkAuthorizationStatus()
        guard !Task.isCancelled else { return }

        // Recalculate badge in case filter scope changed while inactive.
        coreManager.updateAppBadge()
    }
}
