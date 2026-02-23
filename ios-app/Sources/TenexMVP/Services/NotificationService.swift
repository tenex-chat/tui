import Foundation
import UserNotifications
import os.log

// MARK: - Notification Service

/// Service for managing local push notifications for ask events.
/// Uses iOS UserNotifications framework for local notifications without server infrastructure.
/// Suitable for TestFlight/App Store apps.
@MainActor
final class NotificationService: NSObject, ObservableObject {
    static let shared = NotificationService()

    /// Logger for notification service events (nonisolated to allow use in delegate callbacks)
    private nonisolated static let logger = Logger(subsystem: "com.tenex.mvp", category: "NotificationService")

    /// Whether notification permission has been granted (includes provisional/ephemeral)
    @Published private(set) var isAuthorized = false

    /// Current authorization status
    @Published private(set) var authorizationStatus: UNAuthorizationStatus = .notDetermined

    private let notificationCenter = UNUserNotificationCenter.current()

    private override init() {
        super.init()
        notificationCenter.delegate = self
    }

    // MARK: - Authorization

    /// Result of attempting to request notification authorization.
    enum AuthorizationResult {
        /// Permission was granted (either just now or previously)
        case granted
        /// Permission was just denied by the user
        case denied
        /// Permission was previously denied - user must enable in Settings
        case previouslyDenied
        /// An error occurred during the request
        case error(Error)
    }

    /// Request notification authorization from the user.
    /// This method checks the current status first and only shows the system dialog
    /// if the status is `.notDetermined`. If permission was previously denied,
    /// returns `.previouslyDenied` so the caller can prompt the user to go to Settings.
    ///
    /// - Returns: The result of the authorization attempt
    @discardableResult
    func requestAuthorization() async -> AuthorizationResult {
        // First, check current status
        await checkAuthorizationStatus()

        Self.logger.info("Current authorization status: \(self.authorizationStatus.description)")

        switch authorizationStatus {
        case .authorized, .provisional, .ephemeral:
            Self.logger.debug("Already authorized")
            return .granted

        case .denied:
            Self.logger.info("Previously denied - user must enable in Settings")
            return .previouslyDenied

        case .notDetermined:
            // Only case where iOS will show the permission dialog
            Self.logger.info("Status is notDetermined - requesting authorization...")
            do {
                let granted = try await notificationCenter.requestAuthorization(options: [.alert, .sound, .badge])
                isAuthorized = granted
                await checkAuthorizationStatus()
                Self.logger.info("Authorization \(granted ? "granted" : "denied") by user")
                return granted ? .granted : .denied
            } catch {
                Self.logger.error("Authorization request failed: \(error)")
                isAuthorized = false
                return .error(error)
            }

        @unknown default:
            Self.logger.warning("Unknown authorization status: \(self.authorizationStatus.rawValue)")
            return .denied
        }
    }

    /// Check current authorization status.
    func checkAuthorizationStatus() async {
        let settings = await notificationCenter.notificationSettings()
        authorizationStatus = settings.authorizationStatus
        // Treat .authorized, .provisional, and .ephemeral as authorized
        // since they all allow local notifications to be delivered
        isAuthorized = settings.authorizationStatus.allowsLocalNotifications
        Self.logger.debug("Checked status: \(self.authorizationStatus.description), isAuthorized: \(self.isAuthorized)")
    }

    /// Whether the user has previously denied notification permission.
    /// Use this to determine if we should show an in-app prompt to go to Settings.
    var isPreviouslyDenied: Bool {
        authorizationStatus == .denied
    }

    // MARK: - Badge Management

    /// Update the app badge number.
    /// - Parameter count: The number to display on the app icon badge. Pass 0 to clear.
    func updateBadge(count: Int) async {
        do {
            try await notificationCenter.setBadgeCount(count)
            Self.logger.debug("Badge updated to \(count)")
        } catch {
            Self.logger.error("Failed to update badge: \(error)")
        }
    }

    /// Clear the app badge.
    func clearBadge() async {
        await updateBadge(count: 0)
    }

    // MARK: - Local Notifications

    /// Schedule a local notification for a new ask event.
    /// - Parameters:
    ///   - askEventId: Unique identifier for the ask event (used as notification ID)
    ///   - title: Title for the notification
    ///   - body: Body text for the notification
    ///   - fromAgent: Name of the agent who sent the ask
    ///   - projectId: Optional project ID for context
    ///   - conversationId: Optional conversation ID for deep linking
    func scheduleAskNotification(
        askEventId: String,
        title: String,
        body: String,
        fromAgent: String,
        projectId: String?,
        conversationId: String?
    ) async {
        guard isAuthorized else {
            Self.logger.debug("Not authorized - skipping notification")
            return
        }

        let content = UNMutableNotificationContent()
        content.title = "Question from \(fromAgent)"
        content.subtitle = title.isEmpty ? "New question" : title
        content.body = body.prefix(200).description + (body.count > 200 ? "..." : "")
        content.sound = .default
        content.categoryIdentifier = "ASK_EVENT"

        // Store metadata for deep linking
        var userInfo: [String: String] = [
            "askEventId": askEventId,
            "type": "ask"
        ]
        if let projectId = projectId {
            userInfo["projectId"] = projectId
        }
        if let conversationId = conversationId {
            userInfo["conversationId"] = conversationId
        }
        content.userInfo = userInfo

        // Trigger immediately (iOS requires >= 1s for time-interval triggers)
        let trigger = UNTimeIntervalNotificationTrigger(timeInterval: 1, repeats: false)

        let request = UNNotificationRequest(
            identifier: "ask-\(askEventId)",
            content: content,
            trigger: trigger
        )

        do {
            try await notificationCenter.add(request)
            Self.logger.info("Scheduled notification for ask event: \(askEventId.prefix(12))...")
        } catch {
            Self.logger.error("Failed to schedule notification: \(error)")
        }
    }

