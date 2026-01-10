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
    widgets::{Block, Borders, Paragraph},
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

    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(state.content_scroll)
        .take(content_area.height as usize)
        .collect();

    let is_focused = state.focus == ReportViewerFocus::Content;
    let border_style = if is_focused {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::BORDER_INACTIVE)
    };

    let content = Paragraph::new(visible_lines)
        .block(Block::default()
            .borders(Borders::LEFT)
            .border_style(border_style));

    f.render_widget(content, content_area);
}

fn render_diff_view(current: &str, previous: Option<&str>) -> Vec<Line<'static>> {
    let Some(previous) = previous else {
        return vec![
            Line::from(Span::styled(
                "No previous version available for diff".to_string(),
                Style::default().fg(theme::TEXT_MUTED),
            ))
        ];
    };

    let mut lines = Vec::new();
    let current_lines: Vec<&str> = current.lines().collect();
    let previous_lines: Vec<&str> = previous.lines().collect();

    // Simple line-by-line diff
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
            "No changes from previous version".to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        )));
    }

    lines
}

fn render_threads_sidebar(f: &mut Frame, app: &App, state: &ReportViewerState, area: Rect) {
    let is_focused = state.focus == ReportViewerFocus::Threads;
    let border_style = if is_focused {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::BORDER_INACTIVE)
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

fn get_document_threads(_app: &App, _report: &tenex_core::models::Report) -> Vec<tenex_core::models::Thread> {
    // Get threads that reference this document via a-tag
    // For now, return empty - will be populated when we add document thread support (Task 9)
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
        .border_style(Style::default().fg(theme::BORDER_INACTIVE));
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
