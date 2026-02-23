import Foundation

/// Shared conversation render policy matching TUI semantics for tool summaries
/// and q-tag behavior.
enum ConversationRenderPolicy {
    /// Tools that use q-tags for internal references (not delegation/ask previews).
    static let qTagRenderDenylist: Set<String> = [
        "mcp__tenex__report_write",
        "mcp__tenex__report_read",
        "mcp__tenex__report_delete",
        "mcp__tenex__lesson_learn",
        "mcp__tenex__lesson_get"
    ]

    struct ToolSummary {
        let icon: String
        let text: String
    }

    static func shouldRenderQTags(toolName: String?) -> Bool {
        guard let toolName else { return true }
        return !qTagRenderDenylist.contains(toolName)
    }

    static func toolSummary(
        toolName: String?,
        toolArgs: String?,
        contentFallback: String?
    ) -> ToolSummary {
        let normalizedName = (toolName ?? "").lowercased()
        let args = parseArgs(toolArgs)

        if isTodoWrite(normalizedName) {
            let count = todoCount(from: args)
            return ToolSummary(icon: icon(for: normalizedName), text: "▸ \(count) tasks")
        }

        let text: String
        let target = extractTarget(toolName: normalizedName, args: args) ?? ""

        switch normalizedName {
        case "bash", "execute_bash", "shell":
            text = "$ \(target)"
        case "ask", "askuserquestion":
            let title = nonEmptyString(args["title"]) ?? "Question"
            let headers = askHeaders(from: args["questions"])
            if headers.isEmpty {
                text = "Asking: \"\(title)\""
            } else {
                text = "Asking: \"\(title)\" [\(headers.joined(separator: ", "))]"
            }
        case "read", "file_read", "fs_read":
            text = "📖 \(target)"
        case "write", "file_write", "fs_write", "edit", "str_replace_editor", "fs_edit":
            text = "✏️ \(target)"
        case "glob", "find", "grep", "search", "web_search", "websearch", "fs_glob", "fs_grep":
            text = "🔍 \(target)"
        case "task", "agent":
            let description = nonEmptyString(args["description"]) ?? "agent"
            text = "▶ \(truncate(description, max: 40))"
        case "change_model":
            let variant = nonEmptyString(args["variant"]) ?? "default"
            text = "🧠 → \(variant)"
        case "conversation_get", "mcp__tenex__conversation_get":
            let conversationId = nonEmptyString(args["conversationId"])
                ?? nonEmptyString(args["conversation_id"])
                ?? "unknown"
            let conversationDisplay = truncate(conversationId, max: 12)
            if let prompt = nonEmptyString(args["prompt"]) {
                text = "📜 \(conversationDisplay) → \"\(truncate(prompt, max: 50))\""
            } else {
                text = "📜 \(conversationDisplay)"
            }
        default:
            if let description = nonEmptyString(args["description"] as? String) {
                text = truncate(description, max: 80)
            } else if let fallback = contentFallback?.trimmingCharacters(in: .whitespacesAndNewlines),
               !fallback.isEmpty {
                text = truncate(fallback, max: 80)
            } else {
                let verb = toolVerb(for: normalizedName)
                if verb.isEmpty {
                    if target.isEmpty {
                        text = normalizedName.isEmpty ? "tool" : normalizedName
                    } else if normalizedName.isEmpty {
                        text = target
                    } else {
                        text = "\(normalizedName) \(target)"
                    }
                } else if target.isEmpty {
                    text = verb
                } else {
                    text = "\(verb) \(target)"
                }
            }
        }

        return ToolSummary(icon: icon(for: normalizedName), text: text)
    }

