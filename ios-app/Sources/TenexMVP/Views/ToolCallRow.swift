import SwiftUI

// MARK: - Tool Call Row

/// Compact single-line tool rendering with SF Symbols icons.
/// Tapping the row opens a ToolCallDetailSheet with full args.
struct ToolCallRow: View {
    let toolName: String?
    let toolArgs: String?
    let contentFallback: String?

    @State private var showDetail = false

    init(toolName: String?, toolArgs: String?, contentFallback: String? = nil) {
        self.toolName = toolName
        self.toolArgs = toolArgs
        self.contentFallback = contentFallback
    }

    private var displayInfo: ConversationRenderPolicy.ToolSummary {
        ConversationRenderPolicy.toolSummary(
            toolName: toolName,
            toolArgs: toolArgs,
            contentFallback: contentFallback
        )
    }

    var body: some View {
        Button {
            showDetail = true
        } label: {
            HStack(spacing: 8) {
                Image(systemName: displayInfo.icon)
                    .font(.caption)
                    .foregroundStyle(.secondary)
                    .frame(width: 16)

                Text(displayInfo.text)
                    .font(.system(.caption, design: .monospaced))
                    .foregroundStyle(.secondary)
                    .lineLimit(1)
                    .truncationMode(.middle)

                Image(systemName: "chevron.right")
                    .font(.caption2)
                    .foregroundStyle(.tertiary)
            }
            .padding(.horizontal, 10)
            .padding(.vertical, 6)
            .background(Color.systemGray6)
            .clipShape(RoundedRectangle(cornerRadius: 6))
        }
        .buttonStyle(.plain)
        .sheet(isPresented: $showDetail) {
            ToolCallDetailSheet(
                toolName: toolName,
                toolArgs: toolArgs,
                contentFallback: contentFallback
            )
        }
    }
}

// MARK: - Tool Call Detail Sheet

struct ToolCallDetailSheet: View {
    let toolName: String?
    let toolArgs: String?
    let contentFallback: String?

    @Environment(\.dismiss) private var dismiss

    private var displayInfo: ConversationRenderPolicy.ToolSummary {
        ConversationRenderPolicy.toolSummary(
            toolName: toolName,
            toolArgs: toolArgs,
            contentFallback: contentFallback
        )
    }

    private var prettyArgs: String {
        guard let toolArgs,
              !toolArgs.isEmpty,
              let data = toolArgs.data(using: .utf8),
              let obj = try? JSONSerialization.jsonObject(with: data),
              let prettyData = try? JSONSerialization.data(withJSONObject: obj, options: [.prettyPrinted, .sortedKeys]),
              let prettyString = String(data: prettyData, encoding: .utf8)
        else {
            return toolArgs ?? ""
        }
        return prettyString
    }

    var body: some View {
        NavigationStack {
            ScrollView {
                VStack(alignment: .leading, spacing: 20) {
                    // Header
                    HStack(spacing: 10) {
                        Image(systemName: displayInfo.icon)
                            .font(.title2)
                            .foregroundStyle(.secondary)
                            .frame(width: 28)

                        VStack(alignment: .leading, spacing: 2) {
                            Text(toolName ?? "tool")
                                .font(.system(.body, design: .monospaced))
                                .fontWeight(.semibold)

                            Text(displayInfo.text)
                                .font(.caption)
                                .foregroundStyle(.secondary)
                                .lineLimit(2)
                        }
                    }
                    .frame(maxWidth: .infinity, alignment: .leading)

                    Divider()

                    // Arguments
                    if let toolArgs, !toolArgs.isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            Label("Arguments", systemImage: "curlybraces")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(.secondary)
                                .textCase(.uppercase)

                            ScrollView(.horizontal, showsIndicators: true) {
                                Text(prettyArgs)
                                    .font(.system(.caption, design: .monospaced))
                                    .textSelection(.enabled)
                                    .frame(maxWidth: .infinity, alignment: .leading)
                                    .padding(12)
                            }
                            .background(Color.systemGray6)
                            .clipShape(RoundedRectangle(cornerRadius: 8))
                        }
                    }

                    // Content fallback
                    if let content = contentFallback,
                       !content.trimmingCharacters(in: .whitespacesAndNewlines).isEmpty {
                        VStack(alignment: .leading, spacing: 8) {
                            Label("Content", systemImage: "text.alignleft")
                                .font(.caption.weight(.semibold))
                                .foregroundStyle(.secondary)
                                .textCase(.uppercase)

                            Text(content)
                                .font(.callout)
                                .textSelection(.enabled)
                                .frame(maxWidth: .infinity, alignment: .leading)
                        }
                    }
                }
                .padding()
            }
            .navigationTitle(toolName ?? "Tool Call")
            #if os(iOS)
            .navigationBarTitleDisplayMode(.inline)
            #else
            .toolbarTitleDisplayMode(.inline)
            #endif
            .toolbar {
                ToolbarItem(placement: .confirmationAction) {
                    Button("Done") { dismiss() }
                }
            }
        }
        #if os(iOS)
        .presentationDetents([.medium, .large])
        #endif
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
