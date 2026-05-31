---
title: Agent Config Event
slug: agent-config-event
summary: "Kind:24020 agent config events are always global (no project a-tag); the 'Change all projects' checkbox and Ctrl+G toggle are removed."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-01
updated: 2026-05-11
verified: 2026-05-01
compiled-from: conversation
sources:
  - session:e400d601-236c-4034-8f48-6e685a5d30fd
  - session:8be83c3a-c07d-4837-b3cd-4a5fda1b78b7
  - session:7e8bae19-1378-449a-a622-b9deba26edf2
---

# Agent Config Event

## Global Scope

Kind:24020 agent config events are always global (no project a-tag); the 'Change all projects' checkbox and Ctrl+G toggle are removed. The 24020 event must not include any "tool" tags.

<!-- citations: [^e400d-4] [^8be83-1] -->

## Tool Parameter Removal (Rust)

The `tools` parameter and field must be removed from `build_agent_config_event`, `UpdateAgentConfig`, `UpdateGlobalAgentConfig`, and all associated handler functions and call sites in `worker.rs`, `panels.rs`, `agent.rs`, `agents_api.rs`, `protocol.rs`, `main.rs`, and `daemon.rs`. [^8be83-2]

## Tool Parameter Removal (Swift FFI)

The `tools: [String]` parameter must be removed from both `updateAgentConfig` and `updateGlobalAgentConfig` in the `tenex_core.swift` FFI bindings. [^8be83-3]

## Testing

Tests for the 24020 event must assert that tool tags are absent. [^8be83-4]

## Settings Toolbar Button

A gear/settings toolbar button appears to the right of the agent avatar button in the conversation toolbar; tapping it opens AgentConfigSheet for the current agent, and the button is disabled when no agent is selected. [^7e8ba-1]
## See Also

