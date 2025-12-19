mod models;
mod nostr;
mod store;
mod tracing_setup;
mod ui;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Layout},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;
use tracing::info_span;

use nostr::{DataChange, NostrCommand, NostrWorker};
use nostrdb::{FilterBuilder, Ndb, NoteKey, SubscriptionStream};
use std::sync::mpsc;
use store::AppDataStore;

use ui::views::login::{render_login, LoginStep};
use ui::{App, InputMode, View};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_setup::init_tracing();

    // Create shared nostrdb instance
    std::fs::create_dir_all("tenex_data")?;
    let ndb = Arc::new(nostrdb::Ndb::new("tenex_data", &nostrdb::Config::new())?);

    // Create app data store (single source of truth)
    let data_store = Rc::new(RefCell::new(AppDataStore::new(ndb.clone())));

    let db = store::Database::with_ndb(ndb.clone(), "tenex_data")?;
    let mut app = App::new(db, data_store.clone());
    let mut terminal = ui::init_terminal()?;

    let (command_tx, command_rx) = mpsc::channel::<NostrCommand>();
    let (data_tx, data_rx) = mpsc::channel::<DataChange>();

    app.set_channels(command_tx.clone(), data_rx);

    let worker = NostrWorker::new(ndb.clone(), data_tx, command_rx);

    let worker_handle = std::thread::spawn(move || {
        worker.run();
    });

    let mut login_step = if nostr::has_stored_credentials(&app.db.credentials_conn()) {
        LoginStep::Unlock
    } else {
        LoginStep::Nsec
    };
    let mut pending_nsec: Option<String> = None;

    let result = run_app(&mut terminal, &mut app, data_store.clone(), ndb.clone(), &mut login_step, &mut pending_nsec).await;

    command_tx.send(NostrCommand::Shutdown).ok();
    worker_handle.join().ok();

    ui::restore_terminal()?;

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }

    Ok(())
}

async fn run_app(
    terminal: &mut ui::Tui,
    app: &mut App,
    data_store: Rc<RefCell<AppDataStore>>,
    ndb: Arc<Ndb>,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
) -> Result<()> {
    // Create async event stream for terminal events
    let mut event_stream = EventStream::new();

    // Create a tick interval for regular updates (data channel polling, etc.)
    let mut tick_interval = tokio::time::interval(Duration::from_millis(50));

    // Create nostrdb subscription for all event kinds we care about:
    // - 31933: Projects
    // - 11: Threads
    // - 1111: Messages (GenericReply)
    // - 0: Profiles
    // - 4199: Agent definitions
    // - 24010: Project status
    // - 21111: Streaming deltas
    // - 513: Conversation metadata
    let ndb_filter = FilterBuilder::new()
        .kinds([31933, 11, 1111, 0, 4199, 24010, 21111, 513])
        .build();
    let ndb_subscription = ndb.subscribe(&[ndb_filter])?;
    let mut ndb_stream = SubscriptionStream::new((*ndb).clone(), ndb_subscription);

    while app.running {
        // Render first
        {
            let _span = info_span!("render").entered();
            terminal.draw(|f| render(f, app, login_step))?;
        }

        // Wait for events using tokio::select!
        tokio::select! {
            // Terminal UI events
            maybe_event = event_stream.next() => {
                if let Some(Ok(event)) = maybe_event {
                    match event {
                        Event::Key(key) if key.kind == KeyEventKind::Press => {
                            // Handle Ctrl+C for quit
                            if key.code == KeyCode::Char('c') && key.modifiers.contains(KeyModifiers::CONTROL) {
                                if app.pending_quit {
                                    // Second Ctrl+C - quit immediately
                                    app.quit();
                                } else {
                                    // First Ctrl+C - set pending and show warning
                                    app.pending_quit = true;
                                    app.set_status("Press Ctrl+C again to quit");
                                }
                            } else if key.code == KeyCode::Char('v') && key.modifiers.contains(KeyModifiers::CONTROL) {
                                // Ctrl+V - check clipboard for image
                                app.pending_quit = false;
                                if app.view == View::Chat && app.input_mode == InputMode::Editing {
                                    if let Some(keys) = app.keys.clone() {
                                        handle_clipboard_paste(app, &keys).await;
                                    }
                                }
                            } else {
                                // Any other key clears pending quit state
                                app.pending_quit = false;
                                let _span = info_span!("handle_key", key = ?key.code).entered();
                                handle_key(app, key, login_step, pending_nsec);
                            }
                        }
                        Event::Paste(text) => {
                            // Handle paste event - only in Chat view with editing mode
                            if app.view == View::Chat && app.input_mode == InputMode::Editing {
                                if app.showing_attachment_modal {
                                    // Paste into attachment modal
                                    app.attachment_modal_editor.handle_paste(&text);
                                } else {
                                    // Check if pasted text is an image file path (drag & drop)
                                    if let Some(keys) = app.keys.clone() {
                                        if !handle_image_file_paste(app, &text, &keys).await {
                                            // Not an image file - regular paste
                                            app.chat_editor.handle_paste(&text);
                                            app.save_chat_draft();
                                        }
                                    } else {
                                        app.chat_editor.handle_paste(&text);
                                        app.save_chat_draft();
                                    }
                                }
                            } else if app.input_mode == InputMode::Editing {
                                // Simple paste for login/threads views
                                for c in text.chars() {
                                    app.enter_char(c);
                                }
                            }
                        }
                        _ => {}
                    }
                }
            }

            // nostrdb notifications - events are ready to query
            Some(note_keys) = ndb_stream.next() => {
                let _span = info_span!("ndb_subscription", note_count = note_keys.len()).entered();
                handle_ndb_notes(&data_store, app, &ndb, &note_keys)?;
            }

            // Tick for regular updates (data channel polling for non-message updates)
            _ = tick_interval.tick() => {
                let _span = info_span!("check_data_updates").entered();
                app.check_for_data_updates()?;
            }
        }
    }
    Ok(())
}

