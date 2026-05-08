# TENEX Agent Architecture

> **Critical:** agents, agent definitions, project rosters, backend inventory, and agent config are separate concepts. Do not infer one from another.

## Core Concepts

### Agent = Nostr User

An agent is a Nostr user with a pubkey. Agents are not special event types — they are users that happen to be AI-powered.

The agent's own kind:0 metadata event carries both:

- Standard NIP-01 profile fields (`name`, `about`, `picture`) in `content`
- TENEX-specific configuration in tags (see "Agent Config" below)

### AgentDefinition (kind:4199) = Template

An AgentDefinition describes how to instantiate or configure an agent. It can be authored by anyone and is not the agent identity.

Use kind:4199 for browsing templates, creating agents, and installing definitions to backends. Do not use it as the project roster or as proof that an agent is available.

### Project (kind:31933) = Ordered Roster

The user-signed kind:31933 project event is the authoritative project roster.

- Repeated `p` tags are the ordered agent pubkeys for the project.
- The first `p` tag is the project manager and default agent.
- Reordering `p` tags is how PM/default changes are represented.
- Missing backend status must not remove roster members from selectors.

### Backend Inventory (kind:24011) = Availability

Backend-authored kind:24011 inventory tells clients which agent pubkeys are available from approved backends.

Use 24011 to annotate roster agents as available/unavailable and to choose backend provenance for install/config UI. Do not use kind:24010 to create roster rows, mark PM, choose defaults, or decide whether a roster lookup is allowed.

### Agent Config (kind:0, agent-authored) = Per-Agent Config

Each agent publishes its own kind:0 NIP-01 metadata event, signed with the agent's own key. In addition to the standard profile fields, the tags carry per-agent configuration:

- `["slug", "<agent-slug>"]` — human-friendly slug
- `["use-criteria", "<text>"]` — when to pick this agent
- `["p", "<backend_pubkey>"]` — backend that runs this agent (traceability only, not identity)
- `["model", "<slug>"]` — currently-selected model
- `["skill", "<id>", "active"]` — enabled skill (omit `"active"` for visible-but-inactive)
- `["mcp", "<slug>", "active"]` — MCP server in `mcpAccess` (omit for configured-but-inactive)
- `["tool", "<id>"]` — visible tool

The catalogue of *available* models lives on kind:24011 (the backend inventory), **not** on kind:0 — kind:0 carries only the active selection plus per-agent visible skills/tools/MCPs.

> Note: TENEX intentionally uses kind:0 for per-agent config (an unusual extension of NIP-01). There is no separate replaceable kind for it. The historical kind:34011 is unused.

### Agent Config Command (kind:24020) = Change Request

Clients publish kind:24020 to request a config change. The command is ephemeral and not durable state. UIs should consider a change confirmed only when a fresh kind:0 from the agent arrives reflecting the requested values.

### ProjectStatus (kind:24010) = Runtime Status Only

Kind:24010 is runtime/status traffic. It is not the project roster, not PM state, not default selection state, and not agent configuration state.

Swift and TUI roster surfaces should ignore any 24010 agent payload for membership/default decisions.

## Project Agent Lookup

1. Read the project kind:31933.
2. Preserve repeated `p` tags in order.
3. Mark the first `p` tag as PM/default.
4. Overlay availability/backend details from kind:24011 inventory.
5. Overlay model/tool/skill/MCP details from each agent's kind:0 metadata.
6. Use the same kind:0 metadata for `name`/`about`/`picture` profile display.

## Data Flow

```
kind:31933 project
  ordered p tags
      |
      v
project roster rows
  first p tag = PM/default
      |
      +-- kind:24011 inventory -> available/backend labels
      |
      +-- kind:0 metadata (per agent) -> active model/tools/skills/MCPs
      |                              -> name/avatar/profile
```

## Swift Notes

`ProjectAgent` rows in Swift represent the merged roster view:

```swift
struct ProjectAgent: Identifiable, Sendable {
    let pubkey: String
    let name: String
    let backendPubkey: String
    let isPm: Bool       // true only for first 31933 p tag
    let isOnline: Bool   // inventory-backed availability
    let model: String?   // from agent's kind:0 when cached
    let tools: [String]  // from agent's kind:0 when cached
    let skills: [String]
    let mcpServers: [String]
}
```

Selectors and composers should use the ordered project roster cache, not a 24010 online-agent list. Unavailable roster members should remain selectable and visibly marked unavailable.

## Common Mistakes

1. Treating AgentDefinition author pubkeys as agent pubkeys.
2. Fetching kind:4199 to decide who is on a project roster.
3. Treating kind:24010 as the roster, PM/default source, or config source.
4. Saving PM state through agent config tags. PM is project p-tag order.
5. Hiding p-tagged agents just because no backend inventory currently advertises them.
6. Looking up agents by display name. Display name comes from kind:0 and may collide; pubkey is the only stable identity.

## Event Kind Reference

| Kind  | Name              | Purpose |
|-------|-------------------|---------|
| 0     | Metadata          | Agent profile display **and** per-agent config (model/tools/skills/MCP) |
| 4199  | AgentDefinition   | Optional install/configuration template |
| 4202  | Skill             | Skill definitions referenced by `["skill", ...]` tags |
| 31933 | Project           | Authoritative ordered roster via `p` tags |
| 24010 | ProjectStatus     | Runtime status only; not roster/config |
| 24011 | Backend Inventory | Availability/backend provenance + available-models catalog |
| 24020 | Agent Config Cmd  | Ephemeral change request; confirm via fresh kind:0 |
