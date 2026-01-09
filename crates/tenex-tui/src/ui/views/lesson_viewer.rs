use crate::models::Lesson;
use crate::ui::markdown::render_markdown;
use crate::ui::{card, theme, App};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};

/// Render the lesson viewer - full-screen pager-style interface
pub fn render_lesson_viewer(f: &mut Frame, app: &App, area: Rect, lesson: &Lesson) {
    // Clear the background
    f.render_widget(Clear, area);

    // Layout: Header | Content | Footer
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header (title + metadata)
        Constraint::Min(0),    // Content (scrollable)
        Constraint::Length(2), // Footer (navigation help)
    ])
    .split(area);

    // Render header
    render_header(f, app, lesson, chunks[0]);

    // Render content
    render_content(f, app, lesson, chunks[1]);

    // Render footer
    render_footer(f, app, lesson, chunks[2]);
}

fn render_header(f: &mut Frame, app: &App, lesson: &Lesson, area: Rect) {
    let author_name = app.data_store.borrow().get_profile_name(&lesson.pubkey);

    let mut title_line = vec![
        Span::styled("📚 ", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(&lesson.title, Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)),
    ];

    if let Some(ref category) = lesson.category {
        title_line.push(Span::styled(" | ", Style::default().fg(theme::TEXT_MUTED)));
        title_line.push(Span::styled(category, Style::default().fg(theme::ACCENT_SPECIAL)));
    }

    let meta_line = vec![
        Span::styled("by ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&author_name, Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled(card::META_SEPARATOR, Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(lesson.reading_time(), Style::default().fg(theme::ACCENT_PRIMARY)),
    ];

    let header = Paragraph::new(vec![
        Line::from(title_line),
        Line::from(meta_line),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
    );

    f.render_widget(header, area);
}

fn render_content(f: &mut Frame, app: &App, lesson: &Lesson, area: Rect) {
    let sections = lesson.sections();
    let current_section = app.lesson_viewer_section.min(sections.len().saturating_sub(1));

    let (section_name, section_content) = sections.get(current_section).unwrap_or(&("Summary", ""));

    // Render markdown for the current section
    let lines = render_markdown(section_content);

    // Calculate visible height and handle scrolling
    let content_height = lines.len();
    let visible_height = area.height.saturating_sub(4) as usize; // Account for borders and section header

    // Clamp scroll offset
    let max_scroll = content_height.saturating_sub(visible_height);
    let scroll_offset = app.scroll_offset.min(max_scroll);

    // Slice the visible content
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll_offset)
        .take(visible_height)
        .collect();

    // Section header
    let mut section_header_spans = vec![
        Span::styled(
            format!("{}. ", current_section + 1),
            Style::default().fg(theme::ACCENT_WARNING)
        ),
        Span::styled(
            *section_name,
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
        ),
    ];

    // Add scroll indicator if content is scrollable
    if content_height > visible_height {
        let scroll_percent = if max_scroll > 0 {
            (scroll_offset * 100) / max_scroll
        } else {
            0
        };
        section_header_spans.push(Span::styled(
            format!(" ({}%)", scroll_percent),
            Style::default().fg(theme::TEXT_MUTED)
        ));
    }

    let mut content_lines = vec![
        Line::from(""),
        Line::from(section_header_spans),
        Line::from(""),
    ];
    content_lines.extend(visible_lines);

    let content = Paragraph::new(content_lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
        );

    f.render_widget(content, area);
}

fn render_footer(f: &mut Frame, app: &App, lesson: &Lesson, area: Rect) {
    let sections = lesson.sections();
    let section_count = sections.len();
    let current_section = app.lesson_viewer_section.min(section_count.saturating_sub(1));

    // Build section indicators (1-5 for navigation)
    let mut section_spans = vec![];
    for (i, (name, _)) in sections.iter().enumerate().take(5) {
        if i > 0 {
            section_spans.push(Span::styled(" ", Style::default()));
        }

        let style = if i == current_section {
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        section_spans.push(Span::styled(format!("{}", i + 1), style));
        section_spans.push(Span::styled(":", style));
        section_spans.push(Span::styled(
            if name.len() > 8 { &name[..8] } else { name },
            style
        ));
    }

    let mut help_spans = vec![
        Span::styled("Sections: ", Style::default().fg(theme::TEXT_MUTED)),
    ];
    help_spans.extend(section_spans);
    help_spans.extend(vec![
        Span::styled(" | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("j/k", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" scroll | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" back", Style::default().fg(theme::TEXT_MUTED)),
    ]);

    let footer = Paragraph::new(vec![Line::from(help_spans)])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
        );

    f.render_widget(footer, area);
}
