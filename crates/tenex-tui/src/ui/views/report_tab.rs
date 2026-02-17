//! Report Tab View
//!
//! Renders a report as a tab with a split view:
//! - Left: Report content (scrollable markdown)
//! - Right: Chat sidebar for conversing with report author
//!
//! Tab key switches focus between content and chat.

use crate::ui::format::{format_relative_time, truncate_with_ellipsis};
use crate::ui::markdown::render_markdown;
use crate::ui::state::{ReportTabFocus, ReportTabState};
use crate::ui::{card, theme, App};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the report tab content
pub fn render_report_tab(f: &mut Frame, app: &App, area: Rect) {
    // Get report state from the active tab
    let report_state = app.tabs.active_tab().and_then(|t| t.report_state.as_ref());

    let Some(state) = report_state else {
        render_no_report_state(f, area);
        return;
    };

    // Get the report from the data store using a_tag (globally unique)
    // This handles slug collisions between different authors
    let data_store = app.data_store.borrow();
    let report = data_store.reports.get_report_by_a_tag(&state.a_tag);

    let Some(report) = report else {
        drop(data_store);
        render_report_not_found(f, area, &state.slug);
        return;
    };

    // Clone data we need before dropping the borrow
    let report: tenex_core::models::Report = report.clone();
    let author_name = data_store.get_profile_name(&report.author);
    // Get previous version for diff view if available
    let previous_content: Option<String> = data_store
        .reports
        .get_previous_report_version(&state.slug, &report.id)
        .map(|r| r.content.clone());
    drop(data_store);

    // Layout: Header | Content (split) | Help Bar
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Min(0),    // Content area
        Constraint::Length(1), // Help bar
    ])
    .split(area);

    render_header(f, &report, &author_name, state, chunks[0]);

    // Split content area: 65% report, 35% chat sidebar
    let content_chunks =
        Layout::horizontal([Constraint::Percentage(65), Constraint::Percentage(35)])
            .split(chunks[1]);

    render_report_content(
        f,
        &report,
        state,
        previous_content.as_deref(),
        content_chunks[0],
    );
    render_chat_sidebar(f, app, state, &author_name, content_chunks[1]);

    render_help_bar(f, state, chunks[2]);
}

fn render_no_report_state(f: &mut Frame, area: Rect) {
    let message = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "No report state available",
            Style::default().fg(theme::TEXT_MUTED),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BORDER_INACTIVE)),
    );
    f.render_widget(message, area);
}

fn render_report_not_found(f: &mut Frame, area: Rect, slug: &str) {
    let message = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("Report '{}' not found", slug),
            Style::default().fg(theme::ACCENT_ERROR),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "The report may have been deleted or is not yet synced.",
            Style::default().fg(theme::TEXT_MUTED),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::BORDER_INACTIVE)),
    );
    f.render_widget(message, area);
}

fn render_header(
    f: &mut Frame,
    report: &tenex_core::models::Report,
    author_name: &str,
    state: &ReportTabState,
    area: Rect,
) {
    let time_str = format_relative_time(report.created_at);
    let reading_time = format!("{}m read", report.reading_time_mins);

    // Line 1: Title
    let title_max = area.width as usize - 20;
    let title = truncate_with_ellipsis(&report.title, title_max);

    let line1 = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(
            title,
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        ),
    ]);

    // Line 2: View mode indicator and metadata
    let view_mode = if state.show_diff {
        Span::styled(
            "[Changes]",
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(
            "[Current]",
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        )
    };

    let focus_indicator = match state.focus {
        ReportTabFocus::Content => "Content",
        ReportTabFocus::Chat => "Chat",
    };

    let line2 = Line::from(vec![
        Span::styled("  ", Style::default()),
        view_mode,
        Span::styled(
            format!("  {} · {} · @{}", reading_time, time_str, author_name),
            Style::default().fg(theme::TEXT_MUTED),
        ),
        Span::styled(
            format!("  [Focus: {}]", focus_indicator),
            Style::default().fg(theme::TEXT_MUTED),
        ),
    ]);

    let header = Paragraph::new(vec![line1, Line::from(""), line2]);
    f.render_widget(header, area);
}

fn render_report_content(
    f: &mut Frame,
    report: &tenex_core::models::Report,
    state: &ReportTabState,
    previous_content: Option<&str>,
    area: Rect,
) {
    let content_area = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(3),
        area.height,
    );

    // Render markdown content or diff depending on toggle
    let lines: Vec<Line> = if state.show_diff {
        render_diff_view(&report.content, previous_content)
    } else {
        render_markdown(&report.content)
    };

    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(state.content_scroll)
        .take(content_area.height as usize)
        .collect();

    let is_focused = state.focus == ReportTabFocus::Content;
    let border_style = if is_focused {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::BORDER_INACTIVE)
    };

    let content = Paragraph::new(visible_lines).block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .border_style(border_style),
    );

    f.render_widget(content, content_area);
}

