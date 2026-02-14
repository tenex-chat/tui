import SwiftUI

/// A sheet for selecting an agent to p-tag in a message.
/// Uses OnlineAgentInfo from project status (kind:24010) which contains
/// actual agent instance pubkeys for proper profile picture lookup.
struct AgentSelectorSheet: View {
    // MARK: - Properties

    /// Available agents to choose from (from project status)
    let agents: [OnlineAgentInfo]

    /// Project ID for agent configuration
    let projectId: String

    /// Currently selected agent pubkey (binding for single-select)
    @Binding var selectedPubkey: String?

    /// Callback when selection is confirmed
    var onDone: (() -> Void)?

    // MARK: - Environment

    @EnvironmentObject var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

    // MARK: - State

    /// Local copy of selection - only committed on Done, discarded on Cancel
    @State private var localSelectedPubkey: String?
    @State private var searchText = ""
    @State private var agentToConfig: OnlineAgentInfo?

    // MARK: - Computed

    private var filteredAgents: [OnlineAgentInfo] {
        let filtered: [OnlineAgentInfo]
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

    private var selectedAgent: OnlineAgentInfo? {
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
                        }
                    }
                }
                .listStyle(.plain)
                .contentMargins(.top, 0, for: .scrollContent)
            }
            .searchable(text: $searchText, prompt: "Search agents...")
            .navigationTitle("Select Agent")
            .navigationBarTitleDisplayMode(.inline)
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
                }
            }
            .onAppear {
                // Initialize local state from parent binding
                localSelectedPubkey = selectedPubkey
            }
            .sheet(item: $agentToConfig) { agent in
                AgentConfigSheet(agent: agent, projectId: projectId)
                    .environmentObject(coreManager)
            }
        }
    }

    // MARK: - Subviews

    private func selectedAgentBar(_ agent: OnlineAgentInfo) -> some View {
        HStack(spacing: 8) {
            Text("@\(agent.name)")
                .font(.subheadline)
                .fontWeight(.medium)

            Button(action: { localSelectedPubkey = nil }) {
                Image(systemName: "xmark.circle.fill")
                    .font(.caption)
                    .foregroundStyle(.secondary)
            }
            .buttonStyle(.plain)

            Spacer()
        }
        .padding(.horizontal, 16)
        .padding(.vertical, 12)
        .background(Color.systemGray6)
        .foregroundStyle(.blue)
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

    private func selectAgent(_ agent: OnlineAgentInfo) {
        // Toggle selection (single-select)
        if localSelectedPubkey == agent.pubkey {
            localSelectedPubkey = nil
        } else {
            localSelectedPubkey = agent.pubkey
        }
    }
}

// MARK: - OnlineAgentInfo Identifiable

extension OnlineAgentInfo: Identifiable {
    public var id: String { pubkey }
}

// MARK: - Online Agent Row View

struct OnlineAgentRowView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    let agent: OnlineAgentInfo
    let isSelected: Bool
    let onTap: () -> Void
    var onConfig: (() -> Void)?

    var body: some View {
        HStack(spacing: 10) {
            // Tappable main content for selection
            Button(action: onTap) {
                HStack(spacing: 10) {
                    // Agent avatar - uses actual agent pubkey for profile lookup
                    AgentAvatarView(
                        agentName: agent.name,
                        pubkey: agent.pubkey,
                        size: 36,
                        showBorder: false,
                        isSelected: isSelected
                    )
                    .environmentObject(coreManager)

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
                                            .fill(Color.blue)
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
                        .foregroundStyle(isSelected ? .blue : .secondary)
                }
            }
            .buttonStyle(.plain)

            // Config gear button (separate from selection)
            if let onConfig = onConfig {
                Button(action: onConfig) {
                    Image(systemName: "gearshape")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)
                        .frame(minWidth: 44, minHeight: 44)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.vertical, 4)
    }
}

// MARK: - Preview

#Preview {
    AgentSelectorSheet(
        agents: [
            OnlineAgentInfo(
                pubkey: "abc123def456",
                name: "claude-code",
                isPm: true,
                model: "claude-3-opus",
                tools: ["Read", "Write", "Bash"]
            ),
            OnlineAgentInfo(
                pubkey: "def456ghi789",
                name: "architect",
                isPm: false,
                model: "claude-3-sonnet",
                tools: ["Read", "Edit"]
            ),
            OnlineAgentInfo(
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
    .environmentObject(TenexCoreManager())
}
