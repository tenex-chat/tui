# TENEX Agent Architecture

> **CRITICAL**: This document explains the fundamental distinction between Agents and AgentDefinitions. Getting this wrong leads to many problems.

## Core Concepts

### Agent = Nostr User

An **Agent** is simply a Nostr user with a pubkey. Like any Nostr user:
- Has a pubkey (hex string)
- Has kind:0 metadata (profile) for name, avatar, about, etc.
- Can publish events, receive mentions, etc.

**Agents are NOT special event types.** They're just users that happen to be AI-powered.

### AgentDefinition (kind:4199) = Configuration Template

An **AgentDefinition** is a configuration event that defines HOW to set up/instantiate an agent:
- Can be authored by ANYONE (not necessarily the agent's pubkey)
- Contains: instructions, role, use-criteria, phase associations
- Used as a template/recipe to configure agent behavior
- **NOT all agents have AgentDefinitions** - an agent can exist without one

### ProjectStatus (kind:24010) = Online Agents

**ProjectStatus** announces which agents are currently online for a project:

```
Tags:
- ["agent", <pubkey>, <name>, "global"?]  // Online agent
- ["model", <model-slug>, <agent-name>, ...]  // Model assignment
- ["tool", <tool-name>, <agent-name>, ...]  // Tool assignment
- ["a", <project-coordinate>] or ["e", <project-id>]  // Project reference
```

The `agent` tag contains:
1. `pubkey` - The agent's Nostr pubkey (use this for p-tags, profile fetches)
2. `name` - Denormalized agent name (for display without extra fetch)
3. Optional `"global"` marker

## How to Get Agent Info for a Project

```
1. Subscribe to ProjectStatus (kind:24010) for the project
2. Parse `agent` tags → get list of ProjectAgent { pubkey, name, model, tools, isGlobal }
3. To show avatar/full profile → fetch kind:0 for the agent's pubkey (same as any user)
```

**DO NOT** try to fetch AgentDefinition (kind:4199) to get agent info for online agents.

## Data Flow Diagram

```
┌─────────────────────────────────────────────────────────────────┐
│                        PROJECT                                   │
│                    (kind:31933)                                  │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                     PROJECT STATUS                               │
│                      (kind:24010)                                │
│                                                                  │
│  Tags:                                                           │
│  ["agent", "abc123...", "Claude", "global"]                     │
│  ["agent", "def456...", "GPT-4"]                                │
│  ["model", "claude-sonnet-4", "Claude"]                         │
│  ["tool", "web-search", "Claude", "GPT-4"]                      │
└─────────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────────┐
│                    AGENT (Nostr User)                            │
│                       pubkey: abc123...                          │
│                                                                  │
│  Profile (kind:0):                                               │
│  - name: "Claude"                                                │
│  - picture: "https://..."                                        │
│  - about: "AI assistant"                                         │
└─────────────────────────────────────────────────────────────────┘

┌─────────────────────────────────────────────────────────────────┐
│              AGENT DEFINITION (kind:4199)                        │
│              (SEPARATE - Optional Configuration)                 │
│                                                                  │
│  Tags:                                                           │
│  ["title", "Research Assistant"]                                │
│  ["role", "Expert researcher..."]                               │
│  ["instructions", "When asked..."]                              │
│  ["use-criteria", "Use for research tasks"]                     │
│                                                                  │
│  NOTE: This can be authored by ANYONE, not the agent itself     │
└─────────────────────────────────────────────────────────────────┘
```

## iOS Implementation

### ProjectAgent Model (from ProjectStatus)

```swift
struct ProjectAgent: Identifiable, Sendable {
    let pubkey: String      // Agent's Nostr pubkey
    let name: String        // Denormalized name from agent tag
    let isGlobal: Bool      // Whether agent is global
    let model: String?      // LLM model being used
    let tools: [String]     // Available tools

    var id: String { pubkey }
}
```

### Displaying Agent Info

To show an agent's avatar/profile:
1. Use the `pubkey` from ProjectAgent
2. Fetch kind:0 profile metadata for that pubkey
3. Display like any other Nostr user

### @Mention Autocomplete

When user types `@`:
1. Get online agents from ProjectStatus
2. Filter by name matching query
3. On selection, insert `@AgentName` and add p-tag with agent's pubkey

### AgentDefinition (kind:4199) - When to Use

Use AgentDefinition for:
- Agents Tab showing available agent templates
- Agent configuration/setup flows
- Browsing agent templates to add to projects

**DO NOT use for:**
- Getting info about currently running agents (use ProjectStatus)
- Displaying agent avatars (use kind:0 profile)

## Common Mistakes to Avoid

1. **Assuming AgentDefinition is authored by the agent** - WRONG. Anyone can author a definition.

2. **Fetching kind:4199 to get online agent info** - WRONG. Use ProjectStatus (kind:24010).

3. **Thinking agents need AgentDefinitions** - WRONG. Agents are just users with pubkeys.

4. **Using AgentDefinition pubkey as agent pubkey** - WRONG. The author of a definition is NOT necessarily the agent.

## Event Kind Reference

| Kind | Name | Purpose |
|------|------|---------|
| 0 | Metadata | Agent profile (name, avatar) - same as any user |
| 4199 | AgentDefinition | Configuration template (optional) |
| 24010 | ProjectStatus | Online agents for a project |
