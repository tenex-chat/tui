//! Nudge delete confirmation dialog

use crate::ui::components::{Modal, ModalSize, render_modal_items, ModalItem};
use crate::ui::modal::NudgeDeleteConfirmState;
use crate::ui::theme;
use crate::ui::App;
use ratatui::{
    layout::Rect,
    style::Style,
    widgets::Paragraph,
    Frame,
};

/// Render the nudge delete confirmation dialog
pub fn render_nudge_delete_confirm(f: &mut Frame, app: &App, area: Rect, state: &NudgeDeleteConfirmState) {
    let data_store = app.data_store.borrow();
    let nudge = data_store.nudges.get(&state.nudge_id);

    let nudge_title = nudge.map(|n| n.title.as_str()).unwrap_or("Unknown");
    let title = "Delete Nudge";

    let popup_area = Modal::new(title)
        .size(ModalSize {
            max_width: 50,
            height_percent: 0.3,
        })
        .render(f, area, |f, content_area| {
            // Warning message
            let warning = Paragraph::new(format!(
                "Are you sure you want to delete '/{}' ?\n\nThis action cannot be undone.",
                nudge_title
            ))
            .style(Style::default().fg(theme::ACCENT_WARNING));
            f.render_widget(warning, Rect::new(content_area.x, content_area.y, content_area.width, 3));

            // Action buttons
            let actions_area = Rect::new(
                content_area.x,
                content_area.y + 4,
                content_area.width,
                content_area.height.saturating_sub(4),
            );

            let items = vec![
                ModalItem::new("Cancel")
                    .with_shortcut("Esc")
                    .selected(state.selected_index == 0),
                ModalItem::new("Delete")
                    .with_shortcut("d")
                    .selected(state.selected_index == 1),
            ];

            render_modal_items(f, actions_area, &items);
        });

    // Hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let hints = Paragraph::new("↑↓ navigate · Enter confirm · Esc cancel")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}
