# Chat View Conversation Sidebar Design

## Overview

Add a persistent right-side sidebar to the chat view displaying conversation-specific content: todo lists and metadata summary.

## Layout & Position

- **Location:** Right side of chat view, only visible when viewing a conversation
- **Width:** 42 characters (fixed, matches dashboard sidebar)
- **Height:** Full height of chat area
- **Background:** `BG_SIDEBAR` - `Color::Rgb(12, 12, 12)`

```
┌─────────────────────────────────┬──────────────────────────────────────────┐
│                                 │  TODOS                                   │
│                                 │  ○ Pending task                          │
│       Chat Messages             │  ● In progress task                      │
│                                 │  ✓ Completed task                        │
│                                 ├──────────────────────────────────────────┤
│                                 │  METADATA                                │
│                                 │  Title: Fix TENEX Tool...                │
│                                 │  Status: Completed                       │
│                                 │  Summary: The investigation...           │
│                                 │  Tags: api, toolset                      │
└─────────────────────────────────┴──────────────────────────────────────────┘
```

## Todos Section

**Header:** "TODOS" in muted text with separator line

**Todo items (always expanded):**
- Status icons:
  - `○` (empty circle) - pending, `TEXT_MUTED` gray
  - `◐` (half circle) - in_progress, `ACCENT_WARNING` orange
  - `✓` (checkmark) - completed/done, `ACCENT_SUCCESS` green
- Todo text truncated/wrapped to fit width
- Active (in_progress) item highlighted

**Progress indicator:** "3/5 completed" or ASCII bar `[████░░░░░░] 40%`

**Empty state:** "No active tasks" in muted text

**Data source:** Parse `tool` and `tool-args` tags from conversation NDKEvents (same logic as Svelte's `aggregateTodoState()`)

## Metadata Section

**Header:** "METADATA" in muted text with separator line

**Fields:**
- **title** - Conversation title, can wrap
- **status-label** - Color coded:
  - Completed → `ACCENT_SUCCESS` green
  - In Progress → `ACCENT_WARNING` orange
  - Other → `TEXT_MUTED` gray
- **status-current-activity** - Current state description
- **summary** - Truncated to 3-4 lines with "..."
- **tags** - Inline: `api · toolset · service`

**Layout:**
```
─ METADATA ─────────────────────────────
title  Fix TENEX Tool Wrapping for
       Claude Provider
status Completed
       Fix applied; build passes...

summary
The investigation identified that
TENEX tools were failing because...

tags   api · toolset · service
```

**Empty state:** "No metadata" in muted text

**Data source:** Most recent metadata event in conversation

## Implementation

### Files to modify/create

1. **`crates/tenex-tui/src/ui/views/chat.rs`**
   - Add sidebar to chat view layout
   - Split horizontally: chat content + sidebar

2. **`crates/tenex-tui/src/ui/views/chat_sidebar.rs`** (new)
   - `render_chat_sidebar()` - Main entry point
   - `render_todos_section()` - Todo list rendering
   - `render_metadata_section()` - Metadata fields rendering

3. **`crates/tenex-tui/src/ui/views/todo_aggregator.rs`** (new)
   - Parse `tool` and `tool-args` tags from events
   - Support `todo_write` format (backend standard)
   - Return aggregated todo state

### Data flow

```
Conversation messages (NDKEvents)
    ↓
todo_aggregator::aggregate_todo_state()
    ↓
chat_sidebar::render_todos_section()

Conversation metadata event
    ↓
chat_sidebar::render_metadata_section()
```

No new application state needed - sidebar derives everything from existing conversation data.
