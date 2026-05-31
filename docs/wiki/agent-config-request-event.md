---
title: Agent Config Request Event
slug: agent-config-request-event
summary: The event kind constant must be named AGENT_CONFIG_REQUEST (not AGENT_CONFIG)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-08
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:307f5061-9802-4a47-bd42-797aa71dd277
  - session:9ba9c406-93ec-4ec5-a080-9531e27b5048
---

# Agent Config Request Event

## Event Kind and Subscription Filtering

The event kind constant must be named AGENT_CONFIG_REQUEST (not AGENT_CONFIG). Subscriptions for kind:34011 must be tightened to only subscribe based on kind:31933/kind:24011 data. [^307f5-1]



Contract: 24020 is a request to change config, confirmed only by observing a fresh kind:0. [^9ba9c-1]
## Known Gaps

Known gaps left unfixed: `get_project_config_options` still aggregates from kind:24010 instead of kind:34011; iOS views still render case 24020 as 'Agent Config' instead of 'Agent Config Request'; kind:24011 subscription gating by trust lacks worker→FFI plumbing. [^307f5-2]

## Projection Layer

The projection layer must surface a pending_config_change shape for kind:24020 requests to support optimistic UI rendering during the round-trip to kind:0 confirmation. [^9ba9c-2]
## See Also

