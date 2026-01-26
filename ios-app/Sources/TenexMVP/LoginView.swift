import SwiftUI

struct LoginView: View {
    @Binding var isLoggedIn: Bool
    @Binding var userNpub: String
    @EnvironmentObject var coreManager: TenexCoreManager

    @State private var nsecInput = ""
    @State private var isLoading = false
    @State private var errorMessage: String?
    @State private var showSuccess = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 24) {
                // Header
                VStack(spacing: 8) {
                    Image(systemName: "key.fill")
                        .font(.system(size: 60))
                        .foregroundStyle(.blue)

                    Text("Login to TENEX")
                        .font(.largeTitle)
                        .fontWeight(.bold)

                    Text("Enter your Nostr secret key")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }
                .padding(.top, 40)

                // Input Section
                VStack(alignment: .leading, spacing: 8) {
                    Text("NSEC Key")
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    SecureField("nsec1...", text: $nsecInput)
                        .textFieldStyle(.roundedBorder)
                        .autocapitalization(.none)
                        .autocorrectionDisabled()
                        .font(.system(.body, design: .monospaced))
                }
                .padding(.horizontal)

                // Error Message
                if let error = errorMessage {
                    HStack {
                        Image(systemName: "exclamationmark.triangle.fill")
                            .foregroundStyle(.red)
                        Text(error)
                            .foregroundStyle(.red)
                            .font(.footnote)
                    }
                    .padding(.horizontal)
                    .padding(.vertical, 8)
                    .background(Color.red.opacity(0.1))
                    .clipShape(RoundedRectangle(cornerRadius: 8))
                    .padding(.horizontal)
                }

                // Success Message
                if showSuccess {
                    VStack(spacing: 8) {
                        Image(systemName: "checkmark.circle.fill")
                            .font(.system(size: 40))
                            .foregroundStyle(.green)

                        Text("Login Successful!")
                            .font(.headline)
                            .foregroundStyle(.green)

                        Text("Your npub:")
                            .font(.caption)
                            .foregroundStyle(.secondary)

                        Text(userNpub)
                            .font(.system(.caption, design: .monospaced))
                            .lineLimit(1)
                            .truncationMode(.middle)
                            .padding(.horizontal)
                    }
                    .padding()
                    .background(Color.green.opacity(0.1))
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
                    Text("Your key is stored in memory only")
                        .font(.caption2)
                        .foregroundStyle(.secondary)

                    Text("It will be cleared when you close the app")
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
        isLoading = true

        // Validate input format
        let trimmedInput = nsecInput.trimmingCharacters(in: .whitespacesAndNewlines)

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

        // Perform login on background thread
        DispatchQueue.global(qos: .userInitiated).async {
            do {
                let result = try coreManager.core.login(nsec: trimmedInput)

                DispatchQueue.main.async {
                    self.isLoading = false
                    if result.success {
                        self.userNpub = result.npub
                        self.showSuccess = true
                    } else {
                        self.errorMessage = "Login failed"
                    }
                }
            } catch let error as TenexError {
                DispatchQueue.main.async {
                    self.isLoading = false
                    switch error {
                    case .InvalidNsec(let message):
                        self.errorMessage = "Invalid key: \(message)"
                    case .NotLoggedIn:
                        self.errorMessage = "Not logged in"
                    case .Internal(let message):
                        self.errorMessage = "Error: \(message)"
                    case .LogoutFailed(let message):
                        self.errorMessage = "Logout failed: \(message)"
                    }
                }
            } catch {
                DispatchQueue.main.async {
                    self.isLoading = false
                    self.errorMessage = "Unexpected error: \(error.localizedDescription)"
                }
            }
        }
    }

    private func continueToApp() {
        isLoggedIn = true
    }
}

#Preview {
    LoginView(isLoggedIn: .constant(false), userNpub: .constant(""))
        .environmentObject(TenexCoreManager())
}
