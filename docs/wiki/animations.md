---
title: Animations
slug: animations
summary: "Animation-driven rendering: spinner, streaming reveal, wave offset, and how they interact with the dirty-flag redraw gate"
tags:
  - animation
  - spinner
  - rendering
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:9c577a4f-cb7b-4a0e-a174-e013a84f0f33
---

# Animations

> Animation-driven rendering: spinner, streaming reveal, wave offset, and how they interact with the dirty-flag redraw gate

## Spinner Visibility

The spinner is purely time-driven: its visibility depends on whether the application is in a "working" state. The `wave_offset` value used in `render.rs:145` is global. After windowing made each frame cheap, the unconditional 20fps tick is acceptable (~13% CPU) and dirty-flag gating was deferred. If dirty-flag gating is implemented in the future, `wave_offset` must be properly scoped to only be active during actual work to avoid defeating the optimization.

<!-- citations: [^9c577-16] [^9c577-29] -->
## Streaming Animation Tick

`tick_stream_animation()` returns a bool indicating whether the streaming animation state changed during the tick. This signal is used to determine whether the 50ms tick interval must remain active for animation purposes. [^9c577-17]

## See Also
- [[render-loop|Render Loop]] — related guide

