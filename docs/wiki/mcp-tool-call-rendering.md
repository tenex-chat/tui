---
title: MCP Tool Call Rendering
slug: mcp-tool-call-rendering
summary: MCP tool calls render in the TUI as 'server · Humanized Tool Name [target]' instead of 'Executing'
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-14
updated: 2026-05-15
verified: 2026-05-14
compiled-from: conversation
sources:
  - session:a73278cc-be62-42b8-a54a-6e629734f9a9
  - session:2b390916-9f28-4ffe-ac4b-42231037218d
---

# MCP Tool Call Rendering

## MCP Tool Call Rendering

MCP tool calls render in the TUI as 'server · Humanized Tool Name [target]' instead of 'Executing'. MCP tool names are parsed from the mcp__ prefix format by splitting on double underscores to extract server and tool segments, then humanized by converting snake_case to title case. The url parameter is extracted as a display target for tool calls. The run_workflow tool renders as '▶ {name}' (using the workflow name field from tool-args) instead of the generic 'Executing' label in both TUI and iOS. The kill tool renders as '✕ {target}' (using the target field from tool-args) instead of 'Executing {id}' in both TUI and iOS. The kill tool renders in an error/accent-red color (ACCENT_ERROR #f47070 in TUI, .red in iOS) instead of the default muted/secondary color. The iOS ToolSummary struct supports an optional color field that defaults to nil, which ToolCallRow applies via foregroundStyle(displayInfo.color ?? .secondary). MCP tool call rendering is mirrored consistently in both the Rust TUI and the Swift iOS app.

<!-- citations: [^a7327-1] [^2b390-1] -->
## See Also

