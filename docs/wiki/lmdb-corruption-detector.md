---
title: LMDB Pre-open Corruption Detector
slug: lmdb-corruption-detector
summary: tenex-core includes a pre-open corruption detector that checks LMDB free-list entries for impossible values and auto-wipes data.mdb and lock.mdb if corruption i
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

# LMDB Pre-open Corruption Detector

## LMDB Corruption Detector

tenex-core includes a pre-open corruption detector that checks LMDB free-list entries for impossible values and auto-wipes data.mdb and lock.mdb if corruption is detected. [^8835c-4]

## See Also

