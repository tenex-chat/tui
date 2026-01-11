# Reports Tab Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a Reports tab to the TUI with feature parity to the Svelte docs section - list view with search, document viewer with markdown/versions/diff, and document-scoped discussion threads.

**Architecture:** Reports are Nostr kind:30023 (NDKArticle) events with version tracking via d-tag slugs. The Reports tab appears after Inbox in the Home view header. Document viewing happens in a modal with optional threads sidebar. Threads are kind:1 events that #a-tag the document.

**Tech Stack:** Rust, ratatui, nostrdb, pulldown-cmark (existing markdown renderer)

---

## Task 1: Add Report Model

**Files:**
- Create: `crates/tenex-core/src/models/report.rs`
- Modify: `crates/tenex-core/src/models/mod.rs`

**Step 1: Create the Report model file**

```rust
// crates/tenex-core/src/models/report.rs
use nostrdb::Note;

/// A report/document (kind:30023 - Article)
#[derive(Debug, Clone)]
pub struct Report {
    /// Event ID (hex)
    pub id: String,
    /// d-tag slug (for version tracking - same slug = same document, different versions)
    pub slug: String,
    /// Project a-tag this report belongs to
    pub project_a_tag: String,
    /// Author pubkey (hex)
    pub author: String,
    /// Document title (from title tag)
    pub title: String,
    /// Summary (from summary tag, or first 160 chars of content)
    pub summary: String,
    /// Full markdown content
    pub content: String,
    /// Hashtags (t-tags)
    pub hashtags: Vec<String>,
    /// Creation timestamp
    pub created_at: u64,
    /// Calculated reading time in minutes (content length / 200 words per minute)
    pub reading_time_mins: u8,
}

impl Report {
    /// Parse a Report from a nostrdb Note (kind:30023)
    pub fn from_note(note: &Note) -> Option<Self> {
        let id = hex::encode(note.id());
        let author = hex::encode(note.pubkey());
        let content = note.content().to_string();
        let created_at = note.created_at();

        let mut slug = String::new();
        let mut project_a_tag = String::new();
        let mut title = String::new();
        let mut summary = String::new();
        let mut hashtags = Vec::new();

        for tag in note.tags() {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            match tag_name {
                Some("d") => {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        slug = val.to_string();
                    }
                }
                Some("a") => {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        project_a_tag = val.to_string();
                    }
                }
                Some("title") => {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        title = val.to_string();
                    }
                }
                Some("summary") => {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        summary = val.to_string();
                    }
                }
                Some("t") => {
                    if let Some(val) = tag.get(1).and_then(|t| t.variant().str()) {
                        hashtags.push(val.to_string());
                    }
                }
                _ => {}
            }
        }

        // Require slug and project_a_tag
        if slug.is_empty() || project_a_tag.is_empty() {
            return None;
        }

        // Default title from first line of content
        if title.is_empty() {
            title = content.lines().next().unwrap_or("Untitled").to_string();
        }

        // Default summary from content
        if summary.is_empty() {
            summary = content.chars().take(160).collect();
        }

        // Calculate reading time (average 200 words per minute)
        let word_count = content.split_whitespace().count();
        let reading_time_mins = ((word_count as f32 / 200.0).ceil() as u8).max(1);

        Some(Self {
            id,
            slug,
            project_a_tag,
            author,
            title,
            summary,
            content,
            hashtags,
            created_at,
            reading_time_mins,
        })
    }

    /// Get the a-tag for this report (for thread references)
    pub fn a_tag(&self) -> String {
        format!("30023:{}:{}", self.author, self.slug)
    }
}
```

**Step 2: Add to mod.rs exports**

In `crates/tenex-core/src/models/mod.rs`, add:

```rust
pub mod report;
pub use report::Report;
```

**Step 3: Commit**

```bash
git add crates/tenex-core/src/models/report.rs crates/tenex-core/src/models/mod.rs
git commit -m "feat(models): add Report model for kind:30023 articles"
```

---

## Task 2: Add Reports to AppDataStore

**Files:**
- Modify: `crates/tenex-core/src/store/app_data_store.rs`

**Step 1: Add reports fields to AppDataStore struct**

After line 33 (after `nudges` field), add:

```rust
    // Reports - kind:30023 events (articles/documents)
    // Key: report slug (d-tag) -> latest version
    pub reports: HashMap<String, Report>,
    // All versions by slug (for version history)
    pub reports_all_versions: HashMap<String, Vec<Report>>,
```

**Step 2: Initialize in constructor**

In `AppDataStore::new()` (around line 57), add to the initializer:

```rust
            reports: HashMap::new(),
            reports_all_versions: HashMap::new(),
```

**Step 3: Add load_reports method**

After `load_nudges` method (around line 216), add:

```rust
    /// Load all reports from nostrdb (kind:30023)
    fn load_reports(&mut self) {
        use nostrdb::{Filter, Transaction};

        let Ok(txn) = Transaction::new(&self.ndb) else {
            return;
        };

        let filter = Filter::new().kinds([30023]).build();
        let Ok(results) = self.ndb.query(&txn, &[filter], 1000) else {
            return;
        };

        tracing::info!("Loading {} reports (kind:30023)", results.len());

        for result in results {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, result.note_key) {
                if let Some(report) = Report::from_note(&note) {
                    self.add_report(report);
                }
            }
        }
    }

    /// Add a report, maintaining version history and latest-by-slug
    fn add_report(&mut self, report: Report) {
        let slug = report.slug.clone();

        // Add to all versions
        let versions = self.reports_all_versions.entry(slug.clone()).or_default();

        // Check for duplicate (same id)
        if versions.iter().any(|r| r.id == report.id) {
            return;
        }

        versions.push(report.clone());

        // Sort versions by created_at descending (newest first)
        versions.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        // Update latest version
        if let Some(latest) = versions.first() {
            self.reports.insert(slug, latest.clone());
        }
    }
```

