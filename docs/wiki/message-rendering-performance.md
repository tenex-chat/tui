---
title: Message Rendering Performance
slug: message-rendering-performance
summary: Why rendering all messages every frame pegs the CPU, and the caching + windowing fixes
tags:
  - performance
  - rendering
  - messages
  - cpu
volatility: hot
confidence: medium
created: 2026-05-30
updated: 2026-05-03
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:9c577a4f-cb7b-4a0e-a174-e013a84f0f33
  - session:83822ddb-cc93-4ac6-945d-9cd6f6fd6b88
---

# Message Rendering Performance

> Why rendering all messages every frame pegs the CPU, and the caching + windowing fixes

## Per-Frame Rebuild of Entire History

`render_messages_panel` (messages.rs:179) rebuilds the entire conversation from scratch on every frame. It loops over all messages (messages.rs:266, :285), calls `render_markdown`/`markdown_lines` on each with no caching, and assembles a full `Vec<Line>` of the whole history. The rendered output is then handed to `Paragraph::new(messages_text).scroll((scroll, 0))` (messages.rs:1233). Ratatui's `Paragraph::scroll()` does not skip off-screen lines — it walks and grapheme-segments every line of the entire conversation to compute layout, even though only approximately `visible_height` lines are shown.

Chat messages and the latest reply card always render in full with no gradient fade, removing all 'Read more' / 'Show less' collapse logic. [^83822-4]

<!-- citations: [^9c577-6] [^9c577-18] -->
## Grapheme Segmentation Hot Spot

The dominant CPU consumer is `unicode_segmentation::GraphemeCursor::next_boundary` (including `bsearch_range_table` for InCB_Extend and related Unicode properties). This is invoked inside ratatui's `Paragraph::render` → `LineTruncator` / reflow path, which runs over the entire message history text on every frame.

The key insight from sampling: the 649 hot samples in `render_messages_panel` are entirely inside `f.render_widget(...)` → ratatui `Paragraph` → reflow → grapheme segmentation. The line-building code (`render_markdown`, the per-message loops) does not appear in the hot path at all. This means `Paragraph` re-segments whatever lines it receives every frame regardless of their origin — caching built lines saves the cheap part and leaves the expensive part intact. [^9c577-23]

<!-- citations: [^9c577-7] [^9c577-19] -->
## Relationship to Prior Caching Fix

This is the same class of bug as commit `9b95ba92` ("cache inbox_items and recent_threads to fix CPU peg") — derived data recomputed every frame — but the chat message rendering path was never cached. [^9c577-8]

## Caching Strategy: Rendered Line Cache

Caching the built `Vec<Line>` was initially proposed as the primary fix, but this was determined to be insufficient on its own. The 649 hot samples in `render_messages_panel` are entirely inside ratatui `Paragraph` → reflow → grapheme segmentation, not in the line-building code (`render_markdown`, the per-message loops). `Paragraph` re-segments whatever lines it is handed every frame, regardless of whether they were cached or freshly built. Caching saves the cheap part (line assembly) and leaves the expensive part (grapheme segmentation) untouched.

<!-- citations: [^9c577-9] [^9c577-20] -->
## Windowing Strategy

The implemented fix is windowing: instead of handing the entire conversation to `Paragraph`, the visible range is sliced out before constructing the `Paragraph`. Since `scroll`, `visible_height`, and `total_lines` are already computed, the code computes `end = (scroll + visible_height).min(total_lines)`, then calls `messages_text.truncate(end)` to drop rows below the viewport and `let visible = messages_text.split_off(scroll)` to drop rows above it. The resulting `visible` slice is passed to `Paragraph::new(visible)` with no `.scroll()` needed. Both `truncate` and `split_off` move `Line`s without cloning. Segmentation drops from thousands of lines per frame to approximately `visible_height` (~50). This fixes both the idle peg and the active-streaming peg, since every frame is cheap regardless of redraw rate.

<!-- citations: [^9c577-10] [^9c577-21] -->
## Implementation Priority

The user initially selected option #3 (decouple redraw from tick), but after the advisor corrected the cost model — revealing that `Paragraph` re-segments lines regardless of caching and that option 3 only fixes the idle case — the user chose option #2 (windowing). Windowing was implemented as the sole fix because it makes every frame cheap (~50 lines segmented vs thousands), which fixes both idle and active-streaming CPU usage in one change. Option 3 (dirty-flag gating) is a potential follow-up to drive idle CPU from ~13% to ~0%, but is not needed for correctness after windowing.

<!-- citations: [^9c577-11] [^9c577-22] -->

## Diagnostic Log: Idle vs Active Determination

To determine whether a process is idle or actively streaming when sampled, inspect the loop counters in the diagnostic log. In the sampled process, between the last 1000-loop windows, `tick` climbed by ~1000 (the 50ms timer firing ~20×/s), while `terminal` was flat at 5333 (no keypresses), `ndb` flat (~1 event), and `upload`/`audio` zero. This confirms the process was idle — pure tick-driven redraws at ~20fps with nothing actually changing. [^9c577-24]

## Post-Fix Verification

After the windowing fix (PID 88128), the same sampling methodology that previously showed 91% of time inside `draw` now shows the main thread parked/idle 83% of the time (2054/2481 samples in `tokio park` / `parking_lot Condvar::wait`). Only ~13% (335/2481) remains in `Terminal::draw` — the 20fps tick redraws still happen, but each one is cheap. The `ps` lifetime average dropped from 79.1% to 14.8% (inflated by startup cost; instantaneous is much lower). The remaining ~13% is the unconditional 20fps tick redraw, which option 3 (dirty-flag gating) could drive to ~0% if desired. [^9c577-25]

## Fix Option Analysis

Three fixes were analyzed, ranked cheapest to most thorough: (1) Cache the rendered `Vec<Line>` per conversation, keyed on message count + last event id/timestamp + viewport width — rejected because `Paragraph` re-segments lines regardless of caching, so it saves only the cheap line-building and leaves the expensive grapheme segmentation. (2) Window the work to the visible range by slicing `messages_text[scroll .. scroll+visible_height]` and passing a pre-sliced text to `Paragraph` with `scroll((0,0))` — implemented. (3) Decouple redraw from the 50ms tick via a dirty flag, only redrawing on input, new ndb events, or active animations — fixes only the idle case (~80% → 0%) but leaves active-streaming pegged; deferred as a follow-up. Options 2 and 3 compose: windowing makes each frame cheap; dirty-flag gating additionally skips frames when idle. [^9c577-28]
## See Also
- [[render-loop|Render Loop]] — related guide
- [[cpu-profiling|CPU Profiling]] — related guide

