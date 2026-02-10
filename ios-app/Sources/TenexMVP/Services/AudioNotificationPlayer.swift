import Foundation
import AVFoundation

/// Audio playback state
enum AudioPlaybackState {
    case idle
    case playing
    case paused
}

/// Service for playing audio notifications using AVAudioPlayer
/// Handles:
/// - Playing MP3 files from the audio_notifications directory
/// - Playback state management
/// - Replay functionality
/// - UI status indicator support
@MainActor
final class AudioNotificationPlayer: NSObject, ObservableObject {
    // MARK: - Published Properties

    /// Current playback state (observable for UI updates)
    @Published private(set) var playbackState: AudioPlaybackState = .idle

    /// Currently playing or last played file name
    @Published private(set) var currentFileName: String?

    /// Whether audio is currently playing
    var isPlaying: Bool {
        playbackState == .playing
    }

    // MARK: - Private Properties

    private var audioPlayer: AVAudioPlayer?
    private var currentFilePath: URL?

    // MARK: - Singleton

    static let shared = AudioNotificationPlayer()

    private override init() {
        super.init()
        configureAudioSession()
    }

    // MARK: - Audio Session Configuration

    private func configureAudioSession() {
        #if os(iOS)
        do {
            let audioSession = AVAudioSession.sharedInstance()
            try audioSession.setCategory(.playback, mode: .default)
            try audioSession.setActive(true)
        } catch {
            print("[AudioNotificationPlayer] Failed to configure audio session: \(error)")
        }
        #endif
    }

    // MARK: - Playback Control

    /// Play an audio file from the given path
    /// - Parameter path: Full path to the audio file
    /// - Throws: Error if playback fails
    func play(path: String) throws {
        let fileURL = URL(fileURLWithPath: path)
        try play(url: fileURL)
    }

    /// Play an audio file from a URL
    /// - Parameter url: URL to the audio file
    /// - Throws: Error if playback fails
    func play(url: URL) throws {
        // Stop any current playback
        stop()

        // Create and configure the audio player
        audioPlayer = try AVAudioPlayer(contentsOf: url)
        audioPlayer?.delegate = self
        audioPlayer?.prepareToPlay()

        guard let player = audioPlayer else {
            throw AudioPlayerError.initializationFailed
        }

        // Start playback
        if player.play() {
            currentFilePath = url
            currentFileName = url.deletingPathExtension().lastPathComponent
            playbackState = .playing
        } else {
            throw AudioPlayerError.playbackFailed
        }
    }

    /// Stop the current playback
    func stop() {
        audioPlayer?.stop()
        audioPlayer = nil
        playbackState = .idle
    }

    /// Pause the current playback
    func pause() {
        audioPlayer?.pause()
        playbackState = .paused
    }

    /// Resume paused playback
    func resume() {
        if playbackState == .paused, let player = audioPlayer {
            player.play()
            playbackState = .playing
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
            // If there's a previous file, replay it
            if let path = currentFilePath {
                try? play(url: path)
            }
        }
    }

    /// Replay the last audio file
    /// - Throws: Error if no file to replay or playback fails
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

    /// Get playback progress (0.0 to 1.0)
    var progress: Double {
        guard let player = audioPlayer, player.duration > 0 else { return 0 }
        return player.currentTime / player.duration
    }
}

// MARK: - AVAudioPlayerDelegate

extension AudioNotificationPlayer: AVAudioPlayerDelegate {
    nonisolated func audioPlayerDidFinishPlaying(_ player: AVAudioPlayer, successfully flag: Bool) {
        Task { @MainActor in
            self.playbackState = .idle
        }
    }

    nonisolated func audioPlayerDecodeErrorDidOccur(_ player: AVAudioPlayer, error: Error?) {
        Task { @MainActor in
            self.playbackState = .idle
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

// MARK: - Audio Playing Indicator View

import SwiftUI

/// A view that shows when audio is playing
struct AudioPlayingIndicator: View {
    @ObservedObject var player = AudioNotificationPlayer.shared
    @State private var animationPhase = 0.0
    @Environment(\.accessibilityReduceMotion) var reduceMotion

    var body: some View {
        if player.isPlaying {
            HStack(spacing: 4) {
                // Animated speaker icon
                Image(systemName: animationPhase > 0.5 ? "speaker.wave.3.fill" : "speaker.wave.2.fill")
                    .foregroundStyle(.green)
                    .font(.caption)

                if let fileName = player.currentFileName {
                    Text(fileName)
                        .font(.caption2)
                        .foregroundStyle(.secondary)
                        .lineLimit(1)
                }

                // Stop button
                Button {
                    player.stop()
                } label: {
                    Image(systemName: "xmark.circle.fill")
                        .foregroundStyle(.secondary)
                        .font(.caption)
                }
                .buttonStyle(.plain)
            }
            .padding(.horizontal, 8)
            .padding(.vertical, 4)
            .background(Color(.systemGray4), in: Capsule())
            .onAppear {
                if !reduceMotion {
                    withAnimation(.easeInOut(duration: 0.5).repeatForever(autoreverses: true)) {
                        animationPhase = 1.0
                    }
                }
            }
        }
    }
}

/// A compact audio control view for embedding in navigation bars or status areas
struct AudioStatusBarView: View {
    @ObservedObject var player = AudioNotificationPlayer.shared

    var body: some View {
        if player.playbackState != .idle {
            HStack(spacing: 8) {
                // Play/Pause toggle
                Button {
                    player.togglePlayPause()
                } label: {
                    Image(systemName: player.isPlaying ? "pause.fill" : "play.fill")
                        .foregroundStyle(.blue)
                }
                .buttonStyle(.plain)

                // Replay button
                Button {
                    try? player.replay()
                } label: {
                    Image(systemName: "arrow.counterclockwise")
                        .foregroundStyle(.blue)
                }
                .buttonStyle(.plain)

                // Stop button
                Button {
                    player.stop()
                } label: {
                    Image(systemName: "stop.fill")
                        .foregroundStyle(.red)
                }
                .buttonStyle(.plain)
            }
        }
    }
}

#Preview("Audio Playing Indicator") {
    VStack {
        AudioPlayingIndicator()
        AudioStatusBarView()
    }
    .padding()
}