**Step 4: Call load_reports in rebuild_from_ndb**

In `rebuild_from_ndb` method (around line 167), add after `self.load_operations_status();`:

```rust
        // Load reports (kind:30023)
        self.load_reports();
```

**Step 5: Add handle_report_event**

In `handle_event` method (around line 356), add a new case:

```rust
            30023 => self.handle_report_event(note),
```

Then add the handler method after `handle_operations_status_event`:

```rust
    fn handle_report_event(&mut self, note: &Note) {
        if let Some(report) = Report::from_note(note) {
            self.add_report(report);
        }
    }
```

**Step 6: Add getter methods**

After `get_nudge` method (around line 853), add:

```rust
    // ===== Report Methods (kind:30023) =====

    /// Get all reports (latest version of each), sorted by created_at descending
    pub fn get_reports(&self) -> Vec<&Report> {
        let mut reports: Vec<_> = self.reports.values().collect();
        reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        reports
    }

    /// Get reports for a specific project
    pub fn get_reports_by_project(&self, project_a_tag: &str) -> Vec<&Report> {
        let mut reports: Vec<_> = self.reports
            .values()
            .filter(|r| r.project_a_tag == project_a_tag)
            .collect();
        reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        reports
    }

    /// Get a specific report by slug (latest version)
    pub fn get_report(&self, slug: &str) -> Option<&Report> {
        self.reports.get(slug)
    }

    /// Get all versions of a report by slug
    pub fn get_report_versions(&self, slug: &str) -> Vec<&Report> {
        self.reports_all_versions
            .get(slug)
            .map(|v| v.iter().collect())
            .unwrap_or_default()
    }

    /// Get the previous version of a report (for diff)
    pub fn get_previous_report_version(&self, slug: &str, current_id: &str) -> Option<&Report> {
        let versions = self.reports_all_versions.get(slug)?;
        let current_idx = versions.iter().position(|r| r.id == current_id)?;
        versions.get(current_idx + 1)
    }
```

**Step 7: Commit**

```bash
git add crates/tenex-core/src/store/app_data_store.rs
git commit -m "feat(store): add reports storage and event handling for kind:30023"
```

---

## Task 3: Add HomeTab::Reports Variant

**Files:**
- Modify: `crates/tenex-tui/src/ui/app.rs`

**Step 1: Add Reports variant to HomeTab enum**

Find `HomeTab` enum (around line 52) and add:

```rust
#[derive(Debug, Clone, PartialEq)]
pub enum HomeTab {
    Recent,
    Inbox,
    Reports,
}
```

**Step 2: Add reports-related state to App struct**

After `selected_inbox_index` (around line 142), add:

```rust
    pub selected_report_index: usize,
    pub report_search_filter: String,
```

**Step 3: Initialize in App::new**

In the `App::new` function, add after `selected_inbox_index: 0,`:

```rust
            selected_report_index: 0,
            report_search_filter: String::new(),
```

**Step 4: Add reports helper methods**

After `inbox_items` method (around line 1175), add:

```rust
    /// Get reports for Home view (filtered by visible_projects and search filter)
    pub fn reports(&self) -> Vec<tenex_core::models::Report> {
        // Empty visible_projects = show nothing
        if self.visible_projects.is_empty() {
            return vec![];
        }

        let store = self.data_store.borrow();
        let filter = self.report_search_filter.to_lowercase();

        store.get_reports()
            .into_iter()
            .filter(|r| self.visible_projects.contains(&r.project_a_tag))
            .filter(|r| {
                if filter.is_empty() {
                    return true;
                }
                r.title.to_lowercase().contains(&filter)
                    || r.summary.to_lowercase().contains(&filter)
                    || r.content.to_lowercase().contains(&filter)
                    || r.hashtags.iter().any(|h| h.to_lowercase().contains(&filter))
            })
            .cloned()
            .collect()
    }
```

**Step 5: Commit**

```bash
git add crates/tenex-tui/src/ui/app.rs
git commit -m "feat(app): add HomeTab::Reports and reports state"
```

---

## Task 4: Add Reports Tab to Home View Header

**Files:**
- Modify: `crates/tenex-tui/src/ui/views/home.rs`

**Step 1: Update render_tab_header to include Reports**

Find `render_tab_header` function (around line 116) and update the spans:

