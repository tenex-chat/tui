---
title: Backends Settings Tab
slug: backends-settings-tab
summary: The Settings modal includes a 'Backends' tab for viewing and managing approved, pending, and blocked backends.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-04
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:7c985e1d-978a-4cc1-a489-174d823a8617
---

# Backends Settings Tab

## Backends Tab Overview

The Settings modal includes a 'Backends' tab for viewing and managing approved, pending, and blocked backends. [^7c985-1]


The tab displays three sections: Pending, Approved, and Blocked backends. Each backend is displayed as a truncated pubkey (first8…last6) with a colored badge: yellow for pending, green for approved, red for blocked. [^7c985-2]

The hints bar at the bottom of the Settings modal updates context-sensitively when on the Backends tab. [^7c985-3]

## Backends Tab Interactions

Pressing 'a' or Enter approves the selected backend (works on pending and blocked backends). Pressing 'b' blocks the selected pending or approved backend. [^7c985-4]

Pressing 'd' deletes the selected backend, removing it from tracking entirely without blocking. A 'remove' method exists on the trust store to delete a backend without blocking it. [^7c985-5]
## See Also

