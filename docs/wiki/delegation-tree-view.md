---
title: Delegation Tree View
slug: delegation-tree-view
summary: The conversation list supports inline TUI-style caret expansion (▶/▼) for delegation trees, with indented child conversations and a +N descendant count badge on
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-03
updated: 2026-05-03
verified: 2026-05-03
compiled-from: conversation
sources:
  - session:88148493-78da-44b5-bf35-d4bfc1a10523
---

# Delegation Tree View

## Delegation Tree View

The conversation list supports inline TUI-style caret expansion (▶/▼) for delegation trees, with indented child conversations and a +N descendant count badge on collapsed parents. On iPhone (compact size class), long-pressing a conversation row with children opens DelegationTreeView as a .large sheet. [^88148-1]


## Caret Column Layout

The caret column in the conversation list only reserves left margin space when at least one visible row has children to expand, preventing wasted space on leaf-only lists. [^88148-2]
## See Also