/// Handle notes that nostrdb reports as ready
fn handle_ndb_notes(
    data_store: &Rc<RefCell<AppDataStore>>,
    app: &mut App,
    ndb: &Ndb,
    note_keys: &[NoteKey]
) -> Result<()> {
    let txn = nostrdb::Transaction::new(ndb)?;

    for &note_key in note_keys {
        if let Ok(note) = ndb.get_note_by_key(&txn, note_key) {
            let kind = note.kind();

            // Update data store (single source of truth)
            data_store.borrow_mut().handle_event(kind, &note);

            // Handle UI-specific updates (auto-select agent/branch, scroll, streaming)
            match kind {
                1111 => {
                    // Message - if it's for our selected thread, scroll to bottom and clear streaming
                    if let Some(ref thread) = app.selected_thread {
                        for tag in note.tags() {
                            if tag.count() >= 2 {
                                let tag_name = tag.get(0).and_then(|t| t.variant().str());
                                if tag_name == Some("E") {
                                    // Try string first, then id bytes
                                    let tag_value = if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                                        s.to_string()
                                    } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                                        hex::encode(id_bytes)
                                    } else {
                                        continue;
                                    };

                                    if tag_value == thread.id {
                                        app.scroll_offset = usize::MAX;
                                        app.streaming_accumulator.clear_message(&thread.id);
                                        break;
                                    }
                                }
                            }
                        }
                    }
                }
                24010 => {
                    // Project status - auto-select agent/branch if this is for the selected project
                    if let Some(status) = models::ProjectStatus::from_note(&note) {
                        if app.selected_project.as_ref().map(|p| p.a_tag()) == Some(status.project_coordinate.clone()) {
                            if app.selected_agent.is_none() {
                                if let Some(pm) = status.pm_agent() {
                                    app.selected_agent = Some(pm.clone());
                                }
                            }
                            if app.selected_branch.is_none() {
                                app.selected_branch = status.default_branch().map(String::from);
                            }
                        }
                    }
                }
                _ => {}
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

    // Determine chrome color based on pending_quit state
    let chrome_color = if app.pending_quit { Color::Red } else { Color::Cyan };
    let border_style = if app.pending_quit {
        Style::default().fg(Color::Red)
    } else {
        Style::default()
    };

    // Header
    let title = match app.view {
        View::Login => "TENEX - Login",
        View::Projects => "TENEX - Projects",
        View::Threads => "TENEX - Threads",
        View::Chat => "TENEX - Chat",
    };
    let header = Paragraph::new(title)
        .style(Style::default().fg(chrome_color))
        .block(Block::default().borders(Borders::BOTTOM).border_style(border_style));
    f.render_widget(header, chunks[0]);

    // Main content
    match app.view {
        View::Login => render_login(f, app, chunks[1], login_step),
        View::Projects => ui::views::render_projects(f, app, chunks[1]),
        View::Threads => ui::views::render_threads(f, app, chunks[1]),
        View::Chat => ui::views::render_chat(f, app, chunks[1]),
    }

    // Footer - show quit warning if pending, otherwise normal hints
    let (footer_text, footer_style) = if app.pending_quit {
        ("⚠ Press Ctrl+C again to quit".to_string(), Style::default().fg(Color::Red))
    } else {
        let text = match (&app.view, &app.input_mode) {
            (View::Login, InputMode::Editing) => format!("> {}", "*".repeat(app.input.len())),
            (View::Projects, _) => "Type to filter · Tab expand offline · Enter select · q quit".to_string(),
            (_, InputMode::Normal) => "Press 'q' to quit".to_string(),
            _ => String::new(), // Chat/Threads editing has its own input box
        };
        (text, Style::default().fg(Color::DarkGray))
    };
    let footer = Paragraph::new(footer_text)
        .style(footer_style)
        .block(Block::default().borders(Borders::TOP).border_style(border_style));
    f.render_widget(footer, chunks[2]);
}

fn handle_key(app: &mut App, key: KeyEvent, login_step: &mut LoginStep, pending_nsec: &mut Option<String>) {
    let code = key.code;

    // Handle attachment modal when open
    if app.showing_attachment_modal {
        handle_attachment_modal_key(app, key);
        return;
    }

    // Handle agent selector when open
    if app.showing_agent_selector {
        match code {
            KeyCode::Up => {
                if app.agent_selector_index > 0 {
                    app.agent_selector_index -= 1;
                }
            }
            KeyCode::Down => {
                let max = app.filtered_agents().len().saturating_sub(1);
                if app.agent_selector_index < max {
                    app.agent_selector_index += 1;
                }
            }
            KeyCode::Enter => {
                let filtered = app.filtered_agents();
                if let Some(agent) = filtered.get(app.agent_selector_index) {
                    let agent_name = agent.name.clone();
                    app.select_filtered_agent_by_index(app.agent_selector_index);
                    // Insert @agent_name into chat editor
                    let mention = format!("@{} ", agent_name);
                    for c in mention.chars() {
                        app.chat_editor.insert_char(c);
                    }
                    app.save_chat_draft();
                }
                app.showing_agent_selector = false;
                app.selector_filter.clear();
            }
            KeyCode::Esc => {
                app.showing_agent_selector = false;
                app.selector_filter.clear();
            }
            KeyCode::Backspace => {
                app.selector_filter.pop();
                app.agent_selector_index = 0;
            }
            KeyCode::Char(c) => {
                app.selector_filter.push(c);
                app.agent_selector_index = 0;
            }
            _ => {}
        }
        return;
    }

    // Handle branch selector when open
    if app.showing_branch_selector {
        match code {
            KeyCode::Up => {
                if app.branch_selector_index > 0 {
                    app.branch_selector_index -= 1;
                }
            }
            KeyCode::Down => {
                let max = app.filtered_branches().len().saturating_sub(1);
                if app.branch_selector_index < max {
                    app.branch_selector_index += 1;
                }
            }
            KeyCode::Enter => {
                app.select_branch_by_index(app.branch_selector_index);
                app.showing_branch_selector = false;
                app.selector_filter.clear();
            }
            KeyCode::Esc => {
                app.showing_branch_selector = false;
                app.selector_filter.clear();
            }
            KeyCode::Backspace => {
                app.selector_filter.pop();
                app.branch_selector_index = 0;
            }
            KeyCode::Char(c) => {
                app.selector_filter.push(c);
                app.branch_selector_index = 0;
            }
            _ => {}
        }
        return;
    }

    // Handle Chat view with rich text editor
    if app.view == View::Chat && app.input_mode == InputMode::Editing {
        handle_chat_editor_key(app, key);
        return;
    }

    match app.input_mode {
        InputMode::Normal => match code {
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
                View::Threads if app.selected_thread_index < app.threads().len().saturating_sub(1) => {
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
                    if let Some((project, _is_online)) = ui::views::get_project_at_index(app, app.selected_project_index) {
                        let a_tag = project.a_tag();
                        app.selected_project = Some(project);

                        // Load threads for this project into data store
                        app.data_store.borrow_mut().reload_threads_for_project(&a_tag);

                        // Auto-select PM agent and default branch from status
                        if let Some(status) = app.data_store.borrow().get_project_status(&a_tag) {
                            if app.selected_agent.is_none() {
                                if let Some(pm) = status.pm_agent() {
                                    app.selected_agent = Some(pm.clone());
                                }
                            }
                            if app.selected_branch.is_none() {
                                app.selected_branch = status.default_branch().map(String::from);
                            }
                        }

                        app.selected_thread_index = 0;
                        app.project_filter.clear();
                        app.view = View::Threads;
                    }
                }
                View::Threads => {
                    let threads = app.threads();
                    if !threads.is_empty() && app.selected_thread_index < threads.len() {
                        let _span = info_span!("enter_chat_view").entered();
                        tracing::info!("Entering chat view");

                        let thread = threads[app.selected_thread_index].clone();
                        app.selected_thread = Some(thread.clone());

                        // Load messages for this thread into data store
                        app.data_store.borrow_mut().reload_messages_for_thread(&thread.id);

                        // Auto-select first available agent if none selected
                        {
                            let _span = info_span!("select_agent").entered();
                            if app.selected_agent.is_none() {
                                app.select_agent_by_index(0);
                            }
                        }

                        // Scroll to bottom of chat and auto-focus input
                        app.scroll_offset = usize::MAX;
                        app.view = View::Chat;
                        app.input_mode = InputMode::Editing; // Auto-focus input

                        // Restore any saved draft for this thread
                        app.restore_chat_draft();

                        tracing::info!("Chat view ready");
                    }
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
                View::Chat => {
                    app.save_chat_draft();
                    app.chat_editor.clear();
                    app.view = View::Threads;
                }
                _ => {}
            },
            _ => {}
        },
        // Editing mode for non-Chat views (Login, Threads)
        InputMode::Editing => match code {
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
                                let branch = app.selected_branch.clone();

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
                    _ => {}
                }
            }
            _ => {}
        },
    }
}

