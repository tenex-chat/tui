import AuthenticationServices
import CryptoKit
import Foundation
import Security

#if os(iOS)
import UIKit
#elseif os(macOS)
import AppKit
#endif

struct BYOKCredentialInfo: Codable, Equatable {
    let provider: String
    let keyID: String?
    let keyLabel: String?
    let appName: String?
    let issuedAt: Date
}

enum BYOKProvider: String, CaseIterable {
    case openRouter = "openrouter"
    case elevenLabs = "elevenlabs"

    var scope: String {
        "key:\(rawValue)"
    }
}

struct BYOKProviderGrant {
    let apiKey: String
    let credentialInfo: BYOKCredentialInfo
}

enum BYOKCredentialMetadataStore {
    private static let openRouterInfoKey = "settings.byok.openrouter.info.v1"
    private static let elevenLabsInfoKey = "settings.byok.elevenlabs.info.v1"

    static func loadInfo(for provider: BYOKProvider) -> BYOKCredentialInfo? {
        guard let data = UserDefaults.standard.data(forKey: key(for: provider)) else {
            return nil
        }
        return try? JSONDecoder().decode(BYOKCredentialInfo.self, from: data)
    }

    static func saveInfo(_ info: BYOKCredentialInfo, for provider: BYOKProvider) {
        guard let data = try? JSONEncoder().encode(info) else {
            return
        }
        UserDefaults.standard.set(data, forKey: key(for: provider))
    }

    static func deleteInfo(for provider: BYOKProvider) {
        UserDefaults.standard.removeObject(forKey: key(for: provider))
    }

    private static func key(for provider: BYOKProvider) -> String {
        switch provider {
        case .openRouter:
            return openRouterInfoKey
        case .elevenLabs:
            return elevenLabsInfoKey
        }
    }

    static func loadOpenRouterInfo() -> BYOKCredentialInfo? {
        loadInfo(for: .openRouter)
    }

    static func saveOpenRouterInfo(_ info: BYOKCredentialInfo) {
        saveInfo(info, for: .openRouter)
    }

    static func deleteOpenRouterInfo() {
        deleteInfo(for: .openRouter)
    }
}

enum BYOKConnectError: LocalizedError {
    case invalidAuthorizationURL
    case unableToStartSession
    case authenticationCancelled
    case missingCallbackURL
    case invalidCallback
    case authorizationDenied(String)
    case missingAuthorizationCode
    case stateMismatch
    case tokenRequestFailed(statusCode: Int, message: String)
    case invalidTokenResponse
    case invalidProvider(String)
    case missingAPIKey
    case randomGenerationFailed

    var errorDescription: String? {
        switch self {
        case .invalidAuthorizationURL:
            return "Could not build the BYOK authorization URL."
        case .unableToStartSession:
            return "Could not start the BYOK sign-in session."
        case .authenticationCancelled:
            return "BYOK sign-in was cancelled."
        case .missingCallbackURL:
            return "BYOK did not return an authorization callback."
        case .invalidCallback:
            return "BYOK returned an invalid callback."
        case .authorizationDenied(let message):
            return message.isEmpty ? "BYOK authorization was denied." : "BYOK authorization failed: \(message)"
        case .missingAuthorizationCode:
            return "BYOK did not return an authorization code."
        case .stateMismatch:
            return "BYOK returned an unexpected authorization state."
        case .tokenRequestFailed(let statusCode, let message):
            if message.isEmpty {
                return "BYOK token exchange failed (\(statusCode))."
            }
            return "BYOK token exchange failed (\(statusCode)): \(message)"
        case .invalidTokenResponse:
            return "BYOK returned an invalid token response."
        case .invalidProvider(let provider):
            return "BYOK returned an unexpected provider: \(provider)."
        case .missingAPIKey:
            return "BYOK did not return an API key."
        case .randomGenerationFailed:
            return "Could not generate secure BYOK authorization state."
        }
    }
}

@MainActor
final class BYOKConnectService {
    static let shared = BYOKConnectService()

    private static let baseURL = URL(string: "https://byok.f7z.io")!
    private static let clientID = "com.tenex.mvp"
    private static let appName = "TENEX"
    private static let redirectURI = "tenex://byok"
    private static let callbackScheme = "tenex"

    private let presentationContextProvider = BYOKAuthenticationPresentationContextProvider()
    private var activeSession: ASWebAuthenticationSession?

