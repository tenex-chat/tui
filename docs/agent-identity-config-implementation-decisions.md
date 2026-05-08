# Agent Identity And Config Implementation Decisions

Date: 2026-05-04

Status: **SUPERSEDED 2026-05-07.** The "kind:34011" direction described
below was abandoned. Per-agent config is carried by **kind:0 NIP-01 metadata
with TENEX-specific tags** (slug/model/skill/mcp/tool/use-criteria/p). See
`AGENT_ARCHITECTURE.md` (the canonical doc), `crates/tenex-core/src/models/
agent_config.rs`, and `crates/tenex-core/src/constants.rs::kinds` for the
real shape.

Original status (historical): handoff summary for continuing implementation in a new context window.

Scope: TENEX shared Rust core, TUI, REPL, CLI, iOS app, and Mac app surfaces that deal with agent identity, display names, project runtime status, agent config, skills, nudges, and MCP access.

## Purpose

This document records a historical handoff. Do not use the original 34011
sections as current implementation guidance. The current source-of-truth docs
are `AGENT_ARCHITECTURE.md` and
`docs/2026-05-07-inconsistency-audit-findings.md`.

## Canonical Concepts

| Concept | Decision |
| --- | --- |
| Agent identity | A Nostr pubkey. This is the only durable identity key. |
| Agent display name | The displayed label from kind:0 profile metadata for that pubkey. Never an identity key. |
| Duplicate display names | No special handling needed. If two pubkeys have the same kind:0 name, they still remain distinct by pubkey internally. |
| Project agent membership | Agent pubkeys tagged by the user on the user-signed kind:31933 project event. |
| Backend inventory | kind:24011 advertises which agent pubkeys are available from approved backends. |
| Project runtime status | kind:24010 is runtime/status traffic only. It is not roster, PM/default, availability, or config truth. |
| Agent config state | kind:0 NIP-01 metadata authored by the agent, with TENEX-specific tags. |
| Agent config command | kind:24020, a request/command to change an agent config. It is not durable state. |
| Historical 34011 direction | Abandoned and unused. Do not subscribe to or project config state from kind:34011. |
| Skill | Prompt-facing capability attachable to a message or enabled for an agent. |
| Nudge | Delete/retire entirely. Do not restore as a product concept. |
| MCP server access | Agent-level external server/tool permission set. UI parity work is deferred for now. |

## Hard Rules

1. Pubkey is the only agent identity key.
2. Do not key durable maps, aggregation, selections, or commands by display name, slug, or config label.
3. Display names shown for pubkeys must come from kind:0 only.
4. Do not display or fall back to the agent slug from 24011, historical 34011, or installed-agent inventory as the agent name.
5. `24010` must not be treated as roster membership, PM/default state, availability, or agent configuration.
6. Current model/tool/skill/MCP state comes from agent-authored kind:0 metadata.
7. UI config changes publish `24020`; confirmation comes from receiving the updated kind:0 metadata.
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

### kind:0 Agent Profile And Config State

Owned by the agent. This is the durable current profile and config state for
that agent. TENEX intentionally extends kind:0 with tags for config.

Implementation implications:

- Standard profile display fields (`name`, `about`, `picture`) come from the
  kind:0 content.
- Active model/tools/skills/MCP servers come from kind:0 tags.
- Visible tool/skill/MCP options come from kind:0 tags.
- Available model catalog data comes from approved backend kind:24011
  inventory, not from 24010 or 34011.
- Clients should render current config controls from kind:0.
- If no agent-authored kind:0 config is known, config UI should show that
  config has not arrived rather than inferring from 24010.
- Historical kind:34011 must be ignored for current config projection.

### kind:24020 Agent Config Command

Command/request event asking an agent to update its config.

Implementation implications:

- UIs publish 24020 when changing config.
- UIs should not mark the change confirmed until a matching/new kind:0 arrives.
- Naming should distinguish command/request from state.

## Subscription And Refresh Decisions

Subscribe to kind:0 for every relevant agent pubkey from:

- 31933 project membership `p` tags.
- Approved backend inventory from 24011.

When a relevant kind:0 arrives:

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

- Current model/tool/skill/MCP state comes from kind:0.
- Available model catalog data comes from approved 24011 backend inventory.
- Save publishes 24020.
- Confirmation comes from kind:0.
- Replacing whole config is acceptable; do not silently preserve stale inferred fields from 24010.

MCP UI parity is deferred. For now, keep the protocol rule clear: MCP access is
per agent and lives in kind:0/24020 config flows.

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
- Historical: moved agent config/options behavior toward 34011. This was later
  superseded by the 2026-05-07 kind:0 contract.
- Added the worksheet at `docs/technical-debt-mixup-worksheet.md`.

The current tree may contain additional uncommitted work. Inspect `git status --short` before editing or staging anything.

## Immediate Next Implementation Work

1. Tighten kind:0 subscriptions from 31933/24011.
   - Subscribe to every project roster `p` tag.
   - Subscribe to every approved 24011 inventory agent.
   - Request kind:0 profiles for those agent pubkeys.
   - Keep subscription deduplication.

2. Refresh affected runtime views from kind:0.
   - When a relevant kind:0 arrives, identify all projects affected by that agent through 31933 membership.
   - Emit the existing refresh/delta path or introduce a dedicated config-changed delta if needed.
   - Ensure iOS/Mac and TUI receive the same semantic update.

3. Update tests.
   - Same display name across two pubkeys remains distinct.
   - 24010 does not populate current config.
   - 31933 roster and 24011 inventory pubkeys trigger kind:0 subscription.
   - 34011 does not populate current config.
   - kind:0 refreshes project views where the agent is relevant.

4. Update docs and names.
   - Replace any remaining text that says 24010 carries agent config.
   - Replace any remaining text that says 34011 is current config state.
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
- 34011 does not set current model/tools/skills/MCPs.
- kind:0 updates visibly refresh config surfaces without app restart.
- TUI, iOS, Mac, REPL, and CLI agree that config state comes from kind:0 and config changes are requested through 24020.
- Agent names shown for pubkeys are kind:0-only.
