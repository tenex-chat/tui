---
title: Project Agent Roster
slug: project-agent-roster
summary: "Kind:31933 events define project agent membership (not kind:24010), and agent availability comes from approved kind:24011 backend inventories"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-08
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:307f5061-9802-4a47-bd42-797aa71dd277
  - session:5aad59a2-de95-4c7e-8090-5c61325c5932
  - session:9ba9c406-93ec-4ec5-a080-9531e27b5048
---

# Project Agent Roster

## Project Agent Roster

Kind:31933 events define project agent membership (not kind:24010), and agent availability comes from approved kind:24011 backend inventories. Project roster logic must be centralized in dedicated `roster` modules (in tenex-core/store, tenex-repl, tenex-tui/ui, tenex-cli) rather than being scattered inline. All non-core surfaces (Swift/TUI/CLI/REPL) must replace local roster recomputation and merge shims with a single call to the core `get_project_roster` API, and the `get_online_agents` FFI alias must be removed with its callers migrated to `get_project_roster`. Non-core surfaces must keep only UI-local state (selected pubkeys, sheet drafts, scroll/navigation state, transient form edits) and must not decide how to merge 31933 + kind:0 + 24011 + 24010. Before deleting Swift's `preferredBackendPubkey` heuristic, the core `build_project_roster` output must be diffed against it for projects with multi-backend agents to detect silent semantic drift in `backend_pubkey` tiebreaking. Contract: 31933 provides durable project roster, order, and PM information; 24011 provides live backend inventory, availability, and catalog. The add-agent picker lists agents from kind:24011 events, not from the project's kind:31933 p-tags. A backend's kind:24011 inventory only appears in the picker if that backend's pubkey is in this machine's approved-backends list. A kind:24011 event lists all agents of a single backend as a full snapshot, and a kind:24011 event from a given backend overwrites the entire inventory for that backend (no merge, no created_at comparison); an older event arriving after a newer one from the same backend will also overwrite the newer inventory (last-arrival-wins). Across backends, agent inventory is additive and unioned by agent pubkey; duplicate pubkeys across backends are flagged as a problem. Kind 2xxxx ephemeral events are not persisted in the nostrdb cache; the live agent inventory state resides in-memory only.

<!-- citations: [^307f5-6] [^5aad5-1] [^9ba9c-9] -->
## See Also

