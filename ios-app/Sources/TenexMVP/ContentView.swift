import SwiftUI
import CryptoKit

struct ContentView: View {
    @Binding var userNpub: String
    @Binding var isLoggedIn: Bool
    @EnvironmentObject var coreManager: TenexCoreManager

    @State private var projects: [ProjectInfo] = []
    @State private var isLoading = false
    @State private var selectedProject: ProjectInfo?
    @State private var showLogoutError = false
    @State private var logoutErrorMessage = ""
    @State private var isLoggingOut = false
    @State private var showNewConversation = false

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // User header
                UserHeaderView(npub: userNpub, onLogout: logout, isLoggingOut: isLoggingOut)

                Divider()

                // Project list
                if isLoading {
                    Spacer()
                    ProgressView("Loading projects...")
                    Spacer()
                } else if projects.isEmpty {
                    Spacer()
                    EmptyStateView(onRefresh: loadProjects)
                    Spacer()
                } else {
                    ProjectListView(
                        projects: projects,
                        selectedProject: $selectedProject
                    )
                }
            }
            .navigationTitle("Projects")
            .navigationBarTitleDisplayMode(.large)
            .navigationDestination(for: ProjectInfo.self) { project in
                ConversationsView(project: project)
                    .environmentObject(coreManager)
            }
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    HStack(spacing: 16) {
                        Button(action: loadProjects) {
                            Image(systemName: "arrow.clockwise")
                        }
                        .disabled(isLoading)

                        Button(action: { showNewConversation = true }) {
                            Image(systemName: "plus.message")
                        }
                    }
                }
            }
            .onAppear {
                loadProjects()
            }
            .alert("Logout Error", isPresented: $showLogoutError) {
                Button("Retry") {
                    logout()
                }
                Button("Cancel", role: .cancel) { }
            } message: {
                Text(logoutErrorMessage)
            }
            .sheet(isPresented: $showNewConversation) {
                MessageComposerView(
                    project: nil,
                    onSend: { _ in
                        // Optionally refresh projects or navigate somewhere
                        loadProjects()
                    }
                )
                .environmentObject(coreManager)
            }
        }
    }

    private func loadProjects() {
        isLoading = true

        DispatchQueue.global(qos: .userInitiated).async {
            // Use the shared core manager - it's already initialized
            _ = coreManager.core.refresh()
            let fetchedProjects = coreManager.core.getProjects()

            DispatchQueue.main.async {
                self.projects = fetchedProjects
                self.isLoading = false
            }
        }
    }

    private func logout() {
        isLoggingOut = true

        DispatchQueue.global(qos: .userInitiated).async {
            // First perform core logout - only clear credentials if logout succeeds
            do {
                try coreManager.core.logout()

                // Logout succeeded - now clear credentials from keychain
                let clearError = coreManager.clearCredentials()
                if let error = clearError {
                    // Log warning but don't fail - logout already succeeded
                    print("[TENEX] Warning: Failed to clear credentials after logout: \(error)")
                }

                DispatchQueue.main.async {
                    self.isLoggingOut = false
                    self.isLoggedIn = false
                }
            } catch TenexError.LogoutFailed(let message) {
                DispatchQueue.main.async {
                    self.isLoggingOut = false
                    // Keep isLoggedIn = true to stay synced with core state (still connected)
                    // DO NOT clear credentials - user is still logged in
                    print("[TENEX] Logout failed: \(message)")
                    self.logoutErrorMessage = "Logout failed: \(message). Please try again."
                    self.showLogoutError = true
                }
            } catch {
                DispatchQueue.main.async {
                    self.isLoggingOut = false
                    // For other unexpected errors, also keep state synced
                    // DO NOT clear credentials - user may still be logged in
                    print("[TENEX] Unexpected logout error: \(error)")
                    self.logoutErrorMessage = "Logout error: \(error.localizedDescription)"
                    self.showLogoutError = true
                }
            }
        }
    }
}

// MARK: - User Header View

struct UserHeaderView: View {
    let npub: String
    let onLogout: () -> Void
    var isLoggingOut: Bool = false

