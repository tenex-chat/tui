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
                        Text("Hiring team definitions into a project is disabled until projects can target installed backend agents.")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                    .padding(.vertical, 2)
                }
            }
            #if os(iOS)
            .listStyle(.insetGrouped)
            #else
            .listStyle(.inset)
            #endif
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
                        dismiss()
                    }
                    .fontWeight(.semibold)
                    .disabled(true)
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
