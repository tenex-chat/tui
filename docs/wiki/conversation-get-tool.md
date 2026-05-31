---
title: Conversation Get Tool
slug: conversation-get-tool
summary: The `conversation_get` tool handler looks up the `conversation_id` key (snake_case) first and falls back to `conversationId` (camelCase) for parameter extractio
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-06
updated: 2026-05-06
verified: 2026-05-06
compiled-from: conversation
sources:
  - session:19913d35-95bc-40d3-b684-df378f85f5f4
---

# Conversation Get Tool

## Parameter Extraction

The `conversation_get` tool handler looks up the `conversation_id` key (snake_case) first and falls back to `conversationId` (camelCase) for parameter extraction. [^19913-1]


When no prompt is present, the `conversation_get` tool display shows the description from tool-args with a `:` separator. [^19913-2]

The `conversation_get` tool display always prefixes with `get` to indicate it is a read/fetch operation. [^19913-3]

A `conversation_get` event with a snake_case ID and a description renders as `📜 get <id>: <description>`. [^19913-4]
## See Also

