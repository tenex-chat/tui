import Foundation
import AVFoundation

/// Audio playback state
enum AudioPlaybackState: Equatable {
    case idle
    case playing
    case paused
}

/// Service for playing audio notifications using AVAudioPlayer
/// Handles:
/// - Playing MP3 files from the audio_notifications directory
/// - Playback state management
/// - Replay functionality
/// - Metadata tracking for Now Playing bar
@MainActor
final class AudioNotificationPlayer: NSObject, ObservableObject {
    // MARK: - Published Properties

    /// Current playback state (observable for UI updates)
    @Published private(set) var playbackState: AudioPlaybackState = .idle

    /// Metadata from the audio notification
    @Published private(set) var currentAgentPubkey: String?
    @Published private(set) var currentConversationTitle: String?
    @Published private(set) var currentTextSnippet: String?
    @Published private(set) var currentConversationId: String?

    /// Published progress (0.0 to 1.0), updated by timer for SwiftUI reactivity
    @Published private(set) var playbackProgress: Double = 0

    /// Whether audio is currently playing
    var isPlaying: Bool {
        playbackState == .playing
    }

    // MARK: - Private Properties

    private var audioPlayer: AVAudioPlayer?
    private var currentFilePath: URL?
    private var progressTimer: Timer?

    // MARK: - Singleton

    static let shared = AudioNotificationPlayer()

    private override init() {
        super.init()
    }

    // MARK: - Audio Session Configuration

    private func activateAudioSession() {
        #if os(iOS)
        do {
            let audioSession = AVAudioSession.sharedInstance()
            try audioSession.setCategory(.playback, mode: .default, options: .duckOthers)
            try audioSession.setActive(true)
        } catch {
            print("[AudioNotificationPlayer] Failed to activate audio session: \(error)")
        }
        #endif
    }

    private func deactivateAudioSession() {
        #if os(iOS)
        do {
            let audioSession = AVAudioSession.sharedInstance()
            try audioSession.setActive(false, options: .notifyOthersOnDeactivation)
        } catch {
            print("[AudioNotificationPlayer] Failed to deactivate audio session: \(error)")
        }
        #endif
    }

    // MARK: - Progress Timer

    private func startProgressTimer() {
        progressTimer?.invalidate()
        progressTimer = Timer.scheduledTimer(withTimeInterval: 0.1, repeats: true) { [weak self] _ in
            Task { @MainActor [weak self] in
                guard let self, let player = self.audioPlayer, player.duration > 0 else { return }
                self.playbackProgress = player.currentTime / player.duration
            }
        }
    }

    private func stopProgressTimer() {
        progressTimer?.invalidate()
        progressTimer = nil
    }

    // MARK: - Playback Control

    /// Play an audio notification with full metadata
    func play(notification: AudioNotificationInfo, conversationId: String?) throws {
        let fileURL = URL(fileURLWithPath: notification.audioFilePath)
        try play(url: fileURL)
        // Set metadata AFTER play(url:) succeeds — play(url:) calls stop() which clears metadata
        currentAgentPubkey = notification.agentPubkey
        currentConversationTitle = notification.conversationTitle
        currentTextSnippet = notification.massagedText
        currentConversationId = conversationId
    }

    /// Play an audio file from the given path
    func play(path: String) throws {
        let fileURL = URL(fileURLWithPath: path)
        try play(url: fileURL)
    }

    /// Play an audio file from a URL
    func play(url: URL) throws {
        // Stop any current playback
        stop()

        // Activate audio session only when we actually need to play
        activateAudioSession()

        // Create and configure the audio player
        audioPlayer = try AVAudioPlayer(contentsOf: url)
        audioPlayer?.delegate = self
        audioPlayer?.prepareToPlay()

        guard let player = audioPlayer else {
            deactivateAudioSession()
            throw AudioPlayerError.initializationFailed
        }

        // Start playback
        if player.play() {
            currentFilePath = url
            playbackState = .playing
            playbackProgress = 0
            startProgressTimer()
        } else {
            deactivateAudioSession()
            throw AudioPlayerError.playbackFailed
        }
    }

    /// Stop the current playback
    func stop() {
        stopProgressTimer()
        audioPlayer?.stop()
        audioPlayer = nil
        playbackState = .idle
        playbackProgress = 0
        clearMetadata()
        deactivateAudioSession()
    }

    /// Pause the current playback
    func pause() {
        audioPlayer?.pause()
        playbackState = .paused
        stopProgressTimer()
    }

    /// Resume paused playback
    func resume() {
        if playbackState == .paused, let player = audioPlayer {
            player.play()
            playbackState = .playing
            startProgressTimer()
        }
    }

    /// Toggle play/pause state
    func togglePlayPause() {
        switch playbackState {
        case .playing:
            pause()
        case .paused:
            resume()
        case .idle:
            if let path = currentFilePath {
                try? play(url: path)
            }
        }
    }

    /// Replay the last audio file
    func replay() throws {
        guard let path = currentFilePath else {
            throw AudioPlayerError.noFileToReplay
        }
        try play(url: path)
    }

    /// Get the current playback time in seconds
    var currentTime: TimeInterval {
        audioPlayer?.currentTime ?? 0
    }

    /// Get the duration in seconds
    var duration: TimeInterval {
        audioPlayer?.duration ?? 0
    }

    // MARK: - Private Helpers

    private func clearMetadata() {
        currentAgentPubkey = nil
        currentConversationTitle = nil
        currentTextSnippet = nil
        currentConversationId = nil
    }
}

// MARK: - AVAudioPlayerDelegate

extension AudioNotificationPlayer: AVAudioPlayerDelegate {
    nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        Task { @MainActor in
            self.stopProgressTimer()
            self.playbackState = .idle
            self.playbackProgress = 0
            self.clearMetadata()
            self.deactivateAudioSession()
        }
    }

    nonisolated func audioPlayerDecodeErrorDidOccur(_ player: AVAudioPlayer, error: Error?) {
        Task { @MainActor in
            self.stopProgressTimer()
            self.playbackState = .idle
            self.playbackProgress = 0
            self.clearMetadata()
            self.deactivateAudioSession()
            if let error = error {
                print("[AudioNotificationPlayer] Decode error: \(error)")
            }
        }
    }
}

// MARK: - Errors

enum AudioPlayerError: LocalizedError {
    case initializationFailed
    case playbackFailed
    case noFileToReplay

    var errorDescription: String? {
        switch self {
        case .initializationFailed:
            return "Failed to initialize audio player"
        case .playbackFailed:
            return "Failed to start audio playback"
        case .noFileToReplay:
            return "No audio file to replay"
        }
    }
}
