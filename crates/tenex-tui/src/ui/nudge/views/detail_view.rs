//! Nudge detail view - read-only view of a nudge

use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::NudgeDetailState;
use crate::ui::theme;
use crate::ui::App;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the nudge detail view
pub fn render_nudge_detail(f: &mut Frame, app: &App, area: Rect, state: &NudgeDetailState) {
    let data_store = app.data_store.borrow();
    let nudge = match data_store.nudges.get(&state.nudge_id) {
        Some(n) => n,
        None => {
            // Nudge not found - render error
            let (_, content_area) = Modal::new("Nudge Not Found")
                .size(ModalSize {
                    max_width: 60,
                    height_percent: 0.3,
                })
                .render_frame(f, area);

            let msg = Paragraph::new("The requested nudge could not be found.")
                .style(Style::default().fg(theme::ACCENT_ERROR));
            f.render_widget(msg, content_area);
            return;
        }
    };

    let title = format!("/{}", nudge.title);

    let (popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 80,
            height_percent: 0.8,
        })
        .render_frame(f, area);

    let mut y = content_area.y;

    // Description
    if !nudge.description.is_empty() {
        let desc = Paragraph::new(&*nudge.description)
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(desc, Rect::new(content_area.x, y, content_area.width, 1));
        y += 2;
    }

    // Metadata row: author, created_at, hashtags
    let author_short = if nudge.pubkey.len() > 16 {
        format!("{}...{}", &nudge.pubkey[..8], &nudge.pubkey[nudge.pubkey.len()-8..])
    } else {
        nudge.pubkey.clone()
    };

    let created_at = format_timestamp(nudge.created_at);

    let meta_spans = vec![
        Span::styled("by ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&author_short, Style::default().fg(theme::user_color(&nudge.pubkey))),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(created_at, Style::default().fg(theme::TEXT_MUTED)),
    ];
    let meta_line = Paragraph::new(Line::from(meta_spans));
    f.render_widget(meta_line, Rect::new(content_area.x, y, content_area.width, 1));
    y += 1;

    // Hashtags
    if !nudge.hashtags.is_empty() {
        let tags: Vec<Span> = nudge
            .hashtags
            .iter()
            .flat_map(|t| {
                vec![
                    Span::styled(format!("#{}", t), Style::default().fg(theme::ACCENT_WARNING)),
                    Span::styled(" ", Style::default()),
                ]
            })
            .collect();
        let tags_line = Paragraph::new(Line::from(tags));
        f.render_widget(tags_line, Rect::new(content_area.x, y, content_area.width, 1));
        y += 2;
    } else {
        y += 1;
    }

    // Content section
    let content_label = Paragraph::new("Content")
        .style(Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD));
    f.render_widget(content_label, Rect::new(content_area.x, y, content_area.width, 1));
    y += 1;

    // Content in a bordered box
    let remaining_height = content_area.height.saturating_sub(y - content_area.y + 3);
    let content_box_area = Rect::new(content_area.x, y, content_area.width, remaining_height);

    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE));

    let content_inner = content_block.inner(content_box_area);
    f.render_widget(content_block, content_box_area);

    // Render content with scroll
    let lines: Vec<&str> = nudge.content.lines().collect();
    let visible_height = content_inner.height as usize;

    for (i, line) in lines
        .iter()
        .skip(state.scroll_offset)
        .take(visible_height)
        .enumerate()
    {
        let line_para = Paragraph::new(*line).style(Style::default().fg(theme::TEXT_PRIMARY));
        f.render_widget(
            line_para,
            Rect::new(content_inner.x, content_inner.y + i as u16, content_inner.width, 1),
        );
    }

    // Scroll indicator
    if lines.len() > visible_height {
        let indicator = format!(
            "{}/{}",
            state.scroll_offset + 1,
            lines.len().saturating_sub(visible_height) + 1
        );
        let indicator_para = Paragraph::new(indicator).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(
            indicator_para,
            Rect::new(
                content_inner.x + content_inner.width.saturating_sub(10),
                content_box_area.y + content_box_area.height.saturating_sub(1),
                10,
                1,
            ),
        );
    }

    // Hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let hint_spans = vec![
        Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" scroll", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("e", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" edit", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("d", Style::default().fg(theme::ACCENT_ERROR)),
        Span::styled(" delete", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("c", Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled(" copy ID", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)),
    ];

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}

/// Format a Unix timestamp to a human-readable string
fn format_timestamp(timestamp: u64) -> String {
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let diff = now.saturating_sub(timestamp);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else {
        // Format as date
        let secs_since_epoch = Duration::from_secs(timestamp);
        let datetime = UNIX_EPOCH + secs_since_epoch;
        if let Ok(duration) = datetime.duration_since(UNIX_EPOCH) {
            let days = duration.as_secs() / 86400;
            let years = 1970 + days / 365;
            let remaining_days = days % 365;
            let months = remaining_days / 30 + 1;
            let day = remaining_days % 30 + 1;
            format!("{:04}-{:02}-{:02}", years, months, day)
        } else {
            "unknown".to_string()
        }
    }
}
