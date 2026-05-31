---
title: Guessed Responses (Smart Reply Suggestions)
slug: guessed-responses
summary: Guessed responses (smart reply suggestions) are only generated when the last agent message p-tags the current user's pubkey
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-11
updated: 2026-05-11
verified: 2026-05-11
compiled-from: conversation
sources:
  - session:d4fe90da-ffc7-427f-8b05-a53d310a7a43
---

# Guessed Responses (Smart Reply Suggestions)

## Generation Conditions

Guessed responses (smart reply suggestions) are only generated when the last agent message p-tags the current user's pubkey. The p-tag comparison against the current user's pubkey is case-insensitive. [^d4fe9-1]

## See Also

