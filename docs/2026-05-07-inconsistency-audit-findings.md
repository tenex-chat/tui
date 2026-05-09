# TENEX-TUI Inconsistency Audit Findings

Date: 2026-05-07

This file is the durable output of an inconsistency review. Status of each
finding: **bug**, **drift**, **hygiene**, or **doc-only**. Severity: **HIGH**,
**MED**, **LOW**.

---

## Ground Truth (confirmed by project owner, 2026-05-07)

- **Per-agent config** (active model, skills, MCPs, tools, slug) is carried by
  **kind:0 NIP-01 metadata** authored by the agent itself, with extra tags
  (`slug`, `model`, `skill`, `mcp`, `use-criteria`, `p` for backend).
- **kind:34011 is unused**. Every reference that presents 34011 as current
  config state in docs/comments is stale.
- **kind:0 with these extra tags is intentional**, even though it's an unusual
  use of NIP-01.
- Other event-kind responsibilities are as described in
  `crates/tenex-core/src/constants.rs::kinds`.

---

## C1 — `kind:34011` doc/code drift `[doc-only, MED]`

The kind is referenced **only** in markdown and Swift comments; no Rust
parser, subscription, or store touches it. Real source is kind:0.

### Initial files with stale 34011 references

- `AGENT_ARCHITECTURE.md` (~8 mentions; entire "Agent Config (kind:34011)"
  section, the data-flow diagram, the Swift sample struct, the Event Kind
  Reference table)
