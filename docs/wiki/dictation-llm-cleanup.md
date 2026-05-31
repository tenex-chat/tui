---
title: Dictation LLM Cleanup
slug: dictation-llm-cleanup
summary: Audio/dictation messages are cleaned via an LLM before sending, removing filler words (ums, etc.) without substantially modifying the message
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-12
updated: 2026-05-12
verified: 2026-05-12
compiled-from: conversation
sources:
  - session:d170ed07-181f-444f-8944-f7cf0bd143e8
---

# Dictation LLM Cleanup

## Dictation LLM Cleanup

Audio/dictation messages are cleaned via an LLM before sending, removing filler words (ums, etc.) without substantially modifying the message. The LLM cleaning step uses the `cleanDictatedText` method on `OpenRouterPromptRewriteService` with a conservative system prompt that only strips filler words and does not rephrase. Only the dictated portion of the message text is sent through the LLM; any typed prefix is sliced off, the dictated portion is cleaned, and then reassembled before sending. The LLM cleaning step is skipped for short text under 8 characters. If the LLM call fails or no OpenRouter API key is configured, the message sends as-is with the original dictated text (graceful fallback). The model used for dictation cleanup is hardcoded to `openai/gpt-4o-mini`, decoupled from the audio settings model. The dictation cleanup uses temperature 0.1 for minimal variability. [^d170e-1]

## See Also

