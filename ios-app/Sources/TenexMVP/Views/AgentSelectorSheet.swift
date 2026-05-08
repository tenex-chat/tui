import SwiftUI

/// A sheet for selecting an agent to p-tag in a message.
/// Uses the ordered project roster from kind:31933, annotated with kind:24011 availability.
struct AgentSelectorSheet: View {
    // MARK: - Properties

    /// Ordered project roster rows to choose from.
    let agents: [ProjectAgent]

    /// Project ID for agent configuration
    let projectId: String

    /// Currently selected agent pubkey (binding for single-select)
    @Binding var selectedPubkey: String?

    /// Callback when selection is confirmed
    var onDone: (() -> Void)?

    /// Optional initial search query (used by @ trigger in composer)
    var initialSearchQuery: String = ""

    // MARK: - Environment

    @Environment(TenexCoreManager.self) var coreManager
    @Environment(\.dismiss) private var dismiss

    // MARK: - State

    @State private var searchText = ""
    @State private var agentToConfig: ProjectAgent?
    @State private var agentToDelete: ProjectAgent?

    // MARK: - Computed

    private var filteredAgents: [ProjectAgent] {
        let filtered: [ProjectAgent]
        if searchText.isEmpty {
            filtered = agents
        } else {
            // Use locale-aware filtering for correct international matching
            filtered = agents.filter { agent in
                AgentDisplayName.matches(
                    pubkey: agent.pubkey,
                    query: searchText,
                    coreManager: coreManager
                )
            }
        }

        return filtered
    }

    // MARK: - Body

    var body: some View {
        NavigationStack {
            agentList
            .searchable(text: $searchText, prompt: "Search agents...")
            .navigationTitle("Select Agent")
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
            }
            .onAppear {
                if !initialSearchQuery.isEmpty {
                    searchText = initialSearchQuery
                }
            }
            .sheet(item: $agentToConfig) { agent in
                AgentConfigSheet(agent: agent)
                    .environment(coreManager)
            }
            #if os(iOS)
            .sheet(item: $agentToDelete) { agent in
                AgentDeletionSheet(agent: agent, projectId: projectId)
                    .environment(coreManager)
            }
            #endif
        }
        #if os(iOS)
        .tenexModalPresentation(detents: [.large])
        #else
        .frame(minWidth: 480, idealWidth: 540, minHeight: 420, idealHeight: 520)
        #endif
    }

    // MARK: - Subviews

    @ViewBuilder
    private var agentList: some View {
        #if os(macOS)
        List {
            if filteredAgents.isEmpty {
                emptyStateView
            } else {
                ForEach(filteredAgents, id: \.pubkey) { agent in
                    RosterAgentRowView(
                        agent: agent,
                        onTap: { selectAgent(agent) },
                        onConfig: { agentToConfig = agent }
                    )
                }
            }
        }
        .listStyle(.inset)
        #else
        List {
            if filteredAgents.isEmpty {
                emptyStateView
            } else {
                ForEach(filteredAgents, id: \.pubkey) { agent in
                    RosterAgentRowView(
                        agent: agent,
                        onTap: { selectAgent(agent) },
                        onConfig: { agentToConfig = agent }
                    )
                    .swipeActions(edge: .trailing, allowsFullSwipe: false) {
                        Button(role: .destructive) {
                            agentToDelete = agent
                        } label: {
                            Label("Delete", systemImage: "trash")
                        }
                    }
                }
            }
        }
        .listStyle(.insetGrouped)
        #endif
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: searchText.isEmpty ? "person.2.slash" : "magnifyingglass")
                .font(.system(.largeTitle))
                .foregroundStyle(.secondary)

            if searchText.isEmpty {
                Text("No Agents")
                    .font(.headline)
                Text("This project has no selectable agents.")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            } else {
                Text("No Results")
                    .font(.headline)
                Text("No agents match \"\(searchText)\"")
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
            }
        }
        .frame(maxWidth: .infinity)
        .padding(.vertical, 60)
        .listRowBackground(Color.clear)
        .listRowSeparator(.hidden)
    }

    // MARK: - Actions

    private func selectAgent(_ agent: ProjectAgent) {
        selectedPubkey = agent.pubkey
        onDone?()
        dismiss()
    }
}

