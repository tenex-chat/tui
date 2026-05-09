import SwiftUI

struct WorkspaceEditorView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss

    let target: WorkspaceEditorTarget

    @State private var name: String
    @State private var selectedProjectIds: Set<String>
    @State private var searchText = ""
    @State private var errorMessage: String?
    @State private var isSaving = false

    init(target: WorkspaceEditorTarget) {
        self.target = target
        _name = State(initialValue: target.workspace?.name ?? "")
        _selectedProjectIds = State(initialValue: target.initialProjectIds)
    }

    var body: some View {
        NavigationStack {
            List {
                Section("Name") {
                    TextField("Workspace name", text: $name)
                        .accessibilityIdentifier("workspace_editor_name")
                }

                Section("Projects") {
                    Text("\(selectedProjectIds.count) selected")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                ForEach(projectSections, id: \.title) { section in
                    if !section.projects.isEmpty {
                        Section(section.title) {
                            ForEach(section.projects, id: \.id) { project in
                                WorkspaceProjectChecklistRow(
                                    project: project,
                                    isSelected: selectedProjectIds.contains(project.id),
                                    isOnline: coreManager.projectOnlineStatus[project.id] ?? false,
                                    agentCount: coreManager.projectRosterAgents[project.id]?.count ?? project.agentPubkeys.count,
                                    onToggle: { toggleProject(project.id) }
                                )
                                .accessibilityIdentifier("workspace_editor_project_\(project.id)")
                            }
                        }
                    }
                }
            }
            #if os(iOS)
            .listStyle(.insetGrouped)
            #else
            .listStyle(.inset)
            .frame(minWidth: 560, minHeight: 640)
            #endif
            .searchable(text: $searchText, prompt: "Search projects...")
            .navigationTitle(target.workspace == nil ? "New Workspace" : "Edit Workspace")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") { dismiss() }
                }

                ToolbarItem(placement: .confirmationAction) {
                    Button {
                        save()
                    } label: {
                        if isSaving {
                            ProgressView()
                        } else {
                            Text("Save")
                                .fontWeight(.semibold)
                        }
                    }
                    .disabled(!canSave || isSaving)
                    .accessibilityIdentifier("workspace_editor_save")
                }
            }
        }
        .alert("Workspace Save Failed", isPresented: errorBinding) {
            Button("OK") { errorMessage = nil }
        } message: {
            Text(errorMessage ?? "")
        }
    }

    private var canSave: Bool {
        !name.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
            && !selectedProjectIds.isEmpty
    }

    private var filteredProjects: [Project] {
        let allProjects = coreManager.projects.sorted {
            $0.title.localizedCaseInsensitiveCompare($1.title) == .orderedAscending
        }
        guard !searchText.isEmpty else { return allProjects }
        return allProjects.filter {
            $0.title.localizedCaseInsensitiveContains(searchText)
                || ($0.description?.localizedCaseInsensitiveContains(searchText) ?? false)
        }
    }

    private var projectSections: [WorkspaceProjectSection] {
        let available = filteredProjects.filter { coreManager.projectOnlineStatus[$0.id] ?? false }
        let unavailable = filteredProjects.filter { !(coreManager.projectOnlineStatus[$0.id] ?? false) }
        return [
            WorkspaceProjectSection(title: "Available", projects: available),
            WorkspaceProjectSection(title: "Unavailable", projects: unavailable)
        ]
    }

    private var errorBinding: Binding<Bool> {
        Binding(
            get: { errorMessage != nil },
            set: { isPresented in
                if !isPresented {
                    errorMessage = nil
                }
            }
        )
    }

    private func toggleProject(_ projectId: String) {
        if selectedProjectIds.contains(projectId) {
            selectedProjectIds.remove(projectId)
        } else {
            selectedProjectIds.insert(projectId)
        }
    }

    private func save() {
        let trimmedName = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmedName.isEmpty, !selectedProjectIds.isEmpty else { return }
        isSaving = true

        Task {
            do {
                if let workspace = target.workspace {
                    try await coreManager.updateWorkspace(
                        workspace,
                        name: trimmedName,
                        projectIds: selectedProjectIds
                    )
                } else {
                    _ = try await coreManager.createWorkspace(
                        name: trimmedName,
                        projectIds: selectedProjectIds,
                        activate: true
                    )
                }
                await MainActor.run {
                    isSaving = false
                    dismiss()
                }
            } catch {
                await MainActor.run {
                    isSaving = false
                    errorMessage = error.localizedDescription
                }
            }
        }
    }
}

private struct WorkspaceProjectSection {
    let title: String
    let projects: [Project]
}

private struct WorkspaceProjectChecklistRow: View {
    let project: Project
    let isSelected: Bool
    let isOnline: Bool
    let agentCount: Int
    let onToggle: () -> Void

    var body: some View {
        Button(action: onToggle) {
            HStack(spacing: 12) {
                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .font(.title3)
                    .foregroundStyle(isSelected ? Color.accentColor : .secondary)

                ProjectColorDot(projectId: project.id, size: 28)

                VStack(alignment: .leading, spacing: 4) {
                    Text(project.title)
                        .font(.headline)
                        .foregroundStyle(.primary)
                        .lineLimit(1)

                    HStack(spacing: 8) {
                        Text(isOnline ? "Available" : "Unavailable")
                            .font(.caption)
                            .foregroundStyle(isOnline ? Color.presenceOnline : .secondary)

                        if agentCount > 0 {
                            Label("\(agentCount)", systemImage: "person.3.sequence")
                                .font(.caption)
                                .foregroundStyle(.tertiary)
                        }
                    }
                }

                Spacer()
            }
            .padding(.vertical, 4)
        }
        .buttonStyle(.borderless)
    }
}