```rust
fn render_tab_header(f: &mut Frame, app: &App, area: Rect) {
    let inbox_count = app.inbox_items().iter().filter(|i| !i.is_read).count();

    let tab_style = |tab: HomeTab| {
        if app.home_panel_focus == tab {
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        }
    };

    // Build tab spans
    let mut spans = vec![
        Span::styled("  TENEX", Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)),
        Span::styled("    ", Style::default()),
        Span::styled("Recent", tab_style(HomeTab::Recent)),
        Span::styled("   ", Style::default()),
        Span::styled("Inbox", tab_style(HomeTab::Inbox)),
    ];

    if inbox_count > 0 {
        spans.push(Span::styled(
            format!(" ({})", inbox_count),
            Style::default().fg(theme::ACCENT_ERROR),
        ));
    }

    spans.push(Span::styled("   ", Style::default()));
    spans.push(Span::styled("Reports", tab_style(HomeTab::Reports)));

    let header_line = Line::from(spans);

    // Second line: tab indicator underline
    let accent = Style::default().fg(theme::ACCENT_PRIMARY);
    let blank = Style::default();

    let indicator_spans = vec![
        Span::styled("         ", blank), // Padding for "  TENEX  "
        Span::styled(if app.home_panel_focus == HomeTab::Recent { "──────" } else { "      " },
            if app.home_panel_focus == HomeTab::Recent { accent } else { blank }),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Inbox { "─────" } else { "     " },
            if app.home_panel_focus == HomeTab::Inbox { accent } else { blank }),
        Span::styled(if inbox_count > 0 { "    " } else { "" }, blank),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Reports { "───────" } else { "       " },
            if app.home_panel_focus == HomeTab::Reports { accent } else { blank }),
    ];
    let indicator_line = Line::from(indicator_spans);

    let header = Paragraph::new(vec![header_line, indicator_line]);
    f.render_widget(header, area);
}
```

**Step 2: Update render_home to handle Reports tab**

In `render_home` function (around line 64), update the match:

```rust
    match app.home_panel_focus {
        HomeTab::Recent => render_recent_with_feed(f, app, padded_content),
        HomeTab::Inbox => render_inbox_cards(f, app, padded_content),
        HomeTab::Reports => render_reports_list(f, app, padded_content),
    }
```

**Step 3: Add render_reports_list function**

After `render_inbox_cards` function (around line 600), add:

```rust
/// Render the reports list with search
fn render_reports_list(f: &mut Frame, app: &App, area: Rect) {
    let reports = app.reports();

    // Layout: Search bar + List
    let chunks = Layout::vertical([
        Constraint::Length(2), // Search bar
        Constraint::Min(0),    // List
    ])
    .split(area);

    // Render search bar
    let search_style = if !app.report_search_filter.is_empty() {
        Style::default().fg(theme::TEXT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let search_text = if app.report_search_filter.is_empty() {
        "/ Search reports...".to_string()
    } else {
        format!("/ {}", app.report_search_filter)
    };

    let search_line = Paragraph::new(search_text).style(search_style);
    f.render_widget(search_line, chunks[0]);

    // Empty state
    if reports.is_empty() {
        let msg = if app.report_search_filter.is_empty() {
            "No reports found"
        } else {
            "No matching reports"
        };
        let empty = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty, chunks[1]);
        return;
    }

    // Render report cards
    let mut y_offset = 0u16;
    for (i, report) in reports.iter().enumerate() {
        let is_selected = i == app.selected_report_index;
        let card_height = 3u16; // title, summary, spacing

        if y_offset + card_height > chunks[1].height {
            break;
        }

        let card_area = Rect::new(
            chunks[1].x,
            chunks[1].y + y_offset,
            chunks[1].width,
            card_height,
        );

        render_report_card(f, app, report, is_selected, card_area);
        y_offset += card_height;
    }
}

/// Render a single report card
fn render_report_card(
    f: &mut Frame,
    app: &App,
    report: &tenex_core::models::Report,
    is_selected: bool,
    area: Rect,
) {
    let store = app.data_store.borrow();
    let project_name = store.get_project_name(&report.project_a_tag);
    let author_name = store.get_profile_name(&report.author);
    drop(store);

    let time_str = crate::ui::format::format_relative_time(report.created_at);
    let reading_time = format!("{}m", report.reading_time_mins);

    let title_style = if is_selected {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    let bullet = if is_selected { card::BULLET } else { card::SPACER };

    // Line 1: Title + project + reading time + timestamp
    let title_max = area.width as usize - 30;
    let title = crate::ui::format::truncate_with_ellipsis(&report.title, title_max);

    let line1 = Line::from(vec![
        Span::styled(bullet, Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(title, title_style),
        Span::styled("  ", Style::default()),
        Span::styled(&project_name, Style::default().fg(theme::project_color(&report.project_a_tag))),
        Span::styled(format!("  {} · {}", reading_time, time_str), Style::default().fg(theme::TEXT_MUTED)),
    ]);

    // Line 2: Summary + hashtags + author
    let summary_max = area.width as usize - 40;
    let summary = crate::ui::format::truncate_with_ellipsis(&report.summary, summary_max);
    let hashtags: String = report.hashtags.iter().take(3).map(|h| format!("#{} ", h)).collect();

    let line2 = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(summary, Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(format!("  {}", hashtags.trim()), Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(format!("  @{}", author_name), Style::default().fg(theme::ACCENT_SPECIAL)),
    ]);

    // Line 3: Spacing
    let line3 = Line::from("");

    let content = Paragraph::new(vec![line1, line2, line3]);

    if is_selected {
        f.render_widget(content.style(Style::default().bg(theme::BG_SELECTED)), area);
    } else {
        f.render_widget(content, area);
    }
}
```

**Step 4: Update help_bar for Reports tab**

In `render_help_bar` function (around line 880), add the Reports case:

