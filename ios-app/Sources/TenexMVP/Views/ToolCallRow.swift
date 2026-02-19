import SwiftUI

// MARK: - Tool Call Row

/// Compact single-line tool rendering with SF Symbols icons.
/// Displays tool calls in a minimal format showing the most relevant parameter.
struct ToolCallRow: View {
    let toolName: String?
    let toolArgs: String?
    let contentFallback: String?

    init(toolName: String?, toolArgs: String?, contentFallback: String? = nil) {
        self.toolName = toolName
        self.toolArgs = toolArgs
        self.contentFallback = contentFallback
    }

    /// Parsed display info for the tool call
    private var displayInfo: ConversationRenderPolicy.ToolSummary {
        ConversationRenderPolicy.toolSummary(
            toolName: toolName,
            toolArgs: toolArgs,
            contentFallback: contentFallback
        )
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
