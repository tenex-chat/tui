use crate::ui::format::{format_relative_time, truncate_with_ellipsis};
use crate::ui::markdown::render_markdown;
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::cell::Ref;
use std::collections::{HashMap, HashSet};
use tenex_core::models::Report;
use tenex_core::store::AppDataStore;

/// A display entry in the reports list - either a single report or a document group.
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
            ReportEntry::Group {
                project_a_tag,
                document,
                ..
            } => Some(format!("{}|{}", project_a_tag, document)),
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
pub fn build_visible_items(
    entries: &[ReportEntry],
    expanded_groups: &HashSet<String>,
) -> Vec<(usize, Option<usize>)> {
    let mut items = Vec::new();
    for (i, entry) in entries.iter().enumerate() {
        items.push((i, None));
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
        let empty_lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "No reports",
                Style::default().fg(theme::TEXT_MUTED),
            )),
        ];
        let empty = Paragraph::new(empty_lines).alignment(ratatui::layout::Alignment::Center);
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
                let lines_needed = 3u16;
                if y + lines_needed > area.height {
                    break;
                }
                let item_area = Rect::new(area.x, area.y + y, area.width, lines_needed);
                render_single_report_row(f, report, &store, is_selected, item_area);
                y += lines_needed;
            }
            (
                ReportEntry::Group {
                    document,
                    project_a_tag,
                    reports,
                },
                None,
            ) => {
                let is_expanded = entry
                    .group_key()
                    .map(|k| app.reports_expanded_groups.contains(&k))
                    .unwrap_or(false);
                let lines_needed = 2u16;
                if y + lines_needed > area.height {
                    break;
                }
                let item_area = Rect::new(area.x, area.y + y, area.width, lines_needed);
                render_group_row(
                    f,
                    document,
                    project_a_tag,
                    reports.len(),
                    is_expanded,
                    &store,
                    is_selected,
                    item_area,
                );
                y += lines_needed;
            }
            (ReportEntry::Group { reports, .. }, Some(j)) => {
                if let Some(report) = reports.get(j) {
                    let lines_needed = 3u16;
                    if y + lines_needed > area.height {
                        break;
                    }
                    let item_area = Rect::new(
                        area.x + 2,
                        area.y + y,
                        area.width.saturating_sub(2),
                        lines_needed,
                    );
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
    store: &Ref<AppDataStore>,
    is_selected: bool,
    area: Rect,
) {
    let project_name = store.get_project_name(&report.project_a_tag);
    let title = if report.title.is_empty() {
        "Untitled"
    } else {
        &report.title
    };
    let title_max = (area.width as usize)
        .saturating_sub(project_name.chars().count() + 4)
        .max(10);
    let truncated_title = truncate_with_ellipsis(title, title_max);

    let title_style = if is_selected {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD)
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
    let summary_source = if report.summary.is_empty() {
        &report.content
    } else {
        &report.summary
    };
    let summary = truncate_with_ellipsis(summary_source, summary_max);
    let line2 = Line::from(Span::styled(
        summary,
        Style::default().fg(theme::TEXT_MUTED),
    ));

    // Line 3: reading time + relative time
    let reading_time = if report.reading_time_mins == 1 {
        "1 min read".to_string()
    } else {
        format!("{} min read", report.reading_time_mins)
    };
    let time_ago = format_relative_time(report.created_at);
    let line3 = Line::from(Span::styled(
        format!("{} · {}", reading_time, time_ago),
        Style::default()
            .fg(theme::TEXT_MUTED)
            .add_modifier(Modifier::DIM),
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
    store: &Ref<AppDataStore>,
    is_selected: bool,
    area: Rect,
) {
    let project_name = store.get_project_name(project_a_tag);
    let prefix = if is_expanded { "[-]" } else { "[+]" };

    let title_style = if is_selected {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD)
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

/// Render report detail content (used inside a tab).
pub fn render_report_detail(f: &mut Frame, app: &App, area: Rect) {
    let slug = app
        .tabs
        .active_tab()
        .and_then(|t| t.report_slug.as_deref())
        .unwrap_or("");

    let report = app.data_store.borrow().reports.get_report(slug).cloned();

    let Some(report) = report else {
        let msg =
            Paragraph::new("Report not found").style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(msg, area);
        return;
    };

    // Build author/project display strings from store, then drop the borrow.
    let (author_name, project_name, project_color) = {
        let store = app.data_store.borrow();
        (
            store.get_profile_name(&report.author),
            store.get_project_name(&report.project_a_tag),
            theme::project_color(&report.project_a_tag),
        )
    };

    let mut lines: Vec<Line<'static>> = Vec::new();

    // Title
    let title = if report.title.is_empty() {
        "Untitled".to_string()
    } else {
        report.title.clone()
    };
    lines.push(Line::from(Span::styled(
        title,
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )));

    // Summary
    if !report.summary.is_empty() {
        lines.push(Line::from(Span::styled(
            report.summary.clone(),
            Style::default().fg(theme::TEXT_MUTED),
        )));
    }

    // Meta line: reading time + author + date + project
    let reading_time = if report.reading_time_mins == 1 {
        "1 min read".to_string()
    } else {
        format!("{} min read", report.reading_time_mins)
    };
    let time_ago = format_relative_time(report.created_at);
    lines.push(Line::from(vec![
        Span::styled(reading_time, Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · by ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(author_name, Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(time_ago, Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(project_name, Style::default().fg(project_color)),
    ]));

    // Divider
    let divider = "─".repeat(area.width as usize);
    lines.push(Line::from(Span::styled(
        divider,
        Style::default().fg(theme::BORDER_INACTIVE),
    )));
    lines.push(Line::from(""));

    // Markdown content
    let md_lines = render_markdown(&report.content);
    lines.extend(md_lines);

    // Apply scroll offset
    let scroll = app.scroll_offset;
    let visible_lines: Vec<Line<'static>> = lines.into_iter().skip(scroll).collect();

    let para = Paragraph::new(visible_lines).style(Style::default().bg(theme::BG_APP));
    f.render_widget(para, area);
}
