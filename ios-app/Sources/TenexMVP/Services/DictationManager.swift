#if os(iOS)
import Speech
import AVFoundation
import Observation

/// Manages voice dictation with live transcription using iOS 26's SpeechAnalyzer API.
/// Provides real-time streaming transcription with simplified edit-and-insert flow.
@MainActor
@Observable
final class DictationManager {
    enum State: Equatable {
        case idle
        case recording(partialText: String)

        var isIdle: Bool {
            if case .idle = self { return true }
            return false
        }

        var isRecording: Bool {
            if case .recording = self { return true }
            return false
        }
    }

    private(set) var state: State = .idle
    private(set) var finalText: String = ""
    private(set) var error: String?

    private var audioEngine: AVAudioEngine?
    private var transcriptionTask: Task<Void, Never>?
    private var analyzer: SpeechAnalyzer?
    private var transcriber: SpeechTranscriber?
    private var inputBuilder: AsyncStream<AnalyzerInput>.Continuation?

    let phoneticLearner = PhoneticLearner()

    // MARK: - Recording Control

    func startRecording() async throws {
        guard state.isIdle else { return }

        error = nil

        // Request permissions
        let audioPermission = await requestMicrophonePermission()
        guard audioPermission else {
            error = "Microphone access denied"
            return
        }

        let speechPermission = await requestSpeechRecognitionPermission()
        guard speechPermission else {
            error = "Speech recognition access denied"
            return
        }

        state = .recording(partialText: "")

        try await startTranscription()
    }

    func stopRecording() async {
        guard case .recording(let partialText) = state else {
            return
        }

        // Capture the partial text before stopping - transcriber may not produce a final result
        let capturedPartialText = partialText

        // Stop audio engine
        audioEngine?.stop()
        audioEngine?.inputNode.removeTap(onBus: 0)

        // Signal end of audio stream
        inputBuilder?.finish()

        // Cancel the transcription task - the results stream doesn't complete on its own
        transcriptionTask?.cancel()
        transcriptionTask = nil

        // Cleanup
        audioEngine = nil
        inputBuilder = nil
        analyzer = nil
        transcriber = nil

        // If finalText is empty but we had partial text, use that
        if finalText.isEmpty && !capturedPartialText.isEmpty {
            finalText = capturedPartialText
        }


        // Go directly to idle - let user edit in FinalTranscriptionView
        state = .idle
    }

    func cancelRecording() {
        audioEngine?.stop()
        audioEngine?.inputNode.removeTap(onBus: 0)
        audioEngine = nil

        inputBuilder?.finish()
        inputBuilder = nil
        analyzer = nil
        transcriber = nil

        transcriptionTask?.cancel()
        transcriptionTask = nil
        finalText = ""
        state = .idle
    }

    func reset() {
        finalText = ""
        state = .idle
        error = nil
    }

    // MARK: - Private Methods

    private func requestMicrophonePermission() async -> Bool {
        await withCheckedContinuation { continuation in
            AVAudioApplication.requestRecordPermission { granted in
                continuation.resume(returning: granted)
            }
        }
    }

    private func requestSpeechRecognitionPermission() async -> Bool {
        await withCheckedContinuation { continuation in
            SFSpeechRecognizer.requestAuthorization { status in
                continuation.resume(returning: status == .authorized)
            }
        }
    }

    // MARK: - Transcription

    private func startTranscription() async throws {
        // Create transcriber with explicit options for real-time results
        let newTranscriber = SpeechTranscriber(
            locale: Locale.current,
            transcriptionOptions: [],
            reportingOptions: [.volatileResults],
            attributeOptions: []
        )
        transcriber = newTranscriber

        // Get the best audio format for this transcriber
        guard let analyzerFormat = await SpeechAnalyzer.bestAvailableAudioFormat(compatibleWith: [newTranscriber]) else {
            throw DictationError.audioEngineSetupFailed
        }

        // Create analyzer with the transcriber module
        let newAnalyzer = SpeechAnalyzer(modules: [newTranscriber])
        analyzer = newAnalyzer

        // Create async stream for audio input
        let (inputSequence, continuation) = AsyncStream<AnalyzerInput>.makeStream()
        inputBuilder = continuation

        // Start the analyzer
        try await newAnalyzer.start(inputSequence: inputSequence)

        // Setup audio engine
        try await setupAudioEngine(targetFormat: analyzerFormat)

        // Start listening for results
        transcriptionTask = Task { [weak self] in
            guard let self = self else {
                return
            }
            var lastText = ""
            do {
                for try await result in newTranscriber.results {
                    let text = String(result.text.characters)
                    lastText = text
                    await MainActor.run {
                        if result.isFinal {
                            self.finalText = text
                        } else {
                            // Volatile/partial result
                            self.state = .recording(partialText: text)
                        }
                    }
                }
                // Stream ended - if no final result was produced, use the last text we saw
                await MainActor.run {
                    if self.finalText.isEmpty && !lastText.isEmpty {
                        self.finalText = lastText
                    }
                }
            } catch {
                await MainActor.run {
                    self.error = error.localizedDescription
                    // Also capture last text on error
                    if self.finalText.isEmpty && !lastText.isEmpty {
                        self.finalText = lastText
                    }
                }
            }
        }
    }

