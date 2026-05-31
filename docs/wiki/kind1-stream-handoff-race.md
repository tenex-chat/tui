---
title: "Kind:1 Stream Handoff and Race Conditions"
slug: kind1-stream-handoff-race
summary: "A kind:1 message race condition exists where the nostrdb processing path can fire handoff_local_stream_to_kind1 before the data channel path has created the str"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-06
updated: 2026-05-19
verified: 2026-05-06
compiled-from: conversation
sources:
  - session:e6d2dab7-938b-4ce4-974e-2fd406e33c36
  - session:da96bd80-d19b-4c95-9d9d-66fbfddd8ab9
---

# Kind:1 Stream Handoff and Race Conditions

## Kind1 Stream Handoff Race Condition

A kind:1 message race condition exists where the nostrdb processing path can fire handoff_local_stream_to_kind1 before the data channel path has created the stream buffer, leaving the buffer stuck in streaming state with is_complete=false and no superseded_message_id. This occurs because kind:1 and kind:24135 events arrive on separate relay subscriptions and are processed via two independent paths (nostrdb subscription vs data channel with 50ms tick interval). [^e6d2d-1]


## Handoff Delta Fix

When a stream delta arrives and the local stream buffer exists but has not yet received a handoff, the TUI checks the data store for an already-arrived kind:1 message from the same agent and immediately calls handoff_local_stream_to_kind1. The handoff search must skip tool-call messages (empty content, tool_name present) when looking for an agent's kind:1 text message. This fix is safe because process_note_keys stores the kind:1 message in data_store.messages_by_thread via handle_event before pushing CoreEvent::Message, ensuring the message is available in the store by the time a later delta's fix checks for it. However, there is a false positive risk that a previous session's kind:1 in the same thread could trigger a premature handoff on a new streaming session.

<!-- citations: [^e6d2d-2] [^da96b-1] -->
## Stream Buffer Lifecycle Constraints

The worker always sends is_finish: false in StreamTextDelta events, meaning is_complete can only be set to true by handoff_local_stream_to_kind1 or a true is_finish. The LocalStreamBuffer is never removed by tick_stream_animation unless superseded_message_id is Some and visible_text_chars >= text_char_count. [^e6d2d-3]

## Stale Stream Fallback

The stale check in the messages view is a fallback that hides the stream buffer when its text_content exactly matches a kind:1 message from the same agent in all_messages. [^e6d2d-4]

## Handoff Animation Speed

The handoff_local_stream_to_kind1 function transitions stream buffers at 24 chars/tick compared to 3 chars/tick for live streaming. [^e6d2d-5]

## Speculative Handoff Bug

The speculative handoff block in app.rs that fires on every delta must be deleted — it force-rebases a new stream buffer onto an agent's previous-turn kind:1 message, causing the vanish/flicker/reset loop. The stream buffer accumulates bytes from 24135 deltas without speculative rebasing; it is killed only when a kind:1 from that agent arrives for that conversation. [^da96b-2]

## Kind:1 Permanence and New Stream Detection

A completed kind:1 message from an agent in a conversation stays as a permanent message and is not suppressed or removed when a newer 24135 stream begins from the same agent. When a 24135 delta arrives with created_at later than the agent's latest kind:1 in that conversation, it is a new streaming message and is forwarded as a new stream buffer. [^da96b-3]

## Prefix Gate Revert

The prefix gate added to state.rs handoff_local_stream_to_kind1 is wrong and must be reverted — it was treating a symptom of the speculative handoff rather than the root cause. [^da96b-4]
## See Also

