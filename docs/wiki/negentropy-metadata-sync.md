---
title: Negentropy Metadata Sync
slug: negentropy-metadata-sync
summary: "The Go relay supports negentropy syncing for kind:0 events"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-05
updated: 2026-05-05
verified: 2026-05-05
compiled-from: conversation
sources:
  - session:e0f35264-2ac1-4497-bb12-ff13c885ba2b
---

# Negentropy Metadata Sync

## Negentropy Metadata Sync

The Go relay supports negentropy syncing for kind:0 events. Clients sync kind:0 metadata of all kind:0 authors present in their local system as part of normal negentropy syncing. Negentropy syncing runs on startup. [^e0f35-1]


## Implementation Details

The Rust core collects all locally-stored kind:0 event authors and reconciles them via negentropy against the relay in sync_all_filters(). The Go relay runs a kind:0 negentropy reconciliation loop that executes immediately on startup and repeats every 30 minutes. The Go relay's kind:0 negentropy sync uses a custom NIP-77 handler and drains the HaveNots channel concurrently in a goroutine to prevent blocking during large syncs. [^e0f35-2]
## See Also

