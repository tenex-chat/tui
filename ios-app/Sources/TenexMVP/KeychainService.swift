import Foundation
import Security

// MARK: - Keychain Error Types

/// Errors that can occur during Keychain operations
enum KeychainError: Error, LocalizedError {
    case itemNotFound
    case duplicateItem
    case unexpectedData
    case accessDenied
    case unhandledError(status: OSStatus)
    case encodingFailed
    case decodingFailed

    var errorDescription: String? {
        switch self {
        case .itemNotFound:
            return "No credential found in Keychain"
        case .duplicateItem:
            return "Credential already exists in Keychain"
        case .unexpectedData:
            return "Unexpected data format in Keychain"
        case .accessDenied:
            return "Access to Keychain denied"
        case .unhandledError(let status):
            return "Keychain error: \(status)"
        case .encodingFailed:
            return "Failed to encode credential data"
        case .decodingFailed:
            return "Failed to decode credential data"
        }
    }
}

/// Result type for Keychain operations
typealias KeychainResult<T> = Result<T, KeychainError>

// MARK: - Keychain Service

/// Service for securely storing and retrieving credentials from iOS Keychain
/// All operations are designed to be called from background threads
final class KeychainService {

    // MARK: - Constants

    /// Unique identifier for the nsec credential item
    private static let nsecServiceKey = "com.tenex.mvp.nsec"
    private static let nsecAccountKey = "tenex-user-nsec"

    /// Unique identifier for ElevenLabs API key
    private static let elevenLabsServiceKey = "com.tenex.mvp.elevenlabs"
    private static let elevenLabsAccountKey = "tenex-elevenlabs-api-key"

    /// Unique identifier for OpenRouter API key
    private static let openRouterServiceKey = "com.tenex.mvp.openrouter"
    private static let openRouterAccountKey = "tenex-openrouter-api-key"

    // MARK: - Singleton

    static let shared = KeychainService()

    private init() {}

    // MARK: - Internal Sync API (Background Thread Only)

    /// Saves the nsec credential to Keychain
    /// - Parameter nsec: The nsec string to save
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func saveNsec(_ nsec: String) -> KeychainResult<Void> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        guard let nsecData = nsec.data(using: .utf8) else {
            return .failure(.encodingFailed)
        }

