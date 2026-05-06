# Reports Tab for TUI Client

## Summary

Add a Reports tab to the TUI home view, matching iOS feature parity. Reports (kind:30023 articles) are already loaded and stored by `tenex-core` — the TUI just needs UI to display them.

## Tab Position

`Conversations | Inbox | Active | Reports | Stats`

New `HomeTab::Reports` variant between `ActiveWork` and `Stats`.

## Report List View

Renders in the main content area when Reports tab is focused. Shows reports filtered by visible projects, sorted by most recent `created_at`.

### Document Grouping

Matches iOS logic: reports sharing the same `(project_a_tag, document)` tag with 2+ entries are grouped into a folder row. Singles show individually.

**Single report row:**
```
  Report Title Here                              ProjectName
  Summary text goes here, truncated to fit...
  3 min read · 2 days ago
```

**Group row (folder):**
```
  [+] document-tag-name                          ProjectName
      3 documents
```

Expanded group shows individual reports indented beneath.

### Navigation

- `j`/`k` or arrows: move through list
- `Enter` on single report: opens in a new tab
- `Enter` on group: toggles expand/collapse inline
- Visible projects sidebar filter applies

## Report Detail View (Tab)

Opens as a new tab via `TabContentType::Report`. Tab title shows the report title.

### Layout

```
  Report Title
  Summary text if present
  3 min read · by AuthorName · 2 days ago
  ─────────────────────────────────────────

  [Full markdown content, scrollable]
```

### Key Bindings

- `j`/`k` or arrows: scroll content
- `c`: Open Chat — create new conversation referencing report via `reference_report_a_tag` (30023:author:slug)
- `q` or `Esc`: close tab, return to Reports list

## State Changes

### `HomeTab` enum (app.rs)
Add `Reports` variant.

### `TabContentType` enum (state.rs)
Add `Report` variant.

### `OpenTab` (state.rs)
Add `report_slug: Option<String>` field and `OpenTab::for_report()` constructor.

### `App` struct (app.rs)
Add:
- `reports_list_index: usize`
- `reports_expanded_groups: HashSet<String>` (keyed by `"project_a_tag|document"`)
- `reports_scroll_offset: usize`

### New files
- `crates/tenex-tui/src/ui/views/reports.rs` — list + detail rendering
- `crates/tenex-tui/src/input/modal_handlers/reports.rs` — input handling for reports list

## Data Flow

No new data loading. Reads from `app.data_store.borrow().reports`:
- `get_reports()` for all reports
- `get_reports_by_project(a_tag)` for filtered views
- `get_report(slug)` for detail view

Document grouping computed at render time.

## Open Chat Action

Pressing `c` in report detail creates a new conversation tab with `reference_report_a_tag` set to the report's a-tag (`30023:author:slug`). This mirrors the iOS `referenceReportATag` in `MessageComposerView`.