    private init() {}

    func requestOpenRouterKey() async throws -> BYOKProviderGrant {
        try await requestKey(for: .openRouter)
    }

    func requestKey(for provider: BYOKProvider) async throws -> BYOKProviderGrant {
        let state = try Self.randomURLSafeString(byteCount: 32)
        let codeVerifier = try Self.randomURLSafeString(byteCount: 32)
        let authorizationURL = try Self.authorizationURL(
            provider: provider,
            state: state,
            codeChallenge: Self.codeChallenge(for: codeVerifier)
        )

        let callbackURL = try await authenticate(url: authorizationURL)
        let callback = try Self.parseAuthorizationCallback(
            callbackURL,
            expectedProvider: provider,
            expectedState: state
        )
        let token = try await Self.exchangeAuthorizationCode(
            callback.code,
            codeVerifier: codeVerifier
        )

        let returnedProvider = token.provider.lowercased()
        guard returnedProvider == provider.rawValue else {
            throw BYOKConnectError.invalidProvider(token.provider)
        }

        let apiKey = token.apiKey.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !apiKey.isEmpty else {
            throw BYOKConnectError.missingAPIKey
        }

        let issuedAt = token.issuedAt.map { Date(timeIntervalSince1970: TimeInterval($0)) } ?? Date()
        let info = BYOKCredentialInfo(
            provider: returnedProvider,
            keyID: token.keyID ?? callback.keyID,
            keyLabel: token.keyLabel ?? callback.keyLabel,
            appName: token.appName,
            issuedAt: issuedAt
        )

        return BYOKProviderGrant(apiKey: apiKey, credentialInfo: info)
    }

    private func authenticate(url: URL) async throws -> URL {
        try await withCheckedThrowingContinuation { continuation in
            let session = ASWebAuthenticationSession(
                url: url,
                callbackURLScheme: Self.callbackScheme
            ) { [weak self] callbackURL, error in
                Task { @MainActor in
                    self?.activeSession = nil
                }

                if let error {
                    if let authError = error as? ASWebAuthenticationSessionError,
                       authError.code == .canceledLogin {
                        continuation.resume(throwing: BYOKConnectError.authenticationCancelled)
                    } else {
                        continuation.resume(throwing: error)
                    }
                    return
                }

                guard let callbackURL else {
                    continuation.resume(throwing: BYOKConnectError.missingCallbackURL)
                    return
                }

                continuation.resume(returning: callbackURL)
            }

            session.presentationContextProvider = presentationContextProvider
            session.prefersEphemeralWebBrowserSession = false
            activeSession = session

            guard session.start() else {
                activeSession = nil
                continuation.resume(throwing: BYOKConnectError.unableToStartSession)
                return
            }
        }
    }

    private static func authorizationURL(
        provider: BYOKProvider,
        state: String,
        codeChallenge: String
    ) throws -> URL {
        var components = URLComponents(url: baseURL.appendingPathComponent("authorize"), resolvingAgainstBaseURL: false)
        components?.queryItems = [
            URLQueryItem(name: "response_type", value: "code"),
            URLQueryItem(name: "client_id", value: clientID),
            URLQueryItem(name: "app_name", value: appName),
            URLQueryItem(name: "redirect_uri", value: redirectURI),
            URLQueryItem(name: "scope", value: provider.scope),
            URLQueryItem(name: "state", value: state),
            URLQueryItem(name: "code_challenge", value: codeChallenge),
            URLQueryItem(name: "code_challenge_method", value: "S256")
        ]

        guard let url = components?.url else {
            throw BYOKConnectError.invalidAuthorizationURL
        }
        return url
    }

    private static func parseAuthorizationCallback(
        _ url: URL,
        expectedProvider: BYOKProvider,
        expectedState: String
    ) throws -> AuthorizationCallback {
        guard let components = URLComponents(url: url, resolvingAgainstBaseURL: false),
              components.scheme?.lowercased() == callbackScheme,
              components.host?.lowercased() == "byok" else {
            throw BYOKConnectError.invalidCallback
        }

        let queryItems = components.queryItems ?? []
        func value(_ name: String) -> String? {
            queryItems.first { $0.name == name }?.value
        }

        if let error = value("error") {
            let description = value("error_description") ?? error
            throw BYOKConnectError.authorizationDenied(description)
        }

        guard value("state") == expectedState else {
            throw BYOKConnectError.stateMismatch
        }

        guard let code = value("code"), !code.isEmpty else {
            throw BYOKConnectError.missingAuthorizationCode
        }

        if let provider = value("provider")?.lowercased(), provider != expectedProvider.rawValue {
            throw BYOKConnectError.invalidProvider(provider)
        }

        return AuthorizationCallback(
            code: code,
            keyID: value("key_id"),
            keyLabel: value("key_label")
        )
    }

