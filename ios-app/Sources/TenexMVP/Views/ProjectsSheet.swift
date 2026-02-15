import SwiftUI

/// Unified Projects Sheet that combines filtering and project management.
/// Each row has a dual-zone design:
/// - Left toggle: filter on/off for conversations
/// - Center tap: main action (boot if offline, open project details if online)
/// - Right: boot icon (offline) or chevron (online)
struct ProjectsSheet: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @Binding var selectedProjectIds: Set<String>
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        NavigationStack {
            List {
                // "All Projects" toggle
                allProjectsRow

                if !coreManager.projects.isEmpty {
                    Section {
                        ForEach(sortedProjects, id: \.id) { project in
                            ProjectsSheetRow(
                                project: project,
                                isFiltered: selectedProjectIds.contains(project.id),
                                onToggleFilter: { toggleProject(project.id) }
                            )
                            .environmentObject(coreManager)
                        }
                    } header: {
                        Text("Projects")
                    }
                }
            }
            .listStyle(.insetGrouped)
            .navigationTitle("Projects")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                    .fontWeight(.semibold)
                }
            }
        }
        .presentationDetents([.medium, .large])
        .presentationDragIndicator(.visible)
    }

    // MARK: - All Projects Row

    private var allProjectsRow: some View {
        Button(action: { selectedProjectIds.removeAll() }) {
            HStack(spacing: 12) {
                // Icon
                RoundedRectangle(cornerRadius: 8)
                    .fill(Color.blue.gradient)
                    .frame(width: 44, height: 44)
                    .overlay {
                        Image(systemName: "square.grid.2x2.fill")
                            .foregroundStyle(.white)
                            .font(.title3)
                    }

                VStack(alignment: .leading, spacing: 2) {
                    Text("All Projects")
                        .font(.headline)
                        .foregroundStyle(.primary)

                    Text("\(coreManager.projects.count) projects")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer()

                // Checkmark when showing all (no filter active)
                if selectedProjectIds.isEmpty {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.title2)
                        .foregroundStyle(.blue)
                } else {
                    Image(systemName: "circle")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                }
            }
            .padding(.vertical, 4)
        }
        .buttonStyle(.plain)
    }

    // MARK: - Helpers

    /// Projects sorted with online first, then alphabetical
    private var sortedProjects: [ProjectInfo] {
        coreManager.projects.sorted { a, b in
            let aOnline = coreManager.projectOnlineStatus[a.id] ?? false
            let bOnline = coreManager.projectOnlineStatus[b.id] ?? false
            if aOnline != bOnline { return aOnline }
            return a.title.localizedCaseInsensitiveCompare(b.title) == .orderedAscending
        }
    }

    private func toggleProject(_ id: String) {
        if selectedProjectIds.contains(id) {
            selectedProjectIds.remove(id)
        } else {
            selectedProjectIds.insert(id)
        }
    }
}

// MARK: - Project Row with Dual-Zone Design

/// A row with dual-zone interaction:
/// - Toggle on left: filter conversations by this project
/// - Tap center/right: boot (if offline) or view project (if online)
private struct ProjectsSheetRow: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let project: ProjectInfo
    let isFiltered: Bool
    let onToggleFilter: () -> Void

    @State private var isBooting = false
    @State private var showBootError = false
    @State private var bootError: String?
    @State private var showProjectDetail = false

    /// Reactive online status from TenexCoreManager
    private var isOnline: Bool {
        coreManager.projectOnlineStatus[project.id] ?? false
    }

    /// Count of online agents for this project
    private var onlineAgentCount: Int {
        coreManager.onlineAgents[project.id]?.count ?? 0
    }

    var body: some View {
        HStack(spacing: 0) {
            // Left zone: Filter toggle
            filterToggleZone

            Divider()
                .frame(height: 40)
                .padding(.horizontal, 8)

            // Center/Right zone: Project info + action
            mainActionZone
        }
        .padding(.vertical, 4)
        .alert("Boot Failed", isPresented: $showBootError) {
            Button("OK") { bootError = nil }
        } message: {
            if let error = bootError {
                Text(error)
            }
        }
        .sheet(isPresented: $showProjectDetail) {
            ProjectDetailSheet(project: project)
        }
    }

    // MARK: - Left Zone: Filter Toggle

    private var filterToggleZone: some View {
        Button(action: onToggleFilter) {
            HStack(spacing: 8) {
                Image(systemName: isFiltered ? "checkmark.circle.fill" : "circle")
                    .font(.title2)
                    .foregroundStyle(isFiltered ? .blue : .secondary)

                Text("Filter")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
        .buttonStyle(.plain)
        .frame(width: 70)
    }

    // MARK: - Center/Right Zone: Main Action

    private var mainActionZone: some View {
        Button(action: performMainAction) {
            HStack(spacing: 12) {
                // Project icon with online indicator
                projectIconView

                // Project info
                VStack(alignment: .leading, spacing: 2) {
                    HStack(spacing: 6) {
                        Text(project.title)
                            .font(.headline)
                            .foregroundStyle(.primary)
                            .lineLimit(1)

                        // Status badge
                        statusBadge
                    }

                    // Agent count or description
                    if isOnline && onlineAgentCount > 0 {
                        Text("\(onlineAgentCount) agent\(onlineAgentCount == 1 ? "" : "s") online")
                            .font(.caption)
                            .foregroundStyle(.green)
                    } else if let description = project.description {
                        Text(description)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }

                Spacer()

                // Right action indicator
                actionIndicator
            }
        }
        .buttonStyle(.plain)
        .disabled(isBooting)
    }

    private var projectIconView: some View {
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
                        .stroke(Color.systemBackground, lineWidth: 2)
                }
                .offset(x: 2, y: 2)
        }
    }

    private var statusBadge: some View {
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

    @ViewBuilder
    private var actionIndicator: some View {
        if isBooting {
            ProgressView()
                .scaleEffect(0.8)
        } else if isOnline {
            // Online: chevron to view details
            Image(systemName: "chevron.right")
                .font(.caption)
                .foregroundStyle(.tertiary)
        } else {
            // Offline: boot button
            Image(systemName: "power")
                .font(.body)
                .foregroundStyle(.blue)
        }
    }

    // MARK: - Actions

    private func performMainAction() {
        if isOnline {
            showProjectDetail = true
        } else {
            bootProject()
        }
    }

    private func bootProject() {
        isBooting = true
        bootError = nil

        Task {
            do {
                try await coreManager.safeCore.bootProject(projectId: project.id)
                // UI updates reactively when kind:24010 status event arrives
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

// MARK: - Preview

#Preview {
    ProjectsSheet(selectedProjectIds: .constant([]))
        .environmentObject(TenexCoreManager())
}
