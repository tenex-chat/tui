import SwiftUI

// MARK: - Layout Mode

enum AgentsLayoutMode {
    case adaptive
    case shellList
    case shellDetail
}

// MARK: - Data Model

struct AgentInstance: Identifiable, Hashable {
    let pubkey: String
    let name: String
    let isPm: Bool
    let model: String?
    let tools: [String]
    let projectId: String
    let projectTitle: String
    let isProjectOnline: Bool
    let projectCount: Int

    var id: String { pubkey }
}

// MARK: - Agents Tab View

struct AgentsTabView: View {
    @Environment(TenexCoreManager.self) var coreManager

    let layoutMode: AgentsLayoutMode
    private let selectedAgentBindingOverride: Binding<AgentInstance?>?
    private let onNavigateToChat: ((String, String) -> Void)?

    @State private var selectedAgentState: AgentInstance?
    @State private var searchText = ""
    @State private var composerTarget: AgentComposerTarget?

    #if os(iOS)
    @Environment(\.horizontalSizeClass) private var horizontalSizeClass
    #endif

    init(
        layoutMode: AgentsLayoutMode = .adaptive,
        selectedAgent: Binding<AgentInstance?>? = nil,
        onNavigateToChat: ((String, String) -> Void)? = nil
    ) {
        self.layoutMode = layoutMode
        self.selectedAgentBindingOverride = selectedAgent
        self.onNavigateToChat = onNavigateToChat
    }

    private var selectedAgentBinding: Binding<AgentInstance?> {
        selectedAgentBindingOverride ?? $selectedAgentState
    }

    private var useSplitView: Bool {
        if layoutMode == .shellList || layoutMode == .shellDetail {
            return true
        }
        #if os(macOS)
        return true
        #else
        return horizontalSizeClass == .regular
        #endif
    }

    // MARK: - Data

    private var allAgents: [AgentInstance] {
        var allInstances: [AgentInstance] = []
        let projects = coreManager.projects
        let onlineStatus = coreManager.projectOnlineStatus
        let onlineAgents = coreManager.onlineAgents

        var projectCountByPubkey: [String: Int] = [:]
        for project in projects {
            guard let agents = onlineAgents[project.id], !agents.isEmpty else { continue }
            for agent in agents {
                projectCountByPubkey[agent.pubkey, default: 0] += 1
            }
        }

        for project in projects {
            let isOnline = onlineStatus[project.id] ?? false
            guard let agents = onlineAgents[project.id], !agents.isEmpty else { continue }
            for agent in agents {
                allInstances.append(AgentInstance(
                    pubkey: agent.pubkey,
                    name: agent.name,
                    isPm: agent.isPm,
                    model: agent.model,
                    tools: agent.tools,
                    projectId: project.id,
                    projectTitle: project.title,
                    isProjectOnline: isOnline,
                    projectCount: projectCountByPubkey[agent.pubkey] ?? 1
                ))
            }
        }

        let sorted = allInstances.sorted { a, b in
            if a.isProjectOnline != b.isProjectOnline { return a.isProjectOnline }
            if a.isPm != b.isPm { return a.isPm }
            return a.name.localizedCaseInsensitiveCompare(b.name) == .orderedAscending
        }

        var seen = Set<String>()
        return sorted.filter { agent in
            guard !seen.contains(agent.pubkey) else { return false }
            seen.insert(agent.pubkey)
            return true
        }
    }

    private var filteredAgents: [AgentInstance] {
        guard !searchText.isEmpty else { return allAgents }
        let query = searchText.lowercased()
        return allAgents.filter {
            $0.name.lowercased().contains(query) ||
            $0.projectTitle.lowercased().contains(query) ||
            ($0.model?.lowercased().contains(query) ?? false)
        }
    }

    private var onlineAgentsCount: Int {
        allAgents.filter(\.isProjectOnline).count
    }

    // MARK: - Body