```rust
        match app.home_panel_focus {
            HomeTab::Recent => "→ projects · ↑↓ navigate · Space fold · Enter open · n new · m filter · f time · A agents · q quit",
            HomeTab::Inbox => "→ projects · ↑↓ navigate · Enter open · r mark read · m filter · f time · A agents · q quit",
            HomeTab::Reports => "→ projects · / search · ↑↓ navigate · Enter view · Esc clear · A agents · q quit",
        }
```

**Step 5: Commit**

```bash
git add crates/tenex-tui/src/ui/views/home.rs
git commit -m "feat(ui): add Reports tab to home view with list rendering"
```

---

## Task 5: Add Tab Navigation for Reports

**Files:**
- Modify: `crates/tenex-tui/src/main.rs`

**Step 1: Add tab cycling to include Reports**

Find the home view input handling section. Add navigation between tabs using Tab key:

```rust
// In home view normal mode input handling, add:
KeyCode::Tab => {
    app.home_panel_focus = match app.home_panel_focus {
        HomeTab::Recent => HomeTab::Inbox,
        HomeTab::Inbox => HomeTab::Reports,
        HomeTab::Reports => HomeTab::Recent,
    };
}
KeyCode::BackTab => {
    app.home_panel_focus = match app.home_panel_focus {
        HomeTab::Recent => HomeTab::Reports,
        HomeTab::Inbox => HomeTab::Recent,
        HomeTab::Reports => HomeTab::Inbox,
    };
}
```

**Step 2: Add Reports-specific key handling**

Add handling for Reports tab navigation and search:

```rust
// When home_panel_focus == HomeTab::Reports:
KeyCode::Char('/') => {
    app.input_mode = InputMode::Editing;
    // Focus on search - handle input in editing mode
}
KeyCode::Up | KeyCode::Char('k') => {
    if app.selected_report_index > 0 {
        app.selected_report_index -= 1;
    }
}
KeyCode::Down | KeyCode::Char('j') => {
    let count = app.reports().len();
    if app.selected_report_index + 1 < count {
        app.selected_report_index += 1;
    }
}
KeyCode::Enter => {
    let reports = app.reports();
    if let Some(report) = reports.get(app.selected_report_index) {
        // Open report viewer modal (Task 6)
        app.modal_state = ModalState::ReportViewer(ReportViewerState::new(report.clone()));
    }
}
KeyCode::Esc => {
    if !app.report_search_filter.is_empty() {
        app.report_search_filter.clear();
        app.selected_report_index = 0;
    }
}
```

**Step 3: Add search input handling in editing mode**

```rust
// In InputMode::Editing for Reports tab:
if app.home_panel_focus == HomeTab::Reports {
    match key.code {
        KeyCode::Char(c) => {
            app.report_search_filter.push(c);
            app.selected_report_index = 0;
        }
        KeyCode::Backspace => {
            app.report_search_filter.pop();
            app.selected_report_index = 0;
        }
        KeyCode::Esc | KeyCode::Enter => {
            app.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}
```

**Step 4: Commit**

```bash
git add crates/tenex-tui/src/main.rs
git commit -m "feat(input): add Reports tab navigation and search handling"
```

---

## Task 6: Add ReportViewer Modal State

**Files:**
- Modify: `crates/tenex-tui/src/ui/modal.rs`

**Step 1: Add ReportViewerState struct**

After `ProjectActionsState` (around line 337), add:

```rust
/// Focus area in report viewer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportViewerFocus {
    Content,
    Threads,
}

/// View mode in report viewer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportViewMode {
    Current,
    Changes,
}

/// State for the report viewer modal
#[derive(Debug, Clone)]
pub struct ReportViewerState {
    pub report: tenex_core::models::Report,
    pub focus: ReportViewerFocus,
    pub view_mode: ReportViewMode,
    pub content_scroll: usize,
    pub threads_scroll: usize,
    pub selected_thread_index: usize,
    pub version_index: usize,
    pub show_threads: bool,
    pub show_copy_menu: bool,
    pub copy_menu_index: usize,
}

impl ReportViewerState {
    pub fn new(report: tenex_core::models::Report) -> Self {
        Self {
            report,
            focus: ReportViewerFocus::Content,
            view_mode: ReportViewMode::Current,
            content_scroll: 0,
            threads_scroll: 0,
            selected_thread_index: 0,
            version_index: 0,
            show_threads: false,
            show_copy_menu: false,
            copy_menu_index: 0,
        }
    }
}

/// Copy menu options for report viewer
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReportCopyOption {
    Bech32Id,
    RawEvent,
    Markdown,
}

impl ReportCopyOption {
    pub const ALL: [ReportCopyOption; 3] = [
        ReportCopyOption::Bech32Id,
        ReportCopyOption::RawEvent,
        ReportCopyOption::Markdown,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            ReportCopyOption::Bech32Id => "Copy Event ID (bech32)",
            ReportCopyOption::RawEvent => "Copy Raw Event (JSON)",
            ReportCopyOption::Markdown => "Copy Markdown Content",
        }
    }
}
```

**Step 2: Add to ModalState enum**

In the `ModalState` enum (around line 381), add:

```rust
    /// Report viewer modal with document, versions, and threads
    ReportViewer(ReportViewerState),
```

**Step 3: Commit**

```bash
git add crates/tenex-tui/src/ui/modal.rs
git commit -m "feat(modal): add ReportViewerState for document viewing"
```

---