    private static func exchangeAuthorizationCode(
        _ code: String,
        codeVerifier: String
    ) async throws -> TokenResponse {
        let url = baseURL.appendingPathComponent("api/token")
        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 45
        request.addValue("application/json", forHTTPHeaderField: "Content-Type")
        request.httpBody = try JSONEncoder().encode(TokenRequest(
            grantType: "authorization_code",
            code: code,
            codeVerifier: codeVerifier,
            clientID: clientID,
            redirectURI: redirectURI
        ))

        let (data, response) = try await URLSession.shared.data(for: request)
        guard let httpResponse = response as? HTTPURLResponse else {
            throw BYOKConnectError.invalidTokenResponse
        }

        guard (200...299).contains(httpResponse.statusCode) else {
            let message = (try? JSONDecoder().decode(TokenErrorResponse.self, from: data).error) ?? ""
            throw BYOKConnectError.tokenRequestFailed(statusCode: httpResponse.statusCode, message: message)
        }

        do {
            return try JSONDecoder().decode(TokenResponse.self, from: data)
        } catch {
            throw BYOKConnectError.invalidTokenResponse
        }
    }

    private static func codeChallenge(for verifier: String) -> String {
        let digest = SHA256.hash(data: Data(verifier.utf8))
        return base64URLEncode(Data(digest))
    }

    private static func randomURLSafeString(byteCount: Int) throws -> String {
        var bytes = [UInt8](repeating: 0, count: byteCount)
        let status = SecRandomCopyBytes(kSecRandomDefault, byteCount, &bytes)
        guard status == errSecSuccess else {
            throw BYOKConnectError.randomGenerationFailed
        }
        return base64URLEncode(Data(bytes))
    }

    private static func base64URLEncode(_ data: Data) -> String {
        data.base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    private struct AuthorizationCallback {
        let code: String
        let keyID: String?
        let keyLabel: String?
    }

    private struct TokenRequest: Encodable {
        let grantType: String
        let code: String
        let codeVerifier: String
        let clientID: String
        let redirectURI: String

        private enum CodingKeys: String, CodingKey {
            case grantType = "grant_type"
            case code
            case codeVerifier = "code_verifier"
            case clientID = "client_id"
            case redirectURI = "redirect_uri"
        }
    }

    private struct TokenResponse: Decodable {
        let tokenType: String
        let provider: String
        let apiKey: String
        let keyID: String?
        let keyLabel: String?
        let appName: String?
        let issuedAt: Int?

        private enum CodingKeys: String, CodingKey {
            case tokenType = "token_type"
            case provider
            case apiKey = "api_key"
            case keyID = "key_id"
            case keyLabel = "key_label"
            case appName = "app_name"
            case issuedAt = "issued_at"
        }
    }

    private struct TokenErrorResponse: Decodable {
        let error: String?
    }
}

private final class BYOKAuthenticationPresentationContextProvider: NSObject, ASWebAuthenticationPresentationContextProviding {
    func presentationAnchor(for session: ASWebAuthenticationSession) -> ASPresentationAnchor {
        #if os(iOS)
        let scenes = UIApplication.shared.connectedScenes.compactMap { $0 as? UIWindowScene }
        let windows = scenes.flatMap(\.windows)
        if let window = windows.first(where: { $0.isKeyWindow }) ?? windows.first {
            return window
        }
        if let scene = scenes.first {
            return UIWindow(windowScene: scene)
        }
        preconditionFailure("BYOK authentication requires an active window scene")
        #elseif os(macOS)
        return NSApplication.shared.keyWindow ??
            NSApplication.shared.mainWindow ??
            NSApplication.shared.windows.first ??
            NSWindow(contentRect: .zero, styleMask: [], backing: .buffered, defer: false)
        #endif
    }
}
