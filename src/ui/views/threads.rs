use crate::store::get_profile_name;
use crate::ui::App;
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

pub fn render_threads(f: &mut Frame, app: &App, area: Rect) {
    if app.creating_thread {
        let chunks = Layout::vertical([
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

        if app.threads.is_empty() {
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
        return;
    }

    if app.threads.is_empty() {
        let empty = Paragraph::new("No threads found. Press 'n' to create a new thread.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, area);
        return;
    }

    render_thread_list(f, app, area);
}

fn render_thread_list(f: &mut Frame, app: &App, area: Rect) {

    let items: Vec<ListItem> = app
        .threads
        .iter()
        .enumerate()
        .map(|(i, thread)| {
            let is_selected = i == app.selected_thread_index;
            let prefix = if is_selected { "▶ " } else { "  " };

            let author_name = get_profile_name(&app.db.connection(), &thread.pubkey);

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
