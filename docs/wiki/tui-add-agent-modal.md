---
title: TUI Add-Agent Modal
slug: tui-add-agent-modal
summary: The add-agent modal in the TUI groups agents by backend using a tab bar at the top
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-05
updated: 2026-05-05
verified: 2026-05-05
compiled-from: conversation
sources:
  - session:8b83b8be-6184-42de-b7b9-db029ea0f790
---

# TUI Add-Agent Modal

## Add-Agent Modal Backend Tab Bar

The add-agent modal in the TUI groups agents by backend using a tab bar at the top. Left/right arrow keys switch between backends. ProjectDialogState includes an add_agent_backend_index field (usize, initialized to 0) tracking the selected backend tab. The backend list for the tab bar is derived from the full unfiltered inventory, sorted by display name, with no 'All' tab (starting at index 0). Multi-backend agents appear in each applicable backend tab. Switching backend tabs resets the agent list selection index (add_agent_index) to 0. The add_agent_backend_index is reset to 0 when exiting add-agent mode. [^8b83b-3]


## Add-Agent Picker List Display

Per-row backend labels are removed from the add-agent picker list since the backend is shown in the tab bar. [^8b83b-4]
## See Also

