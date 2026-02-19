import Foundation

@MainActor
final class AppSessionStore: ObservableObject {
    private let credentials: CredentialStoring
    private var autoLoginTask: Task<Void, Never>?

    @Published var isLoggedIn = false
    @Published var userNpub = ""
    @Published var isAttemptingAutoLogin = false
    @Published var autoLoginError: String?

    init(credentials: CredentialStoring = KeychainService.shared) {
        self.credentials = credentials
    }

    func reset() {
        autoLoginTask?.cancel()
        autoLoginTask = nil
        isLoggedIn = false
        userNpub = ""
        isAttemptingAutoLogin = false
        autoLoginError = nil
    }

    func applyAutoLoginResult(_ result: AutoLoginResult) {
        switch result {
        case .noCredentials:
            break
        case .success(let npub):
            userNpub = npub
            isLoggedIn = true
        case .invalidCredential:
            Task {
                _ = await self.credentials.deleteNsecAsync()
            }
            autoLoginError = "Stored credential was invalid. Please log in again."
        case .transientError(let error):
            autoLoginError = "Could not auto-login: \(error)"
        }
    }

    func attemptAutoLogin(coreManager: TenexCoreManager) {
        autoLoginTask?.cancel()
        isAttemptingAutoLogin = true
        autoLoginError = nil
        let startedAt = CFAbsoluteTimeGetCurrent()
        PerformanceProfiler.shared.logEvent("attemptAutoLogin start", category: .general)

        let debugNsec = getDebugNsec()

        autoLoginTask = Task { [weak self] in
            guard let self else { return }

            if let nsec = debugNsec {
                await self.attemptDebugAutoLogin(nsec: nsec, coreManager: coreManager)
                return
            }

            let result = await self.runOnBackgroundQueue {
                coreManager.attemptAutoLogin()
            }

            guard !Task.isCancelled else { return }
            self.isAttemptingAutoLogin = false
            let elapsedMs = (CFAbsoluteTimeGetCurrent() - startedAt) * 1000
            PerformanceProfiler.shared.logEvent(
                "attemptAutoLogin finished result=\(String(describing: result)) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                category: .general,
                level: elapsedMs >= 200 ? .error : .info
            )

            self.applyAutoLoginResult(result)
            self.autoLoginTask = nil
        }
    }

    private func attemptDebugAutoLogin(nsec: String, coreManager: TenexCoreManager) async {
        let debugLoginStartedAt = CFAbsoluteTimeGetCurrent()
        let loginResult = await runOnBackgroundQueue {
            Result { try coreManager.core.login(nsec: nsec) }
        }

        guard !Task.isCancelled else { return }
        isAttemptingAutoLogin = false
        let elapsedMs = (CFAbsoluteTimeGetCurrent() - debugLoginStartedAt) * 1000

        switch loginResult {
        case .success(let result):
            PerformanceProfiler.shared.logEvent(
                "attemptAutoLogin debug-nsec login success=\(result.success) elapsedMs=\(String(format: "%.2f", elapsedMs))",
                category: .general,
                level: elapsedMs >= 120 ? .error : .info
            )

            if result.success {
                userNpub = result.npub
                isLoggedIn = true
            } else {
                autoLoginError = "Debug nsec login failed"
            }
        case .failure(let error):
            PerformanceProfiler.shared.logEvent(
                "attemptAutoLogin debug-nsec failed elapsedMs=\(String(format: "%.2f", elapsedMs)) error=\(error.localizedDescription)",
                category: .general,
                level: .error
            )
            autoLoginError = "Debug nsec invalid: \(error.localizedDescription)"
        }

        autoLoginTask = nil
    }

    private func runOnBackgroundQueue<T>(_ work: @escaping () -> T) async -> T {
        await withCheckedContinuation { continuation in
            DispatchQueue.global(qos: .userInitiated).async {
                continuation.resume(returning: work())
            }
        }
    }

    private func getDebugNsec() -> String? {
        #if DEBUG
        let args = ProcessInfo.processInfo.arguments
        if let index = args.firstIndex(of: "--debug-nsec"), index + 1 < args.count {
            let nsec = args[index + 1]
            if nsec.hasPrefix("nsec1") {
                return nsec
            }
        }

        if let nsec = ProcessInfo.processInfo.environment["TENEX_DEBUG_NSEC"],
           nsec.hasPrefix("nsec1") {
            return nsec
        }
        #endif

        return nil
    }
}