## Task 7: Create Report Viewer Rendering

**Files:**
- Create: `crates/tenex-tui/src/ui/views/report_viewer.rs`
- Modify: `crates/tenex-tui/src/ui/views/mod.rs`

**Step 1: Create report_viewer.rs**

```rust
// crates/tenex-tui/src/ui/views/report_viewer.rs
use crate::ui::components::{modal_area, render_modal_background, render_modal_overlay, ModalSize};
use crate::ui::markdown::render_markdown;
use crate::ui::modal::{ReportCopyOption, ReportViewerFocus, ReportViewerState, ReportViewMode};
use crate::ui::{card, theme, App};
use crate::ui::format::{format_relative_time, truncate_with_ellipsis};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Scrollbar, ScrollbarOrientation, ScrollbarState},
    Frame,
};

pub fn render_report_viewer(f: &mut Frame, app: &App, area: Rect, state: &ReportViewerState) {
    render_modal_overlay(f, area);

    let size = ModalSize {
        max_width: 120,
        height_percent: 0.9,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    // Layout: Header | Content (with optional threads sidebar) | Help
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Min(0),    // Content
        Constraint::Length(1), // Help bar
    ])
    .split(popup_area);

    render_header(f, app, state, chunks[0]);

    if state.show_threads {
        // Split content into document and threads sidebar
        let content_chunks = Layout::horizontal([
            Constraint::Percentage(65), // Document
            Constraint::Percentage(35), // Threads
        ])
        .split(chunks[1]);

        render_document_content(f, app, state, content_chunks[0]);
        render_threads_sidebar(f, app, state, content_chunks[1]);
    } else {
        render_document_content(f, app, state, chunks[1]);
    }

    render_help_bar(f, state, chunks[2]);

    // Copy menu overlay
    if state.show_copy_menu {
        render_copy_menu(f, state, popup_area);
    }
}

fn render_header(f: &mut Frame, app: &App, state: &ReportViewerState, area: Rect) {
    let store = app.data_store.borrow();
    let author_name = store.get_profile_name(&state.report.author);
    let versions = store.get_report_versions(&state.report.slug);
    let version_count = versions.len();
    drop(store);

    let time_str = format_relative_time(state.report.created_at);
    let reading_time = format!("{}m read", state.report.reading_time_mins);

    // Line 1: Title
    let title_max = area.width as usize - 20;
    let title = truncate_with_ellipsis(&state.report.title, title_max);

    let line1 = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(title, Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
    ]);

    // Line 2: View toggle, version nav, copy button, metadata
    let current_style = if state.view_mode == ReportViewMode::Current {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let changes_style = if state.view_mode == ReportViewMode::Changes {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let version_str = if version_count > 1 {
        format!("  v{}/{}", state.version_index + 1, version_count)
    } else {
        String::new()
    };

    let line2 = Line::from(vec![
        Span::styled("  [", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Current", current_style),
        Span::styled("] [", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Changes", changes_style),
        Span::styled("]", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(version_str, Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(format!("  y:copy  {} · {} · @{}", reading_time, time_str, author_name),
            Style::default().fg(theme::TEXT_MUTED)),
    ]);

    let header = Paragraph::new(vec![line1, Line::from(""), line2]);
    f.render_widget(header, area);
}

fn render_document_content(f: &mut Frame, _app: &App, state: &ReportViewerState, area: Rect) {
    let content_area = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(4),
        area.height,
    );

    let lines: Vec<Line> = match state.view_mode {
        ReportViewMode::Current => render_markdown(&state.report.content),
        ReportViewMode::Changes => render_diff_view(&state.report.content),
    };

    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(state.content_scroll)
        .take(content_area.height as usize)
        .collect();

    let is_focused = state.focus == ReportViewerFocus::Content;
    let border_style = if is_focused {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::BORDER)
    };

    let content = Paragraph::new(visible_lines)
        .block(Block::default()
            .borders(Borders::LEFT)
            .border_style(border_style));

    f.render_widget(content, content_area);
}

fn render_diff_view(content: &str) -> Vec<Line<'static>> {
    // Placeholder - in real implementation, compute diff from previous version
    vec![
        Line::from(Span::styled(
            "No previous version available for diff",
            Style::default().fg(theme::TEXT_MUTED),
        ))
    ]
}

fn render_threads_sidebar(f: &mut Frame, app: &App, state: &ReportViewerState, area: Rect) {
    let is_focused = state.focus == ReportViewerFocus::Threads;
    let border_style = if is_focused {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::BORDER)
    };

    // Header
    let header_area = Rect::new(area.x, area.y, area.width, 2);
    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled("  Threads", Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::styled("  n: new", Style::default().fg(theme::TEXT_MUTED)),
        ]),
        Line::from(""),
    ]);
    f.render_widget(header, header_area);

    // Thread list area
    let list_area = Rect::new(area.x, area.y + 2, area.width, area.height.saturating_sub(2));

    // Get threads for this document (kind:1 events with #a tag referencing this document)
    let threads = get_document_threads(app, &state.report);

    if threads.is_empty() {
        let empty = Paragraph::new("  No discussions yet")
            .style(Style::default().fg(theme::TEXT_MUTED))
            .block(Block::default().borders(Borders::LEFT).border_style(border_style));
        f.render_widget(empty, list_area);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    for (i, thread) in threads.iter().enumerate() {
        let is_selected = is_focused && i == state.selected_thread_index;
        let bullet = if is_selected { card::BULLET } else { card::SPACER };
        let style = if is_selected {
            Style::default().fg(theme::ACCENT_PRIMARY)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        let store = app.data_store.borrow();
        let author_name = store.get_profile_name(&thread.pubkey);
        drop(store);

        let title_max = area.width as usize - 6;
        let title = truncate_with_ellipsis(&thread.title, title_max);
        let time_str = format_relative_time(thread.last_activity);

        lines.push(Line::from(vec![
            Span::styled(bullet, Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled(title, style),
        ]));
        lines.push(Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(format!("@{} · {}", author_name, time_str), Style::default().fg(theme::TEXT_MUTED)),
        ]));
        lines.push(Line::from(""));
    }

    let content = Paragraph::new(lines)
        .block(Block::default().borders(Borders::LEFT).border_style(border_style));
    f.render_widget(content, list_area);
}

fn get_document_threads(app: &App, report: &tenex_core::models::Report) -> Vec<tenex_core::models::Thread> {
    // Get threads that reference this document via a-tag
    // For now, return empty - will be populated when we add document thread support
    vec![]
}

fn render_help_bar(f: &mut Frame, state: &ReportViewerState, area: Rect) {
    let hints = match state.focus {
        ReportViewerFocus::Content => {
            "↑↓/jk scroll · Tab toggle view · [/] versions · t threads · h/l focus · y copy · Esc close"
        }
        ReportViewerFocus::Threads => {
            "↑↓/jk navigate · Enter open · n new thread · h/l focus · Esc close"
        }
    };

    let help = Paragraph::new(format!("  {}", hints))
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(help, area);
}

fn render_copy_menu(f: &mut Frame, state: &ReportViewerState, parent_area: Rect) {
    let menu_width = 30u16;
    let menu_height = 5u16;

    let menu_area = Rect::new(
        parent_area.x + parent_area.width.saturating_sub(menu_width + 4),
        parent_area.y + 3,
        menu_width,
        menu_height,
    );

    let bg = Block::default()
        .style(Style::default().bg(theme::BG_MODAL))
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER));
    f.render_widget(bg, menu_area);

    let inner = Rect::new(menu_area.x + 1, menu_area.y + 1, menu_area.width - 2, menu_area.height - 2);

    let items: Vec<Line> = ReportCopyOption::ALL
        .iter()
        .enumerate()
        .map(|(i, opt)| {
            let is_selected = i == state.copy_menu_index;
            let bullet = if is_selected { card::BULLET } else { card::SPACER };
            let style = if is_selected {
                Style::default().fg(theme::ACCENT_PRIMARY)
            } else {
                Style::default().fg(theme::TEXT_PRIMARY)
            };
            Line::from(vec![
                Span::styled(bullet, Style::default().fg(theme::ACCENT_PRIMARY)),
                Span::styled(opt.label(), style),
            ])
        })
        .collect();

    let menu = Paragraph::new(items);
    f.render_widget(menu, inner);
}
```

