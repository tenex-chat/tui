import SwiftUI

struct AppRootSceneView: View {
    @ObservedObject var coreManager: TenexCoreManager
    @ObservedObject var sessionStore: AppSessionStore
    let notificationScheduler: NotificationScheduling

    @State private var showNotificationDeniedAlert = false
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
                    .environmentObject(coreManager)
            } else {
                LoginView(
                    isLoggedIn: $sessionStore.isLoggedIn,
                    userNpub: $sessionStore.userNpub,
                    autoLoginError: sessionStore.autoLoginError
                )
                .environmentObject(coreManager)
            }
        }
        .onAppear {
            Task { @MainActor in
                PerformanceProfiler.shared.startRuntimeMonitorsIfNeeded()
            }
            PerformanceProfiler.shared.logEvent("App root appeared", category: .general)
            #if os(macOS)
            MacWindowAuthSizing.updateMainWindowForAuthState(isLoggedIn: sessionStore.isLoggedIn)
            #endif
        }
        .onChange(of: coreManager.isInitialized) { _, isInitialized in
            PerformanceProfiler.shared.logEvent(
                "coreManager.isInitialized changed value=\(isInitialized)",
                category: .general
            )
            if isInitialized {
                sessionStore.attemptAutoLogin(coreManager: coreManager)
            }
        }
        .onChange(of: sessionStore.isLoggedIn) { _, loggedIn in
            PerformanceProfiler.shared.logEvent(
                "isLoggedIn changed value=\(loggedIn)",
                category: .general
            )
            #if os(macOS)
            MacWindowAuthSizing.updateMainWindowForAuthState(isLoggedIn: loggedIn)
            #endif
            // Register/unregister event callback based on login state
            if loggedIn {
                coreManager.registerEventCallback()
                // Initial data fetch on login with proper authorization sequencing
                Task { @MainActor in
                    // Request authorization FIRST so badge can be set after data load
                    // This checks status first - only shows dialog if status is .notDetermined
                    let result = await notificationScheduler.requestAuthorization()

                    // Handle the authorization result
                    switch result {
                    case .granted:
                        break
                    case .denied, .previouslyDenied:
                        // User denied notifications - show alert directing them to Settings.
                        showNotificationDeniedAlert = true
                    case .error(let error):
                        _ = error
                        break
                    }

                    await coreManager.fetchData()
                    // Update badge after both authorization and data load complete
                    coreManager.updateAppBadge()
                }
            } else {
                coreManager.unregisterEventCallback()
                // Clear badge on logout
                Task {
                    await notificationScheduler.clearBadge()
                }
            }
        }
        .onChange(of: scenePhase) { _, newPhase in
            // Handle app becoming active
            if newPhase == .active && sessionStore.isLoggedIn {
                Task {
                    // Refresh authorization status (user may have changed permissions in Settings)
                    await notificationScheduler.checkAuthorizationStatus()
                    // Recalculate badge in case global filter scope changed while inactive
                    await MainActor.run {
                        coreManager.updateAppBadge()
                    }
                }
            }
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
}
