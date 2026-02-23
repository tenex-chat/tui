import Foundation
import os.log

#if os(iOS)
import UIKit
#endif

// MARK: - TenexCoreManager Push Notifications

struct PushRegistrationDebugInfo: Sendable {
    let deviceId: String
    let tokenPreview: String
    let tokenLength: Int
    let tokenUpdatedAt: UInt64?
    let lastPublishAt: UInt64?
    let lastPublishEnable: Bool?
    let lastPublishBackends: [String]
    let lastPublishError: String?
    let approvedBackends: [String]
}

/// Handles APNs (Apple Push Notification service) registration and lifecycle.
///
/// ## Flow
/// 1. User grants notification permission (handled in `AppRootSceneView`).
/// 2. `registerForRemoteNotifications()` is called — this schedules APNs registration.
/// 3. iOS delivers the token to `AppDelegate.didRegisterForRemoteNotificationsWithDeviceToken`.
/// 4. AppDelegate posts `Notification.Name.apnsTokenRegistered` via NotificationCenter.
/// 5. The observer set up in `registerApnsObserver()` receives it and calls
///    `publishApnsRegistration(token:)`.
/// 6. The token is published to approved backends via kind:25000 Nostr events
///    (NIP-44 encrypted, one event per backend).
///
/// Registration is re-attempted on every app launch because tokens can change.
/// Deregistration (`enable: false`) is published on logout.
extension TenexCoreManager {

    private nonisolated static let pushLogger = Logger(subsystem: "com.tenex.mvp", category: "PushNotifications")
    private nonisolated static let apnsTokenDefaultsKey = "push.apns.cachedToken"
    private nonisolated static let apnsTokenUpdatedAtDefaultsKey = "push.apns.tokenUpdatedAt"
    private nonisolated static let apnsLastPublishAtDefaultsKey = "push.apns.lastPublishAt"
    private nonisolated static let apnsLastPublishEnableDefaultsKey = "push.apns.lastPublishEnable"
    private nonisolated static let apnsLastPublishBackendsDefaultsKey = "push.apns.lastPublishBackends"
    private nonisolated static let apnsLastPublishErrorDefaultsKey = "push.apns.lastPublishError"

    nonisolated static func normalizedBackendPubkeys(_ pubkeys: [String]) -> [String] {
        let normalized = pubkeys
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
        return Array(Set(normalized)).sorted()
    }

    private nonisolated static func tokenPreview(_ token: String) -> String {
        guard !token.isEmpty else { return "(none)" }
        if token.count <= 20 {
            return token
        }
        return "\(token.prefix(12))...\(token.suffix(8))"
    }

    private nonisolated static func storeCachedApnsToken(_ token: String) {
        let defaults = UserDefaults.standard
        defaults.set(token, forKey: apnsTokenDefaultsKey)
        defaults.set(UInt64(Date().timeIntervalSince1970), forKey: apnsTokenUpdatedAtDefaultsKey)
    }

    private nonisolated static func cachedApnsToken() -> String? {
        UserDefaults.standard.string(forKey: apnsTokenDefaultsKey)
    }

    private nonisolated static func tokenUpdatedAt() -> UInt64? {
        let value = UserDefaults.standard.object(forKey: apnsTokenUpdatedAtDefaultsKey) as? NSNumber
        return value?.uint64Value
    }

    private nonisolated static func recordPushPublishResult(
        enable: Bool,
        backendPubkeys: [String],
        error: String?
    ) {
        let defaults = UserDefaults.standard
        defaults.set(UInt64(Date().timeIntervalSince1970), forKey: apnsLastPublishAtDefaultsKey)
        defaults.set(enable, forKey: apnsLastPublishEnableDefaultsKey)
        defaults.set(backendPubkeys, forKey: apnsLastPublishBackendsDefaultsKey)
        if let error, !error.isEmpty {
            defaults.set(error, forKey: apnsLastPublishErrorDefaultsKey)
        } else {
            defaults.removeObject(forKey: apnsLastPublishErrorDefaultsKey)
        }
    }

    private nonisolated static func approvedBackendPubkeys(safeCore: SafeTenexCore) async throws -> [String] {
        let snapshot = try await safeCore.getBackendTrustSnapshot()
        return normalizedBackendPubkeys(snapshot.approved)
    }

    private nonisolated static func publishApnsEventToApprovedBackends(
        safeCore: SafeTenexCore,
        deviceToken: String,
        enable: Bool,
        deviceId: String
    ) async throws -> [String] {
        let backendPubkeys = try await approvedBackendPubkeys(safeCore: safeCore)
        guard !backendPubkeys.isEmpty else { return [] }

        for backendPubkey in backendPubkeys {
            try await safeCore.registerApnsToken(
                deviceToken: deviceToken,
                enable: enable,
                backendPubkey: backendPubkey,
                deviceId: deviceId
            )
        }

        return backendPubkeys
    }

