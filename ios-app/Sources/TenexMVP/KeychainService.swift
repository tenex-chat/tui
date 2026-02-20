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

    /// Plaintext file names used by macOS storage backend.
    private static let nsecFileName = "nsec.txt"
    private static let elevenLabsFileName = "elevenlabs_api_key.txt"
    private static let openRouterFileName = "openrouter_api_key.txt"

    // MARK: - Singleton

    static let shared = KeychainService()

    private init() {}

    // MARK: - Internal Sync API (Background Thread Only)

    /// Saves the nsec credential to credential storage
    /// - Parameter nsec: The nsec string to save
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func saveNsec(_ nsec: String) -> KeychainResult<Void> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return saveFileCredential(nsec, fileName: Self.nsecFileName)
        #else
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
        #endif
    }

    /// Retrieves the stored nsec credential from credential storage
    /// - Returns: Result containing the nsec string or specific failure
    /// - Precondition: Must be called from a background thread
    func loadNsec() -> KeychainResult<String> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return loadFileCredential(fileName: Self.nsecFileName)
        #else
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
        #endif
    }

    /// Deletes the stored nsec credential from credential storage
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func deleteNsec() -> KeychainResult<Void> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return deleteFileCredential(fileName: Self.nsecFileName)
        #else
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
        #endif
    }

    /// Checks if nsec credential exists in credential storage without retrieving it
    /// - Returns: Result indicating whether credential exists
    /// - Precondition: Must be called from a background thread
    func hasStoredNsec() -> KeychainResult<Bool> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return hasFileCredential(fileName: Self.nsecFileName)
        #else
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
        #endif
    }

    // MARK: - ElevenLabs API Key Methods

    /// Saves the ElevenLabs API key to credential storage
    /// - Parameter key: The API key string to save
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func saveElevenLabsApiKey(_ key: String) -> KeychainResult<Void> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return saveFileCredential(key, fileName: Self.elevenLabsFileName)
        #else
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
        #endif
    }

    /// Retrieves the stored ElevenLabs API key from credential storage
    /// - Returns: Result containing the API key or specific failure
    /// - Precondition: Must be called from a background thread
    func loadElevenLabsApiKey() -> KeychainResult<String> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return loadFileCredential(fileName: Self.elevenLabsFileName)
        #else
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
        #endif
    }

    /// Deletes the stored ElevenLabs API key from credential storage
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func deleteElevenLabsApiKey() -> KeychainResult<Void> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return deleteFileCredential(fileName: Self.elevenLabsFileName)
        #else
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
        #endif
    }

    /// Checks if ElevenLabs API key exists in credential storage without retrieving it
    /// - Returns: Result indicating whether credential exists
    /// - Precondition: Must be called from a background thread
    func hasElevenLabsApiKey() -> KeychainResult<Bool> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return hasFileCredential(fileName: Self.elevenLabsFileName)
        #else
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
        #endif
    }

    // MARK: - OpenRouter API Key Methods

    /// Saves the OpenRouter API key to credential storage
    /// - Parameter key: The API key string to save
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func saveOpenRouterApiKey(_ key: String) -> KeychainResult<Void> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return saveFileCredential(key, fileName: Self.openRouterFileName)
        #else
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
        #endif
    }

    /// Retrieves the stored OpenRouter API key from credential storage
    /// - Returns: Result containing the API key or specific failure
    /// - Precondition: Must be called from a background thread
    func loadOpenRouterApiKey() -> KeychainResult<String> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return loadFileCredential(fileName: Self.openRouterFileName)
        #else
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
        #endif
    }

    /// Deletes the stored OpenRouter API key from credential storage
    /// - Returns: Result indicating success or specific failure
    /// - Precondition: Must be called from a background thread
    func deleteOpenRouterApiKey() -> KeychainResult<Void> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return deleteFileCredential(fileName: Self.openRouterFileName)
        #else
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
        #endif
    }

    /// Checks if OpenRouter API key exists in credential storage without retrieving it
    /// - Returns: Result indicating whether credential exists
    /// - Precondition: Must be called from a background thread
    func hasOpenRouterApiKey() -> KeychainResult<Bool> {
        precondition(!Foundation.Thread.isMainThread, "Credential storage operations must not be called on the main thread")

        #if os(macOS)
        return hasFileCredential(fileName: Self.openRouterFileName)
        #else
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

    /// Saves nsec credential asynchronously on a background thread
    /// - Parameter nsec: The nsec string to save
    /// - Returns: Result indicating success or failure
    func saveNsecAsync(_ nsec: String) async -> KeychainResult<Void> {
        await runAsync { self.saveNsec(nsec) }
    }

    /// Loads nsec credential asynchronously on a background thread
    /// - Returns: Result containing the nsec or failure
    func loadNsecAsync() async -> KeychainResult<String> {
        await runAsync { self.loadNsec() }
    }

    /// Deletes nsec credential asynchronously on a background thread
    /// - Returns: Result indicating success or failure
    func deleteNsecAsync() async -> KeychainResult<Void> {
        await runAsync { self.deleteNsec() }
    }

    /// Checks for stored credential asynchronously on a background thread
    /// - Returns: Result indicating whether credential exists
    func hasStoredNsecAsync() async -> KeychainResult<Bool> {
        await runAsync { self.hasStoredNsec() }
    }

    // MARK: - ElevenLabs Async Extensions

    /// Saves ElevenLabs API key asynchronously on a background thread
    /// - Parameter key: The API key to save
    /// - Returns: Result indicating success or failure
    func saveElevenLabsApiKeyAsync(_ key: String) async -> KeychainResult<Void> {
        await runAsync { self.saveElevenLabsApiKey(key) }
    }

    /// Loads ElevenLabs API key asynchronously on a background thread
    /// - Returns: Result containing the API key or failure
    func loadElevenLabsApiKeyAsync() async -> KeychainResult<String> {
        await runAsync { self.loadElevenLabsApiKey() }
    }

    /// Deletes ElevenLabs API key asynchronously on a background thread
    /// - Returns: Result indicating success or failure
    func deleteElevenLabsApiKeyAsync() async -> KeychainResult<Void> {
        await runAsync { self.deleteElevenLabsApiKey() }
    }

    /// Checks for stored ElevenLabs API key asynchronously on a background thread
    /// - Returns: Result indicating whether credential exists
    func hasElevenLabsApiKeyAsync() async -> KeychainResult<Bool> {
        await runAsync { self.hasElevenLabsApiKey() }
    }

    // MARK: - OpenRouter Async Extensions

    /// Saves OpenRouter API key asynchronously on a background thread
    /// - Parameter key: The API key to save
    /// - Returns: Result indicating success or failure
    func saveOpenRouterApiKeyAsync(_ key: String) async -> KeychainResult<Void> {
        await runAsync { self.saveOpenRouterApiKey(key) }
    }

    /// Loads OpenRouter API key asynchronously on a background thread
    /// - Returns: Result containing the API key or failure
    func loadOpenRouterApiKeyAsync() async -> KeychainResult<String> {
        await runAsync { self.loadOpenRouterApiKey() }
    }

    /// Deletes OpenRouter API key asynchronously on a background thread
    /// - Returns: Result indicating success or failure
    func deleteOpenRouterApiKeyAsync() async -> KeychainResult<Void> {
        await runAsync { self.deleteOpenRouterApiKey() }
    }

    /// Checks for stored OpenRouter API key asynchronously on a background thread
    /// - Returns: Result indicating whether credential exists
    func hasOpenRouterApiKeyAsync() async -> KeychainResult<Bool> {
        await runAsync { self.hasOpenRouterApiKey() }
    }
}
