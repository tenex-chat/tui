import SwiftUI
import CryptoKit

struct ContentView: View {
    @Binding var userNpub: String
    @Binding var isLoggedIn: Bool
    @EnvironmentObject var coreManager: TenexCoreManager

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

                // Project list - uses centralized coreManager.projects
                if coreManager.projects.isEmpty {
                    Spacer()
                    EmptyStateView()
                    Spacer()
                } else {
                    ProjectListView(
                        projects: coreManager.projects,
                        selectedProject: $selectedProject
                    )
                    .environmentObject(coreManager)
                    .refreshable {
                        await coreManager.manualRefresh()
                    }
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
                    Button(action: { showNewConversation = true }) {
                        Image(systemName: "plus.message")
                    }
                }
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
                        // Data will auto-refresh via polling
                    }
                )
                .environmentObject(coreManager)
            }
        }
    }

    private func logout() {
        isLoggingOut = true

        Task.detached(priority: .userInitiated) {
            // First perform core logout - only clear credentials if logout succeeds
            do {
                try await coreManager.safeCore.logout()

                // Logout succeeded - now clear credentials from keychain
                let clearError = await coreManager.clearCredentials()
                if let error = clearError {
                    // Log warning but don't fail - logout already succeeded
                    print("[TENEX] Warning: Failed to clear credentials after logout: \(error)")
                }

                await MainActor.run {
                    self.isLoggingOut = false
                    self.isLoggedIn = false
                }
            } catch TenexError.LogoutFailed(let message) {
                await MainActor.run {
                    self.isLoggingOut = false
                    // Keep isLoggedIn = true to stay synced with core state (still connected)
                    // DO NOT clear credentials - user is still logged in
                    print("[TENEX] Logout failed: \(message)")
                    self.logoutErrorMessage = "Logout failed: \(message). Please try again."
                    self.showLogoutError = true
                }
            } catch {
                await MainActor.run {
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
    var body: some View {
        VStack(spacing: 16) {
            Image(systemName: "folder.badge.questionmark")
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text("No Projects Found")
                .font(.title2)
                .fontWeight(.semibold)

            Text("Projects will appear automatically")
                .font(.subheadline)
                .foregroundStyle(.secondary)
        }
        .padding()
    }
}

// MARK: - Project List View

struct ProjectListView: View {
    let projects: [ProjectInfo]
    @Binding var selectedProject: ProjectInfo?
    @EnvironmentObject var coreManager: TenexCoreManager

    var body: some View {
        List {
            ForEach(projects, id: \.id) { project in
                NavigationLink(value: project) {
                    ProjectRowView(project: project)
                        .environmentObject(coreManager)
                }
            }
        }
        .listStyle(.plain)
    }
}

// MARK: - Project Row View

struct ProjectRowView: View {
    let project: ProjectInfo
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var isBooting = false
    @State private var bootError: String?
    @State private var showBootError = false

    /// Reactive online status from TenexCoreManager.
    /// This is automatically updated when kind:24010 events arrive from any source.
    private var isOnline: Bool {
        coreManager.projectOnlineStatus[project.id] ?? false
    }

    var body: some View {
        HStack(spacing: 12) {
            // Project icon with online indicator
            ZStack(alignment: .bottomTrailing) {
                RoundedRectangle(cornerRadius: 10)
                    .fill(projectColor.gradient)
                    .frame(width: 44, height: 44)
                    .overlay {
                        Image(systemName: "folder.fill")
                            .foregroundStyle(.white)
                            .font(.title3)
                    }

                // Online/Offline indicator dot
                Circle()
                    .fill(isOnline ? Color.green : Color.gray)
                    .frame(width: 12, height: 12)
                    .overlay {
                        Circle()
                            .stroke(Color(.systemBackground), lineWidth: 2)
                    }
                    .offset(x: 2, y: 2)
            }

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(project.title)
                        .font(.headline)
                        .lineLimit(1)

                    // Status badge
                    Text(isOnline ? "Online" : "Offline")
                        .font(.caption2)
                        .fontWeight(.medium)
                        .foregroundStyle(isOnline ? Color.green : Color.gray)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(
                            Capsule()
                                .fill(isOnline ? Color.green.opacity(0.15) : Color.gray.opacity(0.15))
                        )
                }

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

            // Boot button for offline projects
            if !isOnline {
                Button {
                    bootProject()
                } label: {
                    if isBooting {
                        ProgressView()
                            .scaleEffect(0.7)
                    } else {
                        Image(systemName: "power")
                            .font(.body)
                            .foregroundStyle(.blue)
                    }
                }
                .buttonStyle(.borderless)
                .disabled(isBooting)
            }

            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        }
        .padding(.vertical, 8)
        .alert("Boot Failed", isPresented: $showBootError) {
            Button("OK") {
                bootError = nil
            }
        } message: {
            if let error = bootError {
                Text(error)
            }
        }
    }

    /// Boot the project.
    /// The UI will update automatically when the backend publishes a kind:24010
    /// status event - no polling or manual refresh needed.
    private func bootProject() {
        isBooting = true
        bootError = nil

        Task {
            do {
                try await coreManager.safeCore.bootProject(projectId: project.id)
                // No delay or manual refresh needed - the Rust core will receive
                // the kind:24010 status event and push it via EventCallback,
                // which updates coreManager.projectOnlineStatus reactively.
            } catch {
                await MainActor.run {
                    bootError = error.localizedDescription
                    showBootError = true
                }
            }
            await MainActor.run {
                isBooting = false
            }
        }
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
