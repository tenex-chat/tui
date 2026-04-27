import SwiftUI

/// A sheet for configuring an agent's model, skills, and role.
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
    @State private var allSkills: [String] = []
    @State private var allMcpServers: [String] = []

    // User selections
    @State private var selectedModelIndex: Int = 0
    @State private var selectedSkills: Set<String> = []
    @State private var selectedMcpServers: Set<String> = []
    @State private var isPm: Bool = false

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

                if !allSkills.isEmpty {
                    togglePanel(
                        title: "Skills",
                        subtitle: "Select the skills available to this agent.",
                        items: allSkills,
                        selection: $selectedSkills
                    )
                }

                if !allMcpServers.isEmpty {
                    togglePanel(
                        title: "MCP Servers",
                        subtitle: "Select the MCP servers available to this agent.",
                        items: allMcpServers,
                        selection: $selectedMcpServers
                    )
                }

                GlassPanel(
                    title: "Options",
                    subtitle: "Applies wherever this agent is used."
                ) {
                    VStack(alignment: .leading, spacing: 10) {
                        Toggle("Set as Project Manager", isOn: $isPm)
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
            subtitle: "Configure shared model, skills, and role."
        ) {
            HStack(spacing: 10) {
                statPill(label: "Name", value: agent.name)
                statPill(label: "Model", value: selectedModelLabel)
                statPill(label: "Skills", value: "\(selectedSkills.count)")
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

    // MARK: - Toggle Panel

    private func togglePanel(
        title: String,
        subtitle: String,
        items: [String],
        selection: Binding<Set<String>>
    ) -> some View {
        GlassPanel(title: title, subtitle: subtitle) {
            VStack(alignment: .leading, spacing: 10) {
                HStack {
                    Text("\(selection.wrappedValue.count) selected")
                        .font(.caption)
                        .foregroundStyle(.secondary)

                    Spacer(minLength: 0)

                    Button("Select All") {
                        selection.wrappedValue = Set(items)
                    }
                    .buttonStyle(.plain)
                    .disabled(selection.wrappedValue == Set(items))

                    Text("•")
                        .foregroundStyle(.tertiary)

                    Button("Clear") {
                        selection.wrappedValue.removeAll()
                    }
                    .buttonStyle(.plain)
                    .disabled(selection.wrappedValue.isEmpty)
                }

                VStack(alignment: .leading, spacing: 0) {
                    ForEach(Array(items.enumerated()), id: \.element) { offset, item in
                        Toggle(isOn: Binding(
                            get: { selection.wrappedValue.contains(item) },
                            set: { isSelected in
                                if isSelected {
                                    selection.wrappedValue.insert(item)
                                } else {
                                    selection.wrappedValue.remove(item)
                                }
                            }
                        )) {
                            Text(item)
                                .foregroundStyle(.primary)
                        }
                        #if os(macOS)
                        .toggleStyle(.checkbox)
                        #endif
                        if offset < items.count - 1 {
                            Divider()
                                .opacity(0.30)
                        }
                    }
                }
                .padding(.horizontal, 10)
                .padding(.vertical, 8)
                #if os(macOS)
                .background(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .fill(Color(nsColor: .windowBackgroundColor))
                        .overlay(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .stroke(Color(nsColor: .separatorColor), lineWidth: 1)
                        )
                )
                #else
                .background(
                    RoundedRectangle(cornerRadius: 12, style: .continuous)
                        .fill(Color.systemBackground.opacity(reduceTransparency ? 1 : 0.36))
                        .overlay(
                            RoundedRectangle(cornerRadius: 12, style: .continuous)
                                .stroke(.white.opacity(reduceTransparency ? 0.06 : 0.14), lineWidth: 1)
                        )
                )
                #endif
            }
        }
    }

    // MARK: - Actions

    private func loadConfigOptions() async {
        isLoading = true
        loadError = nil
        saveError = nil

        do {
            let options = try await coreManager.safeCore.getAgentConfigOptions(
                projectId: projectId,
                agentPubkey: agent.pubkey
            )
            allModels = options.allModels
            allSkills = options.allSkills
            allMcpServers = options.allMcpServers

            let configs = try await coreManager.safeCore.getAgentConfigs(backendPubkey: agent.backendPubkey)
            if let config = configs.first(where: { $0.pubkey == agent.pubkey }) {
                if let activeModel = config.activeModel,
                   let modelIndex = allModels.firstIndex(of: activeModel) {
                    selectedModelIndex = modelIndex
                }
                selectedSkills = Set(config.activeSkills)
                selectedMcpServers = Set(config.activeMcps)
            }
            isPm = agent.isPm

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

            try await coreManager.safeCore.updateAgentConfig(
                agentPubkey: agent.pubkey,
                model: selectedModel,
                skills: Array(selectedSkills),
                mcpServers: Array(selectedMcpServers),
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
            isOnline: true
        ),
        projectId: "test-project"
    )
    .environment(TenexCoreManager())
}
