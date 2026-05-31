---
title: Agent Display and Status
slug: agent-display-and-status
summary: "Agent names in the delegation card and agent list use the kind:0 profile name for display, not installed-agent catalog slugs"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-01
updated: 2026-05-15
verified: 2026-05-01
compiled-from: conversation
sources:
  - session:e400d601-236c-4034-8f48-6e685a5d30fd
  - session:034f18db-33b3-4a32-9b9b-467e096c7ea6
  - session:9c66a2fc-d87d-4750-a468-42d035d75422
  - session:8b83b8be-6184-42de-b7b9-db029ea0f790
  - session:10262079-d787-43af-8010-c1c34cc9cb92
  - session:f14a552b-b71b-4005-b0e1-6caec8db60b3
  - session:9ba9c406-93ec-4ec5-a080-9531e27b5048
  - session:b1f689ea-341e-4a7d-b16e-b36542ed6a51
---

# Agent Display and Status

## Agent Display and Status

Contract: kind:0 provides the agent profile and the active per-agent config, replacing the historical 34011. Agent names in the delegation card use the kind:0 profile name for display, not installed-agent catalog slugs. Kind:0 is the only allowed name source for agent display names — slugs, definition titles, and config events are explicitly excluded. Agent display name resolution prefers the display_name field over the name field in kind:0 content, falling back to a truncated pubkey if neither exists. When a project has agents running on multiple backends, the agent selector displays agentName@backendName; when all agents share one backend, it displays only agentName. The agent_selector_parts method returns (name, Option<"@backend">) to allow separate styling of the backend suffix. In the agent config modal list, the @backendName suffix renders in TEXT_MUTED (gray), or black when the row is selected with the warning-orange highlight, while the agent name uses name_style (white/dim based on online status, highlighted when selected). In the input context chip, the @backendName suffix renders in TEXT_MUTED (gray), or dark near-black on the blue focus highlight, while the agent name and model remain in ACCENT_PRIMARY (blue). Projects with a single backend display no backend suffix, leaving the UI unchanged from the original behavior. The agent list in project settings displays the agent name by falling back through installed agent slug, then kind:0 profile name, then a truncated pubkey. The assigned agents list in project settings shows the backend display name instead of the truncated pubkey. Multi-backend agents in the assigned list display '⚠ N backends' in an error accent color. When the orchestrator posts a delegation thread root where t.pubkey equals parent_pubkey, the delegation card resolves the target agent name from t.p_tags[0] instead of t.pubkey, so the card shows the actual target agent rather than the delegator. Agents without an explicit kind:24010 record are visually grayed out, but no online/offline label or conceptual distinction is displayed. An agent's model, skills, and MCP servers are displayed when available, and shown as empty when unavailable. The empty-agents state in project settings displays 'No agents assigned. Press a to add.' without differentiating online vs offline. The agent configuration modal displays the backend's name next to 'Set as PM' in the bottom footer area, formatted as '· Backend: <name>' appended next to the toggle. The backend name is derived from the kind:24010 event, using the kind:0 profile name or falling back to the slug from kind:24010. The backend name label is truncated to fit the available width in the footer toggles row. Fallback display names (truncated pubkeys) are not permanently cached in the iOS profileNameCache — only real resolved names are stored. The iOS invalidateProfileNameCache() is called when installedAgentsChanged fires, so agent names are re-fetched after kind:0 subscriptions populate nostrdb. The requested_profiles set is cleared on reconnect so profiles are re-fetched each session. Fresh kind:0 relay events must trigger a DataChange::NoteKeys notification so that handle_agent_config_event populates agent_configs_by_pubkey immediately. The AgentConfig modal must invalidate its active_agent_pubkey lock when a kind:0 event for the displayed agent is processed, allowing the next render to re-read the now-populated store. The delegate, delegate_followup, and delegate_crossproject tool names return an empty verb from tool_verb() to suppress the generic 'Executing' label.

<!-- citations: [^e400d-5] [^034f1-1] [^9c66a-1] [^8b83b-2] [^10262-1] [^f14a5-1] [^9ba9c-3] [^b1f68-1] -->
## See Also

