# Reports Tab Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a Reports tab to the TUI home view with document grouping, report detail view in a tab, and "Open Chat" action — matching iOS feature parity.

**Architecture:** The core already has `ReportsStore` with full report loading/storage. We add `HomeTab::Reports` and `TabContentType::Report` variants, a new `reports.rs` view file for rendering, and input handling that follows the exact patterns used by existing tabs (Conversations, ActiveWork, TTS).

**Tech Stack:** Rust, ratatui, pulldown-cmark (via existing `ui::markdown::render_markdown`)

---

### Task 1: Add HomeTab::Reports variant and tab cycling

**Files:**
- Modify: `crates/tenex-tui/src/ui/app.rs:146-152` (HomeTab enum)
- Modify: `crates/tenex-tui/src/input/view_handlers.rs:186-202` (tab cycling)

**Step 1: Add Reports variant to HomeTab enum**

In `crates/tenex-tui/src/ui/app.rs`, change the `HomeTab` enum from:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HomeTab {
    Conversations,
    Inbox,
    ActiveWork,
    Stats,
}
```

to:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HomeTab {
    Conversations,
    Inbox,
    ActiveWork,
    Reports,
    Stats,
}
```

**Step 2: Update tab cycling in view_handlers.rs**

In `crates/tenex-tui/src/input/view_handlers.rs`, update the `NextHomeTab` and `PrevHomeTab` match arms:

```rust
HotkeyId::NextHomeTab => {
    app.home_panel_focus = match app.home_panel_focus {
        HomeTab::Conversations => HomeTab::Inbox,
        HomeTab::Inbox => HomeTab::ActiveWork,
        HomeTab::ActiveWork => HomeTab::Reports,
        HomeTab::Reports => HomeTab::Stats,
        HomeTab::Stats => HomeTab::Conversations,
    };
    return Ok(());
}
HotkeyId::PrevHomeTab => {
    app.home_panel_focus = match app.home_panel_focus {
        HomeTab::Conversations => HomeTab::Stats,
        HomeTab::Inbox => HomeTab::Conversations,
        HomeTab::ActiveWork => HomeTab::Inbox,
        HomeTab::Reports => HomeTab::ActiveWork,
        HomeTab::Stats => HomeTab::Reports,
    };
    return Ok(());
}
```

**Step 3: Fix all exhaustive match statements that match on HomeTab**

Search for all `HomeTab::Stats =>` patterns and add `HomeTab::Reports =>` arms. Key locations:

- `crates/tenex-tui/src/input/view_handlers.rs` — `get_thread_id_at_index()` (~line 108): add `HomeTab::Reports => None,`
- `crates/tenex-tui/src/input/view_handlers.rs` — Down key max calc (~line 281): add `HomeTab::Reports => { ... }` (we'll refine in Task 4)
- `crates/tenex-tui/src/input/view_handlers.rs` — Space key sidebar clamping (~line 322): add `HomeTab::Reports => 0,`
- `crates/tenex-tui/src/ui/views/home/mod.rs` — `render_home` content dispatch (~line 67): add `HomeTab::Reports => content::render_reports(f, app, padded_content),`
- `crates/tenex-tui/src/ui/views/home/mod.rs` — tab header list (~line 257): add `(HomeTab::Reports, "Reports"),`

For now, add placeholder `HomeTab::Reports => 0` or `HomeTab::Reports => {}` for match arms that will be fleshed out in later tasks. The goal is to compile.

**Step 4: Build and verify it compiles**

Run: `cargo build -p tenex-tui 2>&1 | head -40`

Fix any remaining exhaustive match errors. The compiler will tell you every location that needs a `HomeTab::Reports` arm.

**Step 5: Commit**

```
feat: add HomeTab::Reports variant with tab cycling
```

---

### Task 2: Add TabContentType::Report and OpenTab::for_report

**Files:**
- Modify: `crates/tenex-tui/src/ui/state.rs:128-134` (TabContentType enum)
- Modify: `crates/tenex-tui/src/ui/state.rs:275-315` (OpenTab struct)
- Modify: `crates/tenex-tui/src/ui/state.rs:432+` (TabManager)

**Step 1: Add Report variant to TabContentType**

In `crates/tenex-tui/src/ui/state.rs`:

```rust
pub enum TabContentType {
    #[default]
    Conversation,
    TTSControl,
    Report,
}
```

**Step 2: Add report_slug field to OpenTab and constructor**

Add `report_slug: Option<String>` field to `OpenTab` struct. Add it as `None` in all existing constructors (`for_thread`, `draft`, `tts_control`).

Add new constructor:

```rust
/// Create a tab for viewing a report
pub fn for_report(slug: String, title: String, project_a_tag: String) -> Self {
    Self {
        content_type: TabContentType::Report,
        thread_id: String::new(),
        thread_title: title,
        project_a_tag,
        has_unread: false,
        waiting_for_user: false,
        is_agent_working: false,
        draft_id: None,
        navigation_stack: Vec::new(),
        message_history: TabMessageHistory::default(),
        chat_search: ChatSearchState::default(),
        selected_skill_ids: Vec::new(),
        editor: TextEditor::new(),
        reference_conversation_id: None,
        fork_message_id: None,
        tts_state: None,
        report_slug: Some(slug),
    }
}
```

**Step 3: Add open_report method to TabManager**

Follow the `open_tts_control` pattern:

```rust
/// Open a report tab (or switch to it if already open).
pub fn open_report(&mut self, slug: String, title: String, project_a_tag: String) -> usize {
    // Check if this report is already open
    if let Some(idx) = self.tabs.iter().position(|t| {
        t.content_type == TabContentType::Report && t.report_slug.as_deref() == Some(&slug)
    }) {
        self.push_history(idx);
        self.push_view_history(ViewLocation::Tab(idx));
        self.active_index = idx;
        return idx;
    }

    let tab = OpenTab::for_report(slug, title, project_a_tag);
    self.evict_if_needed(false);
    self.tabs.push(tab);
    let new_idx = self.tabs.len() - 1;
    self.push_history(new_idx);
    self.push_view_history(ViewLocation::Tab(new_idx));
    self.active_index = new_idx;
    new_idx
}
```

**Step 4: Add Report dispatch to chat view layout**

In `crates/tenex-tui/src/ui/views/chat/layout.rs:37-45`, extend the match:

```rust
match content_type {
    TabContentType::TTSControl => {
        render_tts_tab_layout(f, app, area);
        return;
    }
    TabContentType::Report => {
        render_report_tab_layout(f, app, area);
        return;
    }
    TabContentType::Conversation => {}
}
```

Add a stub `render_report_tab_layout` at the bottom of the file (we'll flesh it out in Task 5):

```rust
fn render_report_tab_layout(f: &mut Frame, app: &mut App, area: Rect) {
    // Stub — will be implemented in Task 5
    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(layout::TAB_BAR_HEIGHT),
        Constraint::Length(layout::STATUSBAR_HEIGHT),
    ])
    .split(area);

    let placeholder = Paragraph::new("Report view (TODO)")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(placeholder, chunks[0]);
    render_tab_bar(f, app, chunks[1]);
    let (rt, ha, ac) = app.data_store.borrow_mut().get_statusbar_runtime_ms();
    let audio = app.audio_player.is_playing();
    render_statusbar(f, chunks[2], app.current_notification(), rt, ha, ac, app.wave_offset(), audio);
}
```

**Step 5: Add Report dispatch to chat input handler**

In `crates/tenex-tui/src/input/view_handlers.rs:1077-1084`, extend the match:

```rust
match content_type {
    TabContentType::TTSControl => {
        return handle_tts_control_key(app, key);
    }
    TabContentType::Report => {
        return handle_report_tab_key(app, key);
    }
    TabContentType::Conversation => {}
}
```

Add a stub handler function:

```rust
fn handle_report_tab_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.close_current_tab();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.scroll_offset = app.scroll_offset.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            app.scroll_offset = app.scroll_offset.saturating_add(1);
        }
        _ => {}
    }
    Ok(true)
}
```

**Step 6: Build and verify**

Run: `cargo build -p tenex-tui 2>&1 | head -40`

Fix any exhaustive match errors on `TabContentType`.

**Step 7: Commit**

```
feat: add TabContentType::Report and OpenTab::for_report
```

---

### Task 3: Create reports list view with document grouping

**Files:**
- Create: `crates/tenex-tui/src/ui/views/reports.rs`
- Modify: `crates/tenex-tui/src/ui/views/mod.rs` (register module)
- Modify: `crates/tenex-tui/src/ui/views/home/content.rs` (add render_reports function)

**Step 1: Create the reports view module**

Create `crates/tenex-tui/src/ui/views/reports.rs` with the report entry types and list renderer:

```rust
use crate::ui::format::{format_relative_time, truncate_with_ellipsis};
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::collections::HashMap;
use tenex_core::models::Report;

/// A display entry in the reports list — either a single report or a document group.
#[derive(Debug, Clone)]
pub enum ReportEntry {
    Single(Report),
    Group {
        project_a_tag: String,
        document: String,
        reports: Vec<Report>,
    },
}

impl ReportEntry {
    pub fn most_recent_created_at(&self) -> u64 {
        match self {
            ReportEntry::Single(r) => r.created_at,
            ReportEntry::Group { reports, .. } => {
                reports.iter().map(|r| r.created_at).max().unwrap_or(0)
            }
        }
    }

    /// Unique key for this entry (used for expanded group tracking)
    pub fn group_key(&self) -> Option<String> {
        match self {
            ReportEntry::Single(_) => None,
            ReportEntry::Group { project_a_tag, document, .. } => {
                Some(format!("{}|{}", project_a_tag, document))
            }
        }
    }
}

/// Build report entries with document grouping, filtered by visible projects.
pub fn build_report_entries(app: &App) -> Vec<ReportEntry> {
    let store = app.data_store.borrow();
    let all_reports = store.reports.get_reports();

    // Filter by visible projects
    let reports: Vec<&Report> = if app.visible_projects.is_empty() {
        all_reports
    } else {
        all_reports
            .into_iter()
            .filter(|r| app.visible_projects.contains(&r.project_a_tag))
            .collect()
    };

    // Count reports per (project_a_tag, document) for grouping
    let mut group_counts: HashMap<String, usize> = HashMap::new();
    for r in &reports {
        if !r.document.is_empty() {
            let key = format!("{}|{}", r.project_a_tag, r.document);
            *group_counts.entry(key).or_default() += 1;
        }
    }

    // Build entries
    let mut groups: HashMap<String, (String, String, Vec<Report>)> = HashMap::new();
    let mut singles: Vec<Report> = Vec::new();

    for r in &reports {
        let key = format!("{}|{}", r.project_a_tag, r.document);
        if !r.document.is_empty() && group_counts.get(&key).copied().unwrap_or(0) > 1 {
            let entry = groups
                .entry(key)
                .or_insert_with(|| (r.project_a_tag.clone(), r.document.clone(), Vec::new()));
            entry.2.push((*r).clone());
        } else {
            singles.push((*r).clone());
        }
    }

    let mut entries: Vec<ReportEntry> = Vec::new();

    for (_, (project_a_tag, document, mut reports)) in groups {
        reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        entries.push(ReportEntry::Group {
            project_a_tag,
            document,
            reports,
        });
    }

    for r in singles {
        entries.push(ReportEntry::Single(r));
    }

    entries.sort_by(|a, b| b.most_recent_created_at().cmp(&a.most_recent_created_at()));
    entries
}

/// Build a flat list of visible items (accounting for expanded groups).
/// Returns tuples of (entry_index, Option<sub_index>) where sub_index is Some for expanded group children.
pub fn build_visible_items(entries: &[ReportEntry], expanded_groups: &std::collections::HashSet<String>) -> Vec<(usize, Option<usize>)> {
    let mut items = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        items.push((i, None)); // The entry row itself
        if let ReportEntry::Group { reports, .. } = entry {
            if let Some(key) = entry.group_key() {
                if expanded_groups.contains(&key) {
                    for (j, _) in reports.iter().enumerate() {
                        items.push((i, Some(j)));
                    }
                }
            }
        }
    }
    items
}

/// Render the reports list in the home content area.
pub fn render_reports_list(f: &mut Frame, app: &App, area: Rect) {
    let entries = build_report_entries(app);

    if entries.is_empty() {
        let empty = Paragraph::new("No reports")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty, area);
        return;
    }

    let visible = build_visible_items(&entries, &app.reports_expanded_groups);
    let selected = app.current_selection();
    let store = app.data_store.borrow();

    let mut y = 0u16;
    for (vi, &(entry_idx, sub_idx)) in visible.iter().enumerate() {
        if y >= area.height {
            break;
        }

        let is_selected = vi == selected;
        let entry = &entries[entry_idx];

        match (entry, sub_idx) {
            (ReportEntry::Single(report), None) => {
                let lines_needed = 3u16; // title + summary + meta
                if y + lines_needed > area.height {
                    break;
                }
                let item_area = Rect::new(area.x, area.y + y, area.width, lines_needed);
                render_single_report_row(f, report, &store, is_selected, item_area);
                y += lines_needed;
            }
            (ReportEntry::Group { document, project_a_tag, reports }, None) => {
                let is_expanded = entry.group_key()
                    .map(|k| app.reports_expanded_groups.contains(&k))
                    .unwrap_or(false);
                let lines_needed = 2u16; // folder + count
                if y + lines_needed > area.height {
                    break;
                }
                let item_area = Rect::new(area.x, area.y + y, area.width, lines_needed);
                render_group_row(f, document, project_a_tag, reports.len(), is_expanded, &store, is_selected, item_area);
                y += lines_needed;
            }
            (ReportEntry::Group { reports, .. }, Some(j)) => {
                if let Some(report) = reports.get(*j) {
                    let lines_needed = 3u16;
                    if y + lines_needed > area.height {
                        break;
                    }
                    let item_area = Rect::new(area.x + 2, area.y + y, area.width.saturating_sub(2), lines_needed);
                    render_single_report_row(f, report, &store, is_selected, item_area);
                    y += lines_needed;
                }
            }
            _ => {}
        }
    }
}

fn render_single_report_row(
    f: &mut Frame,
    report: &Report,
    store: &std::cell::Ref<tenex_core::store::AppDataStore>,
    is_selected: bool,
    area: Rect,
) {
    let project_name = store.get_project_name(&report.project_a_tag);
    let title = if report.title.is_empty() { "Untitled" } else { &report.title };
    let title_max = (area.width as usize).saturating_sub(project_name.len() + 4).max(10);
    let truncated_title = truncate_with_ellipsis(title, title_max);

    let title_style = if is_selected {
        Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    // Line 1: Title + project badge
    let line1 = Line::from(vec![
        Span::styled(truncated_title, title_style),
        Span::raw("  "),
        Span::styled(
            project_name,
            Style::default().fg(theme::project_color(&report.project_a_tag)),
        ),
    ]);

    // Line 2: Summary
    let summary_max = area.width as usize;
    let summary = truncate_with_ellipsis(
        if report.summary.is_empty() { &report.content } else { &report.summary },
        summary_max,
    );
    let line2 = Line::from(Span::styled(summary, Style::default().fg(theme::TEXT_MUTED)));

    // Line 3: reading time + relative time
    let reading_time = if report.reading_time_mins == 1 {
        "1 min read".to_string()
    } else {
        format!("{} min read", report.reading_time_mins)
    };
    let time_ago = format_relative_time(report.created_at);
    let line3 = Line::from(Span::styled(
        format!("{} · {}", reading_time, time_ago),
        Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM),
    ));

    let para = Paragraph::new(vec![line1, line2, line3]);
    if is_selected {
        f.render_widget(para.style(Style::default().bg(theme::BG_SELECTED)), area);
    } else {
        f.render_widget(para, area);
    }
}

fn render_group_row(
    f: &mut Frame,
    document: &str,
    project_a_tag: &str,
    count: usize,
    is_expanded: bool,
    store: &std::cell::Ref<tenex_core::store::AppDataStore>,
    is_selected: bool,
    area: Rect,
) {
    let project_name = store.get_project_name(project_a_tag);
    let prefix = if is_expanded { "[-]" } else { "[+]" };

    let title_style = if is_selected {
        Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    let line1 = Line::from(vec![
        Span::styled(prefix, Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::raw(" "),
        Span::styled(document.to_string(), title_style),
        Span::raw("  "),
        Span::styled(
            project_name,
            Style::default().fg(theme::project_color(project_a_tag)),
        ),
    ]);

    let line2 = Line::from(Span::styled(
        format!("    {} documents", count),
        Style::default().fg(theme::TEXT_MUTED),
    ));

    let para = Paragraph::new(vec![line1, line2]);
    if is_selected {
        f.render_widget(para.style(Style::default().bg(theme::BG_SELECTED)), area);
    } else {
        f.render_widget(para, area);
    }
}
```

**Step 2: Register the module and add render function to home content**

In `crates/tenex-tui/src/ui/views/mod.rs`, add:
```rust
pub mod reports;
```

In `crates/tenex-tui/src/ui/views/home/content.rs`, add a public function that delegates:

```rust
pub(super) fn render_reports(f: &mut Frame, app: &App, area: Rect) {
    super::super::reports::render_reports_list(f, app, area);
}
```

**Step 3: Add reports_expanded_groups field to App**

In `crates/tenex-tui/src/ui/app.rs`, add to the `App` struct (near the other home view fields, ~line 400):

```rust
/// Expanded document groups in Reports tab
pub reports_expanded_groups: HashSet<String>,
```

Initialize it in `App::new()`:
```rust
reports_expanded_groups: HashSet::new(),
```

**Step 4: Wire up home view dispatch**

In `crates/tenex-tui/src/ui/views/home/mod.rs`, the match on `app.home_panel_focus` (~line 67) should now include:

```rust
HomeTab::Reports => content::render_reports(f, app, padded_content),
```

And the tab header list (~line 257):

```rust
let tabs = vec![
    (HomeTab::Conversations, "Conversations"),
    (HomeTab::Inbox, "Inbox"),
    (HomeTab::ActiveWork, "Active"),
    (HomeTab::Reports, "Reports"),
    (HomeTab::Stats, "Stats"),
];
```

**Step 5: Build and verify**

Run: `cargo build -p tenex-tui 2>&1 | head -40`

**Step 6: Commit**

```
feat: add reports list view with document grouping
```

---

### Task 4: Add reports list input handling (navigation, expand/collapse, open)

**Files:**
- Modify: `crates/tenex-tui/src/input/view_handlers.rs`

**Step 1: Add Reports tab handling to home view key handler**

In the `Down` key handler (~line 266), add the Reports max calculation:

```rust
HomeTab::Reports => {
    let entries = crate::ui::views::reports::build_report_entries(app);
    let visible = crate::ui::views::reports::build_visible_items(&entries, &app.reports_expanded_groups);
    visible.len().saturating_sub(1)
}
```

In the `get_thread_id_at_index` function (~line 108), add:
```rust
HomeTab::Reports => None, // Reports are not threads
```

In sidebar Space key clamping (~line 316-323), add Reports:
```rust
HomeTab::Reports => {
    let entries = crate::ui::views::reports::build_report_entries(app);
    let visible = crate::ui::views::reports::build_visible_items(&entries, &app.reports_expanded_groups);
    visible.len().saturating_sub(1)
}
```

**Step 2: Add Enter key handling for Reports tab**

In the `Enter` key handler for the content area (~line 422), add a `HomeTab::Reports` arm:

```rust
HomeTab::Reports => {
    let entries = crate::ui::views::reports::build_report_entries(app);
    let visible = crate::ui::views::reports::build_visible_items(&entries, &app.reports_expanded_groups);

    if let Some(&(entry_idx, sub_idx)) = visible.get(idx) {
        let entry = &entries[entry_idx];
        match (entry, sub_idx) {
            // Single report or expanded group child — open in tab
            (crate::ui::views::reports::ReportEntry::Single(report), None) |
            (crate::ui::views::reports::ReportEntry::Group { .. }, Some(_)) => {
                let report = match (entry, sub_idx) {
                    (crate::ui::views::reports::ReportEntry::Single(r), _) => r.clone(),
                    (crate::ui::views::reports::ReportEntry::Group { reports, .. }, Some(j)) => {
                        reports.get(j).cloned().unwrap_or_else(|| reports[0].clone())
                    }
                    _ => return Ok(()),
                };
                app.tabs.open_report(
                    report.slug.clone(),
                    report.title.clone(),
                    report.project_a_tag.clone(),
                );
                app.scroll_offset = 0;
                app.view = View::Chat;
                app.input_mode = InputMode::Normal;
            }
            // Group header — toggle expand/collapse
            (crate::ui::views::reports::ReportEntry::Group { .. }, None) => {
                if let Some(key) = entry.group_key() {
                    if app.reports_expanded_groups.contains(&key) {
                        app.reports_expanded_groups.remove(&key);
                    } else {
                        app.reports_expanded_groups.insert(key);
                    }
                }
            }
            _ => {}
        }
    }
}
```

**Step 3: Build and verify**

Run: `cargo build -p tenex-tui 2>&1 | head -40`

**Step 4: Commit**

```
feat: add reports list navigation and open-in-tab
```

---

### Task 5: Implement report detail view rendering

**Files:**
- Modify: `crates/tenex-tui/src/ui/views/reports.rs` (add detail renderer)
- Modify: `crates/tenex-tui/src/ui/views/chat/layout.rs` (wire up render_report_tab_layout)

**Step 1: Add report detail renderer to reports.rs**

Add to `crates/tenex-tui/src/ui/views/reports.rs`:

```rust
use crate::ui::markdown::render_markdown;

/// Render report detail content (used inside a tab).
pub fn render_report_detail(f: &mut Frame, app: &App, area: Rect) {
    let slug = app
        .tabs
        .active_tab()
        .and_then(|t| t.report_slug.as_deref())
        .unwrap_or("");

    let report = app.data_store.borrow().reports.get_report(slug).cloned();

    let Some(report) = report else {
        let msg = Paragraph::new("Report not found")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(msg, area);
        return;
    };

    let store = app.data_store.borrow();

    // Build header lines
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Title
    let title = if report.title.is_empty() { "Untitled".to_string() } else { report.title.clone() };
    lines.push(Line::from(Span::styled(
        title,
        Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD),
    )));

    // Summary
    if !report.summary.is_empty() {
        lines.push(Line::from(Span::styled(
            report.summary.clone(),
            Style::default().fg(theme::TEXT_MUTED),
        )));
    }

    // Meta line: reading time + author + date
    let reading_time = if report.reading_time_mins == 1 {
        "1 min read".to_string()
    } else {
        format!("{} min read", report.reading_time_mins)
    };
    let author_name = store.get_profile_name(&report.author);
    let time_ago = format_relative_time(report.created_at);
    let project_name = store.get_project_name(&report.project_a_tag);
    lines.push(Line::from(vec![
        Span::styled(reading_time, Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · by ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(author_name, Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(time_ago, Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(project_name, Style::default().fg(theme::project_color(&report.project_a_tag))),
    ]));

    // Divider
    let divider = "─".repeat(area.width as usize);
    lines.push(Line::from(Span::styled(divider, Style::default().fg(theme::BORDER_INACTIVE))));
    lines.push(Line::from(""));

    // Drop store borrow before render_markdown (it doesn't need it)
    drop(store);

    // Markdown content
    let md_lines = render_markdown(&report.content);
    lines.extend(md_lines);

    // Apply scroll offset
    let scroll = app.scroll_offset;
    let visible_lines: Vec<Line> = lines.into_iter().skip(scroll).collect();

    let para = Paragraph::new(visible_lines)
        .style(Style::default().bg(theme::BG_APP));
    f.render_widget(para, area);
}
```

**Step 2: Replace the stub render_report_tab_layout in chat/layout.rs**

Replace the stub with:

```rust
fn render_report_tab_layout(f: &mut Frame, app: &mut App, area: Rect) {
    use crate::ui::views::reports::render_report_detail;

    // Layout: Header | Content | Help bar | Tab bar | Statusbar
    let chunks = Layout::vertical([
        Constraint::Length(layout::HEADER_HEIGHT_CHAT), // Header
        Constraint::Min(0),                              // Content
        Constraint::Length(1),                            // Help bar
        Constraint::Length(layout::TAB_BAR_HEIGHT),      // Tab bar
        Constraint::Length(layout::STATUSBAR_HEIGHT),    // Statusbar
    ])
    .split(area);

    // Header
    let title = app
        .tabs
        .active_tab()
        .map(|t| t.thread_title.clone())
        .unwrap_or_else(|| "Report".to_string());
    let padding = " ".repeat(layout::CONTENT_PADDING_H as usize);
    let header = Paragraph::new(format!("\n{}{}", padding, title)).style(
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(ratatui::style::Modifier::BOLD),
    );
    f.render_widget(header, chunks[0]);

    // Content with padding
    let padded = layout::with_content_padding(chunks[1]);
    render_report_detail(f, app, padded);

    // Help bar
    let help = Paragraph::new(Line::from(vec![
        Span::styled(" j/k ", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled("scroll  ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" c ", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled("open chat  ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" q/Esc ", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled("close", Style::default().fg(theme::TEXT_MUTED)),
    ]));
    f.render_widget(help, chunks[2]);

    // Tab bar
    render_tab_bar(f, app, chunks[3]);

    // Statusbar
    let (rt, ha, ac) = app.data_store.borrow_mut().get_statusbar_runtime_ms();
    let audio = app.audio_player.is_playing();
    render_statusbar(f, chunks[4], app.current_notification(), rt, ha, ac, app.wave_offset(), audio);
}
```

**Step 3: Build and verify**

Run: `cargo build -p tenex-tui 2>&1 | head -40`

**Step 4: Commit**

```
feat: implement report detail view with markdown rendering
```

---

### Task 6: Implement report tab key handling with "Open Chat" action

**Files:**
- Modify: `crates/tenex-tui/src/input/view_handlers.rs` (flesh out handle_report_tab_key)

**Step 1: Replace the stub handle_report_tab_key**

```rust
fn handle_report_tab_key(app: &mut App, key: KeyEvent) -> Result<bool> {
    let code = key.code;

    match code {
        KeyCode::Esc | KeyCode::Char('q') => {
            app.close_current_tab();
        }
        KeyCode::Up | KeyCode::Char('k') => {
            app.scroll_offset = app.scroll_offset.saturating_sub(1);
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if app.scroll_offset < app.max_scroll_offset {
                app.scroll_offset = app.scroll_offset.saturating_add(1);
            }
        }
        KeyCode::PageUp => {
            app.scroll_offset = app.scroll_offset.saturating_sub(20);
        }
        KeyCode::PageDown => {
            app.scroll_offset = app.scroll_offset.saturating_add(20).min(app.max_scroll_offset);
        }
        KeyCode::Home => {
            app.scroll_offset = 0;
        }
        KeyCode::End => {
            app.scroll_offset = app.max_scroll_offset;
        }
        KeyCode::Char('c') => {
            // Open Chat referencing this report
            let report_info = app.tabs.active_tab().and_then(|t| {
                t.report_slug.as_ref().map(|slug| {
                    let store = app.data_store.borrow();
                    store.reports.get_report(slug).map(|r| {
                        (r.project_a_tag.clone(), r.a_tag())
                    })
                })
            }).flatten();

            if let Some((project_a_tag, report_a_tag)) = report_info {
                let project_name = app.data_store.borrow().get_project_name(&project_a_tag).to_string();
                let tab_idx = app.tabs.open_draft(project_a_tag, project_name);
                if let Some(tab) = app.tabs.tabs_mut().get_mut(tab_idx) {
                    tab.reference_conversation_id = Some(report_a_tag);
                }
                app.view = View::Chat;
                app.input_mode = InputMode::Editing;
            }
        }
        _ => {}
    }
    Ok(true)
}
```

**Step 2: Build and verify**

Run: `cargo build -p tenex-tui 2>&1 | head -40`

**Step 3: Commit**

```
feat: add report tab key handling with Open Chat action
```

---

### Task 7: Final integration pass — fix all remaining compiler errors and edge cases

**Files:**
- Various files as needed

**Step 1: Full build**

Run: `cargo build -p tenex-tui 2>&1`

Fix any remaining compiler errors. Common ones:
- Missing `HomeTab::Reports` arms in any match statements
- Missing `TabContentType::Report` arms
- Import paths for the new `reports` module
- Borrow checker issues with `data_store`

**Step 2: Run tests**

Run: `cargo test -p tenex-tui 2>&1 | tail -20`
Run: `cargo test -p tenex-core 2>&1 | tail -20`

**Step 3: Manual smoke test**

Run: `cargo run -p tenex-tui` and verify:
- Tab cycling includes Reports between Active and Stats
- Reports tab shows list (or "No reports" empty state)
- If reports exist, document grouping works
- Selecting a report opens it in a new tab
- Report detail shows markdown content
- `j`/`k` scrolls, `q`/`Esc` closes tab
- `c` opens a new conversation draft

**Step 4: Commit**

```
feat: complete Reports tab integration
```

---

Plan complete and saved to `docs/plans/2026-04-08-reports-tab-implementation.md`. Two execution options:

**1. Subagent-Driven (this session)** — I dispatch fresh subagent per task, review between tasks, fast iteration

**2. Parallel Session (separate)** — Open new session with executing-plans, batch execution with checkpoints

Which approach?
