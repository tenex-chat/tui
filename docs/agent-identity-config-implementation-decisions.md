# Agent Identity And Config Implementation Decisions

Date: 2026-05-04

Status: handoff summary for continuing implementation in a new context window.

Scope: TENEX shared Rust core, TUI, REPL, CLI, iOS app, and Mac app surfaces that deal with agent identity, display names, project runtime status, agent config, skills, nudges, and MCP access.

## Purpose

This document records the product and technical decisions that are now settled. It is not a brainstorming worksheet. Use it as the implementation source of truth for follow-up cleanup.

## Canonical Concepts

| Concept | Decision |
| --- | --- |
| Agent identity | A Nostr pubkey. This is the only durable identity key. |
| Agent display name | The displayed label from kind:0 profile metadata for that pubkey. Never an identity key. |
| Duplicate display names | No special handling needed. If two pubkeys have the same kind:0 name, they still remain distinct by pubkey internally. |
| Project agent membership | Agent pubkeys tagged by the user on the user-signed kind:31933 project event. |
| Backend inventory | kind:24011 advertises which agent pubkeys are available from approved backends. |
| Project runtime status | kind:24010 is runtime/status traffic only. It is not roster, PM/default, availability, or config truth. |
| Agent config state | kind:34011, durable per-agent configuration authored by the agent. |
| Agent config command | kind:24020, a request/command to change an agent config. It is not durable state. |
| Skill | Prompt-facing capability attachable to a message or enabled for an agent. |
| Nudge | Delete/retire entirely. Do not restore as a product concept. |
| MCP server access | Agent-level external server/tool permission set. UI parity work is deferred for now. |

## Hard Rules

1. Pubkey is the only agent identity key.
2. Do not key durable maps, aggregation, selections, or commands by display name, slug, or config label.
3. Display names shown for pubkeys must come from kind:0 only.
4. Do not display or fall back to the agent slug from 24011, 34011, or installed-agent inventory as the agent name.
5. `24010` must not be treated as roster membership, PM/default state, availability, or agent configuration.
6. Current model/tool/skill/MCP state comes from `34011`.
7. UI config changes publish `24020`; confirmation comes from receiving the updated `34011`.
8. Missing legacy agent pubkeys should be ignored, not synthesized into local identities.

## Event Responsibilities

### kind:31933 Project Event

Owned by the user. Its agent pubkey `p` tags define project membership.

Implementation implications:

- Project membership editing mutates the user-signed 31933.
- Agent membership is independent of backend runtime status.
- UIs may render project members from 31933 even if no backend is currently running them.

### kind:24010 Project Runtime Advertisement

Owned by a backend. It is runtime/status traffic only.

Implementation implications:

- 24010 is not roster membership, PM/default state, availability, or config state.
- 24010 can be emitted by multiple backends for the same project.
- 24010 agent payloads must not create roster rows or pick defaults.
- 24010 model/tool/skill/MCP-like tags must not be interpreted as current agent configuration.

### kind:24011 Backend Inventory

Owned by a backend. It advertises agent pubkeys available from that backend.

Implementation implications:

- Availability/online labels for roster agents come from 24011.
- Backend provenance in roster and install UI comes from 24011.
- A project roster agent absent from 24011 remains a roster member but is displayed as unavailable.

### kind:34011 Agent Config State

Owned by the agent. This is the durable current config and available config catalog for that agent.

Implementation implications:

- Available models/tools/skills/MCP servers come from 34011.
- Active model/tools/skills/MCP servers come from 34011.
- Clients should render config controls from 34011.
- If no 34011 is known, config UI should show that config has not arrived rather than inferring from 24010.

### kind:24020 Agent Config Command

Command/request event asking an agent to update its config.

Implementation implications:

- UIs publish 24020 when changing config.
- UIs should not mark the change confirmed until a matching/new 34011 arrives.
- Naming should distinguish command/request from state.

## Subscription And Refresh Decisions

