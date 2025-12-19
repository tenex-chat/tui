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
use tokio::runtime::Runtime;
use ui::{App, View, InputMode};
use ui::views::login::{render_login, LoginStep};

fn main() -> Result<()> {
    let rt = Runtime::new()?;
    let db = store::Database::new("tenex.db")?;
    let mut app = App::new(db);
    let mut terminal = ui::init_terminal()?;
    let mut login_step = LoginStep::Nsec;
    let mut pending_nsec: Option<String> = None;

    let result = run_app(&mut terminal, &mut app, &mut login_step, &mut pending_nsec, &rt);

    ui::restore_terminal()?;

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }

    Ok(())
}

fn run_app(
    terminal: &mut ui::Tui,
    app: &mut App,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
    rt: &Runtime,
) -> Result<()> {
    while app.running {
        terminal.draw(|f| render(f, app, login_step))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code, login_step, pending_nsec, rt);
                }
            }
        }
    }
    Ok(())
}

fn render(f: &mut Frame, app: &App, login_step: &LoginStep) {
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
    match app.view {
        View::Login => render_login(f, app, chunks[1], login_step),
        View::Projects => ui::views::render_projects(f, app, chunks[1]),
        View::Threads => ui::views::render_threads(f, app, chunks[1]),
        View::Chat => ui::views::render_chat(f, app, chunks[1]),
    }

    // Footer
    let footer_text = match app.input_mode {
        InputMode::Editing => format!("> {}", "*".repeat(app.input.len())),
        InputMode::Normal => "Press 'q' to quit".to_string(),
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[2]);
}

fn handle_key(
    app: &mut App,
    key: KeyCode,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
    rt: &Runtime,
) {
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
                match app.view {
                    View::Projects if !app.projects.is_empty() => {
                        let project = app.projects[app.selected_project_index].clone();
                        app.selected_project = Some(project.clone());

                        // Subscribe to project content (threads and messages)
                        if let Some(ref client) = app.nostr_client {
                            let project_a_tag = project.a_tag();
                            let conn = app.db.connection();
                            let client_clone = client.clone();

                            rt.block_on(async move {
                                let _ = nostr::subscribe_to_project_content(&client_clone, &project_a_tag, &conn).await;
                            });
                        }

                        // Load threads for this project from db
                        if let Ok(threads) = store::get_threads_for_project(&app.db.connection(), &project.a_tag()) {
                            app.threads = threads;
                        }
                        app.selected_thread_index = 0;
                        app.view = View::Threads;
                    }
                    View::Threads if !app.threads.is_empty() => {
                        let thread = app.threads[app.selected_thread_index].clone();
                        app.selected_thread = Some(thread.clone());
                        // Load messages for this thread
                        if let Ok(messages) = store::get_messages_for_thread(&app.db.connection(), &thread.id) {
                            app.messages = messages;
                        }
                        app.view = View::Chat;
                    }
                    _ => {}
                }
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
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
                app.clear_input();
            }
            KeyCode::Char(c) => app.enter_char(c),
            KeyCode::Backspace => app.delete_char(),
            KeyCode::Left => app.move_cursor_left(),
            KeyCode::Right => app.move_cursor_right(),
            KeyCode::Enter => {
                let input = app.submit_input();
                app.input_mode = InputMode::Normal;

                match app.view {
                    View::Login => {
                        match login_step {
                            LoginStep::Nsec => {
                                if input.starts_with("nsec") {
                                    *pending_nsec = Some(input);
                                    *login_step = LoginStep::Password;
                                } else {
                                    app.set_status("Invalid nsec format");
                                }
                            }
                            LoginStep::Password => {
                                if let Some(ref nsec) = pending_nsec {
                                    let password = if input.is_empty() { None } else { Some(input.as_str()) };
                                    match nostr::auth::login_with_nsec(nsec, password, &app.db.connection()) {
                                        Ok(keys) => {
                                            let user_pubkey = nostr::get_current_pubkey(&keys);

                                            // Create Nostr client and subscribe to projects
                                            let client_result = rt.block_on(async {
                                                let client = nostr::NostrClient::new(keys.clone()).await?;
                                                nostr::subscribe_to_projects(&client, &user_pubkey, &app.db.connection()).await?;
                                                Ok::<nostr::NostrClient, anyhow::Error>(client)
                                            });

                                            match client_result {
                                                Ok(client) => {
                                                    app.keys = Some(keys);
                                                    app.nostr_client = Some(client);

                                                    // Load projects from db
                                                    if let Ok(projects) = store::get_projects(&app.db.connection()) {
                                                        app.projects = projects;
                                                    }
                                                    app.view = View::Projects;
                                                    app.clear_status();
                                                }
                                                Err(e) => {
                                                    app.set_status(&format!("Failed to connect: {}", e));
                                                    *login_step = LoginStep::Nsec;
                                                }
                                            }
                                        }
                                        Err(e) => {
                                            app.set_status(&format!("Login failed: {}", e));
                                            *login_step = LoginStep::Nsec;
                                        }
                                    }
                                }
                                *pending_nsec = None;
                            }
                        }
                    }
                    View::Chat => {
                        if !input.is_empty() {
                            if let (Some(ref client), Some(ref thread)) = (&app.nostr_client, &app.selected_thread) {
                                let thread_id = thread.id.clone();
                                let content = input.clone();
                                let conn = app.db.connection();

                                rt.block_on(async {
                                    if let Ok(_event_id) = nostr::publish_message(client, &thread_id, &content).await {
                                        // Wait a moment for the event to propagate
                                        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
                                    }
                                });

                                // Reload messages from database after publishing
                                if let Ok(messages) = store::get_messages_for_thread(&conn, &thread_id) {
                                    app.messages = messages;
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }
            _ => {}
        },
    }
}
