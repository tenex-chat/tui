import SwiftUI

/// A sheet for selecting an agent to p-tag in a message.
/// Uses ProjectAgent from project status (kind:24010) which contains
/// actual agent instance pubkeys for proper profile picture lookup.
struct AgentSelectorSheet: View {
    // MARK: - Properties

    /// Available agents to choose from (from project status)
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
                agent.name.localizedCaseInsensitiveContains(searchText)
            }
        }

        // Sort: PM agents first, then alphabetically by name, with pubkey tie-breaker for stability
        return filtered.sorted { a, b in
            if a.isPm != b.isPm {
                return a.isPm  // PM agents come first
            }
            let nameComparison = a.name.localizedCaseInsensitiveCompare(b.name)
            if nameComparison != .orderedSame {
                return nameComparison == .orderedAscending
            }
            // Tie-breaker: use pubkey for stable sorting when names are equal
            return a.pubkey < b.pubkey
        }
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
                AgentConfigSheet(agent: agent, projectId: projectId)
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
        .tenexModalPresentation(detents: [.medium, .large])
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
                    OnlineAgentRowView(
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
                    OnlineAgentRowView(
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
                Text("No Agents Online")
                    .font(.headline)
                Text("This project has no online agents.")
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

// MARK: - Online Agent Row View

struct OnlineAgentRowView: View {
    @Environment(TenexCoreManager.self) var coreManager
    let agent: ProjectAgent
    var onTap: (() -> Void)?
    var onConfig: (() -> Void)?

    private var mainContent: some View {
        HStack(spacing: 10) {
            // Agent avatar - uses actual agent pubkey for profile lookup
            AgentAvatarView(
                agentName: agent.name,
                pubkey: agent.pubkey,
                size: 36,
                showBorder: false,
                isSelected: false
            )
            .environment(coreManager)

            // Agent info
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(agent.name)
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

                if let model = agent.model, !model.isEmpty {
                    Text(model)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
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
                            agentName: agent.name,
                            pubkey: agent.pubkey,
                            size: 44,
                            showBorder: false,
                            isSelected: false
                        )
                        .environment(coreManager)

                        VStack(alignment: .leading, spacing: 3) {
                            Text(agent.name)
                                .font(.body.weight(.medium))
                            Text("@\(agent.name)")
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
            try await coreManager.safeCore.deleteAgent(
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
                isPm: true,
                model: "claude-3-opus",
                tools: ["Read", "Write", "Bash"],
                skills: []
            ),
            ProjectAgent(
                pubkey: "def456ghi789",
                name: "architect",
                isPm: false,
                model: "claude-3-sonnet",
                tools: ["Read", "Edit"],
                skills: []
            ),
            ProjectAgent(
                pubkey: "ghi789jkl012",
                name: "test-writer",
                isPm: false,
                model: nil,
                tools: [],
                skills: []
            )
        ],
        projectId: "test-project",
        selectedPubkey: .constant(nil)
    )
    .environment(TenexCoreManager())
}
