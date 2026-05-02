import Foundation

enum OpenRouterModelRole: String, CaseIterable, Identifiable {
    case audioNotifications = "audio_notifications"
    case agentPromptRewrite = "agent_prompt_rewrite"
    case responsePrediction = "response_prediction"

    var id: String { rawValue }

    var title: String {
        switch self {
        case .audioNotifications:
            return "Audio notifications"
        case .agentPromptRewrite:
            return "Agent prompt rewrite"
        case .responsePrediction:
            return "Response prediction"
        }
    }

    var description: String {
        switch self {
        case .audioNotifications:
            return "Rewrites agent messages before speech playback"
        case .agentPromptRewrite:
            return "Generates improved prompts in the agent editor"
        case .responsePrediction:
            return "Predicts what you might want to say next"
        }
    }

    var systemImage: String {
        switch self {
        case .audioNotifications:
            return "waveform"
        case .agentPromptRewrite:
            return "wand.and.sparkles"
        case .responsePrediction:
            return "text.bubble"
        }
    }
}

enum OpenRouterModelSelectionCodec {
    static let multiModelPrefix = "tenex:openrouter_models:v1:"
    static let roleModelPrefix = "tenex:openrouter_roles:v1:"

    static func decodeRoleSelections(from storedValue: String?) -> [OpenRouterModelRole: String] {
        guard let storedValue else { return [:] }
        let trimmed = storedValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return [:] }

        if trimmed.hasPrefix(roleModelPrefix) {
            return decodeEncodedRoleSelections(from: trimmed) ?? [:]
        }

        guard let legacyModel = preferredModel(from: trimmed) else {
            return [:]
        }

        return Dictionary(
            uniqueKeysWithValues: OpenRouterModelRole.allCases.map { role in
                (role, legacyModel)
            }
        )
    }

    static func encodeRoleSelections(_ selections: [OpenRouterModelRole: String]) -> String? {
        let normalized = selections.reduce(into: [String: String]()) { result, element in
            let modelId = element.value.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !modelId.isEmpty else { return }
            result[element.key.rawValue] = modelId
        }

        guard !normalized.isEmpty else { return nil }
        guard let data = try? JSONSerialization.data(withJSONObject: normalized, options: [.sortedKeys]),
              let payload = String(data: data, encoding: .utf8) else {
            return nil
        }
        return roleModelPrefix + payload
    }

    static func selectedModel(for role: OpenRouterModelRole, from storedValue: String?) -> String? {
        let selections = decodeRoleSelections(from: storedValue)
        return selections[role]
    }

    static func decodeSelectedModelIds(from storedValue: String?) -> Set<String> {
        guard let storedValue else { return [] }
        let trimmed = storedValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return [] }

        if trimmed.hasPrefix(roleModelPrefix) {
            return Set((decodeEncodedRoleSelections(from: trimmed) ?? [:]).values)
        }

        if trimmed.hasPrefix(multiModelPrefix) {
            let payload = String(trimmed.dropFirst(multiModelPrefix.count))
            if let data = payload.data(using: .utf8),
               let decoded = try? JSONSerialization.jsonObject(with: data) as? [String] {
                return Set(
                    decoded
                        .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
                        .filter { !$0.isEmpty }
                )
            }
        }

        return [trimmed]
    }

    static func encodeSelectedModelIds(_ modelIds: Set<String>) -> String? {
        let normalized = modelIds
            .map { $0.trimmingCharacters(in: .whitespacesAndNewlines) }
            .filter { !$0.isEmpty }
            .sorted()

        guard !normalized.isEmpty else { return nil }
        if normalized.count == 1 { return normalized[0] }

        guard let data = try? JSONSerialization.data(withJSONObject: normalized),
              let payload = String(data: data, encoding: .utf8) else {
            return normalized[0]
        }
        return multiModelPrefix + payload
    }

    static func preferredModel(from storedValue: String?) -> String? {
        guard let storedValue else { return nil }
        let trimmed = storedValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return nil }

        if trimmed.hasPrefix(roleModelPrefix) {
            let selections = decodeEncodedRoleSelections(from: trimmed) ?? [:]
            return OpenRouterModelRole.allCases.compactMap { selections[$0] }.first
        }

        return decodeSelectedModelIds(from: trimmed).sorted().first
    }

    private static func decodeEncodedRoleSelections(from trimmed: String) -> [OpenRouterModelRole: String]? {
        guard trimmed.hasPrefix(roleModelPrefix) else { return nil }

        let payload = String(trimmed.dropFirst(roleModelPrefix.count))
        guard let data = payload.data(using: .utf8),
              let decoded = try? JSONSerialization.jsonObject(with: data) as? [String: String] else {
            return nil
        }

        return decoded.reduce(into: [OpenRouterModelRole: String]()) { result, element in
            guard let role = OpenRouterModelRole(rawValue: element.key) else { return }
            let modelId = element.value.trimmingCharacters(in: .whitespacesAndNewlines)
            guard !modelId.isEmpty else { return }
            result[role] = modelId
        }
    }
}
