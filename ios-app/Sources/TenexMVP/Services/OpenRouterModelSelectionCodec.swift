import Foundation

enum OpenRouterModelSelectionCodec {
    static let multiModelPrefix = "tenex:openrouter_models:v1:"

    static func decodeSelectedModelIds(from storedValue: String?) -> Set<String> {
        guard let storedValue else { return [] }
        let trimmed = storedValue.trimmingCharacters(in: .whitespacesAndNewlines)
        guard !trimmed.isEmpty else { return [] }

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
        decodeSelectedModelIds(from: storedValue).sorted().first
    }
}
