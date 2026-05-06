---
task: Fix saved kind:1 events not appearing in TUI
slug: 20260430-000000_fix-saved-kind1-events-not-showing
effort: standard
phase: complete
progress: 12/12
mode: interactive
started: 2026-04-30T00:00:00Z
updated: 2026-04-30T00:00:00Z
---

## Context

Kind:1 Nostr events from unknown pubkeys (not the project owner) that a-tag a project never show up in the TUI as threads. Root cause confirmed: when nostr-sdk's `check_id()` finds the event already in nostrdb (returns `Saved`), it does NOT emit `RelayPoolNotification::Event` — only `RelayPoolNotification::Message { RelayMessage::Event }`. The worker's `RelayPoolNotification::Message` handler ignores `RelayMessage::Event`, so `handle_incoming_event()` is never called and the NDB subscription never fires. The fix adds `DataChange::NoteKeys(Vec<u64>)` and re-routes saved events through the note-key processing path.

### Risks

- Exhaustive match on `DataChange` in `ffi/mod.rs` will break compilation without new arm
- `check_for_data_updates` needs access to `ndb` and `core_handle` (both available on App)
- `process_note_keys` returns `CoreEvent`s that drive UI actions — we must handle or drop them appropriately
- Double-processing: new events hit both `RelayPoolNotification::Event` AND `RelayPoolNotification::Message` — idempotency in `handle_thread_event` prevents duplication

## Criteria

- [x] ISC-1: `DataChange::NoteKeys(Vec<u64>)` variant added to enum in worker.rs
- [x] ISC-2: `RelayPoolNotification::Message { RelayMessage::Event }` handled in worker
- [x] ISC-3: Worker looks up note_key from nostrdb for received event
- [x] ISC-4: Worker sends `DataChange::NoteKeys` only for non-ephemeral events
- [x] ISC-5: `DataChange::NoteKeys` arm added to `ffi/mod.rs` exhaustive match
- [x] ISC-6: `DataChange::NoteKeys` arm added to `app.rs` `check_for_data_updates`
- [x] ISC-7: `check_for_data_updates` calls `process_note_keys` with retrieved keys
- [x] ISC-8: `CoreEvent::Message` returned from process_note_keys triggers `mark_tab_unread`
- [x] ISC-9: Codebase compiles without errors after all changes
- [x] ISC-10: No duplicate thread entries (idempotency already guaranteed by handle_thread_event)
- [x] ISC-11: `reconcile_threads_from_ndb` added to `app_data_store.rs` scanning all project kind:1s
- [x] ISC-12: `reconcile_threads_from_ndb` called from `try_load_from_cache` after incremental catchup

## Decisions

## Verification

- ISC-1–4: DataChange::NoteKeys variant confirmed at worker.rs:1022; RelayMessage::Event handler at 2381-2393; ephemeral guard at 2385; NDB lookup + send verified.
- ISC-5: ffi/mod.rs:857 DataChange::NoteKeys(_) no-op arm present.
- ISC-6–8: app.rs:1746-1763 handler calls process_note_key_ids, handles CoreEvent::Message → mark_tab_unread.
- ISC-9: cargo check --workspace → Finished, 0 errors.
- ISC-10: handle_thread_event idempotency unchanged (pre-existing check at line 1742).