        // First, try to update existing item
        let updateQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.nsecServiceKey,
            kSecAttrAccount as String: Self.nsecAccountKey
        ]

        let updateAttributes: [String: Any] = [
            kSecValueData as String: nsecData,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        ]

        var status = SecItemUpdate(updateQuery as CFDictionary, updateAttributes as CFDictionary)

        if status == errSecItemNotFound {
            // Item doesn't exist, add it
            let addQuery: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: Self.nsecServiceKey,
                kSecAttrAccount as String: Self.nsecAccountKey,
                kSecValueData as String: nsecData,
                kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            ]

            status = SecItemAdd(addQuery as CFDictionary, nil)
        }

        return mapOSStatus(status)
    }

    /// Retrieves the stored nsec credential from Keychain
    /// - Returns: Result containing the nsec string or specific failure
    /// - Precondition: Must be called from a background thread
    func loadNsec() -> KeychainResult<String> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.nsecServiceKey,
            kSecAttrAccount as String: Self.nsecAccountKey,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        guard status == errSecSuccess else {
            return .failure(mapOSStatusToError(status))
        }

        guard let data = result as? Data else {
            return .failure(.unexpectedData)
        }

        guard let nsec = String(data: data, encoding: .utf8) else {
            return .failure(.decodingFailed)
        }

        return .success(nsec)
    }

    /// Deletes the stored nsec credential from Keychain
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func deleteNsec() -> KeychainResult<Void> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.nsecServiceKey,
            kSecAttrAccount as String: Self.nsecAccountKey
        ]

        let status = SecItemDelete(query as CFDictionary)

        // Treat "not found" as success for deletion
        if status == errSecItemNotFound {
            return .success(())
        }

        return mapOSStatus(status)
    }

    /// Checks if nsec credential exists in Keychain without retrieving it
    /// - Returns: Result indicating whether credential exists
    /// - Precondition: Must be called from a background thread
    func hasStoredNsec() -> KeychainResult<Bool> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.nsecServiceKey,
            kSecAttrAccount as String: Self.nsecAccountKey,
            kSecReturnData as String: false // Just check existence
        ]

        let status = SecItemCopyMatching(query as CFDictionary, nil)

        switch status {
        case errSecSuccess:
            return .success(true)
        case errSecItemNotFound:
            return .success(false)
        default:
            return .failure(mapOSStatusToError(status))
        }
    }

    // MARK: - ElevenLabs API Key Methods

    /// Saves the ElevenLabs API key to Keychain
    /// - Parameter key: The API key string to save
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func saveElevenLabsApiKey(_ key: String) -> KeychainResult<Void> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        guard let keyData = key.data(using: .utf8) else {
            return .failure(.encodingFailed)
        }

        let updateQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.elevenLabsServiceKey,
            kSecAttrAccount as String: Self.elevenLabsAccountKey
        ]

        let updateAttributes: [String: Any] = [
            kSecValueData as String: keyData,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        ]

        var status = SecItemUpdate(updateQuery as CFDictionary, updateAttributes as CFDictionary)

        if status == errSecItemNotFound {
            let addQuery: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: Self.elevenLabsServiceKey,
                kSecAttrAccount as String: Self.elevenLabsAccountKey,
                kSecValueData as String: keyData,
                kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            ]

            status = SecItemAdd(addQuery as CFDictionary, nil)
        }

        return mapOSStatus(status)
    }

    /// Retrieves the stored ElevenLabs API key from Keychain
    /// - Returns: Result containing the API key or specific failure
    /// - Precondition: Must be called from a background thread
    func loadElevenLabsApiKey() -> KeychainResult<String> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.elevenLabsServiceKey,
            kSecAttrAccount as String: Self.elevenLabsAccountKey,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        guard status == errSecSuccess else {
            return .failure(mapOSStatusToError(status))
        }

        guard let data = result as? Data else {
            return .failure(.unexpectedData)
        }

        guard let key = String(data: data, encoding: .utf8) else {
            return .failure(.decodingFailed)
        }

        return .success(key)
    }

    /// Deletes the stored ElevenLabs API key from Keychain
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func deleteElevenLabsApiKey() -> KeychainResult<Void> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.elevenLabsServiceKey,
            kSecAttrAccount as String: Self.elevenLabsAccountKey
        ]

        let status = SecItemDelete(query as CFDictionary)

        if status == errSecItemNotFound {
            return .success(())
        }

        return mapOSStatus(status)
    }

    /// Checks if ElevenLabs API key exists in Keychain without retrieving it
    /// - Returns: Result indicating whether credential exists
    /// - Precondition: Must be called from a background thread
    func hasElevenLabsApiKey() -> KeychainResult<Bool> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.elevenLabsServiceKey,
            kSecAttrAccount as String: Self.elevenLabsAccountKey,
            kSecReturnData as String: false
        ]

        let status = SecItemCopyMatching(query as CFDictionary, nil)

        switch status {
        case errSecSuccess:
            return .success(true)
        case errSecItemNotFound:
            return .success(false)
        default:
            return .failure(mapOSStatusToError(status))
        }
    }

    // MARK: - OpenRouter API Key Methods

    /// Saves the OpenRouter API key to Keychain
    /// - Parameter key: The API key string to save
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func saveOpenRouterApiKey(_ key: String) -> KeychainResult<Void> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        guard let keyData = key.data(using: .utf8) else {
            return .failure(.encodingFailed)
        }

        let updateQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.openRouterServiceKey,
            kSecAttrAccount as String: Self.openRouterAccountKey
        ]

        let updateAttributes: [String: Any] = [
            kSecValueData as String: keyData,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        ]

        var status = SecItemUpdate(updateQuery as CFDictionary, updateAttributes as CFDictionary)

        if status == errSecItemNotFound {
            let addQuery: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: Self.openRouterServiceKey,
                kSecAttrAccount as String: Self.openRouterAccountKey,
                kSecValueData as String: keyData,
                kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            ]

            status = SecItemAdd(addQuery as CFDictionary, nil)
        }

        return mapOSStatus(status)
    }

    /// Retrieves the stored OpenRouter API key from Keychain
    /// - Returns: Result containing the API key or specific failure
    /// - Precondition: Must be called from a background thread
    func loadOpenRouterApiKey() -> KeychainResult<String> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.openRouterServiceKey,
            kSecAttrAccount as String: Self.openRouterAccountKey,
            kSecReturnData as String: true,
            kSecMatchLimit as String: kSecMatchLimitOne
        ]

        var result: AnyObject?
        let status = SecItemCopyMatching(query as CFDictionary, &result)

        guard status == errSecSuccess else {
            return .failure(mapOSStatusToError(status))
        }

        guard let data = result as? Data else {
            return .failure(.unexpectedData)
        }

        guard let key = String(data: data, encoding: .utf8) else {
            return .failure(.decodingFailed)
        }

        return .success(key)
    }

    /// Deletes the stored OpenRouter API key from Keychain
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func deleteOpenRouterApiKey() -> KeychainResult<Void> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.openRouterServiceKey,
            kSecAttrAccount as String: Self.openRouterAccountKey
        ]

        let status = SecItemDelete(query as CFDictionary)

        if status == errSecItemNotFound {
            return .success(())
        }

        return mapOSStatus(status)
    }

    /// Checks if OpenRouter API key exists in Keychain without retrieving it
    /// - Returns: Result indicating whether credential exists
    /// - Precondition: Must be called from a background thread
    func hasOpenRouterApiKey() -> KeychainResult<Bool> {
        precondition(!Thread.isMainThread, "Keychain operations must not be called on the main thread")
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: Self.openRouterServiceKey,
            kSecAttrAccount as String: Self.openRouterAccountKey,
            kSecReturnData as String: false
        ]

        let status = SecItemCopyMatching(query as CFDictionary, nil)

        switch status {
        case errSecSuccess:
            return .success(true)
        case errSecItemNotFound:
            return .success(false)
        default:
            return .failure(mapOSStatusToError(status))
        }
    }

    // MARK: - Private Helpers

    /// Maps an OSStatus to a Result<Void, KeychainError>
    private func mapOSStatus(_ status: OSStatus) -> KeychainResult<Void> {
        if status == errSecSuccess {
            return .success(())
        }
        return .failure(mapOSStatusToError(status))
    }

    /// Maps an OSStatus to a KeychainError
    private func mapOSStatusToError(_ status: OSStatus) -> KeychainError {
        switch status {
        case errSecItemNotFound:
            return .itemNotFound
        case errSecDuplicateItem:
            return .duplicateItem
        case errSecAuthFailed, errSecInteractionNotAllowed:
            return .accessDenied
        default:
            return .unhandledError(status: status)
        }
    }
}

