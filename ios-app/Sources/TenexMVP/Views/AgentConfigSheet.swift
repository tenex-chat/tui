import SwiftUI

private struct IndexedToolGroup: Identifiable {
    let index: Int
    let group: ToolGroup

    var id: UUID { group.id }
}

/// A sheet for configuring an agent's model and tools.
struct AgentConfigSheet: View {
    // MARK: - Properties

    let agent: ProjectAgent
    let projectId: String

    // MARK: - Environment

    @Environment(TenexCoreManager.self) var coreManager
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceTransparency) private var reduceTransparency

    // MARK: - State

    @State private var isLoading = true
    @State private var loadError: String?
    @State private var saveError: String?
    @State private var isSaving = false

    // Configuration options loaded from core
    @State private var allModels: [String] = []
    @State private var toolGroups: [ToolGroup] = []

    // User selections
    @State private var selectedModelIndex: Int = 0
    @State private var selectedTools: Set<String> = []
    @State private var isPm: Bool = false
    @State private var saveGlobally: Bool = false
    @State private var toolSearchText = ""

    // MARK: - Body

    var body: some View {
        NavigationStack {
            Group {
                content
            }
            .background(backgroundView)
            .navigationTitle("Configure \(agent.name)")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .cancellationAction) {
                    Button("Cancel") {
                        dismiss()
                    }
                    .disabled(isSaving)
                }

                ToolbarItem(placement: .primaryAction) {
                    Button {
                        Task { await saveConfig() }
                    } label: {
                        if isSaving {
                            ProgressView()
                                .controlSize(.small)
                        } else {
                            Text("Save")
                                .fontWeight(.semibold)
                        }
                    }
                    .disabled(isLoading || isSaving || loadError != nil)
                }
            }
        }
        .alert(
            "Unable to Save Configuration",
            isPresented: Binding(
                get: { saveError != nil },
                set: { if !$0 { saveError = nil } }
            )
        ) {
            Button("OK", role: .cancel) {
                saveError = nil
            }
        } message: {
            Text(saveError ?? "Unknown error")
        }
        .task {
            await loadConfigOptions()
        }
        #if os(iOS)
        .tenexModalPresentation(detents: [.medium, .large])
        #else
        .frame(minWidth: 440, idealWidth: 500, minHeight: 420, idealHeight: 520)
        #endif
    }

    // MARK: - Config Content

    @ViewBuilder
    private var content: some View {
        if isLoading {
            ProgressView("Loading configuration…")
                .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else if let loadError {
            VStack(spacing: 16) {
                Image(systemName: "exclamationmark.triangle")
                    .font(.system(.largeTitle))
                    .foregroundStyle(Color.skillBrand)
                Text("Failed to load options")
                    .font(.headline)
                Text(loadError)
                    .font(.subheadline)
                    .foregroundStyle(.secondary)
                    .multilineTextAlignment(.center)
                Button("Retry") {
                    Task { await loadConfigOptions() }
                }
                .adaptiveGlassButtonStyle()
            }
            .padding(24)
            .frame(maxWidth: .infinity, maxHeight: .infinity)
        } else {
            configContent
        }
    }

    private var configContent: some View {
        ScrollView {
            VStack(alignment: .leading, spacing: 12) {
                summaryCard

                GlassPanel(
                    title: "Model",
                    subtitle: "Select the AI model for this agent."
                ) {
                    if allModels.isEmpty {
                        Text("No models available")
                            .foregroundStyle(.secondary)
                            .padding(.vertical, 6)
                    } else {
                        HStack(spacing: 12) {
                            Text("Model")
                                .font(.headline)
                            Spacer(minLength: 0)
                            Picker("Model", selection: $selectedModelIndex) {
                                ForEach(Array(allModels.enumerated()), id: \.offset) { index, model in
                                    Text(model)
                                        .tag(index)
                                }
                            }
                            .pickerStyle(.menu)
                            .labelsHidden()
                            .frame(maxWidth: 320)
                        }
                    }
                }

                GlassPanel(
                    title: "Tools",
                    subtitle: "Select the tools available to this agent."
                ) {
                    VStack(alignment: .leading, spacing: 10) {
                        HStack {
                            Text("\(selectedTools.count) selected")
                                .font(.caption)
                                .foregroundStyle(.secondary)

                            Spacer(minLength: 0)

                            if !toolGroups.isEmpty {
                                Button("Select All") {
                                    selectedTools = Set(toolGroups.flatMap(\.tools))
                                }
                                .buttonStyle(.plain)
                                .disabled(selectedTools == Set(toolGroups.flatMap(\.tools)))

                                Text("•")
                                    .foregroundStyle(.tertiary)

                                Button("Clear") {
                                    selectedTools.removeAll()
                                }
                                .buttonStyle(.plain)
                                .disabled(selectedTools.isEmpty)
                            }
                        }

                        if toolGroups.isEmpty {
                            Text("No tools available")
                                .foregroundStyle(.secondary)
                                .padding(.vertical, 6)
                        } else {
                            if toolGroups.count > 8 {
                                TextField("Filter tools", text: $toolSearchText)
                                    .textFieldStyle(.roundedBorder)
                                    .autocorrectionDisabled()
                                    #if os(iOS)
                                    .textInputAutocapitalization(.never)
                                    #endif
                            }

                            if visibleToolGroups.isEmpty {
                                ContentUnavailableView(
                                    "No Matching Tools",
                                    systemImage: "magnifyingglass",
                                    description: Text("Try a different filter.")
                                )
                                .frame(maxWidth: .infinity)
                                .padding(.vertical, 8)
                            } else {
                                VStack(alignment: .leading, spacing: 0) {
                                    ForEach(Array(visibleToolGroups.enumerated()), id: \.element.id) { offset, entry in
                                        toolGroupRow(group: entry.group, index: entry.index)
                                        if offset < visibleToolGroups.count - 1 {
                                            Divider()
                                                .opacity(0.30)
                                        }
                                    }
                                }
                                .padding(.horizontal, 10)
                                .padding(.vertical, 8)
                                .background(
                                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                                        .fill(Color.systemBackground.opacity(reduceTransparency ? 1 : 0.36))
                                        .overlay(
                                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                                .stroke(.white.opacity(reduceTransparency ? 0.06 : 0.14), lineWidth: 1)
                                        )
                                )
                            }
                        }
                    }
                }

                GlassPanel(
                    title: "Options",
                    subtitle: "Apply role and scope changes."
                ) {
                    VStack(alignment: .leading, spacing: 10) {
                        Toggle("Set as Project Manager", isOn: $isPm)
                        Divider()
                            .opacity(0.30)
                        Toggle("Change all projects this agent is in", isOn: $saveGlobally)
                    }
                }
            }
            .padding(.horizontal, 18)
            .padding(.top, 16)
            .padding(.bottom, 20)
        }
    }

    private var summaryCard: some View {
        GlassPanel(
            title: "Agent",
            subtitle: "Configure model, tools, and role scope."
        ) {
            HStack(spacing: 10) {
                statPill(label: "Name", value: agent.name)
                statPill(label: "Model", value: selectedModelLabel)
                statPill(label: "Tools", value: "\(selectedTools.count)")
            }
            .frame(maxWidth: .infinity, alignment: .leading)
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

    private var selectedModelLabel: String {
        guard allModels.indices.contains(selectedModelIndex) else {
            return "Default"
        }
        return allModels[selectedModelIndex]
    }

    private var visibleToolGroups: [IndexedToolGroup] {
        let indexedGroups = toolGroups.enumerated().map { IndexedToolGroup(index: $0.offset, group: $0.element) }
        let query = toolSearchText.trimmingCharacters(in: .whitespacesAndNewlines).lowercased()
        guard !query.isEmpty else { return indexedGroups }

        return indexedGroups.filter { entry in
            entry.group.name.lowercased().contains(query)
                || entry.group.tools.contains { tool in
                    tool.lowercased().contains(query) || displayName(for: tool).lowercased().contains(query)
                }
        }
    }

    private var backgroundView: some View {
        LinearGradient(
            colors: [
                Color.agentBrand.opacity(reduceTransparency ? 0.03 : 0.10),
                Color.systemGroupedBackground,
                Color.systemGroupedBackground
            ],
            startPoint: .topLeading,
            endPoint: .bottomTrailing
        )
        .ignoresSafeArea()
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
                HStack(spacing: 8) {
                    groupCheckbox(group: group)

                    VStack(alignment: .leading, spacing: 2) {
                        Text(group.name)
                            .font(.body.weight(.medium))
                        Text("\(group.tools.count) tools")
                            .font(.caption)
                            .foregroundStyle(.secondary)
                    }

                    Spacer(minLength: 0)

                    Text("\(group.tools.filter { selectedTools.contains($0) }.count)/\(group.tools.count)")
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }
            }
            .tint(Color.agentBrand)
        }
    }

    private func singleToolRow(tool: String) -> some View {
        Toggle(isOn: Binding(
            get: { selectedTools.contains(tool) },
            set: { isSelected in
                if isSelected {
                    selectedTools.insert(tool)
                } else {
                    selectedTools.remove(tool)
                }
            }
        )) {
            HStack(spacing: 8) {
                Text(displayName(for: tool))
                    .foregroundStyle(.primary)
                Spacer()
            }
        }
        #if os(macOS)
        .toggleStyle(.checkbox)
        #endif
    }

    private func groupCheckbox(group: ToolGroup) -> some View {
        Button {
            toggleGroup(group)
        } label: {
            Image(systemName: groupSelectionIcon(for: group))
                .foregroundStyle(group.isFullySelected(selectedTools) || group.isPartiallySelected(selectedTools) ? Color.agentBrand : .secondary)
        }
        .buttonStyle(.plain)
    }

    // MARK: - Display Helpers

    private func groupSelectionIcon(for group: ToolGroup) -> String {
        if group.isFullySelected(selectedTools) {
            return "checkmark.square.fill"
        }
        if group.isPartiallySelected(selectedTools) {
            return "minus.square.fill"
        }
        return "square"
    }

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
        saveError = nil

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
            isPm = agent.isPm
            toolSearchText = ""

            isLoading = false
        } catch {
            loadError = error.localizedDescription
            isLoading = false
        }
    }

    private func saveConfig() async {
        isSaving = true
        saveError = nil

        do {
            let selectedModel = allModels.isEmpty ? nil : allModels[selectedModelIndex]
            let tags: [String] = isPm ? ["pm"] : []

            if saveGlobally {
                try await coreManager.safeCore.updateGlobalAgentConfig(
                    agentPubkey: agent.pubkey,
                    model: selectedModel,
                    tools: Array(selectedTools),
                    tags: tags
                )
            } else {
                try await coreManager.safeCore.updateAgentConfig(
                    projectId: projectId,
                    agentPubkey: agent.pubkey,
                    model: selectedModel,
                    tools: Array(selectedTools),
                    tags: tags
                )
            }

            dismiss()
        } catch {
            saveError = error.localizedDescription
            isSaving = false
        }
    }
}

// MARK: - Preview

#Preview {
    AgentConfigSheet(
        agent: ProjectAgent(
            pubkey: "abc123",
            name: "claude-code",
            isPm: true,
            model: "claude-3-opus",
            tools: ["Read", "Write", "Bash"]
        ),
        projectId: "test-project"
    )
    .environment(TenexCoreManager())
}
