---
title: Generate TTS Episode Tool
slug: generate-tts-episode
summary: The generate_tts_episode tool currently silently degrades to TTS-reading transcript text for snippet turns when the source episode audio is not downloaded local
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-12
updated: 2026-05-12
verified: 2026-05-12
compiled-from: conversation
sources:
  - session:4e35f5c2-aed5-4604-8c3f-1824ade445f8
---

# Generate TTS Episode Tool

## Silent Degradation Behavior

The generate_tts_episode tool currently silently degrades to TTS-reading transcript text for snippet turns when the source episode audio is not downloaded locally, instead of surfacing an error or warning. [^4e35f-1]


The tool should either splice in the real source audio for snippet turns (requiring the episode to be downloaded first) or return a clear error or warning so the agent can download the episode and retry, rather than silently degrading to TTS. (Previously: the tool silently degrades to TTS-reading transcript text for snippet turns.) [^4e35f-2]
## See Also

