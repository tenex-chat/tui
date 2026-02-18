import SwiftUI

private struct ProjectGeneralDraft: Equatable {
    var title: String = ""
    var description: String = ""
    var repoUrl: String = ""
    var pictureUrl: String = ""
}

struct ProjectSettingsView: View {
    @EnvironmentObject private var coreManager: TenexCoreManager
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    let projectId: String
    @Binding var selectedProjectId: String?

    @State private var generalDraft = ProjectGeneralDraft()
    @State private var baselineGeneralDraft = ProjectGeneralDraft()
    @State private var pendingAgentIds: [String] = []
    @State private var baselineAgentIds: [String] = []
    @State private var pendingToolIds: [String] = []
    @State private var baselineToolIds: [String] = []

    @State private var allAgents: [AgentInfo] = []
    @State private var allMcpTools: [McpToolInfo] = []
    @State private var hasLoadedSelectionData = false

    @State private var showAddAgentSheet = false
    @State private var showAddToolSheet = false
    @State private var showDeleteDialog = false

    @State private var agentSearch = ""
    @State private var toolSearch = ""

    @State private var isSavingGeneral = false
    @State private var isSavingAgents = false
    @State private var isSavingTools = false
    @State private var isBooting = false
    @State private var isDeleting = false

    @State private var errorMessage: String?
    @State private var showErrorAlert = false

    private var project: ProjectInfo? {
        coreManager.projects.first { $0.id == projectId }
    }

    private var generalHasChanges: Bool {
        generalDraft != baselineGeneralDraft
    }

    private var agentsHaveChanges: Bool {
        pendingAgentIds != baselineAgentIds
    }

    private var toolsHaveChanges: Bool {
        pendingToolIds != baselineToolIds
    }

    private var isProjectOnline: Bool {
        coreManager.projectOnlineStatus[projectId] ?? false
    }

    private var onlineAgentCount: Int {
        coreManager.onlineAgents[projectId]?.count ?? 0
    }

    private var filteredAvailableAgents: [AgentInfo] {
        let remaining = allAgents.filter { !pendingAgentIds.contains($0.id) }
        guard !agentSearch.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return remaining
        }

