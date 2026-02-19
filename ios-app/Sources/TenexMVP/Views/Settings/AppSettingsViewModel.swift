import SwiftUI
import AVFoundation

enum SettingsSection: String, CaseIterable, Hashable, Identifiable {
    case relays
    case backends
    case ai
    case audio

    var id: String { rawValue }

    var title: String {
        switch self {
        case .relays: return "Relays"
        case .backends: return "Backends"
        case .ai: return "AI"
        case .audio: return "Audio"
        }
    }

    var icon: String {
        switch self {
        case .relays: return "antenna.radiowaves.left.and.right"
        case .backends: return "shield"
        case .ai: return "brain"
        case .audio: return "waveform"
        }
    }
}

@MainActor
final class AppSettingsViewModel: ObservableObject {
    @Published var hasElevenLabsKey = false
    @Published var hasOpenRouterKey = false
    @Published var audioEnabled = false
    @Published var audioPrompt = ""
    @Published var selectedModelIds: Set<String> = []
    @Published var selectedVoiceIds: Set<String> = []
    @Published var ttsInactivityThresholdSecs: UInt64 = 120

    @Published var availableVoices: [VoiceInfo] = []
    @Published var availableModels: [ModelInfo] = []
    @Published var relayUrls: [String] = []
    @Published var diagnosticsSnapshot: DiagnosticsSnapshot?
    @Published var backendSnapshot: BackendTrustSnapshot?

    @Published var isLoading = true
    @Published var isLoadingVoices = false
    @Published var isLoadingModels = false
    @Published var isSavingApiKey = false
    @Published var errorMessage: String?

    private static let defaultAudioPrompt = """
        You are a text preprocessor for a text-to-speech system. Your task is to convert technical \
        conversation text into natural, speakable prose. Remove code blocks, simplify technical jargon, \
        and focus on the key message being communicated.
        """

    func load(coreManager: TenexCoreManager) async {
        isLoading = true

        async let elevenLabsCheck = KeychainService.shared.hasElevenLabsApiKeyAsync()
        async let openRouterCheck = KeychainService.shared.hasOpenRouterApiKeyAsync()

        let (elevenLabsResult, openRouterResult) = await (elevenLabsCheck, openRouterCheck)
        if case .success(let has) = elevenLabsResult { hasElevenLabsKey = has }
        if case .success(let has) = openRouterResult { hasOpenRouterKey = has }

        do {
            let settings = try await coreManager.safeCore.getAiAudioSettings()
            audioEnabled = settings.enabled
            audioPrompt = settings.audioPrompt
            selectedModelIds = OpenRouterModelSelectionCodec.decodeSelectedModelIds(from: settings.openrouterModel)
            selectedVoiceIds = Set(settings.selectedVoiceIds)
            ttsInactivityThresholdSecs = settings.ttsInactivityThresholdSecs
        } catch {
            // Keep defaults if settings are not available yet.
        }

        await reloadRelays(coreManager: coreManager)
        await reloadBackends(coreManager: coreManager)
        isLoading = false
    }

    func reloadRelays(coreManager: TenexCoreManager) async {
        relayUrls = await coreManager.safeCore.getConfiguredRelays()
        diagnosticsSnapshot = await coreManager.safeCore.getDiagnosticsSnapshot(includeDatabaseStats: false)
    }

    func reconnectRelays(coreManager: TenexCoreManager) async {
        do {
            try await coreManager.safeCore.forceReconnect()
            await coreManager.fetchData()
            await reloadRelays(coreManager: coreManager)
        } catch {
            errorMessage = "Failed to reconnect relays: \(error.localizedDescription)"
        }
    }

    func syncNow(coreManager: TenexCoreManager) async {
        await coreManager.manualRefresh()
        await reloadRelays(coreManager: coreManager)
    }

    func reloadBackends(coreManager: TenexCoreManager) async {
        do {
            backendSnapshot = try await coreManager.safeCore.getBackendTrustSnapshot()
        } catch {
            errorMessage = "Failed to load backend trust state: \(error.localizedDescription)"
        }
    }

