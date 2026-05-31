---
title: ID Dictionary Uniquing
slug: id-dictionary-uniquing
summary: Dictionary initializations built from backend IDs (conversation thread IDs, project IDs, comment row IDs) use `uniquingKeysWith` to select the first occurrence
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-22
updated: 2026-05-22
verified: 2026-05-22
compiled-from: conversation
sources:
  - session:d9036c4e-9003-43da-8181-c7b77c788cb8
---

# ID Dictionary Uniquing

## Dictionary Uniquing

Dictionary initializations built from backend IDs (conversation thread IDs, project IDs, comment row IDs) use `uniquingKeysWith` to select the first occurrence on duplicate keys, preventing fatal crashes. [^d9036-1]

## See Also

