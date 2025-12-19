use crate::ui::App;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

pub fn render_projects(f: &mut Frame, app: &App, area: Rect) {
    // Split into filter input and project list
    let chunks = Layout::vertical([
        Constraint::Length(3), // Filter input
        Constraint::Min(0),    // Project list
    ])
    .split(area);

    // Render filter input
    let filter_style = if app.project_filter.is_empty() {
        Style::default().fg(Color::DarkGray)
    } else {
        Style::default().fg(Color::Yellow)
    };

    let filter_text = if app.project_filter.is_empty() {
        "Type to filter projects..."
    } else {
        &app.project_filter
    };

    let filter_input = Paragraph::new(filter_text)
        .style(filter_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(filter_style)
                .title("🔍 Filter"),
        );
    f.render_widget(filter_input, chunks[0]);

    // Set cursor position in filter input
    if !app.project_filter.is_empty() {
        f.set_cursor_position((
            chunks[0].x + app.project_filter.len() as u16 + 1,
            chunks[0].y + 1,
        ));
    }

    let data_store = app.data_store.borrow();
    if data_store.get_projects().is_empty() {
        let empty = Paragraph::new("No projects found. Create a project to get started.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, chunks[1]);
        return;
    }

    // Get filtered and sorted projects
    let (online_projects, offline_projects) = app.filtered_projects();
    let total_online = online_projects.len();
    let total_offline = offline_projects.len();

    if online_projects.is_empty() && offline_projects.is_empty() {
        let empty = Paragraph::new("No projects match the filter.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, chunks[1]);
        return;
    }

    let mut items: Vec<ListItem> = Vec::new();
    let mut list_index = 0;

    // Online projects section
    if !online_projects.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            format!("● ONLINE ({})", total_online),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ))));
        list_index += 1;

        for project in &online_projects {
            let is_selected = list_index - 1 == app.selected_project_index;
            let prefix = if is_selected { "  ▶ " } else { "    " };

            let owner_name = data_store.get_profile_name(&project.pubkey);
            let agent_count = data_store
                .get_project_status(&project.a_tag())
                .map(|s| s.agents.len())
                .unwrap_or(0);
            let info = format!("{} agent(s) · Owner: {}", agent_count, owner_name);

            let style = if is_selected {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::from(Span::styled(format!("{}{}", prefix, project.name), style)),
                Line::from(Span::styled(
                    format!("      {}", info),
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            items.push(ListItem::new(content));
            list_index += 1;
        }
    }

    // Offline projects section
    if !offline_projects.is_empty() {
        // Add spacing if we had online projects
        if !online_projects.is_empty() {
            items.push(ListItem::new(Line::from("")));
        }

        let offline_header = if app.offline_projects_expanded {
            format!("○ OFFLINE ({}) ▼", total_offline)
        } else {
            format!("○ OFFLINE ({}) ▶ (press Tab to expand)", total_offline)
        };

        // Check if the offline header is selected (for toggling)
        let header_selected = if online_projects.is_empty() {
            app.selected_project_index == 0
        } else {
            false // Header not selectable when there are online projects
        };

        let header_style = if header_selected {
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(Color::DarkGray)
                .add_modifier(Modifier::BOLD)
        };

        items.push(ListItem::new(Line::from(Span::styled(
            offline_header,
            header_style,
        ))));

        if app.offline_projects_expanded {
            for (offline_idx, project) in offline_projects.iter().enumerate() {
                let is_selected = if online_projects.is_empty() {
                    // When no online projects, selection is directly the offline index
                    offline_idx == app.selected_project_index
                } else {
                    // When there are online projects, offset by their count
                    online_projects.len() + offline_idx == app.selected_project_index
                };
                let prefix = if is_selected { "  ▶ " } else { "    " };

                let owner_name = data_store.get_profile_name(&project.pubkey);
                let agent_count = data_store
                    .get_project_status(&project.a_tag())
                    .map(|s| s.agents.len())
                    .unwrap_or(0);
                let info = format!("{} agent(s) · Owner: {}", agent_count, owner_name);

                let style = if is_selected {
                    Style::default()
                        .fg(Color::Yellow)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::DarkGray)
                };

                let content = vec![
                    Line::from(Span::styled(format!("{}{}", prefix, project.name), style)),
                    Line::from(Span::styled(
                        format!("      {}", info),
                        Style::default().fg(Color::DarkGray),
                    )),
                ];

                items.push(ListItem::new(content));
            }
        }
    }

    let title = if app.project_filter.is_empty() {
        "Projects (↑/↓ navigate, Enter select, Tab expand offline)"
    } else {
        "Filtered Projects"
    };

    let list = List::new(items).block(Block::default().borders(Borders::NONE).title(title));

    f.render_widget(list, chunks[1]);
}

/// Get the actual project at the given selection index
/// Returns (project, is_online)
pub fn get_project_at_index(app: &App, index: usize) -> Option<(crate::models::Project, bool)> {
    let (online_projects, offline_projects) = app.filtered_projects();

    if index < online_projects.len() {
        online_projects.get(index).map(|p| (p.clone(), true))
    } else if app.offline_projects_expanded {
        let offline_index = index - online_projects.len();
        offline_projects.get(offline_index).map(|p| (p.clone(), false))
    } else {
        None
    }
}

/// Get the total count of selectable projects
pub fn selectable_project_count(app: &App) -> usize {
    let (online, offline) = app.filtered_projects();
    if app.offline_projects_expanded {
        online.len() + offline.len()
    } else {
        online.len()
    }
}
