use crate::ui::views::chat::render_tab_bar;
use crate::ui::App;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

pub fn render_threads(f: &mut Frame, app: &App, area: Rect) {
    let has_tabs = !app.open_tabs.is_empty();

    // Main vertical layout: content + optional tab bar
    let vertical_chunks = if has_tabs {
        Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(1),
        ])
        .split(area)
    } else {
        Layout::vertical([Constraint::Min(0)]).split(area)
    };

    let main_area = vertical_chunks[0];

    // Split into main content and status sidebar
    let main_chunks = Layout::horizontal([
        Constraint::Min(40),
        Constraint::Length(30),
    ])
    .split(main_area);

    let content_area = main_chunks[0];
    let status_area = main_chunks[1];

    // Render status sidebar
    render_status_sidebar(f, app, status_area);

    if app.creating_thread {
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(content_area);

        let threads = app.threads();
        if threads.is_empty() {
            let empty = Paragraph::new("No threads found.")
                .style(Style::default().fg(Color::DarkGray));
            f.render_widget(empty, chunks[0]);
        } else {
            render_thread_list(f, app, chunks[0]);
        }

        let input_widget = Paragraph::new(app.input.as_str())
            .style(Style::default().fg(Color::Yellow))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title("Enter thread title (Enter to create, Esc to cancel)")
                    .border_style(Style::default().fg(Color::Yellow)),
            );
        f.render_widget(input_widget, chunks[1]);

        // Tab bar (if tabs exist)
        if has_tabs {
            render_tab_bar(f, app, vertical_chunks[1]);
        }
        return;
    }

    let threads = app.threads();
    if threads.is_empty() {
        let empty = Paragraph::new("No threads found. Press 'n' to create a new thread.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, content_area);

        // Tab bar (if tabs exist)
        if has_tabs {
            render_tab_bar(f, app, vertical_chunks[1]);
        }
        return;
    }

    render_thread_list(f, app, content_area);

    // Render agent selector overlay if showing
    if app.showing_agent_selector {
        render_agent_selector(f, app, area);
    }

    // Tab bar (if tabs exist)
    if has_tabs {
        render_tab_bar(f, app, vertical_chunks[1]);
    }
}

fn render_thread_list(f: &mut Frame, app: &App, area: Rect) {
    let threads = app.threads();
    let items: Vec<ListItem> = threads
        .iter()
        .enumerate()
        .map(|(i, thread)| {
            let is_selected = i == app.selected_thread_index;
            let prefix = if is_selected { "▶ " } else { "  " };

            let author_name = app.data_store.borrow().get_profile_name(&thread.pubkey);

            let style = if is_selected {
                Style::default()
                    .fg(Color::Green)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            let content = vec![
                Line::from(Span::styled(format!("{}{}", prefix, thread.title), style)),
                Line::from(Span::styled(
                    format!("  by {}", author_name),
                    Style::default().fg(Color::DarkGray),
                )),
            ];

            ListItem::new(content)
        })
        .collect();

    let project_name = app
        .selected_project
        .as_ref()
        .map(|p| p.name.clone())
        .unwrap_or_else(|| "Unknown".to_string());

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::NONE)
                .title(format!("{} - Threads (Esc to go back, 'n' for new)", project_name)),
        )
        .highlight_style(Style::default().bg(Color::DarkGray));

    let mut state = ListState::default();
    state.select(Some(app.selected_thread_index));

    f.render_stateful_widget(list, area, &mut state);
}

fn render_status_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    if let Some(status) = app.get_selected_project_status() {
        // Online indicator
        let online_style = if status.is_online() {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::Red)
        };
        lines.push(Line::from(Span::styled(
            if status.is_online() { "● Online" } else { "○ Offline" },
            online_style,
        )));
        lines.push(Line::from(""));

        // Agents section
        lines.push(Line::from(Span::styled(
            "Agents:",
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));
        for agent in &status.agents {
            let selected = app.selected_agent.as_ref().map(|a| &a.pubkey) == Some(&agent.pubkey);
            let prefix = if selected { "▶ " } else { "  " };
            let pm_badge = if agent.is_pm { " [PM]" } else { "" };
            let style = if selected {
                Style::default().fg(Color::Green)
            } else {
                Style::default().fg(Color::White)
            };
            lines.push(Line::from(Span::styled(
                format!("{}{}{}", prefix, agent.name, pm_badge),
                style,
            )));
            if let Some(ref model) = agent.model {
                lines.push(Line::from(Span::styled(
                    format!("    {}", model),
                    Style::default().fg(Color::DarkGray),
                )));
            }
        }
        lines.push(Line::from(""));

        // Branches section
        if !status.branches.is_empty() {
            lines.push(Line::from(Span::styled(
                "Branches:",
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));
            for branch in &status.branches {
                lines.push(Line::from(Span::styled(
                    format!("  {}", branch),
                    Style::default().fg(Color::White),
                )));
            }
            lines.push(Line::from(""));
        }

        // Tools section (summarized)
        let tools = status.tools();
        if !tools.is_empty() {
            lines.push(Line::from(Span::styled(
                format!("Tools: {}", tools.len()),
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
            )));
        }
    } else {
        lines.push(Line::from(Span::styled(
            "No status",
            Style::default().fg(Color::DarkGray),
        )));
        lines.push(Line::from(Span::styled(
            "Waiting for sync...",
            Style::default().fg(Color::DarkGray),
        )));
    }

    // Add key hints
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "'a' - Select agent",
        Style::default().fg(Color::DarkGray),
    )));

    let sidebar = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::LEFT)
                .title("Status")
                .border_style(Style::default().fg(Color::DarkGray)),
        );
    f.render_widget(sidebar, area);
}

fn render_agent_selector(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::widgets::Clear;

    // Calculate popup area (centered)
    let popup_width = 40.min(area.width.saturating_sub(4));
    let popup_height = (app.available_agents().len() as u16 + 4).min(area.height.saturating_sub(4));
    let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the background
    f.render_widget(Clear, popup_area);

    let agents = app.available_agents();
    let items: Vec<ListItem> = agents
        .iter()
        .enumerate()
        .map(|(i, agent)| {
            let is_selected = i == app.agent_selector_index;
            let prefix = if is_selected { "▶ " } else { "  " };
            let pm_badge = if agent.is_pm { " [PM]" } else { "" };

            let style = if is_selected {
                Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };

            ListItem::new(Line::from(Span::styled(
                format!("{}{}{}", prefix, agent.name, pm_badge),
                style,
            )))
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Cyan))
                .title("Select Agent (↑↓ to move, Enter to select, Esc to cancel)"),
        );

    let mut state = ListState::default();
    state.select(Some(app.agent_selector_index));

    f.render_stateful_widget(list, popup_area, &mut state);
}