/// Handle key events for the chat editor (rich text editing)
fn handle_chat_editor_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);

    match code {
        // Ctrl+Enter = newline
        KeyCode::Enter if has_ctrl => {
            app.chat_editor.insert_newline();
            app.save_chat_draft();
        }
        // Enter = send message
        KeyCode::Enter => {
            let content = app.chat_editor.submit();
            if !content.is_empty() {
                if let (Some(ref command_tx), Some(ref thread), Some(ref project)) =
                    (&app.command_tx, &app.selected_thread, &app.selected_project)
                {
                    let thread_id = thread.id.clone();
                    let project_a_tag = project.a_tag();
                    let agent_pubkey = app.selected_agent.as_ref().map(|a| a.pubkey.clone());
                    let branch = app.selected_branch.clone();
                    // NIP-22: lowercase "e" tag references the FIRST (oldest) event in current view
                    let reply_to = app.messages().first().map(|m| m.id.clone());

                    if let Err(e) = command_tx.send(NostrCommand::PublishMessage {
                        thread_id,
                        project_a_tag,
                        content,
                        agent_pubkey,
                        reply_to,
                        branch,
                    }) {
                        app.set_status(&format!("Failed to publish message: {}", e));
                    } else {
                        app.delete_chat_draft();
                    }
                }
            }
        }
        // Esc = go back to threads
        KeyCode::Esc => {
            app.save_chat_draft();
            app.input_mode = InputMode::Normal;
            app.chat_editor.clear();
            app.view = View::Threads;
        }
        // Tab = cycle focus between input and attachments
        KeyCode::Tab if !app.chat_editor.attachments.is_empty() => {
            app.chat_editor.cycle_focus();
            // If focused on an attachment, open the modal
            if app.chat_editor.focused_attachment.is_some() {
                app.open_attachment_modal();
            }
        }
        // @ = open agent selector
        KeyCode::Char('@') => {
            app.showing_agent_selector = true;
            app.agent_selector_index = 0;
            app.selector_filter.clear();
        }
        // % = open branch selector
        KeyCode::Char('%') => {
            app.showing_branch_selector = true;
            app.branch_selector_index = 0;
            app.selector_filter.clear();
        }
        // Ctrl+A = move to beginning of line
        KeyCode::Char('a') if has_ctrl => {
            app.chat_editor.move_to_line_start();
        }
        // Ctrl+E = move to end of line
        KeyCode::Char('e') if has_ctrl => {
            app.chat_editor.move_to_line_end();
        }
        // Ctrl+K = kill to end of line
        KeyCode::Char('k') if has_ctrl => {
            app.chat_editor.kill_to_line_end();
            app.save_chat_draft();
        }
        // Alt+Left = word left
        KeyCode::Left if has_alt => {
            app.chat_editor.move_word_left();
        }
        // Alt+Right = word right
        KeyCode::Right if has_alt => {
            app.chat_editor.move_word_right();
        }
        // Basic navigation
        KeyCode::Left => {
            app.chat_editor.move_left();
        }
        KeyCode::Right => {
            app.chat_editor.move_right();
        }
        KeyCode::Backspace => {
            // If an attachment is focused, delete it
            if app.chat_editor.focused_attachment.is_some() {
                app.chat_editor.delete_focused_attachment();
            } else {
                app.chat_editor.delete_char_before();
            }
            app.save_chat_draft();
        }
        KeyCode::Delete => {
            // If an attachment is focused, delete it
            if app.chat_editor.focused_attachment.is_some() {
                app.chat_editor.delete_focused_attachment();
            } else {
                app.chat_editor.delete_char_at();
            }
            app.save_chat_draft();
        }
        // Scrolling while editing
        KeyCode::Up if has_ctrl => {
            app.scroll_offset = app.scroll_offset.saturating_sub(3);
        }
        KeyCode::Down if has_ctrl => {
            app.scroll_offset = app.scroll_offset.saturating_add(3);
        }
        KeyCode::PageUp => {
            app.scroll_offset = app.scroll_offset.saturating_sub(20);
        }
        KeyCode::PageDown => {
            app.scroll_offset = app.scroll_offset.saturating_add(20);
        }
        // Regular character input
        KeyCode::Char(c) => {
            app.chat_editor.insert_char(c);
            app.save_chat_draft();
        }
        _ => {}
    }
}