    func approveBackend(coreManager: TenexCoreManager, pubkey: String) async {
        do {
            try await coreManager.safeCore.approveBackend(pubkey: pubkey)
            await coreManager.fetchData()
            await reloadBackends(coreManager: coreManager)
        } catch {
            errorMessage = "Failed to approve backend: \(error.localizedDescription)"
        }
    }

    func blockBackend(coreManager: TenexCoreManager, pubkey: String) async {
        do {
            try await coreManager.safeCore.blockBackend(pubkey: pubkey)
            await coreManager.fetchData()
            await reloadBackends(coreManager: coreManager)
        } catch {
            errorMessage = "Failed to block backend: \(error.localizedDescription)"
        }
    }

    func removeFromTrustedLists(coreManager: TenexCoreManager, pubkey: String) async {
        guard let snapshot = backendSnapshot else { return }
        let approved = snapshot.approved.filter { $0 != pubkey }
        let blocked = snapshot.blocked.filter { $0 != pubkey }

        do {
            try await coreManager.safeCore.setTrustedBackends(approved: approved, blocked: blocked)
            await coreManager.fetchData()
            await reloadBackends(coreManager: coreManager)
        } catch {
            errorMessage = "Failed to update backend lists: \(error.localizedDescription)"
        }
    }

    func saveElevenLabsKey(_ key: String) async {
        guard !key.isEmpty else { return }
        isSavingApiKey = true
        let result = await KeychainService.shared.saveElevenLabsApiKeyAsync(key)
        isSavingApiKey = false

        switch result {
        case .success:
            hasElevenLabsKey = true
        case .failure(let error):
            errorMessage = "Failed to save ElevenLabs key: \(error.localizedDescription)"
        }
    }

    func deleteElevenLabsKey() async {
        let result = await KeychainService.shared.deleteElevenLabsApiKeyAsync()
        if case .success = result {
            hasElevenLabsKey = false
            availableVoices = []
        }
    }

    func saveOpenRouterKey(_ key: String) async {
        guard !key.isEmpty else { return }
        isSavingApiKey = true
        let result = await KeychainService.shared.saveOpenRouterApiKeyAsync(key)
        isSavingApiKey = false

        switch result {
        case .success:
            hasOpenRouterKey = true
        case .failure(let error):
            errorMessage = "Failed to save OpenRouter key: \(error.localizedDescription)"
        }
    }

    func deleteOpenRouterKey() async {
        let result = await KeychainService.shared.deleteOpenRouterApiKeyAsync()
        if case .success = result {
            hasOpenRouterKey = false
            availableModels = []
        }
    }

    func fetchModels(coreManager: TenexCoreManager) async {
        isLoadingModels = true
        defer { isLoadingModels = false }

        let keyResult = await KeychainService.shared.loadOpenRouterApiKeyAsync()
        guard case .success(let apiKey) = keyResult else {
            errorMessage = "OpenRouter API key not found in local storage"
            return
        }

        do {
            availableModels = try await coreManager.safeCore.fetchOpenrouterModels(apiKey: apiKey)
        } catch {
            errorMessage = "Failed to fetch models: \(error.localizedDescription)"
        }
    }

    func fetchVoices(coreManager: TenexCoreManager) async {
        isLoadingVoices = true
        defer { isLoadingVoices = false }

        let keyResult = await KeychainService.shared.loadElevenLabsApiKeyAsync()
        guard case .success(let apiKey) = keyResult else {
            errorMessage = "ElevenLabs API key not found in local storage"
            return
        }

        do {
            availableVoices = try await coreManager.safeCore.fetchElevenlabsVoices(apiKey: apiKey)
        } catch {
            errorMessage = "Failed to fetch voices: \(error.localizedDescription)"
        }
    }

    func toggleSelectedModel(coreManager: TenexCoreManager, modelId: String) async {
        let previous = selectedModelIds
        if selectedModelIds.contains(modelId) {
            selectedModelIds.remove(modelId)
        } else {
            selectedModelIds.insert(modelId)
        }

        await persistSelectedModels(coreManager: coreManager, rollbackTo: previous)
    }

