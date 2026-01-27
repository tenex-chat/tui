import SwiftUI

/// A sheet for selecting an agent to p-tag in a message.
/// Supports search and single-select with proper cancel semantics.
struct AgentSelectorSheet: View {
    // MARK: - Properties

    /// Available agents to choose from
    let agents: [AgentInfo]

    /// Currently selected agent pubkey (binding for single-select)
    @Binding var selectedPubkey: String?

    /// Callback when selection is confirmed
    var onDone: (() -> Void)?

    // MARK: - Environment

    @Environment(\.dismiss) private var dismiss

    // MARK: - State

    /// Local copy of selection - only committed on Done, discarded on Cancel
    @State private var localSelectedPubkey: String?
    @State private var searchText = ""

    // MARK: - Computed

    private var filteredAgents: [AgentInfo] {
        if searchText.isEmpty {
            return agents
        }

        let lowercasedSearch = searchText.lowercased()
        return agents.filter { agent in
            agent.name.lowercased().contains(lowercasedSearch) ||
            agent.dTag.lowercased().contains(lowercasedSearch) ||
            agent.role.lowercased().contains(lowercasedSearch) ||
            agent.description.lowercased().contains(lowercasedSearch)
        }
    }

    private var selectedAgent: AgentInfo? {
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
                        ForEach(filteredAgents, id: \.id) { agent in
                            AgentRowView(
                                agent: agent,
                                isSelected: localSelectedPubkey == agent.pubkey
                            ) {
                                selectAgent(agent)
                            }
                        }
                    }
                }
                .listStyle(.plain)
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
        }
    }

    // MARK: - Subviews

    private func selectedAgentBar(_ agent: AgentInfo) -> some View {
        HStack(spacing: 8) {
            Text("@\(agent.dTag)")
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
                Text("No Agents Available")
                    .font(.headline)
                Text("This project has no agents configured.")
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

    private func selectAgent(_ agent: AgentInfo) {
        // Toggle selection (single-select)
        if localSelectedPubkey == agent.pubkey {
            localSelectedPubkey = nil
        } else {
            localSelectedPubkey = agent.pubkey
        }
    }
}

// MARK: - Agent Row View

struct AgentRowView: View {
    let agent: AgentInfo
    let isSelected: Bool
    let onTap: () -> Void

    var body: some View {
        Button(action: onTap) {
            HStack(spacing: 12) {
                // Agent avatar
                agentAvatar

                // Agent info
                VStack(alignment: .leading, spacing: 4) {
                    HStack {
                        Text(agent.name)
                            .font(.headline)
                            .foregroundStyle(.primary)

                        if !agent.role.isEmpty {
                            Text("•")
                                .foregroundStyle(.secondary)
                            Text(agent.role)
                                .font(.subheadline)
                                .foregroundStyle(.secondary)
                        }
                    }

                    Text("@\(agent.dTag)")
                        .font(.subheadline)
                        .foregroundStyle(.secondary)

                    if !agent.description.isEmpty {
                        Text(agent.description)
                            .font(.caption)
                            .foregroundStyle(.tertiary)
                            .lineLimit(2)
                    }

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
            .padding(.vertical, 8)
        }
        .buttonStyle(.plain)
    }

    private var agentAvatar: some View {
        Group {
            if let pictureUrl = agent.picture, let url = URL(string: pictureUrl) {
                AsyncImage(url: url) { image in
                    image
                        .resizable()
                        .aspectRatio(contentMode: .fill)
                } placeholder: {
                    placeholderAvatar
                }
            } else {
                placeholderAvatar
            }
        }
        .frame(width: 48, height: 48)
        .clipShape(Circle())
        .overlay(
            Circle()
                .strokeBorder(isSelected ? Color.blue : Color.clear, lineWidth: 2)
        )
    }

    private var placeholderAvatar: some View {
        Circle()
            .fill(agentColor.gradient)
            .overlay {
                Text(String(agent.name.prefix(1)).uppercased())
                    .font(.title3)
                    .fontWeight(.semibold)
                    .foregroundStyle(.white)
            }
    }

    private var agentColor: Color {
        let colors: [Color] = [.blue, .purple, .orange, .green, .pink, .indigo, .teal, .cyan]
        let hash = agent.pubkey.hashValue
        return colors[abs(hash) % colors.count]
    }
}

// MARK: - Preview

#Preview {
    AgentSelectorSheet(
        agents: [
            AgentInfo(
                id: "1",
                pubkey: "abc123",
                dTag: "architect",
                name: "Architect Agent",
                description: "Designs system architecture and makes high-level decisions",
                role: "Developer",
                picture: nil,
                model: "claude-3-opus"
            ),
            AgentInfo(
                id: "2",
                pubkey: "def456",
                dTag: "code-reviewer",
                name: "Code Review Agent",
                description: "Reviews code for quality and best practices",
                role: "Reviewer",
                picture: nil,
                model: "claude-3-sonnet"
            ),
            AgentInfo(
                id: "3",
                pubkey: "ghi789",
                dTag: "test-writer",
                name: "Test Writer",
                description: "Writes comprehensive test suites",
                role: "QA",
                picture: nil,
                model: nil
            )
        ],
        selectedPubkey: .constant("abc123")
    )
}
