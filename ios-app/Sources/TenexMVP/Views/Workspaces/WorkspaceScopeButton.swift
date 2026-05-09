import SwiftUI

enum WorkspaceScopeButtonStyle {
    case sidebar
    case toolbar
}

struct WorkspaceScopeButton: View {
    @Environment(TenexCoreManager.self) private var coreManager

    let style: WorkspaceScopeButtonStyle
    @State private var showManager = false
    @State private var editorTarget: WorkspaceEditorTarget?

    var body: some View {
        Menu {
            Button {
                Task { await coreManager.applyWorkspace(nil) }
            } label: {
                selectableLabel("All Projects", isSelected: isAllProjectsSelected)
            }
            .accessibilityIdentifier("workspace_scope_all_projects")

            if !coreManager.sortedWorkspaces.isEmpty {
                Section("Workspaces") {
                    ForEach(coreManager.sortedWorkspaces, id: \.id) { workspace in
                        Button {
                            Task { await coreManager.applyWorkspace(workspace) }
                        } label: {
                            workspaceMenuLabel(workspace)
                        }
                        .accessibilityIdentifier("workspace_scope_workspace_\(workspace.id)")
                    }
                }
            }

            if coreManager.hasManualProjectScope {
                Section("Current Scope") {
                    Button {
                        editorTarget = WorkspaceEditorTarget(
                            workspace: nil,
                            initialProjectIds: coreManager.appFilterProjectIds
                        )
                    } label: {
                        Label("Save as Workspace", systemImage: "plus.square.on.square")
                    }
                    .accessibilityIdentifier("workspace_scope_save_current")
                }
            }

            Divider()

            Button {
                showManager = true
            } label: {
                Label("Manage Workspaces", systemImage: "square.grid.2x2")
            }
            .accessibilityIdentifier("workspace_scope_manage")
        } label: {
            scopeLabel
        }
        .accessibilityIdentifier("workspace_scope_button")
        .accessibilityLabel("Workspace Scope")
        .accessibilityValue(coreManager.currentProjectScopeName)
        .sheet(isPresented: $showManager) {
            WorkspaceManagerView()
                .environment(coreManager)
        }
        .sheet(item: $editorTarget) { target in
            WorkspaceEditorView(target: target)
                .environment(coreManager)
        }
    }

    private var isAllProjectsSelected: Bool {
        coreManager.activeWorkspace == nil && coreManager.appFilterProjectIds.isEmpty
    }

    @ViewBuilder
    private var scopeLabel: some View {
        switch style {
        case .sidebar:
            HStack(spacing: 10) {
                Image(systemName: "square.grid.2x2")
                    .foregroundStyle(Color.accentColor)
                    .frame(width: 18)

                VStack(alignment: .leading, spacing: 2) {
                    Text(coreManager.currentProjectScopeName)
                        .font(.subheadline.weight(.semibold))
                        .foregroundStyle(.primary)
                        .lineLimit(1)

                    Text(scopeSubtitle)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                Spacer(minLength: 8)

                Image(systemName: "chevron.up.chevron.down")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            .padding(.vertical, 4)
            .contentShape(Rectangle())

        case .toolbar:
            Label(coreManager.currentProjectScopeName, systemImage: "square.grid.2x2")
                .lineLimit(1)
        }
    }

    private var scopeSubtitle: String {
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

    @ViewBuilder
    private func selectableLabel(_ title: String, isSelected: Bool) -> some View {
        if isSelected {
            Label(title, systemImage: "checkmark")
        } else {
            Text(title)
        }
    }

    @ViewBuilder
    private func workspaceMenuLabel(_ workspace: WorkspaceInfo) -> some View {
        let isSelected = coreManager.activeWorkspaceId == workspace.id
        if isSelected {
            Label(workspace.name, systemImage: "checkmark")
        } else if workspace.pinned {
            Label(workspace.name, systemImage: "pin.fill")
        } else {
            Text(workspace.name)
        }
    }
}