    var body: some View {
        HStack(spacing: 12) {
            // User avatar placeholder
            Circle()
                .fill(Color.blue.gradient)
                .frame(width: 44, height: 44)
                .overlay {
                    Image(systemName: "person.fill")
                        .foregroundStyle(.white)
                        .font(.title3)
                }

            VStack(alignment: .leading, spacing: 2) {
                Text("Logged in as")
                    .font(.caption)
                    .foregroundStyle(.secondary)

                Text(truncatedNpub)
                    .font(.system(.footnote, design: .monospaced))
                    .foregroundStyle(.primary)
            }

            Spacer()

            if isLoggingOut {
                ProgressView()
                    .scaleEffect(0.8)
            } else {
                Button(action: onLogout) {
                    Text("Logout")
                        .font(.subheadline)
                        .foregroundStyle(.red)
                }
            }
        }
        .padding()
        .background(Color(.systemBackground))
    }

    private var truncatedNpub: String {
        guard npub.count > 20 else { return npub }
        let prefix = npub.prefix(pubkeyDisplayPrefixLength)
        let suffix = npub.suffix(8)
        return "\(prefix)...\(suffix)"
    }
}

// MARK: - Empty State View

struct EmptyStateView: View {
    let onRefresh: () -> Void

    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "folder.badge.questionmark")
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text("No Projects Found")
                .font(.title2)
                .fontWeight(.semibold)

            Text("Tap refresh to load your projects")
                .font(.subheadline)
                .foregroundStyle(.secondary)

            Button(action: onRefresh) {
                Label("Refresh", systemImage: "arrow.clockwise")
            }
            .buttonStyle(.bordered)
            .padding(.top, 8)
        }
        .padding()
    }
}

// MARK: - Project List View

struct ProjectListView: View {
    let projects: [ProjectInfo]
    @Binding var selectedProject: ProjectInfo?

    var body: some View {
        List {
            ForEach(projects, id: \.id) { project in
                NavigationLink(value: project) {
                    ProjectRowView(project: project)
                }
            }
        }
        .listStyle(.plain)
    }
}

// MARK: - Project Row View

struct ProjectRowView: View {
    let project: ProjectInfo

    var body: some View {
        HStack(spacing: 12) {
            // Project icon
            RoundedRectangle(cornerRadius: 10)
                .fill(projectColor.gradient)
                .frame(width: 44, height: 44)
                .overlay {
                    Image(systemName: "folder.fill")
                        .foregroundStyle(.white)
                        .font(.title3)
                }

            VStack(alignment: .leading, spacing: 4) {
                Text(project.title)
                    .font(.headline)
                    .lineLimit(1)

                if let description = project.description {
                    Text(description)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }

                Text(project.id)
                    .font(.caption)
                    .foregroundStyle(.tertiary)
                    .lineLimit(1)
            }

            Spacer()

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 8)
    }

    /// Deterministic color using shared utility (stable across app launches)
    private var projectColor: Color {
        deterministicColor(for: project.id)
    }
}

// MARK: - Project Detail Sheet

struct ProjectDetailSheet: View {
    let project: ProjectInfo
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 24) {
                    // Header
                    VStack(alignment: .leading, spacing: 12) {
                        RoundedRectangle(cornerRadius: 16)
                            .fill(Color.blue.gradient)
                            .frame(width: 80, height: 80)
                            .overlay {
                                Image(systemName: "folder.fill")
                                    .foregroundStyle(.white)
                                    .font(.system(size: 36))
                            }

                        Text(project.title)
                            .font(.largeTitle)
                            .fontWeight(.bold)

                        Text(project.id)
                            .font(.system(.subheadline, design: .monospaced))
                            .foregroundStyle(.secondary)
                    }

                    Divider()

                    // Description
                    if let description = project.description {
                        VStack(alignment: .leading, spacing: 8) {
                            Text("Description")
                                .font(.headline)

                            Text(description)
                                .font(.body)
                                .foregroundStyle(.secondary)
                        }
                    }

                    Divider()

                    // Coming Soon
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Conversations")
                            .font(.headline)

                        HStack {
                            Image(systemName: "bubble.left.and.bubble.right")
                                .font(.title2)
                                .foregroundStyle(.secondary)

                            Text("Conversations coming soon...")
                                .foregroundStyle(.secondary)
                        }
                        .padding()
                        .frame(maxWidth: .infinity, alignment: .leading)
                        .background(Color(.systemGray6))
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                    }

                    Spacer()
                }
                .padding()
            }
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
        }
    }
}

// MARK: - ProjectInfo Identifiable conformance

extension ProjectInfo: Identifiable {}

#Preview {
    ContentView(userNpub: .constant("npub1abc123def456..."), isLoggedIn: .constant(true))
        .environmentObject(TenexCoreManager())
}
