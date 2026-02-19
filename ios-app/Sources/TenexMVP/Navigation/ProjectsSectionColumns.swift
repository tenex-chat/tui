import SwiftUI

struct ProjectsSectionContainer: View {
    @Binding var selectedProjectId: String?

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    private var useSplitLayout: Bool {
        #if os(macOS)
        true
        #else
        horizontalSizeClass == .regular
        #endif
    }

    var body: some View {
        Group {
            if useSplitLayout {
                splitLayout
            } else {
                ProjectsSectionListColumn(selectedProjectId: $selectedProjectId)
            }
        }
    }

    @ViewBuilder
    private var splitLayout: some View {
        #if os(macOS)
        HSplitView {
            ProjectsSectionListColumn(selectedProjectId: $selectedProjectId)
                .frame(minWidth: 320, idealWidth: 420, maxWidth: 520, maxHeight: .infinity)

            ProjectsSectionDetailColumn(selectedProjectId: $selectedProjectId)
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #else
        NavigationSplitView {
            ProjectsSectionListColumn(selectedProjectId: $selectedProjectId)
        } detail: {
            ProjectsSectionDetailColumn(selectedProjectId: $selectedProjectId)
        }
        #endif
    }
}

struct ProjectsSectionListColumn: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
    @Binding var selectedProjectId: String?

    private var sortedProjects: [ProjectInfo] {
        coreManager.projects.sorted { a, b in
            let aOnline = coreManager.projectOnlineStatus[a.id] ?? false
            let bOnline = coreManager.projectOnlineStatus[b.id] ?? false
            if aOnline != bOnline { return aOnline }
            return a.title.localizedCaseInsensitiveCompare(b.title) == .orderedAscending
        }
    }

    var body: some View {
        List(selection: $selectedProjectId) {
            ForEach(sortedProjects, id: \.id) { project in
                HStack(spacing: 10) {
                    RoundedRectangle(cornerRadius: 7)
                        .fill(deterministicColor(for: project.id).gradient)
                        .frame(width: 28, height: 28)
                        .overlay {
                            Image(systemName: "folder.fill")
                                .font(.caption)
                                .foregroundStyle(.white)
                        }

                    VStack(alignment: .leading, spacing: 2) {
                        Text(project.title)
                            .font(.headline)
                            .lineLimit(1)

                        Text((coreManager.projectOnlineStatus[project.id] ?? false) ? "Online" : "Offline")
                            .font(.caption)
                            .foregroundStyle((coreManager.projectOnlineStatus[project.id] ?? false) ? Color.presenceOnline : .secondary)
                    }

                    Spacer()
                }
                .tag(Optional(project.id))
            }
        }
        #if os(macOS)
        .listStyle(.inset)
        #else
        .listStyle(.plain)
        #endif
        .navigationTitle("Projects")
    }
}

struct ProjectsSectionDetailColumn: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
    @Binding var selectedProjectId: String?

    var body: some View {
        Group {
            if let selectedProjectId {
                ProjectSettingsView(
                    projectId: selectedProjectId,
                    selectedProjectId: $selectedProjectId
                )
                .environmentObject(coreManager)
            } else {
                ContentUnavailableView(
                    "Select a Project",
                    systemImage: "folder",
                    description: Text("Choose a project from the list")
                )
            }
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
    }
}

struct ProjectsTabView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @State private var selectedProjectIds: Set<String> = []

    var body: some View {
        NavigationStack {
            ProjectsContentView(selectedProjectIds: $selectedProjectIds)
                .environmentObject(coreManager)
        }
    }
}
