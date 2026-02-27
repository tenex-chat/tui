#if os(macOS)
import Foundation
import OSLog

/// Streaming Speech-to-Text service using ElevenLabs WebSocket API.
/// Sends base64-encoded PCM audio chunks as JSON and receives real-time transcription responses.
@MainActor
final class ElevenLabsSTTService: NSObject {

    private static let logger = Logger(subsystem: "com.tenex.mvp", category: "ElevenLabsSTT")

    // MARK: - Types

    struct TranscriptResult {
        let text: String
        let isFinal: Bool
        let language: String?
    }

    enum STTError: LocalizedError {
        case noApiKey
        case connectionFailed(String)
        case invalidResponse
        case serverError(String)

        var errorDescription: String? {
            switch self {
            case .noApiKey:
                return "ElevenLabs API key not configured. Add it in Settings."
            case .connectionFailed(let reason):
                return "STT connection failed: \(reason)"
            case .invalidResponse:
                return "Invalid response from ElevenLabs STT"
            case .serverError(let message):
                return "ElevenLabs STT error: \(message)"
            }
        }
    }

    // MARK: - Properties

    private var webSocketTask: URLSessionWebSocketTask?
    private var urlSession: URLSession?
    private var isConnected = false
    private var openContinuation: CheckedContinuation<Void, Error>?

    /// Callback for transcript updates (partial and final)
    var onTranscript: ((TranscriptResult) -> Void)?

    /// Callback for errors
    var onError: ((Error) -> Void)?

    // MARK: - Connection

    /// Connect to ElevenLabs streaming STT WebSocket.
    /// Awaits the actual WebSocket open handshake before returning.
    /// - Throws: `STTError.noApiKey` if no API key is available
    func connect() async throws {
        let apiKeyResult = await KeychainService.shared.loadElevenLabsApiKeyAsync()

        let apiKey: String
        switch apiKeyResult {
        case .success(let key):
            apiKey = key
        case .failure(let err):
            Self.logger.error("connect: keychain load failed: \(err)")
            throw STTError.noApiKey
        }

        let endpoint = "wss://api.elevenlabs.io/v1/speech-to-text/realtime?model_id=scribe_v1&language_code=en&audio_format=pcm_16000"

        guard let url = URL(string: endpoint) else {
            throw STTError.connectionFailed("Invalid endpoint URL")
        }

        Self.logger.info("connect: opening WebSocket to \(url.absoluteString, privacy: .public)")

        var request = URLRequest(url: url)
        request.setValue(apiKey, forHTTPHeaderField: "xi-api-key")

        let session = URLSession(configuration: .default, delegate: self, delegateQueue: nil)
        let task = session.webSocketTask(with: request)
        self.urlSession = session
        self.webSocketTask = task

        // Wait for the handshake to complete before returning — the delegate resolves this.
        try await withCheckedThrowingContinuation { continuation in
            self.openContinuation = continuation
            task.resume()
        }

        isConnected = true
        Self.logger.info("connect: WebSocket open, starting receive loop")
        startReceiving()
    }

    /// Send a PCM audio chunk to the WebSocket as a base64-encoded JSON message.
    /// - Parameter data: Raw PCM audio data (16kHz, mono, 16-bit signed little-endian)
    func sendAudioChunk(_ data: Data) {
        guard isConnected, let task = webSocketTask else { return }

        let base64Audio = data.base64EncodedString()
        let json = "{\"message_type\":\"input_audio_chunk\",\"audio_base_64\":\"\(base64Audio)\"}"

        task.send(.string(json)) { error in
            if let error = error {
                Task { @MainActor in
                    Self.logger.error("sendAudioChunk: send failed: \(error)")
                    self.onError?(STTError.connectionFailed(error.localizedDescription))
                }
            }
        }
    }

    /// Signal end of audio stream and close the connection.
    func endAudioAndDisconnect() {
        guard isConnected else { return }
        disconnect()
    }

    /// Disconnect the WebSocket.
    func disconnect() {
        isConnected = false
        webSocketTask?.cancel(with: .goingAway, reason: nil)
        webSocketTask = nil
        urlSession?.invalidateAndCancel()
        urlSession = nil
    }