        let query = agentSearch.lowercased()
        return remaining.filter { agent in
            agent.name.lowercased().contains(query)
                || agent.role.lowercased().contains(query)
                || agent.description.lowercased().contains(query)
        }
    }

    private var filteredAvailableTools: [McpToolInfo] {
        let remaining = allMcpTools.filter { !pendingToolIds.contains($0.id) }
        guard !toolSearch.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty else {
            return remaining
        }

        let query = toolSearch.lowercased()
        return remaining.filter { tool in
            tool.name.lowercased().contains(query)
                || tool.command.lowercased().contains(query)
                || tool.description.lowercased().contains(query)
        }
    }

    var body: some View {
        Form {
            if let project {
                headerSection(project: project)
                generalSection(project: project)
                agentsSection(project: project)
                toolsSection(project: project)
                advancedSection
                dangerSection(project: project)
            } else {
                ContentUnavailableView(
                    "Project Not Found",
                    systemImage: "folder.badge.questionmark",
                    description: Text("This project may have been deleted or is no longer available.")
                )
            }
        }
        #if os(macOS)
        .formStyle(.grouped)
        #endif
        .navigationTitle("Project Settings")
        .onAppear {
            syncDraftsFromProject()
            Task { await loadSelectionDataIfNeeded() }
        }
        .onChange(of: project?.createdAt ?? 0) { _, _ in
            syncDraftsFromProject()
        }
        .sheet(isPresented: $showAddAgentSheet) {
            addAgentSheet
        }
        .sheet(isPresented: $showAddToolSheet) {
            addToolSheet
        }
        .confirmationDialog(
            "Delete Project",
            isPresented: $showDeleteDialog,
            titleVisibility: .visible
        ) {
            Button("Delete Project", role: .destructive) {
                Task { await deleteProject() }
            }
            Button("Cancel", role: .cancel) {}
        } message: {
            Text("This republishes kind 31933 with a deleted tag and hides the project everywhere.")
        }
        .alert("Project Settings Error", isPresented: $showErrorAlert) {
            Button("OK") { errorMessage = nil }
        } message: {
            Text(errorMessage ?? "An unknown error occurred.")
        }
    }

    @ViewBuilder
    private func headerSection(project: ProjectInfo) -> some View {
        Section {
            VStack(alignment: .leading, spacing: 12) {
                HStack(alignment: .center, spacing: 12) {
                    Image(systemName: "folder.fill")
                        .font(.title2)
                        .foregroundStyle(.secondary)
                    VStack(alignment: .leading, spacing: 4) {
                        Text(project.title)
                            .font(.headline)
                            .lineLimit(1)
                        Text(project.id)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .textSelection(.enabled)
                    }
                }

                HStack(spacing: 8) {
                    Label(isProjectOnline ? "Online" : "Offline", systemImage: isProjectOnline ? "checkmark.circle.fill" : "xmark.circle")
                        .font(.subheadline)
                        .foregroundStyle(isProjectOnline ? Color.green : .secondary)
                    if isProjectOnline {
                        Text("• \(onlineAgentCount) agent\(onlineAgentCount == 1 ? "" : "s") active")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                    }
                }

                if !isProjectOnline {
                    Button {
                        Task { await bootProject() }
                    } label: {
                        if isBooting {
                            ProgressView()
                                .frame(maxWidth: .infinity)
                        } else {
                            Label("Boot Project", systemImage: "power")
                                .frame(maxWidth: .infinity)
                        }
                    }
                    .disabled(isBooting || isDeleting)
                    .adaptiveGlassButtonStyle()
                }
            }
            .padding(.vertical, 6)
            .padding(.horizontal, 4)
            .modifier(AvailableGlassEffect(reduceTransparency: reduceTransparency))
            .clipShape(RoundedRectangle(cornerRadius: 14, style: .continuous))
        }
    }

    @ViewBuilder
    private func generalSection(project: ProjectInfo) -> some View {
        Section("General") {
            TextField("Title", text: $generalDraft.title)
                .textInputAutocapitalization(.words)

            TextField("Description", text: $generalDraft.description, axis: .vertical)
                .lineLimit(3...8)

            TextField("Repository URL", text: $generalDraft.repoUrl)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()

            TextField("Image URL", text: $generalDraft.pictureUrl)
                .keyboardType(.URL)
                .textInputAutocapitalization(.never)
                .autocorrectionDisabled()

            if let imageUrl = URL(string: generalDraft.pictureUrl.trimmingCharacters(in: .whitespacesAndNewlines)),
               !generalDraft.pictureUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                AsyncImage(url: imageUrl) { phase in
                    switch phase {
                    case .empty:
                        HStack {
                            Spacer()
                            ProgressView()
                            Spacer()
                        }
                        .frame(height: 140)
                    case .success(let image):
                        image
                            .resizable()
                            .scaledToFill()
                            .frame(height: 140)
                            .frame(maxWidth: .infinity)
                            .clipShape(RoundedRectangle(cornerRadius: 10, style: .continuous))
                    case .failure:
                        Label("Unable to load image preview", systemImage: "photo.badge.exclamationmark")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    @unknown default:
                        EmptyView()
                    }
                }
            }

            LabeledContent("Created") {
                Text(Self.dateFormatter.string(from: Date(timeIntervalSince1970: TimeInterval(project.createdAt))))
                    .foregroundStyle(.secondary)
            }

            HStack(spacing: 12) {
                Button("Reset") {
                    generalDraft = baselineGeneralDraft
                }
                .disabled(!generalHasChanges || isSavingGeneral)

                Button {
                    Task { await saveGeneral(project: project) }
                } label: {
                    if isSavingGeneral {
                        ProgressView()
                    } else {
                        Text("Save")
                    }
                }
                .disabled(
                    !generalHasChanges
                        || generalDraft.title.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty
                        || isSavingGeneral
                        || isDeleting
                )
                .adaptiveGlassButtonStyle()
            }
        }
    }

    @ViewBuilder
    private func agentsSection(project: ProjectInfo) -> some View {
        Section {
            if pendingAgentIds.isEmpty {
                ContentUnavailableView(
                    "No Agents Assigned",
                    systemImage: "person.2",
                    description: Text("Add at least one agent definition to this project.")
                )
            } else {
                ForEach(Array(pendingAgentIds.enumerated()), id: \.element) { index, agentId in
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(agentName(for: agentId))
                                .font(.body.weight(.medium))
                                .lineLimit(1)
                            if let description = agentDescription(for: agentId) {
                                Text(description)
                                    .font(.caption)
                                    .foregroundStyle(.secondary)
                                    .lineLimit(2)
                            } else {
                                Text(agentId)
                                    .font(.caption2.monospaced())
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }

                        Spacer()

                        if index == 0 {
                            Text("PM")
                                .font(.caption2.weight(.semibold))
                                .padding(.horizontal, 6)
                                .padding(.vertical, 3)
                                .background(.quaternary, in: Capsule())
                        }

                        Menu {
                            if index != 0 {
                                Button("Set as PM") {
                                    setProjectManager(agentId: agentId)
                                }
                            }
                            Button("Remove", role: .destructive) {
                                removeAgent(agentId: agentId)
                            }
                        } label: {
                            Image(systemName: "ellipsis.circle")
                                .font(.title3)
                        }
                    }
                }
            }

            Button {
                showAddAgentSheet = true
            } label: {
                Label("Add Agent", systemImage: "plus")
            }
            .adaptiveGlassButtonStyle()
            .disabled(isSavingAgents || isDeleting)

            HStack(spacing: 12) {
                Button("Cancel") {
                    pendingAgentIds = baselineAgentIds
                }
                .disabled(!agentsHaveChanges || isSavingAgents)

                Button {
                    Task { await saveAgents(project: project) }
                } label: {
                    if isSavingAgents {
                        ProgressView()
                    } else {
                        Text("Save")
                    }
                }
                .disabled(!agentsHaveChanges || isSavingAgents || isDeleting)
                .adaptiveGlassButtonStyle()
            }
        } header: {
            Text("Agents")
        } footer: {
            Text("The first agent is treated as the default PM.")
        }
    }

    @ViewBuilder
    private func toolsSection(project: ProjectInfo) -> some View {
        Section("Tools") {
            if pendingToolIds.isEmpty {
                ContentUnavailableView(
                    "No Tools Assigned",
                    systemImage: "wrench.and.screwdriver",
                    description: Text("Add MCP tools to make them available to project agents.")
                )
            } else {
                ForEach(pendingToolIds, id: \.self) { toolId in
                    HStack(spacing: 12) {
                        VStack(alignment: .leading, spacing: 4) {
                            Text(toolName(for: toolId))
                                .font(.body.weight(.medium))
                                .lineLimit(1)
                            let command = toolCommand(for: toolId)
                            if !command.isEmpty {
                                Text(command)
                                    .font(.caption.monospaced())
                                    .foregroundStyle(.secondary)
                                    .lineLimit(1)
                            }
                        }
                        Spacer()
                        Button(role: .destructive) {
                            removeTool(toolId: toolId)
                        } label: {
                            Image(systemName: "trash")
                        }
                        .buttonStyle(.borderless)
                    }
                }
            }

            Button {
                showAddToolSheet = true
            } label: {
                Label("Add Tool", systemImage: "plus")
            }
            .adaptiveGlassButtonStyle()
            .disabled(isSavingTools || isDeleting)

            HStack(spacing: 12) {
                Button("Cancel") {
                    pendingToolIds = baselineToolIds
                }
                .disabled(!toolsHaveChanges || isSavingTools)

                Button {
                    Task { await saveTools(project: project) }
                } label: {
                    if isSavingTools {
                        ProgressView()
                    } else {
                        Text("Save")
                    }
                }
                .disabled(!toolsHaveChanges || isSavingTools || isDeleting)
                .adaptiveGlassButtonStyle()
            }
        }
    }

    private var advancedSection: some View {
        Section("Advanced") {
            Label("Coming soon", systemImage: "clock")
                .foregroundStyle(.secondary)
        }
    }

    @ViewBuilder
    private func dangerSection(project: ProjectInfo) -> some View {
        Section("Danger Zone") {
            Button(role: .destructive) {
                showDeleteDialog = true
            } label: {
                if isDeleting {
                    ProgressView()
                        .frame(maxWidth: .infinity)
                } else {
                    Label("Delete Project", systemImage: "trash")
                        .frame(maxWidth: .infinity)
                }
            }
            .disabled(isDeleting || isSavingGeneral || isSavingAgents || isSavingTools)

            Text("This publishes a tombstone kind 31933 event with a deleted tag for \(project.title).")
                .font(.caption)
                .foregroundStyle(.secondary)
        }
    }

    private var addAgentSheet: some View {
        NavigationStack {
            List(filteredAvailableAgents, id: \.id) { agent in
                Button {
                    addAgent(agentId: agent.id)
                } label: {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(agent.name)
                            .font(.body.weight(.medium))
                            .foregroundStyle(.primary)
                        Text(agent.role)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                        if !agent.description.isEmpty {
                            Text(agent.description)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .buttonStyle(.plain)
            }
            .searchable(text: $agentSearch, prompt: "Search agents")
            .navigationTitle("Add Agents")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        showAddAgentSheet = false
                        agentSearch = ""
                    }
                }
            }
        }
        .tenexModalPresentation(detents: [.medium, .large])
    }

    private var addToolSheet: some View {
        NavigationStack {
            List(filteredAvailableTools, id: \.id) { tool in
                Button {
                    addTool(toolId: tool.id)
                } label: {
                    VStack(alignment: .leading, spacing: 4) {
                        Text(tool.name)
                            .font(.body.weight(.medium))
                            .foregroundStyle(.primary)
                        if !tool.command.isEmpty {
                            Text(tool.command)
                                .font(.caption.monospaced())
                                .foregroundStyle(.secondary)
                        }
                        if !tool.description.isEmpty {
                            Text(tool.description)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)
                }
                .buttonStyle(.plain)
            }
            .searchable(text: $toolSearch, prompt: "Search tools")
            .navigationTitle("Add Tools")
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Done") {
                        showAddToolSheet = false
                        toolSearch = ""
                    }
                }
            }
        }
        .tenexModalPresentation(detents: [.medium, .large])
    }

    private func syncDraftsFromProject() {
        guard let project else { return }

        let syncedGeneral = ProjectGeneralDraft(
            title: project.title,
            description: project.description ?? "",
            repoUrl: project.repoUrl ?? "",
            pictureUrl: project.pictureUrl ?? ""
        )
        generalDraft = syncedGeneral
        baselineGeneralDraft = syncedGeneral

        pendingAgentIds = project.agentIds
        baselineAgentIds = project.agentIds

        pendingToolIds = project.mcpToolIds
        baselineToolIds = project.mcpToolIds
    }

    private func loadSelectionDataIfNeeded() async {
        guard !hasLoadedSelectionData else { return }

        do {
            async let fetchedAgents = coreManager.safeCore.getAllAgents()
            async let fetchedTools = coreManager.safeCore.getAllMcpTools()
            let (agents, tools) = try await (fetchedAgents, fetchedTools)

            await MainActor.run {
                allAgents = agents.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
                allMcpTools = tools.sorted { $0.name.localizedCaseInsensitiveCompare($1.name) == .orderedAscending }
                hasLoadedSelectionData = true
            }
        } catch {
            presentError(error)
        }
    }

    private func bootProject() async {
        guard let project else { return }
        isBooting = true
        defer { isBooting = false }

        do {
            try await coreManager.safeCore.bootProject(projectId: project.id)
        } catch {
            presentError(error)
        }
    }

    private func saveGeneral(project: ProjectInfo) async {
        isSavingGeneral = true
        defer { isSavingGeneral = false }

        do {
            try await coreManager.safeCore.updateProject(
                projectId: project.id,
                title: generalDraft.title.trimmingCharacters(in: .whitespacesAndNewlines),
                description: generalDraft.description,
                repoUrl: normalizedOptional(generalDraft.repoUrl),
                pictureUrl: normalizedOptional(generalDraft.pictureUrl),
                agentIds: project.agentIds,
                mcpToolIds: project.mcpToolIds
            )
            baselineGeneralDraft = generalDraft
        } catch {
            presentError(error)
        }
    }

    private func saveAgents(project: ProjectInfo) async {
        isSavingAgents = true
        defer { isSavingAgents = false }

        do {
            try await coreManager.safeCore.updateProject(
                projectId: project.id,
                title: project.title,
                description: project.description ?? "",
                repoUrl: project.repoUrl,
                pictureUrl: project.pictureUrl,
                agentIds: pendingAgentIds,
                mcpToolIds: project.mcpToolIds
            )
            baselineAgentIds = pendingAgentIds
        } catch {
            presentError(error)
        }
    }

    private func saveTools(project: ProjectInfo) async {
        isSavingTools = true
        defer { isSavingTools = false }

        do {
            try await coreManager.safeCore.updateProject(
                projectId: project.id,
                title: project.title,
                description: project.description ?? "",
                repoUrl: project.repoUrl,
                pictureUrl: project.pictureUrl,
                agentIds: project.agentIds,
                mcpToolIds: pendingToolIds
            )
            baselineToolIds = pendingToolIds
        } catch {
            presentError(error)
        }
    }

    private func deleteProject() async {
        guard let project else { return }
        isDeleting = true
        defer { isDeleting = false }

        do {
            try await coreManager.safeCore.deleteProject(projectId: project.id)
            selectedProjectId = nil
        } catch {
            presentError(error)
        }
    }

    private func addAgent(agentId: String) {
        guard !pendingAgentIds.contains(agentId) else { return }
        pendingAgentIds.append(agentId)
    }

    private func removeAgent(agentId: String) {
        pendingAgentIds.removeAll { $0 == agentId }
    }

    private func setProjectManager(agentId: String) {
        guard let index = pendingAgentIds.firstIndex(of: agentId), index > 0 else { return }
        pendingAgentIds.remove(at: index)
        pendingAgentIds.insert(agentId, at: 0)
    }

    private func addTool(toolId: String) {
        guard !pendingToolIds.contains(toolId) else { return }
        pendingToolIds.append(toolId)
    }

    private func removeTool(toolId: String) {
        pendingToolIds.removeAll { $0 == toolId }
    }

    private func agentName(for id: String) -> String {
        allAgents.first(where: { $0.id == id })?.name ?? "Unknown Agent"
    }

    private func agentDescription(for id: String) -> String? {
        allAgents.first(where: { $0.id == id })?.description
    }

    private func toolName(for id: String) -> String {
        allMcpTools.first(where: { $0.id == id })?.name ?? "Unknown Tool"
    }

    private func toolCommand(for id: String) -> String {
        allMcpTools.first(where: { $0.id == id })?.command ?? ""
    }

    private func normalizedOptional(_ value: String) -> String? {
        let trimmed = value.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private func presentError(_ error: Error) {
        errorMessage = error.localizedDescription
        showErrorAlert = true
    }

    private static let dateFormatter: DateFormatter = {
        let formatter = DateFormatter()
        formatter.dateStyle = .medium
        formatter.timeStyle = .short
        return formatter
    }()
}
