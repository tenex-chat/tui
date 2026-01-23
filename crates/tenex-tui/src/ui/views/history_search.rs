//! History search modal view (Ctrl+R) for searching previous messages.

use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::HistorySearchState;
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
    Frame,
};

/// Render the history search modal
pub fn render_history_search(f: &mut Frame, area: Rect, state: &HistorySearchState) {
    let results = &state.results;
    let scope = if state.all_projects {
        "all projects"
    } else {
        "current project"
    };
    let title = if results.is_empty() {
        format!("Search History ({})", scope)
    } else {
        format!("Search History ({} results, {})", results.len(), scope)
    };

    let (_popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 90,
            height_percent: 0.7,
        })
        .search(&state.query, "Type to search your messages...")
        .render_frame(f, area);

    // List area
    let list_area = Rect::new(
        content_area.x,
        content_area.y + 1,
        content_area.width,
        content_area.height.saturating_sub(5),
    );

    if results.is_empty() {
        let msg = if state.query.is_empty() {
            "Type to search through your previous messages."
        } else {
            "No messages match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state.selected_index.min(results.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let items: Vec<ListItem> = results
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, entry)| {
                let is_selected = i == selected_index;

                let mut spans = vec![];

                // Selection indicator
                if is_selected {
                    spans.push(Span::styled("▌ ", Style::default().fg(theme::ACCENT_PRIMARY)));
                } else {
                    spans.push(Span::styled("  ", Style::default()));
                }

                // Content preview (first line, truncated)
                let content_preview: String = entry
                    .content
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take((content_area.width as usize).saturating_sub(20))
                    .collect();

                let content_style = if is_selected {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(content_preview, content_style));

                // Timestamp (relative time)
                let time_ago = format_relative_time(entry.created_at);
                let time_style = Style::default().fg(theme::TEXT_MUTED);
                spans.push(Span::styled(format!(" ({})", time_ago), time_style));

                let line = Line::from(spans);
                let style = if is_selected {
                    Style::default().bg(theme::BG_SELECTED)
                } else {
                    Style::default()
                };

                ListItem::new(line).style(style)
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);

        // Show scroll indicator if needed
        if results.len() > visible_height {
            let indicator = format!(
                " {}/{} ",
                selected_index + 1,
                results.len()
            );
            let indicator_area = Rect::new(
                content_area.x + content_area.width.saturating_sub(indicator.len() as u16 + 2),
                content_area.y,
                indicator.len() as u16 + 2,
                1,
            );
            let indicator_widget = Paragraph::new(indicator)
                .style(Style::default().fg(theme::TEXT_MUTED));
            f.render_widget(indicator_widget, indicator_area);
        }
    }

    // Help text at bottom
    let help_area = Rect::new(
        content_area.x,
        content_area.y + content_area.height.saturating_sub(3),
        content_area.width,
        2,
    );
    let scope_toggle = if state.all_projects {
        "current project"
    } else {
        "all projects"
    };
    let help_text = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" use • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Tab", Style::default().fg(theme::ACCENT_SPECIAL)),
        Span::styled(format!(" {} • ", scope_toggle), Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("↑↓", Style::default().fg(theme::TEXT_PRIMARY)),
        Span::styled(" navigate • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)),
    ]);
    f.render_widget(Paragraph::new(help_text), help_area);
}

/// Format a unix timestamp as relative time (e.g., "5m ago", "2h ago", "3d ago")
fn format_relative_time(created_at: u64) -> String {
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let diff = now.saturating_sub(created_at);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else {
        format!("{}w ago", diff / 604800)
    }
}
