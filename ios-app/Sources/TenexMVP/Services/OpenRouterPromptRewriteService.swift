import Foundation

enum OpenRouterPromptRewriteError: LocalizedError {
    case invalidResponse
    case invalidStatus(code: Int, message: String)
    case emptyOutput

    var errorDescription: String? {
        switch self {
        case .invalidResponse:
            return "OpenRouter returned an invalid response."
        case .invalidStatus(let code, let message):
            if message.isEmpty {
                return "OpenRouter request failed (\(code))."
            }
            return "OpenRouter request failed (\(code)): \(message)"
        case .emptyOutput:
            return "OpenRouter returned an empty prompt."
        }
    }
}

struct OpenRouterPromptRewriteService {
    private struct ChatRequest: Encodable {
        struct Message: Encodable {
            let role: String
            let content: String
        }

        let model: String
        let messages: [Message]
        let temperature: Double
    }

    private struct ErrorEnvelope: Decodable {
        struct OpenRouterError: Decodable {
            let message: String?
        }

        let error: OpenRouterError?
    }

    private struct ChatResponse: Decodable {
        struct Choice: Decodable {
            struct Message: Decodable {
                struct ContentChunk: Decodable {
                    let text: String?
                }

                let content: String

                private enum CodingKeys: String, CodingKey {
                    case content
                }

                init(from decoder: Decoder) throws {
                    let container = try decoder.container(keyedBy: CodingKeys.self)

                    if let direct = try? container.decode(String.self, forKey: .content) {
                        content = direct
                        return
                    }

                    if let chunks = try? container.decode([ContentChunk].self, forKey: .content) {
                        content = chunks
                            .compactMap(\.text)
                            .joined(separator: "\n")
                        return
                    }

                    content = ""
                }
            }

            let message: Message
        }

        let choices: [Choice]
    }

    static func rewritePrompt(
        currentPrompt: String,
        rewriteInstruction: String,
        apiKey: String,
        model: String
    ) async throws -> String {
        guard let url = URL(string: "https://openrouter.ai/api/v1/chat/completions") else {
            throw OpenRouterPromptRewriteError.invalidResponse
        }

        let prompt = """
        You are an expert in writing system prompts for AI agents.
        A user wants to modify an existing system prompt.

        Here is the current system prompt:
        ---
        \(currentPrompt.isEmpty ? "No prompt provided yet - create a new one from scratch." : currentPrompt)
        ---

        Here is the user's instruction for how to change it:
        ---
        \(rewriteInstruction)
        ---

        Please generate the new system prompt based on the user's instruction.
        Respond ONLY with the full, rewritten system prompt text. Do not add any extra explanations or markdown code blocks.
        """

        let requestBody = ChatRequest(
            model: model,
            messages: [ChatRequest.Message(role: "user", content: prompt)],
            temperature: 0.3
        )

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 45
        request.addValue("application/json", forHTTPHeaderField: "Content-Type")
        request.addValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.httpBody = try JSONEncoder().encode(requestBody)

        let (data, response) = try await URLSession.shared.data(for: request)

        guard let httpResponse = response as? HTTPURLResponse else {
            throw OpenRouterPromptRewriteError.invalidResponse
        }

        guard (200...299).contains(httpResponse.statusCode) else {
            let message = (try? JSONDecoder().decode(ErrorEnvelope.self, from: data).error?.message)?
                .trimmingCharacters(in: .whitespacesAndNewlines) ?? ""
            throw OpenRouterPromptRewriteError.invalidStatus(code: httpResponse.statusCode, message: message)
        }

        let decoded = try JSONDecoder().decode(ChatResponse.self, from: data)
        let output = decoded.choices.first?.message.content.trimmingCharacters(in: .whitespacesAndNewlines) ?? ""

        guard !output.isEmpty else {
            throw OpenRouterPromptRewriteError.emptyOutput
        }

        return output
    }
}