/// Handle key events for the attachment modal
fn handle_attachment_modal_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);

    match code {
        // Esc = close modal without saving
        KeyCode::Esc => {
            app.cancel_attachment_modal();
        }
        // Ctrl+S = save and close
        KeyCode::Char('s') if has_ctrl => {
            app.save_and_close_attachment_modal();
        }
        // Ctrl+D = delete attachment
        KeyCode::Char('d') if has_ctrl => {
            app.delete_attachment_and_close_modal();
        }
        // Enter = newline in modal
        KeyCode::Enter => {
            app.attachment_modal_editor.insert_newline();
        }
        // Ctrl+A = move to beginning of line
        KeyCode::Char('a') if has_ctrl => {
            app.attachment_modal_editor.move_to_line_start();
        }
        // Ctrl+E = move to end of line
        KeyCode::Char('e') if has_ctrl => {
            app.attachment_modal_editor.move_to_line_end();
        }
        // Ctrl+K = kill to end of line
        KeyCode::Char('k') if has_ctrl => {
            app.attachment_modal_editor.kill_to_line_end();
        }
        // Alt+Left = word left
        KeyCode::Left if has_alt => {
            app.attachment_modal_editor.move_word_left();
        }
        // Alt+Right = word right
        KeyCode::Right if has_alt => {
            app.attachment_modal_editor.move_word_right();
        }
        // Basic navigation
        KeyCode::Left => {
            app.attachment_modal_editor.move_left();
        }
        KeyCode::Right => {
            app.attachment_modal_editor.move_right();
        }
        KeyCode::Backspace => {
            app.attachment_modal_editor.delete_char_before();
        }
        KeyCode::Delete => {
            app.attachment_modal_editor.delete_char_at();
        }
        // Regular character input
        KeyCode::Char(c) => {
            app.attachment_modal_editor.insert_char(c);
        }
        _ => {}
    }
}

