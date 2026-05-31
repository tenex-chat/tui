---
title: nostr-ndb Fork and Patching
slug: nostr-ndb-fork-and-patching
summary: nostr-ndb is forked to pablof7z/nostr-ndb with its nostrdb dependency bumped from ^0.8 to ^0.10, resolving the type incompatibility between two separate nostrdb
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

# nostr-ndb Fork and Patching

## Fork and Patch Strategy

nostr-ndb is forked to pablof7z/nostr-ndb with its nostrdb dependency bumped from ^0.8 to ^0.10, resolving the type incompatibility between two separate nostrdb instances. Both nostrdb and nostr-ndb are patched via [patch.crates-io] in the workspace Cargo.toml so everything resolves to one instance of each type. [^8835c-5]

## See Also

