---
title: Render Loop
slug: render-loop
summary: "The terminal UI render loop: tick interval, event-driven redraw, and dirty-flag gating"
tags:
  - performance
  - tui
  - event-loop
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:9c577a4f-cb7b-4a0e-a174-e013a84f0f33
---

# Render Loop

> The terminal UI render loop: tick interval, event-driven redraw, and dirty-flag gating

## Tick Interval

The render loop uses a 50ms tick interval (~20 fps), driven by `tokio::time::interval(Duration::from_millis(50))` in `runtime.rs`. Each tick calls `app.tick()`, which historically triggered a full redraw every cycle. [^9c577-1]


The 50ms tick fires unconditionally ~20×/s regardless of whether anything changed. After the message windowing fix, the per-frame cost is low enough that this is acceptable (~13% CPU). The tick continues to fire unconditionally; dirty-flag gating was considered but deferred. [^9c577-27]
## Event-Driven Redraw (Dirty Flag)

A follow-up optimization is to decouple redraw from the tick: only redraw when something actually changed (an input event, a new ndb event, or an active animation). Redraw would be gated on a dirty flag, with the 50ms tick kept only while an animation or spinner is active. This was not implemented — the windowing fix in `render_messages_panel` made each frame cheap enough that the unconditional 20fps tick is no longer a problem (~13% CPU post-fix). The dirty-flag gating remains available as a future optimization to drive idle CPU from ~13% to ~0%.

<!-- citations: [^9c577-2] [^9c577-26] -->
## Animation Awareness

When gating redraw on a dirty flag, the tick must continue firing at 50ms while any visible animation is active (spinner, streaming reveal, wave offset). If `tick()` has no visible work and nothing has changed, the redraw is skipped to avoid wasted frames. Skipping a redraw while something is animating causes the UI to appear frozen, so the tick must report whether any visible change occurred. [^9c577-3]

## Tick Return Signal

`tick_stream_animation()` returns a bool indicating whether visible state changed. The spinner and active notifications are purely time-driven; an "is animating" predicate gates whether the tick interval should remain active. [^9c577-4]

## Hot Path: terminal.draw

When sampled, the event loop spends most of its time in `terminal.draw` (runtime.rs:180), which calls `render()` (render.rs:26) → `render_chat()` (layout.rs:157) → `render_messages_panel()` (messages.rs:1235). [^9c577-5]

## See Also
- [[message-rendering-performance|Message Rendering Performance]] — related guide
- [[cpu-profiling|CPU Profiling]] — related guide
- [[animations|Animations]] — related guide