/// Handle clipboard paste - checks for images and uploads to Blossom
async fn handle_clipboard_paste(app: &mut App, keys: &nostr_sdk::Keys) {
    use arboard::Clipboard;

    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(_e) => {
            return;
        }
    };

    // Check for image in clipboard
    if let Ok(image) = clipboard.get_image() {
        app.set_status("Uploading image...");

        // Convert to PNG bytes
        let png_data = match image_to_png(&image) {
            Ok(data) => data,
            Err(e) => {
                app.set_status(&format!("Failed to convert image: {}", e));
                return;
            }
        };

        // Upload to Blossom
        match nostr::upload_image(&png_data, keys, "image/png").await {
            Ok(url) => {
                // Add as image attachment and insert marker
                let id = app.chat_editor.add_image_attachment(url);
                let marker = format!("[Image #{}] ", id);
                for c in marker.chars() {
                    app.chat_editor.insert_char(c);
                }
                app.save_chat_draft();
                app.clear_status();
            }
            Err(e) => {
                app.set_status(&format!("Upload failed: {}", e));
            }
        }
    } else if let Ok(text) = clipboard.get_text() {
        // Check if clipboard text is a file path to an image
        if !handle_image_file_paste(app, &text, keys).await {
            // Fall back to regular text paste
            app.chat_editor.handle_paste(&text);
            app.save_chat_draft();
        }
    }
}

