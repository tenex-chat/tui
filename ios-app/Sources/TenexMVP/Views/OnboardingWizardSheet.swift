import SwiftUI

private enum OnboardingStep: Int, CaseIterable, Identifiable {
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

struct OnboardingWizardSheet: View {
    @Environment(TenexCoreManager.self) private var coreManager
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    @State private var step: OnboardingStep = .projectInfo

    // Step 1: Project Info
    @State private var projectName = ""
    @State private var projectDescription = ""

    // Step 2: Agent Selection
    @State private var allAgents: [AgentDefinition] = []
    @State private var selectedAgentIds: Set<String> = []
    @State private var agentSearchText = ""

    // Step 3: Configuration
    @State private var repoUrl = ""
    @State private var allMcpTools: [McpTool] = []
    @State private var toolGroups: [ToolGroup] = []
    @State private var selectedToolIds: Set<String> = []

    // State
    @State private var isCreating = false
    @State private var errorMessage: String?

    private var trimmedName: String {
        projectName.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var trimmedDescription: String {
        projectDescription.trimmingCharacters(in: .whitespacesAndNewlines)
    }

    private var canProceed: Bool {
        switch step {
        case .projectInfo:
            return !trimmedName.isEmpty
        case .agents, .configuration:
            return true
        case .review:
            return true
        }
    }

    private var filteredAgents: [AgentDefinition] {
        if agentSearchText.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
            return allAgents
        }
        let query = agentSearchText.lowercased()
        return allAgents.filter {
            $0.name.lowercased().contains(query)
            || $0.description.lowercased().contains(query)
            || $0.role.lowercased().contains(query)
        }
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 14) {
                    heroCard
                    stepRail
                    stepContent
                }
                .padding(.horizontal, 18)
                .padding(.top, 16)
                .padding(.bottom, 84)
            }
            .background(backgroundView)
            .navigationTitle("Set Up Your Project")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Skip") {
                        dismiss()
                    }
                    .disabled(isCreating)
                }
            }
        }
        .safeAreaInset(edge: .bottom) {
            actionBar
        }
        .tenexModalPresentation(detents: [.large])
        #if os(macOS)
        .frame(minWidth: 760, idealWidth: 860, minHeight: 640, idealHeight: 740)
        #endif
        .task {
            await loadAgentsAndTools()
        }
        .alert(
            "Unable to Create Project",
            isPresented: Binding(
                get: { errorMessage != nil },
                set: { if !$0 { errorMessage = nil } }
            )
        ) {
            Button("OK", role: .cancel) { errorMessage = nil }
        } message: {
            Text(errorMessage ?? "Unknown error")
        }
    }

    // MARK: - Background

    private var backgroundView: some View {
        LinearGradient(
            colors: [
                Color.projectBrand.opacity(reduceTransparency ? 0.03 : 0.10),
                Color.systemGroupedBackground,
                Color.systemGroupedBackground
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
        .ignoresSafeArea()
    }

    // MARK: - Hero Card

    @ViewBuilder
    private var heroCard: some View {
        switch step {
        case .projectInfo:
            GlassPanel(
                title: "Create your first project",
                subtitle: "Projects organize your team's work — agents, conversations, and reports all live inside a project."
            ) {
                HStack(spacing: 10) {
                    statPill(label: "Name", value: trimmedName.isEmpty ? "Pending" : trimmedName)
                    statPill(label: "Agents", value: "\(selectedAgentIds.count)")
                    statPill(label: "Tools", value: "\(selectedToolIds.count)")
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        case .agents:
            GlassPanel(
                title: "Assign agents to your project",
                subtitle: "Agents run autonomously — monitoring, summarizing, and acting on your behalf."
            ) {
                HStack(spacing: 10) {
                    statPill(label: "Selected", value: "\(selectedAgentIds.count)")
                    statPill(label: "Available", value: "\(allAgents.count)")
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        case .configuration:
            GlassPanel(
                title: "Connect your tools",
                subtitle: "Link your repository and configure integrations."
            ) {
                HStack(spacing: 10) {
                    statPill(label: "Tools", value: "\(selectedToolIds.count)")
                    statPill(label: "Repo", value: repoUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "None" : "Linked")
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        case .review:
            GlassPanel(
                title: "Ready to launch",
                subtitle: "Here's what we'll set up:"
            ) {
                HStack(spacing: 10) {
                    statPill(label: "Project", value: trimmedName)
                    statPill(label: "Agents", value: "\(selectedAgentIds.count)")
                    statPill(label: "Tools", value: "\(selectedToolIds.count)")
                }
                .frame(maxWidth: .infinity, alignment: .leading)
            }
        }
    }

    private func statPill(label: String, value: String) -> some View {
        VStack(alignment: .leading, spacing: 2) {
            Text(label)
                .font(.caption2)
                .foregroundStyle(.secondary)
            Text(value)
                .font(.caption.weight(.semibold))
                .lineLimit(1)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color.systemBackground.opacity(reduceTransparency ? 1 : 0.55), in: Capsule())
    }

    // MARK: - Step Rail

    private var stepRail: some View {
        HStack(spacing: 8) {
            ForEach(OnboardingStep.allCases) { candidate in
                Button {
                    guard candidate.rawValue <= step.rawValue else { return }
                    step = candidate
                } label: {
                    HStack(spacing: 6) {
                        Image(systemName: candidate.symbol)
                            .font(.caption.weight(.semibold))
                        Text(candidate.title)
                            .font(.caption.weight(.semibold))
                    }
                    .foregroundStyle(step == candidate ? Color.projectBrand : .secondary)
                    .padding(.horizontal, 10)
                    .padding(.vertical, 6)
                    .frame(maxWidth: .infinity)
                    .background(
                        RoundedRectangle(cornerRadius: 10, style: .continuous)
                            .fill(step == candidate ? Color.projectBrand.opacity(0.18) : Color.systemGray6.opacity(0.6))
                    )
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
        case .projectInfo:
            projectInfoStep
        case .agents:
            agentsStep
        case .configuration:
            configurationStep
        case .review:
            reviewStep
        }
    }

    // MARK: Step 1: Project Info

    private var projectInfoStep: some View {
        GlassPanel(
            title: "Project Details",
            subtitle: "Give your project a clear name so collaborators can find it."
        ) {
            VStack(alignment: .leading, spacing: 12) {
                VStack(alignment: .leading, spacing: 5) {
                    fieldLabel("Name", help: "Required")
                    TextField("My First Project", text: $projectName)
                        .textFieldStyle(.roundedBorder)
                }

                VStack(alignment: .leading, spacing: 5) {
                    fieldLabel("Description", help: "Optional")
                    TextField("What this project is about", text: $projectDescription, axis: .vertical)
                        .lineLimit(3...5)
                        .textFieldStyle(.roundedBorder)
                }
            }
        }
    }

    // MARK: Step 2: Agent Selection

    private var agentsStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            GlassPanel(title: "Available Agents", subtitle: "Tap to assign agents to your project.") {
                VStack(alignment: .leading, spacing: 10) {
                    TextField("Search agents…", text: $agentSearchText)
                        .textFieldStyle(.roundedBorder)

                    if filteredAgents.isEmpty {
                        Text(allAgents.isEmpty ? "No agent definitions found." : "No agents match your search.")
                            .foregroundStyle(.secondary)
                            .frame(maxWidth: .infinity, alignment: .center)
                            .padding(.vertical, 20)
                    } else {
                        LazyVGrid(columns: [GridItem(.adaptive(minimum: 240), spacing: 10)], spacing: 10) {
                            ForEach(filteredAgents, id: \.id) { agent in
                                agentCard(agent)
                            }
                        }
                    }
                }
            }
        }
    }

    private func agentCard(_ agent: AgentDefinition) -> some View {
        let isSelected = selectedAgentIds.contains(agent.id)
        return Button {
            if isSelected {
                selectedAgentIds.remove(agent.id)
            } else {
                selectedAgentIds.insert(agent.id)
            }
        } label: {
            HStack(alignment: .top, spacing: 10) {
                VStack(alignment: .leading, spacing: 4) {
                    Text(agent.name)
                        .font(.subheadline.weight(.semibold))
                        .lineLimit(1)
                    Text(agent.role)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                    Text(agent.description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                        .lineLimit(2)
                }
                Spacer(minLength: 0)
                Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                    .foregroundStyle(isSelected ? Color.projectBrand : .secondary)
                    .font(.title3)
            }
            .padding(12)
            .background(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .fill(Color.systemBackground.opacity(reduceTransparency ? 1 : 0.56))
            )
            .overlay(
                RoundedRectangle(cornerRadius: 12, style: .continuous)
                    .stroke(isSelected ? Color.projectBrand.opacity(0.5) : .white.opacity(reduceTransparency ? 0.06 : 0.14), lineWidth: 1)
            )
        }
        .buttonStyle(.plain)
    }

    // MARK: Step 3: Configuration

    private var configurationStep: some View {
        VStack(alignment: .leading, spacing: 12) {
            GlassPanel(
                title: "Git Repository",
                subtitle: "Optional — link a repository for context."
            ) {
                TextField("https://github.com/org/repo", text: $repoUrl)
                    .textFieldStyle(.roundedBorder)
                    .autocorrectionDisabled()
                    #if os(iOS)
                    .textInputAutocapitalization(.never)
                    .keyboardType(.URL)
                    #endif
            }

            GlassPanel(
                title: "MCP Tools",
                subtitle: "Select tools to make available in this project."
            ) {
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
                DisclosureGroup(isExpanded: Binding(
                    get: { toolGroups[index].isExpanded },
                    set: { toolGroups[index].isExpanded = $0 }
                )) {
                    ForEach(group.tools, id: \.self) { tool in
                        singleToolRow(tool: tool)
                            .padding(.leading, 16)
                    }
                } label: {
                    HStack(spacing: 8) {
                        Image(systemName: groupSelectionIcon(group))
                            .foregroundStyle(group.isFullySelected(selectedToolIds) ? Color.projectBrand : .secondary)
                        Text(group.name)
                            .font(.subheadline)
                        Spacer()
                        Text("\(group.tools.filter { selectedToolIds.contains($0) }.count)/\(group.tools.count)")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                    .contentShape(Rectangle())
                    .onTapGesture {
                        toggleGroup(group)
                    }
                }
            }
        }
    }

    private func singleToolRow(tool: String) -> some View {
        Button {
            if selectedToolIds.contains(tool) {
                selectedToolIds.remove(tool)
            } else {
                selectedToolIds.insert(tool)
            }
        } label: {
            HStack(spacing: 8) {
                Image(systemName: selectedToolIds.contains(tool) ? "checkmark.square.fill" : "square")
                    .foregroundStyle(selectedToolIds.contains(tool) ? Color.projectBrand : .secondary)
                Text(tool)
                    .font(.subheadline)
                    .lineLimit(1)
                Spacer()
            }
        }
        .buttonStyle(.plain)
    }

    private func groupSelectionIcon(_ group: ToolGroup) -> String {
        if group.isFullySelected(selectedToolIds) {
            return "checkmark.square.fill"
        } else if group.isPartiallySelected(selectedToolIds) {
            return "minus.square.fill"
        }
        return "square"
    }

    private func toggleGroup(_ group: ToolGroup) {
        if group.isFullySelected(selectedToolIds) {
            for tool in group.tools {
                selectedToolIds.remove(tool)
            }
        } else {
            for tool in group.tools {
                selectedToolIds.insert(tool)
            }
        }
    }

    // MARK: Step 4: Review

    private var reviewStep: some View {
        GlassPanel(
            title: "Summary",
            subtitle: "Everything looks good? Hit Create to launch your project."
        ) {
            VStack(alignment: .leading, spacing: 10) {
                reviewRow(icon: "folder.fill", title: "Project", value: trimmedName)
                reviewRow(
                    icon: "text.alignleft",
                    title: "Description",
                    value: trimmedDescription.isEmpty ? "No description" : trimmedDescription
                )
                reviewRow(
                    icon: "person.3.sequence.fill",
                    title: "Agents",
                    value: "\(selectedAgentIds.count) assigned"
                )
                reviewRow(
                    icon: "wrench.and.screwdriver.fill",
                    title: "Tools",
                    value: "\(selectedToolIds.count) configured"
                )
                reviewRow(
                    icon: "link",
                    title: "Repository",
                    value: repoUrl.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty ? "None" : repoUrl.trimmingCharacters(in: .whitespacesAndNewlines)
                )
            }
        }
    }

    private func reviewRow(icon: String, title: String, value: String) -> some View {
        HStack(spacing: 10) {
            Image(systemName: icon)
                .font(.caption)
                .foregroundStyle(Color.projectBrand)
                .frame(width: 20)
            Text(title)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 80, alignment: .leading)
            Text(value)
                .font(.subheadline)
                .lineLimit(2)
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
                    Label("Back", systemImage: "chevron.left")
                        .frame(minWidth: 90)
                }
                .disabled(isCreating)
                .buttonStyle(.bordered)
            }

            Spacer(minLength: 0)

            Button {
                handlePrimaryAction()
            } label: {
                if isCreating {
                    ProgressView()
                        .controlSize(.small)
                        .frame(minWidth: 110)
                } else {
                    Text(primaryActionTitle)
                        .frame(minWidth: 110)
                }
            }
            .disabled(isCreating || !canProceed)
            .adaptiveGlassButtonStyle()
        }
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

    // MARK: - Helpers

    private func fieldLabel(_ title: String, help: String) -> some View {
        HStack(spacing: 6) {
            Text(title)
                .font(.caption.weight(.semibold))
            Text(help)
                .font(.caption2)
                .foregroundStyle(.secondary)
        }
    }

    private var primaryActionTitle: String {
        switch step {
        case .review: return "Create Project"
        default: return "Next"
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

    private func loadAgentsAndTools() async {
        do {
            let agents = try await coreManager.safeCore.getAllAgents()
            let tools = try await coreManager.safeCore.getAllMcpTools()
            await MainActor.run {
                allAgents = agents
                allMcpTools = tools
                toolGroups = ToolGroup.buildGroups(from: tools.map(\.name))
            }
        } catch {
            // Non-fatal — user can still create a project without agents/tools
        }
    }

    // MARK: - Create Project

    private func createProject() {
        guard !trimmedName.isEmpty else {
            step = .projectInfo
            return
        }

        isCreating = true

        Task {
            do {
                try await coreManager.safeCore.createProject(
                    name: trimmedName,
                    description: trimmedDescription,
                    agentDefinitionIds: Array(selectedAgentIds),
                    mcpToolIds: Array(selectedToolIds)
                )

                await MainActor.run {
                    isCreating = false
                }

                await coreManager.fetchData()
                await MainActor.run {
                    dismiss()
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