**Step 2: Add to mod.rs**

In `crates/tenex-tui/src/ui/views/mod.rs`, add:

```rust
pub mod report_viewer;
pub use report_viewer::render_report_viewer;
```

**Step 3: Commit**

```bash
git add crates/tenex-tui/src/ui/views/report_viewer.rs crates/tenex-tui/src/ui/views/mod.rs
git commit -m "feat(ui): add report viewer modal rendering"
```

---

## Task 8: Wire Up Report Viewer in Main Render Loop

**Files:**
- Modify: `crates/tenex-tui/src/main.rs`

**Step 1: Import report viewer**

Add to imports at top of main.rs:

```rust
use crate::ui::views::render_report_viewer;
```

**Step 2: Render report viewer modal**

In the main render function, after other modal rendering, add:

```rust
// Report viewer modal
if let ModalState::ReportViewer(ref state) = app.modal_state {
    render_report_viewer(f, app, area, state);
}
```

**Step 3: Add report viewer input handling**

Add input handling for the report viewer modal:

```rust
ModalState::ReportViewer(ref mut state) => {
    match key.code {
        KeyCode::Esc => {
            if state.show_copy_menu {
                state.show_copy_menu = false;
            } else {
                app.modal_state = ModalState::None;
            }
        }
        KeyCode::Tab => {
            state.view_mode = match state.view_mode {
                ReportViewMode::Current => ReportViewMode::Changes,
                ReportViewMode::Changes => ReportViewMode::Current,
            };
        }
        KeyCode::Char('t') => {
            state.show_threads = !state.show_threads;
        }
        KeyCode::Char('h') | KeyCode::Left => {
            state.focus = ReportViewerFocus::Content;
        }
        KeyCode::Char('l') | KeyCode::Right => {
            if state.show_threads {
                state.focus = ReportViewerFocus::Threads;
            }
        }
        KeyCode::Char('y') => {
            state.show_copy_menu = !state.show_copy_menu;
        }
        KeyCode::Char('[') => {
            let versions = app.data_store.borrow().get_report_versions(&state.report.slug);
            if state.version_index + 1 < versions.len() {
                state.version_index += 1;
                if let Some(v) = versions.get(state.version_index) {
                    state.report = v.clone();
                    state.content_scroll = 0;
                }
            }
        }
        KeyCode::Char(']') => {
            if state.version_index > 0 {
                state.version_index -= 1;
                let versions = app.data_store.borrow().get_report_versions(&state.report.slug);
                if let Some(v) = versions.get(state.version_index) {
                    state.report = v.clone();
                    state.content_scroll = 0;
                }
            }
        }
        KeyCode::Up | KeyCode::Char('k') => {
            match state.focus {
                ReportViewerFocus::Content => {
                    state.content_scroll = state.content_scroll.saturating_sub(1);
                }
                ReportViewerFocus::Threads => {
                    if state.selected_thread_index > 0 {
                        state.selected_thread_index -= 1;
                    }
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            match state.focus {
                ReportViewerFocus::Content => {
                    state.content_scroll += 1;
                }
                ReportViewerFocus::Threads => {
                    state.selected_thread_index += 1;
                }
            }
        }
        KeyCode::Enter => {
            if state.show_copy_menu {
                // Handle copy action
                let option = ReportCopyOption::ALL[state.copy_menu_index];
                handle_report_copy(app, &state.report, option);
                state.show_copy_menu = false;
            }
        }
        _ => {}
    }
}
```

