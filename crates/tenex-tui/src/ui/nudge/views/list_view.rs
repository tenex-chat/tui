//! Nudge list view - browse and manage nudges

use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::NudgeListState;
use crate::ui::theme;
use crate::ui::App;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
    Frame,
};
use tenex_core::models::Nudge;

/// Render the nudge list view
pub fn render_nudge_list(f: &mut Frame, app: &App, area: Rect, state: &NudgeListState) {
    let nudge_count = app.data_store.borrow().content.nudges.len();
    let title = format!("Nudges ({})", nudge_count);

    let (popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 80,
            height_percent: 0.8,
        })
        .search(&state.filter, "Search nudges...")
        .render_frame(f, area);

    // Get filtered nudges
    let data_store = app.data_store.borrow();
    let filtered_nudges: Vec<&Nudge> = filter_nudges(&data_store.content.nudges, &state.filter);

    // Render list or empty message
    let list_area = Rect::new(
        content_area.x,
        content_area.y + 1,
        content_area.width,
        content_area.height.saturating_sub(4),
    );

    if filtered_nudges.is_empty() {
        let msg = if state.filter.is_empty() {
            "No nudges found. Press 'n' to create one."
        } else {
            "No nudges match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        render_nudge_items(f, list_area, &filtered_nudges, state);
    }

    // Render hints at bottom
    render_hints(f, popup_area);
}

/// Filter nudges by search query
fn filter_nudges<'a>(
    nudges: &'a std::collections::HashMap<String, Nudge>,
    filter: &str,
) -> Vec<&'a Nudge> {
    let filter_lower = filter.to_lowercase();
    let mut filtered: Vec<&Nudge> = nudges
        .values()
        .filter(|n| {
            if filter.is_empty() {
                return true;
            }
            n.title.to_lowercase().contains(&filter_lower)
                || n.description.to_lowercase().contains(&filter_lower)
                || n.content.to_lowercase().contains(&filter_lower)
                || n.hashtags.iter().any(|h| h.to_lowercase().contains(&filter_lower))
        })
        .collect();

    // Sort by created_at descending (most recent first)
    filtered.sort_by(|a, b| b.created_at.cmp(&a.created_at));
    filtered
}

/// Render the list items
fn render_nudge_items(
    f: &mut Frame,
    area: Rect,
    nudges: &[&Nudge],
    state: &NudgeListState,
) {
    let visible_height = area.height as usize;
    let selected_index = state.selected_index.min(nudges.len().saturating_sub(1));

    // Calculate scroll offset
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
            let is_selected = i == selected_index;
            let border_color = theme::user_color(&nudge.pubkey);

            let mut spans = vec![];

            // Cursor indicator
            if is_selected {
                spans.push(Span::styled("▌", Style::default().fg(border_color)));
            } else {
                spans.push(Span::styled("│", Style::default().fg(border_color)));
            }

            // Title with / prefix
            let title_style = if is_selected {
                Style::default()
                    .fg(theme::ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT_PRIMARY)
            };
            spans.push(Span::styled(format!(" /{}", nudge.title), title_style));

            // Description preview
            if !nudge.description.is_empty() {
                let desc_preview = if nudge.description.len() > 35 {
                    format!(" - {}...", &nudge.description[..32])
                } else {
                    format!(" - {}", nudge.description)
                };
                spans.push(Span::styled(desc_preview, Style::default().fg(theme::TEXT_MUTED)));
            }

            // Hashtags
            if !nudge.hashtags.is_empty() {
                let tags: String = nudge
                    .hashtags
                    .iter()
                    .take(3)
                    .map(|t| format!("#{}", t))
                    .collect::<Vec<_>>()
                    .join(" ");
                spans.push(Span::styled(format!(" {}", tags), Style::default().fg(theme::ACCENT_WARNING)));
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, area);

    // Show content preview of selected nudge
    if let Some(nudge) = nudges.get(selected_index) {
        let preview_area = Rect::new(
            area.x,
            area.y + area.height.saturating_sub(2),
            area.width,
            2,
        );
        let content_preview = nudge.content_preview(area.width as usize * 2);
        let preview = Paragraph::new(content_preview).style(Style::default().fg(theme::TEXT_DIM));
        f.render_widget(preview, preview_area);
    }
}

/// Render the hints at the bottom
fn render_hints(f: &mut Frame, popup_area: Rect) {
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let hint_spans = vec![
        Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" nav", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("n", Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled(" new", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("c", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" copy", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("d", Style::default().fg(theme::ACCENT_ERROR)),
        Span::styled(" delete", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Enter", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" view", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)),
    ];

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}
