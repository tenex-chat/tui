# Archive Conversations Feature

## Overview
Allow users to archive conversations locally (no Nostr event) and toggle visibility.

## Key Bindings
- `x` on selected conversation → quick archive/unarchive (Recent and Inbox tabs)
- `/` in Home view (on selected conversation) → opens ConversationActions modal
- `Ctrl+T, A` → toggles show_archived flag (show/hide archived conversations)

## ConversationActions Modal Options
1. Open conversation (o)
2. Export JSONL (e) - copies all events as JSONL to clipboard
3. Toggle archive (a)

## Storage
- `PreferencesStorage.archived_thread_ids: HashSet<String>` - persisted locally
- `App.show_archived: bool` - runtime toggle (default: false)

## Filtering
- Recent/Inbox views filter out archived threads unless show_archived is true
- Archived conversations disappear from list when archived (unless show_archived is on)

## Visual Indicators
- Archived threads show `[archived]` tag (dim) after title when visible
- Header shows `[showing archived]` indicator when show_archived mode is active

## Export JSONL
- Same format as Svelte web client
- Exports all message events in thread as JSON lines (one raw event per line)
- Copies to clipboard