**Step 4: Add copy handler function**

```rust
fn handle_report_copy(app: &mut App, report: &Report, option: ReportCopyOption) {
    use arboard::Clipboard;

    let text = match option {
        ReportCopyOption::Bech32Id => {
            // Convert to bech32 nevent
            format!("nevent1{}", report.id) // Simplified - use proper bech32 encoding
        }
        ReportCopyOption::RawEvent => {
            // Get raw event JSON from nostrdb
            crate::store::get_raw_event_json(&app.db.ndb, &report.id)
                .unwrap_or_else(|| "Failed to get raw event".to_string())
        }
        ReportCopyOption::Markdown => {
            report.content.clone()
        }
    };

    if let Ok(mut clipboard) = Clipboard::new() {
        let _ = clipboard.set_text(&text);
        app.set_status(&format!("Copied to clipboard"));
    }
}
```

**Step 5: Commit**

```bash
git add crates/tenex-tui/src/main.rs
git commit -m "feat(input): wire up report viewer modal with full input handling"
```

---

## Task 9: Add Document Thread Support

**Files:**
- Modify: `crates/tenex-core/src/store/app_data_store.rs`
- Modify: `crates/tenex-tui/src/ui/views/report_viewer.rs`

**Step 1: Add document threads storage to AppDataStore**

After `reports_all_versions` field, add:

```rust
    // Threads by document a-tag (kind:1 events that a-tag a document)
    pub document_threads: HashMap<String, Vec<Thread>>,
```

Initialize in constructor:

```rust
            document_threads: HashMap::new(),
```

**Step 2: Update handle_text_event to track document threads**

In `handle_text_event`, after checking for project threads, add:

```rust
        // Check if this is a document discussion thread (has a-tag for a report)
        if !has_e_tag {
            for tag in note.tags() {
                if tag.get(0).and_then(|t| t.variant().str()) == Some("a") {
                    if let Some(a_val) = tag.get(1).and_then(|t| t.variant().str()) {
                        // Check if it's a report a-tag (30023:pubkey:slug)
                        if a_val.starts_with("30023:") {
                            if let Some(thread) = Thread::from_note(note) {
                                let threads = self.document_threads.entry(a_val.to_string()).or_default();
                                if !threads.iter().any(|t| t.id == thread.id) {
                                    threads.push(thread);
                                    threads.sort_by(|a, b| b.last_activity.cmp(&a.last_activity));
                                }
                            }
                            break;
                        }
                    }
                }
            }
        }
```

**Step 3: Add getter method**

```rust
    /// Get threads for a specific document (by document a-tag)
    pub fn get_document_threads(&self, document_a_tag: &str) -> &[Thread] {
        self.document_threads.get(document_a_tag)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }
```

**Step 4: Update report_viewer.rs to use real threads**

Update `get_document_threads` function:

```rust
fn get_document_threads(app: &App, report: &tenex_core::models::Report) -> Vec<tenex_core::models::Thread> {
    let a_tag = report.a_tag();
    app.data_store.borrow().get_document_threads(&a_tag).to_vec()
}
```

**Step 5: Commit**

```bash
git add crates/tenex-core/src/store/app_data_store.rs crates/tenex-tui/src/ui/views/report_viewer.rs
git commit -m "feat(store): add document thread tracking and retrieval"
```

---

## Task 10: Add Thread Interaction from Report Viewer

**Files:**
- Modify: `crates/tenex-tui/src/main.rs`
- Modify: `crates/tenex-tui/src/ui/views/report_viewer.rs`

**Step 1: Add thread opening from report viewer**

In report viewer input handling, add Enter key handling for threads:

```rust
KeyCode::Enter => {
    if state.show_copy_menu {
        // Handle copy action
        let option = ReportCopyOption::ALL[state.copy_menu_index];
        handle_report_copy(app, &state.report, option);
        state.show_copy_menu = false;
    } else if state.focus == ReportViewerFocus::Threads {
        // Open selected thread
        let threads = get_document_threads(app, &state.report);
        if let Some(thread) = threads.get(state.selected_thread_index) {
            // Find project for this thread
            if let Some(project_a_tag) = app.data_store.borrow().find_project_for_thread(&thread.id) {
                app.open_thread_from_home(thread, &project_a_tag);
                app.modal_state = ModalState::None;
            }
        }
    }
}
```