// MARK: - Async Extensions

extension KeychainService {

    /// Saves nsec credential asynchronously on a background thread
    /// - Parameter nsec: The nsec string to save
    /// - Returns: Result indicating success or failure
    func saveNsecAsync(_ nsec: String) async -> KeychainResult<Void> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.saveNsec(nsec)
                continuation.resume(returning: result)
            }
        }
    }

    /// Loads nsec credential asynchronously on a background thread
    /// - Returns: Result containing the nsec or failure
    func loadNsecAsync() async -> KeychainResult<String> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.loadNsec()
                continuation.resume(returning: result)
            }
        }
    }

    /// Deletes nsec credential asynchronously on a background thread
    /// - Returns: Result indicating success or failure
    func deleteNsecAsync() async -> KeychainResult<Void> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.deleteNsec()
                continuation.resume(returning: result)
            }
        }
    }

    /// Checks for stored credential asynchronously on a background thread
    /// - Returns: Result indicating whether credential exists
    func hasStoredNsecAsync() async -> KeychainResult<Bool> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.hasStoredNsec()
                continuation.resume(returning: result)
            }
        }
    }

    // MARK: - ElevenLabs Async Extensions

    /// Saves ElevenLabs API key asynchronously on a background thread
    /// - Parameter key: The API key to save
    /// - Returns: Result indicating success or failure
    func saveElevenLabsApiKeyAsync(_ key: String) async -> KeychainResult<Void> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.saveElevenLabsApiKey(key)
                continuation.resume(returning: result)
            }
        }
    }

    /// Loads ElevenLabs API key asynchronously on a background thread
    /// - Returns: Result containing the API key or failure
    func loadElevenLabsApiKeyAsync() async -> KeychainResult<String> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.loadElevenLabsApiKey()
                continuation.resume(returning: result)
            }
        }
    }

    /// Deletes ElevenLabs API key asynchronously on a background thread
    /// - Returns: Result indicating success or failure
    func deleteElevenLabsApiKeyAsync() async -> KeychainResult<Void> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.deleteElevenLabsApiKey()
                continuation.resume(returning: result)
            }
        }
    }

    /// Checks for stored ElevenLabs API key asynchronously on a background thread
    /// - Returns: Result indicating whether credential exists
    func hasElevenLabsApiKeyAsync() async -> KeychainResult<Bool> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.hasElevenLabsApiKey()
                continuation.resume(returning: result)
            }
        }
    }

    // MARK: - OpenRouter Async Extensions

    /// Saves OpenRouter API key asynchronously on a background thread
    /// - Parameter key: The API key to save
    /// - Returns: Result indicating success or failure
    func saveOpenRouterApiKeyAsync(_ key: String) async -> KeychainResult<Void> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.saveOpenRouterApiKey(key)
                continuation.resume(returning: result)
            }
        }
    }

    /// Loads OpenRouter API key asynchronously on a background thread
    /// - Returns: Result containing the API key or failure
    func loadOpenRouterApiKeyAsync() async -> KeychainResult<String> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.loadOpenRouterApiKey()
                continuation.resume(returning: result)
            }
        }
    }

    /// Deletes OpenRouter API key asynchronously on a background thread
    /// - Returns: Result indicating success or failure
    func deleteOpenRouterApiKeyAsync() async -> KeychainResult<Void> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.deleteOpenRouterApiKey()
                continuation.resume(returning: result)
            }
        }
    }

    /// Checks for stored OpenRouter API key asynchronously on a background thread
    /// - Returns: Result indicating whether credential exists
    func hasOpenRouterApiKeyAsync() async -> KeychainResult<Bool> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                let result = self.hasOpenRouterApiKey()
                continuation.resume(returning: result)
            }
        }
    }
}
