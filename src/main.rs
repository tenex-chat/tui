mod models;
mod nostr;
mod store;
mod streaming;
mod tracing_setup;
mod ui;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
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
use tracing::{debug, info_span, warn};

use nostr::{DataChange, NostrCommand, NostrWorker};
use nostrdb::{FilterBuilder, Ndb, NoteKey, SubscriptionStream};
use std::sync::mpsc;
use store::AppDataStore;

use ui::views::login::{render_login, LoginStep};
use ui::{App, HomeTab, InputMode, NewThreadField, RecentPanelFocus, View};

#[tokio::main]
async fn main() -> Result<()> {
    // Set up panic hook to restore terminal on panic
    let original_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        // Restore terminal before showing panic
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(
            std::io::stdout(),
            crossterm::terminal::LeaveAlternateScreen,
            crossterm::event::DisableMouseCapture
        );
        // Print panic info to stderr
        eprintln!("\n\n=== PANIC ===");
        eprintln!("{}", panic_info);
        eprintln!("=============\n");
        // Call original hook
        original_hook(panic_info);
    }));

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
        if nostr::credentials_need_password(&app.db.credentials_conn()) {
            // Password required - show unlock prompt with autofocus
            app.input_mode = InputMode::Editing;
            LoginStep::Unlock
        } else {
            // No password - auto-login with unencrypted credentials
            match nostr::load_unencrypted_keys(&app.db.credentials_conn()) {
                Ok(keys) => {
                    let user_pubkey = nostr::get_current_pubkey(&keys);
                    app.keys = Some(keys.clone());
                    app.data_store.borrow_mut().set_user_pubkey(user_pubkey.clone());

                    if let Err(e) = command_tx.send(NostrCommand::Connect {
                        keys: keys.clone(),
                        user_pubkey: user_pubkey.clone(),
                    }) {
                        app.set_status(&format!("Failed to connect: {}", e));
                        LoginStep::Nsec
                    } else if let Err(e) = command_tx.send(NostrCommand::Sync) {
                        app.set_status(&format!("Failed to sync: {}", e));
                        LoginStep::Nsec
                    } else {
                        app.view = View::Home;
                        LoginStep::Nsec // Won't be shown since view is Home
                    }
                }
                Err(e) => {
                    app.set_status(&format!("Failed to load credentials: {}", e));
                    LoginStep::Nsec
                }
            }
        }
    } else {
        LoginStep::Nsec
    };
    let mut pending_nsec: Option<String> = None;

    let result = run_app(&mut terminal, &mut app, data_store.clone(), ndb.clone(), &mut login_step, &mut pending_nsec).await;

    command_tx.send(NostrCommand::Shutdown).ok();
    worker_handle.join().ok();

    ui::restore_terminal()?;

    // Flush pending traces before exit
    tracing_setup::shutdown_tracing();

    if let Err(err) = result {
        eprintln!("Error: {err}");
    }

    Ok(())
}

/// Result of a background image upload
enum UploadResult {
    Success(String), // URL
    Error(String),   // Error message
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

    // Channel for receiving upload results from background tasks
    let (upload_tx, mut upload_rx) = tokio::sync::mpsc::channel::<UploadResult>(10);

    // Create nostrdb subscription for all event kinds we care about:
    // - 31933: Projects
    // - 1: Text (unified kind for threads and messages)
    // - 0: Profiles
    // - 4199: Agent definitions
    // - 24010: Project status
    // - 21111: Streaming deltas
    // - 513: Conversation metadata
    let ndb_filter = FilterBuilder::new()
        .kinds([31933, 1, 0, 4199, 24010, 21111, 513])
        .build();
    let ndb_subscription = ndb.subscribe(&[ndb_filter])?;
    let mut ndb_stream = SubscriptionStream::new((*ndb).clone(), ndb_subscription);