**Step 2: Add new thread creation shortcut**

```rust
KeyCode::Char('n') => {
    if state.focus == ReportViewerFocus::Threads || state.show_threads {
        // Create new thread on this document
        // For now, just set status - full implementation would open thread creator
        app.set_status("Thread creation not yet implemented");
    }
}
```

**Step 3: Commit**

```bash
git add crates/tenex-tui/src/main.rs
git commit -m "feat(reports): add thread opening from report viewer"
```

---

## Task 11: Add Diff View Implementation

**Files:**
- Modify: `crates/tenex-tui/src/ui/views/report_viewer.rs`

**Step 1: Add diff computation**

Update the `render_diff_view` function to compute actual diffs:

```rust
fn render_diff_view(current: &str, previous: Option<&str>) -> Vec<Line<'static>> {
    let Some(previous) = previous else {
        return vec![
            Line::from(Span::styled(
                "No previous version available for diff",
                Style::default().fg(theme::TEXT_MUTED),
            ))
        ];
    };

    let mut lines = Vec::new();
    let current_lines: Vec<&str> = current.lines().collect();
    let previous_lines: Vec<&str> = previous.lines().collect();

    // Simple line-by-line diff (for more sophisticated diff, use similar crate)
    let max_len = current_lines.len().max(previous_lines.len());

    for i in 0..max_len {
        let curr = current_lines.get(i).copied();
        let prev = previous_lines.get(i).copied();

        match (curr, prev) {
            (Some(c), Some(p)) if c == p => {
                // Unchanged
                lines.push(Line::from(Span::styled(
                    format!("  {}", c),
                    Style::default().fg(theme::TEXT_MUTED),
                )));
            }
            (Some(c), Some(p)) => {
                // Changed - show both
                lines.push(Line::from(Span::styled(
                    format!("- {}", p),
                    Style::default().fg(theme::ACCENT_ERROR),
                )));
                lines.push(Line::from(Span::styled(
                    format!("+ {}", c),
                    Style::default().fg(theme::ACCENT_SUCCESS),
                )));
            }
            (Some(c), None) => {
                // Added
                lines.push(Line::from(Span::styled(
                    format!("+ {}", c),
                    Style::default().fg(theme::ACCENT_SUCCESS),
                )));
            }
            (None, Some(p)) => {
                // Removed
                lines.push(Line::from(Span::styled(
                    format!("- {}", p),
                    Style::default().fg(theme::ACCENT_ERROR),
                )));
            }
            (None, None) => break,
        }
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No changes from previous version",
            Style::default().fg(theme::TEXT_MUTED),
        )));
    }

    lines
}
```

**Step 2: Update render_document_content to pass previous version**

```rust
fn render_document_content(f: &mut Frame, app: &App, state: &ReportViewerState, area: Rect) {
    let content_area = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(4),
        area.height,
    );

    let lines: Vec<Line> = match state.view_mode {
        ReportViewMode::Current => render_markdown(&state.report.content),
        ReportViewMode::Changes => {
            let previous = app.data_store.borrow()
                .get_previous_report_version(&state.report.slug, &state.report.id)
                .map(|r| r.content.clone());
            render_diff_view(&state.report.content, previous.as_deref())
        }
    };

    // ... rest unchanged
}
```

**Step 3: Commit**

```bash
git add crates/tenex-tui/src/ui/views/report_viewer.rs
git commit -m "feat(reports): implement diff view for version comparison"
```

---

## Task 12: Subscribe to Kind 30023 Events

**Files:**
- Modify: `crates/tenex-core/src/nostr/worker.rs`

**Step 1: Add kind 30023 to subscription filter**

Find where subscriptions are set up and add kind 30023 to the kinds array:

```rust
// In the main subscription filter, add 30023 to the kinds
let filter = Filter::new()
    .kinds([1, 513, 4129, 4199, 4201, 24010, 24133, 30023, 31933])
    // ... rest of filter
```

**Step 2: Commit**

```bash
git add crates/tenex-core/src/nostr/worker.rs
git commit -m "feat(nostr): subscribe to kind:30023 report events"
```

---

## Task 13: Final Integration and Testing

**Step 1: Build and fix any compilation errors**

```bash
cargo build
```

**Step 2: Run and test the Reports tab**

```bash
cargo run
```

Test:
- Tab cycles through Recent → Inbox → Reports
- Reports list shows documents from visible projects
- Search filters reports by title/summary/content/hashtags
- Selecting a report opens the viewer modal
- Viewer shows markdown content
- `t` toggles threads sidebar
- `Tab` toggles Current/Changes view
- `[` and `]` navigate versions (if multiple)
- `y` opens copy menu
- `h`/`l` switches focus between content and threads
- `Esc` closes viewer

**Step 3: Final commit**

```bash
git add -A
git commit -m "feat(reports): complete Reports tab implementation with viewer and threads"
```

---

## Summary

This plan implements the Reports tab with:

1. **Data Layer**: Report model, store integration, event handling
2. **Reports List**: Tab in Home view, search, card rendering
3. **Document Viewer**: Modal with markdown rendering, version navigation, diff view
4. **Threads Integration**: Sidebar showing document-scoped discussions
5. **Copy Menu**: Bech32 ID, raw event, markdown content options
6. **Keybindings**: Full navigation with vim-style keys

All code follows existing patterns in the codebase and uses the established component architecture.
