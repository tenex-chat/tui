---
title: NostrDB Projection Layer
slug: nostrdb-projection-layer
summary: nostrdb is the durable source of truth, with a Rust core projection/read model layer mediating between nostrdb and UI render caches via FFI snapshots and deltas
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-08
updated: 2026-05-21
verified: 2026-05-08
compiled-from: conversation
sources:
  - session:9ba9c406-93ec-4ec5-a080-9531e27b5048
  - session:22ca3a53-7ac3-4b05-8469-cf0a16e53ede
---

# NostrDB Projection Layer

## Architecture

nostrdb is the durable source of truth, with a Rust core projection/read model layer mediating between nostrdb and UI render caches via FFI snapshots and deltas. The nostrdb Rust bindings retry LMDB open by halving the mapsize (starting from 8 GiB down to a 512 MiB minimum), printing an error on each failed attempt. rebuild_from_ndb() must be removed from interactive refresh paths and reserved for startup and recovery only, with an explicit trigger added for trust changes. kind:0 cold-start truth must be complete: rebuild_from_ndb() loads agent configs into the projection without relying on fallback getters. The projection must define which fields it merges from which created_at timestamp for replaceable kind:0 events, rather than applying a naive latest-wins strategy, because profile changes and config changes can land out of order.

<!-- citations: [^9ba9c-5] [^9ba9c-6] [^9ba9c-7] [^9ba9c-8] [^22ca3-2] -->
## See Also

