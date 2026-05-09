import SwiftUI

struct WorkspaceEditorTarget: Identifiable {
    let workspace: WorkspaceInfo?
    let initialProjectIds: Set<String>

    var id: String {
        workspace?.id ?? "new:\(initialProjectIds.sorted().joined(separator: ","))"
    }
}

struct WorkspaceManagerView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss

    @State private var editorTarget: WorkspaceEditorTarget?
    @State private var workspacePendingDelete: WorkspaceInfo?

    var body: some View {
        NavigationStack {
            List {
                Section("Current Scope") {
                    currentScopeRow
                    if coreManager.activeWorkspace != nil || !coreManager.appFilterProjectIds.isEmpty {
                        Button {
                            Task { await coreManager.applyWorkspace(nil) }
                        } label: {
                            Label("Show All Projects", systemImage: "xmark.circle")
                        }
                        .accessibilityIdentifier("workspace_manager_show_all")
                    }
                }

                if coreManager.hasManualProjectScope {
                    Section("Unsaved Scope") {
                        Button {
                            editorTarget = WorkspaceEditorTarget(
                                workspace: nil,
                                initialProjectIds: coreManager.appFilterProjectIds
                            )
                        } label: {
                            Label("Save Current Scope", systemImage: "plus.square.on.square")
                        }
                        .accessibilityIdentifier("workspace_manager_save_current")
                    }
                }

                Section("Saved Workspaces") {
                    if coreManager.sortedWorkspaces.isEmpty {
                        ContentUnavailableView(
                            "No Saved Workspaces",
                            systemImage: "square.grid.2x2",
                            description: Text("Save a project scope to switch context quickly.")
                        )
                        .frame(maxWidth: .infinity)
                    } else {
                        ForEach(coreManager.sortedWorkspaces, id: \.id) { workspace in
                            WorkspaceManagerRow(
                                workspace: workspace,
                                isActive: coreManager.activeWorkspaceId == workspace.id,
                                projectCount: coreManager.workspaceProjectCount(workspace),
                                onlineProjectCount: coreManager.workspaceOnlineProjectCount(workspace),
                                onSelect: {
                                    Task { await coreManager.applyWorkspace(workspace) }
                                },
                                onEdit: {
                                    editorTarget = WorkspaceEditorTarget(
                                        workspace: workspace,
                                        initialProjectIds: coreManager.workspaceProjectIds(for: workspace)
                                    )
                                },
                                onPin: {
                                    Task { try? await coreManager.toggleWorkspacePinned(workspace) }
                                },
                                onBootOffline: {
                                    bootOfflineProjects(in: workspace)
                                },
                                onDelete: {
                                    workspacePendingDelete = workspace
                                }
                            )
                            .accessibilityIdentifier("workspace_manager_row_\(workspace.id)")
                        }
                    }
                }
            }
            #if os(iOS)
            .listStyle(.insetGrouped)
            #else
            .listStyle(.inset)
            .frame(minWidth: 560, minHeight: 620)
            #endif
            .navigationTitle("Workspaces")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") { dismiss() }
                }

                ToolbarItem(placement: .primaryAction) {
                    Button {
                        editorTarget = WorkspaceEditorTarget(
                            workspace: nil,
                            initialProjectIds: coreManager.appFilterProjectIds
                        )
                    } label: {
                        Image(systemName: "plus")
                    }
                    .accessibilityLabel("Create workspace")
                }
            }
        }
        .sheet(item: $editorTarget) { target in
            WorkspaceEditorView(target: target)
                .environment(coreManager)
        }
        .alert("Delete Workspace?", isPresented: deleteBinding) {
            Button("Cancel", role: .cancel) {
                workspacePendingDelete = nil
            }
            Button("Delete", role: .destructive) {
                guard let workspace = workspacePendingDelete else { return }
                Task {
                    try? await coreManager.deleteWorkspace(workspace)
                    workspacePendingDelete = nil
                }
            }
        } message: {
            Text("This removes the saved scope. Projects and conversations stay untouched.")
        }
    }

    private var currentScopeRow: some View {
        HStack(spacing: 12) {
            Image(systemName: "square.grid.2x2.fill")
                .font(.title3)
                .foregroundStyle(Color.accentColor)

            VStack(alignment: .leading, spacing: 3) {
                Text(coreManager.currentProjectScopeName)
                    .font(.headline)
                    .foregroundStyle(.primary)
                    .lineLimit(1)
                Text(currentScopeSubtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()
        }
        .accessibilityIdentifier("workspace_manager_current_scope")
    }

    private var currentScopeSubtitle: String {
        if let activeWorkspace = coreManager.activeWorkspace {
            let total = coreManager.workspaceProjectCount(activeWorkspace)
            let online = coreManager.workspaceOnlineProjectCount(activeWorkspace)
            return "\(total) projects · \(online) available"
        }
        if coreManager.appFilterProjectIds.isEmpty {
            return "\(coreManager.projects.count) projects"
        }
        return "\(coreManager.appFilterProjectIds.count) selected projects"
    }

    private var deleteBinding: Binding<Bool> {
        Binding(
            get: { workspacePendingDelete != nil },
            set: { isPresented in
                if !isPresented {
                    workspacePendingDelete = nil
                }
            }
        )
    }

    private func bootOfflineProjects(in workspace: WorkspaceInfo) {
        let projectIds = coreManager.workspaceProjectIds(for: workspace)
        Task {
            for projectId in projectIds where !(coreManager.projectOnlineStatus[projectId] ?? false) {
                try? await coreManager.core.bootProject(projectId: projectId)
            }
        }
    }
}

private struct WorkspaceManagerRow: View {
    let workspace: WorkspaceInfo
    let isActive: Bool
    let projectCount: Int
    let onlineProjectCount: Int
    let onSelect: () -> Void
    let onEdit: () -> Void
    let onPin: () -> Void
    let onBootOffline: () -> Void
    let onDelete: () -> Void

    var body: some View {
        Button(action: onSelect) {
            HStack(spacing: 12) {
                Image(systemName: isActive ? "checkmark.circle.fill" : "square.grid.2x2")
                    .font(.title3)
                    .foregroundStyle(isActive ? Color.accentColor : .secondary)
                    .frame(width: 26)

                VStack(alignment: .leading, spacing: 4) {
                    HStack(spacing: 6) {
                        Text(workspace.name)
                            .font(.headline)
                            .foregroundStyle(.primary)
                            .lineLimit(1)

                        if workspace.pinned {
                            Image(systemName: "pin.fill")
                                .font(.caption2)
                                .foregroundStyle(.secondary)
                        }
                    }

                    Text("\(projectCount) projects · \(onlineProjectCount) available")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer()
            }
            .padding(.vertical, 4)
        }
        .buttonStyle(.borderless)
        .contextMenu {
            Button(action: onEdit) {
                Label("Edit", systemImage: "pencil")
            }
            Button(action: onPin) {
                Label(workspace.pinned ? "Unpin" : "Pin", systemImage: workspace.pinned ? "pin.slash" : "pin")
            }
            Button(action: onBootOffline) {
                Label("Boot Offline Projects", systemImage: "power")
            }
            Button(role: .destructive, action: onDelete) {
                Label("Delete", systemImage: "trash")
            }
        }
        #if os(iOS)
        .swipeActions(edge: .trailing, allowsFullSwipe: false) {
            Button(role: .destructive, action: onDelete) {
                Label("Delete", systemImage: "trash")
            }
            Button(action: onEdit) {
                Label("Edit", systemImage: "pencil")
            }
            .tint(.blue)
        }
        .swipeActions(edge: .leading, allowsFullSwipe: false) {
            Button(action: onPin) {
                Label(workspace.pinned ? "Unpin" : "Pin", systemImage: workspace.pinned ? "pin.slash" : "pin")
            }
            .tint(.orange)
        }
        #endif
    }
}
