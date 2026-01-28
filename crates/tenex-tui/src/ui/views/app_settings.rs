//! App Settings modal view - global application settings accessible via comma key

use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::AppSettingsState;
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Render the app settings modal
pub fn render_app_settings(f: &mut Frame, app: &App, area: Rect, state: &AppSettingsState) {
    let (popup_area, content_area) = Modal::new("Settings")
        .size(ModalSize {
            max_width: 70,
            height_percent: 0.5,
        })
        .render_frame(f, area);

    // Content area with horizontal padding
    let remaining = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height,
    );

    // Section header: Trace Viewer
    let header_area = Rect::new(remaining.x, remaining.y, remaining.width, 1);
    let header = Paragraph::new(Line::from(vec![Span::styled(
        "Trace Viewer",
        Style::default()
            .fg(theme::ACCENT_WARNING)
            .add_modifier(Modifier::ITALIC),
    )]));
    f.render_widget(header, header_area);

    // Jaeger endpoint setting row
    let row_y = remaining.y + 2;
    let row_area = Rect::new(remaining.x, row_y, remaining.width, 3);

    let is_selected = state.selected_index == 0;

    // Left border indicator
    let border_char = if is_selected { "▌" } else { "│" };
    let border_color = if is_selected {
        theme::ACCENT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };

    let mut spans = vec![Span::styled(border_char, Style::default().fg(border_color))];

    // Label
    let label_style = if is_selected {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    spans.push(Span::styled(" Jaeger Endpoint: ", label_style));

    // Value (editable)
    if state.editing_jaeger_endpoint {
        // Show input with cursor
        let input = &state.jaeger_endpoint_input;
        spans.push(Span::styled(
            format!("{}_", input),
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::UNDERLINED),
        ));
    } else {
        let current_value = app.preferences.borrow().jaeger_endpoint().to_string();
        spans.push(Span::styled(
            current_value,
            Style::default().fg(theme::ACCENT_SPECIAL),
        ));
    }

    let row = Paragraph::new(Line::from(spans)).block(Block::default().borders(Borders::NONE));
    f.render_widget(row, row_area);

    // Description below
    let desc_area = Rect::new(remaining.x + 2, row_y + 1, remaining.width.saturating_sub(2), 1);
    let desc = Paragraph::new("URL for opening trace links (e.g., http://localhost:16686)")
        .style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(desc, desc_area);

    // Hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let hint_spans = if state.editing_jaeger_endpoint {
        vec![
            Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" save", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
        ]
    } else {
        vec![
            Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" edit", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)),
        ]
    };

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}