Subscribe to kind:34011 for every relevant agent pubkey from:

- 31933 project membership `p` tags.
- Approved backend inventory from 24011.

When a 34011 arrives:

- Upsert the agent config by agent pubkey.
- Refresh all project/runtime views where that agent is relevant.
- This includes projects whose current 24010 runtime advertisements include that agent, not only projects whose 31933 membership includes that agent.
- TUI and iOS/Mac should share the same core event semantics.
- If a config modal is open for that agent, it should update live when there are no local unsaved edits.

## UI Rules

### Agent Names

- Render agent/user names through a single helper per platform.
- Rust core helper: kind:0 display name, then kind:0 name, then shortened pubkey.
- Swift helper should call the core profile-name API and only use the shortened pubkey fallback.
- Do not title-case, slug-format, or derive names from agent definitions when rendering a pubkey identity.

### Agent Definition Titles

Agent definition/catalog names are different from pubkey identity display names.

Allowed:

- Showing an agent definition title in catalog/discovery/hiring UI.

Not allowed:

- Using that definition title as the displayed name for a running agent pubkey.
- Targeting an agent action by definition title.

### Agent Config UI

- Model/tool/skill/MCP options come from 34011.
- Save publishes 24020.
- Confirmation comes from 34011.
- Replacing whole config is acceptable; do not silently preserve stale inferred fields from 24010.

MCP UI parity is deferred. For now, keep the protocol rule clear: MCP access is per agent and lives in 34011/24020 config flows.

### Nudge Cleanup

Nudge is deletion work:

- Remove user-facing "Nudge" labels.
- Remove stale `NudgeSkill` names.
- Remove dead nudge IDs from active send paths.
- Keep skill-only payload behavior tested.

## Implementation Already Started

The prior context committed a cleanup with this headline:

`89e06c18 Clarify agent identity and config sources`

Key pieces from that work:

- Added a Rust `agent_display` helper for kind:0-only display names.
- Added a Swift `AgentDisplayName` helper.
- Stopped using project-status or installed-agent slugs as display fallbacks.
- Changed project status aggregation toward pubkey identity.
- Moved agent config/options behavior toward 34011.
- Added the worksheet at `docs/technical-debt-mixup-worksheet.md`.

The current tree may contain additional uncommitted work. Inspect `git status --short` before editing or staging anything.

## Immediate Next Implementation Work

1. Tighten 34011 subscriptions from 31933/24011.
   - Subscribe to every project roster `p` tag.
   - Subscribe to every approved 24011 inventory agent.
   - Request kind:0 profiles for those agent pubkeys.
   - Keep subscription deduplication.

2. Refresh affected runtime views from 34011.
   - When a 34011 arrives, identify all projects affected by that agent through 31933 membership.
   - Emit the existing refresh/delta path or introduce a dedicated config-changed delta if needed.
   - Ensure iOS/Mac and TUI receive the same semantic update.

3. Update tests.
   - Same display name across two pubkeys remains distinct.
   - 24010 does not populate current config.
   - 31933 roster and 24011 inventory pubkeys trigger 34011 subscription.
   - 34011 refreshes project views where the agent is currently running.

4. Update docs and names.
   - Replace any remaining text that says 24010 carries agent config.
   - Rename any constants/types that imply 24020 is durable config state.
   - Keep "agent display name" strictly tied to kind:0.

## Deferred Work

- MCP selector UI parity on iOS/Mac.
- Full nudge removal if not already completed in current dirty work.
- Broader command/docs cleanup beyond the already-settled daemon command direction.

## Acceptance Criteria

- No durable agent map is keyed by display name or slug.
- UI actions targeting agents pass pubkeys.
- 24010 parsing does not set current model/tools/skills/MCPs.
- 34011 updates visibly refresh config surfaces without app restart.
- TUI, iOS, Mac, REPL, and CLI agree that config state comes from 34011 and config changes are requested through 24020.
- Agent names shown for pubkeys are kind:0-only.
