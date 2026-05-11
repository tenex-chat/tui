import SwiftUI

/// A sheet for configuring an agent's model and skills.
struct AgentConfigSheet: View {
    // MARK: - Properties

    let agent: ProjectAgent

    // MARK: - Environment

    @Environment(TenexCoreManager.self) var coreManager
    @Environment(\.dismiss) private var dismiss

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

    // MARK: - Body

    var body: some View {
        NavigationStack {
            Group {
                content
            }
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
        Form {
            Section {
                LabeledContent("Name", value: agentDisplayName)
            } header: {
                Text("Agent")
            }

            Section {
                if allModels.isEmpty {
                    Text("No models available")
                        .foregroundStyle(.secondary)
                } else {
                    Picker("Model", selection: $selectedModelIndex) {
                        ForEach(Array(allModels.enumerated()), id: \.offset) { index, model in
                            Text(model).tag(index)
                        }
                    }
                    #if os(iOS)
                    .pickerStyle(.navigationLink)
                    #endif
                }
            } header: {
                Text("Model")
            } footer: {
                Text("Select the AI model for this agent.")
            }

            if !allSkills.isEmpty {
                Section {
                    ForEach(allSkills, id: \.self) { skill in
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
                        }
                        #if os(macOS)
                        .toggleStyle(.checkbox)
                        #endif
                    }
                } header: {
                    HStack {
                        Text("Skills")
                        Spacer()
                        Button("Select All") {
                            selectedSkills = Set(allSkills)
                        }
                        .font(.caption)
                        .buttonStyle(.borderless)
                        .disabled(selectedSkills == Set(allSkills))

                        Text("·")
                            .foregroundStyle(.tertiary)
                            .font(.caption)

                        Button("Clear") {
                            selectedSkills.removeAll()
                        }
                        .font(.caption)
                        .buttonStyle(.borderless)
                        .disabled(selectedSkills.isEmpty)
                    }
                } footer: {
                    Text("\(selectedSkills.count) of \(allSkills.count) selected")
                }
            }
        }
        #if os(macOS)
        .formStyle(.grouped)
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

            try await coreManager.core.updateAgentConfig(
                agentPubkey: agent.pubkey,
                model: selectedModel,
                skills: Array(selectedSkills),
                mcpServers: selectedMcpServers,
                tags: tags
            )

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
        )
    )
    .environment(TenexCoreManager())
}