    // MARK: - Private

    private func startReceiving() {
        guard isConnected, let task = webSocketTask else { return }

        task.receive { [weak self] result in
            Task { @MainActor in
                guard let self = self, self.isConnected else { return }

                switch result {
                case .success(let message):
                    switch message {
                    case .string(let text):
                        Self.logger.debug("receive: \(text, privacy: .public)")
                    case .data(let data):
                        Self.logger.debug("receive: data(\(data.count) bytes)")
                    @unknown default:
                        break
                    }
                    self.handleMessage(message)
                    self.startReceiving()

                case .failure(let error):
                    if self.isConnected {
                        Self.logger.error("receive: failed: \(error)")
                        self.onError?(STTError.connectionFailed(error.localizedDescription))
                    }
                }
            }
        }
    }

    private func handleMessage(_ message: URLSessionWebSocketTask.Message) {
        switch message {
        case .string(let text):
            parseTranscriptJSON(text)
        case .data(let data):
            if let text = String(data: data, encoding: .utf8) {
                parseTranscriptJSON(text)
            }
        @unknown default:
            break
        }
    }

    private func parseTranscriptJSON(_ jsonString: String) {
        guard let data = jsonString.data(using: .utf8) else { return }

        do {
            guard let json = try JSONSerialization.jsonObject(with: data) as? [String: Any] else {
                Self.logger.warning("parseTranscriptJSON: non-dict JSON: \(jsonString, privacy: .public)")
                return
            }

            if let errorMessage = json["error"] as? String {
                Self.logger.error("parseTranscriptJSON: server error: \(errorMessage, privacy: .public)")
                onError?(STTError.serverError(errorMessage))
                return
            }

            if let text = json["text"] as? String {
                let isFinal = json["is_final"] as? Bool ?? false
                let language = json["language"] as? String
                onTranscript?(TranscriptResult(text: text, isFinal: isFinal, language: language))
            } else if let type = json["type"] as? String, type == "transcript",
                      let resultData = json["data"] as? [String: Any],
                      let text = resultData["text"] as? String {
                let isFinal = resultData["is_final"] as? Bool ?? false
                let language = resultData["language"] as? String
                onTranscript?(TranscriptResult(text: text, isFinal: isFinal, language: language))
            }
        } catch {
            // Silently ignore unparseable messages (e.g., keep-alive pings)
        }
    }
}

// MARK: - URLSessionWebSocketDelegate

extension ElevenLabsSTTService: URLSessionWebSocketDelegate {
    nonisolated func urlSession(
        _ session: URLSession,
        webSocketTask: URLSessionWebSocketTask,
        didOpenWithProtocol protocol: String?
    ) {
        Task { @MainActor in
            Self.logger.info("delegate: didOpen protocol=\((`protocol` ?? "none"), privacy: .public)")
            self.openContinuation?.resume()
            self.openContinuation = nil
        }
    }

    nonisolated func urlSession(
        _ session: URLSession,
        webSocketTask: URLSessionWebSocketTask,
        didCloseWith closeCode: URLSessionWebSocketTask.CloseCode,
        reason: Data?
    ) {
        let reasonString = reason.flatMap { String(data: $0, encoding: .utf8) } ?? ""
        Task { @MainActor in
            Self.logger.info("delegate: didClose code=\(closeCode.rawValue) reason=\(reasonString, privacy: .public)")
            if let continuation = self.openContinuation {
                continuation.resume(throwing: STTError.connectionFailed("Server closed with code \(closeCode.rawValue): \(reasonString)"))
                self.openContinuation = nil
            }
        }
    }

    nonisolated func urlSession(
        _ session: URLSession,
        task: URLSessionTask,
        didCompleteWithError error: Error?
    ) {
        guard let error else { return }
        Task { @MainActor in
            Self.logger.error("delegate: didCompleteWithError: \(error)")
            if let continuation = self.openContinuation {
                continuation.resume(throwing: STTError.connectionFailed(error.localizedDescription))
                self.openContinuation = nil
            } else if self.isConnected {
                self.isConnected = false
                self.onError?(STTError.connectionFailed(error.localizedDescription))
            }
        }
    }
}
#endif
