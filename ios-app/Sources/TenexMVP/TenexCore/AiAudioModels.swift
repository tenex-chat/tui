import Foundation

/// AI Audio Settings - configuration status and preferences
/// Note: API keys are never exposed, only their configuration status
public struct AiAudioSettings {
    public let elevenlabsApiKeyConfigured: Bool
    public let openrouterApiKeyConfigured: Bool
    public let selectedVoiceIds: [String]
    public let openrouterModel: String?
    public let audioPrompt: String
    public let enabled: Bool
    public let ttsInactivityThresholdSecs: UInt64

    public init(
        elevenlabsApiKeyConfigured: Bool,
        openrouterApiKeyConfigured: Bool,
        selectedVoiceIds: [String],
        openrouterModel: String?,
        audioPrompt: String,
        enabled: Bool,
        ttsInactivityThresholdSecs: UInt64
    ) {
        self.elevenlabsApiKeyConfigured = elevenlabsApiKeyConfigured
        self.openrouterApiKeyConfigured = openrouterApiKeyConfigured
        self.selectedVoiceIds = selectedVoiceIds
        self.openrouterModel = openrouterModel
        self.audioPrompt = audioPrompt
        self.enabled = enabled
        self.ttsInactivityThresholdSecs = ttsInactivityThresholdSecs
    }
}

// NOTE: VoiceInfo and ModelInfo types are now provided by the Rust FFI bindings
// (tenex_core.swift). Do not redefine them here to avoid type conflicts.
