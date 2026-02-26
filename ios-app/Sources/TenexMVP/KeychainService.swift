import Foundation
import Security

// MARK: - Keychain Error Types

/// Errors that can occur during credential storage operations
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
            return "No credential found"
        case .duplicateItem:
            return "Credential already exists"
        case .unexpectedData:
            return "Unexpected credential data format"
        case .accessDenied:
            return "Access to credential storage denied"
        case .unhandledError(let status):
            return "Credential storage error: \(status)"
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

/// Service for storing and retrieving credentials.
/// On iOS/iPadOS this uses Keychain; on macOS this uses plaintext files.
/// All operations are designed to be called from background threads.
final class KeychainService {

    private enum CredentialDescriptor {
        case nsec
        case elevenLabsApiKey
        case openRouterApiKey

        var serviceKey: String {
            switch self {
            case .nsec:
                return "com.tenex.mvp.nsec"
            case .elevenLabsApiKey:
                return "com.tenex.mvp.elevenlabs"
            case .openRouterApiKey:
                return "com.tenex.mvp.openrouter"
            }
        }

        var accountKey: String {
            switch self {
            case .nsec:
                return "tenex-user-nsec"
            case .elevenLabsApiKey:
                return "tenex-elevenlabs-api-key"
            case .openRouterApiKey:
                return "tenex-openrouter-api-key"
            }
        }

        var fileName: String {
            switch self {
            case .nsec:
                return "nsec.txt"
            case .elevenLabsApiKey:
                return "elevenlabs_api_key.txt"
            case .openRouterApiKey:
                return "openrouter_api_key.txt"
            }
        }
    }

    // MARK: - Singleton

    static let shared = KeychainService()

    private init() {}

    // MARK: - Internal Sync API (Background Thread Only)

    /// Saves the nsec credential to credential storage
    /// - Parameter nsec: The nsec string to save
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func saveNsec(_ nsec: String) -> KeychainResult<Void> {
        saveCredential(nsec, descriptor: .nsec)
    }

    /// Retrieves the stored nsec credential from credential storage
    /// - Returns: Result containing the nsec string or specific failure
    /// - Precondition: Must be called from a background thread
    func loadNsec() -> KeychainResult<String> {
        loadCredential(descriptor: .nsec)
    }

    /// Deletes the stored nsec credential from credential storage
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func deleteNsec() -> KeychainResult<Void> {
        deleteCredential(descriptor: .nsec)
    }

    /// Checks if nsec credential exists in credential storage without retrieving it
    /// - Returns: Result indicating whether credential exists
    /// - Precondition: Must be called from a background thread
    func hasStoredNsec() -> KeychainResult<Bool> {
        hasCredential(descriptor: .nsec)
    }

    // MARK: - ElevenLabs API Key Methods

    /// Saves the ElevenLabs API key to credential storage
    /// - Parameter key: The API key string to save
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func saveElevenLabsApiKey(_ key: String) -> KeychainResult<Void> {
        saveCredential(key, descriptor: .elevenLabsApiKey)
    }

    /// Retrieves the stored ElevenLabs API key from credential storage
    /// - Returns: Result containing the API key or specific failure
    /// - Precondition: Must be called from a background thread
    func loadElevenLabsApiKey() -> KeychainResult<String> {
        loadCredential(descriptor: .elevenLabsApiKey)
    }

    /// Deletes the stored ElevenLabs API key from credential storage
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func deleteElevenLabsApiKey() -> KeychainResult<Void> {
        deleteCredential(descriptor: .elevenLabsApiKey)
    }

    /// Checks if ElevenLabs API key exists in credential storage without retrieving it
    /// - Returns: Result indicating whether credential exists
    /// - Precondition: Must be called from a background thread
    func hasElevenLabsApiKey() -> KeychainResult<Bool> {
        hasCredential(descriptor: .elevenLabsApiKey)
    }

    // MARK: - OpenRouter API Key Methods

