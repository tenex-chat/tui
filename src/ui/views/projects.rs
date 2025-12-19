use crate::store::get_profile_name;
use crate::ui::App;
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState},
    Frame,
};

pub fn render_projects(f: &mut Frame, app: &App, area: Rect) {
    if app.projects.is_empty() {
        let empty = ratatui::widgets::Paragraph::new("No projects found. Create a project to get started.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = app
        .projects
        .iter()
        .enumerate()
        .map(|(i, project)| {
            let is_selected = i == app.selected_project_index;
            let prefix = if is_selected { "▶ " } else { "  " };

            let owner_name = get_profile_name(&app.db.connection(), &project.pubkey);
            let info = format!(
                "{} participant(s) · Owner: {}",
                project.participants.len(),
                owner_name
            );

            let style = if is_selected {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::from(Span::styled(format!("{}{}", prefix, project.name), style)),
                Line::from(Span::styled(format!("  {}", info), Style::default().fg(Color::DarkGray))),
            ];

            ListItem::new(content)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::NONE)
                .title("Use ↑/↓ to navigate, Enter to select"),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));

    let mut state = ListState::default();
    state.select(Some(app.selected_project_index));

    f.render_stateful_widget(list, area, &mut state);
}