    func clearSelectedModels(coreManager: TenexCoreManager) async {
        let previous = selectedModelIds
        selectedModelIds = []
        await persistSelectedModels(coreManager: coreManager, rollbackTo: previous)
    }

    var selectedModelsSummary: String {
        guard !selectedModelIds.isEmpty else { return "Not selected" }
        if selectedModelIds.count == 1, let modelId = selectedModelIds.first {
            if let model = availableModels.first(where: { $0.id == modelId }) {
                return model.name ?? modelId
            }
            return modelId
        }
        return "\(selectedModelIds.count) selected"
    }

    func isModelSelected(_ modelId: String) -> Bool {
        selectedModelIds.contains(modelId)
    }

    func setAudioEnabled(coreManager: TenexCoreManager, enabled: Bool) async {
        let previous = audioEnabled
        audioEnabled = enabled
        do {
            try await coreManager.safeCore.setAudioNotificationsEnabled(enabled: enabled)
        } catch {
            audioEnabled = previous
            errorMessage = "Failed to save setting: \(error.localizedDescription)"
        }
    }

    func setTtsInactivityThreshold(coreManager: TenexCoreManager, secs: UInt64) async {
        let previous = ttsInactivityThresholdSecs
        ttsInactivityThresholdSecs = secs
        do {
            try await coreManager.safeCore.setTtsInactivityThreshold(secs: secs)
        } catch {
            ttsInactivityThresholdSecs = previous
            errorMessage = "Failed to save inactivity threshold: \(error.localizedDescription)"
        }
    }

    func saveAudioPrompt(coreManager: TenexCoreManager) async {
        do {
            try await coreManager.safeCore.setAudioPrompt(prompt: audioPrompt)
        } catch {
            errorMessage = "Failed to save prompt: \(error.localizedDescription)"
        }
    }

    func resetAudioPrompt(coreManager: TenexCoreManager) async {
        do {
            try await coreManager.safeCore.setAudioPrompt(prompt: Self.defaultAudioPrompt)
            audioPrompt = Self.defaultAudioPrompt
        } catch {
            errorMessage = "Failed to reset prompt: \(error.localizedDescription)"
        }
    }

    func toggleVoice(coreManager: TenexCoreManager, voiceId: String) async {
        let wasSelected = selectedVoiceIds.contains(voiceId)
        if wasSelected {
            selectedVoiceIds.remove(voiceId)
        } else {
            selectedVoiceIds.insert(voiceId)
        }

        do {
            try await coreManager.safeCore.setSelectedVoiceIds(voiceIds: Array(selectedVoiceIds))
        } catch {
            if wasSelected {
                selectedVoiceIds.insert(voiceId)
            } else {
                selectedVoiceIds.remove(voiceId)
            }
            errorMessage = "Failed to save voice selection: \(error.localizedDescription)"
        }
    }

    private func persistSelectedModels(coreManager: TenexCoreManager, rollbackTo previous: Set<String>) async {
        do {
            let encoded = OpenRouterModelSelectionCodec.encodeSelectedModelIds(selectedModelIds)
            try await coreManager.safeCore.setOpenRouterModel(model: encoded)
        } catch {
            selectedModelIds = previous
            errorMessage = "Failed to save model selection: \(error.localizedDescription)"
        }
    }
}

@MainActor
final class VoicePreviewPlayer: ObservableObject {
    @Published var playingVoiceId: String?
    private var player: AVPlayer?

    func toggle(voiceId: String, previewUrl: String?) {
        guard let previewUrl,
              let url = URL(string: previewUrl) else {
            return
        }

        if playingVoiceId == voiceId {
            stop()
            return
        }

        stop()
        player = AVPlayer(url: url)
        playingVoiceId = voiceId
        player?.play()
    }

    func stop() {
        player?.pause()
        player = nil
        playingVoiceId = nil
    }
}
