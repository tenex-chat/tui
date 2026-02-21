import SwiftUI

struct TeamHireSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss

    let team: TeamInfo
    let onConfirm: (Project) -> Void

    @State private var selectedProjectId: String?
    @State private var searchText = ""

    private var sortedProjects: [Project] {
        coreManager.projects
            .filter { !$0.isDeleted }
            .sorted { lhs, rhs in
                lhs.title.localizedCaseInsensitiveCompare(rhs.title) == .orderedAscending
            }
    }

    private var filteredProjects: [Project] {
        let query = searchText.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !query.isEmpty else { return sortedProjects }

        return sortedProjects.filter { project in
            project.title.localizedCaseInsensitiveContains(query)
                || project.id.localizedCaseInsensitiveContains(query)
                || (project.description?.localizedCaseInsensitiveContains(query) ?? false)
        }
    }

    var body: some View {
        NavigationStack {
            List {
                Section {
                    VStack(alignment: .leading, spacing: 6) {
                        Text(team.title.isEmpty ? "Untitled Team" : team.title)
                            .font(.headline)
                        Text("Hire \(team.agentDefinitionIds.count) agent definition\(team.agentDefinitionIds.count == 1 ? "" : "s") into one project")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 2)
                }

                Section {
                    if filteredProjects.isEmpty {
                        ContentUnavailableView(
                            "No Projects",
                            systemImage: "folder.badge.questionmark",
                            description: Text(searchText.isEmpty ? "No projects available." : "No projects match your search.")
                        )
                    } else {
                        ForEach(filteredProjects, id: \.id) { project in
                            Button {
                                selectedProjectId = project.id
                            } label: {
                                HStack(spacing: 10) {
                                    VStack(alignment: .leading, spacing: 4) {
                                        Text(project.title)
                                            .font(.body.weight(.medium))
                                            .foregroundStyle(.primary)

                                        Text(project.id)
                                            .font(.caption2.monospaced())
                                            .foregroundStyle(.secondary)
                                            .lineLimit(1)
                                    }

                                    Spacer()

                                    Image(systemName: selectedProjectId == project.id ? "checkmark.circle.fill" : "circle")
                                        .font(.title3)
                                        .foregroundStyle(selectedProjectId == project.id ? Color.accentColor : .secondary)
                                }
                                .contentShape(Rectangle())
                            }
                            .buttonStyle(.plain)
                        }
                    }
                }
            }
            #if os(iOS)
            .listStyle(.insetGrouped)
            #else
            .listStyle(.inset)
            #endif
            .searchable(text: $searchText, prompt: "Search projects")
            .navigationTitle("Hire Team")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Hire") {
                        guard let selectedProjectId,
                              let project = sortedProjects.first(where: { $0.id == selectedProjectId }) else {
                            return
                        }
                        onConfirm(project)
                        dismiss()
                    }
                    .fontWeight(.semibold)
                    .disabled(selectedProjectId == nil)
                }
            }
        }
        #if os(iOS)
        .tenexModalPresentation(detents: [.medium, .large])
        #else
        .frame(minWidth: 500, idealWidth: 620, minHeight: 420, idealHeight: 560)
        #endif
    }
}
