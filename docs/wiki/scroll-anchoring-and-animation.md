---
title: Scroll Anchoring and Animation
slug: scroll-anchoring-and-animation
summary: The `isTranscriptAtBottom` state is set to true outside the `withAnimation` block in `scrollToBottom`, so the state mutation does not run inside a CAAnimation c
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-10
updated: 2026-05-10
verified: 2026-05-10
compiled-from: conversation
sources:
  - session:1aa5547a-aedd-4c11-b43f-1aa41c8652fb
  - session:d18381a0-39d5-4141-be58-03362b5bd636
  - session:acaa32eb-b3b5-4a83-9aee-822648c76ca7
---

# Scroll Anchoring and Animation

## Scroll Anchoring and Animation

The `isTranscriptAtBottom` state mutation is set to true before the `withAnimation` block rather than inside it, so it does not run inside a CAAnimation context.

<!-- citations: [^1aa55-2] [^d1838-1] [^acaa3-1] -->
## See Also

