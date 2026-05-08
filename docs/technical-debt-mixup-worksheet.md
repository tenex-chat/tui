# TENEX Client Concept Mixup Worksheet

Date: 2026-05-04

Status: **HISTORICAL / SUPERSEDED 2026-05-07.** Decisions 2 and 3 below
discuss a "kind:34011" direction that was never implemented and was
abandoned. The actual current architecture uses **kind:0 NIP-01 metadata
with extra tags** for per-agent config. Treat the `[x]` checkboxes in this
file as records of *intent at the time*, not as accurate descriptions of
the current code. See `AGENT_ARCHITECTURE.md` and `docs/2026-05-07-
inconsistency-audit-findings.md` for the up-to-date picture.

Original status: Draft for product and technical clarification

Scope: TENEX TUI Client, shared Rust core, TUI, iOS app, Mac app, and CLI surfaces where agent identity, agent configuration, `24010` semantics, skills, nudges, and MCP access overlap.

Use this document as a review form. Check one option in each decision block, add clarifying notes, then turn the chosen answers into implementation tickets.

## Executive Summary

The codebase has several concepts that changed direction over time but were not
fully renamed, migrated, or separated. The current 2026-05-07 clarification has
settled several key definitions:

- Agent identity is a Nostr pubkey. Display name is presentation only.
- Project agent membership is encoded by agent pubkey `p` tags on the user-signed `31933` project event.
- Backend inventory and availability are encoded by approved backend-authored `24011` inventory.
- Durable per-agent configuration state is agent-authored kind:0 NIP-01 metadata with TENEX-specific tags.
- Agent configuration command/request is currently `24020`.
- `34011` is historical/unused and must not be treated as current config state.
- MCP server access is agent-level.
- Nudges should be entirely removed and cleaned up.

The remaining highest-risk mixups are:

- Existing code still sometimes treats agent display name as an identity key.
- Agent configuration behavior must stay split between durable kind:0 state and `24020` requests, with no fallback to `24010` or historical `34011`.
- UI refresh behavior for kind:0 config updates appears incomplete or inconsistent.
- TUI and iOS expose different parts of the same agent configuration surface.
- "Nudge" remains in code and UI names even though it should be removed.
- Some operational docs still describe older command shapes.

The recurring pattern is that identity, status, command, catalog, and UI state are intertwined. The cleanup should start by naming the concepts precisely, then making one source of truth per concept.

## Glossary To Confirm

Fill this in before implementation. These definitions should become the language used in code, docs, and UI.

| Concept | Proposed definition | Confirmed? | Clarification |
| --- | --- | --- | --- |
| Agent identity | A Nostr pubkey. Display name is presentation only. | [x] Yes [ ] No |  |
| Agent display name | A mutable label from kind:0 profile metadata. Never a key. | [x] Yes [ ] No | No status metadata, installed-agent slug, or config label fallback. |
| Project agent membership | Agent pubkeys tagged by the user on the user-signed `31933` project event. | [x] Yes [ ] No |  |
| Backend inventory | Approved backend-authored `24011` events advertise which agent pubkeys are available from which backend. | [x] Yes [ ] No | This is availability/online truth for roster UI. |
| Project runtime status | `24010`: runtime/status traffic only. | [x] Yes [ ] No | It does not publish roster membership, PM/default state, or agent configuration. |
| Agent config state | Durable per-agent configuration, carried by agent-authored kind:0 metadata tags. | [x] Yes [ ] No | Historical `34011` is unused. |
| Agent config command | A request to change config, currently represented by `24020`. | [x] Yes [ ] No |  |
| Skill | A prompt-facing capability that can be attached to a message or enabled for an agent. | [x] Yes [ ] No |  |
| Nudge | Remove entirely and clean up stale code, UI labels, event paths, and naming. | [x] Yes [ ] No | Chosen direction: deletion, not restoration. |
| MCP server access | Agent-level external tool/server permission set. | [x] Yes [ ] No |  |

## Decision 1: Agent Identity

Problem: Architecture describes agents as Nostr pubkeys, but project status aggregation can still use display names or slugs as keys. That can collapse two distinct agents with the same name, overwrite entries, or target the wrong agent.

Evidence:

- `AGENT_ARCHITECTURE.md` says "Agent = Nostr User" and describes pubkeys in status tags.
- `crates/tenex-core/src/models/project_status.rs` has an aggregation key that prefers `agent.name` over `agent.pubkey`.
- Project status parsing stores agents in a map keyed by `tag[2]`, which is the name in the documented tag shape.

Decision:

- [x] Pubkey is the only identity key. Names are display-only.

Remaining clarifications:

- Legacy events without pubkeys should be handled by:
  - [x] Ignoring the agent entry.
  - [ ] Rendering it as "unknown legacy agent" without actionable controls.
  - [ ] Creating a temporary local synthetic ID.
  - [ ] Other:
- Same display name across different pubkeys should:
  - [ ] Show duplicate names with pubkey suffix.
  - [ ] Prefer profile metadata to disambiguate.
  - [x] Other: display name still only comes from kind:0; pubkey remains the action/identity target.

