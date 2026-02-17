import SwiftUI

/// AI Settings view for configuring audio notifications
struct AISettingsView: View {
    @EnvironmentObject var coreManager: TenexCoreManager
    @Environment(\.dismiss) private var dismiss
    @Environment(\.accessibilityReduceTransparency) var reduceTransparency

    // API Key states
    @State private var elevenLabsKeyInput = ""
    @State private var openRouterKeyInput = ""
    @State private var hasElevenLabsKey = false
    @State private var hasOpenRouterKey = false
    @State private var isEditingElevenLabsKey = false
    @State private var isEditingOpenRouterKey = false

    // Settings states
    @State private var audioEnabled = false
    @State private var audioPrompt = ""
    @State private var selectedModel: String?
    @State private var selectedVoiceIds: Set<String> = []

    // Available options
    @State private var availableVoices: [VoiceInfo] = []
    @State private var availableModels: [ModelInfo] = []

    // Loading states
    @State private var isLoadingSettings = true
    @State private var isLoadingVoices = false
    @State private var isLoadingModels = false
    @State private var isSavingApiKey = false

    // Sheet states
    @State private var showVoiceSelector = false
    @State private var showModelSelector = false

    // Error states
    @State private var errorMessage: String?
    @State private var showError = false

    var body: some View {
        NavigationStack {
            Form {
                // API Keys Section
                Section {
                    apiKeyRow(
                        title: "ElevenLabs API Key",
                        description: "For voice synthesis",
                        hasKey: hasElevenLabsKey,
                        isEditing: $isEditingElevenLabsKey,
                        keyInput: $elevenLabsKeyInput,
                        onSave: saveElevenLabsKey,
                        onDelete: deleteElevenLabsKey
                    )

                    apiKeyRow(
                        title: "OpenRouter API Key",
                        description: "For text message LLM",
                        hasKey: hasOpenRouterKey,
                        isEditing: $isEditingOpenRouterKey,
                        keyInput: $openRouterKeyInput,
                        onSave: saveOpenRouterKey,
                        onDelete: deleteOpenRouterKey
                    )
                } header: {
                    Text("API Keys")
                } footer: {
                    Text("API keys are stored securely in your device's keychain.")
                }

                // Audio Settings Section (only show if both keys are configured)
                if hasElevenLabsKey && hasOpenRouterKey {
                    Section {
                        Toggle("Enable Audio Notifications", isOn: $audioEnabled)
                            .onChange(of: audioEnabled) { _, newValue in
                                saveAudioEnabled(newValue)
                            }
                    } header: {
                        Text("Audio Notifications")
                    }

                    // Voice & Model selection rows that open sheets
                    Section("Voice Configuration") {
                        Button {
                            if availableVoices.isEmpty {
                                fetchVoices()
                            }
                            showVoiceSelector = true
                        } label: {
                            HStack {
                                Text("Voice Whitelist")
                                    .foregroundStyle(.primary)
                                Spacer()
                                if selectedVoiceIds.isEmpty {
                                    Text("None selected")
                                        .foregroundStyle(.secondary)
                                } else {
                                    Text("\(selectedVoiceIds.count) selected")
                                        .foregroundStyle(.secondary)
                                }
                                Image(systemName: "chevron.right")
                                    .font(.caption)
                                    .foregroundStyle(.tertiary)
                            }
                        }

                        Button {
                            if availableModels.isEmpty {
                                fetchModels()
                            }
                            showModelSelector = true
                        } label: {
                            HStack {
                                Text("Text Message Model")
                                    .foregroundStyle(.primary)
                                Spacer()
                                if let model = selectedModel,
                                   let modelInfo = availableModels.first(where: { $0.id == model }) {
                                    Text(modelInfo.name ?? model)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                } else if let model = selectedModel {
                                    Text(model)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(1)
                                } else {
                                    Text("Not selected")
                                        .foregroundStyle(.secondary)
                                }
                                Image(systemName: "chevron.right")
                                    .font(.caption)
                                    .foregroundStyle(.tertiary)
                            }
                        }
                    }

                    // Audio Prompt Section
                    Section {
                        TextEditor(text: $audioPrompt)
                            .frame(minHeight: 80)
                            .font(.callout)
                        HStack {
                            Button("Save Prompt") {
                                saveAudioPrompt()
                            }
                            .buttonStyle(.borderedProminent)
                            .controlSize(.small)
                            .disabled(audioPrompt.isEmpty)
                            Spacer()
                            Button("Reset to Default", role: .destructive) {
                                resetAudioPrompt()
                            }
                            .controlSize(.small)
                        }
                    } header: {
                        Text("Audio Prompt")
                    } footer: {
                        Text("Instructions for how the LLM should process text for speech synthesis.")
                    }

                    // Debug Section
                    Section("Debug") {
                        NavigationLink {
                            AudioNotificationsLogView()
                                .environmentObject(coreManager)
                        } label: {
                            Label("Audio Debug Log", systemImage: "list.bullet.rectangle")
                        }
                    }
                }
            }
            .formStyle(.grouped)
            .navigationTitle("AI Settings")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") {
                        dismiss()
                    }
                }
            }
            .onAppear {
                loadSettings()
            }
            .alert("Error", isPresented: $showError) {
                Button("OK") {
                    errorMessage = nil
                }
            } message: {
                if let error = errorMessage {
                    Text(error)
                }
            }
            .sheet(isPresented: $showVoiceSelector) {
                VoiceSelectorSheet(
                    availableVoices: $availableVoices,
                    selectedVoiceIds: $selectedVoiceIds,
                    isLoading: $isLoadingVoices,
                    onToggle: toggleVoice,
                    onFetch: fetchVoices
                )
                #if os(macOS)
                .frame(minWidth: 400, idealWidth: 480, minHeight: 400, idealHeight: 500)
                #endif
            }
            .sheet(isPresented: $showModelSelector) {
                ModelSelectorSheet(
                    availableModels: $availableModels,
                    selectedModel: $selectedModel,
                    isLoading: $isLoadingModels,
                    onSelect: { model in
                        let previous = selectedModel
                        selectedModel = model
                        saveSelectedModel(previousModel: previous)
                    },
                    onFetch: fetchModels
                )
                #if os(macOS)
                .frame(minWidth: 400, idealWidth: 480, minHeight: 400, idealHeight: 500)
                #endif
            }
            .overlay {
                if isLoadingSettings {
                    ProgressView("Loading settings...")
                        .padding()
                        .background {
                            if reduceTransparency {
                                RoundedRectangle(cornerRadius: 12)
                                    .fill(.regularMaterial)
                            } else if #available(iOS 26.0, macOS 26.0, *) {
                                RoundedRectangle(cornerRadius: 12)
                                    .glassEffect(.clear)
                            } else {
                                RoundedRectangle(cornerRadius: 12)
                                    .fill(.regularMaterial)
                            }
                        }
                        .clipShape(RoundedRectangle(cornerRadius: 12))
                }
            }
            #if os(macOS)
            .frame(minWidth: 480, idealWidth: 520, minHeight: 500, idealHeight: 650)
            #endif
        }
    }

    // MARK: - API Key Row Component

    @ViewBuilder
    private func apiKeyRow(
        title: String,
        description: String,
        hasKey: Bool,
        isEditing: Binding<Bool>,
        keyInput: Binding<String>,
        onSave: @escaping () -> Void,
        onDelete: @escaping () -> Void
    ) -> some View {
        VStack(alignment: .leading, spacing: 8) {
            HStack {
                VStack(alignment: .leading) {
                    Text(title)
                        .font(.body)
                    Text(description)
                        .font(.caption)
                        .foregroundStyle(.secondary)
                }

                Spacer()

                if hasKey && !isEditing.wrappedValue {
                    HStack(spacing: 8) {
                        Text("••••••••")
                            .foregroundStyle(.secondary)
                        Button(role: .destructive) {
                            onDelete()
                        } label: {
                            Image(systemName: "trash")
                                .foregroundStyle(Color.composerDestructive)
                        }
                        .buttonStyle(.borderless)
                    }
                } else if !isEditing.wrappedValue {
                    Button("Set Key") {
                        isEditing.wrappedValue = true
                    }
                    .buttonStyle(.bordered)
                }
            }

            if isEditing.wrappedValue {
                HStack {
                    SecureField("Enter API key", text: keyInput)
                        .textFieldStyle(.roundedBorder)
                        .autocorrectionDisabled()
                        #if os(iOS)
                        .textInputAutocapitalization(.never)
                        #endif

                    Button("Save") {
                        onSave()
                        isEditing.wrappedValue = false
                    }
                    .buttonStyle(.borderedProminent)
                    .disabled(keyInput.wrappedValue.isEmpty || isSavingApiKey)

                    Button("Cancel") {
                        keyInput.wrappedValue = ""
                        isEditing.wrappedValue = false
                    }
                    .buttonStyle(.bordered)
                }
            }
        }
        .padding(.vertical, 4)
    }

    // MARK: - Data Loading

    private func loadSettings() {
        isLoadingSettings = true

        Task {
            async let elevenLabsCheck = KeychainService.shared.hasElevenLabsApiKeyAsync()
            async let openRouterCheck = KeychainService.shared.hasOpenRouterApiKeyAsync()

            let (elevenLabsResult, openRouterResult) = await (elevenLabsCheck, openRouterCheck)

            let hasElevenlabs = if case .success(let has) = elevenLabsResult { has } else { false }
            let hasOpenrouter = if case .success(let has) = openRouterResult { has } else { false }

            var settings: AiAudioSettings?
            do {
                settings = try await coreManager.safeCore.getAiAudioSettings()
            } catch {
                // Settings may not exist yet
            }

            await MainActor.run {
                hasElevenLabsKey = hasElevenlabs
                hasOpenRouterKey = hasOpenrouter
                if let settings {
                    audioEnabled = settings.enabled
                    audioPrompt = settings.audioPrompt
                    selectedModel = settings.openrouterModel
                    selectedVoiceIds = Set(settings.selectedVoiceIds)
                }
                isLoadingSettings = false
            }
        }
    }

    // MARK: - API Key Management

    private func saveElevenLabsKey() {
        guard !elevenLabsKeyInput.isEmpty else { return }
        isSavingApiKey = true
        let keyToSave = elevenLabsKeyInput

        Task {
            let result = await KeychainService.shared.saveElevenLabsApiKeyAsync(keyToSave)

            await MainActor.run {
                isSavingApiKey = false
                switch result {
                case .success:
                    hasElevenLabsKey = true
                    elevenLabsKeyInput = ""
                case .failure(let error):
                    errorMessage = "Failed to save API key: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    private func deleteElevenLabsKey() {
        Task {
            let result = await KeychainService.shared.deleteElevenLabsApiKeyAsync()

            await MainActor.run {
                if case .success = result {
                    hasElevenLabsKey = false
                    availableVoices = []
                }
            }
        }
    }

    private func saveOpenRouterKey() {
        guard !openRouterKeyInput.isEmpty else { return }
        isSavingApiKey = true
        let keyToSave = openRouterKeyInput

        Task {
            let result = await KeychainService.shared.saveOpenRouterApiKeyAsync(keyToSave)

            await MainActor.run {
                isSavingApiKey = false
                switch result {
                case .success:
                    hasOpenRouterKey = true
                    openRouterKeyInput = ""
                case .failure(let error):
                    errorMessage = "Failed to save API key: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    private func deleteOpenRouterKey() {
        Task {
            let result = await KeychainService.shared.deleteOpenRouterApiKeyAsync()

            await MainActor.run {
                if case .success = result {
                    hasOpenRouterKey = false
                    availableModels = []
                }
            }
        }
    }

    // MARK: - Settings Management

    private func saveAudioEnabled(_ enabled: Bool) {
        let previousValue = !enabled
        Task {
            do {
                try await coreManager.safeCore.setAudioNotificationsEnabled(enabled: enabled)
            } catch {
                await MainActor.run {
                    audioEnabled = previousValue
                    errorMessage = "Failed to save setting: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    private func saveAudioPrompt() {
        Task {
            do {
                try await coreManager.safeCore.setAudioPrompt(prompt: audioPrompt)
            } catch {
                await MainActor.run {
                    errorMessage = "Failed to save prompt: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    /// Default audio prompt used for text-to-speech massage
    private static let defaultAudioPrompt = """
        You are a text preprocessor for a text-to-speech system. Your task is to convert technical \
        conversation text into natural, speakable prose. Remove code blocks, simplify technical jargon, \
        and focus on the key message being communicated.
        """

    private func resetAudioPrompt() {
        let defaultPrompt = Self.defaultAudioPrompt
        Task {
            do {
                try await coreManager.safeCore.setAudioPrompt(prompt: defaultPrompt)
                await MainActor.run {
                    audioPrompt = defaultPrompt
                }
            } catch {
                await MainActor.run {
                    errorMessage = "Failed to reset prompt: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    private func saveSelectedModel(previousModel: String?) {
        Task {
            do {
                try await coreManager.safeCore.setOpenRouterModel(model: selectedModel)
            } catch {
                await MainActor.run {
                    selectedModel = previousModel
                    errorMessage = "Failed to save model: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    private func toggleVoice(_ voiceId: String) {
        let wasSelected = selectedVoiceIds.contains(voiceId)

        if wasSelected {
            selectedVoiceIds.remove(voiceId)
        } else {
            selectedVoiceIds.insert(voiceId)
        }

        Task {
            do {
                try await coreManager.safeCore.setSelectedVoiceIds(voiceIds: Array(selectedVoiceIds))
            } catch {
                await MainActor.run {
                    if wasSelected {
                        selectedVoiceIds.insert(voiceId)
                    } else {
                        selectedVoiceIds.remove(voiceId)
                    }
                    errorMessage = "Failed to save voice selection: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    // MARK: - API Fetching

    private func fetchVoices() {
        isLoadingVoices = true

        Task {
            let keyResult = await KeychainService.shared.loadElevenLabsApiKeyAsync()
            guard case .success(let apiKey) = keyResult else {
                await MainActor.run {
                    isLoadingVoices = false
                    errorMessage = "ElevenLabs API key not found in keychain"
                    showError = true
                }
                return
            }

            do {
                let voices = try await coreManager.safeCore.fetchElevenlabsVoices(apiKey: apiKey)
                await MainActor.run {
                    availableVoices = voices
                    isLoadingVoices = false
                }
            } catch {
                await MainActor.run {
                    isLoadingVoices = false
                    errorMessage = "Failed to fetch voices: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    private func fetchModels() {
        isLoadingModels = true

        Task {
            let keyResult = await KeychainService.shared.loadOpenRouterApiKeyAsync()
            guard case .success(let apiKey) = keyResult else {
                await MainActor.run {
                    isLoadingModels = false
                    errorMessage = "OpenRouter API key not found in keychain"
                    showError = true
                }
                return
            }

            do {
                let models = try await coreManager.safeCore.fetchOpenrouterModels(apiKey: apiKey)
                await MainActor.run {
                    availableModels = models
                    isLoadingModels = false
                }
            } catch {
                await MainActor.run {
                    isLoadingModels = false
                    errorMessage = "Failed to fetch models: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }
}

// MARK: - Voice Selector Sheet

private struct VoiceSelectorSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Binding var availableVoices: [VoiceInfo]
    @Binding var selectedVoiceIds: Set<String>
    @Binding var isLoading: Bool
    let onToggle: (String) -> Void
    let onFetch: () -> Void

    var body: some View {
        NavigationStack {
            Group {
                if isLoading {
                    ProgressView("Loading voices...")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if availableVoices.isEmpty {
                    ContentUnavailableView {
                        Label("No Voices", systemImage: "waveform")
                    } description: {
                        Text("Fetch voices from ElevenLabs to get started.")
                    } actions: {
                        Button("Fetch Voices") {
                            onFetch()
                        }
                        .buttonStyle(.borderedProminent)
                    }
                } else {
                    List(availableVoices, id: \.voiceId) { voice in
                        HStack {
                            VStack(alignment: .leading) {
                                Text(voice.name)
                                    .font(.body)
                                if let description = voice.description {
                                    Text(description)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                }
                            }
                            Spacer()
                            if selectedVoiceIds.contains(voice.voiceId) {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(Color.agentBrand)
                            }
                        }
                        .contentShape(Rectangle())
                        .onTapGesture {
                            onToggle(voice.voiceId)
                        }
                    }
                }
            }
            .navigationTitle("Voice Whitelist")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}

// MARK: - Model Selector Sheet

private struct ModelSelectorSheet: View {
    @Environment(\.dismiss) private var dismiss
    @Binding var availableModels: [ModelInfo]
    @Binding var selectedModel: String?
    @Binding var isLoading: Bool
    let onSelect: (String?) -> Void
    let onFetch: () -> Void

    var body: some View {
        NavigationStack {
            Group {
                if isLoading {
                    ProgressView("Loading models...")
                        .frame(maxWidth: .infinity, maxHeight: .infinity)
                } else if availableModels.isEmpty {
                    ContentUnavailableView {
                        Label("No Models", systemImage: "cpu")
                    } description: {
                        Text("Fetch models from OpenRouter to get started.")
                    } actions: {
                        Button("Fetch Models") {
                            onFetch()
                        }
                        .buttonStyle(.borderedProminent)
                    }
                } else {
                    List(availableModels, id: \.id) { model in
                        HStack {
                            VStack(alignment: .leading) {
                                Text(model.name ?? model.id)
                                    .font(.body)
                                if let description = model.description {
                                    Text(description)
                                        .font(.caption)
                                        .foregroundStyle(.secondary)
                                        .lineLimit(2)
                                }
                            }
                            Spacer()
                            if selectedModel == model.id {
                                Image(systemName: "checkmark")
                                    .foregroundStyle(Color.agentBrand)
                            }
                        }
                        .contentShape(Rectangle())
                        .onTapGesture {
                            onSelect(model.id)
                        }
                    }
                }
            }
            .navigationTitle("Text Message Model")
            .navigationBarTitleDisplayMode(.inline)
            .toolbar {
                ToolbarItem(placement: .topBarTrailing) {
                    Button("Done") { dismiss() }
                }
            }
        }
    }
}

#Preview {
    AISettingsView()
        .environmentObject(TenexCoreManager())
}
