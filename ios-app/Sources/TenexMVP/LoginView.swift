import SwiftUI

struct LoginView: View {
    @Binding var isLoggedIn: Bool
    @Binding var userNpub: String
    @Environment(TenexCoreManager.self) var coreManager

    /// Error message from auto-login attempt (if any)
    var autoLoginError: String?

    @State private var nsecInput = ""
    @State private var isLoading = false
    @State private var errorMessage: String?
    private let loginCardWidth: CGFloat = 320

    var body: some View {
        NavigationStack {
            ZStack {
                VStack(spacing: 14) {
                    // Header
                    VStack(spacing: 6) {
                        Image(systemName: "key.fill")
                            .font(.system(size: 42))
                            .foregroundStyle(Color.agentBrand)

                        Text("Login to TENEX")
                            .font(.title3)
                            .fontWeight(.semibold)

                        Text("Enter your Nostr secret key")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    // Auto-login error (from previous session)
                    if let autoError = autoLoginError {
                        HStack(spacing: 6) {
                            Image(systemName: "info.circle.fill")
                                .foregroundStyle(Color.healthWarning)
                            Text(autoError)
                                .foregroundStyle(Color.healthWarning)
                                .font(.footnote)
                        }
                        .padding(.horizontal, 10)
                        .padding(.vertical, 8)
                        .background(Color.healthWarning.opacity(0.1))
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                    }

                    VStack(alignment: .leading, spacing: 7) {
                        Text("NSEC Key")
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        SecureField("nsec1...", text: $nsecInput)
                            .textFieldStyle(.roundedBorder)
                            #if os(iOS)
                            .autocapitalization(.none)
                            #endif
                            .autocorrectionDisabled()
                            .font(.system(.body, design: .monospaced))
                    }

                    // Error Message
                    if let error = errorMessage {
                        HStack(spacing: 6) {
                            Image(systemName: "exclamationmark.triangle.fill")
                                .foregroundStyle(Color.healthError)
                            Text(error)
                                .foregroundStyle(Color.healthError)
                                .font(.footnote)
                        }
                        .padding(.horizontal, 10)
                        .padding(.vertical, 8)
                        .background(Color.healthError.opacity(0.1))
                        .clipShape(RoundedRectangle(cornerRadius: 8))
                    }

                    // Login Button
                    Button(action: login) {
                        if isLoading {
                            ProgressView()
                                .progressViewStyle(CircularProgressViewStyle(tint: .white))
                                .frame(maxWidth: .infinity)
                        } else {
                            Label("Login", systemImage: "arrow.right.circle.fill")
                                .frame(maxWidth: .infinity)
                        }
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(nsecInput.isEmpty || isLoading)

                    // Footer Info
                    VStack(spacing: 2) {
                        #if os(macOS)
                        Text("Your key is stored in a local file on this Mac")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                        #else
                        Text("Your key is stored securely in Keychain when available")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                        #endif

                        Text("If saved, you'll be auto-logged in on next launch")
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                    }
                    .padding(.top, 2)
                }
                .padding(18)
                .frame(maxWidth: loginCardWidth)
                #if os(macOS)
                .background(.regularMaterial, in: RoundedRectangle(cornerRadius: 14, style: .continuous))
                .overlay(
                    RoundedRectangle(cornerRadius: 14, style: .continuous)
                        .stroke(Color.white.opacity(0.08), lineWidth: 1)
                )
                #endif
            }
            .frame(maxWidth: .infinity, maxHeight: .infinity)
            .padding(24)
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
        }
    }

    private func login() {
        // Reset state
        errorMessage = nil
        isLoading = true

        // Validate input format - capture and clear input immediately
        let trimmedInput = nsecInput.trimmingCharacters(in: .whitespacesAndNewlines)

        // Clear sensitive input from UI state IMMEDIATELY after capturing
        // This minimizes the time sensitive data exists in memory
        nsecInput = ""

        guard !trimmedInput.isEmpty else {
            errorMessage = "Please enter your nsec key"
            isLoading = false
            return
        }

        guard trimmedInput.hasPrefix("nsec1") else {
            errorMessage = "Key must start with 'nsec1'"
            isLoading = false
            return
        }

        // Perform login using async/await with SafeTenexCore
        Task {
            do {
                let result = try await coreManager.safeCore.login(nsec: trimmedInput)

                if result.success {
                    // Save credential for future auto-login.
                    _ = await coreManager.saveCredential(nsec: trimmedInput)
                    isLoading = false
                    userNpub = result.npub
                    // Always continue directly to the app after successful auth.
                    isLoggedIn = true
                } else {
                    isLoading = false
                    errorMessage = "Login failed"
                }
            } catch let error as CoreError {
                isLoading = false
                switch error {
                case .tenex(let tenexError):
                    switch tenexError {
                    case .InvalidNsec(let message):
                        errorMessage = "Invalid key: \(message)"
                    case .NotLoggedIn:
                        errorMessage = "Not logged in"
                    case .Internal(let message):
                        errorMessage = "Error: \(message)"
                    case .LogoutFailed(let message):
                        errorMessage = "Logout failed: \(message)"
                    case .LockError(let resource):
                        errorMessage = "Lock error: \(resource)"
                    case .CoreNotInitialized:
                        errorMessage = "Core not initialized"
                    }
                case .notInitialized:
                    errorMessage = "Core not initialized"
                }
            } catch {
                isLoading = false
                errorMessage = "Unexpected error: \(error.localizedDescription)"
            }
        }
    }

}

#Preview {
    LoginView(isLoggedIn: .constant(false), userNpub: .constant(""), autoLoginError: nil)
        .environment(TenexCoreManager())
}
