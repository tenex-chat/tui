import Foundation
import os.log

#if os(iOS)
import UIKit
#endif

// MARK: - TenexCoreManager Push Notifications

/// Handles APNs (Apple Push Notification service) registration and lifecycle.
///
/// ## Flow
/// 1. User grants notification permission (handled in `AppRootSceneView`).
/// 2. `registerForRemoteNotifications()` is called — this schedules APNs registration.
/// 3. iOS delivers the token to `AppDelegate.didRegisterForRemoteNotificationsWithDeviceToken`.
/// 4. AppDelegate posts `Notification.Name.apnsTokenRegistered` via NotificationCenter.
/// 5. The observer set up in `registerApnsObserver()` receives it and calls
///    `handleApnsTokenReceived(_:)`.
/// 6. The token is published to the backend via a kind:25000 Nostr event (NIP-44 encrypted).
///
/// Registration is re-attempted on every app launch because tokens can change.
/// Deregistration (`enable: false`) is published on logout.
extension TenexCoreManager {

    // MARK: - Constants

    /// Hex-encoded public key of the TENEX backend that handles APNs delivery.
    /// The backend subscribes to kind:25000 events addressed to this pubkey and
    /// uses the decrypted payload to register/deregister devices with APNs.
    ///
    /// TODO: Make this configurable via Settings once the production backend pubkey
    ///       is finalised.  For now the owner's pubkey is used as a default.
    static let tenexBackendPubkey = "09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7"

    private static let pushLogger = Logger(subsystem: "com.tenex.mvp", category: "PushNotifications")

    // MARK: - Observer Lifecycle

    /// Begin observing APNs token delivery notifications.
    ///
    /// Must be called after login (i.e. when `registerEventCallback()` is called)
    /// so token-received events are only processed while a user session is active.
    /// Call `unregisterApnsObserver()` on logout.
    @MainActor
    func registerApnsObserver() {
        #if os(iOS)
        NotificationCenter.default.addObserver(
            self,
            selector: #selector(apnsTokenNotificationReceived(_:)),
            name: .apnsTokenRegistered,
            object: nil
        )
        Self.pushLogger.debug("APNs token observer registered")
        #endif
    }

    /// Stop observing APNs token delivery notifications and deregister the device.
    @MainActor
    func unregisterApnsObserver() {
        #if os(iOS)
        NotificationCenter.default.removeObserver(self, name: .apnsTokenRegistered, object: nil)
        Self.pushLogger.debug("APNs token observer removed")

        // Publish a disable event so the backend stops sending pushes to this device.
        Task { @MainActor [weak self] in
            await self?.publishApnsDeregistration()
        }
        #endif
    }

    // MARK: - Token Registration

    /// Ask iOS to register this device with APNs and deliver a fresh token.
    ///
    /// Safe to call from any context — schedules the registration on the main queue.
    /// Must be called **after** notification permission is granted.
    ///
    /// Tokens can change (e.g. after backup-restore, OS update, etc.) so this
    /// should be called on every app launch after a successful login.
    @MainActor
    func registerForRemoteNotifications() {
        #if os(iOS)
        UIApplication.shared.registerForRemoteNotifications()
        Self.pushLogger.info("Requested APNs device token registration")
        #endif
    }

    // MARK: - Private Handlers

    /// Selector-compatible wrapper for the NotificationCenter observer callback.
    @objc private func apnsTokenNotificationReceived(_ notification: Foundation.Notification) {
        #if os(iOS)
        guard let token = notification.userInfo?[APNSNotificationKey.deviceToken] as? String,
              !token.isEmpty else {
            Self.pushLogger.warning("apnsTokenRegistered notification missing token")
            return
        }
        Task { @MainActor [weak self] in
            await self?.publishApnsRegistration(token: token)
        }
        #endif
    }

    /// Publish a kind:25000 registration event to the backend via the Rust core.
    @MainActor
    private func publishApnsRegistration(token: String) async {
        #if os(iOS)
        guard core.isLoggedIn() else {
            Self.pushLogger.info("Skipping APNs registration publish — not logged in")
            return
        }

        let deviceId = UIDevice.current.identifierForVendor?.uuidString ?? "unknown"
        Self.pushLogger.info("Publishing APNs registration, device: \(deviceId.prefix(8))...")

        let safeCore = self.safeCore
        Task.detached(priority: .utility) {
            do {
                try await safeCore.registerApnsToken(
                    deviceToken: token,
                    enable: true,
                    backendPubkey: TenexCoreManager.tenexBackendPubkey,
                    deviceId: deviceId
                )
                Self.pushLogger.info("APNs registration event published successfully")
            } catch {
                Self.pushLogger.error("Failed to publish APNs registration: \(error)")
            }
        }
        #endif
    }

    /// Publish a kind:25000 deregistration event (enable: false) on logout.
    @MainActor
    private func publishApnsDeregistration() async {
        #if os(iOS)
        guard core.isLoggedIn() else { return }

        let deviceId = UIDevice.current.identifierForVendor?.uuidString ?? "unknown"
        Self.pushLogger.info("Publishing APNs deregistration")

        let safeCore = self.safeCore
        Task.detached(priority: .utility) {
            do {
                try await safeCore.registerApnsToken(
                    deviceToken: "",
                    enable: false,
                    backendPubkey: TenexCoreManager.tenexBackendPubkey,
                    deviceId: deviceId
                )
                Self.pushLogger.info("APNs deregistration event published successfully")
            } catch {
                Self.pushLogger.error("Failed to publish APNs deregistration: \(error)")
            }
        }
        #endif
    }
}
