import SwiftUI

/// AI Settings view for configuring audio notifications
/// Allows users to:
/// - Configure ElevenLabs and OpenRouter API keys
/// - Select voices for text-to-speech
/// - Choose LLM model for text massage
/// - Edit the audio prompt
/// - Enable/disable audio notifications
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
                        description: "For text massage LLM",
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
                    } footer: {
                        Text("When enabled, messages that mention you will be read aloud.")
                    }

                    // Voice Selection Section
                    Section {
                        if isLoadingVoices {
                            HStack {
                                ProgressView()
                                    .scaleEffect(0.8)
                                Text("Loading voices...")
                                    .foregroundStyle(.secondary)
                            }
                        } else if availableVoices.isEmpty {
                            Button("Fetch Available Voices") {
                                fetchVoices()
                            }
                        } else {
                            ForEach(availableVoices, id: \.voiceId) { voice in
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
                                            .foregroundStyle(.blue)
                                    }
                                }
                                .contentShape(Rectangle())
                                .onTapGesture {
                                    toggleVoice(voice.voiceId)
                                }
                            }
                        }
                    } header: {
                        Text("Voice Whitelist")
                    } footer: {
                        Text("Select voices to use for notifications. Agents are deterministically assigned to voices.")
                    }

                    // Model Selection Section
                    Section {
                        if isLoadingModels {
                            HStack {
                                ProgressView()
                                    .scaleEffect(0.8)
                                Text("Loading models...")
                                    .foregroundStyle(.secondary)
                            }
                        } else if availableModels.isEmpty {
                            Button("Fetch Available Models") {
                                fetchModels()
                            }
                        } else {
                            Picker("LLM Model", selection: Binding(
                                get: { selectedModel ?? "" },
                                set: { newValue in
                                    let previousModel = selectedModel
                                    selectedModel = newValue.isEmpty ? nil : newValue
                                    saveSelectedModel(previousModel: previousModel)
                                }
                            )) {
                                Text("Select a model").tag("")
                                ForEach(availableModels, id: \.id) { model in
                                    Text(model.name ?? model.id).tag(model.id)
                                }
                            }
                        }
                    } header: {
                        Text("Text Massage Model")
                    } footer: {
                        Text("This model converts agent messages into natural speech text.")
                    }

                    // Audio Prompt Section
                    Section {
                        TextEditor(text: $audioPrompt)
                            .frame(minHeight: 100)
                            .onChange(of: audioPrompt) { _, _ in
                                // Debounce save
                            }
                        Button("Save Prompt") {
                            saveAudioPrompt()
                        }
                        .disabled(audioPrompt.isEmpty)
                    } header: {
                        Text("Audio Prompt")
                    } footer: {
                        Text("Instructions for how the LLM should massage text for speech synthesis.")
                    }
                }
            }
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
                                .foregroundStyle(.red)
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
            // Check keychain for existing API keys
            async let elevenLabsCheck = KeychainService.shared.hasElevenLabsApiKeyAsync()
            async let openRouterCheck = KeychainService.shared.hasOpenRouterApiKeyAsync()

            let (elevenLabsResult, openRouterResult) = await (elevenLabsCheck, openRouterCheck)

            await MainActor.run {
                if case .success(let hasKey) = elevenLabsResult {
                    hasElevenLabsKey = hasKey
                }
                if case .success(let hasKey) = openRouterResult {
                    hasOpenRouterKey = hasKey
                }
            }

            // Load AI settings from core
            do {
                let settings = try await coreManager.safeCore.getAiAudioSettings()
                await MainActor.run {
                    audioEnabled = settings.enabled
                    audioPrompt = settings.audioPrompt
                    selectedModel = settings.openrouterModel
                    selectedVoiceIds = Set(settings.selectedVoiceIds)
                    hasElevenLabsKey = settings.elevenlabsApiKeyConfigured
                    hasOpenRouterKey = settings.openrouterApiKeyConfigured
                    isLoadingSettings = false
                }
            } catch {
                await MainActor.run {
                    isLoadingSettings = false
                    // Settings may not exist yet, that's OK
                }
            }
        }
    }

    // MARK: - API Key Management

    private func saveElevenLabsKey() {
        guard !elevenLabsKeyInput.isEmpty else { return }
        isSavingApiKey = true
        let keyToSave = elevenLabsKeyInput

        Task {
            // TRANSACTIONAL: Save to core first (more likely to fail), then keychain
            // If keychain fails, rollback core change
            do {
                // Step 1: Save to core (Rust secure storage)
                try await coreManager.safeCore.setElevenLabsApiKey(key: keyToSave)

                // Step 2: Save to iOS keychain
                let keychainResult = await KeychainService.shared.saveElevenLabsApiKeyAsync(keyToSave)

                switch keychainResult {
                case .success:
                    await MainActor.run {
                        isSavingApiKey = false
                        hasElevenLabsKey = true
                        elevenLabsKeyInput = ""
                    }
                case .failure(let keychainError):
                    // Rollback core change on keychain failure
                    do {
                        try await coreManager.safeCore.setElevenLabsApiKey(key: nil)
                        await MainActor.run {
                            isSavingApiKey = false
                            errorMessage = "Failed to save API key to keychain: \(keychainError.localizedDescription)"
                            showError = true
                        }
                    } catch let rollbackError {
                        // Rollback failed - warn user about inconsistent state
                        await MainActor.run {
                            isSavingApiKey = false
                            errorMessage = "Keychain save failed (\(keychainError.localizedDescription)) and rollback also failed (\(rollbackError.localizedDescription)). State may be inconsistent."
                            showError = true
                        }
                    }
                }
            } catch {
                // Core save failed - no rollback needed
                await MainActor.run {
                    isSavingApiKey = false
                    errorMessage = "Failed to save API key: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    private func deleteElevenLabsKey() {
        Task {
            let keychainResult = await KeychainService.shared.deleteElevenLabsApiKeyAsync()

            // Also clear in core
            do {
                try await coreManager.safeCore.setElevenLabsApiKey(key: nil)
            } catch {
                // Log but continue
            }

            await MainActor.run {
                if case .success = keychainResult {
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
            // TRANSACTIONAL: Save to core first (more likely to fail), then keychain
            // If keychain fails, rollback core change
            do {
                // Step 1: Save to core (Rust secure storage)
                try await coreManager.safeCore.setOpenRouterApiKey(key: keyToSave)

                // Step 2: Save to iOS keychain
                let keychainResult = await KeychainService.shared.saveOpenRouterApiKeyAsync(keyToSave)

                switch keychainResult {
                case .success:
                    await MainActor.run {
                        isSavingApiKey = false
                        hasOpenRouterKey = true
                        openRouterKeyInput = ""
                    }
                case .failure(let keychainError):
                    // Rollback core change on keychain failure
                    do {
                        try await coreManager.safeCore.setOpenRouterApiKey(key: nil)
                        await MainActor.run {
                            isSavingApiKey = false
                            errorMessage = "Failed to save API key to keychain: \(keychainError.localizedDescription)"
                            showError = true
                        }
                    } catch let rollbackError {
                        // Rollback failed - warn user about inconsistent state
                        await MainActor.run {
                            isSavingApiKey = false
                            errorMessage = "Keychain save failed (\(keychainError.localizedDescription)) and rollback also failed (\(rollbackError.localizedDescription)). State may be inconsistent."
                            showError = true
                        }
                    }
                }
            } catch {
                // Core save failed - no rollback needed
                await MainActor.run {
                    isSavingApiKey = false
                    errorMessage = "Failed to save API key: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    private func deleteOpenRouterKey() {
        Task {
            let keychainResult = await KeychainService.shared.deleteOpenRouterApiKeyAsync()

            // Also clear in core
            do {
                try await coreManager.safeCore.setOpenRouterApiKey(key: nil)
            } catch {
                // Log but continue
            }

            await MainActor.run {
                if case .success = keychainResult {
                    hasOpenRouterKey = false
                    availableModels = []
                }
            }
        }
    }

    // MARK: - Settings Management

    private func saveAudioEnabled(_ enabled: Bool) {
        let previousValue = !enabled  // Toggle changed it, so previous is opposite
        Task {
            do {
                try await coreManager.safeCore.setAudioNotificationsEnabled(enabled: enabled)
            } catch {
                // Rollback on failure
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

    private func saveSelectedModel(previousModel: String?) {
        Task {
            do {
                try await coreManager.safeCore.setOpenRouterModel(model: selectedModel)
            } catch {
                // Rollback on failure
                await MainActor.run {
                    selectedModel = previousModel
                    errorMessage = "Failed to save model: \(error.localizedDescription)"
                    showError = true
                }
            }
        }
    }

    private func toggleVoice(_ voiceId: String) {
        // Capture previous state for rollback
        let wasSelected = selectedVoiceIds.contains(voiceId)

        // Apply optimistic update
        if wasSelected {
            selectedVoiceIds.remove(voiceId)
        } else {
            selectedVoiceIds.insert(voiceId)
        }

        Task {
            do {
                try await coreManager.safeCore.setSelectedVoiceIds(voiceIds: Array(selectedVoiceIds))
            } catch {
                // Rollback on failure
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
            do {
                let voices = try await coreManager.safeCore.fetchElevenLabsVoices()
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
            do {
                let models = try await coreManager.safeCore.fetchOpenRouterModels()
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

#Preview {
    AISettingsView()
        .environmentObject(TenexCoreManager())
}
