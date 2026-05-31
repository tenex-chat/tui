---
title: "OperationsStatus Event (kind:24133)"
slug: operations-status-event
summary: "The Swift app tracks Nostr kind:24133 OperationsStatus events to determine which agents are actively working on a conversation"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-12
updated: 2026-05-12
verified: 2026-05-12
compiled-from: conversation
sources:
  - session:946e795a-ad90-454d-99d5-0241b2fdacb3
---

# OperationsStatus Event (kind:24133)

## Operations Status Event

The Swift app tracks Nostr kind:24133 OperationsStatus events to determine which agents are actively working on a conversation. No new 24133 subscription is needed because applyActiveConversationsChanged already processes 24133 callbacks and sets isActive on conversations. [^946e7-1]

## See Also

