---
title: Ask Event Encoding
slug: ask-event-encoding
summary: "How ask events are encoded on the Nostr wire: the intent tag, title, question/multiselect tags, and the dedicated encode_ask path"
tags:
  - protocol
  - nostr
  - ask
  - events
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:06b93166-2310-4980-92b4-d9bd5df410fd
---

# Ask Event Encoding

> How ask events are encoded on the Nostr wire: the intent tag, title, question/multiselect tags, and the dedicated encode_ask path

## Overview

Ask events are a structured kind-1 event that carry a set of questions to a recipient. They must be encoded using the dedicated `encode_ask` function path — not as generic tool-use events — for the TUI to render them as interactive inline ask modals. [^06b93-1]

## Required Tags for Inline Ask Rendering

For the TUI to recognize and render an event as an ask, the event must carry these tags: `["intent", "ask"]` (not `["tool", "ask"]`), a top-level `["title", "..."]` tag, and one or more `["question", "<title>", "<prompt>", ...options]` or `["multiselect", "<title>", "<prompt>", ...options]` tags. The event must also include a `["p", "<recipient-pubkey>"]` tag for the intended recipient. [^06b93-2]

## What encode_ask Produces

The `encode_ask` function in `tenex-protocol/src/nostr/encoder.rs:171` produces: a `["intent", "ask"]` tag, a top-level `["title", "..."]` tag with the ask title, one `["question", ...]` tag per single-select question, one `["multiselect", ...]` tag per multi-select question, and a p-tag identifying the recipient. [^06b93-3]

## Tool-Use Encoding Is Incorrect for Asks

Encoding an ask via the generic tool-use path (producing `["tool", "ask"]` and `["tool-args", "<JSON>"]`) is incorrect. The TUI renderer treats `["tool", "ask"]` as a generic tool call, which skips the ask UI entirely. The JSON blob in `["tool-args", ...]` is not read by `parse_ask_event()`, which only looks for `["question", ...]`, `["multiselect", ...]`, and `["title", ...]` tags. [^06b93-4]

## Branch Tag

Ask events carry a `["branch", "..."]` tag (e.g., `["branch", "main"]`) indicating the git branch context. [^06b93-5]

## LLM Model and RAL Tags

Ask events carry `["llm-model", "..."]` and `["llm-ral", "..."]` tags identifying the LLM model and retrieval-augmented-learning flag for the backend. [^06b93-6]

## See Also
- [[tui-ask-rendering|TUI Ask Rendering]] — related guide
- [[ask-delivery-patterns|Ask Delivery Patterns]] — related guide

