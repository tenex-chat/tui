# TENEX Agent Architecture

> **Critical:** agents, agent definitions, project rosters, backend inventory, and agent config are separate concepts. Do not infer one from another.

## Core Concepts

### Agent = Nostr User

An agent is a Nostr user with a pubkey. Like any Nostr user, it can have kind:0 metadata for display name, avatar, and profile details.

Agents are not special event types. They are users that happen to be AI-powered.

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

### Agent Config (kind:34011) = Per-Agent Config

Agent-authored kind:34011 supplies per-agent configuration:

- active and available models
- active and available tools
- active and available skills
- active and available MCP servers
- backend provenance when present

Config writes may still go through command flows such as 24020, but visible config state should refresh from 34011.

### ProjectStatus (kind:24010) = Runtime Status Only

Kind:24010 is runtime/status traffic. It is not the project roster, not PM state, not default selection state, and not agent configuration state.

Swift and TUI roster surfaces should ignore any 24010 agent payload for membership/default decisions.

## Project Agent Lookup

1. Read the project kind:31933.
2. Preserve repeated `p` tags in order.
3. Mark the first `p` tag as PM/default.
4. Overlay availability/backend details from kind:24011 inventory.
5. Overlay model/tool/skill/MCP details from kind:34011 when cached.
6. Use kind:0 metadata for profile display.

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
      +-- kind:34011 config -> model/tools/skills/MCP
      |
      +-- kind:0 metadata -> name/avatar/profile
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
    let model: String?   // from 34011 when cached
    let tools: [String]  // from 34011 when cached
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

## Event Kind Reference

| Kind | Name | Purpose |
|------|------|---------|
| 0 | Metadata | Agent profile display |
| 4199 | AgentDefinition | Optional install/configuration template |
| 31933 | Project | Authoritative ordered roster via `p` tags |
| 24011 | Backend Inventory | Availability/backend provenance |
| 34011 | Agent Config | Per-agent model/tools/skills/MCP config |
| 24010 | ProjectStatus | Runtime status only; not roster/config |
