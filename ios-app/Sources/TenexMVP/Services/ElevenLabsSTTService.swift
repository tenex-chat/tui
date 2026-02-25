#if os(macOS)
import Foundation

/// Streaming Speech-to-Text service using ElevenLabs WebSocket API.
/// Sends binary PCM audio chunks and receives real-time JSON transcription responses.
@MainActor
final class ElevenLabsSTTService {

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

    /// Callback for transcript updates (partial and final)
    var onTranscript: ((TranscriptResult) -> Void)?

    /// Callback for errors
    var onError: ((Error) -> Void)?

    // MARK: - Connection

    /// Connect to ElevenLabs streaming STT WebSocket.
    /// - Throws: `STTError.noApiKey` if no API key is available
    func connect() async throws {
        let apiKeyResult = await KeychainService.shared.loadElevenLabsApiKeyAsync()

        let apiKey: String
        switch apiKeyResult {
        case .success(let key):
            apiKey = key
        case .failure:
            throw STTError.noApiKey
        }

        let endpoint = "wss://api.elevenlabs.io/v1/stt/streaming?model_id=scribe_v1&language_code=en&encoding=pcm_s16le&sample_rate=16000"

        guard let url = URL(string: endpoint) else {
            throw STTError.connectionFailed("Invalid endpoint URL")
        }

        var request = URLRequest(url: url)
        request.setValue(apiKey, forHTTPHeaderField: "xi-api-key")

        let session = URLSession(configuration: .default)
        let task = session.webSocketTask(with: request)

        self.urlSession = session
        self.webSocketTask = task

        task.resume()
        isConnected = true

        // Start listening for responses
        startReceiving()
    }

    /// Send a binary audio chunk to the WebSocket.
    /// - Parameter data: Raw PCM audio data (16kHz, mono, 16-bit signed little-endian)
    func sendAudioChunk(_ data: Data) {
        guard isConnected, let task = webSocketTask else { return }

        task.send(.data(data)) { error in
            if let error = error {
                Task { @MainActor in
                    self.onError?(STTError.connectionFailed(error.localizedDescription))
                }
            }
        }
    }

    /// Signal end of audio stream and close the connection.
    func endAudioAndDisconnect() {
        guard isConnected else { return }

        // Send end_audio signal as text message
        let endMessage = "{\"end_audio\": true}"
        webSocketTask?.send(.string(endMessage)) { [weak self] _ in
            // Close after sending end signal
            Task { @MainActor in
                self?.disconnect()
            }
        }
    }

    /// Disconnect the WebSocket without sending end signal.
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
                    self.handleMessage(message)
                    // Continue receiving
                    self.startReceiving()

                case .failure(let error):
                    // Only report if still connected (not a deliberate disconnect)
                    if self.isConnected {
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
                return
            }

            // Check for error messages
            if let errorMessage = json["error"] as? String {
                onError?(STTError.serverError(errorMessage))
                return
            }

            // Parse transcript response
            // ElevenLabs Scribe streaming returns: {"type": "transcript", "data": {"text": "...", ...}}
            // or simpler format: {"text": "...", "is_final": true/false}
            if let text = json["text"] as? String {
                let isFinal = json["is_final"] as? Bool ?? false
                let language = json["language"] as? String

                let result = TranscriptResult(
                    text: text,
                    isFinal: isFinal,
                    language: language
                )
                onTranscript?(result)
            } else if let type = json["type"] as? String, type == "transcript",
                      let resultData = json["data"] as? [String: Any],
                      let text = resultData["text"] as? String {
                let isFinal = resultData["is_final"] as? Bool ?? false
                let language = resultData["language"] as? String

                let result = TranscriptResult(
                    text: text,
                    isFinal: isFinal,
                    language: language
                )
                onTranscript?(result)
            }
        } catch {
            // Silently ignore unparseable messages (e.g., keep-alive pings)
        }
    }
}
#endif