/// Check if text is an image file path and upload it if so
/// Returns true if it was an image file that was handled, false otherwise
async fn handle_image_file_paste(app: &mut App, text: &str, keys: &nostr_sdk::Keys) -> bool {
    let path = text.trim();

    // Skip if empty or doesn't look like a file path
    if path.is_empty() {
        return false;
    }

    // Handle file:// URLs (common from some terminals/apps)
    let path = if let Some(file_path) = path.strip_prefix("file://") {
        urlencoded_decode(file_path)
    } else {
        // Handle backslash-escaped spaces (from terminal drag-and-drop)
        path.replace("\\ ", " ")
    };

    // Check if it's a valid path to an image file
    let path_obj = std::path::Path::new(&path);

    // Must have an image extension
    let extension = match path_obj.extension().and_then(|e| e.to_str()) {
        Some(ext) => ext.to_lowercase(),
        None => return false,
    };

    let mime_type = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => return false,
    };

    // Check if file exists
    if !path_obj.exists() {
        return false;
    }

    // Read the file
    app.set_status("Uploading image...");
    let data = match std::fs::read(&path) {
        Ok(data) => data,
        Err(e) => {
            app.set_status(&format!("Failed to read file: {}", e));
            return true;
        }
    };

    // Upload to Blossom
    match nostr::upload_image(&data, keys, mime_type).await {
        Ok(url) => {
            // Add as image attachment and insert marker
            let id = app.chat_editor.add_image_attachment(url);
            let marker = format!("[Image #{}] ", id);
            for c in marker.chars() {
                app.chat_editor.insert_char(c);
            }
            app.save_chat_draft();
            app.clear_status();
        }
        Err(e) => {
            app.set_status(&format!("Upload failed: {}", e));
        }
    }

    true
}

/// Simple URL decoding for file paths
fn urlencoded_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            // Try to parse the next two characters as hex
            let mut hex = String::with_capacity(2);
            if let Some(&h1) = chars.peek() {
                hex.push(h1);
                chars.next();
            }
            if let Some(&h2) = chars.peek() {
                hex.push(h2);
                chars.next();
            }
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                // Invalid escape, keep original
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Convert arboard ImageData to PNG bytes
fn image_to_png(image: &arboard::ImageData) -> anyhow::Result<Vec<u8>> {
    use std::io::Cursor;

    let width = image.width as u32;
    let height = image.height as u32;

    // arboard gives us RGBA bytes
    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut png_data), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&image.bytes)?;
    }

    Ok(png_data)
}
