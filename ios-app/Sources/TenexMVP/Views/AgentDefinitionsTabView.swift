import SwiftUI

enum AgentDefinitionsLayoutMode {
    case adaptive
    case shellList
    case shellDetail
}

private func awesomeAgentsProfileURL(for pubkey: String) -> URL? {
    guard let npub = Bech32.hexToNpub(pubkey) else { return nil }
    return URL(string: "https://awesome-agents.com/p/\(npub)")
}

struct AgentDefinitionsTabView: View {
    @Environment(TenexCoreManager.self) private var coreManager

    let layoutMode: AgentDefinitionsLayoutMode
    private let selectedAgentBindingOverride: Binding<AgentDefinition?>?
    private let onShowAllTeams: (() -> Void)?

    @StateObject private var viewModel = AgentDefinitionsViewModel()
    @StateObject private var teamsViewModel = TeamsViewModel()
    @State private var selectedAgentState: AgentDefinition?
    @State private var hasConfiguredViewModel = false
    @State private var hasConfiguredTeamsViewModel = false
    @State private var navigationPath: [AgentDefinitionListItem] = []
    @State private var assignmentTarget: AgentDefinitionListItem?
    @State private var assignmentResult: AgentAssignmentResult?
    @State private var teamCreationTarget: AgentDefinitionListItem?
    @State private var selectedFeaturedTeam: TeamListItem?
    @State private var showNewAgentDefinitionModal = false
    @State private var showAllTeamsSheet = false

    init(
        layoutMode: AgentDefinitionsLayoutMode = .adaptive,
        selectedAgent: Binding<AgentDefinition?>? = nil,
        onShowAllTeams: (() -> Void)? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedAgentBindingOverride = selectedAgent
        self.onShowAllTeams = onShowAllTeams
    }

    private var selectedAgentBinding: Binding<AgentDefinition?> {
        selectedAgentBindingOverride ?? $selectedAgentState
    }

    var body: some View {
        Group {
            switch layoutMode {
            case .shellList, .adaptive:
                navigationListLayout
            case .shellDetail:
                shellDetailLayout
            }
        }
        .task {
            if !hasConfiguredViewModel {
                viewModel.configure(with: coreManager)
                hasConfiguredViewModel = true
            }
            if !hasConfiguredTeamsViewModel {
                teamsViewModel.configure(with: coreManager)
                hasConfiguredTeamsViewModel = true
            }

            async let agentLoad: Void = viewModel.loadIfNeeded()
            async let teamLoad: Void = teamsViewModel.loadIfNeeded()
            _ = await (agentLoad, teamLoad)
        }
        .onChange(of: coreManager.diagnosticsVersion) { _, _ in
            Task { await viewModel.refresh() }
        }
        .onChange(of: coreManager.teamsVersion) { _, _ in
            Task { await teamsViewModel.refresh() }
        }
        .sheet(item: $assignmentTarget) { item in
            AgentDefinitionProjectAssignmentSheet(item: item) { result in
                assignmentResult = result
            }
            .environment(coreManager)
        }
        .sheet(item: $selectedFeaturedTeam) { item in
            NavigationStack {
                TeamDetailView(teamId: item.id, viewModel: teamsViewModel)
            }
            .environment(coreManager)
        }
        .sheet(item: $teamCreationTarget) { item in
            AgentDefinitionTeamCreationSheet(item: item) { result in
                assignmentResult = result
            }
            .environment(coreManager)
        }
        .sheet(isPresented: $showAllTeamsSheet) {
            TeamsTabView(layoutMode: .adaptive)
                .environment(coreManager)
        }
        .sheet(isPresented: $showNewAgentDefinitionModal) {
            NewAgentDefinitionSheet {
                Task {
                    await viewModel.refresh()
                    if let newestMine = viewModel.mine.first {
                        selectedAgentBinding.wrappedValue = newestMine.agent
                        navigationPath = [newestMine]
                    }
                }
            }
            .environment(coreManager)
        }
        .alert(item: $assignmentResult) { result in
            Alert(
                title: Text(result.title),
                message: Text(result.message),
                dismissButton: .default(Text("OK"))
            )
        }
        .alert(
            "Unable to Load Agent Definitions",
            isPresented: Binding(
                get: { viewModel.errorMessage != nil },
                set: { isPresented in
                    if !isPresented {
                        viewModel.errorMessage = nil
                    }
                }
            )
        ) {
            Button("OK", role: .cancel) {
                viewModel.errorMessage = nil
            }
        } message: {
            Text(viewModel.errorMessage ?? "Unknown error")
        }
    }