    private func setupAudioEngine(targetFormat: AVAudioFormat) async throws {
        let audioSession = AVAudioSession.sharedInstance()
        try audioSession.setCategory(.record, mode: .measurement, options: .duckOthers)
        try audioSession.setActive(true, options: .notifyOthersOnDeactivation)

        audioEngine = AVAudioEngine()

        guard let audioEngine = audioEngine else {
            throw DictationError.audioEngineSetupFailed
        }

        let inputNode = audioEngine.inputNode
        let inputFormat = inputNode.outputFormat(forBus: 0)

        // Create converter if formats don't match
        var converter: AVAudioConverter?
        if inputFormat != targetFormat {
            converter = AVAudioConverter(from: inputFormat, to: targetFormat)
        }

        inputNode.installTap(onBus: 0, bufferSize: 4096, format: inputFormat) { [weak self] buffer, _ in
            guard let self = self else { return }

            if let converter = converter {
                // Convert buffer to target format
                let frameCount = AVAudioFrameCount(targetFormat.sampleRate * Double(buffer.frameLength) / inputFormat.sampleRate)
                guard let convertedBuffer = AVAudioPCMBuffer(pcmFormat: targetFormat, frameCapacity: frameCount) else { return }

                var error: NSError?
                converter.convert(to: convertedBuffer, error: &error) { inNumPackets, outStatus in
                    outStatus.pointee = .haveData
                    return buffer
                }

                if error == nil {
                    self.inputBuilder?.yield(AnalyzerInput(buffer: convertedBuffer))
                }
            } else {
                self.inputBuilder?.yield(AnalyzerInput(buffer: buffer))
            }
        }

        audioEngine.prepare()
        try audioEngine.start()
    }

}

// MARK: - Errors

enum DictationError: LocalizedError {
    case audioEngineSetupFailed
    case speechRecognitionFailed
    case permissionDenied

    var errorDescription: String? {
        switch self {
        case .audioEngineSetupFailed:
            return "Failed to setup audio engine"
        case .speechRecognitionFailed:
            return "Speech recognition failed"
        case .permissionDenied:
            return "Permission denied"
        }
    }
}

#elseif os(macOS)
import Foundation
import Observation

/// macOS stub for DictationManager - voice dictation is not available on macOS.
@MainActor
@Observable
final class DictationManager {
    enum State: Equatable {
        case idle
        case recording(partialText: String)

        var isIdle: Bool {
            if case .idle = self { return true }
            return false
        }

        var isRecording: Bool {
            if case .recording = self { return true }
            return false
        }
    }

    private(set) var state: State = .idle
    private(set) var finalText: String = ""
    private(set) var error: String?

    func startRecording() async throws {
        error = "Voice dictation is not available on macOS"
    }

    func stopRecording() async {
        state = .idle
    }

    func cancelRecording() {
        finalText = ""
        state = .idle
    }

    func reset() {
        finalText = ""
        state = .idle
        error = nil
    }
}

enum DictationError: LocalizedError {
    case audioEngineSetupFailed
    case speechRecognitionFailed
    case permissionDenied

    var errorDescription: String? {
        switch self {
        case .audioEngineSetupFailed:
            return "Failed to setup audio engine"
        case .speechRecognitionFailed:
            return "Speech recognition failed"
        case .permissionDenied:
            return "Permission denied"
        }
    }
}
#endif
