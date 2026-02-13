import SwiftUI

/// A sheet for configuring an agent's model and tools.
struct AgentConfigSheet: View {
    // MARK: - Properties

    let agent: OnlineAgentInfo
    let projectId: String

    // MARK: - Environment

    @EnvironmentObject var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss

    // MARK: - State

    @State private var isLoading = true
    @State private var loadError: String?
    @State private var isSaving = false

    // Configuration options loaded from core
    @State private var allModels: [String] = []
    @State private var toolGroups: [ToolGroup] = []

    // User selections
    @State private var selectedModelIndex: Int = 0
    @State private var selectedTools: Set<String> = []

    // MARK: - Body

    var body: some View {
        NavigationStack {
            Group {
                if isLoading {
                    ProgressView("Loading...")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if let error = loadError {
                    VStack(spacing: 16) {
                        Image(systemName: "exclamationmark.triangle")
                            .font(.system(.largeTitle))
                            .foregroundStyle(.orange)
                        Text("Failed to load options")
                            .font(.headline)
                        Text(error)
                            .font(.subheadline)
                            .foregroundStyle(.secondary)
                            .multilineTextAlignment(.center)
                        Button("Retry") {
                            Task { await loadConfigOptions() }
                        }
                    }
                    .padding()
                } else {
                    configContent
                }
            }
            .navigationTitle("Configure \(agent.name)")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                    .disabled(isSaving)
                }

                ToolbarItem(placement: .primaryAction) {
                    Button("Save") {
                        Task { await saveConfig() }
                    }
                    .fontWeight(.semibold)
                    .disabled(isLoading || isSaving)
                }
            }
        }
        .task {
            await loadConfigOptions()
        }
    }

    // MARK: - Config Content

    private var configContent: some View {
        List {
            // Model section
            Section {
                if allModels.isEmpty {
                    Text("No models available")
                        .foregroundStyle(.secondary)
                } else {
                    Picker("Model", selection: $selectedModelIndex) {
                        ForEach(Array(allModels.enumerated()), id: \.offset) { index, model in
                            Text(model)
                                .tag(index)
                        }
                    }
                    .pickerStyle(.menu)
                }
            } header: {
                Text("Model")
            } footer: {
                Text("Select the AI model for this agent")
            }

            // Tools section
            Section {
                if toolGroups.isEmpty {
                    Text("No tools available")
                        .foregroundStyle(.secondary)
                } else {
                    ForEach(Array(toolGroups.enumerated()), id: \.element.id) { index, group in
                        toolGroupRow(group: group, index: index)
                    }
                }
            } header: {
                HStack {
                    Text("Tools")
                    Spacer()
                    Text("\(selectedTools.count) selected")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            } footer: {
                Text("Select the tools available to this agent")
            }
        }
        #if os(iOS)
                .listStyle(.insetGrouped)
                #else
                .listStyle(.inset)
                #endif
    }

    // MARK: - Tool Group Row

    @ViewBuilder
    private func toolGroupRow(group: ToolGroup, index: Int) -> some View {
        if group.tools.count == 1 {
            // Single tool - show as checkbox
            singleToolRow(tool: group.tools[0])
        } else {
            // Group with multiple tools - expandable
            DisclosureGroup(isExpanded: Binding(
                get: { toolGroups[index].isExpanded },
                set: { toolGroups[index].isExpanded = $0 }
            )) {
                ForEach(group.tools, id: \.self) { tool in
                    singleToolRow(tool: tool)
                        .padding(.leading, 16)
                }
            } label: {
                HStack {
                    groupCheckbox(group: group)

                    VStack(alignment: .leading) {
                        Text(group.name)
                            .font(.body)
                        Text("\(group.tools.count) tools")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }
                }
            }
        }
    }

    private func singleToolRow(tool: String) -> some View {
        Button {
            toggleTool(tool)
        } label: {
            HStack {
                Image(systemName: selectedTools.contains(tool) ? "checkmark.square.fill" : "square")
                    .foregroundStyle(selectedTools.contains(tool) ? .blue : .secondary)

                Text(displayName(for: tool))
                    .foregroundStyle(.primary)

                Spacer()
            }
        }
        .buttonStyle(.plain)
    }

    private func groupCheckbox(group: ToolGroup) -> some View {
        Button {
            toggleGroup(group)
        } label: {
            if group.isFullySelected(selectedTools) {
                Image(systemName: "checkmark.square.fill")
                    .foregroundStyle(.blue)
            } else if group.isPartiallySelected(selectedTools) {
                Image(systemName: "minus.square.fill")
                    .foregroundStyle(.blue)
            } else {
                Image(systemName: "square")
                    .foregroundStyle(.secondary)
            }
        }
        .buttonStyle(.plain)
    }

    // MARK: - Display Helpers

    private func displayName(for tool: String) -> String {
        // For MCP tools, show just the method name
        if tool.hasPrefix("mcp__") {
            let parts = tool.split(separator: "__")
            if parts.count >= 3 {
                return String(parts[2])
            }
        }
        return tool
    }

    // MARK: - Actions

    private func toggleTool(_ tool: String) {
        if selectedTools.contains(tool) {
            selectedTools.remove(tool)
        } else {
            selectedTools.insert(tool)
        }
    }

    private func toggleGroup(_ group: ToolGroup) {
        if group.isFullySelected(selectedTools) {
            // Deselect all
            for tool in group.tools {
                selectedTools.remove(tool)
            }
        } else {
            // Select all
            for tool in group.tools {
                selectedTools.insert(tool)
            }
        }
    }

    private func loadConfigOptions() async {
        isLoading = true
        loadError = nil

        do {
            let options = try await coreManager.safeCore.getProjectConfigOptions(projectId: projectId)
            allModels = options.allModels
            toolGroups = ToolGroup.buildGroups(from: options.allTools)

            // Set initial selections from agent
            if let currentModel = agent.model,
               let modelIndex = allModels.firstIndex(of: currentModel) {
                selectedModelIndex = modelIndex
            }
            selectedTools = Set(agent.tools)

            isLoading = false
        } catch {
            loadError = error.localizedDescription
            isLoading = false
        }
    }

    private func saveConfig() async {
        isSaving = true

        do {
            let selectedModel = allModels.isEmpty ? nil : allModels[selectedModelIndex]

            try await coreManager.safeCore.updateAgentConfig(
                projectId: projectId,
                agentPubkey: agent.pubkey,
                model: selectedModel,
                tools: Array(selectedTools)
            )

            dismiss()
        } catch {
            // Could show an alert here, but for now just log
            print("Failed to update agent config: \(error)")
            isSaving = false
        }
    }
}

// MARK: - Preview

#Preview {
    AgentConfigSheet(
        agent: OnlineAgentInfo(
            pubkey: "abc123",
            name: "claude-code",
            isPm: true,
            model: "claude-3-opus",
            tools: ["Read", "Write", "Bash"]
        ),
        projectId: "test-project"
    )
    .environmentObject(TenexCoreManager())
}