    let mut loop_count: u64 = 0;
    while app.running {
        loop_count += 1;
        if loop_count % 100 == 0 {
            debug!("Event loop iteration {}", loop_count);
        }

        // Render
        let _span = info_span!("render").entered();
        terminal.draw(|f| render(f, app, login_step))?;

        // Wait for events using tokio::select!
        tokio::select! {
            // Terminal UI events
            maybe_event = event_stream.next() => {
                debug!("Received terminal event");
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
                                        handle_clipboard_paste(app, &keys, upload_tx.clone());
                                    }
                                }
                            } else {
                                // Any other key clears pending quit state
                                app.pending_quit = false;
                                let _span = info_span!("handle_key", key = ?key.code).entered();
                                handle_key(app, key, login_step, pending_nsec)?;
                            }
                        }
                        Event::Mouse(mouse) => {
                            // Handle mouse scroll in Chat view
                            if app.view == View::Chat {
                                match mouse.kind {
                                    MouseEventKind::ScrollUp => {
                                        app.scroll_up(3);
                                    }
                                    MouseEventKind::ScrollDown => {
                                        app.scroll_down(3);
                                    }
                                    _ => {}
                                }
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
                                        if !handle_image_file_paste(app, &text, &keys, upload_tx.clone()) {
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
                debug!("ndb_stream received {} note keys", note_keys.len());
                let _span = info_span!("ndb_subscription", note_count = note_keys.len()).entered();
                handle_ndb_notes(&data_store, app, &ndb, &note_keys)?;
                debug!("ndb_stream processing complete");
            }

            // Tick for regular updates (data channel polling for non-message updates)
            _ = tick_interval.tick() => {
                let _span = info_span!("check_data_updates").entered();
                app.check_for_data_updates()?;
            }

            // Handle upload results from background tasks
            Some(result) = upload_rx.recv() => {
                match result {
                    UploadResult::Success(url) => {
                        let id = app.chat_editor.add_image_attachment(url);
                        let marker = format!("[Image #{}] ", id);
                        for c in marker.chars() {
                            app.chat_editor.insert_char(c);
                        }
                        app.save_chat_draft();
                        app.clear_status();
                    }
                    UploadResult::Error(msg) => {
                        app.set_status(&msg);
                    }
                }
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
    let note_count = note_keys.len();
    if note_count > 10 {
        warn!("Processing large batch of {} notes - this may cause UI lag", note_count);
    }
    debug!("handle_ndb_notes: processing {} notes", note_count);

    let txn = nostrdb::Transaction::new(ndb)?;

    for (idx, &note_key) in note_keys.iter().enumerate() {
        if let Ok(note) = ndb.get_note_by_key(&txn, note_key) {
            let kind = note.kind();
            debug!("Processing note {}/{}: kind={}", idx + 1, note_count, kind);

            // Update data store (single source of truth)
            data_store.borrow_mut().handle_event(kind, &note);

            // Handle UI-specific updates (auto-select agent/branch, scroll, streaming)
            match kind {
                1111 => {
                    // Message - check which thread it belongs to
                    let mut message_thread_id: Option<String> = None;

                    for tag in note.tags() {
                        if tag.count() >= 2 {
                            let tag_name = tag.get(0).and_then(|t| t.variant().str());
                            if tag_name == Some("E") {
                                // Try string first, then id bytes
                                message_thread_id = if let Some(s) = tag.get(1).and_then(|t| t.variant().str()) {
                                    Some(s.to_string())
                                } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                                    Some(hex::encode(id_bytes))
                                } else {
                                    None
                                };
                                break;
                            }
                        }
                    }

                    if let Some(thread_id) = message_thread_id {
                        // Mark tab as unread if it's not the active one
                        app.mark_tab_unread(&thread_id);

                        // Clear local streaming buffer when Nostr message arrives
                        // This ensures streaming content is replaced by the final message
                        app.clear_local_stream_buffer(&thread_id);

                        // Scroll to bottom if it's the current thread
                        if app.selected_thread.as_ref().map(|t| t.id.as_str()) == Some(thread_id.as_str()) {
                            app.scroll_offset = usize::MAX;
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

fn render(f: &mut Frame, app: &mut App, login_step: &LoginStep) {
    // Home view has its own chrome - give it full area
    if app.view == View::Home {
        ui::views::render_home(f, app, f.area());
        return;
    }

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
        View::Home => "TENEX - Home", // Won't reach here
        View::Threads => "TENEX - Threads",
        View::Chat => "TENEX - Chat",
        View::LessonViewer => "TENEX - Lesson",
    };
    let header = Paragraph::new(title)
        .style(Style::default().fg(chrome_color))
        .block(Block::default().borders(Borders::BOTTOM).border_style(border_style));
    f.render_widget(header, chunks[0]);

    // Main content
    match app.view {
        View::Login => render_login(f, app, chunks[1], login_step),
        View::Home => {} // Won't reach here
        View::Threads => ui::views::render_threads(f, app, chunks[1]),
        View::Chat => ui::views::render_chat(f, app, chunks[1]),
        View::LessonViewer => {
            if let Some(ref lesson_id) = app.viewing_lesson_id.clone() {
                if let Some(lesson) = app.data_store.borrow().get_lesson(lesson_id) {
                    ui::views::render_lesson_viewer(f, app, chunks[1], lesson);
                }
            }
        }
    }

    // Footer - show quit warning if pending, otherwise normal hints
    let (footer_text, footer_style) = if app.pending_quit {
        ("⚠ Press Ctrl+C again to quit".to_string(), Style::default().fg(Color::Red))
    } else {
        let text = match (&app.view, &app.input_mode) {
            (View::Login, InputMode::Editing) => format!("> {}", "*".repeat(app.input.len())),
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

fn handle_key(
    app: &mut App,
    key: KeyEvent,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
) -> Result<()> {
    let code = key.code;

    // Handle attachment modal when open
    if app.showing_attachment_modal {
        handle_attachment_modal_key(app, key);
        return Ok(());
    }

    // Handle ask modal when open
    if app.ask_modal_state.is_some() {
        handle_ask_modal_key(app, key);
        return Ok(());
    }

    // Ctrl+R is no longer needed - ask UI auto-shows when there's an unanswered ask event
    // Users just press 'i' to enter editing mode and the ask UI will appear automatically

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
        return Ok(());
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
        return Ok(());
    }

    // Handle Home view (projects modal and panel navigation)
    if app.view == View::Home {
        handle_home_view_key(app, key)?;
        return Ok(());
    }

    // Handle Chat view with rich text editor
    if app.view == View::Chat && app.input_mode == InputMode::Editing {
        handle_chat_editor_key(app, key);
        return Ok(());
    }

    // Handle tab navigation in Chat view (Normal mode)
    if app.view == View::Chat && app.input_mode == InputMode::Normal {
        let modifiers = key.modifiers;
        let has_shift = modifiers.contains(KeyModifiers::SHIFT);

        match code {
            // Number keys 1-9 to jump to tabs
            KeyCode::Char(c) if c >= '1' && c <= '9' => {
                let tab_index = (c as usize) - ('1' as usize);
                if tab_index < app.open_tabs.len() {
                    app.switch_to_tab(tab_index);
                }
                return Ok(());
            }
            // Tab key cycles through tabs (Shift+Tab = prev, Tab = next)
            KeyCode::Tab => {
                if has_shift {
                    app.prev_tab();
                } else {
                    app.next_tab();
                }
                return Ok(());
            }
            // x closes current tab
            KeyCode::Char('x') => {
                app.close_current_tab();
                return Ok(());
            }
            _ => {}
        }
    }

    match app.input_mode {
        InputMode::Normal => match code {
            KeyCode::Char('q') => {
                app.quit();
            }
            KeyCode::Char('i') => {
                app.input_mode = InputMode::Editing;
            }
            KeyCode::Char('r') => {
                if let Some(ref command_tx) = app.command_tx {
                    let tx = command_tx.clone();
                    app.set_status("Syncing...");
                    if let Err(e) = tx.send(NostrCommand::Sync) {
                        app.set_status(&format!("Sync request failed: {}", e));
                    }
                }
            }
            KeyCode::Char(c) => {
                if c == 'n' && app.view == View::Threads {
                    app.creating_thread = true;
                    app.input_mode = InputMode::Editing;
                    app.clear_input();
                } else if c == 'a' && app.view == View::Chat && !app.available_agents().is_empty() {
                    // 'a' opens agent selector
                    app.showing_agent_selector = true;
                    app.agent_selector_index = 0;
                } else if c == '@' && (app.view == View::Threads || app.view == View::Chat) && !app.available_agents().is_empty() {
                    app.showing_agent_selector = true;
                    app.agent_selector_index = 0;
                } else if c == 'o' && app.view == View::Chat {
                    app.open_first_image();
                } else if c == 'j' && app.view == View::LessonViewer {
                    app.scroll_down(3);
                } else if c == 'k' && app.view == View::LessonViewer {
                    app.scroll_up(3);
                } else if c >= '1' && c <= '5' && app.view == View::LessonViewer {
                    // Navigate to section 1-5
                    let section_index = (c as usize) - ('1' as usize);
                    if let Some(ref lesson_id) = app.viewing_lesson_id {
                        if let Some(lesson) = app.data_store.borrow().get_lesson(lesson_id) {
                            if section_index < lesson.sections().len() {
                                app.lesson_viewer_section = section_index;
                                app.scroll_offset = 0; // Reset scroll when changing sections
                            }
                        }
                    }
                }
            }
            KeyCode::Up => match app.view {
                View::Threads if app.selected_thread_index > 0 => {
                    app.selected_thread_index -= 1;
                }
                View::Chat => {
                    if app.selected_message_index > 0 {
                        app.selected_message_index -= 1;
                    }
                }
                View::LessonViewer => {
                    app.scroll_up(3);
                }
                _ => {}
            },
            KeyCode::Down => match app.view {
                View::Threads if app.selected_thread_index < app.threads().len().saturating_sub(1) => {
                    app.selected_thread_index += 1;
                }
                View::LessonViewer => {
                    app.scroll_down(3);
                }
                View::Chat => {
                    // Get display message count for bounds checking
                    let messages = app.messages();
                    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());
                    let display_count = if let Some(ref root_id) = app.subthread_root {
                        messages.iter()
                            .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                            .count()
                    } else {
                        messages.iter()
                            .filter(|m| m.reply_to.is_none() || m.reply_to.as_deref() == thread_id)
                            .count()
                    };

                    if app.selected_message_index < display_count.saturating_sub(1) {
                        app.selected_message_index += 1;
                    }
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
                    app.scroll_to_bottom();
                }
            }
            KeyCode::PageUp => {
                if app.view == View::Chat {
                    app.scroll_up(20);
                }
            }
            KeyCode::PageDown => {
                if app.view == View::Chat {
                    app.scroll_down(20);
                }
            }
            KeyCode::Enter => match app.view {
                View::Threads => {
                    let threads = app.threads();
                    if !threads.is_empty() && app.selected_thread_index < threads.len() {
                        let thread = threads[app.selected_thread_index].clone();
                        let project_a_tag = app.selected_project.as_ref().map(|p| p.a_tag()).unwrap_or_default();

                        // Open thread in a tab
                        app.open_tab(&thread, &project_a_tag);
                        app.selected_thread = Some(thread);

                        // Auto-select first available agent if none selected
                        if app.selected_agent.is_none() {
                            app.select_agent_by_index(0);
                        }

                        // Restore any saved draft for this thread
                        app.restore_chat_draft();

                        // Enter chat view with editing mode
                        app.view = View::Chat;
                        app.input_mode = InputMode::Editing;
                        app.scroll_offset = usize::MAX; // Scroll to bottom
                    }
                }
                View::Chat => {
                    // Navigate into subthread if selected message has replies
                    let messages = app.messages();
                    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());

                    // Get display messages based on current view
                    let display_messages: Vec<&crate::models::Message> = if let Some(ref root_id) = app.subthread_root {
                        messages.iter()
                            .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                            .collect()
                    } else {
                        messages.iter()
                            .filter(|m| m.reply_to.is_none() || m.reply_to.as_deref() == thread_id)
                            .collect()
                    };

                    if let Some(msg) = display_messages.get(app.selected_message_index) {
                        // Check if this message has replies
                        let has_replies = messages.iter().any(|m| m.reply_to.as_deref() == Some(msg.id.as_str()));
                        if has_replies {
                            app.enter_subthread((*msg).clone());
                        }
                    }
                }
                _ => {}
            },
            KeyCode::Esc => match app.view {
                View::Threads => app.view = View::Home,
                View::Chat => {
                    if app.in_subthread() {
                        // Exit subthread view and return to main thread view
                        app.exit_subthread();
                    } else {
                        // Exit chat and go back to threads
                        app.save_chat_draft();
                        app.chat_editor.clear();
                        app.view = View::Threads;
                    }
                }
                View::LessonViewer => {
                    // Return to home view
                    app.view = View::Home;
                    app.viewing_lesson_id = None;
                    app.lesson_viewer_section = 0;
                    app.scroll_offset = 0;
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
                                    app.data_store.borrow_mut().set_user_pubkey(user_pubkey.clone());

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
                                            app.view = View::Home;
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
                                    app.data_store.borrow_mut().set_user_pubkey(user_pubkey.clone());

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
                                            app.view = View::Home;
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

    Ok(())
}

/// Handle key events for Home view (panel navigation and projects modal)
fn handle_home_view_key(app: &mut App, key: KeyEvent) -> Result<()> {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    // Handle projects modal when showing
    if app.showing_projects_modal {
        match code {
            KeyCode::Esc => {
                app.showing_projects_modal = false;
                app.project_filter.clear();
            }
            KeyCode::Enter => {
                // Select project and go to Threads view
                if let Some((project, _)) = ui::views::get_project_at_index(app, app.selected_project_index) {
                    let a_tag = project.a_tag();
                    app.selected_project = Some(project);

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
                    app.showing_projects_modal = false;
                    app.view = View::Threads;
                }
            }
            KeyCode::Up if app.selected_project_index > 0 => {
                app.selected_project_index -= 1;
            }
            KeyCode::Down => {
                let max = ui::views::selectable_project_count(app).saturating_sub(1);
                if app.selected_project_index < max {
                    app.selected_project_index += 1;
                }
            }
            KeyCode::Char(c) => {
                app.project_filter.push(c);
                app.selected_project_index = 0;
            }
            KeyCode::Backspace => {
                app.project_filter.pop();
                app.selected_project_index = 0;
            }
            _ => {}
        }
        return Ok(());
    }

    // Handle new thread modal when showing
    if app.showing_new_thread_modal {
        match code {
            KeyCode::Esc => {
                app.close_new_thread_modal();
            }
            KeyCode::Tab => {
                app.new_thread_modal_next_field();
            }
            KeyCode::Enter => {
                match app.new_thread_modal_focus {
                    NewThreadField::Project => {
                        let projects = app.new_thread_filtered_projects();
                        if let Some(project) = projects.get(app.new_thread_project_index).cloned() {
                            app.new_thread_select_project(project);
                        }
                    }
                    NewThreadField::Agent => {
                        let agents = app.new_thread_filtered_agents();
                        if let Some(agent) = agents.get(app.new_thread_agent_index).cloned() {
                            app.new_thread_select_agent(agent);
                        }
                    }
                    NewThreadField::Content => {
                        // Submit if valid
                        if app.can_submit_new_thread() {
                            if let (Some(ref command_tx), Some(ref project), Some(ref agent)) = (
                                &app.command_tx,
                                &app.new_thread_selected_project,
                                &app.new_thread_selected_agent,
                            ) {
                                let content = app.new_thread_editor.build_full_content();
                                let project_a_tag = project.a_tag();
                                let agent_pubkey = Some(agent.pubkey.clone());

                                // Publish the thread (kind:11)
                                if let Err(e) = command_tx.send(NostrCommand::PublishThread {
                                    project_a_tag: project_a_tag.clone(),
                                    title: content.lines().next().unwrap_or("New Thread").to_string(),
                                    content: content.clone(),
                                    agent_pubkey,
                                    branch: None,
                                }) {
                                    app.set_status(&format!("Failed to publish thread: {}", e));
                                } else {
                                    app.set_last_project(&project_a_tag);
                                    app.delete_project_draft(&project_a_tag);
                                    app.close_new_thread_modal();
                                    app.set_status("Thread created");
                                }
                            }
                        }
                    }
                }
            }
            KeyCode::Up => {
                match app.new_thread_modal_focus {
                    NewThreadField::Project => {
                        if app.new_thread_project_index > 0 {
                            app.new_thread_project_index -= 1;
                        }
                    }
                    NewThreadField::Agent => {
                        if app.new_thread_agent_index > 0 {
                            app.new_thread_agent_index -= 1;
                        }
                    }
                    NewThreadField::Content => {}
                }
            }
            KeyCode::Down => {
                match app.new_thread_modal_focus {
                    NewThreadField::Project => {
                        let max = app.new_thread_filtered_projects().len().saturating_sub(1);
                        if app.new_thread_project_index < max {
                            app.new_thread_project_index += 1;
                        }
                    }
                    NewThreadField::Agent => {
                        let max = app.new_thread_filtered_agents().len().saturating_sub(1);
                        if app.new_thread_agent_index < max {
                            app.new_thread_agent_index += 1;
                        }
                    }
                    NewThreadField::Content => {}
                }
            }
            KeyCode::Char(c) => {
                match app.new_thread_modal_focus {
                    NewThreadField::Project => {
                        app.new_thread_project_filter.push(c);
                        app.new_thread_project_index = 0;
                    }
                    NewThreadField::Agent => {
                        app.new_thread_agent_filter.push(c);
                        app.new_thread_agent_index = 0;
                    }
                    NewThreadField::Content => {
                        app.new_thread_editor.insert_char(c);
                    }
                }
            }
            KeyCode::Backspace => {
                match app.new_thread_modal_focus {
                    NewThreadField::Project => {
                        app.new_thread_project_filter.pop();
                        app.new_thread_project_index = 0;
                    }
                    NewThreadField::Agent => {
                        app.new_thread_agent_filter.pop();
                        app.new_thread_agent_index = 0;
                    }
                    NewThreadField::Content => {
                        app.new_thread_editor.delete_char_before();
                    }
                }
            }
            _ => {}
        }
        return Ok(());
    }

    // Normal Home view navigation
    match code {
        KeyCode::Char('q') => app.quit(),
        KeyCode::Char('p') => {
            app.showing_projects_modal = true;
            app.project_filter.clear();
            app.selected_project_index = 0;
        }
        KeyCode::Char('n') => {
            app.open_new_thread_modal();
        }
        KeyCode::Tab => {
            // Switch between tabs (forward)
            app.home_panel_focus = match app.home_panel_focus {
                HomeTab::Recent => HomeTab::Inbox,
                HomeTab::Inbox => HomeTab::Projects,
                HomeTab::Projects => HomeTab::Recent,
            };
        }
        KeyCode::BackTab if has_shift => {
            // Shift+Tab switches tabs (backward)
            app.home_panel_focus = match app.home_panel_focus {
                HomeTab::Recent => HomeTab::Projects,
                HomeTab::Inbox => HomeTab::Recent,
                HomeTab::Projects => HomeTab::Inbox,
            };
        }
        KeyCode::Left if app.home_panel_focus == HomeTab::Recent => {
            app.recent_panel_focus = RecentPanelFocus::Conversations;
        }
        KeyCode::Right if app.home_panel_focus == HomeTab::Recent => {
            app.recent_panel_focus = RecentPanelFocus::Feed;
        }
        KeyCode::Up => {
            match app.home_panel_focus {
                HomeTab::Inbox => {
                    if app.selected_inbox_index > 0 {
                        app.selected_inbox_index -= 1;
                    }
                }
                HomeTab::Recent => {
                    match app.recent_panel_focus {
                        RecentPanelFocus::Conversations => {
                            if app.selected_recent_index > 0 {
                                app.selected_recent_index -= 1;
                            }
                        }
                        RecentPanelFocus::Feed => {
                            if app.selected_feed_index > 0 {
                                app.selected_feed_index -= 1;
                            }
                        }
                    }
                }
                HomeTab::Projects => {
                    if app.selected_project_index > 0 {
                        app.selected_project_index -= 1;
                    }
                }
            }
        }
        KeyCode::Down => {
            match app.home_panel_focus {
                HomeTab::Inbox => {
                    let max = app.inbox_items().len().saturating_sub(1);
                    if app.selected_inbox_index < max {
                        app.selected_inbox_index += 1;
                    }
                }
                HomeTab::Recent => {
                    match app.recent_panel_focus {
                        RecentPanelFocus::Conversations => {
                            let max = app.recent_threads().len().saturating_sub(1);
                            if app.selected_recent_index < max {
                                app.selected_recent_index += 1;
                            }
                        }
                        RecentPanelFocus::Feed => {
                            let max = app.agent_chatter().len().saturating_sub(1);
                            if app.selected_feed_index < max {
                                app.selected_feed_index += 1;
                            }
                        }
                    }
                }
                HomeTab::Projects => {
                    let (online, offline) = app.filtered_projects();
                    let max = (online.len() + offline.len()).saturating_sub(1);
                    if app.selected_project_index < max {
                        app.selected_project_index += 1;
                    }
                }
            }
        }
        KeyCode::Enter => {
            match app.home_panel_focus {
                HomeTab::Inbox => {
                    let items = app.inbox_items();
                    if let Some(item) = items.get(app.selected_inbox_index) {
                        // Mark as read
                        let item_id = item.id.clone();
                        app.data_store.borrow_mut().mark_inbox_read(&item_id);

                        // Navigate to thread if available
                        if let Some(ref thread_id) = item.thread_id {
                            let project_a_tag = item.project_a_tag.clone();

                            // Find the thread
                            let thread = app.data_store.borrow().get_threads(&project_a_tag)
                                .iter()
                                .find(|t| t.id == *thread_id)
                                .cloned();

                            if let Some(thread) = thread {
                                app.open_thread_from_home(&thread, &project_a_tag);
                            }
                        }
                    }
                }
                HomeTab::Recent => {
                    match app.recent_panel_focus {
                        RecentPanelFocus::Conversations => {
                            let recent = app.recent_threads();
                            if let Some((thread, a_tag)) = recent.get(app.selected_recent_index).cloned() {
                                app.open_thread_from_home(&thread, &a_tag);
                            }
                        }
                        RecentPanelFocus::Feed => {
                            let chatter = app.agent_chatter();
                            if let Some(item) = chatter.get(app.selected_feed_index).cloned() {
                                use crate::models::AgentChatter;
                                match item {
                                    AgentChatter::Message { thread_id, project_a_tag, .. } => {
                                        // Find the thread
                                        let thread = app.data_store.borrow().get_threads(&project_a_tag)
                                            .iter()
                                            .find(|t| t.id == thread_id)
                                            .cloned();

                                        if let Some(thread) = thread {
                                            app.open_thread_from_home(&thread, &project_a_tag);
                                        }
                                    }
                                    AgentChatter::Lesson { id, .. } => {
                                        // Open lesson viewer
                                        app.viewing_lesson_id = Some(id);
                                        app.lesson_viewer_section = 0;
                                        app.scroll_offset = 0;
                                        app.view = View::LessonViewer;
                                    }
                                }
                            }
                        }
                    }
                }
                HomeTab::Projects => {
                    // Select project and go to threads view
                    let (online, offline) = app.filtered_projects();
                    let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();

                    if let Some(project) = all_projects.get(app.selected_project_index) {
                        app.selected_project = Some((*project).clone());
                        app.view = View::Threads;
                        app.selected_thread_index = 0;
                    }
                }
            }
        }
        KeyCode::Char('r') if app.home_panel_focus == HomeTab::Inbox => {
            // Mark current inbox item as read
            let items = app.inbox_items();
            if let Some(item) = items.get(app.selected_inbox_index) {
                let item_id = item.id.clone();
                app.data_store.borrow_mut().mark_inbox_read(&item_id);
            }
        }
        // Number keys for tab switching (same as Chat view)
        KeyCode::Char(c) if c >= '1' && c <= '9' => {
            let tab_index = (c as usize) - ('1' as usize);
            if tab_index < app.open_tabs.len() {
                app.switch_to_tab(tab_index);
                app.view = View::Chat;
            }
        }
        _ => {}
    }
    Ok(())
}

/// Handle key events for the chat editor (rich text editing)
fn handle_chat_editor_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);

    match code {
        // Alt+Enter = newline
        KeyCode::Enter if has_alt => {
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
                    // NIP-22: lowercase "e" tag references the parent message
                    // When in subthread, reply to the subthread root
                    // When in main view, reply to the thread root (or first message)
                    let reply_to = if let Some(ref root_id) = app.subthread_root {
                        Some(root_id.clone())
                    } else {
                        Some(thread_id.clone())
                    };

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
        // Esc = exit input mode (then navigate back via normal mode Esc)
        KeyCode::Esc => {
            app.save_chat_draft();
            app.input_mode = InputMode::Normal;
        }
        // Tab = cycle focus between input and attachments
        KeyCode::Tab if app.chat_editor.has_attachments() => {
            app.chat_editor.cycle_focus();
            // If focused on a paste attachment, open the modal (not for images)
            if app.chat_editor.get_focused_attachment().is_some() {
                app.open_attachment_modal();
            }
        }
        // Up = focus attachments (when there are any)
        KeyCode::Up if app.chat_editor.has_attachments() && app.chat_editor.focused_attachment.is_none() => {
            app.chat_editor.focus_attachments();
        }
        // Down = unfocus attachments (return to input)
        KeyCode::Down if app.chat_editor.focused_attachment.is_some() => {
            app.chat_editor.unfocus_attachments();
        }
        // Left/Right = navigate between attachments when focused
        KeyCode::Left if app.chat_editor.focused_attachment.is_some() => {
            if let Some(idx) = app.chat_editor.focused_attachment {
                if idx > 0 {
                    app.chat_editor.focused_attachment = Some(idx - 1);
                }
            }
        }
        KeyCode::Right if app.chat_editor.focused_attachment.is_some() => {
            if let Some(idx) = app.chat_editor.focused_attachment {
                let total = app.chat_editor.total_attachments();
                if idx + 1 < total {
                    app.chat_editor.focused_attachment = Some(idx + 1);
                }
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
            app.scroll_up(3);
        }
        KeyCode::Down if has_ctrl => {
            app.scroll_down(3);
        }
        KeyCode::PageUp => {
            app.scroll_up(20);
        }
        KeyCode::PageDown => {
            app.scroll_down(20);
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
fn handle_ask_modal_key(app: &mut App, key: KeyEvent) {
    use crate::ui::ask_input::InputMode as AskInputMode;

    let code = key.code;
    let modifiers = key.modifiers;

    // Extract modal_state to avoid borrow issues
    let modal_state = match &mut app.ask_modal_state {
        Some(state) => state,
        None => return,
    };

    let input_state = &mut modal_state.input_state;

    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    match input_state.mode {
        AskInputMode::Selection => {
            match code {
                KeyCode::Up | KeyCode::Char('k') => {
                    input_state.prev_option();
                }
                KeyCode::Down | KeyCode::Char('j') => {
                    input_state.next_option();
                }
                KeyCode::Right => {
                    // Skip this question
                    input_state.skip_question();
                    if input_state.is_complete() {
                        submit_ask_response(app);
                    }
                }
                KeyCode::Left => {
                    // Go back to previous question
                    input_state.prev_question();
                }
                KeyCode::Char(' ') if input_state.is_multi_select() => {
                    input_state.toggle_multi_select();
                }
                KeyCode::Enter => {
                    input_state.select_current_option();
                    if input_state.is_complete() {
                        submit_ask_response(app);
                    }
                }
                KeyCode::Esc => {
                    app.close_ask_modal();
                }
                _ => {}
            }
        }
        AskInputMode::CustomInput => {
            match code {
                KeyCode::Enter if has_shift => {
                    // Shift+Enter adds newline
                    input_state.insert_char('\n');
                }
                KeyCode::Enter => {
                    // Enter submits custom input
                    input_state.submit_custom_answer();
                    if input_state.is_complete() {
                        submit_ask_response(app);
                    }
                }
                KeyCode::Esc => {
                    input_state.cancel_custom_mode();
                }
                KeyCode::Left => {
                    input_state.move_cursor_left();
                }
                KeyCode::Right => {
                    input_state.move_cursor_right();
                }
                KeyCode::Backspace => {
                    input_state.delete_char();
                }
                KeyCode::Char(c) => {
                    input_state.insert_char(c);
                }
                _ => {}
            }
        }
    }
}

fn submit_ask_response(app: &mut App) {
    let modal_state = match app.ask_modal_state.take() {
        Some(state) => state,
        None => return,
    };

    let response_text = modal_state.input_state.format_response();
    let message_id = modal_state.message_id;

    // Send reply to the ask event
    if let (Some(ref command_tx), Some(ref thread), Some(ref project)) =
        (&app.command_tx, &app.selected_thread, &app.selected_project)
    {
        let _ = command_tx.send(NostrCommand::PublishMessage {
            thread_id: thread.id.clone(),
            project_a_tag: project.a_tag(),
            content: response_text,
            agent_pubkey: None,
            reply_to: Some(message_id),
            branch: None,
        });
    }

    app.input_mode = InputMode::Editing;
}

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
fn handle_clipboard_paste(app: &mut App, keys: &nostr_sdk::Keys, upload_tx: tokio::sync::mpsc::Sender<UploadResult>) {
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

        // Spawn background upload task
        let keys = keys.clone();
        tokio::spawn(async move {
            let result = match nostr::upload_image(&png_data, &keys, "image/png").await {
                Ok(url) => UploadResult::Success(url),
                Err(e) => UploadResult::Error(format!("Upload failed: {}", e)),
            };
            let _ = upload_tx.send(result).await;
        });
    } else if let Ok(text) = clipboard.get_text() {
        // Check if clipboard text is a file path to an image
        if !handle_image_file_paste(app, &text, keys, upload_tx) {
            // Fall back to regular text paste
            app.chat_editor.handle_paste(&text);
            app.save_chat_draft();
        }
    }
}

/// Check if text is an image file path and upload it if so
/// Returns true if it was an image file that was handled, false otherwise
fn handle_image_file_paste(app: &mut App, text: &str, keys: &nostr_sdk::Keys, upload_tx: tokio::sync::mpsc::Sender<UploadResult>) -> bool {
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

    // Spawn background upload task
    let keys = keys.clone();
    let mime_type = mime_type.to_string();
    tokio::spawn(async move {
        let result = match nostr::upload_image(&data, &keys, &mime_type).await {
            Ok(url) => UploadResult::Success(url),
            Err(e) => UploadResult::Error(format!("Upload failed: {}", e)),
        };
        let _ = upload_tx.send(result).await;
    });

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