    /// Saves the OpenRouter API key to credential storage
    /// - Parameter key: The API key string to save
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func saveOpenRouterApiKey(_ key: String) -> KeychainResult<Void> {
        saveCredential(key, descriptor: .openRouterApiKey)
    }

    /// Retrieves the stored OpenRouter API key from credential storage
    /// - Returns: Result containing the API key or specific failure
    /// - Precondition: Must be called from a background thread
    func loadOpenRouterApiKey() -> KeychainResult<String> {
        loadCredential(descriptor: .openRouterApiKey)
    }

    /// Deletes the stored OpenRouter API key from credential storage
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func deleteOpenRouterApiKey() -> KeychainResult<Void> {
        deleteCredential(descriptor: .openRouterApiKey)
    }

    /// Checks if OpenRouter API key exists in credential storage without retrieving it
    /// - Returns: Result indicating whether credential exists
    /// - Precondition: Must be called from a background thread
    func hasOpenRouterApiKey() -> KeychainResult<Bool> {
        hasCredential(descriptor: .openRouterApiKey)
    }

    private func saveCredential(_ value: String, descriptor: CredentialDescriptor) -> KeychainResult<Void> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return saveFileCredential(value, fileName: descriptor.fileName)
        #else
        return saveKeychainCredential(value, serviceKey: descriptor.serviceKey, accountKey: descriptor.accountKey)
        #endif
    }

    private func loadCredential(descriptor: CredentialDescriptor) -> KeychainResult<String> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return loadFileCredential(fileName: descriptor.fileName)
        #else
        return loadKeychainCredential(serviceKey: descriptor.serviceKey, accountKey: descriptor.accountKey)
        #endif
    }

    private func deleteCredential(descriptor: CredentialDescriptor) -> KeychainResult<Void> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return deleteFileCredential(fileName: descriptor.fileName)
        #else
        return deleteKeychainCredential(serviceKey: descriptor.serviceKey, accountKey: descriptor.accountKey)
        #endif
    }

    private func hasCredential(descriptor: CredentialDescriptor) -> KeychainResult<Bool> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return hasFileCredential(fileName: descriptor.fileName)
        #else
        return hasKeychainCredential(serviceKey: descriptor.serviceKey, accountKey: descriptor.accountKey)
        #endif
    }

    // MARK: - Private Helpers

    private var credentialDirectoryURL: URL {
        if let applicationSupport = FileManager.default.urls(for: .applicationSupportDirectory, in: .userDomainMask).first {
            return applicationSupport
                .appendingPathComponent("com.tenex.mvp", isDirectory: true)
                .appendingPathComponent("credentials", isDirectory: true)
        }

        return URL(fileURLWithPath: NSHomeDirectory(), isDirectory: true)
            .appendingPathComponent("Library", isDirectory: true)
            .appendingPathComponent("Application Support", isDirectory: true)
            .appendingPathComponent("com.tenex.mvp", isDirectory: true)
            .appendingPathComponent("credentials", isDirectory: true)
    }

    private func credentialFileURL(fileName: String) -> URL {
        credentialDirectoryURL.appendingPathComponent(fileName, isDirectory: false)
    }

    private func ensureCredentialDirectoryExists() throws {
        let directoryURL = credentialDirectoryURL
        if !FileManager.default.fileExists(atPath: directoryURL.path) {
            try FileManager.default.createDirectory(
                at: directoryURL,
                withIntermediateDirectories: true,
                attributes: nil
            )
        }
    }

    private func mapFileError(_ error: Error) -> KeychainError {
        guard let cocoaError = error as? CocoaError else {
            return .unhandledError(status: errSecIO)
        }

        switch cocoaError.code {
        case .fileReadNoSuchFile, .fileNoSuchFile:
            return .itemNotFound
        case .fileReadNoPermission, .fileWriteNoPermission:
            return .accessDenied
        default:
            return .unhandledError(status: errSecIO)
        }
    }

    private func saveFileCredential(_ value: String, fileName: String) -> KeychainResult<Void> {
        guard let data = value.data(using: .utf8) else {
            return .failure(.encodingFailed)
        }

        do {
            try ensureCredentialDirectoryExists()
            let fileURL = credentialFileURL(fileName: fileName)
            try data.write(to: fileURL, options: .atomic)
            try FileManager.default.setAttributes([.posixPermissions: 0o600], ofItemAtPath: fileURL.path)
            return .success(())
        } catch {
            return .failure(mapFileError(error))
        }
    }

    private func loadFileCredential(fileName: String) -> KeychainResult<String> {
        let fileURL = credentialFileURL(fileName: fileName)
        guard FileManager.default.fileExists(atPath: fileURL.path) else {
            return .failure(.itemNotFound)
        }

        do {
            let data = try Data(contentsOf: fileURL)
            guard let value = String(data: data, encoding: .utf8) else {
                return .failure(.decodingFailed)
            }
            return .success(value)
        } catch {
            return .failure(mapFileError(error))
        }
    }

    private func deleteFileCredential(fileName: String) -> KeychainResult<Void> {
        let fileURL = credentialFileURL(fileName: fileName)
        guard FileManager.default.fileExists(atPath: fileURL.path) else {
            return .success(())
        }

        do {
            try FileManager.default.removeItem(at: fileURL)
            return .success(())
        } catch {
            return .failure(mapFileError(error))
        }
    }

    private func hasFileCredential(fileName: String) -> KeychainResult<Bool> {
        let fileURL = credentialFileURL(fileName: fileName)
        return .success(FileManager.default.fileExists(atPath: fileURL.path))
    }

    private func saveKeychainCredential(_ value: String, serviceKey: String, accountKey: String) -> KeychainResult<Void> {
        guard let valueData = value.data(using: .utf8) else {
            return .failure(.encodingFailed)
        }

        let updateQuery: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: serviceKey,
            kSecAttrAccount as String: accountKey
        ]

        let updateAttributes: [String: Any] = [
            kSecValueData as String: valueData,
            kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
        ]

        var status = SecItemUpdate(updateQuery as CFDictionary, updateAttributes as CFDictionary)

        if status == errSecItemNotFound {
            let addQuery: [String: Any] = [
                kSecClass as String: kSecClassGenericPassword,
                kSecAttrService as String: serviceKey,
                kSecAttrAccount as String: accountKey,
                kSecValueData as String: valueData,
                kSecAttrAccessible as String: kSecAttrAccessibleWhenUnlockedThisDeviceOnly
            ]
            status = SecItemAdd(addQuery as CFDictionary, nil)
        }

        return mapOSStatus(status)
    }

    private func loadKeychainCredential(serviceKey: String, accountKey: String) -> KeychainResult<String> {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: serviceKey,
            kSecAttrAccount as String: accountKey,
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

        guard let value = String(data: data, encoding: .utf8) else {
            return .failure(.decodingFailed)
        }

        return .success(value)
    }

    private func deleteKeychainCredential(serviceKey: String, accountKey: String) -> KeychainResult<Void> {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: serviceKey,
            kSecAttrAccount as String: accountKey
        ]

        let status = SecItemDelete(query as CFDictionary)
        if status == errSecItemNotFound {
            return .success(())
        }

        return mapOSStatus(status)
    }

    private func hasKeychainCredential(serviceKey: String, accountKey: String) -> KeychainResult<Bool> {
        let query: [String: Any] = [
            kSecClass as String: kSecClassGenericPassword,
            kSecAttrService as String: serviceKey,
            kSecAttrAccount as String: accountKey,
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
    /// Runs synchronous credential storage work on a background queue and returns the result asynchronously.
    private func runAsync<T>(_ operation: @escaping () -> KeychainResult<T>) async -> KeychainResult<T> {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                continuation.resume(returning: operation())
            }
        }
    }

    private func saveCredentialAsync(_ value: String, descriptor: CredentialDescriptor) async -> KeychainResult<Void> {
        await runAsync { self.saveCredential(value, descriptor: descriptor) }
    }

    private func loadCredentialAsync(descriptor: CredentialDescriptor) async -> KeychainResult<String> {
        await runAsync { self.loadCredential(descriptor: descriptor) }
    }

    private func deleteCredentialAsync(descriptor: CredentialDescriptor) async -> KeychainResult<Void> {
        await runAsync { self.deleteCredential(descriptor: descriptor) }
    }

    private func hasCredentialAsync(descriptor: CredentialDescriptor) async -> KeychainResult<Bool> {
        await runAsync { self.hasCredential(descriptor: descriptor) }
    }

    /// Saves nsec credential asynchronously on a background thread
    /// - Parameter nsec: The nsec string to save
    /// - Returns: Result indicating success or failure
    func saveNsecAsync(_ nsec: String) async -> KeychainResult<Void> {
        await saveCredentialAsync(nsec, descriptor: .nsec)
    }

    /// Loads nsec credential asynchronously on a background thread
    /// - Returns: Result containing the nsec or failure
    func loadNsecAsync() async -> KeychainResult<String> {
        await loadCredentialAsync(descriptor: .nsec)
    }

    /// Deletes nsec credential asynchronously on a background thread
    /// - Returns: Result indicating success or failure
    func deleteNsecAsync() async -> KeychainResult<Void> {
        await deleteCredentialAsync(descriptor: .nsec)
    }

    /// Checks for stored credential asynchronously on a background thread
    /// - Returns: Result indicating whether credential exists
    func hasStoredNsecAsync() async -> KeychainResult<Bool> {
        await hasCredentialAsync(descriptor: .nsec)
    }

    // MARK: - ElevenLabs Async Extensions

    /// Saves ElevenLabs API key asynchronously on a background thread
    /// - Parameter key: The API key to save
    /// - Returns: Result indicating success or failure
    func saveElevenLabsApiKeyAsync(_ key: String) async -> KeychainResult<Void> {
        await saveCredentialAsync(key, descriptor: .elevenLabsApiKey)
    }

    /// Loads ElevenLabs API key asynchronously on a background thread
    /// - Returns: Result containing the API key or failure
    func loadElevenLabsApiKeyAsync() async -> KeychainResult<String> {
        await loadCredentialAsync(descriptor: .elevenLabsApiKey)
    }

    /// Deletes ElevenLabs API key asynchronously on a background thread
    /// - Returns: Result indicating success or failure
    func deleteElevenLabsApiKeyAsync() async -> KeychainResult<Void> {
        await deleteCredentialAsync(descriptor: .elevenLabsApiKey)
    }

    /// Checks for stored ElevenLabs API key asynchronously on a background thread
    /// - Returns: Result indicating whether credential exists
    func hasElevenLabsApiKeyAsync() async -> KeychainResult<Bool> {
        await hasCredentialAsync(descriptor: .elevenLabsApiKey)
    }

    // MARK: - OpenRouter Async Extensions

    /// Saves OpenRouter API key asynchronously on a background thread
    /// - Parameter key: The API key to save
    /// - Returns: Result indicating success or failure
    func saveOpenRouterApiKeyAsync(_ key: String) async -> KeychainResult<Void> {
        await saveCredentialAsync(key, descriptor: .openRouterApiKey)
    }

    /// Loads OpenRouter API key asynchronously on a background thread
    /// - Returns: Result containing the API key or failure
    func loadOpenRouterApiKeyAsync() async -> KeychainResult<String> {
        await loadCredentialAsync(descriptor: .openRouterApiKey)
    }

    /// Deletes OpenRouter API key asynchronously on a background thread
    /// - Returns: Result indicating success or failure
    func deleteOpenRouterApiKeyAsync() async -> KeychainResult<Void> {
        await deleteCredentialAsync(descriptor: .openRouterApiKey)
    }

    /// Checks for stored OpenRouter API key asynchronously on a background thread
    /// - Returns: Result indicating whether credential exists
    func hasOpenRouterApiKeyAsync() async -> KeychainResult<Bool> {
        await hasCredentialAsync(descriptor: .openRouterApiKey)
    }
}