Decision notes:

> Confirmed: display names are never keys.

Acceptance criteria:

- [ ] No durable agent map is keyed by display name.
- [ ] UI actions targeting an agent pass pubkey, not name.
- [ ] Duplicate display names remain distinct.
- [ ] Tests cover same-name agents from different pubkeys/backends.

## Decision 2: Agent Config Source Of Truth

Problem: The code uses multiple event kinds for related ideas:

- kind:0: durable per-agent signed profile/config state.
- `34011`: historical durable-config direction that was abandoned.
- `24020`: currently named `AGENT_CONFIG`, used as a config update command.
- `24010`: runtime status only; not roster, PM/default, availability, or agent configuration.

Confirmed:

- [x] kind:0 is durable per-agent configuration state.
- [x] `34011` is historical/unused.
- [x] `24020` is an agent configuration command/request.

Remaining clarifications:

- When a user changes an agent model, the UI should publish:
  - [x] `24020` command, then wait for the agent to publish kind:0.
  - [ ] Direct kind:0 update signed by the agent identity.
  - [ ] Direct local store update plus async publish.
  - [ ] Other:
- Confirmation should come from:
  - [x] Receiving updated kind:0.
  - [ ] Receiving updated `24010`.
  - [ ] Local optimistic state only.
  - [ ] Other:
- Available model/tool/skill catalogs should come from:
  - [x] Current tool/skill/MCP visibility from agent kind:0; available model catalog from approved 24011 inventory.
  - [ ] Backend/provider catalog.
  - [ ] `24010`, if its clarified role includes catalogs. OBSOLETE: 24010 must not carry roster/config catalog truth.
  - [ ] Separate catalog event/API.
  - [ ] Other:
- Define `24010`:
  - Answer: Runtime/status traffic only. It must not create roster agents, mark PM, select defaults, gate roster lookup, or provide agent configuration.
- Define `24011`:
  - Answer: Approved backend inventory. It determines whether roster agents are available and which backend advertises them.

Decision notes:

> Do not call `24020` durable config state. It is a command/request: update this agent to use this configuration. Do not call `34011` current config state; it is historical/unused.

Acceptance criteria:

- [x] Constants and type names distinguish config state from config command.
- [x] CLI, TUI, iOS, and Mac agree on confirmation semantics.
- [x] API comments match runtime behavior.
- [x] Tests cover model change propagation from publish to UI refresh.

## Decision 3: kind:0 Subscription And UI Refresh

Problem: Agent config is carried by kind:0, but not every client surface has a
dedicated live-update signal when relevant kind:0 events arrive. Project
membership is confirmed as agent pubkey `p` tags on the user-signed `31933`;
approved 24011 inventory can add agent pubkeys that also need kind:0 fetches.
Historical kind:34011 must not drive subscriptions or UI refresh.

Choose the desired flow:

- [ ] A. Worker emits a dedicated `AgentConfigChanged` delta for relevant kind:0. Recommended.
- [ ] B. Worker emits `ProjectStatusChanged` for any kind:0 that affects current project agents.
- [ ] C. UIs poll/reload config after relevant note-key events.
- [x] D. Other: event-driven reactive UI updates when relevant kind:0 events arrive.

Clarifications:

- Subscribe to kind:0 for agents from:
  - [x] Project membership `p` tags on user-signed `31933`.
  - [x] Approved backend inventory from kind:24011.
  - [ ] Installed agent list.
  - [ ] Currently opened config sheet only.
- If the selected agent config changes while the modal is open:
  - [x] Update the modal live.
  - [ ] Show a "remote changes available" state.
  - [ ] Ignore until modal is reopened.
  - [ ] Other:

Decision notes:

>

Acceptance criteria:

- [x] Receiving relevant kind:0 causes visible config updates without app restart.
- [ ] Any non-`31933` source for relevant agents is explicitly justified and subscribed; approved 24011 inventory is the current justified source.
- [x] Same active agent can refresh while selected.
- [x] TUI and iOS use the same core event semantics.

## Decision 4: Cross-Platform Agent Config Parity

Problem: MCP server access is confirmed as agent-level, but TUI and iOS/Mac do not currently expose the same controls. TUI exposes MCP server selection in the agent config modal. iOS appears to preserve existing `mcpServers` when saving but does not expose equivalent selection UI.

Choose one:

- [x] A. iOS/Mac must expose agent-level MCP server selection now.
- [ ] B. MCP selection remains TUI-only, but iOS/Mac must be read-only and unable to clobber it.
- [ ] C. Other:

Confirmed:

- [x] MCP servers are selected per agent.

Remaining clarifications:

- Saving a partial config form should:
  - [ ] Preserve unknown fields from latest kind:0.
  - [x] Replace the whole config.
  - [ ] Patch only changed fields.
  - [ ] Other:

Decision notes:

>

Acceptance criteria:

- [ ] TUI, iOS, and Mac cannot silently overwrite each other's MCP changes.
- [x] FFI exposes the complete config/options needed by every client.
- [ ] Partial-save behavior is explicit and tested.

