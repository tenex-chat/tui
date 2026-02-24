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

    /// Local copy of selection - only committed on Done, discarded on Cancel
    @State private var localSelectedPubkey: String?
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

    private var selectedAgent: ProjectAgent? {
        agents.first { $0.pubkey == localSelectedPubkey }
    }

    // MARK: - Body

    var body: some View {
        NavigationStack {
            VStack(spacing: 0) {
                // Selected agent bar (if any)
                if let agent = selectedAgent {
                    selectedAgentBar(agent)
                }

                // Search and agent list
                agentList
            }
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
                        // Discard local changes
                        dismiss()
                    }
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Done") {
                        // Commit local changes to parent binding
                        selectedPubkey = localSelectedPubkey
                        onDone?()
                        dismiss()
                    }
                    .fontWeight(.semibold)
                    .keyboardShortcut(.defaultAction)
                }
            }
            .onAppear {
                // Initialize local state from parent binding
                localSelectedPubkey = selectedPubkey
                if !initialSearchQuery.isEmpty {
                    searchText = initialSearchQuery
                    autoSelectBestMatch(for: initialSearchQuery)
                }
            }
            .onChange(of: searchText) { _, newQuery in
                autoSelectBestMatch(for: newQuery)
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
        List(selection: $localSelectedPubkey) {
            if filteredAgents.isEmpty {
                emptyStateView
            } else {
                ForEach(filteredAgents, id: \.pubkey) { agent in
                    OnlineAgentRowView(
                        agent: agent,
                        isSelected: localSelectedPubkey == agent.pubkey,
                        onTap: nil,
                        onConfig: {
                            agentToConfig = agent
                        }
                    )
                    .tag(agent.pubkey)
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
                        isSelected: localSelectedPubkey == agent.pubkey,
                        onTap: {
                            selectAgent(agent)
                        },
                        onConfig: {
                            agentToConfig = agent
                        }
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

    private func selectedAgentBar(_ agent: ProjectAgent) -> some View {
        HStack(spacing: 8) {
            Text("@\(agent.name)")
                .font(.subheadline)
                .fontWeight(.medium)
                .foregroundStyle(Color.agentBrand)

            Button(action: { localSelectedPubkey = nil }) {
                Image(systemName: "xmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.borderless)

            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(.bar)
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
        // Toggle selection (single-select)
        if localSelectedPubkey == agent.pubkey {
            localSelectedPubkey = nil
        } else {
            localSelectedPubkey = agent.pubkey
        }
    }

    /// Auto-select the best matching agent for the current query.
    /// Ranking: exact > prefix > substring > subsequence.
    private func autoSelectBestMatch(for query: String) {
        let trimmed = query.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return }

        var best: (score: Int, distance: Int, pubkey: String)?
        let normalizedQuery = trimmed.lowercased()

        for agent in filteredAgents {
            let name = agent.name.lowercased()
            if let rank = matchRank(name: name, query: normalizedQuery) {
                if let current = best {
                    if rank.score < current.score || (rank.score == current.score && rank.distance < current.distance) {
                        best = (rank.score, rank.distance, agent.pubkey)
                    }
                } else {
                    best = (rank.score, rank.distance, agent.pubkey)
                }
            }
        }

        if let bestPubkey = best?.pubkey {
            localSelectedPubkey = bestPubkey
        }
    }

    private func matchRank(name: String, query: String) -> (score: Int, distance: Int)? {
        if name == query {
            return (0, 0)
        }
        if name.hasPrefix(query) {
            return (1, name.count - query.count)
        }
        if let range = name.range(of: query) {
            let startDistance = name.distance(from: name.startIndex, to: range.lowerBound)
            return (2, startDistance)
        }

        // Subsequence match for tolerant matching (e.g. "hr" -> "human-resources")
        var queryIndex = query.startIndex
        var consumed = 0
        for ch in name {
            consumed += 1
            if queryIndex < query.endIndex && ch == query[queryIndex] {
                queryIndex = query.index(after: queryIndex)
                if queryIndex == query.endIndex {
                    return (3, consumed - query.count)
                }
            }
        }
        return nil
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
    let isSelected: Bool
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
                isSelected: isSelected
            )
            .environment(coreManager)

            // Agent info
            VStack(alignment: .leading, spacing: 2) {
                HStack(spacing: 6) {
                    Text(agent.name)
                        .font(.subheadline)
                        .fontWeight(.medium)
                        .foregroundStyle(.primary)

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

                    if let model = agent.model, !model.isEmpty {
                        Text(model)
                            .font(.caption2)
                            .foregroundStyle(.secondary)
                    }
                }

                Text("@\(agent.name)")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }

            Spacer()

            // Selection indicator
            Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                .font(.body)
                .foregroundStyle(isSelected ? Color.accentColor : .secondary)
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
        .padding(.vertical, 4)
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
                tools: ["Read", "Write", "Bash"]
            ),
            ProjectAgent(
                pubkey: "def456ghi789",
                name: "architect",
                isPm: false,
                model: "claude-3-sonnet",
                tools: ["Read", "Edit"]
            ),
            ProjectAgent(
                pubkey: "ghi789jkl012",
                name: "test-writer",
                isPm: false,
                model: nil,
                tools: []
            )
        ],
        projectId: "test-project",
        selectedPubkey: .constant("abc123def456")
    )
    .environment(TenexCoreManager())
}
