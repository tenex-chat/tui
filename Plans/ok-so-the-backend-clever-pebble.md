# Obsolete Plan: Historical 34011 Capability Migration

This plan captured an older intermediate design where kind:24010 still carried
project-agent roster, PM, and some project-scoped configuration state, and a
later-but-abandoned design where kind:34011 carried per-agent config. Both are
now obsolete.

## Current Contract

- kind:31933 is the authoritative project roster.
- Repeated kind:31933 `p` tags are ordered and must be preserved.
- The first kind:31933 `p` tag is the PM/default agent.
- kind:24011 backend inventory determines whether roster agents are available and which backend advertises them.
- kind:0 NIP-01 metadata, authored by the agent, supplies current per-agent config: model, tools, skills, and MCP access.
- kind:34011 is historical/unused and must not project into config state.
- kind:24010 is runtime status only. It must not create roster agents, mark PM, select defaults, or gate roster lookup.

## Swift/TUI Implications

- Composer and agent selectors should read ordered roster rows from project `agentPubkeys`.
- Unavailable roster members remain visible/selectable and are labelled unavailable.
- PM changes are project p-tag ordering changes, not agent-config tag writes.
- Agent configuration UI should load current state from agent-authored kind:0.
- Generated Swift bindings already expose `ProjectAgent` fields broad enough for the merged roster row. If the core FFI later renames legacy `onlineAgents` callback fields, regenerate Swift bindings with `./scripts/generate-swift-bindings.sh`.

## Remaining Backend/Core Work

This Swift/docs pass does not edit Rust crates. If core work is still pending, it should ensure:

- `getAgentInventory()` is fed only from approved kind:24011 inventory.
- roster-facing `ProjectAgent` rows are built from kind:31933 order plus 24011 availability and kind:0 config overlays.
- 24010 parsing remains tolerant but does not populate roster, PM/default, or config state.
