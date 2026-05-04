use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::Style,
    widgets::Paragraph,
    Frame,
};

/// Render the agent deletion confirmation dialog (kind:24030)
pub fn render_agent_deletion_confirm(
    f: &mut Frame,
    area: Rect,
    state: &crate::ui::modal::AgentDeletionState,
) {
    use crate::ui::components::{render_modal_items, Modal, ModalItem, ModalSize};
    use crate::ui::modal::AgentDeletionScope;

    let title = "Delete Agent";

    let popup_area = Modal::new(title)
        .size(ModalSize {
            max_width: 60,
            height_percent: 0.35,
        })
        .render(f, area, |f, content_area| {
            let warning = Paragraph::new(format!(
                "Publish deletion event for agent '{}'?\n\nThis publishes a kind:24030 event to\nNostr relays.",
                state.agent_name
            ))
            .style(Style::default().fg(theme::ACCENT_WARNING));
            f.render_widget(
                warning,
                Rect::new(content_area.x, content_area.y, content_area.width, 4),
            );

            let scope_label = match state.scope {
                AgentDeletionScope::Project => "Scope: [Project] / Global",
                AgentDeletionScope::Global => "Scope: Project / [Global]",
            };
            let scope_text = Paragraph::new(scope_label)
                .style(Style::default().fg(theme::TEXT_PRIMARY));
            f.render_widget(
                scope_text,
                Rect::new(content_area.x, content_area.y + 5, content_area.width, 1),
            );

            let actions_area = Rect::new(
                content_area.x,
                content_area.y + 7,
                content_area.width,
                content_area.height.saturating_sub(7),
            );

            let items = vec![
                ModalItem::new("Cancel")
                    .with_shortcut("Esc")
                    .selected(state.selected_index == 0),
                ModalItem::new("Delete")
                    .with_shortcut("Enter")
                    .selected(state.selected_index == 1),
            ];

            render_modal_items(f, actions_area, &items);
        });

    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let hints = Paragraph::new("↑↓ select · Tab toggle scope · Enter confirm · Esc cancel")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}