/// Render a simple line-by-line diff view.
/// If previous_content is None, shows current content with "no previous version" message.
fn render_diff_view(current_content: &str, previous_content: Option<&str>) -> Vec<Line<'static>> {
    match previous_content {
        Some(prev) => {
            // Simple line-by-line diff visualization
            let current_lines: Vec<&str> = current_content.lines().collect();
            let prev_lines: Vec<&str> = prev.lines().collect();

            let mut result: Vec<Line<'static>> = Vec::new();
            result.push(Line::from(Span::styled(
                "═══ Changes from previous version ═══".to_string(),
                Style::default()
                    .fg(theme::ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )));
            result.push(Line::from(""));

            let max_lines = current_lines.len().max(prev_lines.len());
            for i in 0..max_lines {
                let curr = current_lines.get(i).copied().unwrap_or("");
                let prev_line = prev_lines.get(i).copied().unwrap_or("");

                if curr != prev_line {
                    if !prev_line.is_empty() && curr.is_empty() {
                        // Line removed
                        result.push(Line::from(Span::styled(
                            format!("- {}", prev_line),
                            Style::default().fg(theme::ACCENT_ERROR),
                        )));
                    } else if prev_line.is_empty() && !curr.is_empty() {
                        // Line added
                        result.push(Line::from(Span::styled(
                            format!("+ {}", curr),
                            Style::default().fg(theme::ACCENT_SUCCESS),
                        )));
                    } else {
                        // Line changed
                        result.push(Line::from(Span::styled(
                            format!("- {}", prev_line),
                            Style::default().fg(theme::ACCENT_ERROR),
                        )));
                        result.push(Line::from(Span::styled(
                            format!("+ {}", curr),
                            Style::default().fg(theme::ACCENT_SUCCESS),
                        )));
                    }
                } else {
                    // Unchanged line
                    result.push(Line::from(Span::styled(
                        format!("  {}", curr),
                        Style::default().fg(theme::TEXT_MUTED),
                    )));
                }
            }

            result
        }
        None => {
            // No previous version available
            vec![
                Line::from(""),
                Line::from(Span::styled(
                    "No previous version available for comparison.".to_string(),
                    Style::default().fg(theme::TEXT_MUTED),
                )),
                Line::from(""),
                Line::from(Span::styled(
                    "This is the first version of this report.".to_string(),
                    Style::default().fg(theme::TEXT_MUTED),
                )),
            ]
        }
    }
}

fn render_chat_sidebar(
    f: &mut Frame,
    app: &App,
    state: &ReportTabState,
    author_name: &str,
    area: Rect,
) {
    let is_focused = state.focus == ReportTabFocus::Chat;
    let border_style = if is_focused {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::BORDER_INACTIVE)
    };

    // Split into header, messages, and input
    let chunks = Layout::vertical([
        Constraint::Length(2), // Header
        Constraint::Min(0),    // Messages
        Constraint::Length(3), // Input
    ])
    .split(area);

    // Header
    let header = Paragraph::new(vec![Line::from(vec![
        Span::styled("  Chat with ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("@{}", author_name),
            Style::default().fg(theme::ACCENT_PRIMARY),
        ),
    ])])
    .block(
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_style(border_style),
    );
    f.render_widget(header, chunks[0]);

    // Messages area (placeholder for now - will show conversation with author)
    let messages = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "  Start a conversation about this report...",
            Style::default().fg(theme::TEXT_MUTED),
        )),
    ])
    .block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .border_style(border_style),
    );
    f.render_widget(messages, chunks[1]);

    // Input area
    let input_text = &state.chat_editor.text;
    let input_hint = if input_text.is_empty() {
        "Type a message..."
    } else {
        ""
    };

    let display_text = if input_text.is_empty() {
        input_hint.to_string()
    } else {
        input_text.to_string()
    };

    let input_style = if input_text.is_empty() {
        Style::default().fg(theme::TEXT_MUTED)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    let input = Paragraph::new(display_text).style(input_style).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(Span::styled(
                " Message ",
                Style::default().fg(theme::TEXT_MUTED),
            )),
    );
    f.render_widget(input, chunks[2]);
}

fn render_help_bar(f: &mut Frame, state: &ReportTabState, area: Rect) {
    let hints = match state.focus {
        ReportTabFocus::Content => "j/k: scroll · Tab: focus chat · d: toggle diff · q/Esc: close",
        ReportTabFocus::Chat => "Tab: focus content · Enter: send · q/Esc: close",
    };

    let help = Paragraph::new(format!("  {}", hints)).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(help, area);
}
