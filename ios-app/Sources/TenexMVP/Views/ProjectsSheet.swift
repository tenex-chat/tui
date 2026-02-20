import SwiftUI

/// Reusable projects content for filtering conversations by project.
/// Row tap only toggles filter selection; project boot remains a dedicated button.
struct ProjectsContentView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @Binding var selectedProjectIds: Set<String>
    var showDoneButton: Bool = false
    @Environment(\.dismiss) private var dismiss

    var body: some View {
        List {
            allProjectsRow

            if !coreManager.projects.isEmpty {
                Section("Projects") {
                    ForEach(sortedProjects, id: \.id) { project in
                        ProjectsSheetRow(
                            project: project,
                            isFiltered: selectedProjectIds.contains(project.id),
                            isOnline: coreManager.projectOnlineStatus[project.id] ?? false,
                            onlineAgentCount: coreManager.onlineAgents[project.id]?.count ?? 0,
                            onToggleFilter: { toggleProject(project.id) }
                        )
                    }
                }
            }
        }
        #if os(iOS)
        .listStyle(.insetGrouped)
        .navigationTitle("Projects")
        .navigationBarTitleDisplayMode(.inline)
        #else
        .listStyle(.inset)
        .navigationTitle("Projects")
        #endif
        .toolbar {
            if showDoneButton {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                        .fontWeight(.semibold)
                }
            }
        }
    }

    private var allProjectsRow: some View {
        Button(action: { selectedProjectIds.removeAll() }) {
            HStack(spacing: 12) {
                Image(systemName: selectedProjectIds.isEmpty ? "checkmark.circle.fill" : "circle")
                    .font(.title3)
                    .foregroundStyle(selectedProjectIds.isEmpty ? Color.accentColor : .secondary)

                VStack(alignment: .leading, spacing: 2) {
                    Text("All Projects")
                        .font(.headline)
                        .foregroundStyle(.primary)
                    Text("\(coreManager.projects.count) projects")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer()
            }
            .padding(.vertical, 4)
        }
        .buttonStyle(.borderless)
        .accessibilityLabel("Show all projects")
        .accessibilityHint("Clears project filters")
    }

    private var sortedProjects: [Project] {
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

/// Sheet wrapper for `ProjectsContentView`.
struct ProjectsSheet: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @Binding var selectedProjectIds: Set<String>

    var body: some View {
        NavigationStack {
            ProjectsContentView(
                selectedProjectIds: $selectedProjectIds,
                showDoneButton: true
            )
            .environmentObject(coreManager)
        }
        #if os(iOS)
        .tenexModalPresentation(detents: [.medium, .large])
        #else
        .frame(minWidth: 500, minHeight: 600)
        #endif
    }
}

private struct ProjectsSheetRow: View {
    @EnvironmentObject var coreManager: TenexCoreManager

    let project: Project
    let isFiltered: Bool
    let isOnline: Bool
    let onlineAgentCount: Int
    let onToggleFilter: () -> Void

    @State private var isBooting = false
    @State private var showBootError = false
    @State private var bootError: String?

    var body: some View {
        HStack(spacing: 12) {
            Button(action: onToggleFilter) {
                HStack(spacing: 12) {
                    Image(systemName: isFiltered ? "checkmark.circle.fill" : "circle")
                        .font(.title3)
                        .foregroundStyle(isFiltered ? Color.accentColor : .secondary)

                    projectIconView

                    VStack(alignment: .leading, spacing: 3) {
                        Text(project.title)
                            .font(.headline)
                            .foregroundStyle(.primary)
                            .lineLimit(1)

                        HStack(spacing: 6) {
                            Text(isOnline ? "Online" : "Offline")
                                .font(.caption)
                                .foregroundStyle(isOnline ? Color.presenceOnline : .secondary)

                            if isOnline {
                                Text("•")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)

                                Text("\(onlineAgentCount) agent\(onlineAgentCount == 1 ? "" : "s")")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                            }
                        }
                    }

                    Spacer()
                }
                .padding(.vertical, 6)
            }
            .buttonStyle(.borderless)
            .accessibilityLabel("Filter by \(project.title)")
            .accessibilityValue(isFiltered ? "On" : "Off")
            .accessibilityHint("Double tap to \(isFiltered ? "remove from" : "add to") filter")

            if !isOnline {
                Button(action: bootProject) {
                    if isBooting {
                        ProgressView()
                            .scaleEffect(0.8)
                    } else {
                        Image(systemName: "power")
                            .font(.body)
                            .foregroundStyle(Color.agentBrand)
                    }
                }
                .buttonStyle(.borderless)
                .disabled(isBooting)
                .accessibilityLabel("Boot \(project.title)")
                .accessibilityHint("Starts this project")
            }
        }
        .alert("Boot Failed", isPresented: $showBootError) {
            Button("OK") { bootError = nil }
        } message: {
            if let error = bootError {
                Text(error)
            }
        }
    }

    private var projectIconView: some View {
        RoundedRectangle(cornerRadius: 8)
            .fill(projectColor.gradient)
            .frame(width: 36, height: 36)
            .overlay {
                Image(systemName: "folder.fill")
                    .foregroundStyle(.white)
                    .font(.body)
            }
    }

    private func bootProject() {
        isBooting = true
        bootError = nil

        Task {
            do {
                try await coreManager.safeCore.bootProject(projectId: project.id)
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

    private var projectColor: Color {
        deterministicColor(for: project.id)
    }
}

#Preview {
    ProjectsSheet(selectedProjectIds: .constant([]))
        .environmentObject(TenexCoreManager())
}