// MARK: - ProjectAgent Identifiable

extension ProjectAgent: Identifiable {
    public var id: String { pubkey }
}

// MARK: - Roster Agent Row View

struct RosterAgentRowView: View {
    @Environment(TenexCoreManager.self) var coreManager
    let agent: ProjectAgent
    var onTap: (() -> Void)?
    var onConfig: (() -> Void)?

    private var mainContent: some View {
        HStack(spacing: 10) {
            // Agent avatar - uses actual agent pubkey for profile lookup
            AgentAvatarView(
                agentName: AgentDisplayName.resolve(pubkey: agent.pubkey, coreManager: coreManager),
                pubkey: agent.pubkey,
                size: 36,
                showBorder: false,
                isSelected: false
            )
            .environment(coreManager)

            // Agent info
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(AgentDisplayName.resolve(pubkey: agent.pubkey, coreManager: coreManager))
                        .font(.subheadline)
                        .fontWeight(.medium)
                        .foregroundStyle(Color.primary)

                    if agent.isPm {
                        Text("PM")
                            .font(.caption2)
                            .fontWeight(.semibold)
                            .foregroundStyle(.white)
                            .padding(.horizontal, 5)
                            .padding(.vertical, 1)
                            .background(
                                Capsule()
                                    .fill(Color.agentBrand)
                            )
                    }
                }

                Text(agentAvailabilityLabel)
                    .font(.caption)
                    .foregroundStyle(agent.isOnline ? Color.presenceOnline : .secondary)
            }

            Spacer()
        }
    }

    var body: some View {
        HStack(spacing: 10) {
            if let onTap {
                Button(action: onTap) {
                    mainContent
                }
                .buttonStyle(.borderless)
            } else {
                mainContent
            }

            // Config gear button (separate from selection)
            if let onConfig = onConfig {
                Button(action: onConfig) {
                    Image(systemName: "gearshape")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .frame(minWidth: 44, minHeight: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.borderless)
            }
        }
    }

    private var agentAvailabilityLabel: String {
        guard agent.isOnline else { return "Unavailable" }
        if let model = agent.model, !model.isEmpty {
            return model
        }
        return "Available"
    }
}

// MARK: - Agent Deletion Sheet

#if os(iOS)
/// A sheet to confirm deletion of an agent, with scope picker (project vs global).
struct AgentDeletionSheet: View {
    // MARK: - Properties

    let agent: ProjectAgent
    let projectId: String

    // MARK: - Environment

    @Environment(TenexCoreManager.self) var coreManager
    @Environment(\.dismiss) private var dismiss

    // MARK: - State

    @State private var scope: DeletionScope = .project
    @State private var reason: String = ""
    @State private var isDeleting = false
    @State private var errorMessage: String?
    @State private var showError = false

    // MARK: - Types

    enum DeletionScope: CaseIterable {
        case project
        case global

        var title: String {
            switch self {
            case .project: return "This Project"
            case .global: return "All Projects (Global)"
            }
        }

        var description: String {
            switch self {
            case .project: return "Remove this agent from the current project only."
            case .global: return "Remove this agent from all projects system-wide."
            }
        }

        var systemImage: String {
            switch self {
            case .project: return "folder"
            case .global: return "globe"
            }
        }
    }

    // MARK: - Computed

    private var project: Project? {
        coreManager.projects.first { $0.id == projectId }
    }

    // MARK: - Body

