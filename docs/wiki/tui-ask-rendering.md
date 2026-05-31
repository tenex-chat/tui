---
title: TUI Ask Rendering
slug: tui-ask-rendering
summary: "How the TUI decides whether to render an inline ask modal: tool-use gate, parse_ask_event, and modal discovery logic"
tags:
  - tui
  - rendering
  - ask
  - messages
volatility: hot
confidence: medium
created: 2026-05-30
updated: 2026-05-12
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:06b93166-2310-4980-92b4-d9bd5df410fd
  - session:f14a552b-b71b-4005-b0e1-6caec8db60b3
  - session:4e35f5c2-aed5-4604-8c3f-1824ade445f8
---

# TUI Ask Rendering

> How the TUI decides whether to render an inline ask modal: tool-use gate, parse_ask_event, and modal discovery logic

## Overview

The TUI message renderer determines how each message is displayed based on its parsed fields. The rendering decision for ask events depends on two key checks: whether the message is a tool-use, and whether it carries ask data directly. [^06b93-7]

## Tool-Use Gate

The TUI classifies a message as a tool-use message based on the presence of a 'tool' tag, setting the tool name from that tag. At `messages.rs:322`, the renderer checks: `let is_tool_use = msg.tool_name.is_some() || !msg.q_tags.is_empty();`. If `is_tool_use` is true, the message renders as a compact tool-call line and the ask-event rendering path is never reached. This means any message with `["tool", "ask"]` (which sets `tool_name = Some("ask")`) will always be treated as a tool call, never as an interactive ask.

When a tool-use message has no 'tool-args' tag and cannot be parsed as an embedded JSON tool call, the TUI renders it as a single summary line consisting of the tool name plus the first 50 characters of the content. When a delegate tool call has no 'tool-args', the full message content should be rendered like a normal message since the content is the prompt being delegated.

The `render_tool_line` function renders `delegate`, `delegate_followup`, and `delegate_crossproject` tool calls as '→ @recipient' rather than 'Executing'.

<!-- citations: [^06b93-8] [^f14a5-2] [^4e35f-3] -->
## Ask Event Parse Check

The `parse_ask_event()` function at `message.rs:413` parses an ask event from tags by looking for `["question", ...]`, `["multiselect", ...]`, and `["title", ...]` tags. It does not parse JSON blobs in `["tool-args", ...]`. If the event uses `["tool-args", ...]` encoding, `msg.ask_event` will be `None` even if questions are present. [^06b93-9]

## Inline Ask Modal Rendering

At `messages.rs:474`, the renderer checks `app.ask_modal_state()` for a modal matching the message ID. If the modal is open and its `message_id` matches, the inline ask UI is rendered. If the modal was never opened (see Inbox and Ask Modal Discovery), the inline ask UI is never shown even when `ask_event` is populated. [^06b93-10]

## Ask Modal Discovery Gap

`get_unanswered_ask_for_thread()` scans for asks in two ways: (1) messages with q-tags pointing to a separate ask event, and (2) the thread root message having `ask_event` set. It does NOT scan for reply messages that carry ask data directly via inline `["question", ...]` tags without a q-tag and without being the thread root. This means a reply message that correctly encodes an ask inline (Pattern B) will not trigger `maybe_open_pending_ask()` to open the modal. [^06b93-11]

## See Also
- [[ask-event-encoding|Ask Event Encoding]] — related guide
- [[ask-delivery-patterns|Ask Delivery Patterns]] — related guide
- [[tui-inbox|TUI Inbox]] — related guide

