#if os(iOS)
import UIKit
import os.log

// MARK: - AppDelegate

/// UIApplicationDelegate for handling iOS-specific lifecycle events.
///
/// Connected to SwiftUI via `@UIApplicationDelegateAdaptor` in `TenexMVPApp`.
/// Responsible for:
/// - Forwarding APNs device tokens to `TenexCoreManager` (via NotificationCenter)
/// - Forwarding silent remote notification payloads for processing
final class AppDelegate: NSObject, UIApplicationDelegate {

    private static let logger = Logger(subsystem: "com.tenex.mvp", category: "AppDelegate")

    // MARK: - UIApplicationDelegate

    func application(
        _ application: UIApplication,
        didFinishLaunchingWithOptions launchOptions: [UIApplication.LaunchOptionsKey: Any]? = nil
    ) -> Bool {
        // Remote notification registration is triggered after user grants permission.
        // See TenexCoreManager+PushNotifications.swift and AppRootSceneView.swift.
        return true
    }

    // MARK: - APNs Token Registration

    /// Called when iOS successfully registers the device with APNs.
    /// Converts the binary token to a hex string and broadcasts it via NotificationCenter
    /// so `TenexCoreManager` can publish a kind:25000 event to the backend.
    func application(
        _ application: UIApplication,
        didRegisterForRemoteNotificationsWithDeviceToken deviceToken: Data
    ) {
        let hexToken = deviceToken.map { String(format: "%02x", $0) }.joined()
        Self.logger.info("APNs registration succeeded, token prefix: \(hexToken.prefix(12))...")

        NotificationCenter.default.post(
            name: .apnsTokenRegistered,
            object: nil,
            userInfo: [APNSNotificationKey.deviceToken: hexToken]
        )
    }

    /// Called when APNs registration fails.
    /// On simulator this is expected — APNs only works on physical devices.
    func application(
        _ application: UIApplication,
        didFailToRegisterForRemoteNotificationsWithError error: Error
    ) {
        // APNs registration always fails on the simulator — suppress at info level.
        #if targetEnvironment(simulator)
        Self.logger.info("APNs registration not available on simulator: \(error.localizedDescription)")
        #else
        Self.logger.error("APNs registration failed: \(error.localizedDescription)")
        #endif
    }

    // MARK: - Remote Notification Delivery

    /// Called for silent/background remote notifications (content-available: 1).
    /// User-visible push notifications are handled by `UNUserNotificationCenterDelegate`
    /// in `NotificationService` after the user taps the banner.
    func application(
        _ application: UIApplication,
        didReceiveRemoteNotification userInfo: [AnyHashable: Any],
        fetchCompletionHandler completionHandler: @escaping (UIBackgroundFetchResult) -> Void
    ) {
        Self.logger.info("Received remote notification (background fetch)")
        Task { @MainActor in
            NotificationService.shared.handleRemoteNotification(userInfo: userInfo)
        }
        completionHandler(.newData)
    }
}

// MARK: - Notification Name & Keys

extension Notification.Name {
    /// Posted when APNs returns a device token.
    /// `userInfo` contains `APNSNotificationKey.deviceToken` → `String` (hex).
    static let apnsTokenRegistered = Notification.Name("com.tenex.mvp.apnsTokenRegistered")
}

/// Keys used in `apnsTokenRegistered` notification's `userInfo` dictionary.
enum APNSNotificationKey {
    static let deviceToken = "deviceToken"
}

#endif // os(iOS)
