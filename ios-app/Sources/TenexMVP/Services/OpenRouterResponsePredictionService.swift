import Foundation

struct OpenRouterResponsePredictionService {
    private struct ChatRequest: Encodable {
        struct Message: Encodable {
            let role: String
            let content: String
        }

        let model: String
        let messages: [Message]
        let temperature: Double
        // keep completions short: a JSON array of 2-3 brief strings
        let max_tokens: Int
    }

    private struct ChatResponse: Decodable {
        struct Choice: Decodable {
            struct Message: Decodable {
                let content: String
            }

            let message: Message
        }

        let choices: [Choice]
    }

    private struct ErrorEnvelope: Decodable {
        struct OpenRouterError: Decodable {
            let message: String?
        }

        let error: OpenRouterError?
    }

    /// Returns 2-3 short predicted replies for the given conversation history.
    /// The `messages` parameter should be the last ~6 messages in the thread.
    static func predictReplies(
        messages: [Message],
        currentUserPubkey: String?,
        apiKey: String,
        model: String
    ) async throws -> [String] {
        guard let url = URL(string: "https://openrouter.ai/api/v1/chat/completions") else {
            return []
        }

        let contextLines = messages.suffix(6).map { msg -> String in
            let isUser = currentUserPubkey.map {
                msg.pubkey.caseInsensitiveCompare($0) == .orderedSame
            } ?? false
            let role = isUser ? "User" : "Agent"
            let text = msg.content.trimmingCharacters(in: .whitespacesAndNewlines).prefix(300)
            return "\(role): \(text)"
        }.joined(separator: "\n")

        let systemPrompt = """
        You are predicting what the user might want to say next in a conversation with an AI agent.
        Based on the conversation history, generate 2-3 short, natural reply options the user might send.
        Reply ONLY with a JSON array of strings. No markdown, no code fences, no explanation.
        Each string should be brief (under 15 words). Example: ["Thanks!", "Can you expand on that?", "How long will it take?"]
        """

        let userPrompt = """
        Conversation history:
        \(contextLines)

        Generate 2-3 short reply options the user might type next.
        """

        let requestBody = ChatRequest(
            model: model,
            messages: [
                ChatRequest.Message(role: "system", content: systemPrompt),
                ChatRequest.Message(role: "user", content: userPrompt),
            ],
            temperature: 0.8,
            max_tokens: 120
        )

        var request = URLRequest(url: url)
        request.httpMethod = "POST"
        request.timeoutInterval = 15
        request.addValue("application/json", forHTTPHeaderField: "Content-Type")
        request.addValue("Bearer \(apiKey)", forHTTPHeaderField: "Authorization")
        request.httpBody = try JSONEncoder().encode(requestBody)

        let (data, response) = try await URLSession.shared.data(for: request)

        guard let httpResponse = response as? HTTPURLResponse,
              (200...299).contains(httpResponse.statusCode) else {
            return []
        }

        guard let decoded = try? JSONDecoder().decode(ChatResponse.self, from: data),
              let rawContent = decoded.choices.first?.message.content else {
            return []
        }

        return parseSuggestions(from: rawContent)
    }

    private static func parseSuggestions(from raw: String) -> [String] {
        var cleaned = raw.trimmingCharacters(in: .whitespacesAndNewlines)

        // Strip optional markdown code fences
        if cleaned.hasPrefix("```") {
            if let end = cleaned.range(of: "```", range: cleaned.index(cleaned.startIndex, offsetBy: 3)..<cleaned.endIndex) {
                cleaned = String(cleaned[cleaned.index(cleaned.startIndex, offsetBy: 3)..<end.lowerBound])
                    .trimmingCharacters(in: .whitespacesAndNewlines)
                // Strip optional language tag on opening fence
                if let newline = cleaned.firstIndex(of: "\n") {
                    let firstLine = cleaned[..<newline]
                    if !firstLine.contains("[") {
                        cleaned = String(cleaned[cleaned.index(after: newline)...])
                            .trimmingCharacters(in: .whitespacesAndNewlines)
                    }
                }
            }
        }

        guard let jsonData = cleaned.data(using: .utf8),
              let array = try? JSONDecoder().decode([String].self, from: jsonData) else {
            return []
        }

        return array
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
            .prefix(3)
            .map { String($0) }
    }
}
