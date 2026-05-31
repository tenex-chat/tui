---
title: CPU Profiling
slug: cpu-profiling
summary: Workflow for diagnosing high CPU usage in the TUI using sampling and call-graph analysis
tags:
  - performance
  - diagnostics
  - sampling
volatility: cold
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:9c577a4f-cb7b-4a0e-a174-e013a84f0f33
---

# CPU Profiling

> Workflow for diagnosing high CPU usage in the TUI using sampling and call-graph analysis

## Sampling Workflow

When a TUI process shows unexpectedly high CPU usage, use `sample $PID` (macOS) for a few seconds to capture the call graph. The sampling duration should be sufficient to collect thousands of samples — 3 seconds produced 2,233 samples in the diagnostic session. The dominant call chain reveals the hot path. [^9c577-12]


Before interpreting results, determine whether the process is idle or active by comparing loop counters in the diagnostic log across sampling windows. If `tick` climbs at ~20/s while `terminal`, `ndb`, `upload`, and `audio` are flat, the process is idle and the CPU usage is pure tick-driven redraw overhead. [^9c577-30]
## Interpreting Results

Look for the single hottest symbol: if a large proportion of samples (e.g., 91%, 2,029 of 2,233) cluster in one call, that is the bottleneck. Trace the full call chain from the event loop entry point (`runtime.rs:180`, `terminal.draw`) through the rendering pipeline to the hot leaf function. The chain reveals whether the cost is in application code (message assembly, markdown rendering) or downstream (ratatui layout, unicode segmentation). [^9c577-13]


Post-fix verification should use the same sampling methodology: compare the before and after profiles. A successful fix shows the main thread parked/idle the majority of the time (e.g., `tokio park` / `parking_lot Condvar::wait`) rather than in `terminal.draw`. The `ps` %CPU is a lifetime average that includes startup cost; instantaneous sampling is more reliable. [^9c577-31]
## Checking Prior Fixes

Review recent commits for the same class of bug. The prior caching fix for inbox_items and recent_threads (commit `9b95ba92`) served as a template — the chat message path exhibited the same pattern (uncached derived data recomputed every frame) and was simply missed in that earlier pass. [^9c577-14]

## Generating Fix Options

Produce a ranked list of fix options from cheapest/lowest-risk to most thorough. Each option should include its mechanism and estimated impact. The caching layer (option #1) is the highest-leverage and lowest-risk. Windowing (option #2) is complementary. Event-driven redraw (option #3) addresses the systemic issue but carries correctness risk if animations are not properly accounted for. [^9c577-15]

## See Also
- [[render-loop|Render Loop]] — related guide
- [[message-rendering-performance|Message Rendering Performance]] — related guide

