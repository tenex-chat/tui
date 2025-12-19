mod models;
mod nostr;
mod store;
mod tracing_setup;
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
use ui::views::login::{render_login, LoginStep};
use nostr::{NostrWorker, NostrCommand, DataChange};
use std::sync::mpsc;

fn main() -> Result<()> {
    tracing_setup::init_tracing();

    let db = store::Database::new("tenex.db")?;
    let mut app = App::new(db);
    let mut terminal = ui::init_terminal()?;

    let (command_tx, command_rx) = mpsc::channel::<NostrCommand>();
    let (data_tx, data_rx) = mpsc::channel::<DataChange>();

    app.set_channels(command_tx.clone(), data_rx);

    let db_conn = app.db.connection();
    let worker = NostrWorker::new(db_conn, data_tx, command_rx);

    let worker_handle = std::thread::spawn(move || {
        worker.run();
    });

    let mut login_step = if nostr::has_stored_credentials(&app.db.connection()) {
        LoginStep::Unlock
    } else {
        LoginStep::Nsec
    };
    let mut pending_nsec: Option<String> = None;

    let result = run_app(&mut terminal, &mut app, &mut login_step, &mut pending_nsec);

    command_tx.send(NostrCommand::Shutdown).ok();
    worker_handle.join().ok();

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
) -> Result<()> {
    while app.running {
        app.check_for_data_updates()?;

        terminal.draw(|f| render(f, app, login_step))?;

        if event::poll(Duration::from_millis(100))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    handle_key(app, key.code, login_step, pending_nsec);
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
) {
    match app.input_mode {
        InputMode::Normal => match key {
            KeyCode::Char('q') => app.quit(),
            KeyCode::Char('i') => app.input_mode = InputMode::Editing,
            KeyCode::Char('r') => {
                if let Some(ref command_tx) = app.command_tx {
                    let tx = command_tx.clone();
                    app.set_status("Syncing...");
                    if let Err(e) = tx.send(NostrCommand::Sync) {
                        app.set_status(&format!("Sync request failed: {}", e));
                    }
                }
            }
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
                        // Load messages for this thread from db
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
            KeyCode::Char('n') => {
                if app.view == View::Threads {
                    app.creating_thread = true;
                    app.input_mode = InputMode::Editing;
                    app.clear_input();
                }
            }
            _ => {}
        },
        InputMode::Editing => match key {
            KeyCode::Esc => {
                app.input_mode = InputMode::Normal;
                app.clear_input();
                if app.creating_thread {
                    app.creating_thread = false;
                }
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
                                // Check if user wants to use stored credentials
                                if input.is_empty() && nostr::has_stored_credentials(&app.db.connection()) {
                                    *pending_nsec = None;
                                    *login_step = LoginStep::Password;
                                } else if input.starts_with("nsec") {
                                    *pending_nsec = Some(input);
                                    *login_step = LoginStep::Password;
                                } else {
                                    app.set_status("Invalid nsec format");
                                }
                            }
                            LoginStep::Password => {
                                let keys_result = if pending_nsec.is_none() {
                                    nostr::load_stored_keys(&input, &app.db.connection())
                                } else if let Some(ref nsec) = pending_nsec {
                                    let password = if input.is_empty() { None } else { Some(input.as_str()) };
                                    nostr::auth::login_with_nsec(nsec, password, &app.db.connection())
                                } else {
                                    Err(anyhow::anyhow!("No credentials provided"))
                                };

                                match keys_result {
                                    Ok(keys) => {
                                        let user_pubkey = nostr::get_current_pubkey(&keys);
                                        app.keys = Some(keys.clone());

                                        if let Some(ref command_tx) = app.command_tx {
                                            if let Err(e) = command_tx.send(NostrCommand::Connect {
                                                keys: keys.clone(),
                                                user_pubkey: user_pubkey.clone()
                                            }) {
                                                app.set_status(&format!("Failed to connect: {}", e));
                                                *login_step = LoginStep::Nsec;
                                            } else {
                                                if let Err(e) = command_tx.send(NostrCommand::Sync) {
                                                    app.set_status(&format!("Failed to sync: {}", e));
                                                } else {
                                                    app.view = View::Projects;
                                                    app.clear_status();
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        app.set_status(&format!("Login failed: {}", e));
                                        *login_step = LoginStep::Nsec;
                                    }
                                }
                                *pending_nsec = None;
                            }
                            LoginStep::Unlock => {
                                let keys_result = nostr::load_stored_keys(&input, &app.db.connection());

                                match keys_result {
                                    Ok(keys) => {
                                        let user_pubkey = nostr::get_current_pubkey(&keys);
                                        app.keys = Some(keys.clone());

                                        if let Some(ref command_tx) = app.command_tx {
                                            if let Err(e) = command_tx.send(NostrCommand::Connect {
                                                keys: keys.clone(),
                                                user_pubkey: user_pubkey.clone()
                                            }) {
                                                app.set_status(&format!("Failed to connect: {}", e));
                                                *login_step = LoginStep::Unlock;
                                            } else {
                                                if let Err(e) = command_tx.send(NostrCommand::Sync) {
                                                    app.set_status(&format!("Failed to sync: {}", e));
                                                } else {
                                                    app.view = View::Projects;
                                                    app.clear_status();
                                                }
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        app.set_status(&format!("Unlock failed: {}. Press Esc to clear input and retry.", e));
                                    }
                                }
                            }
                        }
                    }
                    View::Threads => {
                        if app.creating_thread && !input.is_empty() {
                            if let (Some(ref command_tx), Some(ref project)) = (&app.command_tx, &app.selected_project) {
                                let title = input.clone();
                                let content = input.clone();
                                let project_a_tag = project.a_tag();

                                if let Err(e) = command_tx.send(NostrCommand::PublishThread {
                                    project_a_tag,
                                    title,
                                    content,
                                }) {
                                    app.set_status(&format!("Failed to publish thread: {}", e));
                                }

                                app.creating_thread = false;
                            }
                        }
                    }
                    View::Chat => {
                        if !input.is_empty() {
                            if let (Some(ref command_tx), Some(ref thread)) = (&app.command_tx, &app.selected_thread) {
                                let thread_id = thread.id.clone();
                                let content = input.clone();

                                if let Err(e) = command_tx.send(NostrCommand::PublishMessage {
                                    thread_id,
                                    content,
                                }) {
                                    app.set_status(&format!("Failed to publish message: {}", e));
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
