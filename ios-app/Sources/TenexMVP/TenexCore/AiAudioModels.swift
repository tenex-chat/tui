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
    
    public init(
        elevenlabsApiKeyConfigured: Bool,
        openrouterApiKeyConfigured: Bool,
        selectedVoiceIds: [String],
        openrouterModel: String?,
        audioPrompt: String,
        enabled: Bool
    ) {
        self.elevenlabsApiKeyConfigured = elevenlabsApiKeyConfigured
        self.openrouterApiKeyConfigured = openrouterApiKeyConfigured
        self.selectedVoiceIds = selectedVoiceIds
        self.openrouterModel = openrouterModel
        self.audioPrompt = audioPrompt
        self.enabled = enabled
    }
}

/// Voice from ElevenLabs API
public struct VoiceInfo {
    public let voiceId: String
    public let name: String
    public let category: String?
    public let description: String?
    
    public init(voiceId: String, name: String, category: String?, description: String?) {
        self.voiceId = voiceId
        self.name = name
        self.category = category
        self.description = description
    }
}

/// Model from OpenRouter API
public struct ModelInfo {
    public let modelId: String
    public let name: String
    public let description: String?
    public let contextLength: UInt32?
    
    public init(modelId: String, name: String, description: String?, contextLength: UInt32?) {
        self.modelId = modelId
        self.name = name
        self.description = description
        self.contextLength = contextLength
    }
}
