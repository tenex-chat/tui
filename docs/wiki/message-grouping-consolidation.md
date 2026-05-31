---
title: Message Grouping Consolidation
slug: message-grouping-consolidation
summary: "Message grouping across all clients is unified to a single rule: per-pubkey header consolidation, where consecutive same-author messages share one header"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-09
updated: 2026-05-19
verified: 2026-05-09
compiled-from: conversation
sources:
  - session:24bb0337-8ff0-4fa2-b191-971471cadba8
  - session:da96bd80-d19b-4c95-9d9d-66fbfddd8ab9
---

# Message Grouping Consolidation

## Unified Message Grouping

Message grouping across all clients is unified to a single rule: per-pubkey header consolidation, where consecutive same-author messages share one header. The Web client is disregarded for message grouping decisions because it is not kept up to date. The foldedGroup feature that collapsed runs of untagged messages into a placeholder is removed entirely. A non-tool message must not be considered consecutive to a tool-call message from the same agent; it always gets a proper author line instead of a dot_line.

<!-- citations: [^24bb0-1] [^da96b-5] -->
## See Also

