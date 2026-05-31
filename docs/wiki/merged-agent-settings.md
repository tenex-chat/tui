---
title: Merged Agent Settings
slug: merged-agent-settings
summary: The TUI Agent Configuration modal displays, for each agent, skills and MCP servers that are the union of the 34011-announced set and the current project's 24010
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-03
updated: 2026-05-05
verified: 2026-05-03
compiled-from: conversation
sources:
  - session:568510a3-5ac1-46ac-ada5-aebf27ab0840
  - session:f19bcebf-0b1a-45f9-b6d6-312d9fad37d7
  - session:e9579051-925c-4cbc-b360-c6669eba56bb
  - session:188c28d3-b111-449c-8129-ef89b3647bf9
  - session:fb97ac0c-6ce5-4e58-873a-84c46204bed5
  - session:69354a99-8b35-4dd5-bd92-a0403286b4d7
---

# Merged Agent Settings

## Merged Agent Settings

Skills selectable in the app are derived from skill tags present in kind 24010 and kind 34011 events.

The TUI worker subscribes to agent config via kind:0 (Metadata) instead of kind:34011.

The TUI Agent Configuration modal displays, for each agent, skills and MCP servers that are the union of the 34011-announced set and the current project's 24010-announced set.

The MergedAgentSettingsInputs computation provides: models from 34011 only; skills as the union of 34011 and project 24010; MCPs as the union of 34011 and project 24010. Active selections prefer 34011 values and fall back to 24010 values. The `compute` method accepts a `project_mcp_servers: Vec<String>` parameter for the project's available MCP servers and merges project MCP servers from kind:24010 with the agent's own MCP servers from kind:0, using a union with deduplication.

MCP servers announced by the backend via kind:24010 ProjectStatus heartbeat tags are displayed as available in the agent config UI even when absent from the agent's kind:0 event.

In the AgentConfigSheet, the available skills catalog is sourced from the project-level 24010 event via getProjectConfigOptions, merged and deduplicated with the agent's kind:0 skills, and falls back to the agent's own list if no 24010 event exists.

When an agent is offline, the Model, Skills, and MCP Server columns in the agent-config modal must be empty. The `build_agent_settings_for` function must pass empty `available_models`, `available_skills`, and `available_mcp_servers` lists, or skip building settings entirely and show an 'Agent offline' placeholder.

<!-- citations: [^56851-5] [^f19bc-1] [^e9579-1] [^188c2-3] [^fb97a-2] [^69354-1] -->
## See Also