    private var navigationListLayout: some View {
        NavigationStack(path: $navigationPath) {
            listContent
                .navigationTitle("Agent Definitions")
                #if os(iOS)
                .navigationBarTitleDisplayMode(.inline)
                #else
                .toolbarTitleDisplayMode(.inline)
                #endif
                .navigationDestination(for: AgentDefinitionListItem.self) { item in
                    AgentDefinitionDetailView(
                        item: item,
                        canDelete: viewModel.canDelete(item),
                        onAssignmentResult: { result in
                            assignmentResult = result
                        },
                        onDelete: {
                            let deleted = await viewModel.deleteAgentDefinition(id: item.id)
                            if deleted {
                                selectedAgentBinding.wrappedValue = nil
                                navigationPath.removeAll { $0.id == item.id }
                            }
                            return deleted
                        }
                    )
                }
                .searchable(text: $viewModel.searchText, placement: .toolbar, prompt: "Search definitions")
                .toolbar {
                    ToolbarItem(placement: .automatic) {
                        if viewModel.isLoading {
                            ProgressView()
                                .controlSize(.small)
                        }
                    }

                    ToolbarItem(placement: .automatic) {
                        Button {
                            showNewAgentDefinitionModal = true
                        } label: {
                            Label("New", systemImage: "plus")
                        }
                    }

                    ToolbarItem(placement: .automatic) {
                        Button {
                            Task { await viewModel.refresh() }
                        } label: {
                            Label("Refresh", systemImage: "arrow.clockwise")
                        }
                        .disabled(viewModel.isLoading)
                    }
                }
        }
        .accessibilityIdentifier("section_list_column")
    }

    private var shellDetailLayout: some View {
        ContentUnavailableView(
            "Agent Definitions",
            systemImage: "person.3.sequence",
            description: Text("Agent details now open from the definitions list.")
        )
        .accessibilityIdentifier("detail_column")
    }

    private var listContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                AgentDefinitionsHeroHeader(
                    mineCount: viewModel.filteredMine.count,
                    communityCount: viewModel.filteredCommunity.count
                )

                featuredTeamsMicroSection