    var body: some View {
        NavigationStack {
            List {
                // Agent info section
                Section {
                    HStack(spacing: 12) {
                        AgentAvatarView(
                            agentName: AgentDisplayName.resolve(pubkey: agent.pubkey, coreManager: coreManager),
                            pubkey: agent.pubkey,
                            size: 44,
                            showBorder: false,
                            isSelected: false
                        )
                        .environment(coreManager)

                        VStack(alignment: .leading, spacing: 3) {
                            Text(AgentDisplayName.resolve(pubkey: agent.pubkey, coreManager: coreManager))
                                .font(.body.weight(.medium))
                            Text(AgentDisplayName.shortPubkey(agent.pubkey))
                                .font(.caption)
                                .foregroundStyle(.secondary)
                        }
                    }
                    .padding(.vertical, 4)
                } header: {
                    Text("Agent to Remove")
                }

                // Scope picker section
                Section {
                    ForEach(DeletionScope.allCases, id: \.self) { option in
                        Button {
                            scope = option
                        } label: {
                            HStack(spacing: 12) {
                                Image(systemName: option.systemImage)
                                    .frame(width: 24)
                                    .foregroundStyle(option == scope ? Color.accentColor : .secondary)

                                VStack(alignment: .leading, spacing: 3) {
                                    Text(option.title)
                                        .font(.body)
                                        .foregroundStyle(.primary)
                                    Text(option.description)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }

                                Spacer()

                                if scope == option {
                                    Image(systemName: "checkmark")
                                        .font(.body.weight(.medium))
                                        .foregroundStyle(Color.accentColor)
                                }
                            }
                            .contentShape(Rectangle())
                        }
                        .buttonStyle(.borderless)
                        .padding(.vertical, 2)
                    }
                } header: {
                    Text("Scope")
                } footer: {
                    if scope == .global {
                        Text("⚠️ This publishes a kind:24030 event. The backend will remove this agent from all projects.")
                            .foregroundStyle(.secondary)
                    }
                }

                // Optional reason section
                Section {
                    TextField("Optional reason", text: $reason, axis: .vertical)
                        .lineLimit(3, reservesSpace: true)
                } header: {
                    Text("Reason (Optional)")
                }
            }
            .listStyle(.insetGrouped)
            .navigationTitle("Remove Agent")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                    .disabled(isDeleting)
                }

                ToolbarItem(placement: .primaryAction) {
                    Button(role: .destructive) {
                        Task { await confirmDeletion() }
                    } label: {
                        if isDeleting {
                            ProgressView()
                        } else {
                            Text("Remove")
                                .fontWeight(.semibold)
                                .foregroundStyle(.red)
                        }
                    }
                    .disabled(isDeleting)
                }
            }
            .alert("Deletion Failed", isPresented: $showError) {
                Button("OK", role: .cancel) {}
            } message: {
                Text(errorMessage ?? "An unknown error occurred.")
            }
        }
        .tenexModalPresentation(detents: [.medium, .large])
    }

    // MARK: - Actions

    private func confirmDeletion() async {
        isDeleting = true
        defer { isDeleting = false }

        let projectATag: String?
        if scope == .project, let p = project {
            // Build project a-tag: 31933:<project_pubkey>:<project_d_tag>
            projectATag = "31933:\(p.pubkey):\(p.id)"
        } else {
            projectATag = nil
        }

        let trimmedReason = reason.trimmingCharacters(in: .whitespacesAndNewlines)

        do {
            try await coreManager.core.deleteAgent(
                agentPubkey: agent.pubkey,
                projectATag: projectATag,
                reason: trimmedReason.isEmpty ? nil : trimmedReason
            )
            dismiss()
        } catch {
            errorMessage = error.localizedDescription
            showError = true
        }
    }
}
#endif

// MARK: - Preview

#Preview {
    AgentSelectorSheet(
        agents: [
            ProjectAgent(
                pubkey: "abc123def456",
                name: "claude-code",
                backendPubkey: "backend",
                isPm: true,
                isOnline: true,
                model: "claude-3-opus",
                tools: ["Read", "Write", "Bash"],
                skills: [],
                mcpServers: []
            ),
            ProjectAgent(
                pubkey: "def456ghi789",
                name: "architect",
                backendPubkey: "backend",
                isPm: false,
                isOnline: true,
                model: "claude-3-sonnet",
                tools: ["Read", "Edit"],
                skills: [],
                mcpServers: []
            ),
            ProjectAgent(
                pubkey: "ghi789jkl012",
                name: "test-writer",
                backendPubkey: "",
                isPm: false,
                isOnline: false,
                model: nil,
                tools: [],
                skills: [],
                mcpServers: []
            )
        ],
        projectId: "test-project",
        selectedPubkey: .constant(nil)
    )
    .environment(TenexCoreManager())
}
