pub mod agent_browser;
pub mod ask_modal;
pub mod chat;
pub mod command_palette;
pub mod create_agent;
pub mod create_project;
pub mod debug_stats;
pub mod draft_navigator;
pub mod history_search;
mod home_helpers;
pub mod home;
pub mod inline_ask;
pub mod lesson_viewer;
pub mod login;
pub mod nudge_selector;
pub mod project_settings;
pub mod report_viewer;

pub use agent_browser::render_agent_browser;
pub use ask_modal::render_ask_modal;
pub use chat::render_chat;
pub use command_palette::render_command_palette;
pub use create_agent::render_create_agent;
pub use create_project::render_create_project;
pub use debug_stats::render_debug_stats;
pub use draft_navigator::render_draft_navigator;
pub use history_search::render_history_search;
pub use home::render_home;
pub use inline_ask::render_inline_ask_lines;
pub use lesson_viewer::render_lesson_viewer;
pub use nudge_selector::render_nudge_selector;
pub use project_settings::{render_project_settings, available_agent_count, get_agent_id_at_index};
pub use report_viewer::render_report_viewer;

use crate::ui::components::{render_modal_items, Modal, ModalItem, ModalSize};
use crate::ui::modal::{BackendApprovalAction, BackendApprovalState};
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::Style,
    widgets::Paragraph,
    Frame,
};

/// Render the backend approval modal
pub fn render_backend_approval_modal(
    f: &mut Frame,
    area: Rect,
    state: &BackendApprovalState,
) {
    let actions = BackendApprovalAction::ALL;
    let content_height = (actions.len() + 4) as u16;
    let total_height = content_height + 6;
    let height_percent = (total_height as f32 / area.height as f32).min(0.5);

    // Truncate pubkey for display
    let short_pubkey = if state.backend_pubkey.len() > 16 {
        format!("{}...{}", &state.backend_pubkey[..8], &state.backend_pubkey[state.backend_pubkey.len()-8..])
    } else {
        state.backend_pubkey.clone()
    };

    let title = "Unknown Backend";

    let items: Vec<ModalItem> = actions
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let is_selected = i == state.selected_index;
            ModalItem::new(action.label())
                .with_shortcut(action.hotkey().to_string())
                .selected(is_selected)
        })
        .collect();

    let popup_area = Modal::new(title)
        .size(ModalSize {
            max_width: 50,
            height_percent,
        })
        .render(f, area, |f, content_area| {
            // Render description
            let desc_area = Rect::new(content_area.x, content_area.y, content_area.width, 2);
            let desc = Paragraph::new(format!(
                "Backend {} wants to send status updates.\nDo you trust this backend?",
                short_pubkey
            ))
            .style(Style::default().fg(theme::TEXT_MUTED))
            .wrap(ratatui::widgets::Wrap { trim: true });
            f.render_widget(desc, desc_area);

            // Render actions below description
            let actions_area = Rect::new(
                content_area.x,
                content_area.y + 3,
                content_area.width,
                content_area.height.saturating_sub(3),
            );
            render_modal_items(f, actions_area, &items);
        });

    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("a approve · r reject · b block · ↑↓ navigate · esc dismiss")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}