                if viewModel.filteredMine.isEmpty, viewModel.filteredCommunity.isEmpty {
                    emptyState
                } else {
                    if !viewModel.filteredMine.isEmpty {
                        cardSection(
                            title: "Mine",
                            subtitle: "Definitions you authored",
                            items: viewModel.filteredMine
                        )
                    }

                    if !viewModel.filteredCommunity.isEmpty {
                        cardSection(
                            title: "Community",
                            subtitle: "Definitions from other authors",
                            items: viewModel.filteredCommunity
                        )
                    }
                }
            }
            .frame(maxWidth: 960, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.horizontal, 20)
            .padding(.vertical, 24)
        }
        .background(Color.systemBackground.ignoresSafeArea())
        #if os(iOS)
        .refreshable {
            async let refreshAgents: Void = viewModel.refresh()
            async let refreshTeams: Void = teamsViewModel.refresh()
            _ = await (refreshAgents, refreshTeams)
        }
        #endif
    }

    private var featuredTeamsMicroSection: some View {
        VStack(alignment: .leading, spacing: 14) {
            HStack(alignment: .firstTextBaseline, spacing: 12) {
                VStack(alignment: .leading, spacing: 3) {
                    Text("Featured Teams")
                        .font(.title3.weight(.bold))
                    Text("Discover ready-to-hire squads built by the community.")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                }

                Spacer(minLength: 0)

                Button(action: showAllTeams) {
                    HStack(spacing: 5) {
                        Text("Show all")
                        Image(systemName: "chevron.right")
                            .font(.caption.weight(.bold))
                    }
                    .font(.subheadline.weight(.semibold))
                    .foregroundStyle(Color.agentBrand)
                }
                .buttonStyle(.plain)
            }

            if teamsViewModel.isLoading, teamsViewModel.featuredTeams.isEmpty {
                HStack(spacing: 8) {
                    ProgressView()
                        .controlSize(.small)
                    Text("Loading featured teams...")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
                .frame(maxWidth: .infinity, alignment: .leading)
                .padding(.vertical, 10)
            } else if teamsViewModel.featuredTeams.isEmpty {
                ContentUnavailableView(
                    "No Featured Teams Yet",
                    systemImage: "person.2.slash",
                    description: Text("Teams will appear here as soon as they are discovered.")
                )
                .frame(maxWidth: .infinity, minHeight: 150)
            } else {
                ScrollView(.horizontal, showsIndicators: false) {
                    HStack(spacing: 14) {
                        ForEach(teamsViewModel.featuredTeams.prefix(10)) { item in
                            Button {
                                selectedFeaturedTeam = item
                            } label: {
                                TeamFeaturedCard(item: item)
                            }
                            .buttonStyle(.plain)
                        }
                    }
                    .padding(.vertical, 2)
                }
            }
        }
    }

    private func cardSection(
        title: String,
        subtitle: String,
        items: [AgentDefinitionListItem]
    ) -> some View {
        let imageItems = items.filter(hasDefinitionImage)
        let compactItems = items.filter { !hasDefinitionImage($0) }

        return VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.headline)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            if !imageItems.isEmpty {
                LazyVGrid(columns: imageCardColumns, alignment: .leading, spacing: 14) {
                    ForEach(imageItems) { item in
                        agentDefinitionButton(for: item, style: .visual)
                    }
                }
            }

            if !compactItems.isEmpty {
                LazyVGrid(columns: compactCardColumns, alignment: .leading, spacing: 12) {
                    ForEach(compactItems) { item in
                        agentDefinitionButton(for: item, style: .compact)
                    }
                }
            }
        }
    }

    private enum AgentDefinitionCardStyle {
        case visual
        case compact
    }

    private var imageCardColumns: [GridItem] {
        #if os(macOS) || targetEnvironment(macCatalyst)
        return Array(
            repeating: GridItem(.flexible(minimum: 0, maximum: .infinity), spacing: 14, alignment: .top),
            count: 4
        )
        #else
        return [
            GridItem(
                .adaptive(
                    minimum: AgentDefinitionVisualCard.gridMinimumWidth,
                    maximum: AgentDefinitionVisualCard.gridMaximumWidth
                ),
                spacing: 14,
                alignment: .top
            )
        ]
        #endif
    }

    private var compactCardColumns: [GridItem] {
        #if os(macOS) || targetEnvironment(macCatalyst)
        return Array(
            repeating: GridItem(.flexible(minimum: 0, maximum: .infinity), spacing: 14, alignment: .top),
            count: 2
        )
        #else
        return [GridItem(.adaptive(minimum: 320, maximum: .infinity), spacing: 12, alignment: .top)]
        #endif
    }

    @ViewBuilder
    private func agentDefinitionCard(for item: AgentDefinitionListItem, style: AgentDefinitionCardStyle) -> some View {
        switch style {
        case .visual:
            AgentDefinitionVisualCard(item: item)
        case .compact:
            AgentDefinitionCompactCard(item: item)
        }
    }

    private func agentDefinitionButton(for item: AgentDefinitionListItem, style: AgentDefinitionCardStyle) -> some View {
        Button {
            selectedAgentBinding.wrappedValue = item.agent
            navigationPath.append(item)
        } label: {
            agentDefinitionCard(for: item, style: style)
        }
        .buttonStyle(.plain)
        .contextMenu {
            Button {
                presentAssignmentSheet(for: item)
            } label: {
                Label("Add to Projects", systemImage: "plus")
            }

            Button {
                presentTeamCreationSheet(for: item)
            } label: {
                Label("Create Team", systemImage: "person.3")
            }
        }
    }

    private func hasDefinitionImage(_ item: AgentDefinitionListItem) -> Bool {
        guard
            let raw = item.agent.picture?.trimmingCharacters(in: .whitespacesAndNewlines),
            !raw.isEmpty
        else {
            return false
        }

        return URL(string: raw) != nil
    }

    private func presentAssignmentSheet(for item: AgentDefinitionListItem) {
        if assignmentTarget?.id == item.id {
            assignmentTarget = nil
            DispatchQueue.main.async {
                assignmentTarget = item
            }
            return
        }

        assignmentTarget = item
    }

    private func presentTeamCreationSheet(for item: AgentDefinitionListItem) {
        if teamCreationTarget?.id == item.id {
            teamCreationTarget = nil
            DispatchQueue.main.async {
                teamCreationTarget = item
            }
            return
        }

        teamCreationTarget = item
    }

    private func showAllTeams() {
        if let onShowAllTeams {
            onShowAllTeams()
        } else {
            showAllTeamsSheet = true
        }
    }

    private var emptyState: some View {
        ContentUnavailableView(
            "No Agent Definitions",
            systemImage: "person.3.sequence",
            description: Text(viewModel.searchText.isEmpty ? "Definitions will appear here when discovered" : "Try adjusting your search query")
        )
        .frame(maxWidth: .infinity, minHeight: 280)
    }
}

