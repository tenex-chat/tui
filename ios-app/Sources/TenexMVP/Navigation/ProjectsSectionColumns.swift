import SwiftUI

// MARK: - Projects Layout Mode

enum ProjectsLayoutMode {
    case adaptive
    case shellList
    case shellDetail
    case shellComposite
}

// MARK: - Projects Tab View

struct ProjectsTabView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    let layoutMode: ProjectsLayoutMode
    private let selectedProjectIdBindingOverride: Binding<String?>?
    private let showNewProjectBindingOverride: Binding<Bool>?
    @State private var selectedProjectIdState: String?
    @State private var showNewProjectState = false
    @State private var searchText = ""

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    init(
        layoutMode: ProjectsLayoutMode = .adaptive,
        selectedProjectId: Binding<String?>? = nil,
        showNewProject: Binding<Bool>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedProjectIdBindingOverride = selectedProjectId
        self.showNewProjectBindingOverride = showNewProject
    }

    private var showNewProjectBinding: Binding<Bool> {
        showNewProjectBindingOverride ?? $showNewProjectState
    }

    private var selectedProjectIdBinding: Binding<String?> {
        selectedProjectIdBindingOverride ?? $selectedProjectIdState
    }

    private var useSplitView: Bool {
        if layoutMode == .shellList || layoutMode == .shellDetail || layoutMode == .shellComposite {
            return true
        }
        #if os(macOS)
        return true
        #else
        return horizontalSizeClass == .regular
        #endif
    }

    private var sortedProjects: [Project] {
        coreManager.projects.sorted { a, b in
            let aOnline = coreManager.projectOnlineStatus[a.id] ?? false
            let bOnline = coreManager.projectOnlineStatus[b.id] ?? false
            if aOnline != bOnline { return aOnline }
            return a.title.localizedCaseInsensitiveCompare(b.title) == .orderedAscending
        }
    }

    private var filteredProjects: [Project] {
        guard !searchText.isEmpty else { return sortedProjects }
        return sortedProjects.filter {
            $0.title.localizedCaseInsensitiveContains(searchText)
        }
    }

    var body: some View {
        Group {
            switch layoutMode {
            case .shellList:
                shellListLayout
            case .shellDetail:
                shellDetailLayout
            case .shellComposite:
                shellCompositeLayout
            case .adaptive:
                if useSplitView {
                    splitViewLayout
                } else {
                    stackLayout
                }
            }
        }
        .onChange(of: coreManager.lastDeletedProjectId) { _, deletedId in
            guard let deletedId else { return }
            if selectedProjectIdBinding.wrappedValue == deletedId {
                selectedProjectIdBinding.wrappedValue = nil
            }
        }
    }

    // MARK: - Shell Layouts

    private var shellListLayout: some View {
        projectsListView
            .navigationTitle("Projects")
            .accessibilityIdentifier("section_list_column")
    }

    private var shellDetailLayout: some View {
        projectDetailContent
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .accessibilityIdentifier("detail_column")
    }

    private var shellCompositeLayout: some View {
        #if os(macOS)
        HSplitView {
            shellListLayout
                .frame(minWidth: 340, idealWidth: 430, maxWidth: 520)

            shellDetailLayout
                .frame(minWidth: 520)
        }
        #else
        HStack(spacing: 0) {
            shellListLayout
                .frame(minWidth: 340, idealWidth: 430, maxWidth: 520)

            Divider()

            shellDetailLayout
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #endif
    }

    // MARK: - Adaptive Layouts

    private var splitViewLayout: some View {
        #if os(macOS)
        HSplitView {
            projectsListView
                .navigationTitle("Projects")
                .frame(minWidth: 340, idealWidth: 440, maxWidth: 520, maxHeight: .infinity)

            projectDetailContent
                .frame(minWidth: 520, maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #else
        NavigationSplitView {
            projectsListView
                .navigationTitle("Projects")
        } detail: {
            projectDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #endif
    }

    private var stackLayout: some View {
        NavigationStack {
            projectsListView
                .navigationTitle("Projects")
                .navigationDestination(for: String.self) { projectId in
                    ProjectSettingsView(
                        projectId: projectId,
                        selectedProjectId: selectedProjectIdBinding
                    )
                }
        }
        .sheet(isPresented: showNewProjectBinding) {
            NavigationStack {
                CreateProjectView(onComplete: { showNewProjectBinding.wrappedValue = false })
            }
            .environment(coreManager)
        }
    }

    // MARK: - Detail Content

    @ViewBuilder
    private var projectDetailContent: some View {
        if showNewProjectBinding.wrappedValue {
            CreateProjectView(onComplete: { showNewProjectBinding.wrappedValue = false })
        } else if let projectId = selectedProjectIdBinding.wrappedValue {
            ProjectSettingsView(
                projectId: projectId,
                selectedProjectId: selectedProjectIdBinding
            )
        } else {
            ContentUnavailableView(
                "Select a Project",
                systemImage: "folder",
                description: Text("Choose a project from the list")
            )
        }
    }

    // MARK: - Projects List View

    private var projectsListView: some View {
        Group {
            if filteredProjects.isEmpty {
                emptyStateView
            } else {
                List(selection: useSplitView ? selectedProjectIdBinding : nil) {
                    ForEach(filteredProjects, id: \.id) { project in
                        if useSplitView {
                            ProjectRowView(
                                project: project,
                                isOnline: coreManager.projectOnlineStatus[project.id] ?? false,
                                agentCount: project.agentDefinitionIds.count,
                                toolCount: project.mcpToolIds.count,
                                showsChevron: false
                            )
                            .tag(Optional(project.id))
                        } else {
                            NavigationLink(value: project.id) {
                                ProjectRowView(
                                    project: project,
                                    isOnline: coreManager.projectOnlineStatus[project.id] ?? false,
                                    agentCount: project.agentDefinitionIds.count,
                                    toolCount: project.mcpToolIds.count,
                                    showsChevron: true
                                )
                            }
                        }
                    }
                }
                #if os(iOS)
                .listStyle(.plain)
                #else
                .listStyle(.inset)
                #endif
                .refreshable {
                    await coreManager.manualRefresh()
                }
            }
        }
        .searchable(text: $searchText, prompt: "Search projects...")
        .toolbar {
            #if os(macOS)
            ToolbarItem(placement: .navigation) {
                AppGlobalFilterToolbarButton()
            }
            ToolbarItem(placement: .navigation) {
                Button {
                    showNewProjectBinding.wrappedValue = true
                } label: {
                    Image(systemName: "plus")
                }
            }
            #else
            ToolbarItem(placement: .automatic) {
                AppGlobalFilterToolbarButton()
            }
            ToolbarItem(placement: .automatic) {
                Button {
                    showNewProjectBinding.wrappedValue = true
                } label: {
                    Image(systemName: "plus")
                }
            }
            #endif
        }
    }

    // MARK: - Empty State

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: emptyStateIcon)
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text(emptyStateTitle)
                .font(.title2)
                .fontWeight(.semibold)

            Text(emptyStateMessage)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            if !searchText.isEmpty || !coreManager.isAppFilterDefault {
                Button {
                    searchText = ""
                    coreManager.resetAppFilterToDefaults()
                } label: {
                    Label("Clear Filters", systemImage: "xmark.circle")
                }
                .adaptiveGlassButtonStyle()
                .padding(.top, 8)
            }
        }
        .padding()
    }

    private var emptyStateIcon: String {
        if !searchText.isEmpty {
            return "magnifyingglass"
        } else if !coreManager.isAppFilterDefault {
            return "line.3.horizontal.decrease.circle"
        } else {
            return "folder"
        }
    }

    private var emptyStateTitle: String {
        if !searchText.isEmpty {
            return "No Matching Projects"
        } else if !coreManager.isAppFilterDefault {
            return "No Projects in Current Filter"
        } else {
            return "No Projects"
        }
    }

    private var emptyStateMessage: String {
        if !searchText.isEmpty {
            return "Try adjusting your search terms"
        } else if !coreManager.isAppFilterDefault {
            return "Try adjusting your project/time filter"
        } else {
            return "Create a project to get started"
        }
    }
}