    var body: some View {
        Group {
            switch layoutMode {
            case .shellList:
                shellListLayout
            case .shellDetail:
                shellDetailLayout
            case .adaptive:
                if useSplitView {
                    splitViewLayout
                } else {
                    stackLayout
                }
            }
        }
        .onChange(of: coreManager.onlineAgents) { _, _ in
            guard let selected = selectedAgentBinding.wrappedValue else { return }
            if !filteredAgents.contains(where: { $0.id == selected.id }) {
                selectedAgentBinding.wrappedValue = nil
            }
        }
        .sheet(item: $composerTarget) { target in
            // TODO(#modal-composer-deprecation): migrate this modal composer entry point to inline flow.
            MessageComposerView(
                project: target.project,
                initialAgentPubkey: target.agentPubkey
            )
            .environment(coreManager)
            .tenexModalPresentation(detents: [.large])
        }
    }

    // MARK: - Split View Layout (iPad/Mac)

    private var splitViewLayout: some View {
        #if os(macOS)
        HSplitView {
            agentsListView
                .navigationTitle("Agents")
                .frame(minWidth: 340, idealWidth: 440, maxWidth: 520, maxHeight: .infinity)

            agentDetailContent
                .frame(minWidth: 600, maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #else
        NavigationSplitView {
            agentsListView
                .navigationTitle("Agents")
        } detail: {
            agentDetailContent
                .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        }
        #endif
    }

    @ViewBuilder
    private var agentDetailContent: some View {
        if let agent = selectedAgentBinding.wrappedValue {
            AgentDetailView(agent: agent, onStartConversation: startConversation)
                .id(agent.id)
        } else {
            ContentUnavailableView(
                "Select an Agent",
                systemImage: "cpu",
                description: Text("Choose an agent from the list to view details")
            )
        }
    }

    // MARK: - Stack Layout (iPhone)

    private var stackLayout: some View {
        NavigationStack {
            agentsListView
                .navigationTitle("Agents")
                .navigationDestination(for: AgentInstance.self) { agent in
                    AgentDetailView(agent: agent, onStartConversation: startConversation)
                }
        }
    }

    private var shellListLayout: some View {
        agentsListView
            .navigationTitle("Agents")
            .accessibilityIdentifier("section_list_column")
    }

    private var shellDetailLayout: some View {
        agentDetailContent
            .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
            .accessibilityIdentifier("detail_column")
    }

    // MARK: - List

    private var agentsListView: some View {
        Group {
            if filteredAgents.isEmpty {
                emptyStateView
            } else {
                List(filteredAgents, selection: useSplitView ? selectedAgentBinding : nil) { agent in
                    if useSplitView {
                        AgentRowView(agent: agent)
                            .tag(agent)
                    } else {
                        NavigationLink(value: agent) {
                            AgentRowView(agent: agent)
                        }
                    }
                }
                #if os(iOS)
                .listStyle(.plain)
                #else
                .listStyle(.inset)
                #endif
            }
        }
        .searchable(text: $searchText, prompt: "Search agents...")
        .toolbar {
            ToolbarItem(placement: .automatic) {
                AppGlobalFilterToolbarButton()
            }
        }
    }

    // MARK: - Empty State

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: emptyIcon)
                .font(.system(size: 60))
                .foregroundStyle(.secondary)

            Text(emptyTitle)
                .font(.title2)
                .fontWeight(.semibold)

            Text(emptyMessage)
                .font(.subheadline)
                .foregroundStyle(.secondary)
                .multilineTextAlignment(.center)

            if !searchText.isEmpty {
                Button {
                    searchText = ""
                } label: {
                    Label("Clear Search", systemImage: "xmark.circle")
                }
                .adaptiveGlassButtonStyle()
                .padding(.top, 8)
            }
        }
        .padding()
    }

    private var emptyIcon: String {
        !searchText.isEmpty ? "magnifyingglass" : "cpu"
    }

    private var emptyTitle: String {
        !searchText.isEmpty ? "No Matching Agents" : "No Online Agents"
    }

    private var emptyMessage: String {
        if !searchText.isEmpty {
            return "Try adjusting your search terms"
        }
        return "Agents will appear here when projects are booted"
    }

    // MARK: - Actions

    private func startConversation(with agent: AgentInstance) {
        if let onNavigateToChat {
            onNavigateToChat(agent.projectId, agent.pubkey)
        } else {
            guard let project = coreManager.projects.first(where: { $0.id == agent.projectId }) else { return }
            composerTarget = AgentComposerTarget(project: project, agentPubkey: agent.pubkey)
        }
    }
}

