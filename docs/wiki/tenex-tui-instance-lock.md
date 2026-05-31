---
title: tenex-tui Instance Lock
slug: tenex-tui-instance-lock
summary: Tenex-TUI acquires an exclusive file lock on `~/.tenex/cli/tenex-tui.lock` before opening the database
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-25
updated: 2026-05-25
verified: 2026-05-25
compiled-from: conversation
sources:
  - session:8835ccc3-ba38-4ce4-b9ed-353e747333f5
---

# tenex-tui Instance Lock

## Instance Lock

Tenex-TUI acquires an exclusive file lock on `~/.tenex/cli/tenex-tui.lock` before opening the database. If another instance is already running, the application exits immediately with a clear message. [^8835c-7]

## See Also

