import SwiftUI

private enum CreateProjectStep: Int, CaseIterable, Identifiable {
    case projectInfo
    case agents
    case configuration
    case review

    var id: Int { rawValue }

    var title: String {
        switch self {
        case .projectInfo: return "Project"
        case .agents: return "Agents"
        case .configuration: return "Tools"
        case .review: return "Review"
        }
    }

    var subtitle: String {
        switch self {
        case .projectInfo: return "Name and describe your project"
        case .agents: return "Assign agents to your project"
        case .configuration: return "Connect repos and tools"
        case .review: return "Final check before launch"
        }
    }

    var symbol: String {
        switch self {
        case .projectInfo: return "folder.badge.plus"
        case .agents: return "person.3.sequence"
        case .configuration: return "wrench.and.screwdriver"
        case .review: return "checkmark.seal"
        }
    }
}

struct CreateProjectView: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    let onComplete: () -> Void

    @State private var step: CreateProjectStep = .projectInfo

    // Step 1
    @State private var projectName = ""
    @State private var projectDescription = ""

    // Step 2 — agent selection
    @StateObject private var agentDefViewModel = AgentDefinitionsViewModel()
    @StateObject private var teamsViewModel = TeamsViewModel()
    @State private var selectedAgentIds: Set<String> = []
    @State private var selectedTeamIds: Set<String> = []

    // Step 3 — tools
    @State private var repoUrl = ""
    @State private var allMcpTools: [McpTool] = []
    @State private var toolGroups: [ToolGroup] = []
    @State private var selectedToolIds: Set<String> = []

    @State private var isCreating = false
    @State private var errorMessage: String?

    private var trimmedName: String { projectName.trimmingCharacters(in: .whitespacesAndNewlines) }
    private var trimmedDescription: String { projectDescription.trimmingCharacters(in: .whitespacesAndNewlines) }

    // All agent IDs to submit: individually selected + all from selected teams
    private var allSelectedAgentIds: Set<String> {
        var ids = selectedAgentIds
        for teamId in selectedTeamIds {
            if let team = teamsViewModel.featuredTeams.first(where: { $0.id == teamId }) {
                ids.formUnion(team.team.agentDefinitionIds)
            }
        }
        return ids
    }

    private var canProceed: Bool {
        switch step {
        case .projectInfo: return !trimmedName.isEmpty
        case .agents, .configuration, .review: return true
        }
    }

    var body: some View {
        Group {
            #if os(macOS)
            macOSLayout
            #else
            iOSLayout
            #endif
        }
        .task {
            agentDefViewModel.configure(with: coreManager)
            teamsViewModel.configure(with: coreManager)
            async let agentLoad: Void = agentDefViewModel.loadIfNeeded()
            async let teamLoad: Void = teamsViewModel.loadIfNeeded()
            async let toolLoad: Void = loadTools()
            _ = await (agentLoad, teamLoad, toolLoad)
        }
        .alert(
            "Unable to Create Project",
            isPresented: Binding(get: { errorMessage != nil }, set: { if !$0 { errorMessage = nil } })
        ) {
            Button("OK", role: .cancel) { errorMessage = nil }
        } message: {
            Text(errorMessage ?? "Unknown error")
        }
    }

    // MARK: - macOS Layout

    private var macOSLayout: some View {
        HStack(spacing: 0) {
            wizardSidebar
                .frame(width: 210)

            Divider()

            VStack(spacing: 0) {
                ScrollView {
                    stepContent
                        .padding(24)
                        .frame(maxWidth: stepContentMaxWidth, alignment: .leading)
                        .frame(maxWidth: .infinity, alignment: .center)
                }

                Divider()

                actionBar
                    .padding(.horizontal, 16)
                    .padding(.vertical, 10)
            }
        }
        .background(Color.systemGroupedBackground)
    }

    private var stepContentMaxWidth: CGFloat {
        switch step {
        case .projectInfo: return 560
        case .agents: return 960
        case .configuration: return 640
        case .review: return 560
        }
    }

    // MARK: - Sidebar

    private var wizardSidebar: some View {
        VStack(alignment: .leading, spacing: 0) {
            VStack(alignment: .leading, spacing: 2) {
                ForEach(CreateProjectStep.allCases) { candidate in
                    sidebarRow(candidate)
                }
            }
            .padding(12)

            Spacer()

            if !trimmedName.isEmpty || !allSelectedAgentIds.isEmpty || !selectedToolIds.isEmpty {
                Divider()
                sidebarSummary.padding(12)
            }
        }
        .background(Color.systemBackground.opacity(0.4))
    }

    private func sidebarRow(_ candidate: CreateProjectStep) -> some View {
        let isCompleted = candidate.rawValue < step.rawValue
        let isActive = candidate == step
        let isReachable = candidate.rawValue <= step.rawValue

        return Button {
            guard isReachable else { return }
            step = candidate
        } label: {
            HStack(spacing: 10) {
                Group {
                    if isCompleted {
                        Image(systemName: "checkmark.circle.fill").foregroundStyle(.green)
                    } else if isActive {
                        Image(systemName: candidate.symbol).foregroundStyle(Color.projectBrand)
                    } else {
                        Image(systemName: candidate.symbol).foregroundStyle(Color.gray.opacity(0.4))
                    }
                }
                .font(.body)
                .frame(width: 22)

                VStack(alignment: .leading, spacing: 2) {
                    Text(candidate.title)
                        .font(.subheadline.weight(isActive ? .semibold : .regular))
                        .foregroundStyle(isActive ? Color.primary : (isReachable ? Color.secondary : Color.gray.opacity(0.4)))
                    Text(candidate.subtitle)
                        .font(.caption2)
                        .foregroundStyle(isActive ? Color.secondary : Color.gray.opacity(0.3))
                        .lineLimit(2)
                }
                Spacer(minLength: 0)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 8)
            .background(RoundedRectangle(cornerRadius: 8, style: .continuous)
                .fill(isActive ? Color.projectBrand.opacity(0.12) : .clear))
        }
        .buttonStyle(.plain)
        .disabled(isCreating)
    }

    private var sidebarSummary: some View {
        VStack(alignment: .leading, spacing: 6) {
            Text("Summary")
                .font(.caption.weight(.semibold))
                .foregroundStyle(.secondary)
            if !trimmedName.isEmpty {
                summaryRow(label: "Name", value: trimmedName)
            }
            let agentCount = allSelectedAgentIds.count
            if agentCount > 0 {
                summaryRow(label: "Agents", value: "\(agentCount)")
            }
            if !selectedTeamIds.isEmpty {
                summaryRow(label: "Teams", value: "\(selectedTeamIds.count)")
            }
            if !selectedToolIds.isEmpty {
                summaryRow(label: "Tools", value: "\(selectedToolIds.count)")
            }
        }
    }

    private func summaryRow(label: String, value: String) -> some View {
        HStack {
            Text(label).font(.caption).foregroundStyle(.secondary)
            Spacer()
            Text(value).font(.caption.weight(.medium)).lineLimit(1)
        }
    }

    // MARK: - iOS Layout

    private var iOSLayout: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 14) {
                iOSStepRail
                stepContent
            }
            .padding(.horizontal, 18)
            .padding(.top, 16)
            .padding(.bottom, 84)
        }
        .background(Color.systemGroupedBackground.ignoresSafeArea())
        .navigationTitle("New Project")
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .cancellationAction) {
                Button("Cancel") { onComplete() }.disabled(isCreating)
            }
        }
        .safeAreaInset(edge: .bottom) {
            actionBar
                .padding(.horizontal, 16)
                .padding(.vertical, 10)
                .background {
                    RoundedRectangle(cornerRadius: 16, style: .continuous)
                        .fill(Color.systemBackground.opacity(reduceTransparency ? 1 : 0.6))
                        .modifier(AvailableGlassEffect(reduceTransparency: reduceTransparency))
                }
                .padding(.horizontal, 12)
                .padding(.bottom, 8)
        }
    }

    private var iOSStepRail: some View {
        HStack(spacing: 8) {
            ForEach(CreateProjectStep.allCases) { candidate in
                let isCompleted = candidate.rawValue < step.rawValue
                let isActive = candidate == step
                let isReachable = candidate.rawValue <= step.rawValue
                Button {
                    guard isReachable else { return }
                    step = candidate
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: isCompleted ? "checkmark" : candidate.symbol)
                            .font(isCompleted ? .caption2.weight(.bold) : .caption.weight(.semibold))
                        Text(candidate.title).font(.caption.weight(.semibold))
                    }
                    .foregroundStyle(isActive ? Color.projectBrand : isCompleted ? Color.green : isReachable ? Color.secondary : Color.gray.opacity(0.3))
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .frame(maxWidth: .infinity)
                    .background(RoundedRectangle(cornerRadius: 10, style: .continuous)
                        .fill(isActive ? Color.projectBrand.opacity(0.15) : isCompleted ? Color.green.opacity(0.08) : Color.systemGray6.opacity(0.6)))
                }
                .buttonStyle(.plain)
                .disabled(isCreating)
            }
        }
    }

    // MARK: - Step Content

    @ViewBuilder
    private var stepContent: some View {
        switch step {
        case .projectInfo: projectInfoStep
        case .agents: agentsStep
        case .configuration: configurationStep
        case .review: reviewStep
        }
    }

    // MARK: Step 1: Project Info

    private var projectInfoStep: some View {
        VStack(alignment: .leading, spacing: 16) {
            sectionHeader("Project Details", subtitle: "Give your project a clear name so collaborators can find it.")
            VStack(alignment: .leading, spacing: 12) {
                fieldGroup("Name", help: "Required") {
                    TextField("My First Project", text: $projectName)
                        .textFieldStyle(.plain)
                        .padding(10)
                        .background(Color.systemGray6.opacity(0.8), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
                fieldGroup("Description", help: "Optional") {
                    TextField("What this project is about", text: $projectDescription, axis: .vertical)
                        .lineLimit(3...5)
                        .textFieldStyle(.plain)
                        .padding(10)
                        .background(Color.systemGray6.opacity(0.8), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                }
            }
        }
    }

    // MARK: Step 2: Agents (teams + definitions using same components as AgentDefinitionsTabView)

    private var agentsStep: some View {
        VStack(alignment: .leading, spacing: 24) {
            // Featured Teams
            VStack(alignment: .leading, spacing: 14) {
                Text("Featured Teams")
                    .font(.title3.weight(.bold))
                Text("Select a team to add all its agents at once.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)

                if teamsViewModel.isLoading, teamsViewModel.featuredTeams.isEmpty {
                    ProgressView().frame(maxWidth: .infinity, alignment: .leading).padding(.vertical, 10)
                } else if teamsViewModel.featuredTeams.isEmpty {
                    Text("No featured teams yet.")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .padding(.vertical, 8)
                } else {
                    ScrollView(.horizontal, showsIndicators: false) {
                        HStack(spacing: 14) {
                            ForEach(teamsViewModel.featuredTeams.prefix(10)) { item in
                                let isSelected = selectedTeamIds.contains(item.id)
                                Button { toggleTeam(item) } label: {
                                    TeamFeaturedCard(item: item)
                                        .overlay(alignment: .topTrailing) {
                                            if isSelected {
                                                Image(systemName: "checkmark.circle.fill")
                                                    .font(.title3)
                                                    .foregroundStyle(.white, Color.projectBrand)
                                                    .padding(10)
                                                    .shadow(radius: 2)
                                            }
                                        }
                                        .overlay {
                                            if isSelected {
                                                RoundedRectangle(cornerRadius: 16, style: .continuous)
                                                    .stroke(Color.projectBrand, lineWidth: 3)
                                            }
                                        }
                                }
                                .buttonStyle(.plain)
                            }
                        }
                        .padding(.vertical, 2)
                    }
                }
            }

            // Agent Definitions
            VStack(alignment: .leading, spacing: 16) {
                HStack(spacing: 12) {
                    TextField("Search agents…", text: $agentDefViewModel.searchText)
                        .textFieldStyle(.plain)
                        .padding(10)
                        .background(Color.systemGray6.opacity(0.8), in: RoundedRectangle(cornerRadius: 8, style: .continuous))

                    if agentDefViewModel.isLoading {
                        ProgressView().controlSize(.small)
                    }
                }

                if agentDefViewModel.filteredMine.isEmpty && agentDefViewModel.filteredCommunity.isEmpty && !agentDefViewModel.isLoading {
                    Text(agentDefViewModel.searchText.isEmpty ? "No agent definitions found." : "No agents match your search.")
                        .foregroundStyle(.secondary)
                        .frame(maxWidth: .infinity, alignment: .center)
                        .padding(.vertical, 20)
                } else {
                    if !agentDefViewModel.filteredMine.isEmpty {
                        agentCardSection(title: "Mine", subtitle: "Definitions you authored", items: agentDefViewModel.filteredMine)
                    }
                    if !agentDefViewModel.filteredCommunity.isEmpty {
                        agentCardSection(title: "Community", subtitle: "Definitions from other authors", items: agentDefViewModel.filteredCommunity)
                    }
                }
            }
        }
    }

    private func agentCardSection(title: String, subtitle: String, items: [AgentDefinitionListItem]) -> some View {
        let imageItems = items.filter { hasDefinitionImage($0) }
        let compactItems = items.filter { !hasDefinitionImage($0) }

        return VStack(alignment: .leading, spacing: 12) {
            VStack(alignment: .leading, spacing: 2) {
                Text(title).font(.headline)
                Text(subtitle).font(.caption).foregroundStyle(.secondary)
            }

            if !imageItems.isEmpty {
                LazyVGrid(columns: imageCardColumns, alignment: .leading, spacing: 14) {
                    ForEach(imageItems) { item in
                        selectableAgentCard(item, style: .visual)
                    }
                }
            }

            if !compactItems.isEmpty {
                LazyVGrid(columns: compactCardColumns, alignment: .leading, spacing: 12) {
                    ForEach(compactItems) { item in
                        selectableAgentCard(item, style: .compact)
                    }
                }
            }
        }
    }

    private enum AgentCardStyle { case visual, compact }

    private var imageCardColumns: [GridItem] {
        #if os(macOS)
        Array(repeating: GridItem(.flexible(minimum: 0, maximum: .infinity), spacing: 14, alignment: .top), count: 4)
        #else
        [GridItem(.adaptive(minimum: AgentDefinitionVisualCard.gridMinimumWidth, maximum: AgentDefinitionVisualCard.gridMaximumWidth), spacing: 14, alignment: .top)]
        #endif
    }

    private var compactCardColumns: [GridItem] {
        #if os(macOS)
        Array(repeating: GridItem(.flexible(minimum: 0, maximum: .infinity), spacing: 14, alignment: .top), count: 2)
        #else
        [GridItem(.adaptive(minimum: 320, maximum: .infinity), spacing: 12, alignment: .top)]
        #endif
    }

    @ViewBuilder
    private func selectableAgentCard(_ item: AgentDefinitionListItem, style: AgentCardStyle) -> some View {
        let isSelected = selectedAgentIds.contains(item.id)
        Button { toggleAgent(item) } label: {
            Group {
                switch style {
                case .visual: AgentDefinitionVisualCard(item: item)
                case .compact: AgentDefinitionCompactCard(item: item)
                }
            }
            .overlay(alignment: .topTrailing) {
                if isSelected {
                    Image(systemName: "checkmark.circle.fill")
                        .font(.title3)
                        .foregroundStyle(.white, Color.projectBrand)
                        .padding(10)
                        .shadow(radius: 2)
                }
            }
            .overlay {
                if isSelected {
                    RoundedRectangle(cornerRadius: style == .visual ? 18 : 12, style: .continuous)
                        .stroke(Color.projectBrand, lineWidth: 3)
                }
            }
        }
        .buttonStyle(.plain)
    }

    private func hasDefinitionImage(_ item: AgentDefinitionListItem) -> Bool {
        guard let raw = item.agent.picture?.trimmingCharacters(in: .whitespacesAndNewlines), !raw.isEmpty else { return false }
        return URL(string: raw) != nil
    }

    private func toggleAgent(_ item: AgentDefinitionListItem) {
        if selectedAgentIds.contains(item.id) {
            selectedAgentIds.remove(item.id)
        } else {
            selectedAgentIds.insert(item.id)
        }
    }

    private func toggleTeam(_ item: TeamListItem) {
        if selectedTeamIds.contains(item.id) {
            selectedTeamIds.remove(item.id)
        } else {
            selectedTeamIds.insert(item.id)
        }
    }

    // MARK: Step 3: Configuration

    private var configurationStep: some View {
        VStack(alignment: .leading, spacing: 20) {
            VStack(alignment: .leading, spacing: 12) {
                sectionHeader("Git Repository", subtitle: "Optional — link a repository for context.")
                TextField("https://github.com/org/repo", text: $repoUrl)
                    .textFieldStyle(.plain)
                    .padding(10)
                    .background(Color.systemGray6.opacity(0.8), in: RoundedRectangle(cornerRadius: 8, style: .continuous))
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
                    #endif
            }

            VStack(alignment: .leading, spacing: 12) {
                sectionHeader("MCP Tools", subtitle: "Select tools to make available in this project.")
                if toolGroups.isEmpty {
                    Text("No MCP tools available.")
                        .foregroundStyle(.secondary)
                        .padding(.vertical, 8)
                } else {
                    VStack(alignment: .leading, spacing: 4) {
                        ForEach(Array(toolGroups.enumerated()), id: \.element.id) { index, group in
                            toolGroupRow(group: group, index: index)
                        }
                    }
                }
                Text("\(selectedToolIds.count) tools selected")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
        }
    }

    private func toolGroupRow(group: ToolGroup, index: Int) -> some View {
        Group {
            if group.tools.count == 1 {
                singleToolRow(tool: group.tools[0])
            } else {
                DisclosureGroup(
                    isExpanded: Binding(
                        get: { toolGroups[index].isExpanded },
                        set: { toolGroups[index].isExpanded = $0 }
                    )
                ) {
                    ForEach(group.tools, id: \.self) { tool in
                        singleToolRow(tool: tool).padding(.leading, 16)
                    }
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: groupSelectionIcon(group))
                            .foregroundStyle(group.isFullySelected(selectedToolIds) ? Color.projectBrand : .secondary)
                        Text(group.name).font(.subheadline)
                        Spacer()
                        Text("\(group.tools.filter { selectedToolIds.contains($0) }.count)/\(group.tools.count)")
                            .font(.caption).foregroundStyle(.secondary)
                    }
                    .contentShape(Rectangle())
                    .onTapGesture { toggleGroup(group) }
                }
            }
        }
    }

    private func singleToolRow(tool: String) -> some View {
        Button {
            if selectedToolIds.contains(tool) { selectedToolIds.remove(tool) } else { selectedToolIds.insert(tool) }
        } label: {
            HStack(spacing: 8) {
                Image(systemName: selectedToolIds.contains(tool) ? "checkmark.square.fill" : "square")
                    .foregroundStyle(selectedToolIds.contains(tool) ? Color.projectBrand : .secondary)
                Text(tool).font(.subheadline).lineLimit(1)
                Spacer()
            }
        }
        .buttonStyle(.plain)
    }

    private func groupSelectionIcon(_ group: ToolGroup) -> String {
        if group.isFullySelected(selectedToolIds) { return "checkmark.square.fill" }
        if group.isPartiallySelected(selectedToolIds) { return "minus.square.fill" }
        return "square"
    }

    private func toggleGroup(_ group: ToolGroup) {
        if group.isFullySelected(selectedToolIds) {
            group.tools.forEach { selectedToolIds.remove($0) }
        } else {
            group.tools.forEach { selectedToolIds.insert($0) }
        }
    }

    // MARK: Step 4: Review

    private var reviewStep: some View {
        VStack(alignment: .leading, spacing: 16) {
            sectionHeader("Summary", subtitle: "Everything looks good? Hit Create to launch your project.")
            VStack(alignment: .leading, spacing: 10) {
                reviewRow(icon: "folder.fill", title: "Project", value: trimmedName)
                reviewRow(icon: "text.alignleft", title: "Description", value: trimmedDescription.isEmpty ? "No description" : trimmedDescription)
                reviewRow(icon: "person.3.sequence.fill", title: "Agents", value: "\(allSelectedAgentIds.count) assigned")
                if !selectedTeamIds.isEmpty {
                    reviewRow(icon: "person.2.fill", title: "Teams", value: "\(selectedTeamIds.count) selected")
                }
                reviewRow(icon: "wrench.and.screwdriver.fill", title: "Tools", value: "\(selectedToolIds.count) configured")
                reviewRow(icon: "link", title: "Repository", value: repoUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "None" : repoUrl.trimmingCharacters(in: .whitespacesAndNewlines))
            }
        }
    }

    private func reviewRow(icon: String, title: String, value: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: icon)
                .font(.caption)
                .foregroundStyle(Color.projectBrand)
                .frame(width: 20)
            Text(title).font(.caption).foregroundStyle(.secondary).frame(width: 80, alignment: .leading)
            Text(value).font(.subheadline).lineLimit(2)
            Spacer(minLength: 0)
        }
    }

    // MARK: - Action Bar

    private var actionBar: some View {
        HStack(spacing: 10) {
            if step != .projectInfo {
                Button {
                    goBack()
                } label: {
                    Label("Back", systemImage: "chevron.left").frame(minWidth: 90)
                }
                .disabled(isCreating)
                .adaptiveGlassButtonStyle()
            }

            #if os(macOS)
            Button("Cancel") { onComplete() }
                .disabled(isCreating)
                .adaptiveGlassButtonStyle()
            #endif

            Spacer(minLength: 0)

            Button {
                handlePrimaryAction()
            } label: {
                if isCreating {
                    ProgressView().controlSize(.small).frame(minWidth: 110)
                } else {
                    Text(step == .review ? "Create Project" : "Next").frame(minWidth: 110)
                }
            }
            .disabled(isCreating || !canProceed)
            .adaptiveProminentGlassButtonStyle()
        }
    }

    // MARK: - Helpers

    private func sectionHeader(_ title: String, subtitle: String?) -> some View {
        VStack(alignment: .leading, spacing: 4) {
            Text(title).font(.headline)
            if let subtitle {
                Text(subtitle).font(.caption).foregroundStyle(.secondary)
            }
        }
    }

    private func fieldGroup<Content: View>(_ title: String, help: String, @ViewBuilder content: () -> Content) -> some View {
        VStack(alignment: .leading, spacing: 5) {
            HStack(spacing: 6) {
                Text(title).font(.caption.weight(.semibold))
                Text(help).font(.caption2).foregroundStyle(.secondary)
            }
            content()
        }
    }

    private func goBack() {
        switch step {
        case .projectInfo: break
        case .agents: step = .projectInfo
        case .configuration: step = .agents
        case .review: step = .configuration
        }
    }

    private func handlePrimaryAction() {
        switch step {
        case .projectInfo: step = .agents
        case .agents: step = .configuration
        case .configuration: step = .review
        case .review: createProject()
        }
    }

    // MARK: - Data Loading

    private func loadTools() async {
        do {
            let tools = try await coreManager.safeCore.getAllMcpTools()
            await MainActor.run {
                allMcpTools = tools
                toolGroups = ToolGroup.buildGroups(from: tools.map(\.name))
            }
        } catch {
            // Non-fatal
        }
    }

    // MARK: - Create Project

    private func createProject() {
        guard !trimmedName.isEmpty else { step = .projectInfo; return }
        isCreating = true
        Task {
            do {
                try await coreManager.safeCore.createProject(
                    name: trimmedName,
                    description: trimmedDescription,
                    agentDefinitionIds: Array(allSelectedAgentIds),
                    mcpToolIds: Array(selectedToolIds)
                )
                await coreManager.fetchData()
                await MainActor.run {
                    isCreating = false
                    onComplete()
                }
            } catch {
                await MainActor.run {
                    isCreating = false
                    errorMessage = error.localizedDescription
                }
            }
        }
    }
}