// MARK: - Composer Target

private struct AgentComposerTarget: Identifiable {
    let project: Project
    let agentPubkey: String
    var id: String { "\(project.id):\(agentPubkey)" }
}

// MARK: - Agent Row View

struct AgentRowView: View {
    let agent: AgentInstance
    @Environment(TenexCoreManager.self) private var coreManager

    var body: some View {
        HStack(spacing: 12) {
            AgentAvatarView(
                agentName: agent.name,
                pubkey: agent.pubkey,
                size: 36
            )

            VStack(alignment: .leading, spacing: 4) {
                HStack(spacing: 6) {
                    Text(coreManager.displayName(for: agent.pubkey))
                        .font(.headline)
                        .lineLimit(1)

                    if agent.isPm {
                        Text("PM")
                            .font(.caption2.weight(.semibold))
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(Color.agentBrand.opacity(0.15))
                            .foregroundStyle(Color.agentBrand)
                            .clipShape(Capsule())
                    }
                }

                HStack(spacing: 6) {
                    if let model = agent.model, !model.isEmpty {
                        Text(model)
                            .font(.caption)
                            .foregroundStyle(.secondary)
                            .lineLimit(1)
                    }

                    if !agent.tools.isEmpty {
                        HStack(spacing: 2) {
                            Image(systemName: "wrench")
                                .font(.caption2)
                            Text("\(agent.tools.count)")
                        }
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                    }
                }

                HStack(spacing: 6) {
                    Text(agent.projectTitle)
                        .font(.caption2)
                        .padding(.horizontal, 6)
                        .padding(.vertical, 2)
                        .background(Color.projectBrandBackground)
                        .foregroundStyle(Color.projectBrand)
                        .clipShape(Capsule())

                    if agent.projectCount > 1 {
                        Text("+\(agent.projectCount - 1) more")
                            .font(.caption2)
                            .foregroundStyle(.tertiary)
                    }

                    if agent.isProjectOnline {
                        HStack(spacing: 3) {
                            Circle()
                                .fill(Color.presenceOnline)
                                .frame(width: 6, height: 6)
                            Text("Online")
                        }
                        .font(.caption2)
                        .foregroundStyle(Color.presenceOnline)
                    }
                }
            }

            Spacer()
        }
        .padding(.vertical, 6)
    }
}

// MARK: - Agent Detail View

struct AgentDetailView: View {
    let agent: AgentInstance
    let onStartConversation: (AgentInstance) -> Void

    @Environment(TenexCoreManager.self) private var coreManager

    private var resolvedName: String {
        coreManager.displayName(for: agent.pubkey)
    }

    private var otherProjectsForAgent: [AgentInstance] {
        let projects = coreManager.projects
        let onlineAgents = coreManager.onlineAgents
        let onlineStatus = coreManager.projectOnlineStatus

        var result: [AgentInstance] = []
        for project in projects where project.id != agent.projectId {
            guard let agents = onlineAgents[project.id] else { continue }
            if let match = agents.first(where: { $0.pubkey == agent.pubkey }) {
                result.append(AgentInstance(
                    pubkey: match.pubkey,
                    name: match.name,
                    isPm: match.isPm,
                    model: match.model,
                    tools: match.tools,
                    projectId: project.id,
                    projectTitle: project.title,
                    isProjectOnline: onlineStatus[project.id] ?? false,
                    projectCount: 1
                ))
            }
        }
        return result
    }

