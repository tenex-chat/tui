---
title: Ask Delivery Patterns
slug: ask-delivery-patterns
summary: "Two valid patterns for delivering asks inline: q-tag references (Pattern A) and direct reply with inline ask data (Pattern B)"
tags:
  - protocol
  - ask
  - tui
  - patterns
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:06b93166-2310-4980-92b4-d9bd5df410fd
---

# Ask Delivery Patterns

> Two valid patterns for delivering asks inline: q-tag references (Pattern A) and direct reply with inline ask data (Pattern B)

## Overview

There are two valid patterns for delivering ask events inline in a conversation thread. Both should be supported by the TUI. [^06b93-12]

## Pattern A — Q-Tag Reference (Delegation)

The thread gets a tool-call message with a `["q", "<ask-event-id>"]` tag pointing to a separate ask event. The ask event is looked up by ID via `get_ask_event_by_id`. The ask event can have e-tags or not — it's fetched by ID from nostrdb and its tag structure is what matters, not its event-kind lineage. [^06b93-13]

## Pattern B — Direct Reply (Inline)

The ask event IS the reply message itself, carrying `["question", ...]`, `["multiselect", ...]`, `["title", ...]`, and `["intent", "ask"]` tags directly. The TUI's `messages.rs:471` already checks `msg.ask_event.is_some()` for this path. The event remains a reply with e-tags — it does not need to be a new-conversation root. [^06b93-14]

## Both Patterns Must Work

The TUI should support both Pattern A and Pattern B for inline ask discovery. Pattern B currently has a gap in `get_unanswered_ask_for_thread()` which only discovers Pattern A asks (via q-tag scan) and thread-root asks, but not inline ask data on reply messages. [^06b93-15]

## Fix for Pattern B Discovery

`get_unanswered_ask_for_thread()` needs a third scan — iterating all messages and checking for ones where `msg.ask_event.is_some()`, the pubkey is not the user's, and the message hasn't been replied to. This scan should go after the q-tag scan and before the thread-root check. The returned tuple is `(message_id, ask_event, asker_pubkey)` to feed `maybe_open_pending_ask()`. [^06b93-16]

## See Also
- [[tui-ask-rendering|TUI Ask Rendering]] — related guide
- [[tui-inbox|TUI Inbox]] — related guide
- [[ask-event-encoding|Ask Event Encoding]] — related guide

