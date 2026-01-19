//! Draft navigator modal view for viewing and restoring saved drafts.

use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::DraftNavigatorState;
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
    Frame,
};

/// Render the draft navigator modal
pub fn render_draft_navigator(f: &mut Frame, area: Rect, state: &DraftNavigatorState) {
    let drafts = state.filtered_drafts();
    let title = if drafts.is_empty() {
        "Drafts".to_string()
    } else {
        format!("Drafts ({} total)", drafts.len())
    };

    let (_popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 80,
            height_percent: 0.7,
        })
        .search(&state.filter, "Search drafts...")
        .render_frame(f, area);

    // List area
    let list_area = Rect::new(
        content_area.x,
        content_area.y + 1,
        content_area.width,
        content_area.height.saturating_sub(5),
    );

    if drafts.is_empty() {
        let msg = if state.filter.is_empty() {
            "No saved drafts. Press Ctrl+T then 's' in edit mode to save a draft."
        } else {
            "No drafts match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state.selected_index.min(drafts.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let items: Vec<ListItem> = drafts
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, draft)| {
                let is_selected = i == selected_index;

                let mut spans = vec![];

                // Selection indicator
                if is_selected {
                    spans.push(Span::styled("▌ ", Style::default().fg(theme::ACCENT_PRIMARY)));
                } else {
                    spans.push(Span::styled("  ", Style::default()));
                }

                // Draft name
                let name_style = if is_selected {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(&draft.name, name_style));

                // Preview (if there's space)
                let preview = draft.preview();
                if !preview.is_empty() && preview != draft.name {
                    let preview_style = Style::default().fg(theme::TEXT_MUTED);
                    let truncated_preview: String = preview.chars().take(40).collect();
                    spans.push(Span::styled(
                        format!(" - {}", truncated_preview),
                        preview_style,
                    ));
                }

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
        if drafts.len() > visible_height {
            let indicator = format!(
                " {}/{} ",
                selected_index + 1,
                drafts.len()
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
    let help_text = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" restore • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Ctrl+D", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" delete • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)),
    ]);
    f.render_widget(Paragraph::new(help_text), help_area);
}
