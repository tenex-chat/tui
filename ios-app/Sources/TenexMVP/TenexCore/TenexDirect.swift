import Foundation

/// Namespace for stateless FFI calls that don't need TenexCore instance state.
/// Each method runs on a detached task to avoid inheriting actor isolation,
/// keeping these network-bound calls off the SafeTenexCore actor queue.
enum TenexDirect {

    static func listAudioNotifications() async throws -> [AudioNotificationInfo] {
        try await Task.detached {
            // Module-qualify to call the free function, not this static method.
            try TenexMVP.listAudioNotifications()
        }.value
    }

    static func deleteAudioNotification(id: String) async throws {
        try await Task.detached {
            try TenexMVP.deleteAudioNotification(id: id)
        }.value
    }

    static func fetchElevenLabsVoices(apiKey: String) async throws -> [VoiceInfo] {
        try await Task.detached {
            try fetchElevenlabsVoices(apiKey: apiKey)
        }.value
    }

    static func fetchOpenRouterModels(apiKey: String) async throws -> [ModelInfo] {
        try await Task.detached {
            try fetchOpenrouterModels(apiKey: apiKey)
        }.value
    }
}
