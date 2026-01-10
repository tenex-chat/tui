mod ui;

pub use tenex_core::models;
pub use tenex_core::nostr;
pub use tenex_core::store;
pub use tenex_core::streaming;
pub use tenex_core::tracing_setup;

use anyhow::Result;
use crossterm::event::{Event, EventStream, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseEventKind};
use futures::StreamExt;
use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Block, Paragraph},
    Frame,
};
use std::time::Duration;
use tracing::{debug, info_span};

use nostr::NostrCommand;
use tenex_core::config::CoreConfig;
use tenex_core::events::CoreEvent;
use tenex_core::runtime::CoreRuntime;

use ui::views::login::{render_login, LoginStep};
use ui::views::home::get_hierarchical_threads;
use ui::views::chat::{group_messages, DisplayItem};
use ui::{App, HomeTab, InputMode, ModalState, View};
use ui::selector::{handle_selector_key, SelectorAction};

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

    let mut core_runtime = CoreRuntime::new(CoreConfig::default())?;
    let data_store = core_runtime.data_store();
    let db = core_runtime.database();
    let mut app = App::new(db.clone(), data_store);
    let mut terminal = ui::init_terminal()?;
    let core_handle = core_runtime.handle();
    let data_rx = core_runtime
        .take_data_rx()
        .ok_or_else(|| anyhow::anyhow!("Core runtime already has active data receiver"))?;
    app.set_core_handle(core_handle.clone(), data_rx);

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

                    if let Err(e) = core_handle.send(NostrCommand::Connect {
                        keys: keys.clone(),
                        user_pubkey: user_pubkey.clone(),
                    }) {
                        app.set_status(&format!("Failed to connect: {}", e));
                        LoginStep::Nsec
                    } else if let Err(e) = core_handle.send(NostrCommand::Sync) {
                        app.set_status(&format!("Failed to sync: {}", e));
                        LoginStep::Nsec
                    } else {
                        app.view = View::Home;
                        app.load_filter_preferences();
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

    let result = run_app(
        &mut terminal,
        &mut app,
        &mut core_runtime,
        &mut login_step,
        &mut pending_nsec,
    )
    .await;

    core_runtime.shutdown();

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
    core_runtime: &mut CoreRuntime,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
) -> Result<()> {
    // Create async event stream for terminal events
    let mut event_stream = EventStream::new();

    // Create a tick interval for regular updates (data channel polling, etc.)
    let mut tick_interval = tokio::time::interval(Duration::from_millis(50));

    // Channel for receiving upload results from background tasks
    let (upload_tx, mut upload_rx) = tokio::sync::mpsc::channel::<UploadResult>(10);

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
                                app.prefix_key_active = false;
                                if app.view == View::Chat && app.input_mode == InputMode::Editing {
                                    if let Some(keys) = app.keys.clone() {
                                        handle_clipboard_paste(app, &keys, upload_tx.clone());
                                    }
                                }
                            } else if key.code == KeyCode::Char('t') && key.modifiers.contains(KeyModifiers::CONTROL) {
                                // Ctrl+T - activate prefix key mode (tmux-style)
                                app.pending_quit = false;
                                app.prefix_key_active = true;
                            } else if app.prefix_key_active {
                                // Handle prefix key commands (Ctrl+T + key)
                                app.prefix_key_active = false;
                                app.pending_quit = false;
                                handle_prefix_key(app, key);
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
                                    app.attachment_modal_editor_mut().handle_paste(&text);
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
            Some(note_keys) = core_runtime.next_note_keys() => {
                debug!("core_runtime received {} note keys", note_keys.len());
                let _span = info_span!("ndb_subscription", note_count = note_keys.len()).entered();
                let events = core_runtime.process_note_keys(&note_keys)?;
                handle_core_events(app, events);

                // Check for pending new thread and navigate to it if found
                check_pending_new_thread(app);

                debug!("core_runtime processing complete");
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

fn handle_core_events(app: &mut App, events: Vec<CoreEvent>) {
    for event in events {
        match event {
            CoreEvent::Message(message) => {
                let thread_id = message.thread_id;

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
            CoreEvent::ProjectStatus(status) => {
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
    }
}

/// Check if a pending new thread has arrived and navigate to it
fn check_pending_new_thread(app: &mut App) {
    let Some(project_a_tag) = app.pending_new_thread_project.clone() else {
        return;
    };

    let user_pubkey = app.keys.as_ref().map(|k| k.public_key().to_hex());
    let Some(user_pubkey) = user_pubkey else {
        return;
    };

    // Find the most recent thread from user (threads sorted by last_activity desc)
    let thread = {
        let store = app.data_store.borrow();
        store.get_threads(&project_a_tag)
            .iter()
            .find(|t| t.pubkey == user_pubkey)
            .cloned()
    };

    if let Some(thread) = thread {
        app.pending_new_thread_project = None;
        app.creating_thread = false;
        app.open_thread_from_home(&thread, &project_a_tag);
    }
}

fn render(f: &mut Frame, app: &mut App, login_step: &LoginStep) {
    // Fill entire frame with app background (pure black)
    let bg_block = Block::default().style(Style::default().bg(ui::theme::BG_APP));
    f.render_widget(bg_block, f.area());

    // Home view has its own chrome - give it full area
    if app.view == View::Home {
        ui::views::render_home(f, app, f.area());
        return;
    }

    // Chrome height varies by view
    let (header_height, footer_height) = match app.view {
        View::Chat => (3, 2), // More padding for chat chrome
        _ => (1, 1),
    };

    let chunks = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Min(0),
        Constraint::Length(footer_height),
    ])
    .split(f.area());

    // Determine chrome color based on pending_quit state
    let chrome_color = if app.pending_quit { ui::theme::ACCENT_ERROR } else { ui::theme::ACCENT_PRIMARY };

    // Header
    let title: String = match app.view {
        View::Login => "TENEX - Login".to_string(),
        View::Home => "TENEX - Home".to_string(), // Won't reach here
        View::Chat => app.selected_thread.as_ref()
            .map(|t| t.title.clone())
            .unwrap_or_else(|| "Chat".to_string()),
        View::LessonViewer => "TENEX - Lesson".to_string(),
        View::AgentBrowser => "TENEX - Agent Definitions".to_string(),
    };

    // For chat view, center the title with padding
    if app.view == View::Chat {
        let header = Paragraph::new(format!("\n  {}", title))
            .style(Style::default().fg(chrome_color).add_modifier(ratatui::style::Modifier::BOLD));
        f.render_widget(header, chunks[0]);
    } else {
        let header = Paragraph::new(title)
            .style(Style::default().fg(chrome_color));
        f.render_widget(header, chunks[0]);
    }

    // Main content
    match app.view {
        View::Login => render_login(f, app, chunks[1], login_step),
        View::Home => {} // Won't reach here
        View::Chat => ui::views::render_chat(f, app, chunks[1]),
        View::LessonViewer => {
            if let Some(ref lesson_id) = app.viewing_lesson_id.clone() {
                if let Some(lesson) = app.data_store.borrow().get_lesson(lesson_id) {
                    ui::views::render_lesson_viewer(f, app, chunks[1], lesson);
                }
            }
        }
        View::AgentBrowser => ui::views::render_agent_browser(f, app, chunks[1]),
    }

    // Footer - show quit warning if pending, otherwise normal hints
    let (footer_text, footer_style) = if app.pending_quit {
        ("⚠ Press Ctrl+C again to quit".to_string(), Style::default().fg(ui::theme::ACCENT_ERROR))
    } else {
        let text = match (&app.view, &app.input_mode) {
            (View::Login, InputMode::Editing) => format!("> {}", "*".repeat(app.input.len())),
            (View::Chat, InputMode::Normal) => {
                // Check if current thread has active operations
                let is_busy = app.selected_thread.as_ref()
                    .map(|t| app.data_store.borrow().is_event_busy(&t.id))
                    .unwrap_or(false);
                if is_busy {
                    "q quit · i edit · s stop".to_string()
                } else {
                    "q quit · i edit".to_string()
                }
            }
            (_, InputMode::Normal) => "Press 'q' to quit".to_string(),
            _ => String::new(), // Chat/Threads editing has its own input box
        };
        (text, Style::default().fg(ui::theme::TEXT_MUTED))
    };

    // For chat view, add padding to footer
    let formatted_footer = if app.view == View::Chat {
        format!("  {}", footer_text)
    } else {
        footer_text
    };
    let footer = Paragraph::new(formatted_footer)
        .style(footer_style);
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
    if app.is_attachment_modal_open() {
        handle_attachment_modal_key(app, key);
        return Ok(());
    }

    // Handle ask modal when open
    if matches!(app.modal_state, ModalState::AskModal(_)) {
        handle_ask_modal_key(app, key);
        return Ok(());
    }

    // Handle tab modal when open
    if app.showing_tab_modal {
        handle_tab_modal_key(app, key);
        return Ok(());
    }

    // Handle search modal when open
    if app.showing_search_modal {
        handle_search_modal_key(app, key);
        return Ok(());
    }

    // Handle agent selector when open (using ModalState)
    if matches!(app.modal_state, ModalState::AgentSelector { .. }) {
        // Get agents BEFORE mutably borrowing modal_state
        let agents = app.filtered_agents();
        let item_count = agents.len();

        if let ModalState::AgentSelector { ref mut selector } = app.modal_state {
            match handle_selector_key(selector, key, item_count, |idx| agents.get(idx).cloned()) {
                SelectorAction::Selected(agent) => {
                    let agent_name = agent.name.clone();
                    app.selected_agent = Some(agent);
                    // Insert @agent_name into chat editor
                    let mention = format!("@{} ", agent_name);
                    for c in mention.chars() {
                        app.chat_editor.insert_char(c);
                    }
                    app.save_chat_draft();
                    app.modal_state = ModalState::None;
                }
                SelectorAction::Cancelled => {
                    app.modal_state = ModalState::None;
                }
                SelectorAction::Continue => {}
            }
        }
        return Ok(());
    }

    // Handle branch selector when open (using ModalState)
    if matches!(app.modal_state, ModalState::BranchSelector { .. }) {
        // Get branches BEFORE mutably borrowing modal_state
        let branches = app.filtered_branches();
        let item_count = branches.len();

        if let ModalState::BranchSelector { ref mut selector } = app.modal_state {
            match handle_selector_key(selector, key, item_count, |idx| branches.get(idx).cloned()) {
                SelectorAction::Selected(branch) => {
                    app.selected_branch = Some(branch);
                    app.modal_state = ModalState::None;
                }
                SelectorAction::Cancelled => {
                    app.modal_state = ModalState::None;
                }
                SelectorAction::Continue => {}
            }
        }
        return Ok(());
    }

    // Handle message actions modal when open
    if matches!(app.modal_state, ModalState::MessageActions { .. }) {
        handle_message_actions_modal_key(app, key);
        return Ok(());
    }

    // Handle view raw event modal when open
    if matches!(app.modal_state, ModalState::ViewRawEvent { .. }) {
        handle_view_raw_event_modal_key(app, key);
        return Ok(());
    }

    // Handle hotkey help modal when open
    if matches!(app.modal_state, ModalState::HotkeyHelp) {
        handle_hotkey_help_modal_key(app, key);
        return Ok(());
    }

    // Global tab navigation with Alt key (works in all views except Login)
    // These bindings work regardless of input mode
    if app.view != View::Login {
        let modifiers = key.modifiers;
        let has_alt = modifiers.contains(KeyModifiers::ALT);
        let has_shift = modifiers.contains(KeyModifiers::SHIFT);

        if has_alt {
            match code {
                // Alt+0 = go to dashboard (home)
                KeyCode::Char('0') => {
                    app.save_chat_draft();
                    app.view = View::Home;
                    return Ok(());
                }
                // Alt+1..9 = jump directly to tab N
                KeyCode::Char(c) if c >= '1' && c <= '9' => {
                    let tab_index = (c as usize) - ('1' as usize);
                    if tab_index < app.open_tabs.len() {
                        app.switch_to_tab(tab_index);
                        app.view = View::Chat;
                    }
                    return Ok(());
                }
                // Alt+Tab = cycle forward through recently viewed tabs
                KeyCode::Tab => {
                    if has_shift {
                        app.cycle_tab_history_backward();
                    } else {
                        app.cycle_tab_history_forward();
                    }
                    if !app.open_tabs.is_empty() {
                        app.view = View::Chat;
                    }
                    return Ok(());
                }
                // Alt+/ = open tab modal
                KeyCode::Char('/') => {
                    if !app.open_tabs.is_empty() || app.view == View::Chat || app.view == View::Home {
                        app.open_tab_modal();
                    }
                    return Ok(());
                }
                _ => {}
            }
        }
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
        let has_alt = modifiers.contains(KeyModifiers::ALT);

        match code {
            // Alt+B = open branch selector
            KeyCode::Char('b') if has_alt => {
                app.open_branch_selector();
                return Ok(());
            }
            // Number keys 1-9 to jump to tabs (without Alt, for backwards compat in Normal mode)
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
            // / = open message actions modal
            KeyCode::Char('/') => {
                app.open_message_actions_modal();
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
                // Just enter editing mode - ask modal auto-opens during render
                app.input_mode = InputMode::Editing;
            }
            KeyCode::Char('r') => {
                if let Some(core_handle) = app.core_handle.clone() {
                    app.set_status("Syncing...");
                    if let Err(e) = core_handle.send(NostrCommand::Sync) {
                        app.set_status(&format!("Sync request failed: {}", e));
                    }
                }
            }
            KeyCode::Char(c) => {
                if c == 'a' && app.view == View::Chat && !app.available_agents().is_empty() {
                    // 'a' opens agent selector
                    app.open_agent_selector();
                } else if c == '@' && app.view == View::Chat && !app.available_agents().is_empty() {
                    app.open_agent_selector();
                } else if c == 's' && app.view == View::Chat {
                    // 's' stops agents working on the current thread
                    if let Some(ref thread) = app.selected_thread {
                        let (is_busy, project_a_tag) = {
                            let store = app.data_store.borrow();
                            let is_busy = store.is_event_busy(&thread.id);
                            let project_a_tag = store.find_project_for_thread(&thread.id);
                            (is_busy, project_a_tag)
                        };
                        if is_busy {
                            if let (Some(core_handle), Some(a_tag)) = (app.core_handle.clone(), project_a_tag) {
                                let working_agents = app.data_store.borrow().get_working_agents(&thread.id);
                                if let Err(e) = core_handle.send(NostrCommand::StopOperations {
                                    project_a_tag: a_tag,
                                    event_ids: vec![thread.id.clone()],
                                    agent_pubkeys: working_agents,
                                }) {
                                    app.set_status(&format!("Failed to stop: {}", e));
                                } else {
                                    app.set_status("Stop command sent");
                                }
                            }
                        }
                    }
                } else if c == 't' && app.view == View::Chat {
                    // 't' toggles todo sidebar
                    app.todo_sidebar_visible = !app.todo_sidebar_visible;
                } else if c == 'o' && app.view == View::Chat {
                    app.open_first_image();
                } else if c == 'j' && app.view == View::LessonViewer {
                    app.scroll_down(3);
                } else if c == 'k' && app.view == View::LessonViewer {
                    app.scroll_up(3);
                } else if c == 'j' && app.view == View::AgentBrowser && app.agent_browser_in_detail {
                    app.scroll_down(3);
                } else if c == 'k' && app.view == View::AgentBrowser && app.agent_browser_in_detail {
                    app.scroll_up(3);
                } else if app.view == View::AgentBrowser && !app.agent_browser_in_detail && c != 'q' {
                    // In list mode, add characters to search filter (but not 'q' for quit)
                    app.agent_browser_filter.push(c);
                    app.agent_browser_index = 0; // Reset selection when filter changes
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
            KeyCode::Backspace => {
                if app.view == View::AgentBrowser && !app.agent_browser_in_detail {
                    app.agent_browser_filter.pop();
                    app.agent_browser_index = 0;
                }
            }
            KeyCode::Up => match app.view {
                View::Chat => {
                    if app.selected_message_index > 0 {
                        app.selected_message_index -= 1;
                    }
                }
                View::LessonViewer => {
                    app.scroll_up(3);
                }
                View::AgentBrowser => {
                    if app.agent_browser_in_detail {
                        app.scroll_up(3);
                    } else if app.agent_browser_index > 0 {
                        app.agent_browser_index -= 1;
                    }
                }
                _ => {}
            },
            KeyCode::Down => match app.view {
                View::LessonViewer => {
                    app.scroll_down(3);
                }
                View::AgentBrowser => {
                    if app.agent_browser_in_detail {
                        app.scroll_down(3);
                    } else {
                        let count = app.filtered_agent_definitions().len();
                        if app.agent_browser_index < count.saturating_sub(1) {
                            app.agent_browser_index += 1;
                        }
                    }
                }
                View::Chat => {
                    // Get grouped item count for bounds checking (selected_message_index is group index)
                    let messages = app.messages();
                    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());
                    let user_pubkey = app.data_store.borrow().user_pubkey.clone();

                    let display_messages: Vec<&crate::models::Message> = if let Some(ref root_id) = app.subthread_root {
                        messages.iter()
                            .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                            .collect()
                    } else {
                        // Include thread root + direct replies
                        messages.iter()
                            .filter(|m| {
                                Some(m.id.as_str()) == thread_id
                                    || m.reply_to.is_none()
                                    || m.reply_to.as_deref() == thread_id
                            })
                            .collect()
                    };

                    // Group messages to get actual count of selectable items
                    let grouped = group_messages(&display_messages, user_pubkey.as_deref());

                    if app.selected_message_index < grouped.len().saturating_sub(1) {
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
                View::Chat => {
                    // Get messages and build grouped display model (same as rendering)
                    let messages = app.messages();
                    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());
                    let user_pubkey = app.data_store.borrow().user_pubkey.clone();

                    // Get display messages based on current view (must match rendering in messages.rs)
                    let display_messages: Vec<&crate::models::Message> = if let Some(ref root_id) = app.subthread_root {
                        messages.iter()
                            .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
                            .collect()
                    } else {
                        messages.iter()
                            .filter(|m| {
                                // Include thread root (id == thread_id) + direct replies
                                Some(m.id.as_str()) == thread_id
                                    || m.reply_to.is_none()
                                    || m.reply_to.as_deref() == thread_id
                            })
                            .collect()
                    };

                    // Group messages to match rendering
                    let grouped = group_messages(&display_messages, user_pubkey.as_deref());

                    // Handle based on what's selected
                    if let Some(item) = grouped.get(app.selected_message_index) {
                        match item {
                            DisplayItem::AgentGroup { messages: group_messages, collapsed_count, .. } => {
                                // For groups with collapsed messages, toggle expansion
                                if *collapsed_count > 0 {
                                    if let Some(first_msg) = group_messages.first() {
                                        app.toggle_group_expansion(&first_msg.id);
                                    }
                                }
                            }
                            DisplayItem::SingleMessage { message: msg, .. } => {
                                // For single messages, navigate into subthread if it has replies
                                let has_replies = messages.iter().any(|m| {
                                    m.reply_to.as_deref() == Some(msg.id.as_str()) &&
                                    // Only count as reply if parent is NOT the thread root
                                    Some(msg.id.as_str()) != thread_id
                                });
                                if has_replies {
                                    app.enter_subthread((*msg).clone());
                                }
                            }
                            DisplayItem::DelegationPreview { thread_id, .. } => {
                                // Navigate to the delegated conversation
                                let thread_and_project = {
                                    let store = app.data_store.borrow();
                                    store.get_thread_by_id(thread_id).map(|t| {
                                        let project_a_tag = store.find_project_for_thread(thread_id)
                                            .unwrap_or_default();
                                        (t.clone(), project_a_tag)
                                    })
                                };
                                if let Some((thread, project_a_tag)) = thread_and_project {
                                    app.open_thread_from_home(&thread, &project_a_tag);
                                }
                            }
                        }
                    }
                }
                View::AgentBrowser => {
                    if !app.agent_browser_in_detail {
                        let agents = app.filtered_agent_definitions();
                        if let Some(agent) = agents.get(app.agent_browser_index) {
                            app.viewing_agent_id = Some(agent.id.clone());
                            app.agent_browser_in_detail = true;
                            app.scroll_offset = 0;
                        }
                    }
                }
                _ => {}
            },
            KeyCode::Esc => match app.view {
                View::Chat => {
                    if app.in_subthread() {
                        // Exit subthread view and return to main thread view
                        app.exit_subthread();
                    } else {
                        // Exit chat and go back to home
                        app.save_chat_draft();
                        app.chat_editor.clear();
                        app.view = View::Home;
                    }
                }
                View::LessonViewer => {
                    // Return to home view
                    app.view = View::Home;
                    app.viewing_lesson_id = None;
                    app.lesson_viewer_section = 0;
                    app.scroll_offset = 0;
                }
                View::AgentBrowser => {
                    if app.agent_browser_in_detail {
                        // Exit detail view and return to list
                        app.agent_browser_in_detail = false;
                        app.viewing_agent_id = None;
                        app.scroll_offset = 0;
                    } else {
                        // Exit browser and go back to home
                        app.view = View::Home;
                        app.agent_browser_filter.clear();
                        app.agent_browser_index = 0;
                    }
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

                                    if let Some(ref core_handle) = app.core_handle {
                                        if let Err(e) = core_handle.send(NostrCommand::Connect {
                                            keys: keys.clone(),
                                            user_pubkey: user_pubkey.clone(),
                                        }) {
                                            app.set_status(&format!("Failed to connect: {}", e));
                                            *login_step = LoginStep::Nsec;
                                        } else if let Err(e) = core_handle.send(NostrCommand::Sync) {
                                            app.set_status(&format!("Failed to sync: {}", e));
                                        } else {
                                            app.view = View::Home;
                                            app.load_filter_preferences();
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

                                    if let Some(ref core_handle) = app.core_handle {
                                        if let Err(e) = core_handle.send(NostrCommand::Connect {
                                            keys: keys.clone(),
                                            user_pubkey: user_pubkey.clone(),
                                        }) {
                                            app.set_status(&format!("Failed to connect: {}", e));
                                            *login_step = LoginStep::Unlock;
                                        } else if let Err(e) = core_handle.send(NostrCommand::Sync) {
                                            app.set_status(&format!("Failed to sync: {}", e));
                                        } else {
                                            app.view = View::Home;
                                            app.load_filter_preferences();
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

    // Handle projects modal when showing (using ModalState)
    if matches!(app.modal_state, ModalState::ProjectsModal { .. }) {
        // Get projects and for_new_thread flag BEFORE mutably borrowing modal_state
        let (online_projects, offline_projects) = app.filtered_projects();
        let all_projects: Vec<_> = online_projects.into_iter().chain(offline_projects).collect();
        let item_count = all_projects.len();
        let for_new_thread = matches!(app.modal_state, ModalState::ProjectsModal { for_new_thread: true, .. });

        if let ModalState::ProjectsModal { ref mut selector, .. } = app.modal_state {
            match handle_selector_key(selector, key, item_count, |idx| all_projects.get(idx).cloned()) {
                SelectorAction::Selected(project) => {
                    let a_tag = project.a_tag();
                    app.selected_project = Some(project);

                    // Auto-select PM agent and default branch from status
                    if let Some(status) = app.data_store.borrow().get_project_status(&a_tag) {
                        // Always select PM agent for new threads
                        if for_new_thread || app.selected_agent.is_none() {
                            if let Some(pm) = status.pm_agent() {
                                app.selected_agent = Some(pm.clone());
                            }
                        }
                        if app.selected_branch.is_none() {
                            app.selected_branch = status.default_branch().map(String::from);
                        }
                    }

                    app.modal_state = ModalState::None;

                    if for_new_thread {
                        // Navigate to chat view to create new thread
                        app.selected_thread = None;
                        app.creating_thread = true;
                        app.view = View::Chat;
                        app.input_mode = InputMode::Editing;
                        app.chat_editor.clear();
                    } else {
                        // Set filter to show only this project (existing behavior)
                        app.visible_projects.clear();
                        app.visible_projects.insert(a_tag);
                    }
                }
                SelectorAction::Cancelled => {
                    app.modal_state = ModalState::None;
                }
                SelectorAction::Continue => {}
            }
        }
        return Ok(());
    }

    // Handle project settings modal when showing
    if matches!(app.modal_state, ModalState::ProjectSettings(_)) {
        handle_project_settings_key(app, key);
        return Ok(());
    }

    // Normal Home view navigation
    match code {
        KeyCode::Char('q') => app.quit(),
        KeyCode::Char('/') => {
            // Open search modal
            app.showing_search_modal = true;
            app.search_filter.clear();
            app.search_index = 0;
        }
        KeyCode::Char('p') => {
            app.open_projects_modal(false);
        }
        KeyCode::Char('n') => {
            // Open projects modal - selecting a project navigates to chat to create new thread
            app.open_projects_modal(true);
        }
        KeyCode::Char('m') => {
            // Toggle "only by me" filter
            app.toggle_only_by_me();
        }
        KeyCode::Char('f') => {
            // Cycle through time filter options
            app.cycle_time_filter();
        }
        KeyCode::Char('A') => {
            // Open agent browser
            app.open_agent_browser();
        }
        KeyCode::Tab => {
            // Switch between tabs (forward)
            app.home_panel_focus = match app.home_panel_focus {
                HomeTab::Recent => HomeTab::Inbox,
                HomeTab::Inbox => HomeTab::Recent,
            };
        }
        KeyCode::BackTab if has_shift => {
            // Shift+Tab switches tabs (backward)
            app.home_panel_focus = match app.home_panel_focus {
                HomeTab::Recent => HomeTab::Inbox,
                HomeTab::Inbox => HomeTab::Recent,
            };
        }
        KeyCode::Right => {
            // Move focus to sidebar (on the right)
            app.sidebar_focused = true;
        }
        KeyCode::Left => {
            // Move focus to content area (on the left)
            app.sidebar_focused = false;
        }
        KeyCode::Up => {
            if app.sidebar_focused {
                // Navigate sidebar projects
                if app.sidebar_project_index > 0 {
                    app.sidebar_project_index -= 1;
                }
            } else {
                // Navigate content
                match app.home_panel_focus {
                    HomeTab::Inbox => {
                        if app.selected_inbox_index > 0 {
                            app.selected_inbox_index -= 1;
                        }
                    }
                    HomeTab::Recent => {
                        if app.selected_recent_index > 0 {
                            app.selected_recent_index -= 1;
                        }
                    }
                }
            }
        }
        KeyCode::Down => {
            if app.sidebar_focused {
                // Navigate sidebar projects
                let (online, offline) = app.filtered_projects();
                let max = (online.len() + offline.len()).saturating_sub(1);
                if app.sidebar_project_index < max {
                    app.sidebar_project_index += 1;
                }
            } else {
                // Navigate content
                match app.home_panel_focus {
                    HomeTab::Inbox => {
                        let max = app.inbox_items().len().saturating_sub(1);
                        if app.selected_inbox_index < max {
                            app.selected_inbox_index += 1;
                        }
                    }
                    HomeTab::Recent => {
                        // Use hierarchy for navigation (respects collapsed state)
                        let hierarchy = get_hierarchical_threads(app);
                        let max = hierarchy.len().saturating_sub(1);
                        if app.selected_recent_index < max {
                            app.selected_recent_index += 1;
                        }
                    }
                }
            }
        }
        KeyCode::Char(' ') if app.sidebar_focused => {
            // Toggle project visibility in sidebar
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                let a_tag = project.a_tag();
                if app.visible_projects.contains(&a_tag) {
                    app.visible_projects.remove(&a_tag);
                } else {
                    app.visible_projects.insert(a_tag);
                }
                app.save_selected_projects();
            }
        }
        KeyCode::Char('s') if app.sidebar_focused => {
            // Open project settings for focused project
            let (online, offline) = app.filtered_projects();
            let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
            if let Some(project) = all_projects.get(app.sidebar_project_index) {
                let a_tag = project.a_tag();
                let project_name = project.name.clone();
                let agent_ids = project.agent_ids.clone();

                app.modal_state = ui::modal::ModalState::ProjectSettings(
                    ui::modal::ProjectSettingsState::new(a_tag, project_name, agent_ids)
                );
            }
        }
        KeyCode::Enter => {
            if app.sidebar_focused {
                // Toggle project visibility (same as space)
                let (online, offline) = app.filtered_projects();
                let all_projects: Vec<_> = online.iter().chain(offline.iter()).collect();
                if let Some(project) = all_projects.get(app.sidebar_project_index) {
                    let a_tag = project.a_tag();
                    if app.visible_projects.contains(&a_tag) {
                        app.visible_projects.remove(&a_tag);
                    } else {
                        app.visible_projects.insert(a_tag);
                    }
                    app.save_selected_projects();
                }
            } else {
                // Open selected item
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
                        // Use hierarchy for selection (respects collapsed state)
                        let hierarchy = get_hierarchical_threads(app);
                        if let Some(item) = hierarchy.get(app.selected_recent_index) {
                            let thread = item.thread.clone();
                            let a_tag = item.a_tag.clone();
                            app.open_thread_from_home(&thread, &a_tag);
                        }
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
        KeyCode::Char(' ') if app.home_panel_focus == HomeTab::Recent => {
            // Toggle collapse for threads with children
            let hierarchy = get_hierarchical_threads(app);
            if let Some(item) = hierarchy.get(app.selected_recent_index) {
                if item.has_children {
                    app.toggle_thread_collapse(&item.thread.id);
                }
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

/// Handle key events for the project settings modal
fn handle_project_settings_key(app: &mut App, key: KeyEvent) {
    use ui::views::{available_agent_count, get_agent_id_at_index};

    let code = key.code;

    // Extract state to avoid borrow issues
    let mut state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::ProjectSettings(s) => s,
        other => {
            app.modal_state = other;
            return;
        }
    };

    if state.in_add_mode {
        // Add agent mode
        match code {
            KeyCode::Esc => {
                state.in_add_mode = false;
                state.add_filter.clear();
                state.add_index = 0;
            }
            KeyCode::Up => {
                if state.add_index > 0 {
                    state.add_index -= 1;
                }
            }
            KeyCode::Down => {
                let count = available_agent_count(app, &state);
                if state.add_index + 1 < count {
                    state.add_index += 1;
                }
            }
            KeyCode::Enter => {
                if let Some(agent_id) = get_agent_id_at_index(app, &state, state.add_index) {
                    state.add_agent(agent_id);
                    state.in_add_mode = false;
                    state.add_filter.clear();
                    state.add_index = 0;
                }
            }
            KeyCode::Char(c) => {
                state.add_filter.push(c);
                state.add_index = 0;
            }
            KeyCode::Backspace => {
                state.add_filter.pop();
                state.add_index = 0;
            }
            _ => {}
        }
    } else {
        // Main settings mode
        match code {
            KeyCode::Esc => {
                // Close modal without saving
                app.modal_state = ModalState::None;
                return;
            }
            KeyCode::Up => {
                if state.selector_index > 0 {
                    state.selector_index -= 1;
                }
            }
            KeyCode::Down => {
                let count = state.pending_agent_ids.len();
                if state.selector_index + 1 < count {
                    state.selector_index += 1;
                }
            }
            KeyCode::Char('a') => {
                state.in_add_mode = true;
                state.add_filter.clear();
                state.add_index = 0;
            }
            KeyCode::Char('d') => {
                if !state.pending_agent_ids.is_empty() {
                    state.remove_agent(state.selector_index);
                    // Adjust index if needed
                    if state.selector_index >= state.pending_agent_ids.len() && state.selector_index > 0 {
                        state.selector_index -= 1;
                    }
                }
            }
            KeyCode::Char('p') => {
                if !state.pending_agent_ids.is_empty() && state.selector_index > 0 {
                    state.set_pm(state.selector_index);
                    state.selector_index = 0; // Move selection to new PM position
                }
            }
            KeyCode::Enter => {
                if state.has_changes() {
                    // Publish the changes
                    let project_a_tag = state.project_a_tag.clone();
                    let agent_ids = state.pending_agent_ids.clone();

                    if let Some(ref core_handle) = app.core_handle {
                        if let Err(e) = core_handle.send(NostrCommand::UpdateProjectAgents {
                            project_a_tag,
                            agent_ids,
                        }) {
                            app.set_status(&format!("Failed to update agents: {}", e));
                        } else {
                            app.set_status("Project agents updated");
                        }
                    }

                    app.modal_state = ModalState::None;
                    return;
                }
            }
            _ => {}
        }
    }

    // Restore the state
    app.modal_state = ModalState::ProjectSettings(state);
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
        // Enter = send message or create new thread
        KeyCode::Enter => {
            let content = app.chat_editor.submit();
            if !content.is_empty() {
                if let (Some(ref core_handle), Some(ref project)) =
                    (&app.core_handle, &app.selected_project)
                {
                    let project_a_tag = project.a_tag();
                    let agent_pubkey = app.selected_agent.as_ref().map(|a| a.pubkey.clone());
                    let branch = app.selected_branch.clone();

                    if let Some(ref thread) = app.selected_thread {
                        // Reply to existing thread
                        let thread_id = thread.id.clone();
                        // NIP-22: lowercase "e" tag references the parent message
                        // When in subthread, reply to the subthread root
                        // When in main view, reply to the thread root (or first message)
                        let reply_to = if let Some(ref root_id) = app.subthread_root {
                            Some(root_id.clone())
                        } else {
                            Some(thread_id.clone())
                        };

                        if let Err(e) = core_handle.send(NostrCommand::PublishMessage {
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
                    } else {
                        // Create new thread (kind:1)
                        let title = content.lines().next().unwrap_or("New Thread").to_string();
                        if let Err(e) = core_handle.send(NostrCommand::PublishThread {
                            project_a_tag: project_a_tag.clone(),
                            title,
                            content,
                            agent_pubkey,
                            branch,
                        }) {
                            app.set_status(&format!("Failed to create thread: {}", e));
                        } else {
                            // Navigate to it once it arrives via subscription
                            app.pending_new_thread_project = Some(project_a_tag.clone());
                        }
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
            app.open_agent_selector();
        }
        // % = open branch selector
        KeyCode::Char('%') => {
            app.open_branch_selector();
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

/// Handle key events for the ask modal
fn handle_ask_modal_key(app: &mut App, key: KeyEvent) {
    use crate::ui::ask_input::InputMode as AskInputMode;

    let code = key.code;
    let modifiers = key.modifiers;

    // Extract modal_state to avoid borrow issues
    let modal_state = match app.ask_modal_state_mut() {
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
    // Extract the ask modal state
    let modal_state = match std::mem::replace(&mut app.modal_state, ModalState::None) {
        ModalState::AskModal(state) => state,
        other => {
            // Restore the state if it wasn't an ask modal
            app.modal_state = other;
            return;
        }
    };

    let response_text = modal_state.input_state.format_response();
    let message_id = modal_state.message_id;

    // Send reply to the ask event
    if let (Some(ref core_handle), Some(ref thread), Some(ref project)) =
        (&app.core_handle, &app.selected_thread, &app.selected_project)
    {
        let _ = core_handle.send(NostrCommand::PublishMessage {
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
            app.attachment_modal_editor_mut().insert_newline();
        }
        // Ctrl+A = move to beginning of line
        KeyCode::Char('a') if has_ctrl => {
            app.attachment_modal_editor_mut().move_to_line_start();
        }
        // Ctrl+E = move to end of line
        KeyCode::Char('e') if has_ctrl => {
            app.attachment_modal_editor_mut().move_to_line_end();
        }
        // Ctrl+K = kill to end of line
        KeyCode::Char('k') if has_ctrl => {
            app.attachment_modal_editor_mut().kill_to_line_end();
        }
        // Alt+Left = word left
        KeyCode::Left if has_alt => {
            app.attachment_modal_editor_mut().move_word_left();
        }
        // Alt+Right = word right
        KeyCode::Right if has_alt => {
            app.attachment_modal_editor_mut().move_word_right();
        }
        // Basic navigation
        KeyCode::Left => {
            app.attachment_modal_editor_mut().move_left();
        }
        KeyCode::Right => {
            app.attachment_modal_editor_mut().move_right();
        }
        KeyCode::Backspace => {
            app.attachment_modal_editor_mut().delete_char_before();
        }
        KeyCode::Delete => {
            app.attachment_modal_editor_mut().delete_char_at();
        }
        // Regular character input
        KeyCode::Char(c) => {
            app.attachment_modal_editor_mut().insert_char(c);
        }
        _ => {}
    }
}

/// Handle key events for the tab modal (Alt+/)
fn handle_tab_modal_key(app: &mut App, key: KeyEvent) {
    let code = key.code;

    match code {
        // Escape closes the modal
        KeyCode::Esc => {
            app.close_tab_modal();
        }
        // Up arrow moves selection up
        KeyCode::Up => {
            if app.tab_modal_index > 0 {
                app.tab_modal_index -= 1;
            }
        }
        // Down arrow moves selection down
        KeyCode::Down => {
            if app.tab_modal_index + 1 < app.open_tabs.len() {
                app.tab_modal_index += 1;
            }
        }
        // Enter switches to selected tab
        KeyCode::Enter => {
            let idx = app.tab_modal_index;
            app.close_tab_modal();
            if idx < app.open_tabs.len() {
                app.switch_to_tab(idx);
                app.view = View::Chat;
            }
        }
        // 'x' closes the selected tab
        KeyCode::Char('x') => {
            if !app.open_tabs.is_empty() {
                let idx = app.tab_modal_index;
                app.close_tab_at(idx);
                // If no more tabs, close the modal
                if app.open_tabs.is_empty() {
                    app.close_tab_modal();
                }
            }
        }
        // '0' goes to dashboard (home)
        KeyCode::Char('0') => {
            app.close_tab_modal();
            app.save_chat_draft();
            app.view = View::Home;
        }
        // Number keys 1-9 switch directly to that tab
        KeyCode::Char(c) if c >= '1' && c <= '9' => {
            let tab_index = (c as usize) - ('1' as usize);
            app.close_tab_modal();
            if tab_index < app.open_tabs.len() {
                app.switch_to_tab(tab_index);
                app.view = View::Chat;
            }
        }
        _ => {}
    }
}

/// Handle key events for the search modal (/)
fn handle_search_modal_key(app: &mut App, key: KeyEvent) {
    let code = key.code;

    match code {
        // Escape closes the modal
        KeyCode::Esc => {
            app.showing_search_modal = false;
            app.search_filter.clear();
            app.search_index = 0;
        }
        // Up arrow moves selection up
        KeyCode::Up => {
            if app.search_index > 0 {
                app.search_index -= 1;
            }
        }
        // Down arrow moves selection down
        KeyCode::Down => {
            let count = app.search_results().len();
            if app.search_index + 1 < count {
                app.search_index += 1;
            }
        }
        // Enter opens the selected thread
        KeyCode::Enter => {
            let results = app.search_results();
            if let Some(result) = results.get(app.search_index).cloned() {
                app.showing_search_modal = false;
                app.search_filter.clear();
                app.search_index = 0;
                app.open_thread_from_home(&result.thread, &result.project_a_tag);
            }
        }
        // Character input appends to filter
        KeyCode::Char(c) => {
            app.search_filter.push(c);
            app.search_index = 0;
        }
        // Backspace removes last character from filter
        KeyCode::Backspace => {
            app.search_filter.pop();
            app.search_index = 0;
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

/// Handle key events for the message actions modal
fn handle_message_actions_modal_key(app: &mut App, key: KeyEvent) {
    use ui::modal::MessageAction;

    let code = key.code;

    // Get current state
    let (message_id, selected_index, has_trace) = match &app.modal_state {
        ModalState::MessageActions {
            message_id,
            selected_index,
            has_trace,
        } => (message_id.clone(), *selected_index, *has_trace),
        _ => return,
    };

    // Count available actions
    let action_count = if has_trace { 4 } else { 3 };

    match code {
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if selected_index > 0 {
                if let ModalState::MessageActions {
                    selected_index: ref mut idx,
                    ..
                } = app.modal_state
                {
                    *idx -= 1;
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if selected_index + 1 < action_count {
                if let ModalState::MessageActions {
                    selected_index: ref mut idx,
                    ..
                } = app.modal_state
                {
                    *idx += 1;
                }
            }
        }
        KeyCode::Enter => {
            // Execute selected action
            let actions: Vec<MessageAction> = MessageAction::ALL
                .iter()
                .filter(|a| has_trace || !matches!(a, MessageAction::OpenTrace))
                .copied()
                .collect();

            if let Some(action) = actions.get(selected_index) {
                app.execute_message_action(&message_id, *action);
            }
        }
        // Direct hotkeys
        KeyCode::Char('c') => {
            app.execute_message_action(&message_id, MessageAction::CopyRawEvent);
        }
        KeyCode::Char('s') => {
            app.execute_message_action(&message_id, MessageAction::SendAgain);
        }
        KeyCode::Char('v') => {
            app.execute_message_action(&message_id, MessageAction::ViewRawEvent);
        }
        KeyCode::Char('t') if has_trace => {
            app.execute_message_action(&message_id, MessageAction::OpenTrace);
        }
        _ => {}
    }
}

/// Handle key events for the view raw event modal
fn handle_view_raw_event_modal_key(app: &mut App, key: KeyEvent) {
    let code = key.code;

    match code {
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
        }
        KeyCode::Up | KeyCode::Char('k') => {
            if let ModalState::ViewRawEvent {
                scroll_offset: ref mut offset,
                ..
            } = app.modal_state
            {
                *offset = offset.saturating_sub(1);
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let ModalState::ViewRawEvent {
                scroll_offset: ref mut offset,
                ..
            } = app.modal_state
            {
                *offset += 1;
            }
        }
        KeyCode::PageUp => {
            if let ModalState::ViewRawEvent {
                scroll_offset: ref mut offset,
                ..
            } = app.modal_state
            {
                *offset = offset.saturating_sub(20);
            }
        }
        KeyCode::PageDown => {
            if let ModalState::ViewRawEvent {
                scroll_offset: ref mut offset,
                ..
            } = app.modal_state
            {
                *offset += 20;
            }
        }
        _ => {}
    }
}

/// Handle prefix key commands (Ctrl+T + key)
fn handle_prefix_key(app: &mut App, key: KeyEvent) {
    match key.code {
        // m = toggle LLM metadata display
        KeyCode::Char('m') => {
            app.toggle_llm_metadata();
            let status = if app.show_llm_metadata {
                "LLM metadata: ON"
            } else {
                "LLM metadata: OFF"
            };
            app.set_status(status);
        }
        // ? = show hotkey help
        KeyCode::Char('?') => {
            app.modal_state = ModalState::HotkeyHelp;
        }
        // t = toggle todo sidebar (same as plain 't' in normal mode, but available everywhere)
        KeyCode::Char('t') => {
            app.todo_sidebar_visible = !app.todo_sidebar_visible;
        }
        // Unknown prefix command - ignore
        _ => {}
    }
}

/// Handle key events for the hotkey help modal
fn handle_hotkey_help_modal_key(app: &mut App, key: KeyEvent) {
    // Any key closes the modal
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::Char('q') => {
            app.modal_state = ModalState::None;
        }
        _ => {
            app.modal_state = ModalState::None;
        }
    }
}
