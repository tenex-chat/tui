//! History search modal view (Ctrl+R) for searching previous messages.

use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::HistorySearchState;
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph, Wrap},
    Frame,
};

/// Render the history search modal with split layout:
/// - Top section: List of matching messages (40% of space)
/// - Bottom section: Full preview of selected message (60% of space)
///
/// Layout reserves space for:
/// - 1 line for scroll indicator row (at top)
/// - 1 line for separator between panes (in split mode)
/// - 1 line for help text (at bottom)
/// Falls back to single-pane list-only view when terminal is too small.
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

    // Use a larger modal for the split view
    let (_popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 110,
            height_percent: 0.85,
        })
        .search(&state.query, "Type to search your messages...")
        .render_frame(f, area);

    // Calculate split: 40% for list, 60% for preview
    // Reserve space for:
    // - 1 line for scroll indicator row (top)
    // - 1 line for separator (between list and preview in split mode)
    // - 1 line for help text (bottom)
    // Total reserved: 3 lines in split mode, 2 lines in single-pane mode
    let reserved_for_split = 3u16; // indicator + separator + help
    let reserved_for_single = 2u16; // indicator + help
    let available_height = content_area.height.saturating_sub(reserved_for_split);

    // Small terminal fallback: if we can't fit at least 3 lines each for
    // list and preview, use single-pane list-only mode
    let use_single_pane = available_height < 6;

    let (list_height, preview_height) = if use_single_pane {
        // Single pane: all space goes to list (minus indicator + help = 2 lines)
        let list_h = content_area.height.saturating_sub(reserved_for_single).max(1);
        (list_h, 0u16)
    } else {
        // Split pane: proper 40/60 split of available space (after reservations)
        let list_h = (available_height * 40 / 100).max(3);
        let preview_h = available_height.saturating_sub(list_h);
        (list_h, preview_h)
    };

    // Scroll indicator occupies first row (content_area.y)
    // List area starts after indicator row
    let list_area = Rect::new(
        content_area.x,
        content_area.y + 1, // +1 for scroll indicator row
        content_area.width,
        list_height,
    );

    // Preview area at bottom (below list + 1 line separator) - only used in split mode
    // Layout: [indicator][list...][separator][preview...][help]
    let preview_area = if !use_single_pane && preview_height > 0 {
        Rect::new(
            content_area.x,
            content_area.y + 1 + list_height + 1, // +1 indicator, +1 separator
            content_area.width,
            preview_height,
        )
    } else {
        Rect::default()
    };

    if results.is_empty() {
        let msg = if state.query.is_empty() {
            "Your recent messages will appear here.\nType to search through your previous messages."
        } else {
            "No messages match your search."
        };
        let empty_msg = Paragraph::new(msg)
            .style(Style::default().fg(theme::TEXT_MUTED))
            .wrap(Wrap { trim: true });
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state.selected_index.min(results.len().saturating_sub(1));

        // Calculate scroll offset to keep selected item visible
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

                // Content preview (first line, truncated for list view)
                let content_preview: String = entry
                    .content
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take((content_area.width as usize).saturating_sub(18))
                    .collect();

                let content_style = if is_selected {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(content_preview, content_style));

                // Timestamp (relative time) - right-aligned feel
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
            let indicator = format!(" {}/{} ", selected_index + 1, results.len());
            let indicator_area = Rect::new(
                content_area.x + content_area.width.saturating_sub(indicator.len() as u16 + 2),
                content_area.y,
                indicator.len() as u16 + 2,
                1,
            );
            let indicator_widget =
                Paragraph::new(indicator).style(Style::default().fg(theme::TEXT_MUTED));
            f.render_widget(indicator_widget, indicator_area);
        }

        // Only render separator and preview in split-pane mode
        if !use_single_pane && preview_height > 0 {
            // Separator line with "Preview" label (shows the title inline with separator)
            let separator_area = Rect::new(
                content_area.x,
                content_area.y + 1 + list_height,
                content_area.width,
                1,
            );
            // Build separator with embedded "Preview" label
            let label = " Preview ";
            let left_dash_count = 2usize;
            let right_dash_count =
                (content_area.width as usize).saturating_sub(left_dash_count + label.len());
            let separator_line = Line::from(vec![
                Span::styled(
                    "─".repeat(left_dash_count),
                    Style::default().fg(theme::TEXT_MUTED),
                ),
                Span::styled(
                    label,
                    Style::default()
                        .fg(theme::TEXT_MUTED)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    "─".repeat(right_dash_count),
                    Style::default().fg(theme::TEXT_MUTED),
                ),
            ]);
            f.render_widget(Paragraph::new(separator_line), separator_area);

            // Preview of selected message (full content, wrapped)
            if let Some(selected) = results.get(selected_index) {
                let preview_text = Paragraph::new(selected.content.as_str())
                    .style(Style::default().fg(theme::TEXT_PRIMARY))
                    .wrap(Wrap { trim: false });
                f.render_widget(preview_text, preview_area);
            }
        }
    }

    // Help text at bottom
    let help_area = Rect::new(
        content_area.x,
        content_area.y + content_area.height.saturating_sub(1),
        content_area.width,
        1,
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
        Span::styled(
            format!(" {} • ", scope_toggle),
            Style::default().fg(theme::TEXT_MUTED),
        ),
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
