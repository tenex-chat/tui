---
title: Agent Add Flow
slug: agent-add-flow
summary: The agent add sheet on iOS includes a manual pubkey/npub entry section that resolves npub1..
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-01
updated: 2026-05-05
verified: 2026-05-01
compiled-from: conversation
sources:
  - session:e400d601-236c-4034-8f48-6e685a5d30fd
  - session:81382a7a-abbb-4e10-afd2-e3e0785ade50
  - session:dbd0bf65-1948-4b9a-825e-7c9686c33f20
  - session:a60e18bd-8175-4179-8c7f-046e4d368354
  - session:8b83b8be-6184-42de-b7b9-db029ea0f790
---

# Agent Add Flow

## Agent Add Flow

The agent add sheet on iOS includes a manual pubkey/npub entry section that resolves npub1... via Bech32 or accepts a 64-char hex pubkey. Entering an npub or pubkey on the agent add screen publishes a kind:31933 event containing the added pubkey, thereby adding the agent to the user's kind:31933 event. The 'Add Agent' button on iOS is always enabled regardless of backend status, since manual key entry does not require a backend. The `canAddAgents` computed property is removed entirely and the disabled condition is simplified since enabling the button is now trivially true. The iOS add-agent sheet uses a segmented Picker for backend tabs, filtering the agent list by the selected backend, and removes per-row backend and pubkey labels, showing only the display name. The selectedAddBackendIndex is reset to 0 when the add-agent sheet closes.

<!-- citations: [^e400d-1] [^81382-1] [^dbd0b-1] [^8b83b-1] -->
## Key Resolution

resolveToHexPubkey accepts an npub1... string (decoded via Bech32.npubToHex) or a 64-character lowercase hex pubkey. [^e400d-2]

## TUI Agent Add

The TUI agent add flow supports Ctrl+A to activate pubkey input mode, where typing an npub or hex pubkey and pressing Enter validates and adds the agent, and Esc cancels. The 'a' key in project settings always opens add mode regardless of agent online status or backend inventory. The TUI add-agent picker empty state displays 'No agents in catalog. Use ^A to add by pubkey.' without online/backend qualifiers.

<!-- citations: [^e400d-3] [^a60e1-1] -->
## See Also

