---
title: Project Deletion Event
slug: project-deletion-event
summary: "Project deletion publishes a tombstoned kind:31933 event (with a `deleted` tag) before publishing a kind:5 deletion event"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-04
updated: 2026-05-04
verified: 2026-05-04
compiled-from: conversation
sources:
  - session:1a6b41df-729e-4f77-a351-4b04f691c693
---

# Project Deletion Event

## Project Deletion Event

Project deletion publishes a tombstoned kind:31933 event (with a `deleted` tag) before publishing a kind:5 deletion event. The tombstoned kind:31933 event includes a `deleted` tag and a fresh `created_at` timestamp. The kind:5 deletion event is published at least one second after the tombstoned kind:31933 event to avoid recreating it. The kind:5 deletion event references the kind:31933 replaceable event using an `a`-tag coordinate (`31933:pubkey:d-tag`). [^1a6b4-1]

## See Also

