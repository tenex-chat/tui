use crate::ui::format::truncate_with_ellipsis;
use crate::ui::theme;
use crate::ui::todo::{TodoState, TodoStatus};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the todo sidebar on the right side of the chat.
pub fn render_todo_sidebar(f: &mut Frame, todo_state: &TodoState, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Header with count
    let completed = todo_state.completed_count();
    let total = todo_state.items.len();
    lines.push(Line::from(vec![
        Span::styled("Todo List ", theme::text_bold()),
        Span::styled(format!("{}/{}", completed, total), Style::default().fg(theme::TEXT_MUTED)),
    ]));

    // Progress bar
    let progress_width = (area.width as usize).saturating_sub(4);
    let filled = if total > 0 { (completed * progress_width) / total } else { 0 };
    let empty_bar = progress_width.saturating_sub(filled);
    lines.push(Line::from(vec![
        Span::styled("━".repeat(filled), Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled("━".repeat(empty_bar), Style::default().fg(theme::PROGRESS_EMPTY)),
    ]));
    lines.push(Line::from(""));

    // Active task highlight
    if let Some(active) = todo_state.in_progress_item() {
        lines.push(Line::from(Span::styled(
            "In Progress",
            theme::todo_in_progress(),
        )));
        lines.push(Line::from(Span::styled(
            format!(
                "  {}",
                truncate_with_ellipsis(&active.title, (area.width as usize).saturating_sub(4))
            ),
            theme::text_primary(),
        )));
        if let Some(ref desc) = active.description {
            lines.push(Line::from(Span::styled(
                format!(
                    "  {}",
                    truncate_with_ellipsis(desc, (area.width as usize).saturating_sub(4))
                ),
                theme::text_muted(),
            )));
        }
        lines.push(Line::from(""));
    }

    // Todo items
    for item in &todo_state.items {
        let (icon, icon_style) = match item.status {
            TodoStatus::Done => ("✓", theme::todo_done()),
            TodoStatus::InProgress => ("◐", theme::todo_in_progress()),
            TodoStatus::Pending => ("○", theme::todo_pending()),
        };

        let title_style = if item.status == TodoStatus::Done {
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::CROSSED_OUT)
        } else {
            theme::text_primary()
        };

        let title = truncate_with_ellipsis(&item.title, (area.width as usize).saturating_sub(4));
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", icon), icon_style),
            Span::styled(title, title_style),
        ]));
    }

    let sidebar = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(theme::border_inactive()),
        )
        .style(Style::default().bg(theme::BG_SIDEBAR));

    f.render_widget(sidebar, area);
}