    private static func icon(for toolName: String) -> String {
        switch toolName {
        case let name where isTodoWrite(name):
            return "checklist"
        case "bash", "execute_bash", "shell":
            return "terminal"
        case "read", "file_read", "fs_read":
            return "doc.text"
        case "write", "file_write", "fs_write", "edit", "str_replace_editor", "fs_edit":
            return "square.and.pencil"
        case "glob", "find", "grep", "search", "web_search", "websearch", "fs_glob", "fs_grep":
            return "magnifyingglass"
        case "task", "agent":
            return "play.fill"
        case "ask", "askuserquestion":
            return "questionmark.circle"
        case "change_model":
            return "brain"
        case "conversation_get", "mcp__tenex__conversation_get":
            return "doc.text.magnifyingglass"
        default:
            return toolName.hasPrefix("mcp__") ? "puzzlepiece" : "wrench"
        }
    }

    private static func toolVerb(for toolName: String) -> String {
        switch toolName {
        case "read", "file_read", "fs_read":
            return "Reading"
        case "write", "file_write", "fs_write":
            return "Writing"
        case "edit", "str_replace_editor", "fs_edit":
            return "Editing"
        case "bash", "execute_bash", "shell":
            return ""
        case "glob", "find", "grep", "search", "web_search", "websearch", "fs_glob", "fs_grep":
            return "Searching"
        case "task", "agent":
            return ""
        default:
            return "Executing"
        }
    }

    private static func extractTarget(toolName: String, args: [String: Any]) -> String? {
        if matchesFileOperation(toolName),
           let description = nonEmptyString(args["description"]) {
            return truncate(description, max: 60)
        }

        if matchesShellOperation(toolName) {
            if let description = nonEmptyString(args["description"]) {
                return description
            }
            if let command = nonEmptyString(args["command"]) {
                return truncate(command, max: 50)
            }
        }

        for key in ["file_path", "path", "filePath", "file", "target"] {
            if let value = nonEmptyString(args[key]) {
                let parts = value.split(separator: "/", omittingEmptySubsequences: true)
                if parts.count > 2 {
                    return ".../\(parts.suffix(2).joined(separator: "/"))"
                }
                return value
            }
        }

        if let pattern = nonEmptyString(args["pattern"]) {
            return "\"\(truncate(pattern, max: 30))\""
        }

        if let query = nonEmptyString(args["query"]) {
            return "\"\(truncate(query, max: 30))\""
        }

        return nil
    }

    private static func parseArgs(_ toolArgs: String?) -> [String: Any] {
        guard let toolArgs,
              let data = toolArgs.data(using: .utf8),
              let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any]
        else {
            return [:]
        }
        return parsed
    }

    private static func askHeaders(from value: Any?) -> [String] {
        guard let rawQuestions = value as? [Any] else { return [] }
        return rawQuestions.compactMap { raw in
            guard let question = raw as? [String: Any],
                  let header = nonEmptyString(question["header"])
            else {
                return nil
            }
            return header
        }
    }

    private static func todoCount(from args: [String: Any]) -> Int {
        if let todos = args["todos"] as? [Any] {
            return todos.count
        }
        if let items = args["items"] as? [Any] {
            return items.count
        }
        return 0
    }

    private static func isTodoWrite(_ name: String) -> Bool {
        matches(
            name,
            candidates: ["todo_write", "todowrite", "mcp__tenex__todo_write"]
        )
    }

    private static func matchesFileOperation(_ name: String) -> Bool {
        matches(
            name,
            candidates: [
                "read", "file_read", "fs_read",
                "write", "file_write", "fs_write",
                "edit", "str_replace_editor", "fs_edit"
            ]
        )
    }

    private static func matchesShellOperation(_ name: String) -> Bool {
        matches(name, candidates: ["bash", "execute_bash", "shell"])
    }

    private static func matches(_ value: String, candidates: [String]) -> Bool {
        candidates.contains(value)
    }

    private static func nonEmptyString(_ value: Any?) -> String? {
        guard let string = value as? String else { return nil }
        let trimmed = string.trimmingCharacters(in: .whitespacesAndNewlines)
        return trimmed.isEmpty ? nil : trimmed
    }

    private static func truncate(_ text: String, max: Int) -> String {
        guard max > 0 else { return "" }
        guard text.count > max else { return text }
        if max <= 3 {
            return String(text.prefix(max))
        }
        return String(text.prefix(max - 3)) + "..."
    }
}