- `docs/agent-identity-config-implementation-decisions.md` (~15 mentions; Hard
  Rule #6, "Subscription And Refresh Decisions", "Agent Config UI" rules)
- `docs/technical-debt-mixup-worksheet.md` (Decision 2 + Decision 3 are
  effectively dead with `[x]` checkboxes pointing at 34011)
- `Plans/ok-so-the-backend-clever-pebble.md` (already self-titled "Obsolete")
- `ios-app/Sources/TenexMVP/CoreManager/TenexCoreManager.swift:167`
- `ios-app/Sources/TenexMVP/CoreManager/TenexCoreManager+Fetch.swift:59,126`
- `ios-app/Sources/TenexMVP/TenexCore/TenexCoreActor.swift:472`
- `ios-app/Sources/TenexMVP/Views/SkillSelectorSheet.swift:6`

### Resolution

Recommended action: rewrite `AGENT_ARCHITECTURE.md` so it describes kind:0
as the per-agent config carrier. Archive the implementation-decisions doc
and the worksheet (their decisions are historical). Fix the Swift comments.
Delete the self-labelled-obsolete plan.

Cleanup update 2026-05-08: `AGENT_ARCHITECTURE.md` now describes kind:0 as
the per-agent config/profile carrier and marks historical 34011 unused.
`docs/agent-identity-config-implementation-decisions.md`,
`docs/technical-debt-mixup-worksheet.md`, and
`Plans/ok-so-the-backend-clever-pebble.md` now present 34011 only as
historical/unused, not as current durable config state.

---

## C2 — Nudge cleanup never happened `[bug+drift, MED]`

`docs/technical-debt-mixup-worksheet.md` Decision 5 has every acceptance
criterion `[x]` checked, but **nudges are still wired through the entire
system**:

- Model + parser: `crates/tenex-core/src/models/nudge.rs`
- Store: `ContentStore.nudges: HashMap<String, Nudge>`
  (`crates/tenex-core/src/store/content_store.rs:12`)
- FFI: `createNudge`, `deleteNudge`, `getNudges` exposed in Swift bindings
  (auto-generated `tenex_core.swift:663,691,919`)
- Constants: `kinds::NUDGE = 4201` and `DEFAULT_NUDGE_TITLE`
  (`crates/tenex-core/src/constants.rs:20,59`)
- Stats aggregation: `crates/tenex-core/src/stats.rs:161,191` includes 4201
- **Dead-parameter threading**: `nudge_ids: Vec::new()` /
  `nudgeIds: []` is passed at every send call site:
  - `crates/tenex-repl/src/commands.rs:785,905,931`
  - `crates/tenex-repl/src/main.rs:434`
  - `crates/tenex-tui/src/input/editor_handlers.rs:387,462`
  - `crates/tenex-tui/src/input/modal_handlers/ask.rs:145`
  - `ios-app/Sources/TenexMVP/Composer/ComposerDependencies.swift:123,142`
  - FFI signature: `sendMessage(...nudgeIds: [String]...)` and
    `sendThread(...nudgeIds: [String]...)`
- UI test still references nudges: `ios-app/Tests/UITests/ToolbarSlashButtonUITest.swift:130`

### Resolution

Two options for the user to choose:

1. **Delete entirely** (matches the worksheet intent): drop the
   `nudge_ids` parameter from FFI signatures, delete `models/nudge.rs`,
   `ContentStore.nudges`, `kinds::NUDGE`, `DEFAULT_NUDGE_TITLE`, the
   `createNudge`/`deleteNudge`/`getNudges` FFI methods, the
   `nudge_skills_sheet` UI test screenshot. Regenerate Swift bindings.
2. **Mark as deferred**: update the worksheet to uncheck Decision 5
   acceptance criteria and stop pretending nudge cleanup is done.

---

## C3 — Identity-keying violations `[bug, mixed]`

### HIGH — `crates/tenex-cli/src/cli/daemon.rs:2104`

`find_agent_in_project()` does `if agent.name == agent_name` (exact-match
on display name). It's used in three RPC paths: `send_message` (line 868),
`publish_thread` (line 1000), `set_agent_model` (line 1867). External
JSON-RPC clients can target an agent by **display name only**, with no
disambiguation — duplicate names silently route to the first match. The
returned `agent_pubkey` is correct, but the lookup mechanism is wrong.

Fix: take a pubkey directly, OR fall through to pubkey if name is
ambiguous, OR error on collision.

### MED — `crates/tenex-core/src/store/roster.rs:60-63`

Display-name fallback chain: inventory slug → config slug → pubkey
prefix. This is **fine for display** (the field is called `name` and
`pubkey` is unaffected), but renaming an agent or changing inventory can
flip the displayed name without changing identity. Worth a comment so
future readers don't think `name` is stable.

### Confirmed clean — no fix needed

- `crates/tenex-core/src/models/project_status.rs:266`
  (`agent_aggregation_key`) uses pubkey. ✓
- `crates/tenex-core/src/store/content_store.rs:9` agent_definitions
  HashMap is keyed by **event id** (kind:4199 catalog entries). ✓ This is
  catalog content, not running agent identity.
- REPL fuzzy `/agent` selection is interactive — first match wins is
  acceptable for an interactive CLI; the downstream `state.current_agent`
  stores the pubkey, not the name.

---

## C4 — Cross-platform agent-config parity bugs `[bug, HIGH]`

### Field coverage matrix

| Field         | TUI               | iOS/Mac          | CLI               | REPL  |
|---------------|-------------------|------------------|-------------------|-------|
| model         | yes (select)      | yes (picker)     | yes (set)         | no    |
| skills[]      | yes (toggle)      | yes (toggle)     | preserved         | no    |
| mcp_servers[] | yes (toggle)      | **no UI**        | preserved         | no    |
| slug          | read-only         | read-only        | no                | no    |
| tools[]       | **absent**        | yes (toggle)     | ignored           | no    |

### HIGH — iOS silently clobbers MCP servers on save

`ios-app/Sources/TenexMVP/Views/AgentConfigSheet.swift` (~line 318-325 for
UI absence; ~line 38, 553 for the always-empty `selectedMcpServers`; ~line
574-577 for the save call).

When iOS calls `updateGlobalAgentConfig(agentPubkey, model, skills,
mcpServers, tags)`, `mcpServers` is built from `selectedMcpServers`, which
is **declared but never populated from existing config and never displayed
to the user**. Saving from iOS overwrites any MCP server access list to
empty.

Worksheet Decision 4 Decision A ("iOS/Mac must expose agent-level MCP
server selection now") was marked `[x]` but never executed.

Fix options: (a) implement the MCP UI section, OR (b) preserve MCP from
latest kind:0 (don't include in payload if no UI to edit it).

### MED — TUI does not expose `tools[]` selection

`crates/tenex-tui/src/ui/modal_handlers/agent.rs:340-345` builds the save
payload from model/skills/mcp_servers but never `tools`. TUI users can't
view or edit tools. Asymmetric: only iOS supports tools.

### MED — CLI partial-save patches only `model`, re-uses cached skills/mcps

`crates/tenex-cli/src/daemon.rs:1878-1884`. CLI sets only `model` and
re-includes the last-known `skills` and `mcp_servers` from local store. If
the local store is stale, this re-publishes stale config.

Fix: read latest kind:0 right before publishing.

### MED — REPL has no agent-config commands

REPL can switch active agent but cannot configure one. Intentional gap or
TODO?

---

## C5 — FFI surface inconsistencies `[hygiene, MED]`

### MED — Inconsistent error semantics for read methods

Two divergent patterns:

- `auth_api`, `agents_api`, `trust_api`: read methods return
  `Result<T, TenexError>` with explicit `LockError` on lock failure.
- `data_api`, `ui_state_api`, `projects_api`: read methods silently swallow
  lock failure and return empty Vec / 0 / false / None.

Examples of silent-fail:
- `data_api.rs:8-20` `get_projects()` returns `Vec::new()` on lock failure.
- `data_api.rs:64-76,84-86` `get_conversation_runtime_ms()`,
  `get_today_runtime_ms()` return 0 on error.
- `ui_state_api.rs:8-20,77-87` `is_conversation_archived()`,
  `toggle_conversation_archived()` swallow lock failures with `return`.
- `projects_api.rs:94-112` `is_project_online()`,
  `get_project_backend_pubkey()` return bool/Option silently.

Callers can't distinguish "no data" from "lock failure". Pick one
convention.

### MED — Fire-and-forget vs await-with-timeout for mutation methods

Most mutations wait `Duration::from_secs(10)` for `response_rx`. But:
- `agents_api.rs:79-97` `create_backend_agent()` is fire-and-forget.
- `agents_api.rs:440-456` `create_nudge()` is fire-and-forget (also dead;
  see C2).

Callers can't detect publish failure. Pick a policy.

### LOW — Timeout durations vary

Most: 10 s. `bunker_api.rs:52, 102`: 5 s for bunker stop + audit log. No
documented rationale.

### LOW — Async bridging is one-off

Only `standalone_audio_api.rs:50-70` uses an explicit `runtime.block_on()`
for ElevenLabs/OpenRouter clients. Generalize or document the policy.

### LOW — Cross-domain naming drift

Same concept, different names: `backend_pubkey` vs `agent_pubkey` for the
agent-backend relationship. Within a file consistent; across files isn't.

### LOW — Spotty rustdoc

`ui_state_api.rs:5-49` (set_visible_projects, set_audio_prompt, etc.) and
parts of `lifecycle_api.rs:32-36` lack rustdoc; `callback_api.rs:9-18` is
exemplary. Bring the spotty ones up.

---

## C6 — Subscription / refresh path bugs `[bug, HIGH for iOS]`

### Subscriptions in `crates/tenex-core/src/nostr/worker.rs`

| Kind | Where | Notes |
|------|-------|-------|
| 31933 (project) | line 1950 | owned + participant a-tags |
| 24010 (status) | line 1978 | ephemeral, no `since` filter |
| 24011 (inventory) | line 1999 | ephemeral, no `since` filter |
| **0 (agent config)** | line 781 (`subscribe_agent_configs`) | authors = pubkeys from projects + inventories. Replaceable, no `since`. |
| 1 (messages) | line 611 | per-project |
| 30023 (reports) | line 641 | per-project |
| STREAM_TEXT_DELTA | line 668 | per-project |

### Deltas / notifications

Emitted on:
- `CoreEvent::Message(Message)` — kind:1 (worker.rs:2523)
- `CoreEvent::ProjectStatus(ProjectStatus)` — kind:24010 (worker.rs:2500)
- `CoreEvent::PendingBackendApproval` — unknown backend kind:24010
- `CoreEvent::ReportUpsert(Report)` — kind:30023 (worker.rs:2668)

**NOT emitted on kind:0 arrivals.** Worker only sends an internal
`DataChange::NoteKeys` (worker.rs:2548). No `CoreEvent::AgentConfigChanged`
exists.

### HIGH — iOS is blind to live agent-config (kind:0) updates

`ios-app/Sources/TenexMVP/CoreManager/TenexCoreManager+Callbacks.swift`
listens for `Message` / `ProjectStatus` / `ReportUpsert` only. There is no
listener for kind:0 changes. iOS only sees a fresh config on:
- App launch / `manualRefresh()` (`+Fetch.swift:48`)
- Pull-to-refresh (`fetchData()` line 7)

So when an agent rotates its model server-side, iOS UI stays stale until
the user manually refreshes. Worksheet Decision 3 chose option D
("event-driven reactive UI updates when new 34011 events arrive") and
acceptance "TUI and iOS use the same core event semantics" was marked
`[x]`, but iOS does not.

Fix: add a `CoreEvent::AgentConfigChanged { agent_pubkey, config }` (or
piggyback on a generic notification), wire the worker to emit it on
kind:0 metadata for tracked agents, listen on iOS.

### MED — TUI relies on 50ms polling instead of dedicated kind:0 channel

`crates/tenex-tui/src/runtime.rs:61, 289` — TUI ticks every 50 ms and
calls `check_for_data_updates()` plus `process_note_keys()`. This works
for TUI but is the reason iOS misses events (no shared notification path).

### MED — Ephemeral subscriptions have no replay

`crates/tenex-core/src/store/events.rs:14-15` — kind:24010 and 24011 are
ephemeral and not persisted. Subscriptions have no `since` filter, so on
reconnect any event fired during the gap is lost. Acceptable for status
heartbeats but worth confirming the failure mode is intentional.

### MED — State cache may stale per-agent config

`state_cache.rs:11, 154` — cache validated by timestamp only; agent
configs received after a save aren't reflected until full rebuild or
7-day TTL. Reproduces with: change agent config server-side, force
relaunch with cached state, observe stale config.

### LOW — Unbounded iOS profile-picture cache

`TenexCoreManager.swift:41-82` — caches profile pictures indefinitely; no
invalidation. Memory grows over time.

---

## C7 — Duplicate models / parser drift `[mostly clean]`

### LOW — Swift parses kind:4199 directly

`ios-app/Sources/TenexMVP/Bunker/BunkerSignPreviewModel.swift:28-82`
manually extracts title/role/description/version/d-tag/instructions/
content/use-criteria/tools/mcp tags from a kind:4199 event. This is the
**only** event-parsing in Swift outside auto-generated bindings — but it
violates the architecture intent that all parsing routes through tenex-
core via FFI. Add an FFI helper `previewAgentDefinition(eventJson) ->
AgentDefinition` and migrate the bunker preview UI to it.

### LOW — `mcp` / `mcps` / `mcpServers` / `mcpAccess` naming

- Rust event tag: `["mcp", ...]`
- Rust struct fields: `active_mcps`, `mcps` (`agent_config.rs`)
- Swift FFI surface: `mcpServers`
- `agent_config.rs:23` doc-comment uses `mcpAccess`

Pick one canonical term in code/comments and stick to it. Rust→Swift
case mapping is fine; the doc-string `mcpAccess` is the outlier.

### Confirmed clean

- `AgentConfig::from_value` and `from_note` (kind:0): converge on the same
  tag-extraction logic via shared helpers.
- `ProjectStatus::from_value` / `from_note` / `from_tags` (kind:24010):
  converge on `from_tags`. The `"agent" => {}` no-op at line 133 is
  correct (24010 must not seed roster).
- `AgentConfig` vs `InstalledAgent`: overlap is intentional.
- `ProjectAgent` vs `InstalledAgent`: overlap is intentional.

---

## C8 — Docs vs reality `[doc-only, mixed]`

### MED — README.md describes a `tenex-tui --server` mode that doesn't exist

`README.md` and `docs/OPENAI_API_SERVER.md` and `docs/elevenlabs_integration.md`
all describe `tenex-tui --server` / `tenex-tui --server --bind 0.0.0.0:…`.

Reality: `tenex-tui` only accepts `--nsec` and `--relay`. The HTTP server
lives in `tenex-cli` behind `--http`. Users following these docs hit
"unknown flag" errors. Three doc files affected.

Fix: either delete these docs (server mode was never on tenex-tui), or
rewrite them to describe `tenex-cli --http`.

### MED — `AGENTS.md` claims `daemon` is a subcommand; code uses `--daemon` flag

`AGENTS.md:27` says `cargo run -p tenex-cli -- daemon`. `crates/tenex-cli/
src/main.rs:276` defines `--daemon` as a flag. The worksheet's Decision 6
chose "Add a `daemon` subcommand for ergonomic compatibility" with
acceptance criterion "AGENTS.md, README, crate docs, and scripts agree" —
but AGENTS.md was changed and the code wasn't, so docs/code disagree
again, just in the opposite direction from before.

Fix: either implement the `daemon` subcommand (small clap change) OR
revert `AGENTS.md` to use `--daemon`.

### LOW — Several docs/folders look historical

Recommend review for archival/deletion:
- `docs/agent-identity-config-implementation-decisions.md` (stale per C1)
- `docs/technical-debt-mixup-worksheet.md` (stale per C1)
- `docs/TODO_UI_MOCKUPS.md`
- `docs/mockups/` HTML files (delegation-tree-recipient-chain-mock.html,
  home-view-mock.html)
- `docs/plans/` 2026-04-08-dated docs
- `Plans/ok-so-the-backend-clever-pebble.md` (already self-titled obsolete)

### LOW — Empty examples directory

`crates/tenex-cli/examples/` is empty. Either remove or add at least one
example.

### LOW — `crates/tenex-core/examples/probe_profile.rs` un-CI'd

Example present but not verified to compile. Add to CI:
`cargo build --example probe_profile`.

---

## Summary — recommended actions, ranked

### Must fix (HIGH)

1. **C3-HIGH**: `cli/daemon.rs:2104` `find_agent_in_project` — switch RPC
   lookup from name-equality to pubkey, or error on duplicate-name
   collisions.
2. **C4-HIGH**: iOS clobbers MCP servers on save — either expose MCP UI
   on iOS/Mac, or stop sending the field when no UI exists.
3. **C6-HIGH**: iOS missing kind:0 (agent config) live-update path —
   add a `CoreEvent::AgentConfigChanged` and wire iOS callbacks.

### Should fix (MED)

4. **C2-MED**: Decide nudge fate — delete entirely or mark deferred.
5. **C4-MED**: TUI add `tools[]` selection (or document deliberate omission).
6. **C4-MED**: CLI re-read latest kind:0 before partial-save.
7. **C5-MED**: Pick one error-handling convention across `*_api.rs` files.
8. **C5-MED**: Pick fire-and-forget vs await policy for mutation FFI.
9. **C6-MED**: Confirm ephemeral-subscription replay gap is intentional.
10. **C6-MED**: Invalidate state cache on kind:0 arrival.
11. **C8-MED**: Fix `tenex-tui --server` references (either delete or
    rewrite for tenex-cli).
12. **C8-MED**: Reconcile `daemon` subcommand vs `--daemon` flag in
    docs+code.

### Nice to have (LOW)

13. **C1**: Rewrite AGENT_ARCHITECTURE.md, archive worksheet/decisions docs.
14. **C3-MED**: Roster display-name fallback comment.
15. **C5-LOW**: Unify timeout durations / async-bridging policy / FFI
    rustdoc / cross-domain naming.
16. **C6-LOW**: Bound iOS profile-picture cache.
17. **C7-LOW**: Add FFI helper for kind:4199 preview parsing in Swift.
18. **C7-LOW**: Pick one canonical term for "mcp" / "mcps" / "mcp_servers".
19. **C8-LOW**: Archive historical docs.
20. **C8-LOW**: Add `probe_profile` example to CI; remove empty
    `tenex-cli/examples`.