// MARK: - Project Row View

struct ProjectRowView: View {
    let project: Project
    let isOnline: Bool
    let agentCount: Int
    let toolCount: Int
    var showsChevron: Bool = true

    var body: some View {
        HStack(spacing: 12) {
            // Color-coded folder icon
            RoundedRectangle(cornerRadius: 7)
                .fill(deterministicColor(for: project.id).gradient)
                .frame(width: 28, height: 28)
                .overlay {
                    Image(systemName: "folder.fill")
                        .font(.caption)
                        .foregroundStyle(.white)
                }

            VStack(alignment: .leading, spacing: 4) {
                // Row 1: Title + relative time
                HStack {
                    Text(project.title)
                        .font(.headline)
                        .lineLimit(1)

                    Spacer()

                    RelativeTimeText(timestamp: project.createdAt, style: .localizedAbbreviated)
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                }

                // Row 2: Description
                if let description = project.description, !description.isEmpty {
                    Text(description)
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                } else {
                    Text("No description")
                        .font(.subheadline)
                        .foregroundStyle(.tertiary)
                        .italic()
                }

                // Row 3: Status badges
                HStack(spacing: 8) {
                    // Online/Offline badge
                    Text(isOnline ? "Online" : "Offline")
                        .font(.caption2)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background((isOnline ? Color.presenceOnline : .secondary).opacity(0.16))
                        .foregroundStyle(isOnline ? Color.presenceOnline : .secondary)
                        .clipShape(Capsule())

                    if agentCount > 0 {
                        Label("\(agentCount)", systemImage: "person.3.sequence")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }

                    if toolCount > 0 {
                        Label("\(toolCount)", systemImage: "wrench")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }
                }
            }

            if showsChevron {
                Image(systemName: "chevron.right")
                    .font(.caption)
                    .foregroundStyle(.tertiary)
            }
        }
        .padding(.vertical, 8)
    }

}