    @MainActor
    func currentPushRegistrationDebugInfo(approvedBackends: [String]) -> PushRegistrationDebugInfo {
        let token = Self.cachedApnsToken() ?? ""
        let lastPublishAt = (UserDefaults.standard.object(forKey: Self.apnsLastPublishAtDefaultsKey) as? NSNumber)?.uint64Value
        let lastPublishEnable = (UserDefaults.standard.object(forKey: Self.apnsLastPublishEnableDefaultsKey) as? NSNumber)?.boolValue
        let lastPublishBackends = Self.normalizedBackendPubkeys(
            UserDefaults.standard.stringArray(forKey: Self.apnsLastPublishBackendsDefaultsKey) ?? []
        )
        let lastPublishError = UserDefaults.standard.string(forKey: Self.apnsLastPublishErrorDefaultsKey)

        #if os(iOS)
        let deviceId = UIDevice.current.identifierForVendor?.uuidString ?? "unknown"
        #else
        let deviceId = "n/a"
        #endif

        return PushRegistrationDebugInfo(
            deviceId: deviceId,
            tokenPreview: Self.tokenPreview(token),
            tokenLength: token.count,
            tokenUpdatedAt: Self.tokenUpdatedAt(),
            lastPublishAt: lastPublishAt,
            lastPublishEnable: lastPublishEnable,
            lastPublishBackends: lastPublishBackends,
            lastPublishError: lastPublishError,
            approvedBackends: Self.normalizedBackendPubkeys(approvedBackends)
        )
    }

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

    /// Ask APNs for a fresh token and immediately republish the cached token
    /// (if available) to all approved backends.
    @MainActor
    func reregisterPushDeviceNow() async {
        #if os(iOS)
        registerForRemoteNotifications()
        await republishCachedApnsRegistrationNow()
        #endif
    }

    /// Republish the currently cached APNs token to all approved backends.
    @MainActor
    func republishCachedApnsRegistrationNow() async {
        #if os(iOS)
        guard let token = Self.cachedApnsToken(), !token.isEmpty else {
            let message = "No cached APNs token yet; waiting for APNs callback"
            Self.pushLogger.warning("\(message)")
            Self.recordPushPublishResult(enable: true, backendPubkeys: [], error: message)
            return
        }
        await publishApnsRegistration(token: token)
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
        Self.storeCachedApnsToken(token)
        Task { @MainActor [weak self] in
            await self?.publishApnsRegistration(token: token)
        }
        #endif
    }

    /// Publish kind:25000 registration events to approved backends via the Rust core.
    @MainActor
    private func publishApnsRegistration(token: String) async {
        #if os(iOS)
        Self.storeCachedApnsToken(token)
        await publishApnsToken(deviceToken: token, enable: true)
        #endif
    }

    /// Publish kind:25000 deregistration events (enable: false) on logout.
    @MainActor
    private func publishApnsDeregistration() async {
        #if os(iOS)
        await publishApnsToken(deviceToken: "", enable: false)
        #endif
    }

    @MainActor
    private func publishApnsToken(deviceToken: String, enable: Bool) async {
        #if os(iOS)
        guard core.isLoggedIn() else {
            let message = "Skipping APNs publish — not logged in"
            Self.pushLogger.info("\(message)")
            Self.recordPushPublishResult(enable: enable, backendPubkeys: [], error: message)
            return
        }

        let deviceId = UIDevice.current.identifierForVendor?.uuidString ?? "unknown"
        let safeCore = self.safeCore
        let action = enable ? "registration" : "deregistration"
        Self.pushLogger.info("Publishing APNs \(action), device: \(deviceId.prefix(8))...")

        do {
            let backendPubkeys = try await Self.publishApnsEventToApprovedBackends(
                safeCore: safeCore,
                deviceToken: deviceToken,
                enable: enable,
                deviceId: deviceId
            )

            if backendPubkeys.isEmpty {
                let message = "Skipping APNs \(action) publish — no approved backends"
                Self.pushLogger.warning("\(message)")
                Self.recordPushPublishResult(enable: enable, backendPubkeys: [], error: message)
            } else {
                Self.recordPushPublishResult(enable: enable, backendPubkeys: backendPubkeys, error: nil)
                Self.pushLogger.info(
                    "APNs \(action) events published to \(backendPubkeys.count) approved backend(s)"
                )
            }
        } catch {
            let message = "Failed to publish APNs \(action): \(error)"
            Self.pushLogger.error("\(message)")
            Self.recordPushPublishResult(enable: enable, backendPubkeys: [], error: message)
        }
        #endif
    }
}
