import SwiftUI

// MARK: - Debug Auto-Login Support
//
// For automated testing (e.g., ios-tester agent), you can bypass the login screen by:
//
// 1. Launch arguments (recommended for xcrun simctl):
//    xcrun simctl launch <UDID> com.tenex.mvp --debug-nsec "nsec1..."
//
// 2. Environment variables:
//    TENEX_DEBUG_NSEC=nsec1...
//
// Example with simctl:
//    xcrun simctl launch 91722A96-628B-49D9-9A07-3E5A2BDEB65D com.tenex.mvp --debug-nsec "nsec1abc..."
//
// The app will auto-login with the provided nsec and skip the login screen.
// This is only intended for DEBUG builds and automated testing.

@main
struct TenexMVPApp: App {
    @State private var coreManager = TenexCoreManager()
    @StateObject private var sessionStore = AppSessionStore()
    private let notificationScheduler: NotificationScheduling = NotificationService.shared

    var body: some Scene {
        WindowGroup {
            AppRootSceneView(
                coreManager: coreManager,
                sessionStore: sessionStore,
                notificationScheduler: notificationScheduler
            )
        }
        #if os(macOS)
        .defaultSize(width: 1200, height: 800)
        #endif

        #if os(macOS)
        WindowGroup(id: "full-conversation", for: String.self) { $conversationId in
            if let conversationId {
                FullConversationWindow(conversationId: conversationId)
                    .environment(coreManager)
            }
        }
        .defaultSize(width: 800, height: 700)

        WindowGroup(id: "delegation-tree", for: String.self) { $conversationId in
            if let id = conversationId {
                DelegationTreeView(rootConversationId: id)
                    .environment(coreManager)
            }
        }
        .defaultSize(width: 1300, height: 820)
        #endif
    }
}