private struct AgentDefinitionDetailView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.openURL) private var openURL

    let item: AgentDefinitionListItem
    let canDelete: Bool
    let onAssignmentResult: (AgentAssignmentResult) -> Void
    let onDelete: () async -> Bool

    @State private var showDeleteConfirmation = false
    @State private var isDeleting = false
    @State private var showAssignmentSheet = false
    @State private var showTeamCreationSheet = false

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()

    private var displayName: String {
        item.agent.name.isEmpty ? "Unnamed Agent" : item.agent.name
    }

    private var authorProfileURL: URL? {
        awesomeAgentsProfileURL(for: item.agent.pubkey)
    }

    private var contentText: String {
        item.agent.content.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var instructionsText: String {
        item.agent.instructions.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var hasDistinctInstructions: Bool {
        !instructionsText.isEmpty && instructionsText != contentText
    }

    private var agentImageURL: URL? {
        guard
            let rawValue = item.agent.picture?.trimmingCharacters(in: .whitespacesAndNewlines),
            !rawValue.isEmpty,
            let url = URL(string: rawValue)
        else {
            return nil
        }
        return url
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 0) {
                heroSection

                VStack(alignment: .leading, spacing: 24) {
                    if !contentText.isEmpty {
                        MarkdownView(content: contentText)
                    }

                    if hasDistinctInstructions {
                        VStack(alignment: .leading, spacing: 10) {
                            Text("Instructions")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(.secondary)
                            MarkdownView(content: instructionsText)
                        }
                    }

                    if !item.agent.useCriteria.isEmpty {
                        VStack(alignment: .leading, spacing: 10) {
                            Text("Use Criteria")
                                .font(.subheadline.weight(.semibold))
                                .foregroundStyle(.secondary)
                            VStack(alignment: .leading, spacing: 6) {
                                ForEach(item.agent.useCriteria, id: \.self) { criteria in
                                    HStack(alignment: .top, spacing: 6) {
                                        Text("\u{2022}")
                                            .foregroundStyle(.secondary)
                                        Text(criteria)
                                    }
                                }
                            }
                        }
                    }

                    technicalDetailsSection

                    metadataFooter
                }
                .frame(maxWidth: 800, alignment: .leading)
                .frame(maxWidth: .infinity, alignment: .center)
                .padding(.horizontal, 20)
                .padding(.top, 20)
                .padding(.bottom, 40)
            }
        }
        .background(Color.systemBackground.ignoresSafeArea())
        .navigationTitle(displayName)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .confirmationDialog(
            "Delete Agent Definition",
            isPresented: $showDeleteConfirmation,
            titleVisibility: .visible
        ) {
            Button("Delete", role: .destructive) {
                Task {
                    isDeleting = true
                    _ = await onDelete()
                    isDeleting = false
                }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This publishes a NIP-09 kind:5 deletion for this definition event.")
        }
        .sheet(isPresented: $showAssignmentSheet) {
            AgentDefinitionProjectAssignmentSheet(item: item) { result in
                onAssignmentResult(result)
            }
            .environment(coreManager)
        }
        .sheet(isPresented: $showTeamCreationSheet) {
            AgentDefinitionTeamCreationSheet(item: item) { result in
                onAssignmentResult(result)
            }
            .environment(coreManager)
        }
        .toolbar {
            if canDelete {
                ToolbarItem(placement: .destructiveAction) {
                    Button(role: .destructive) {
                        showDeleteConfirmation = true
                    } label: {
                        if isDeleting {
                            ProgressView()
                        } else {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                    .disabled(isDeleting)
                }
            }
        }
    }

    // MARK: - Hero

    @ViewBuilder
    private var heroSection: some View {
        if let imageURL = agentImageURL {
            AsyncImage(url: imageURL) { phase in
                switch phase {
                case let .success(image):
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                case .failure:
                    Rectangle()
                        .fill(Color.systemGray6)
                        .overlay {
                            Image(systemName: "photo")
                                .font(.largeTitle)
                                .foregroundStyle(.quaternary)
                        }
                case .empty:
                    Rectangle()
                        .fill(Color.systemGray6)
                        .overlay { ProgressView().controlSize(.small) }
                @unknown default:
                    EmptyView()
                }
            }
            .frame(maxWidth: .infinity)
            .frame(height: 420)
            .clipped()
            .overlay(alignment: .leading) {
                GeometryReader { geo in
                    heroOverlayContent
                        .padding(24)
                        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .bottomLeading)
                        .frame(width: geo.size.width * 0.5)
                }
                .background(
                        LinearGradient(
                            stops: [
                                .init(color: Color.systemBackground, location: 0),
                                .init(color: Color.systemBackground.opacity(0.85), location: 0.55),
                                .init(color: Color.systemBackground.opacity(0.3), location: 0.75),
                                .init(color: .clear, location: 1.0)
                            ],
                            startPoint: .leading,
                            endPoint: .trailing
                        )
                    )
            }
        } else {
            heroOverlayContent
                .padding(.horizontal, 20)
                .padding(.top, 24)
                .padding(.bottom, 8)
        }
    }

    private var heroOverlayContent: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(spacing: 8) {
                if !item.agent.role.isEmpty {
                    chip(text: item.agent.role, foreground: .primary, background: Color.white.opacity(0.15))
                }
                if let model = item.agent.model, !model.isEmpty {
                    chip(text: model, foreground: Color.agentBrand, background: Color.agentBrand.opacity(0.15))
                }
                if let version = item.agent.version, !version.isEmpty {
                    chip(text: "v\(version)", foreground: .secondary, background: Color.white.opacity(0.1))
                }
            }

            Text(displayName)
                .font(.largeTitle.weight(.bold))

            if !item.agent.description.isEmpty {
                Text(item.agent.description)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .lineLimit(3)
            }

            HStack(spacing: 12) {
                Button {
                    showAssignmentSheet = true
                } label: {
                    Label("Hire", systemImage: "play.fill")
                }
                .buttonStyle(.borderedProminent)

                Button {
                    showTeamCreationSheet = true
                } label: {
                    Label("Create Team", systemImage: "plus")
                }
                .buttonStyle(.bordered)
            }
            .controlSize(.large)
            .padding(.top, 4)
        }
    }

    private var metadataFooter: some View {
        VStack(alignment: .leading, spacing: 10) {
            Divider()

            HStack(spacing: 4) {
                Text("by")
                    .foregroundStyle(.tertiary)
                Button(action: openAuthorProfile) {
                    Text(item.authorDisplayName)
                        .foregroundStyle(Color.agentBrand)
                }
                .buttonStyle(.plain)
                .disabled(authorProfileURL == nil)

                Text("\u{00B7}")
                    .foregroundStyle(.tertiary)
                Text(formatDate(item.agent.createdAt))
            }
            .font(.caption)
            .foregroundStyle(.secondary)

            HStack(spacing: 4) {
                Text(shortHex(item.agent.id))
                    .textSelection(.enabled)
                if !item.agent.dTag.isEmpty {
                    Text("\u{00B7}")
                        .foregroundStyle(.tertiary)
                    Text(item.agent.dTag)
                        .textSelection(.enabled)
                }
            }
            .font(.caption2.monospaced())
            .foregroundStyle(.tertiary)
        }
    }

    // MARK: - Technical Details

    @ViewBuilder
    private var technicalDetailsSection: some View {
        let hasTools = !item.agent.tools.isEmpty
        let hasMcp = !item.agent.mcpServers.isEmpty
        let hasFiles = !item.agent.fileIds.isEmpty

        if hasTools || hasMcp || hasFiles {
            VStack(alignment: .leading, spacing: 16) {
                if hasTools {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Tools")
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.secondary)
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 120), spacing: 6)], alignment: .leading, spacing: 6) {
                            ForEach(item.agent.tools, id: \.self) { tool in
                                chip(text: tool, foreground: Color.skillBrand, background: Color.skillBrandBackground)
                            }
                        }
                    }
                }

                if hasMcp {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("MCP Servers")
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(item.agent.mcpServers, id: \.self) { serverId in
                                Text(serverId)
                                    .font(.caption.monospaced())
                                    .foregroundStyle(.secondary)
                                    .textSelection(.enabled)
                            }
                        }
                    }
                }

                if hasFiles {
                    VStack(alignment: .leading, spacing: 8) {
                        Text("Files")
                            .font(.subheadline.weight(.semibold))
                            .foregroundStyle(.secondary)
                        VStack(alignment: .leading, spacing: 4) {
                            ForEach(item.agent.fileIds, id: \.self) { fileId in
                                HStack(spacing: 8) {
                                    Image(systemName: "paperclip")
                                        .foregroundStyle(Color.skillBrand)
                                    Text(fileId)
                                        .font(.caption.monospaced())
                                        .foregroundStyle(.secondary)
                                        .textSelection(.enabled)
                                }
                            }
                        }
                    }
                }
            }
            .padding(16)
            .background(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(Color.systemGray6.opacity(0.5))
            )
        }
    }

    private func chip(text: String, foreground: Color, background: Color) -> some View {
        Text(text)
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(background, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
            .foregroundStyle(foreground)
    }

    private func formatDate(_ timestamp: UInt64) -> String {
        let date = Date(timeIntervalSince1970: TimeInterval(timestamp))
        return Self.dateFormatter.string(from: date)
    }

    private func shortHex(_ value: String) -> String {
        guard value.count > 16 else { return value }
        return "\(value.prefix(8))...\(value.suffix(8))"
    }

    private func openAuthorProfile() {
        guard let url = authorProfileURL else { return }
        openURL(url)
    }
}

private struct AgentDefinitionProjectAssignmentSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss

    let item: AgentDefinitionListItem
    let onFinished: (AgentAssignmentResult) -> Void

    @State private var selectedProjectIds: Set<String> = []
    @State private var searchText = ""
    @State private var isSaving = false

    private var displayName: String {
        item.agent.name.isEmpty ? "Unnamed Agent" : item.agent.name
    }

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
                if filteredProjects.isEmpty {
                    ContentUnavailableView(
                        "No Projects",
                        systemImage: "folder.badge.questionmark",
                        description: Text(searchText.isEmpty ? "No kind:31933 project events found." : "No projects match your search.")
                    )
                } else {
                    ForEach(filteredProjects, id: \.id) { project in
                        projectRow(project)
                            .listRowSeparator(.visible)
                    }
                }
            }
            #if os(iOS)
            .listStyle(.insetGrouped)
            #else
            .listStyle(.inset)
            #endif
            .searchable(text: $searchText, prompt: "Search projects")
            .navigationTitle("Add to Projects")
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
                    .disabled(isSaving)
                }

                ToolbarItem(placement: .primaryAction) {
                    Button {
                        Task { await assignToProjects() }
                    } label: {
                        if isSaving {
                            ProgressView()
                        } else {
                            Text("Add")
                                .fontWeight(.semibold)
                        }
                    }
                    .disabled(selectedProjectIds.isEmpty || isSaving)
                }
            }
        }
        #if os(iOS)
        .tenexModalPresentation(detents: [.medium, .large])
        #else
        .frame(minWidth: 500, idealWidth: 620, minHeight: 420, idealHeight: 560)
        #endif
    }

    private func projectRow(_ project: Project) -> some View {
        let alreadyAssigned = project.agentDefinitionIds.contains(item.agent.id)
        let isSelected = selectedProjectIds.contains(project.id)

        return Button {
            guard !alreadyAssigned else { return }
            if isSelected {
                selectedProjectIds.remove(project.id)
            } else {
                selectedProjectIds.insert(project.id)
            }
        } label: {
            HStack(spacing: 12) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(project.title)
                        .font(.body.weight(.medium))
                        .foregroundStyle(.primary)
                        .lineLimit(1)

                    Text(project.id)
                        .font(.caption2.monospaced())
                        .foregroundStyle(.secondary)
                        .lineLimit(1)

                    if alreadyAssigned {
                        Text("Already has this agent tag")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }

                Spacer(minLength: 0)

                if alreadyAssigned {
                    Label("Added", systemImage: "checkmark.seal.fill")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                        .font(.title3)
                        .foregroundStyle(isSelected ? Color.accentColor : .secondary)
                }
            }
            .contentShape(Rectangle())
        }
        .buttonStyle(.plain)
        .disabled(isSaving)
    }

    private func assignToProjects() async {
        guard !selectedProjectIds.isEmpty else { return }

        isSaving = true
        defer { isSaving = false }

        let targetProjects = sortedProjects.filter { selectedProjectIds.contains($0.id) }

        var updatedCount = 0
        var failedProjects: [String] = []

        for project in targetProjects {
            if project.agentDefinitionIds.contains(item.agent.id) {
                continue
            }

            var updatedAgentIds = project.agentDefinitionIds
            updatedAgentIds.append(item.agent.id)

            do {
                try await coreManager.safeCore.updateProject(
                    projectId: project.id,
                    title: project.title,
                    description: project.description ?? "",
                    repoUrl: project.repoUrl,
                    pictureUrl: project.pictureUrl,
                    agentDefinitionIds: updatedAgentIds,
                    mcpToolIds: project.mcpToolIds
                )
                updatedCount += 1
            } catch {
                failedProjects.append(project.title)
            }
        }

        if updatedCount > 0 {
            await coreManager.fetchData()
        }

        let result: AgentAssignmentResult
        if failedProjects.isEmpty {
            result = AgentAssignmentResult(
                title: "Agent Added",
                message: "Added \(displayName) to \(updatedCount) project\(updatedCount == 1 ? "" : "s")."
            )
        } else if updatedCount > 0 {
            result = AgentAssignmentResult(
                title: "Partially Added",
                message: "Added to \(updatedCount) project\(updatedCount == 1 ? "" : "s"). Failed for \(failedProjects.count) project\(failedProjects.count == 1 ? "" : "s")."
            )
        } else {
            result = AgentAssignmentResult(
                title: "Unable to Add",
                message: "No projects were updated for \(displayName)."
            )
        }

        onFinished(result)
        dismiss()
    }
}

private struct AgentDefinitionTeamCreationSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss

    let item: AgentDefinitionListItem
    let onFinished: (AgentAssignmentResult) -> Void

    @State private var teamName: String = ""
    @State private var teamDescription: String = ""
    @State private var isSaving = false
    @State private var errorMessage: String?
    @State private var showError = false

    private var displayName: String {
        let trimmed = item.agent.name.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? "Unnamed Agent" : trimmed
    }

    private var defaultTeamName: String {
        let base = item.agent.name.trimmingCharacters(in: .whitespacesAndNewlines)
        return base.isEmpty ? "New Team" : "\(base) Team"
    }

    private var trimmedTeamName: String {
        teamName.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var trimmedDescription: String {
        teamDescription.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    var body: some View {
        NavigationStack {
            Form {
                Section("Team") {
                    TextField("Name", text: $teamName)
#if os(iOS)
                        .textInputAutocapitalization(.words)
#endif

                    TextField("Description", text: $teamDescription, axis: .vertical)
                        .lineLimit(3...8)
                }

                Section("Includes Agent") {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(displayName)
                            .font(.body.weight(.medium))
                        if !item.agent.description.isEmpty {
                            Text(item.agent.description)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                        }
                        Text(item.agent.id)
                            .font(.caption2.monospaced())
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }
                }
            }
            #if os(iOS)
            .listStyle(.insetGrouped)
            #else
            .listStyle(.inset)
            #endif
            .navigationTitle("Create Team")
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
                    .disabled(isSaving)
                }

                ToolbarItem(placement: .primaryAction) {
                    Button {
                        Task { await createTeam() }
                    } label: {
                        if isSaving {
                            ProgressView()
                        } else {
                            Text("Create")
                                .fontWeight(.semibold)
                        }
                    }
                    .disabled(trimmedTeamName.isEmpty || isSaving)
                }
            }
            .alert("Unable to Create Team", isPresented: $showError) {
                Button("OK") {
                    errorMessage = nil
                }
            } message: {
                Text(errorMessage ?? "Unknown error")
            }
        }
        .onAppear {
            if teamName.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                teamName = defaultTeamName
            }
        }
        #if os(iOS)
        .tenexModalPresentation(detents: [.medium, .large])
        #else
        .frame(minWidth: 520, idealWidth: 620, minHeight: 380, idealHeight: 480)
        #endif
    }

    @MainActor
    private func createTeam() async {
        guard !trimmedTeamName.isEmpty else { return }

        isSaving = true
        defer { isSaving = false }

        do {
            try await coreManager.safeCore.createProject(
                name: trimmedTeamName,
                description: trimmedDescription,
                agentDefinitionIds: [item.agent.id],
                mcpToolIds: item.agent.mcpServers
            )

            await coreManager.fetchData()

            onFinished(
                AgentAssignmentResult(
                    title: "Team Created",
                    message: "Created \(trimmedTeamName) with \(displayName)."
                )
            )
            dismiss()
        } catch {
            errorMessage = error.localizedDescription
            showError = true
        }
    }
}

private struct AgentAssignmentResult: Identifiable {
    let id = UUID()
    let title: String
    let message: String
}
