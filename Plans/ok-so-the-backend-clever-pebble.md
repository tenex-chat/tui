# Obsolete Plan: 34011 Capability Migration

This plan captured an older intermediate design where kind:24010 still carried project-agent roster, PM, and some project-scoped configuration state. That is now obsolete.

## Current Contract

- kind:31933 is the authoritative project roster.
- Repeated kind:31933 `p` tags are ordered and must be preserved.
- The first kind:31933 `p` tag is the PM/default agent.
- kind:24011 backend inventory determines whether roster agents are available and which backend advertises them.
- kind:34011 supplies per-agent config: models, tools, skills, and MCP access.
- kind:24010 is runtime status only. It must not create roster agents, mark PM, select defaults, or gate roster lookup.

## Swift/TUI Implications

- Composer and agent selectors should read ordered roster rows from project `agentPubkeys`.
- Unavailable roster members remain visible/selectable and are labelled unavailable.
- PM changes are project p-tag ordering changes, not agent-config tag writes.
- Agent configuration UI should load current state from kind:34011.
- Generated Swift bindings already expose `ProjectAgent` fields broad enough for the merged roster row. If the core FFI later renames legacy `onlineAgents` callback fields, regenerate Swift bindings with `./scripts/generate-swift-bindings.sh`.

## Remaining Backend/Core Work

This Swift/docs pass does not edit Rust crates. If core work is still pending, it should ensure:

- `getAgentInventory()` is fed only from approved kind:24011 inventory.
- roster-facing `ProjectAgent` rows are built from kind:31933 order plus 24011/34011 overlays.
- 24010 parsing remains tolerant but does not populate roster, PM/default, or config state.