    var body: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 24) {
                headerSection
                Divider()
                projectSection
                if !agent.tools.isEmpty {
                    Divider()
                    toolsSection
                }
            }
            .padding()
            .frame(maxWidth: .infinity, alignment: .leading)
        }
        .frame(maxWidth: .infinity, maxHeight: .infinity, alignment: .topLeading)
        .navigationTitle(resolvedName)
        #if os(iOS)
        .navigationBarTitleDisplayMode(.inline)
        #else
        .toolbarTitleDisplayMode(.inline)
        #endif
        .toolbar {
            ToolbarItem(placement: .automatic) {
                Button {
                    onStartConversation(agent)
                } label: {
                    Label("Start Conversation", systemImage: "bubble.left.fill")
                }
            }
        }
    }

    // MARK: - Header

    private var headerSection: some View {
        HStack(spacing: 16) {
            AgentAvatarView(
                agentName: agent.name,
                pubkey: agent.pubkey,
                size: 64
            )

            VStack(alignment: .leading, spacing: 8) {
                HStack(spacing: 8) {
                    Text(resolvedName)
                        .font(.title2)
                        .fontWeight(.bold)

                    if agent.isPm {
                        Text("Project Manager")
                            .font(.caption.weight(.semibold))
                            .padding(.horizontal, 8)
                            .padding(.vertical, 3)
                            .background(Color.agentBrand.opacity(0.15))
                            .foregroundStyle(Color.agentBrand)
                            .clipShape(Capsule())
                    }

                    if agent.isProjectOnline {
                        HStack(spacing: 4) {
                            Circle()
                                .fill(Color.presenceOnline)
                                .frame(width: 8, height: 8)
                            Text("Online")
                        }
                        .font(.caption.weight(.medium))
                        .foregroundStyle(Color.presenceOnline)
                    }
                }

                if let model = agent.model, !model.isEmpty {
                    HStack(spacing: 4) {
                        Image(systemName: "cpu")
                        Text(model)
                    }
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                }

                if let npub = Bech32.hexToNpub(agent.pubkey) {
                    Text(npub)
                        .font(.caption)
                        .foregroundStyle(.tertiary)
                        .lineLimit(1)
                        .truncationMode(.middle)
                        .textSelection(.enabled)
                }
            }
        }
    }

    // MARK: - Projects

    private var projectSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Projects")
                .font(.headline)

            projectRow(for: agent, isCurrent: true)

            ForEach(otherProjectsForAgent) { otherAgent in
                projectRow(for: otherAgent, isCurrent: false)
            }
        }
    }

    private func projectRow(for agentInstance: AgentInstance, isCurrent: Bool) -> some View {
        HStack(spacing: 12) {
            Image(systemName: "folder.fill")
                .font(.title3)
                .foregroundStyle(agentInstance.isProjectOnline ? Color.projectBrand : .secondary)

            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(agentInstance.projectTitle)
                        .font(.subheadline.weight(.medium))

                    if agentInstance.isProjectOnline {
                        Circle()
                            .fill(Color.presenceOnline)
                            .frame(width: 6, height: 6)
                    }

                    if isCurrent {
                        Text("Current")
                            .font(.caption2)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(Color.agentBrand.opacity(0.12))
                            .foregroundStyle(Color.agentBrand)
                            .clipShape(Capsule())
                    }
                }

                if agentInstance.isPm {
                    Text("Project Manager")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                } else {
                    Text("Agent")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }

            Spacer()

            Button {
                onStartConversation(agentInstance)
            } label: {
                Label("Chat", systemImage: "bubble.left")
                    .font(.subheadline)
            }
            .buttonStyle(.bordered)
        }
        .padding(12)
        .background(.regularMaterial)
        .clipShape(RoundedRectangle(cornerRadius: 10))
    }

    // MARK: - Tools

    private var toolsSection: some View {
        VStack(alignment: .leading, spacing: 12) {
            Text("Tools")
                .font(.headline)

            FlowLayout(spacing: 6) {
                ForEach(agent.tools, id: \.self) { tool in
                    Text(tool)
                        .font(.caption)
                        .padding(.horizontal, 10)
                        .padding(.vertical, 5)
                        .background(Color.skillBrandBackground)
                        .foregroundStyle(Color.skillBrand)
                        .clipShape(Capsule())
                }
            }
        }
    }
}


#Preview {
    AgentsTabView()
        .environment(TenexCoreManager())
}
