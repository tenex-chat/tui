---
title: "Project Status Event (kind:24010)"
slug: project-status-event
summary: "The kind:24010 project-status event announces only project-scoped skills and MCP servers; it announces no models."
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-03
updated: 2026-05-08
verified: 2026-05-03
compiled-from: conversation
sources:
  - session:568510a3-5ac1-46ac-ada5-aebf27ab0840
  - session:e9579051-925c-4cbc-b360-c6669eba56bb
  - session:fb97ac0c-6ce5-4e58-873a-84c46204bed5
  - session:9ba9c406-93ec-4ec5-a080-9531e27b5048
---

# Project Status Event (kind:24010)

## Project-Status Event

Kind 24010 events provide live project liveness and status only, not roster or config. ProjectStatusChanged deltas must be split so liveness changes are distinct from roster/config changes.

<!-- citations: [^56851-6] [^56851-7] [^56851-8] [^56851-9] [^e9579-3] [^fb97a-3] [^9ba9c-10] -->
## See Also

