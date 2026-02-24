import Foundation

/// Parsed preview data for a bunker signing request.
struct BunkerSignPreviewModel {
    struct AgentDefinitionPreview {
        let title: String?
        let role: String?
        let description: String?
        let category: String?
        let version: String?
        let dTag: String?
        let instructionsFromTags: [String]
        let contentMarkdown: String
        let useCriteria: [String]
        let tools: [String]
        let mcpServers: [String]
        let fileEventIds: [String]
    }

    let kind: UInt16?
    let rawEventJson: String
    let agentDefinition: AgentDefinitionPreview?

    var isAgentDefinition4199: Bool {
        kind == 4199 && agentDefinition != nil
    }

    init(request: FfiBunkerSignRequest) {
        let parsedObject = Self.parseObject(json: request.eventJson)
        let tagsFromEvent = Self.normalizeTags(from: parsedObject?["tags"])
        let fallbackTags = Self.parseTagsJson(request.eventTagsJson)
        let resolvedTags = tagsFromEvent.isEmpty ? fallbackTags : tagsFromEvent

        var normalizedEvent: [String: Any] = parsedObject ?? [:]
        if normalizedEvent["kind"] == nil, let kind = request.eventKind {
            normalizedEvent["kind"] = Int(kind)
        }
        if normalizedEvent["content"] == nil, let content = request.eventContent {
            normalizedEvent["content"] = content
        }
        if normalizedEvent["tags"] == nil, !resolvedTags.isEmpty {
            normalizedEvent["tags"] = resolvedTags
        }

        let resolvedKind = Self.kind(from: normalizedEvent["kind"]) ?? request.eventKind
        let resolvedContent = (normalizedEvent["content"] as? String) ?? request.eventContent ?? ""

        self.kind = resolvedKind
        self.rawEventJson = Self.prettyJson(from: normalizedEvent)
            ?? request.eventJson
            ?? "(unable to serialize event)"

        if resolvedKind == 4199 {
            self.agentDefinition = Self.buildAgentDefinitionPreview(
                content: resolvedContent,
                tags: resolvedTags
            )
        } else {
            self.agentDefinition = nil
        }
    }

    private static func buildAgentDefinitionPreview(
        content: String,
        tags: [[Any]]
    ) -> AgentDefinitionPreview {
        let valuesByTag = tagValuesByName(tags: tags)

        return AgentDefinitionPreview(
            title: firstValue(for: "title", in: valuesByTag),
            role: firstValue(for: "role", in: valuesByTag),
            description: firstValue(for: "description", in: valuesByTag),
            category: firstValue(for: "category", in: valuesByTag),
            version: firstValue(for: "ver", in: valuesByTag) ?? firstValue(for: "version", in: valuesByTag),
            dTag: firstValue(for: "d", in: valuesByTag),
            instructionsFromTags: valuesByTag["instructions"] ?? [],
            contentMarkdown: content,
            useCriteria: valuesByTag["use-criteria"] ?? [],
            tools: valuesByTag["tool"] ?? [],
            mcpServers: valuesByTag["mcp"] ?? [],
            fileEventIds: valuesByTag["e"] ?? []
        )
    }

    private static func tagValuesByName(tags: [[Any]]) -> [String: [String]] {
        var values: [String: [String]] = [:]

        for tag in tags {
            guard tag.count >= 2 else { continue }
            guard let tagName = nonEmptyString(tag[0]) else { continue }
            guard let tagValue = nonEmptyString(tag[1]) else { continue }
            values[tagName, default: []].append(tagValue)
        }

        return values
    }

    private static func firstValue(for key: String, in valuesByTag: [String: [String]]) -> String? {
        valuesByTag[key]?.first
    }

    private static func parseObject(json: String?) -> [String: Any]? {
        guard let json,
              let data = json.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return nil
        }
        return object
    }

    private static func parseTagsJson(_ tagsJson: String?) -> [[Any]] {
        guard let tagsJson,
              let data = tagsJson.data(using: .utf8),
              let object = try? JSONSerialization.jsonObject(with: data)
        else {
            return []
        }
        return normalizeTags(from: object)
    }

    private static func normalizeTags(from value: Any?) -> [[Any]] {
        guard let rows = value as? [Any] else { return [] }
        return rows.compactMap { $0 as? [Any] }
    }

    private static func kind(from value: Any?) -> UInt16? {
        if let number = value as? NSNumber {
            let intValue = number.intValue
            guard intValue >= 0, intValue <= Int(UInt16.max) else { return nil }
            return UInt16(intValue)
        }
        if let text = value as? String {
            return UInt16(text)
        }
        return nil
    }

    private static func prettyJson(from object: [String: Any]) -> String? {
        guard JSONSerialization.isValidJSONObject(object),
              let data = try? JSONSerialization.data(
                  withJSONObject: object,
                  options: [.prettyPrinted, .sortedKeys]
              ),
              let string = String(data: data, encoding: .utf8)
        else {
            return nil
        }
        return string
    }

    private static func nonEmptyString(_ value: Any?) -> String? {
        switch value {
        case let string as String:
            return string.isEmpty ? nil : string
        case let number as NSNumber:
            let stringValue = number.stringValue
            return stringValue.isEmpty ? nil : stringValue
        default:
            return nil
        }
    }
}
