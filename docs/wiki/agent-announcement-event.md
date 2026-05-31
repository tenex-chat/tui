---
title: "Agent Announcement Event (kind:34011)"
slug: agent-announcement-event
summary: "The backend publishes a kind:34011 Nostr event for each agent, signed by the agent's own keypair via signer_for(agent), announcing the agent's globally-availabl"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-03
updated: 2026-05-04
verified: 2026-05-03
compiled-from: conversation
sources:
  - session:568510a3-5ac1-46ac-ada5-aebf27ab0840
  - session:a1f9504c-5e42-453b-b4ab-ef4224f3e4c5
  - session:188c28d3-b111-449c-8129-ef89b3647bf9
  - session:fb97ac0c-6ce5-4e58-873a-84c46204bed5
---

# Agent Announcement Event (kind:34011)

## Agent Announcement Event (kind:34011)

Agent capability metadata (formerly kind:34011) is merged into the agent's kind:0 (metadata) event. The merged kind:0 event uses a `slug` tag instead of a `d` tag for the agent slug. It includes `skill` and `model` tags with a three-element format: `["skill", "name", "active"]` and `["model", "name", "active"]`, and a `use-criteria` tag containing the "when to use" description string. An agent's individual kind:0 event can list its own skills and MCP servers available to that specific agent. The `AgentConfig` data model includes a `use_criteria` field parsed from the kind:0 event tags. All references to kind:34011 are removed from both backend and TUI codebases, including kind constants, subscriptions, and routing.

<!-- citations: [^56851-1] [^188c2-1] [^fb97a-1] -->
## See Also

