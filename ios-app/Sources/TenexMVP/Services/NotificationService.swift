import Foundation
import UserNotifications

// MARK: - Notification Service

/// Service for managing local push notifications for ask events.
/// Uses iOS UserNotifications framework for local notifications without server infrastructure.
/// Suitable for TestFlight/App Store apps.
@MainActor
final class NotificationService: NSObject, ObservableObject {
    static let shared = NotificationService()

    /// Whether notification permission has been granted
    @Published private(set) var isAuthorized = false

    /// Current authorization status
    @Published private(set) var authorizationStatus: UNAuthorizationStatus = .notDetermined

    private let notificationCenter = UNUserNotificationCenter.current()

    private override init() {
        super.init()
        notificationCenter.delegate = self
    }

    // MARK: - Authorization

    /// Request notification authorization from the user.
    /// Call this early in the app lifecycle (e.g., after login).
    func requestAuthorization() async {
        do {
            let granted = try await notificationCenter.requestAuthorization(options: [.alert, .sound, .badge])
            isAuthorized = granted
            print("[NotificationService] Authorization \(granted ? "granted" : "denied")")

            // Update authorization status
            await checkAuthorizationStatus()
        } catch {
            print("[NotificationService] Authorization request failed: \(error)")
            isAuthorized = false
        }
    }

    /// Check current authorization status.
    func checkAuthorizationStatus() async {
        let settings = await notificationCenter.notificationSettings()
        authorizationStatus = settings.authorizationStatus
        isAuthorized = settings.authorizationStatus == .authorized
    }

    // MARK: - Badge Management

    /// Update the app badge number.
    /// - Parameter count: The number to display on the app icon badge. Pass 0 to clear.
    func updateBadge(count: Int) async {
        do {
            try await notificationCenter.setBadgeCount(count)
            print("[NotificationService] Badge updated to \(count)")
        } catch {
            print("[NotificationService] Failed to update badge: \(error)")
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
            print("[NotificationService] Not authorized - skipping notification")
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
            print("[NotificationService] Scheduled notification for ask event: \(askEventId.prefix(12))...")
        } catch {
            print("[NotificationService] Failed to schedule notification: \(error)")
        }
    }

    /// Remove a pending notification for an ask event (e.g., when user answers it).
    /// - Parameter askEventId: The ask event ID whose notification should be removed.
    func removeNotification(askEventId: String) {
        notificationCenter.removePendingNotificationRequests(withIdentifiers: ["ask-\(askEventId)"])
        notificationCenter.removeDeliveredNotifications(withIdentifiers: ["ask-\(askEventId)"])
        print("[NotificationService] Removed notification for ask event: \(askEventId.prefix(12))...")
    }

    /// Remove all pending ask notifications.
    func removeAllNotifications() {
        notificationCenter.removeAllPendingNotificationRequests()
        notificationCenter.removeAllDeliveredNotifications()
        print("[NotificationService] Removed all notifications")
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
    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        didReceive response: UNNotificationResponse
    ) async {
        let userInfo = response.notification.request.content.userInfo

        // Extract navigation info for deep linking
        if let type = userInfo["type"] as? String, type == "ask" {
            if let conversationId = userInfo["conversationId"] as? String {
                print("[NotificationService] User tapped notification for conversation: \(conversationId.prefix(12))...")
                // Deep linking can be handled via a shared navigation state manager
                // For now, just log - UI layer will handle navigation
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
}

// MARK: - Notification Names

extension Notification.Name {
    /// Posted when user taps an ask notification.
    /// userInfo contains "conversationId" for navigation.
    static let askNotificationTapped = Notification.Name("askNotificationTapped")
}