## Decision 5: Nudge Versus Skill

Problem: Send paths now pass empty nudge IDs while selectors and UI labels still say "NudgeSkill" or "Nudges & Skills." The product decision is now clear: nudge should be entirely removed and cleaned up.

Decision:

- [x] Remove nudge from composer flows, management surfaces, stale names, UI labels, and send payloads.

Confirmed cleanup:

- [x] User-facing label should not include "Nudge".
- [x] Stale `NudgeSkill` naming should be renamed or removed.
- [x] Nudge IDs should not remain as dead parameters in active send paths.

Decision notes:

> Nudge is deletion work, not a product question.

Acceptance criteria:

- [x] Composer labels match actual send payloads.
- [x] Dead enum cases and stale selector names are removed.
- [x] Tests cover skill-only payload behavior.

## Decision 6: Docs And Command Truth

Problem: Some local guidance says `cargo run -p tenex-cli -- daemon`, while the CLI currently uses `--daemon` as a flag.

Choose one:

- [ ] A. Update docs to use `cargo run -p tenex-cli -- --daemon`. Recommended if CLI shape is correct.
- [x] B. Add a `daemon` subcommand for ergonomic compatibility.
- [ ] C. Support both forms.
- [ ] D. Other:

Decision notes:

>

Acceptance criteria:

- [x] `AGENTS.md`, README, crate docs, and scripts agree.
- [x] The documented command has been run successfully.

## Intertwined Concepts Map

Use this section to decide which boundaries should become code boundaries.

```text
User action in client
  -> command/request event, for example 24020
  -> backend or agent processes request
  -> durable state event, kind:0 NIP-01 metadata with TENEX-specific tags
  -> status/catalog/other event, for example 24010 if clarified
  -> UI delta and local store refresh
```

Questions:

- Which steps are allowed to be optimistic?
  - Answer:
- Which steps require signed Nostr confirmation?
  - Answer:
- Which event kinds are commands rather than state?
  - Answer: `24020` is confirmed as an agent config command/request. Others still need review.
- Which event kinds are state rather than commands?
  - Answer: kind:0 is confirmed as durable per-agent profile/config state. Historical `34011` is unused.
- Which event kinds should never be used as UI source of truth?
  - Answer:

## Proposed Cleanup Order

This order keeps identity and data flow fixes ahead of UI cleanup.

1. [ ] Write the final glossary and update comments/constants to match it.
2. [ ] Make pubkey the only durable agent identity key.
3. [ ] Rename or separate `24020` command concepts from kind:0 state concepts.
4. [ ] Add a dedicated kind:0 refresh path and test it.
5. [ ] Align config catalogs/options across CLI, TUI, iOS, and Mac.
6. [ ] Implement agent-level MCP parity or explicit read-only preservation on iOS/Mac.
7. [ ] Remove nudge concepts, stale names, UI labels, and dead send parameters.
8. [ ] Update docs and command examples.
9. [ ] Add regression fixtures for mixed old/new event streams.

## Risk Register

| Risk | Impact | Likelihood | Mitigation |
| --- | --- | --- | --- |
| Same-name agents collapse into one UI/action target | High | Medium | Key by pubkey and add duplicate-name fixture |
| iOS saves stale partial config and loses MCP changes | High | Medium | Patch semantics or preserve unknown latest fields |
| CLI waits for the wrong confirmation event | Medium | Medium | Confirm against chosen source of truth |
| UI shows stale model/skill data after kind:0 | Medium | High | Dedicated config delta and live modal refresh |
| Users see nudge UI that cannot affect messages | Medium | High | Remove nudge concepts and labels entirely |
| Docs send contributors down stale command path | Low | Medium | Run and update command examples |

## Open Questions For Pablo

- Is `24020` still part of the desired protocol, or should clients write kind:0 directly?
  - Answer: `24020` is confirmed as the agent config command/request; clients wait for agent-authored kind:0 confirmation.
- What is the exact definition of `24010` now?
  - Answer: Runtime/status traffic only; not roster, PM/default, availability, or config truth.
- Should `24010` carry available model/tool/skill catalogs?
  - Answer: No. Use kind:0 for per-agent current config and visible tool/skill/MCP state; use `24011` for backend inventory availability and available model catalog.
- For agent config changes, should UI confirmation come from updated kind:0, updated `24010`, or optimistic local state?
  - Answer: Updated kind:0.
- Should iOS/Mac match all TUI agent configuration controls, or intentionally expose a smaller surface?
  - Answer:
- Should old events without pubkeys be migrated, hidden, or displayed as read-only legacy data?
  - Answer:

## Implementation Ticket Template

Copy this block for each chosen cleanup item.

```markdown
### Ticket:

Decision this implements:

Files likely touched:

-

Behavior change:

Test coverage:

- [ ] Unit fixture
- [ ] FFI/core delta test
- [ ] TUI behavior check
- [ ] iOS/Mac behavior check
- [ ] Docs command verification

Rollback plan:

Open questions:

```

## Review Notes

Add extra observations here as the worksheet is reviewed.

>
