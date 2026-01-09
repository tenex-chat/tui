use crate::ui::format::{truncate_plain, truncate_with_ellipsis};
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub fn render_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();

    // Borrow data_store once for all project name lookups
    let data_store = app.data_store.borrow();

    for (i, tab) in app.open_tabs.iter().enumerate() {
        let is_active = i == app.active_tab_index;

        // Tab number with period separator
        let num_style = if is_active {
            Style::default().fg(theme::ACCENT_PRIMARY)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };
        spans.push(Span::styled(format!("{}. ", i + 1), num_style));

        // Unread indicator (moved before project name)
        if tab.has_unread && !is_active {
            spans.push(Span::styled("● ", Style::default().fg(theme::ACCENT_ERROR)));
        } else {
            spans.push(Span::raw("● "));
        }

        // Project name (truncated to 8 chars max)
        let project_name = data_store.get_project_name(&tab.project_a_tag);
        let max_project_len = 8;
        let project_display = truncate_plain(&project_name, max_project_len);

        let project_style = if is_active {
            Style::default().fg(theme::ACCENT_SUCCESS)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };
        spans.push(Span::styled(project_display, project_style));
        spans.push(Span::raw(" | "));

        // Tab title (truncated to fit remaining space)
        let max_title_len = 12;
        let title = truncate_with_ellipsis(&tab.thread_title, max_title_len);

        let title_style = if is_active {
            theme::tab_active()
        } else if tab.has_unread {
            theme::tab_unread()
        } else {
            theme::tab_inactive()
        };
        spans.push(Span::styled(title, title_style));

        // Separator between tabs
        if i < app.open_tabs.len() - 1 {
            spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
        }
    }

    // Add hint at the end
    spans.push(Span::styled("  ", Style::default()));
    spans.push(Span::styled("Tab:cycle x:close", Style::default().fg(theme::TEXT_MUTED)));

    let tab_line = Line::from(spans);
    let tab_bar = Paragraph::new(tab_line);
    f.render_widget(tab_bar, area);
}
