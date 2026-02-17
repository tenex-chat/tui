import SwiftUI

struct LoginView: View {
    @Binding var isLoggedIn: Bool
    @Binding var userNpub: String
    @EnvironmentObject var coreManager: TenexCoreManager

    /// Error message from auto-login attempt (if any)
    var autoLoginError: String?

    @State private var nsecInput = ""
    @State private var isLoading = false
    @State private var errorMessage: String?
    @State private var showSuccess = false
    @State private var credentialSaveWarning: String?

    var body: some View {
        NavigationStack {
            VStack(spacing: 24) {
                // Header
                VStack(spacing: 8) {
                    Image(systemName: "key.fill")
                        .font(.system(size: 60))
                        .foregroundStyle(Color.agentBrand)

                    Text("Login to TENEX")
                        .font(.largeTitle)
                        .fontWeight(.bold)

                    Text("Enter your Nostr secret key")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .padding(.top, 40)

                // Auto-login error (from previous session)
                if let autoError = autoLoginError {
                    HStack {
                        Image(systemName: "info.circle.fill")
                            .foregroundStyle(Color.healthWarning)
                        Text(autoError)
                            .foregroundStyle(Color.healthWarning)
                            .font(.footnote)
                    }
                    .padding(.horizontal)
                    .padding(.vertical, 8)
                    .background(Color.healthWarning.opacity(0.1))
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .padding(.horizontal)
                }

                // Input Section
                VStack(alignment: .leading, spacing: 8) {
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
                .padding(.horizontal)

                // Error Message
                if let error = errorMessage {
                    HStack {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(Color.healthError)
                        Text(error)
                            .foregroundStyle(Color.healthError)
                            .font(.footnote)
                    }
                    .padding(.horizontal)
                    .padding(.vertical, 8)
                    .background(Color.healthError.opacity(0.1))
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .padding(.horizontal)
                }

                // Success Message
                if showSuccess {
                    VStack(spacing: 8) {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 40))
                            .foregroundStyle(Color.healthGood)

                        Text("Login Successful!")
                            .font(.headline)
                            .foregroundStyle(Color.healthGood)

                        Text("Your npub:")
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        Text(userNpub)
                            .font(.system(.caption, design: .monospaced))
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .padding(.horizontal)

                        // Credential save warning (if save failed)
                        if let warning = credentialSaveWarning {
                            HStack {
                                Image(systemName: "exclamationmark.triangle.fill")
                                    .foregroundStyle(Color.healthWarning)
                                Text(warning)
                                    .foregroundStyle(Color.healthWarning)
                                    .font(.caption2)
                            }
                            .padding(.horizontal, 8)
                            .padding(.vertical, 4)
                            .background(Color.healthWarning.opacity(0.1))
                            .clipShape(RoundedRectangle(cornerRadius: 6))
                        }
                    }
                    .padding()
                    .background(Color.healthGood.opacity(0.1))
                    .clipShape(RoundedRectangle(cornerRadius: 12))
                    .padding(.horizontal)
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
                .disabled(nsecInput.isEmpty || isLoading || showSuccess)
                .padding(.horizontal)

                // Continue Button (shown after success)
                if showSuccess {
                    Button(action: continueToApp) {
                        Label("Continue to App", systemImage: "arrow.forward")
                            .frame(maxWidth: .infinity)
                    }
                    .buttonStyle(.bordered)
                    .padding(.horizontal)
                }

                Spacer()

                // Footer Info
                VStack(spacing: 4) {
                    Text("Your key is stored securely in Keychain")
                        .font(.caption2)
                        .foregroundStyle(.secondary)

                    Text("You'll be auto-logged in on next launch")
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                }
                .padding(.bottom, 20)
            }
            .navigationBarTitleDisplayMode(.inline)
        }
    }

    private func login() {
        // Reset state
        errorMessage = nil
        showSuccess = false
        credentialSaveWarning = nil
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
                    // Save credential to keychain
                    let saveError = await coreManager.saveCredential(nsec: trimmedInput)

                    isLoading = false
                    userNpub = result.npub

                    if let error = saveError {
                        // Show warning if credential save failed
                        credentialSaveWarning = "Could not save credentials: \(error). You'll need to log in again next time."
                        showSuccess = true
                    } else {
                        // Auto-navigate to app on successful login
                        isLoggedIn = true
                    }
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

    private func continueToApp() {
        isLoggedIn = true
    }
}

#Preview {
    LoginView(isLoggedIn: .constant(false), userNpub: .constant(""), autoLoginError: nil)
        .environmentObject(TenexCoreManager())
}
