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
import AVFoundation
import Observation

/// Manages voice dictation on macOS using AVAudioEngine for capture and ElevenLabs streaming STT for transcription.
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
    private var sttService: ElevenLabsSTTService?
    private var accumulatedText: String = ""

    // MARK: - Recording Control

    func startRecording() async throws {
        guard state.isIdle else { return }

        error = nil
        accumulatedText = ""

        // Request microphone permission
        let granted = await requestMicrophonePermission()
        guard granted else {
            error = "Microphone access denied. Grant permission in System Settings > Privacy & Security > Microphone."
            return
        }

        state = .recording(partialText: "")

        do {
            try await startStreaming()
        } catch {
            self.error = error.localizedDescription
            state = .idle
        }
    }

    func stopRecording() async {
        guard case .recording(let partialText) = state else { return }

        let capturedPartialText = partialText

        // Stop audio engine
        audioEngine?.stop()
        audioEngine?.inputNode.removeTap(onBus: 0)

        // Signal end of audio to ElevenLabs
        sttService?.endAudioAndDisconnect()

        // Give a brief moment for any final transcript to arrive
        try? await Task.sleep(for: .milliseconds(300))

        // Cleanup
        audioEngine = nil
        sttService = nil

        // If finalText is empty but we had partial text, use that
        if finalText.isEmpty && !capturedPartialText.isEmpty {
            finalText = capturedPartialText
        }

        // If we still have no finalText but accumulated text, use that
        if finalText.isEmpty && !accumulatedText.isEmpty {
            finalText = accumulatedText
        }

        state = .idle
    }

    func cancelRecording() {
        audioEngine?.stop()
        audioEngine?.inputNode.removeTap(onBus: 0)
        audioEngine = nil

        sttService?.disconnect()
        sttService = nil

        finalText = ""
        accumulatedText = ""
        state = .idle
    }

    func reset() {
        finalText = ""
        accumulatedText = ""
        state = .idle
        error = nil
    }

    // MARK: - Private Methods

    private func requestMicrophonePermission() async -> Bool {
        switch AVCaptureDevice.authorizationStatus(for: .audio) {
        case .authorized:
            return true
        case .notDetermined:
            return await AVCaptureDevice.requestAccess(for: .audio)
        case .denied, .restricted:
            return false
        @unknown default:
            return false
        }
    }

    private func startStreaming() async throws {
        // Create and connect STT service
        let service = ElevenLabsSTTService()
        sttService = service

        service.onTranscript = { [weak self] result in
            Task { @MainActor in
                guard let self = self else { return }
                guard case .recording = self.state else { return }

                if result.isFinal {
                    // Accumulate final segments
                    if !result.text.isEmpty {
                        if !self.accumulatedText.isEmpty {
                            self.accumulatedText += " "
                        }
                        self.accumulatedText += result.text
                        self.finalText = self.accumulatedText
                    }
                    self.state = .recording(partialText: self.accumulatedText)
                } else {
                    // Show accumulated text + current partial
                    let displayText: String
                    if self.accumulatedText.isEmpty {
                        displayText = result.text
                    } else {
                        displayText = self.accumulatedText + " " + result.text
                    }
                    self.state = .recording(partialText: displayText)
                }
            }
        }

        service.onError = { [weak self] error in
            Task { @MainActor in
                self?.error = error.localizedDescription
            }
        }

        try await service.connect()

        // Setup audio engine after connection
        try setupAudioEngine()
    }

    private func setupAudioEngine() throws {
        let engine = AVAudioEngine()
        audioEngine = engine

        let inputNode = engine.inputNode
        let inputFormat = inputNode.outputFormat(forBus: 0)

        // Target format: 16kHz mono PCM 16-bit signed little-endian
        guard let targetFormat = AVAudioFormat(
            commonFormat: .pcmFormatInt16,
            sampleRate: 16000,
            channels: 1,
            interleaved: true
        ) else {
            throw DictationError.audioEngineSetupFailed
        }

        // Create converter from mic format to target format
        guard let converter = AVAudioConverter(from: inputFormat, to: targetFormat) else {
            throw DictationError.audioEngineSetupFailed
        }

        inputNode.installTap(onBus: 0, bufferSize: 4096, format: inputFormat) { [weak self] buffer, _ in
            guard let self = self else { return }

            // Calculate output frame count based on sample rate ratio
            let ratio = targetFormat.sampleRate / inputFormat.sampleRate
            let outputFrameCount = AVAudioFrameCount(Double(buffer.frameLength) * ratio)
            guard outputFrameCount > 0 else { return }

            guard let convertedBuffer = AVAudioPCMBuffer(
                pcmFormat: targetFormat,
                frameCapacity: outputFrameCount
            ) else { return }

            var conversionError: NSError?
            converter.convert(to: convertedBuffer, error: &conversionError) { _, outStatus in
                outStatus.pointee = .haveData
                return buffer
            }

            guard conversionError == nil, convertedBuffer.frameLength > 0 else { return }

            // Extract raw PCM data from buffer
            guard let int16Data = convertedBuffer.int16ChannelData else { return }
            let data = Data(
                bytes: int16Data[0],
                count: Int(convertedBuffer.frameLength) * MemoryLayout<Int16>.size
            )

            Task { @MainActor in
                self.sttService?.sendAudioChunk(data)
            }
        }

        engine.prepare()
        try engine.start()
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
