use crate::ui::card;
use crate::ui::markdown::render_markdown;
use crate::ui::{theme, App};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::collections::BTreeMap;
use tenex_core::models::Report;

/// Render the reports view — list or detail mode
pub fn render_reports(f: &mut Frame, app: &App, area: Rect) {
    if app.home.in_report_detail() {
        if let Some(ref slug) = app.home.viewing_report_slug.clone() {
            let report = app.data_store.borrow().reports.get_report(slug).cloned();
            if let Some(report) = report {
                render_report_detail(f, app, area, &report);
            }
        }
    } else {
        render_report_list(f, app, area);
    }
}

/// Flatten grouped reports into an ordered flat list for rendering and navigation.
/// Each entry contains an optional group header label (first item in group) and the Report.
pub fn flat_report_list(app: &App) -> Vec<(Option<String>, Report)> {
    let reports: Vec<Report> = app
        .data_store
        .borrow()
        .reports
        .get_reports()
        .into_iter()
        .cloned()
        .collect();

    if reports.is_empty() {
        return vec![];
    }

    // Group by first hashtag, "Other" for untagged
    let mut named: BTreeMap<String, Vec<Report>> = BTreeMap::new();
    let mut other: Vec<Report> = vec![];

    for report in reports {
        match report.hashtags.first().cloned() {
            Some(tag) => named.entry(tag).or_default().push(report),
            None => other.push(report),
        }
    }

    // BTreeMap is already alphabetically sorted
    let mut groups: Vec<(String, Vec<Report>)> = named.into_iter().collect();
    if !other.is_empty() {
        groups.push(("Other".to_string(), other));
    }

    let mut flat: Vec<(Option<String>, Report)> = vec![];
    for (group_name, mut group_reports) in groups {
        group_reports.sort_by(|a, b| b.created_at.cmp(&a.created_at));
        for (i, report) in group_reports.into_iter().enumerate() {
            let label = if i == 0 { Some(group_name.clone()) } else { None };
            flat.push((label, report));
        }
    }
    flat
}

fn render_report_list(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .split(area);

    render_list_header(f, app, chunks[0]);
    render_list_content(f, app, chunks[1]);
    render_list_footer(f, chunks[2]);
}

fn render_list_header(f: &mut Frame, app: &App, area: Rect) {
    let count = app.data_store.borrow().reports.get_reports().len();

    let spans = vec![
        Span::styled("📄 ", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(
            "Reports",
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(" ({} reports)", count),
            Style::default().fg(theme::TEXT_MUTED),
        ),
    ];

    let header = Paragraph::new(Line::from(spans)).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
    );
    f.render_widget(header, area);
}

fn render_list_content(f: &mut Frame, app: &App, area: Rect) {
    let flat = flat_report_list(app);

    if flat.is_empty() {
        let empty = Paragraph::new(
            "No reports found. Reports (kind:30023 articles) will appear here once received.",
        )
        .style(Style::default().fg(theme::TEXT_MUTED))
        .wrap(Wrap { trim: true })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
        );
        f.render_widget(empty, area);
        return;
    }

    let visible_height = area.height.saturating_sub(2) as usize;
    let selected_index = app.home.reports_index.min(flat.len().saturating_sub(1));

    let scroll_offset = if selected_index >= visible_height {
        selected_index - visible_height + 1
    } else {
        0
    };

    let items: Vec<ListItem> = flat
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(idx, (group_label, report))| {
            let is_selected = idx == selected_index;
            create_report_list_item(report, group_label.as_deref(), is_selected)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
    );
    f.render_widget(list, area);
}

fn create_report_list_item(
    report: &Report,
    group_label: Option<&str>,
    is_selected: bool,
) -> ListItem<'static> {
    let mut lines: Vec<Line<'static>> = vec![];

    // Group header line (only for first item in group)
    if let Some(label) = group_label {
        lines.push(Line::from(vec![Span::styled(
            format!("  {} ", label.to_uppercase()),
            Style::default()
                .fg(theme::ACCENT_SPECIAL)
                .add_modifier(Modifier::BOLD),
        )]));
    }

    let mut spans: Vec<Span<'static>> = vec![];

    if is_selected {
        spans.push(Span::styled(
            card::COLLAPSE_CLOSED,
            Style::default().fg(theme::ACCENT_PRIMARY),
        ));
    } else {
        spans.push(Span::styled(card::SPACER, Style::default()));
    }

    let title_style = if is_selected {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };
    spans.push(Span::styled(report.title.clone(), title_style));

    if report.reading_time_mins > 0 {
        spans.push(Span::styled(
            format!(" ({}m read)", report.reading_time_mins),
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }

    if !report.summary.is_empty() {
        let preview: String = report.summary.chars().take(60).collect();
        let suffix = if report.summary.len() > 60 { "…" } else { "" };
        spans.push(Span::styled(
            format!(" — {}{}", preview, suffix),
            Style::default().fg(theme::TEXT_DIM),
        ));
    }

    lines.push(Line::from(spans));
    ListItem::new(lines)
}

fn render_list_footer(f: &mut Frame, area: Rect) {
    let help_spans = vec![
        Span::styled("↑/↓", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" navigate | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" read | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Tab", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" switch tabs", Style::default().fg(theme::TEXT_MUTED)),
    ];
    let footer = Paragraph::new(Line::from(help_spans)).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(footer, area);
}

fn render_report_detail(f: &mut Frame, app: &App, area: Rect, report: &Report) {
    let chunks = Layout::vertical([
        Constraint::Length(5),
        Constraint::Min(0),
        Constraint::Length(2),
    ])
    .split(area);

    let reading_time = if report.reading_time_mins > 0 {
        format!(" · {} min read", report.reading_time_mins)
    } else {
        String::new()
    };

    let tags_str = if !report.hashtags.is_empty() {
        format!(" [{}]", report.hashtags.join(", "))
    } else {
        String::new()
    };

    let header_text = vec![
        Line::from(vec![Span::styled(
            report.title.clone(),
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![
            Span::styled(reading_time, Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(tags_str, Style::default().fg(theme::ACCENT_SPECIAL)),
        ]),
        Line::from(vec![Span::styled(
            report.summary.clone(),
            Style::default().fg(theme::TEXT_DIM),
        )]),
    ];

    let header = Paragraph::new(header_text)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
        )
        .wrap(Wrap { trim: true });
    f.render_widget(header, chunks[0]);

    let md_lines = render_markdown(&report.content);
    let scroll = app.home.report_detail_scroll as u16;
    let content = Paragraph::new(md_lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::TEXT_DIM)),
        )
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0));
    f.render_widget(content, chunks[1]);

    let footer_spans = vec![
        Span::styled("↑/↓", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" scroll | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" back to list", Style::default().fg(theme::TEXT_MUTED)),
    ];
    let footer = Paragraph::new(Line::from(footer_spans)).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(footer, chunks[2]);
}

/// Get the count of items in the flat report list (for navigation bounds)
pub fn reports_list_len(app: &App) -> usize {
    flat_report_list(app).len()
}

/// Get the slug of the report at the given flat index
pub fn report_slug_at_index(app: &App, index: usize) -> Option<String> {
    flat_report_list(app)
        .into_iter()
        .nth(index)
        .map(|(_, r)| r.slug)
}
