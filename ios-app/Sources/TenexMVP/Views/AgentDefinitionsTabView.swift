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
    @EnvironmentObject private var coreManager: TenexCoreManager

    let layoutMode: AgentDefinitionsLayoutMode
    private let selectedAgentBindingOverride: Binding<AgentDefinition?>?

    @StateObject private var viewModel = AgentDefinitionsViewModel()
    @State private var selectedAgentState: AgentDefinition?
    @State private var hasConfiguredViewModel = false
    @State private var navigationPath: [AgentDefinitionListItem] = []
    @State private var assignmentTarget: AgentDefinitionListItem?
    @State private var assignmentResult: AgentAssignmentResult?
    @State private var showNewAgentDefinitionModal = false

    init(
        layoutMode: AgentDefinitionsLayoutMode = .adaptive,
        selectedAgent: Binding<AgentDefinition?>? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedAgentBindingOverride = selectedAgent
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
            await viewModel.loadIfNeeded()
        }
        .onReceive(coreManager.diagnosticsVersionPublisher) { _ in
            Task { await viewModel.refresh() }
        }
        .sheet(item: $assignmentTarget) { item in
            AgentDefinitionProjectAssignmentSheet(item: item) { result in
                assignmentResult = result
            }
            .environmentObject(coreManager)
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
            .environmentObject(coreManager)
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
                        onAssign: {
                            presentAssignmentSheet(for: item)
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
                    ToolbarItem(placement: .topBarTrailing) {
                        if viewModel.isLoading {
                            ProgressView()
                                .controlSize(.small)
                        }
                    }

                    ToolbarItem(placement: .topBarTrailing) {
                        Button {
                            showNewAgentDefinitionModal = true
                        } label: {
                            Label("New", systemImage: "plus")
                        }
                    }

                    ToolbarItem(placement: .topBarTrailing) {
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
            await viewModel.refresh()
        }
        #endif
    }

    private func cardSection(
        title: String,
        subtitle: String,
        items: [AgentDefinitionListItem]
    ) -> some View {
        VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title)
                    .font(.headline)
                Text(subtitle)
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            LazyVGrid(columns: [GridItem(.adaptive(minimum: 290), spacing: 14)], spacing: 14) {
                ForEach(items) { item in
                    Button {
                        selectedAgentBinding.wrappedValue = item.agent
                        navigationPath.append(item)
                    } label: {
                        AgentDefinitionVisualCard(item: item)
                    }
                    .buttonStyle(.plain)
                    .contextMenu {
                        Button {
                            presentAssignmentSheet(for: item)
                        } label: {
                            Label("Add to Projects", systemImage: "plus")
                        }
                    }
                }
            }
        }
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
    @Environment(\.openURL) private var openURL

    let item: AgentDefinitionListItem
    let canDelete: Bool
    let onAssign: () -> Void
    let onDelete: () async -> Bool

    @State private var showDeleteConfirmation = false
    @State private var isDeleting = false

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

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                header
                metadataSection
                descriptionSection
                instructionsSection

                if !item.agent.useCriteria.isEmpty {
                    useCriteriaSection
                }
                if !item.agent.tools.isEmpty {
                    toolsSection
                }
                if !item.agent.mcpServers.isEmpty {
                    mcpServersSection
                }
                if !item.agent.fileIds.isEmpty {
                    fileReferencesSection
                }
            }
            .frame(maxWidth: 800, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .center)
            .padding(.horizontal, 20)
            .padding(.vertical, 24)
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
    }

    private var header: some View {
        VStack(alignment: .leading, spacing: 12) {
            HStack(alignment: .top, spacing: 12) {
                VStack(alignment: .leading, spacing: 8) {
                    Text(displayName)
                        .font(.title2.weight(.bold))

                    HStack(spacing: 8) {
                        if !item.agent.role.isEmpty {
                            chip(text: item.agent.role, foreground: .primary, background: Color.systemGray6)
                        }
                        if let model = item.agent.model, !model.isEmpty {
                            chip(text: model, foreground: Color.agentBrand, background: Color.agentBrand.opacity(0.15))
                        }
                        if let version = item.agent.version, !version.isEmpty {
                            chip(text: "v\(version)", foreground: .secondary, background: Color.systemGray6)
                        }
                    }
                }

                Spacer(minLength: 0)

                Button(action: onAssign) {
                    Label("Add to Projects", systemImage: "plus")
                }
                    .buttonStyle(.bordered)
                    .accessibilityLabel("Add to Projects")

                if canDelete {
                    Button(role: .destructive) {
                        showDeleteConfirmation = true
                    } label: {
                        if isDeleting {
                            ProgressView()
                        } else {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                    .buttonStyle(.bordered)
                    .disabled(isDeleting)
                }
            }

            Divider()
        }
    }

    private var metadataSection: some View {
        section(title: "Metadata") {
            VStack(alignment: .leading, spacing: 8) {
                HStack(alignment: .center, spacing: 10) {
                    Text("Author")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .frame(width: 110, alignment: .leading)

                    Button(action: openAuthorProfile) {
                        Text(item.authorDisplayName)
                            .font(.caption)
                            .foregroundStyle(Color.agentBrand)
                            .underline()
                            .lineLimit(1)
                    }
                    .buttonStyle(.plain)
                    .disabled(authorProfileURL == nil)

                    Spacer(minLength: 0)
                }
                metadataRow(title: "Created", value: formatDate(item.agent.createdAt))
                metadataRow(title: "Event ID", value: shortHex(item.agent.id))

                if !item.agent.dTag.isEmpty {
                    metadataRow(title: "d-tag", value: item.agent.dTag)
                }
            }
        }
    }

    private var descriptionSection: some View {
        section(title: "Description") {
            Text(item.agent.description.isEmpty ? "No description provided" : item.agent.description)
                .font(.body)
                .foregroundStyle(item.agent.description.isEmpty ? .secondary : .primary)
        }
    }

    private var instructionsSection: some View {
        section(title: "Instructions") {
            if item.agent.instructions.isEmpty {
                Text("No instructions provided")
                    .foregroundStyle(.secondary)
            } else {
                MarkdownView(content: item.agent.instructions)
            }
        }
    }

    private var useCriteriaSection: some View {
        section(title: "Use Criteria") {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(item.agent.useCriteria, id: \.self) { criteria in
                    HStack(alignment: .top, spacing: 6) {
                        Text("•")
                            .foregroundStyle(.secondary)
                        Text(criteria)
                            .foregroundStyle(.primary)
                    }
                }
            }
        }
    }

    private var toolsSection: some View {
        section(title: "Tools") {
            LazyVGrid(columns: [GridItem(.adaptive(minimum: 120), spacing: 6)], alignment: .leading, spacing: 6) {
                ForEach(item.agent.tools, id: \.self) { tool in
                    chip(text: tool, foreground: Color.skillBrand, background: Color.skillBrandBackground)
                }
            }
        }
    }

    private var mcpServersSection: some View {
        section(title: "MCP Servers") {
            VStack(alignment: .leading, spacing: 6) {
                ForEach(item.agent.mcpServers, id: \.self) { serverId in
                    Text(serverId)
                        .font(.caption.monospaced())
                        .foregroundStyle(.secondary)
                        .textSelection(.enabled)
                }
            }
        }
    }

    private var fileReferencesSection: some View {
        section(title: "File References (NIP-94 kind:1063)") {
            VStack(alignment: .leading, spacing: 6) {
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

    private func section<Content: View>(title: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 10) {
            Text(title)
                .font(.headline)
            content()
        }
        .padding(.bottom, 2)
    }

    private func chip(text: String, foreground: Color, background: Color) -> some View {
        Text(text)
            .font(.caption)
            .padding(.horizontal, 8)
            .padding(.vertical, 3)
            .background(background, in: RoundedRectangle(cornerRadius: 6, style: .continuous))
            .foregroundStyle(foreground)
    }

    private func metadataRow(title: String, value: String) -> some View {
        HStack(alignment: .firstTextBaseline, spacing: 10) {
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 110, alignment: .leading)
            Text(value)
                .font(.caption)
                .foregroundStyle(.primary)
                .textSelection(.enabled)
            Spacer(minLength: 0)
        }
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
    @EnvironmentObject private var coreManager: TenexCoreManager
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

private struct AgentAssignmentResult: Identifiable {
    let id = UUID()
    let title: String
    let message: String
}
