import SwiftUI

// MARK: - Tool Call Row

/// Compact single-line tool rendering with SF Symbols icons.
/// Displays tool calls in a minimal format showing the most relevant parameter.
struct ToolCallRow: View {
    let toolName: String?
    let toolArgs: String?

    /// Parsed display info for the tool call
    private var displayInfo: ToolDisplayInfo {
        parseToolCall()
    }

    var body: some View {
        HStack(spacing: 8) {
            // Tool icon
            Image(systemName: displayInfo.icon)
                .font(.caption)
                .foregroundStyle(.secondary)
                .frame(width: 16)

            // Tool display text
            Text(displayInfo.text)
                .font(.system(.caption, design: .monospaced))
                .foregroundStyle(.secondary)
                .lineLimit(1)
                .truncationMode(.middle)
        }
        .padding(.horizontal, 10)
        .padding(.vertical, 6)
        .background(Color.systemGray6)
        .clipShape(RoundedRectangle(cornerRadius: 6))
    }

    // MARK: - Tool Parsing

    private struct ToolDisplayInfo {
        let icon: String
        let text: String
    }

    private func parseToolCall() -> ToolDisplayInfo {
        let name = (toolName ?? "").lowercased()

        // Parse tool arguments JSON
        var args: [String: Any] = [:]
        if let argsJson = toolArgs,
           let data = argsJson.data(using: .utf8),
           let parsed = try? JSONSerialization.jsonObject(with: data) as? [String: Any] {
            args = parsed
        }

        // Match tool patterns and extract relevant info
        if name.contains("bash") {
            let command = extractString(from: args, keys: ["command", "cmd"])
            return ToolDisplayInfo(icon: "terminal", text: "$ \(command)")
        }

        if name.contains("read") {
            let filePath = extractString(from: args, keys: ["file_path", "path", "file"])
            return ToolDisplayInfo(icon: "doc.text", text: filePath)
        }

        // IMPORTANT: Check todo_write BEFORE generic write/edit check.
        // mcp__tenex__todo_write contains "write" so would match the generic pattern otherwise.
        if name.contains("todo_write") || name.contains("todowrite") {
            return ToolDisplayInfo(icon: "checklist", text: "Updated todos")
        }

        if name.contains("write") || name.contains("edit") {
            let filePath = extractString(from: args, keys: ["file_path", "path", "file"])
            return ToolDisplayInfo(icon: "square.and.pencil", text: filePath)
        }

        if name.contains("glob") || name.contains("grep") || name.contains("search") {
            let pattern = extractString(from: args, keys: ["pattern", "query", "glob"])
            return ToolDisplayInfo(icon: "magnifyingglass", text: pattern)
        }

        if name.contains("task") || name.contains("agent") {
            let description = extractString(from: args, keys: ["description", "prompt", "task"])
            return ToolDisplayInfo(icon: "play.fill", text: description)
        }

        if name.contains("web") || name.contains("fetch") {
            let url = extractString(from: args, keys: ["url", "uri"])
            return ToolDisplayInfo(icon: "globe", text: url)
        }

        if name.contains("mcp") {
            // Generic MCP tool fallback - show the extracted tool name
            // NOTE: MCP tools with specific patterns (todo_write, write, task, etc.)
            // are handled by earlier checks. This only catches truly unknown MCP tools.
            let shortName = extractMcpToolName(from: toolName ?? "")
            return ToolDisplayInfo(icon: "puzzlepiece", text: shortName)
        }

        // Default fallback
        let shortName = toolName?.split(separator: "_").last.map(String.init) ?? "tool"
        return ToolDisplayInfo(icon: "wrench", text: shortName)
    }

    /// Extract a string value from args dictionary, trying multiple keys
    private func extractString(from args: [String: Any], keys: [String]) -> String {
        for key in keys {
            if let value = args[key] as? String, !value.isEmpty {
                return value
            }
        }
        return "..."
    }

    /// Extract a readable name from MCP tool name (e.g., "mcp__tenex__report_write" -> "report_write")
    private func extractMcpToolName(from fullName: String) -> String {
        let parts = fullName.split(separator: "_")
        // Skip "mcp" and server name prefixes
        if parts.count >= 4 {
            return parts.suffix(from: 3).joined(separator: "_")
        }
        return fullName
    }
}

// MARK: - Preview

#Preview {
    VStack(alignment: .leading, spacing: 12) {
        ToolCallRow(toolName: "bash", toolArgs: "{\"command\": \"ls -la\"}")
        ToolCallRow(toolName: "read", toolArgs: "{\"file_path\": \"/src/main.swift\"}")
        ToolCallRow(toolName: "write", toolArgs: "{\"file_path\": \"/src/new_file.swift\"}")
        ToolCallRow(toolName: "glob", toolArgs: "{\"pattern\": \"**/*.swift\"}")
        ToolCallRow(toolName: "mcp__tenex__task", toolArgs: "{\"description\": \"Analyze codebase structure\"}")
        ToolCallRow(toolName: "todo_write", toolArgs: "{}")
        ToolCallRow(toolName: "mcp__tenex__todo_write", toolArgs: "{}")
        ToolCallRow(toolName: "web_fetch", toolArgs: "{\"url\": \"https://example.com\"}")
    }
    .padding()
}
