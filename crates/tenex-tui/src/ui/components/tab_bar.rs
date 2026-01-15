use crate::ui::card;
use crate::ui::format::{truncate_plain, truncate_with_ellipsis};
use crate::ui::{theme, App, View};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Renders a two-line tab bar:
/// - Line 1: Tab numbers and titles
/// - Line 2: Project names aligned under each tab
pub fn render_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    // Split area into two lines
    let lines = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

    let data_store = app.data_store.borrow();

    // Build both lines simultaneously
    let mut title_spans: Vec<Span> = Vec::new();
    let mut project_spans: Vec<Span> = Vec::new();

    // First tab is always "Home" (Option+1)
    // Home is active when viewing the Home view
    let home_active = app.view == View::Home;
    let home_num_style = if home_active {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let home_title_style = if home_active {
        Style::default()
            .fg(theme::ACCENT_SUCCESS)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        theme::tab_inactive()
    };

    // Home tab - title line
    title_spans.push(Span::styled("⌥1 ", home_num_style));
    title_spans.push(Span::styled("Home", home_title_style));

    // Home tab - project line (empty space to align)
    project_spans.push(Span::styled("   ", Style::default()));
    project_spans.push(Span::styled("    ", Style::default())); // Same width as "Home"

    // Separator after home
    if !app.open_tabs.is_empty() {
        title_spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
        project_spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
    }

    for (i, tab) in app.open_tabs.iter().enumerate() {
        let is_active = i == app.active_tab_index;
        let tab_num = i + 2; // Start from 2 since 1 is Home

        // Tab number (Option+N shortcut hint)
        let num_style = if is_active {
            Style::default().fg(theme::ACCENT_PRIMARY)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        // Show shortcut hint for tabs 2-9
        let shortcut = if tab_num <= 9 {
            format!("⌥{} ", tab_num)
        } else {
            "   ".to_string()
        };
        title_spans.push(Span::styled(shortcut.clone(), num_style));

        // Unread indicator or draft indicator
        if tab.is_draft() {
            title_spans.push(Span::styled("+", Style::default().fg(theme::ACCENT_SUCCESS)));
        } else if tab.has_unread && !is_active {
            title_spans.push(Span::styled(card::BULLET, Style::default().fg(theme::ACCENT_ERROR)));
        } else {
            title_spans.push(Span::raw(" "));
        }

        // Tab title (truncated)
        let max_title_len = 14;
        let title = truncate_with_ellipsis(&tab.thread_title, max_title_len);
        let title_style = if is_active {
            theme::tab_active()
        } else if tab.has_unread {
            theme::tab_unread()
        } else {
            theme::tab_inactive()
        };
        title_spans.push(Span::styled(title.clone(), title_style));

        // Project name on second line (aligned under title)
        let project_name = data_store.get_project_name(&tab.project_a_tag);
        let max_project_len = max_title_len;
        let project_display = truncate_plain(&project_name, max_project_len);

        let project_style = if is_active {
            Style::default().fg(theme::ACCENT_SUCCESS)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        // Pad project line to align with title line
        // shortcut (3 chars) + bullet/space (1 char)
        project_spans.push(Span::styled("    ", Style::default()));
        // Pad project name to match title width
        let padded_project = format!("{:width$}", project_display, width = title.len());
        project_spans.push(Span::styled(padded_project, project_style));

        // Separator between tabs
        if i < app.open_tabs.len() - 1 {
            title_spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
            project_spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
        }
    }

    // Add hint at the end of title line
    title_spans.push(Span::styled("  ", Style::default()));
    title_spans.push(Span::styled("^T←/→:nav x:close", Style::default().fg(theme::TEXT_MUTED)));

    // Render both lines
    let title_line = Line::from(title_spans);
    let project_line = Line::from(project_spans);

    f.render_widget(Paragraph::new(title_line), lines[0]);
    f.render_widget(Paragraph::new(project_line), lines[1]);
}
