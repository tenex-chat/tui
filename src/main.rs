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
use std::sync::Arc;
use std::time::Duration;
use tracing::info_span;

use nostr::{DataChange, NostrCommand, NostrWorker};
use std::sync::mpsc;
use ui::views::login::{render_login, LoginStep};
use ui::{App, InputMode, View};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_setup::init_tracing();

    // Create shared nostrdb instance
    std::fs::create_dir_all("tenex_data")?;
    let ndb = Arc::new(nostrdb::Ndb::new("tenex_data", &nostrdb::Config::new())?);

    let db = store::Database::with_ndb(ndb.clone(), "tenex_data")?;
    let mut app = App::new(db);
    let mut terminal = ui::init_terminal()?;

    let (command_tx, command_rx) = mpsc::channel::<NostrCommand>();
    let (data_tx, data_rx) = mpsc::channel::<DataChange>();

    app.set_channels(command_tx.clone(), data_rx);

    let worker = NostrWorker::new(ndb, data_tx, command_rx);

    let worker_handle = std::thread::spawn(move || {
        worker.run();
    });

    let mut login_step = if nostr::has_stored_credentials(&app.db.credentials_conn()) {
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
        {
            let _span = info_span!("check_data_updates").entered();
            app.check_for_data_updates()?;
        }

        {
            let _span = info_span!("render").entered();
            terminal.draw(|f| render(f, app, login_step))?;
        }

        {
            let _span = info_span!("poll_events").entered();
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    if key.kind == KeyEventKind::Press {
                        let _span = info_span!("handle_key", key = ?key.code).entered();
                        handle_key(app, key.code, login_step, pending_nsec);
                    }
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

    // Footer - only show masked input for Login view (password), otherwise show hints
    let footer_text = match (&app.view, &app.input_mode) {
        (View::Login, InputMode::Editing) => format!("> {}", "*".repeat(app.input.len())),
        (View::Projects, _) => "Type to filter · Tab expand offline · Enter select · q quit".to_string(),
        (_, InputMode::Normal) => "Press 'q' to quit".to_string(),
        _ => String::new(), // Chat/Threads editing has its own input box
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(Color::DarkGray))
        .block(Block::default().borders(Borders::TOP));
    f.render_widget(footer, chunks[2]);
}

fn handle_key(app: &mut App, key: KeyCode, login_step: &mut LoginStep, pending_nsec: &mut Option<String>) {
    // Handle agent selector when open
    if app.showing_agent_selector {
        match key {
            KeyCode::Up => {
                if app.agent_selector_index > 0 {
                    app.agent_selector_index -= 1;
                }
            }
            KeyCode::Down => {
                let max = app.available_agents().len().saturating_sub(1);
                if app.agent_selector_index < max {
                    app.agent_selector_index += 1;
                }
            }
            KeyCode::Enter => {
                app.select_agent_by_index(app.agent_selector_index);
                app.showing_agent_selector = false;
            }
            KeyCode::Esc => {
                app.showing_agent_selector = false;
            }
            _ => {}
        }
        return;
    }

    match app.input_mode {
        InputMode::Normal => match key {
            KeyCode::Char('q') => {
                // In Projects view, 'q' adds to filter; elsewhere it quits
                if app.view == View::Projects {
                    app.project_filter.push('q');
                    app.selected_project_index = 0; // Reset selection on filter change
                } else {
                    app.quit();
                }
            }
            KeyCode::Char('i') => {
                // In Projects view, 'i' adds to filter; elsewhere starts editing
                if app.view == View::Projects {
                    app.project_filter.push('i');
                    app.selected_project_index = 0;
                } else {
                    app.input_mode = InputMode::Editing;
                }
            }
            KeyCode::Char('r') => {
                if app.view == View::Projects {
                    app.project_filter.push('r');
                    app.selected_project_index = 0;
                } else if let Some(ref command_tx) = app.command_tx {
                    let tx = command_tx.clone();
                    app.set_status("Syncing...");
                    if let Err(e) = tx.send(NostrCommand::Sync) {
                        app.set_status(&format!("Sync request failed: {}", e));
                    }
                }
            }
            KeyCode::Char(c) => {
                // In Projects view, typing filters projects
                if app.view == View::Projects {
                    app.project_filter.push(c);
                    app.selected_project_index = 0; // Reset selection on filter change
                } else if c == 'n' && app.view == View::Threads {
                    app.creating_thread = true;
                    app.input_mode = InputMode::Editing;
                    app.clear_input();
                } else if (c == 'a' || c == '@') && (app.view == View::Threads || app.view == View::Chat) && !app.available_agents().is_empty() {
                    app.showing_agent_selector = true;
                    app.agent_selector_index = 0;
                }
            }
            KeyCode::Backspace => {
                // In Projects view, delete from filter
                if app.view == View::Projects && !app.project_filter.is_empty() {
                    app.project_filter.pop();
                    app.selected_project_index = 0;
                }
            }
            KeyCode::Tab => {
                // In Projects view, toggle offline projects expansion
                if app.view == View::Projects {
                    app.offline_projects_expanded = !app.offline_projects_expanded;
                }
            }
            KeyCode::Up => match app.view {
                View::Projects if app.selected_project_index > 0 => {
                    app.selected_project_index -= 1;
                }
                View::Threads if app.selected_thread_index > 0 => {
                    app.selected_thread_index -= 1;
                }
                View::Chat if app.scroll_offset > 0 => {
                    app.scroll_offset = app.scroll_offset.saturating_sub(3);
                }
                _ => {}
            },
            KeyCode::Down => match app.view {
                View::Projects => {
                    let max = ui::views::selectable_project_count(app).saturating_sub(1);
                    if app.selected_project_index < max {
                        app.selected_project_index += 1;
                    }
                }
                View::Threads if app.selected_thread_index < app.threads.len().saturating_sub(1) => {
                    app.selected_thread_index += 1;
                }
                View::Chat => {
                    app.scroll_offset = app.scroll_offset.saturating_add(3);
                }
                _ => {}
            },
            KeyCode::Home => {
                if app.view == View::Chat {
                    app.scroll_offset = 0;
                }
            }
            KeyCode::End => {
                if app.view == View::Chat {
                    app.scroll_offset = usize::MAX; // Will be clamped in render
                }
            }
            KeyCode::PageUp => {
                if app.view == View::Chat {
                    app.scroll_offset = app.scroll_offset.saturating_sub(20);
                }
            }
            KeyCode::PageDown => {
                if app.view == View::Chat {
                    app.scroll_offset = app.scroll_offset.saturating_add(20);
                }
            }
            KeyCode::Enter => match app.view {
                View::Projects => {
                    // Use the new project selection logic
                    if let Some((project, _is_online)) = ui::views::get_project_at_index(app, app.selected_project_index) {
                        let project = project.clone();
                        app.selected_project = Some(project.clone());

                        // Load threads for this project from nostrdb (with full activity calc)
                        if let Ok(threads) = store::get_threads_for_project_with_activity(&app.db.ndb, &project.a_tag()) {
                            app.threads = threads;
                        }
                        app.selected_thread_index = 0;
                        app.project_filter.clear(); // Clear filter when entering project
                        app.view = View::Threads;
                    }
                }
                View::Threads if !app.threads.is_empty() => {
                    let _span = info_span!("enter_chat_view").entered();
                    tracing::info!("Entering chat view");

                    let thread = app.threads[app.selected_thread_index].clone();
                    app.selected_thread = Some(thread.clone());

                    // Load messages for this thread from nostrdb
                    {
                        let _span = info_span!("load_messages").entered();
                        if let Ok(messages) = store::get_messages_for_thread(&app.db.ndb, &thread.id) {
                            tracing::info!("Loaded {} messages", messages.len());
                            app.messages = messages;
                        }
                    }

                    // Auto-select first available agent if none selected
                    {
                        let _span = info_span!("select_agent").entered();
                        if app.selected_agent.is_none() {
                            app.select_agent_by_index(0);
                        }
                    }

                    // Scroll to bottom of chat
                    app.scroll_offset = usize::MAX;
                    app.view = View::Chat;
                    tracing::info!("Chat view ready");
                }
                _ => {}
            },
            KeyCode::Esc => match app.view {
                View::Projects => {
                    // If filter is active, clear it; otherwise do nothing (can't go back from projects)
                    if !app.project_filter.is_empty() {
                        app.project_filter.clear();
                        app.selected_project_index = 0;
                    }
                }
                View::Threads => app.view = View::Projects,
                View::Chat => app.view = View::Threads,
                _ => {}
            },
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
            KeyCode::Char('@') if app.view == View::Chat && !app.available_agents().is_empty() => {
                // Open agent selector from chat input
                app.showing_agent_selector = true;
                app.agent_selector_index = 0;
            }
            KeyCode::Char(c) => app.enter_char(c),
            KeyCode::Backspace => app.delete_char(),
            KeyCode::Left => app.move_cursor_left(),
            KeyCode::Right => app.move_cursor_right(),
            // Allow scrolling while typing in Chat view
            KeyCode::Up if app.view == View::Chat => {
                app.scroll_offset = app.scroll_offset.saturating_sub(3);
            }
            KeyCode::Down if app.view == View::Chat => {
                app.scroll_offset = app.scroll_offset.saturating_add(3);
            }
            KeyCode::PageUp if app.view == View::Chat => {
                app.scroll_offset = app.scroll_offset.saturating_sub(20);
            }
            KeyCode::PageDown if app.view == View::Chat => {
                app.scroll_offset = app.scroll_offset.saturating_add(20);
            }
            KeyCode::Enter => {
                let input = app.submit_input();
                app.input_mode = InputMode::Normal;

                match app.view {
                    View::Login => match login_step {
                        LoginStep::Nsec => {
                            // Check if user wants to use stored credentials
                            if input.is_empty() && nostr::has_stored_credentials(&app.db.credentials_conn()) {
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
                                nostr::load_stored_keys(&input, &app.db.credentials_conn())
                            } else if let Some(ref nsec) = pending_nsec {
                                let password = if input.is_empty() { None } else { Some(input.as_str()) };
                                nostr::auth::login_with_nsec(nsec, password, &app.db.credentials_conn())
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
                                            user_pubkey: user_pubkey.clone(),
                                        }) {
                                            app.set_status(&format!("Failed to connect: {}", e));
                                            *login_step = LoginStep::Nsec;
                                        } else if let Err(e) = command_tx.send(NostrCommand::Sync) {
                                            app.set_status(&format!("Failed to sync: {}", e));
                                        } else {
                                            app.view = View::Projects;
                                            app.clear_status();
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
                            let keys_result = nostr::load_stored_keys(&input, &app.db.credentials_conn());

                            match keys_result {
                                Ok(keys) => {
                                    let user_pubkey = nostr::get_current_pubkey(&keys);
                                    app.keys = Some(keys.clone());

                                    if let Some(ref command_tx) = app.command_tx {
                                        if let Err(e) = command_tx.send(NostrCommand::Connect {
                                            keys: keys.clone(),
                                            user_pubkey: user_pubkey.clone(),
                                        }) {
                                            app.set_status(&format!("Failed to connect: {}", e));
                                            *login_step = LoginStep::Unlock;
                                        } else if let Err(e) = command_tx.send(NostrCommand::Sync) {
                                            app.set_status(&format!("Failed to sync: {}", e));
                                        } else {
                                            app.view = View::Projects;
                                            app.clear_status();
                                        }
                                    }
                                }
                                Err(e) => {
                                    app.set_status(&format!(
                                        "Unlock failed: {}. Press Esc to clear input and retry.",
                                        e
                                    ));
                                }
                            }
                        }
                    },
                    View::Threads => {
                        if app.creating_thread && !input.is_empty() {
                            if let (Some(ref command_tx), Some(ref project)) =
                                (&app.command_tx, &app.selected_project)
                            {
                                let title = input.clone();
                                let content = input.clone();
                                let project_a_tag = project.a_tag();
                                let agent_pubkey = app.selected_agent.as_ref().map(|a| a.pubkey.clone());
                                let branch = app.project_status.as_ref().and_then(|s| s.default_branch().map(|b| b.to_string()));

                                if let Err(e) = command_tx.send(NostrCommand::PublishThread {
                                    project_a_tag,
                                    title,
                                    content,
                                    agent_pubkey,
                                    branch,
                                }) {
                                    app.set_status(&format!("Failed to publish thread: {}", e));
                                }

                                app.creating_thread = false;
                            }
                        }
                    }
                    View::Chat => {
                        if !input.is_empty() {
                            if let (Some(ref command_tx), Some(ref thread), Some(ref project)) =
                                (&app.command_tx, &app.selected_thread, &app.selected_project)
                            {
                                let thread_id = thread.id.clone();
                                let project_a_tag = project.a_tag();
                                let content = input.clone();
                                let agent_pubkey = app.selected_agent.as_ref().map(|a| a.pubkey.clone());
                                let branch = app.project_status.as_ref().and_then(|s| s.default_branch().map(|b| b.to_string()));

                                // Reply to the most recent message in the thread
                                let reply_to = app.messages.last().map(|m| m.id.clone());

                                if let Err(e) = command_tx.send(NostrCommand::PublishMessage {
                                    thread_id,
                                    project_a_tag,
                                    content,
                                    agent_pubkey,
                                    reply_to,
                                    branch,
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
