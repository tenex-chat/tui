import SwiftUI

/// A sheet for configuring an agent's model and skills.
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

    // User selections
    @State private var selectedModelIndex: Int = 0
    @State private var selectedSkills: Set<String> = []
    @State private var selectedMcpServers: [String] = []
    @State private var allSkills: [String] = []
    @State private var saveGlobally: Bool = false

    // MARK: - Body

    var body: some View {
        NavigationStack {
            Group {
                content
            }
            .background(backgroundView)
            .navigationTitle("Configure \(agentDisplayName)")
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

                if !allSkills.isEmpty {
                    GlassPanel(
                        title: "Skills",
                        subtitle: "Select the skills available to this agent."
                    ) {
                        VStack(alignment: .leading, spacing: 10) {
                            HStack {
                                Text("\(selectedSkills.count) selected")
                                    .font(.caption)
                                    .foregroundStyle(.secondary)

                                Spacer(minLength: 0)

                                Button("Select All") {
                                    selectedSkills = Set(allSkills)
                                }
                                .buttonStyle(.plain)
                                .disabled(selectedSkills == Set(allSkills))

                                Text("•")
                                    .foregroundStyle(.tertiary)

                                Button("Clear") {
                                    selectedSkills.removeAll()
                                }
                                .buttonStyle(.plain)
                                .disabled(selectedSkills.isEmpty)
                            }

                            VStack(alignment: .leading, spacing: 0) {
                                ForEach(Array(allSkills.enumerated()), id: \.element) { offset, skill in
                                    Toggle(isOn: Binding(
                                        get: { selectedSkills.contains(skill) },
                                        set: { isSelected in
                                            if isSelected {
                                                selectedSkills.insert(skill)
                                            } else {
                                                selectedSkills.remove(skill)
                                            }
                                        }
                                    )) {
                                        Text(skill)
                                            .foregroundStyle(.primary)
                                    }
                                    #if os(macOS)
                                    .toggleStyle(.checkbox)
                                    #endif
                                    if offset < allSkills.count - 1 {
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

                GlassPanel(
                    title: "Save Scope",
                    subtitle: "Apply these configuration changes to this agent."
                ) {
                    VStack(alignment: .leading, spacing: 10) {
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
            subtitle: "Configure model and skills."
        ) {
            HStack(spacing: 10) {
                statPill(label: "Name", value: agentDisplayName)
                statPill(label: "Model", value: selectedModelLabel)
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
        #if os(macOS)
        .background(Color(nsColor: .controlColor), in: Capsule())
        #else
        .background(Color.systemBackground.opacity(reduceTransparency ? 1 : 0.55), in: Capsule())
        #endif
    }

    private var selectedModelLabel: String {
        guard allModels.indices.contains(selectedModelIndex) else {
            return "Default"
        }
        return allModels[selectedModelIndex]
    }

    private var backgroundView: some View {
        #if os(macOS)
        Color(nsColor: .windowBackgroundColor)
            .ignoresSafeArea()
        #else
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
        #endif
    }

    private var agentDisplayName: String {
        AgentDisplayName.resolve(pubkey: agent.pubkey, coreManager: coreManager)
    }

    // MARK: - Actions

    private func loadConfigOptions() async {
        isLoading = true
        loadError = nil
        saveError = nil

        do {
            guard let config = try await coreManager.core.getAgentConfig(agentPubkey: agent.pubkey) else {
                allModels = []
                allSkills = []
                selectedSkills = []
                selectedMcpServers = []
                selectedModelIndex = 0
                loadError = "No kind:0 configuration has been received for this agent yet."
                isLoading = false
                return
            }

            // Available models come from the agent's backend's kind:24011
            // inventory, not from kind:0 (which now carries only the
            // currently-active model).
            allModels = try await coreManager.core.getModelsForAgent(agentPubkey: agent.pubkey)
            allSkills = config.skills

            if let currentModel = config.activeModel,
               let modelIndex = allModels.firstIndex(of: currentModel) {
                selectedModelIndex = modelIndex
            } else {
                selectedModelIndex = 0
            }
            selectedSkills = Set(config.activeSkills)
            selectedMcpServers = config.activeMcps

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
            let tags: [String] = []

            if saveGlobally {
                try await coreManager.core.updateGlobalAgentConfig(
                    agentPubkey: agent.pubkey,
                    model: selectedModel,
                    skills: Array(selectedSkills),
                    mcpServers: selectedMcpServers,
                    tags: tags
                )
            } else {
                try await coreManager.core.updateAgentConfig(
                    projectId: projectId,
                    agentPubkey: agent.pubkey,
                    model: selectedModel,
                    skills: Array(selectedSkills),
                    mcpServers: selectedMcpServers,
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
            backendPubkey: "backend",
            isPm: true,
            isOnline: true,
            model: "claude-3-opus",
            tools: ["Read", "Write", "Bash"],
            skills: [],
            mcpServers: []
        ),
        projectId: "test-project"
    )
    .environment(TenexCoreManager())
}
