import Foundation
import AVFoundation

/// Audio playback state
enum AudioPlaybackState: Equatable {
    case idle
    case playing
    case paused
}

/// A queued audio notification waiting to be played
struct QueuedAudioItem: Identifiable {
    let id = UUID()
    let notification: AudioNotificationInfo
    let conversationId: String?
}

/// Service for playing audio notifications using AVAudioPlayer
/// Handles:
/// - Playing MP3 files from the audio_notifications directory
/// - Playback state management
/// - Audio queue (new notifications enqueue instead of interrupting)
/// - Metadata tracking for Now Playing bar
@MainActor
final class AudioNotificationPlayer: NSObject, ObservableObject {
    // MARK: - Published Properties

    /// Current playback state (observable for UI updates)
    @Published private(set) var playbackState: AudioPlaybackState = .idle

    /// Metadata from the currently playing audio notification
    @Published private(set) var currentAgentPubkey: String?
    @Published private(set) var currentConversationTitle: String?
    @Published private(set) var currentTextSnippet: String?
    @Published private(set) var currentConversationId: String?

    /// Published progress (0.0 to 1.0), updated by timer for SwiftUI reactivity
    @Published private(set) var playbackProgress: Double = 0

    /// Audio queue — upcoming notifications waiting to play
    @Published private(set) var queue: [QueuedAudioItem] = []

    /// Whether audio is currently playing
    var isPlaying: Bool {
        playbackState == .playing
    }

    /// Whether there are items waiting in the queue
    var hasQueue: Bool {
        !queue.isEmpty
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
        }
        #endif
    }

    private func deactivateAudioSession() {
        #if os(iOS)
        do {
            let audioSession = AVAudioSession.sharedInstance()
            try audioSession.setActive(false, options: .notifyOthersOnDeactivation)
        } catch {
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

    // MARK: - Queue Management

    /// Enqueue an audio notification. If nothing is playing, starts immediately.
    /// If something is already playing, adds to the queue.
    func enqueue(notification: AudioNotificationInfo, conversationId: String?) {
        if playbackState == .idle {
            do {
                try playImmediate(notification: notification, conversationId: conversationId)
            } catch {
                playNext()
            }
        } else {
            queue.append(QueuedAudioItem(notification: notification, conversationId: conversationId))
        }
    }

    /// Skip the current track and play the next queued item
    func skipToNext() {
        stopCurrentPlayback()
        playNext()
    }

    /// Remove a specific item from the queue by ID
    func removeFromQueue(id: UUID) {
        queue.removeAll { $0.id == id }
    }

    /// Clear the entire queue (does not stop current playback)
    func clearQueue() {
        queue.removeAll()
    }

    // MARK: - Playback Control

    /// Play an audio notification immediately (internal — use enqueue() from outside)
    private func playImmediate(notification: AudioNotificationInfo, conversationId: String?) throws {
        let fileURL = URL(fileURLWithPath: notification.audioFilePath)
        try playURL(fileURL)
        // Set metadata AFTER playURL succeeds — playURL calls stopCurrentPlayback() which clears metadata
        currentAgentPubkey = notification.agentPubkey
        currentConversationTitle = notification.conversationTitle
        currentTextSnippet = notification.massagedText
        currentConversationId = conversationId
    }

    /// Play an audio file from the given path (no queue, no metadata)
    func play(path: String) throws {
        let fileURL = URL(fileURLWithPath: path)
        try playURL(fileURL)
    }

    /// Play an audio file from a URL (low-level)
    private func playURL(_ url: URL) throws {
        stopCurrentPlayback()

        activateAudioSession()

        audioPlayer = try AVAudioPlayer(contentsOf: url)
        audioPlayer?.delegate = self
        audioPlayer?.prepareToPlay()

        guard let player = audioPlayer else {
            deactivateAudioSession()
            throw AudioPlayerError.initializationFailed
        }

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

    /// Stop all playback and clear the queue
    func stop() {
        stopCurrentPlayback()
        queue.removeAll()
        deactivateAudioSession()
    }

    /// Stop only the current track (used internally for advancing queue)
    private func stopCurrentPlayback() {
        stopProgressTimer()
        audioPlayer?.stop()
        audioPlayer = nil
        playbackState = .idle
        playbackProgress = 0
        clearMetadata()
    }

    /// Play the next item in the queue, or go idle if empty
    private func playNext() {
        guard !queue.isEmpty else {
            playbackState = .idle
            deactivateAudioSession()
            return
        }

        let next = queue.removeFirst()
        do {
            try playImmediate(notification: next.notification, conversationId: next.conversationId)
        } catch {
            playNext()
        }
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
                try? playURL(path)
            }
        }
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
            self.playbackProgress = 0
            self.clearMetadata()
            self.playNext()
        }
    }

    nonisolated func audioPlayerDecodeErrorDidOccur(_ player: AVAudioPlayer, error: Error?) {
        Task { @MainActor in
            self.stopProgressTimer()
            self.playbackProgress = 0
            self.clearMetadata()
            if let error = error {
            }
            self.playNext()
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
