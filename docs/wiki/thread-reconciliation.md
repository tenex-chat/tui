---
title: Thread Reconciliation
slug: thread-reconciliation
summary: reconcile_threads_from_ndb uses a 48-hour since window (saved_at - 48h) on its NDB query to avoid scanning the entire database on startup.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-01
updated: 2026-05-01
verified: 2026-05-01
compiled-from: conversation
sources:
  - session:e400d601-236c-4034-8f48-6e685a5d30fd
---

# Thread Reconciliation

## Thread Reconciliation Window

reconcile_threads_from_ndb uses a 48-hour since window (saved_at - 48h) on its NDB query to avoid scanning the entire database on startup. [^e400d-6]

## See Also

