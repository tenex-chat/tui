import Foundation
import AVFoundation
import OSLog
import Observation

/// Manages voice dictation using AVAudioEngine for capture and ElevenLabs streaming STT for transcription.
@MainActor
@Observable
final class DictationManager {

    private static let logger = Logger(subsystem: "com.tenex.mvp", category: "DictationManager")

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
    private(set) var audioLevelSamples: [Float] = []
    private(set) var recordingStartDate: Date?

    private var audioEngine: AVAudioEngine?
    private var sttService: ElevenLabsSTTService?
    private var accumulatedText: String = ""
    // Cumulative partial text for the current VAD segment (reset when a new segment starts)
    private var currentSegmentText: String = ""

    // MARK: - Recording Control

    func startRecording() async throws {
        guard state.isIdle else { return }

        error = nil
        accumulatedText = ""
        currentSegmentText = ""
        audioLevelSamples = []
        recordingStartDate = nil

        let granted = await requestMicrophonePermission()
        guard granted else {
            error = "Microphone access denied. Grant permission in Settings > Privacy & Security > Microphone."
            return
        }

        state = .recording(partialText: "")
        recordingStartDate = Date()

        do {
            Self.logger.info("startRecording: starting streaming")
            try await startStreaming()
            Self.logger.info("startRecording: streaming started successfully")
        } catch {
            Self.logger.error("startRecording: failed with error: \(error)")
            self.error = error.localizedDescription
            state = .idle
        }
    }

    func stopRecording() async {
        guard case .recording(let partialText) = state else { return }

        let capturedPartialText = partialText

        audioEngine?.stop()
        audioEngine?.inputNode.removeTap(onBus: 0)

        sttService?.endAudioAndDisconnect()

        try? await Task.sleep(for: .milliseconds(300))

        audioEngine = nil
        sttService = nil

        // Commit any in-flight segment text that was never finalized
        if !currentSegmentText.isEmpty {
            if !accumulatedText.isEmpty { accumulatedText += " " }
            accumulatedText += currentSegmentText
            currentSegmentText = ""
        }

        if finalText.isEmpty && !capturedPartialText.isEmpty {
            finalText = capturedPartialText
        }
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
        currentSegmentText = ""
        audioLevelSamples = []
        recordingStartDate = nil
        state = .idle
    }

    func reset() {
        finalText = ""
        accumulatedText = ""
        currentSegmentText = ""
        audioLevelSamples = []
        recordingStartDate = nil
        state = .idle
        error = nil
    }

    // MARK: - Private Methods

    private func requestMicrophonePermission() async -> Bool {
#if os(iOS)
        await withCheckedContinuation { continuation in
            AVAudioApplication.requestRecordPermission { granted in
                continuation.resume(returning: granted)
            }
        }
#else
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
#endif
    }

    private func startStreaming() async throws {
        let service = ElevenLabsSTTService()
        sttService = service

        service.onTranscript = { [weak self] result in
            Task { @MainActor in
                guard let self = self else { return }
                guard case .recording = self.state else { return }

                Self.logger.error("onTranscript: isFinal=\(result.isFinal) text='\(result.text, privacy: .public)' accumulated='\(self.accumulatedText, privacy: .public)' segment='\(self.currentSegmentText, privacy: .public)'")

                if result.isFinal {
                    // Use result.text if available; fall back to whatever was in the current segment
                    let toCommit = result.text.isEmpty ? self.currentSegmentText : result.text
                    if !toCommit.isEmpty {
                        if !self.accumulatedText.isEmpty { self.accumulatedText += " " }
                        self.accumulatedText += toCommit
                        self.finalText = self.accumulatedText
                    }
                    self.currentSegmentText = ""
                    self.state = .recording(partialText: self.accumulatedText)
                } else if !result.text.isEmpty {
                    // Detect a new VAD segment: ElevenLabs resets its partial after a commit but
                    // doesn't always send is_final=true. If the new partial doesn't extend the
                    // current segment text, the server started a fresh segment — commit the old one.
                    if !self.currentSegmentText.isEmpty && !result.text.hasPrefix(self.currentSegmentText) {
                        Self.logger.info("onTranscript: new VAD segment detected, committing '\(self.currentSegmentText, privacy: .public)'")
                        if !self.accumulatedText.isEmpty { self.accumulatedText += " " }
                        self.accumulatedText += self.currentSegmentText
                        self.finalText = self.accumulatedText
                    }
                    self.currentSegmentText = result.text
                    let displayText = self.accumulatedText.isEmpty
                        ? self.currentSegmentText
                        : self.accumulatedText + " " + self.currentSegmentText
                    self.state = .recording(partialText: displayText)
                }
            }
        }

        service.onError = { [weak self] error in
            Task { @MainActor in
                Self.logger.error("sttService.onError: \(error)")
                self?.error = error.localizedDescription
            }
        }

        Self.logger.info("startStreaming: connecting to ElevenLabs")
        try await service.connect()
        Self.logger.info("startStreaming: connected")

        try setupAudioEngine()
    }

    private func setupAudioEngine() throws {
#if os(iOS)
        let audioSession = AVAudioSession.sharedInstance()
        try audioSession.setCategory(.record, mode: .measurement, options: .duckOthers)
        try audioSession.setActive(true, options: .notifyOthersOnDeactivation)
#endif

        let engine = AVAudioEngine()
        audioEngine = engine

        let inputNode = engine.inputNode
        let inputFormat = inputNode.outputFormat(forBus: 0)

        guard let targetFormat = AVAudioFormat(
            commonFormat: .pcmFormatInt16,
            sampleRate: 16000,
            channels: 1,
            interleaved: true
        ) else {
            throw DictationError.audioEngineSetupFailed
        }

        guard let converter = AVAudioConverter(from: inputFormat, to: targetFormat) else {
            throw DictationError.audioEngineSetupFailed
        }

        inputNode.installTap(onBus: 0, bufferSize: 4096, format: inputFormat) { [weak self] buffer, _ in
            guard let self = self else { return }

            let frameLength = Int(buffer.frameLength)
            var rmsLevel: Float = 0
            if let channelData = buffer.floatChannelData?[0], frameLength > 0 {
                var sum: Float = 0
                for i in 0..<frameLength {
                    let sample = channelData[i]
                    sum += sample * sample
                }
                rmsLevel = min(sqrt(sum / Float(frameLength)) * 4, 1)
            }

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

            guard let int16Data = convertedBuffer.int16ChannelData else { return }
            let data = Data(
                bytes: int16Data[0],
                count: Int(convertedBuffer.frameLength) * MemoryLayout<Int16>.size
            )

            Task { @MainActor in
                self.sttService?.sendAudioChunk(data)
                self.audioLevelSamples.append(rmsLevel)
                if self.audioLevelSamples.count > 80 {
                    self.audioLevelSamples.removeFirst()
                }
            }
        }

        engine.prepare()
        try engine.start()
        Self.logger.info("setupAudioEngine: started, inputFormat=\(inputFormat, privacy: .public)")
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
