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
        if searchText.isEmpty {
            return agents
        }

        let lowercasedSearch = searchText.lowercased()
        return agents.filter { agent in
            agent.name.lowercased().contains(lowercasedSearch)
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
        .background(Color(.systemGray6))
        .foregroundStyle(.blue)
    }

    private var emptyStateView: some View {
        VStack(spacing: 16) {
            Image(systemName: searchText.isEmpty ? "person.2.slash" : "magnifyingglass")
                .font(.system(size: 40))
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
        HStack(spacing: 12) {
            // Tappable main content for selection
            Button(action: onTap) {
                HStack(spacing: 12) {
                    // Agent avatar - uses actual agent pubkey for profile lookup
                    AgentAvatarView(
                        agentName: agent.name,
                        pubkey: agent.pubkey,
                        size: 48,
                        showBorder: false,
                        isSelected: isSelected
                    )
                    .environmentObject(coreManager)

                    // Agent info
                    VStack(alignment: .leading, spacing: 4) {
                        HStack {
                            Text(agent.name)
                                .font(.headline)
                                .foregroundStyle(.primary)

                            if agent.isPm {
                                Text("PM")
                                    .font(.caption2)
                                    .fontWeight(.semibold)
                                    .foregroundStyle(.white)
                                    .padding(.horizontal, 6)
                                    .padding(.vertical, 2)
                                    .background(
                                        Capsule()
                                            .fill(Color.blue)
                                    )
                            }
                        }

                        Text("@\(agent.name)")
                            .font(.subheadline)
                            .foregroundStyle(.secondary)

                        if let model = agent.model, !model.isEmpty {
                            HStack(spacing: 4) {
                                Image(systemName: "cpu")
                                    .font(.caption2)
                                Text(model)
                                    .font(.caption2)
                            }
                            .foregroundStyle(.secondary)
                            .padding(.horizontal, 6)
                            .padding(.vertical, 2)
                            .background(
                                Capsule()
                                    .fill(Color(.systemGray5))
                            )
                        }
                    }

                    Spacer()

                    // Selection indicator
                    Image(systemName: isSelected ? "checkmark.circle.fill" : "circle")
                        .font(.title2)
                        .foregroundStyle(isSelected ? .blue : .secondary)
                }
            }
            .buttonStyle(.plain)

            // Config gear button (separate from selection)
            if let onConfig = onConfig {
                Button(action: onConfig) {
                    Image(systemName: "gearshape")
                        .font(.title3)
                        .foregroundStyle(.secondary)
                        .padding(8)
                        .contentShape(Rectangle())
                }
                .buttonStyle(.plain)
            }
        }
        .padding(.vertical, 8)
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
