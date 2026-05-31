---
title: GitHub REST API Push Workaround
slug: github-rest-api-push-workaround
summary: Pushes to pablof7z/nostrdb-rs use the GitHub REST API (create blob, create tree, create commit, PATCH ref) because direct git push fails with HTTP 500.
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

# GitHub REST API Push Workaround

## GitHub REST API Push Workaround

Pushes to pablof7z/nostrdb-rs use the GitHub REST API (create blob, create tree, create commit, PATCH ref) because direct git push fails with HTTP 500. [^8835c-1]

## See Also

