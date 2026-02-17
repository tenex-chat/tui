//! TTS Control Tab View
//!
//! Renders the TTS control tab which allows users to:
//! - View the TTS queue (played, current, pending items)
//! - Control playback (pause/play with Space)
//! - Navigate through queue items (j/k)
//! - Open source conversations (Enter)

use crate::ui::format::format_relative_time;
use crate::ui::state::{TTSControlState, TTSQueueItem, TTSQueueItemStatus};
use crate::ui::{card, theme, App};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the TTS control tab content
pub fn render_tts_control(f: &mut Frame, app: &App, area: Rect) {
    // Get TTS state from the active tab
    let tts_state = app.tabs.active_tab().and_then(|t| t.tts_state.as_ref());

    let Some(state) = tts_state else {
        render_no_tts_state(f, area);
        return;
    };

    // Layout: Header | Queue List | Help Bar
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header
        Constraint::Min(0),    // Queue list
        Constraint::Length(2), // Help bar
    ])
    .split(area);

    render_header(f, app, state, chunks[0]);
    render_queue(f, app, state, chunks[1]);
    render_help_bar(f, state, chunks[2]);
}

fn render_no_tts_state(f: &mut Frame, area: Rect) {
    let message = Paragraph::new(vec![
        Line::from(""),
        Line::from(Span::styled(
            "No TTS state available",
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

fn render_header(f: &mut Frame, _app: &App, state: &TTSControlState, area: Rect) {
    let playback_status = if state.is_paused {
        Span::styled(
            " PAUSED ",
            Style::default()
                .fg(theme::ACCENT_WARNING)
                .add_modifier(Modifier::BOLD),
        )
    } else if state.playing_index.is_some() {
        Span::styled(
            " PLAYING ",
            Style::default()
                .fg(theme::ACCENT_SUCCESS)
                .add_modifier(Modifier::BOLD),
        )
    } else {
        Span::styled(" STOPPED ", Style::default().fg(theme::TEXT_MUTED))
    };

    let queue_info = format!("{} items in queue", state.queue.len());

    let pending_count = state
        .queue
        .iter()
        .filter(|i| {
            matches!(
                i.status,
                TTSQueueItemStatus::Pending | TTSQueueItemStatus::Ready
            )
        })
        .count();
    let completed_count = state
        .queue
        .iter()
        .filter(|i| i.status == TTSQueueItemStatus::Completed)
        .count();

    let stats = format!("{} completed, {} pending", completed_count, pending_count);

    let header = Paragraph::new(vec![
        Line::from(vec![
            Span::styled(
                "  TTS Control  ",
                Style::default()
                    .fg(theme::TEXT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            playback_status,
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(queue_info, Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("  ·  ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(stats, Style::default().fg(theme::TEXT_MUTED)),
        ]),
    ]);

    f.render_widget(header, area);
}

fn render_queue(f: &mut Frame, app: &App, state: &TTSControlState, area: Rect) {
    if state.queue.is_empty() {
        let empty_msg = Paragraph::new(vec![
            Line::from(""),
            Line::from(Span::styled(
                "  No items in TTS queue",
                Style::default().fg(theme::TEXT_MUTED),
            )),
            Line::from(""),
            Line::from(Span::styled(
                "  TTS items will appear here when audio is generated.",
                Style::default().fg(theme::TEXT_MUTED),
            )),
        ])
        .block(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(theme::BORDER_INACTIVE)),
        );
        f.render_widget(empty_msg, area);
        return;
    }

    let inner_area = Rect::new(
        area.x + 2,
        area.y,
        area.width.saturating_sub(4),
        area.height,
    );

    let mut lines: Vec<Line> = Vec::new();
    let data_store = app.data_store.borrow();

    for (i, item) in state.queue.iter().enumerate() {
        let is_selected = i == state.selected_index;
        let is_playing = state.playing_index == Some(i);

        // Status indicator
        let (status_icon, status_style) = match item.status {
            TTSQueueItemStatus::Pending => ("○", Style::default().fg(theme::TEXT_MUTED)),
            TTSQueueItemStatus::Generating => ("◐", Style::default().fg(theme::ACCENT_WARNING)),
            TTSQueueItemStatus::Ready => ("●", Style::default().fg(theme::ACCENT_PRIMARY)),
            TTSQueueItemStatus::Playing => ("▶", Style::default().fg(theme::ACCENT_SUCCESS)),
            TTSQueueItemStatus::Completed => ("✓", Style::default().fg(theme::TEXT_MUTED)),
            TTSQueueItemStatus::Failed => ("✗", Style::default().fg(theme::ACCENT_ERROR)),
        };

        // Selection indicator
        let bullet = if is_selected { card::BULLET } else { " " };
        let bullet_style = Style::default().fg(theme::ACCENT_PRIMARY);

        // Text style based on status
        let text_style = if is_playing {
            Style::default()
                .fg(theme::ACCENT_SUCCESS)
                .add_modifier(Modifier::BOLD)
        } else if item.status == TTSQueueItemStatus::Completed {
            Style::default().fg(theme::TEXT_MUTED)
        } else if is_selected {
            Style::default().fg(theme::ACCENT_PRIMARY)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        // Main line: bullet + status + preview
        lines.push(Line::from(vec![
            Span::styled(bullet, bullet_style),
            Span::styled(status_icon, status_style),
            Span::styled(" ", Style::default()),
            Span::styled(&item.preview, text_style),
        ]));

        // Secondary line: conversation info and time
        let time_str = format_relative_time(item.queued_at);
        let conv_info = if let Some(ref conv_id) = item.conversation_id {
            // Try to get conversation title
            let short_id = if conv_id.len() > 8 {
                &conv_id[..8]
            } else {
                conv_id
            };
            format!("from conversation {}", short_id)
        } else {
            "no source".to_string()
        };

        lines.push(Line::from(vec![
            Span::styled("   ", Style::default()),
            Span::styled(conv_info, Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)),
        ]));

        // Add spacing between items
        lines.push(Line::from(""));
    }

    drop(data_store);

    // Calculate visible range based on selection
    let visible_height = inner_area.height as usize;
    let _total_lines = lines.len();
    let lines_per_item = 3; // status line + info line + spacing
    let selected_line = state.selected_index * lines_per_item;

    // Calculate scroll offset to keep selected item visible
    let scroll_offset = if selected_line + lines_per_item > visible_height {
        (selected_line + lines_per_item).saturating_sub(visible_height)
    } else {
        0
    };

    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll_offset)
        .take(visible_height)
        .collect();

    let queue_widget = Paragraph::new(visible_lines).block(
        Block::default()
            .borders(Borders::LEFT)
            .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
    );

    f.render_widget(queue_widget, inner_area);
}

fn render_help_bar(f: &mut Frame, state: &TTSControlState, area: Rect) {
    let pause_hint = if state.is_paused {
        "Space: resume"
    } else {
        "Space: pause"
    };

    let hints = format!(
        "  j/k: navigate · Enter: open conversation · {} · c: clear completed · q/Esc: close",
        pause_hint
    );

    let help = Paragraph::new(hints).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(help, area);
}
