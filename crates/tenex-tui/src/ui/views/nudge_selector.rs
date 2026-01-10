use crate::ui::components::{
    modal_area, render_modal_background, render_modal_header, render_modal_search, ModalSize,
};
use crate::ui::modal::NudgeSelectorState;
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
    Frame,
};

/// Render the nudge selector modal
pub fn render_nudge_selector(f: &mut Frame, app: &App, area: Rect, state: &NudgeSelectorState) {
    let size = ModalSize {
        max_width: 70,
        height_percent: 0.7,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    let inner_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + 1,
        popup_area.width.saturating_sub(4),
        popup_area.height.saturating_sub(3),
    );

    // Header with selection count
    let selected_count = state.selected_nudge_ids.len();
    let title = if selected_count > 0 {
        format!("Select Nudges ({} selected)", selected_count)
    } else {
        "Select Nudges".to_string()
    };
    let remaining = render_modal_header(f, inner_area, &title, "esc");

    // Search
    let remaining = render_modal_search(f, remaining, &state.selector.filter, "Search nudges...");

    // Get filtered nudges
    let nudges = app.filtered_nudges();

    // List area
    let list_area = Rect::new(
        remaining.x,
        remaining.y + 1,
        remaining.width,
        remaining.height.saturating_sub(3),
    );

    if nudges.is_empty() {
        let msg = if state.selector.filter.is_empty() {
            "No nudges available."
        } else {
            "No nudges match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state.selector.index.min(nudges.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let items: Vec<ListItem> = nudges
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, nudge)| {
                let is_cursor = i == selected_index;
                let is_selected = state.selected_nudge_ids.contains(&nudge.id);
                let border_color = theme::user_color(&nudge.pubkey);

                let mut spans = vec![];

                // Checkbox
                let checkbox = if is_selected { "[✓] " } else { "[ ] " };
                let checkbox_style = if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_MUTED)
                };
                spans.push(Span::styled(checkbox, checkbox_style));

                // Left border indicator
                if is_cursor {
                    spans.push(Span::styled("▌", Style::default().fg(border_color)));
                } else {
                    spans.push(Span::styled("│", Style::default().fg(border_color)));
                }

                // Nudge title with / prefix
                let title_style = if is_cursor {
                    Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(format!("/{}", nudge.title), title_style));

                // Description preview
                if !nudge.description.is_empty() {
                    let desc_preview = if nudge.description.len() > 40 {
                        format!(" - {}...", &nudge.description[..37])
                    } else {
                        format!(" - {}", nudge.description)
                    };
                    spans.push(Span::styled(desc_preview, Style::default().fg(theme::TEXT_MUTED)));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);

        // Show content preview of cursor-selected nudge
        if let Some(nudge) = nudges.get(selected_index) {
            let preview_area = Rect::new(
                remaining.x,
                list_area.y + list_area.height,
                remaining.width,
                2,
            );
            let content_preview = nudge.content_preview(remaining.width as usize * 2);
            let preview = Paragraph::new(content_preview)
                .style(Style::default().fg(theme::TEXT_DIM));
            f.render_widget(preview, preview_area);
        }
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
        Span::styled(" navigate", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Space", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" toggle", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled(" confirm", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
    ];

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}
