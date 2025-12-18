mod models;
mod nostr;
mod store;
mod ui;

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::time::Duration;
use ui::{App, View, InputMode};

fn main() -> Result<()> {
    let db = store::Database::new("tenex.db")?;
    let mut app = App::new(db);
    let mut terminal = ui::init_terminal()?;

    let result = run_app(&mut terminal, &mut app);

    ui::restore_terminal()?;

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }

    Ok(())
}

fn run_app(terminal: &mut ui::Tui, app: &mut App) -> Result<()> {
    while app.running {
        terminal.draw(|f| render(f, app))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code);
                }
            }
        }
    }
    Ok(())
}

fn render(f: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(f.area());

    // Header
    let title = match app.view {
        View::Login => "TENEX - Login",
        View::Projects => "TENEX - Projects",
        View::Threads => "TENEX - Threads",
        View::Chat => "TENEX - Chat",
    };
    let header = Paragraph::new(title)
        .style(Style::default().fg(Color::Cyan))
        .block(Block::default().borders(Borders::BOTTOM));
    f.render_widget(header, chunks[0]);

    // Main content
    let content = match app.view {
        View::Login => "Enter your nsec to login:\n\nPress 'i' to start typing, Enter to submit",
        _ => "Content area",
    };
    let main = Paragraph::new(content).block(Block::default().borders(Borders::NONE));
    f.render_widget(main, chunks[1]);

    // Footer / input
    let footer_text = if app.input_mode == InputMode::Editing {
        format!("> {}", app.input)
    } else {
        "Press 'q' to quit, 'i' to edit".to_string()
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::Yellow))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[2]);
}

fn handle_key(app: &mut App, key: KeyCode) {
    match app.input_mode {
        InputMode::Normal => match key {
            KeyCode::Char('q') => app.quit(),
            KeyCode::Char('i') => app.input_mode = InputMode::Editing,
            KeyCode::Up => {
                match app.view {
                    View::Projects if app.selected_project_index > 0 => {
                        app.selected_project_index -= 1;
                    }
                    View::Threads if app.selected_thread_index > 0 => {
                        app.selected_thread_index -= 1;
                    }
                    _ => {}
                }
            }
            KeyCode::Down => {
                match app.view {
                    View::Projects if app.selected_project_index < app.projects.len().saturating_sub(1) => {
                        app.selected_project_index += 1;
                    }
                    View::Threads if app.selected_thread_index < app.threads.len().saturating_sub(1) => {
                        app.selected_thread_index += 1;
                    }
                    _ => {}
                }
            }
            KeyCode::Enter => {
                // Select item based on view
            }
            KeyCode::Esc => {
                match app.view {
                    View::Threads => app.view = View::Projects,
                    View::Chat => app.view = View::Threads,
                    _ => {}
                }
            }
            _ => {}
        },
        InputMode::Editing => match key {
            KeyCode::Esc => app.input_mode = InputMode::Normal,
            KeyCode::Char(c) => app.enter_char(c),
            KeyCode::Backspace => app.delete_char(),
            KeyCode::Left => app.move_cursor_left(),
            KeyCode::Right => app.move_cursor_right(),
            KeyCode::Enter => {
                let _input = app.submit_input();
                app.input_mode = InputMode::Normal;
            }
            _ => {}
        },
    }
}