    /// Remove a pending notification for an ask event (e.g., when user answers it).
    /// - Parameter askEventId: The ask event ID whose notification should be removed.
    func removeNotification(askEventId: String) {
        notificationCenter.removePendingNotificationRequests(withIdentifiers: ["ask-\(askEventId)"])
        notificationCenter.removeDeliveredNotifications(withIdentifiers: ["ask-\(askEventId)"])
        Self.logger.debug("Removed notification for ask event: \(askEventId.prefix(12))...")
    }

    /// Remove all pending ask notifications.
    func removeAllNotifications() {
        notificationCenter.removeAllPendingNotificationRequests()
        notificationCenter.removeAllDeliveredNotifications()
        Self.logger.debug("Removed all notifications")
    }

    // MARK: - Remote Notification Handling

    /// Process a remote notification payload delivered by APNs.
    ///
    /// Called from `AppDelegate.application(_:didReceiveRemoteNotification:fetchCompletionHandler:)`
    /// for background/silent pushes.  User-visible pushes that the user *taps* are
    /// handled by `userNotificationCenter(_:didReceive:)` below.
    ///
    /// Expected APNs payload structure:
    /// ```json
    /// {
    ///   "aps": { "alert": { "title": "…", "body": "…" }, "sound": "default", "badge": 1 },
    ///   "conversation_id": "<hex>",
    ///   "event_id": "<hex>"
    /// }
    /// ```
    @MainActor
    func handleRemoteNotification(userInfo: [AnyHashable: Any]) {
        Self.logger.info("Processing remote notification payload")

        // Extract deep-link identifiers from the payload
        if let conversationId = userInfo["conversation_id"] as? String {
            Self.logger.info("Remote notification references conversation: \(conversationId.prefix(12))...")
            NotificationCenter.default.post(
                name: .askNotificationTapped,
                object: nil,
                userInfo: ["conversationId": conversationId]
            )
        }

        // Update badge count if provided
        if let aps = userInfo["aps"] as? [String: Any],
           let badge = aps["badge"] as? Int {
            Task { await self.updateBadge(count: badge) }
        }
    }
}

// MARK: - UNUserNotificationCenterDelegate

extension NotificationService: UNUserNotificationCenterDelegate {
    /// Handle notification when app is in foreground.
    /// Shows the notification as a banner even when the app is active.
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification
    ) async -> UNNotificationPresentationOptions {
        // Show notification even when app is in foreground
        return [.banner, .sound, .badge]
    }

    /// Handle notification tap (deep linking).
    ///
    /// Handles both:
    /// - **Local ask notifications** (type == "ask", scheduled by `scheduleAskNotification`)
    /// - **Remote APNs push notifications** (contain "conversation_id" in the payload)
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse
    ) async {
        let userInfo = response.notification.request.content.userInfo

        // Local ask notification (scheduled by scheduleAskNotification)
        if let type = userInfo["type"] as? String, type == "ask",
           let conversationId = userInfo["conversationId"] as? String {
            Self.logger.info("User tapped ask notification for conversation: \(conversationId.prefix(12))...")
            await MainActor.run {
                NotificationCenter.default.post(
                    name: .askNotificationTapped,
                    object: nil,
                    userInfo: ["conversationId": conversationId]
                )
            }
            return
        }

        // Remote APNs push notification (delivered by TENEX backend)
        if let conversationId = userInfo["conversation_id"] as? String {
            Self.logger.info("User tapped remote notification for conversation: \(conversationId.prefix(12))...")
            await MainActor.run {
                NotificationCenter.default.post(
                    name: .askNotificationTapped,
                    object: nil,
                    userInfo: ["conversationId": conversationId]
                )
            }
        }
    }
}

// MARK: - Notification Names

extension Notification.Name {
    /// Posted when user taps an ask notification.
    /// userInfo contains "conversationId" for navigation.
    static let askNotificationTapped = Notification.Name("askNotificationTapped")
}

// MARK: - UNAuthorizationStatus Extensions

extension UNAuthorizationStatus {
    /// Human-readable description for logging purposes.
    var description: String {
        switch self {
        case .notDetermined: return "notDetermined"
        case .denied: return "denied"
        case .authorized: return "authorized"
        case .provisional: return "provisional"
        case .ephemeral: return "ephemeral"
        @unknown default: return "unknown(\(rawValue))"
        }
    }

    /// Whether this authorization status allows local notifications to be delivered.
    /// Returns true for `.authorized`, `.provisional`, and `.ephemeral`.
    var allowsLocalNotifications: Bool {
        switch self {
        case .authorized, .provisional, .ephemeral:
            return true
        case .notDetermined, .denied:
            return false
        @unknown default:
            return false
        }
    }
}

// MARK: - Settings URL Helper

#if os(iOS)
import UIKit

extension NotificationService {
    /// Opens the app's notification settings in the iOS Settings app.
    /// On iOS 16+, opens directly to notification settings.
    /// On earlier versions, opens the app's general settings page.
    /// Use this when the user has previously denied notification permission.
    func openNotificationSettings() {
        let settingsURLString: String
        if #available(iOS 16.0, *) {
            settingsURLString = UIApplication.openNotificationSettingsURLString
        } else {
            settingsURLString = UIApplication.openSettingsURLString
        }

        guard let settingsURL = URL(string: settingsURLString) else {
            Self.logger.error("Failed to create Settings URL")
            return
        }
        UIApplication.shared.open(settingsURL) { success in
            Self.logger.debug("Opened Settings: \(success)")
        }
    }
}
#endif
