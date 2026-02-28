use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute, queue};
use futures::StreamExt;
use nostr_sdk::prelude::*;
use std::io::{self, Stdout, Write};
use std::sync::mpsc::Receiver;
use tenex_core::config::CoreConfig;
use tenex_core::nostr::{get_current_pubkey, DataChange, NostrCommand};
use tenex_core::runtime::CoreRuntime;

// ANSI color codes
pub(crate) const CYAN: &str = "\x1b[36m";
pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const BRIGHT_GREEN: &str = "\x1b[1;32m";
pub(crate) const YELLOW: &str = "\x1b[33m";
pub(crate) const RED: &str = "\x1b[31m";
pub(crate) const WHITE_BOLD: &str = "\x1b[1;37m";
pub(crate) const DIM: &str = "\x1b[2m";
pub(crate) const RESET: &str = "\x1b[0m";
pub(crate) const BG_INPUT: &str = "\x1b[48;5;234m";
pub(crate) const BG_HIGHLIGHT: &str = "\x1b[48;5;239m";

macro_rules! raw_println {
    () => {{
        write!(std::io::stdout(), "\r\n").ok();
        std::io::stdout().flush().ok();
    }};
    ($($arg:tt)*) => {{
        let s = format!($($arg)*);
        let mut out = std::io::stdout();
        for line in s.split('\n') {
            write!(out, "{}\r\n", line).ok();
        }
        out.flush().ok();
    }};
}

mod editor;
mod markdown;
mod panels;
mod util;
mod state;
mod completion;
mod format;
mod render;
mod commands;

use clap::Parser;
use editor::LineEditor;
use completion::{CompletionMenu, ItemAction};
use panels::{ConfigPanel, PanelMode, StatusBarNav, StatsPanel, StatusBarAction};
use state::ReplState;
use format::{print_separator_raw, print_agent_message_raw, print_error_raw, print_system_raw, print_help_raw};
use render::{redraw_input, clear_input_area, print_above_input, update_delegation_bar};
use commands::{
    CommandResult, handle_project_command, handle_agent_command, handle_open_command,
    handle_active_command, handle_new_command, handle_send_message, handle_boot_command,
    handle_status_command, handle_config_command, handle_model_command, handle_core_event,
    navigate_to_delegation, pop_conversation_stack, auto_select_project, auto_select_agent,
    subscribe_to_project, open_conversation, UploadResult, try_upload_image_file,
    handle_clipboard_paste,
};
use markdown::colorize_markdown;

#[derive(Parser, Debug)]
#[command(name = "tenex-repl")]
#[command(about = "TENEX Shell-style REPL Chat Client")]
struct Args {
    /// nsec key for authentication (prefer TENEX_NSEC env var)
    #[arg(long)]
    nsec: Option<String>,
}

fn resolve_nsec(args: &Args) -> Result<String> {
    if let Some(ref nsec) = args.nsec {
        return Ok(nsec.clone());
    }
    if let Ok(nsec) = std::env::var("TENEX_NSEC") {
        if !nsec.is_empty() {
            return Ok(nsec);
        }
    }
    anyhow::bail!("No nsec provided. Use --nsec or set TENEX_NSEC env var.")
}

async fn run_repl(
    runtime: &mut CoreRuntime,
    data_rx: &Receiver<DataChange>,
    state: &mut ReplState,
    keys: &Keys,
) -> Result<()> {
    let mut stdout = io::stdout();
    let mut editor = LineEditor::new();
    let mut completion = CompletionMenu::new();
    let mut panel = ConfigPanel::new();
    let mut status_nav = StatusBarNav::new();
    let mut stats_panel = StatsPanel::new();
    let mut events = EventStream::new();
    let (upload_tx, mut upload_rx) = tokio::sync::mpsc::channel::<UploadResult>(8);

    // Initial sync delay
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Process any initial events
    if let Some(note_keys) = runtime.poll_note_keys() {
        let _ = runtime.process_note_keys(&note_keys);
    }

    // Auto-select the first online project (if any)
    auto_select_project(state, runtime);

    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);

    let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(util::TICK_INTERVAL_MS));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            maybe_event = events.next() => {
                let Some(Ok(event)) = maybe_event else {
                    break;
                };

                match event {
                    Event::Paste(text) => {
                        if try_upload_image_file(&text, keys, upload_tx.clone()) {
                            let msg = print_system_raw("Uploading image...");
                            print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                        } else {
                            editor.handle_paste(&text);
                            if editor.has_attachments() {
                                let msg = format!("{DIM}(pasted as text attachment){RESET}");
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                            } else {
                                redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                            }
                        }
                        continue;
                    }
                    Event::Key(KeyEvent { code, modifiers, kind, .. }) => {

                // Only handle key press events (not release/repeat)
                if kind != crossterm::event::KeyEventKind::Press {
                    continue;
                }

                // ── Backend approval prompt handling ──
                {
                    let store = runtime.data_store();
                    let first_pending = store.borrow().trust.pending_backend_approvals.first()
                        .map(|a| a.backend_pubkey.clone());
                    if let Some(backend_pk) = first_pending {
                        match code {
                            KeyCode::Char('a') | KeyCode::Char('y') | KeyCode::Enter => {
                                let mut store_ref = store.borrow_mut();
                                let name = store_ref.get_profile_name(&backend_pk);
                                store_ref.add_approved_backend(&backend_pk);
                                store_ref.save_cache();
                                drop(store_ref);
                                let msg = print_system_raw(&format!("Approved backend: {}", name));
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                continue;
                            }
                            KeyCode::Char('b') | KeyCode::Char('n') => {
                                let mut store_ref = store.borrow_mut();
                                let name = store_ref.get_profile_name(&backend_pk);
                                store_ref.add_blocked_backend(&backend_pk);
                                store_ref.save_cache();
                                drop(store_ref);
                                let msg = print_system_raw(&format!("Blocked backend: {}", name));
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                continue;
                            }
                            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                                raw_println!();
                                raw_println!("{}", print_system_raw("Goodbye."));
                                return Ok(());
                            }
                            _ => {
                                continue;
                            }
                        }
                    }
                }

                // ── Config panel keyboard intercept ──
                if panel.active {
                    match code {
                        KeyCode::Up => panel.move_up(),
                        KeyCode::Down => panel.move_down(),
                        KeyCode::Char(' ') => {
                            match panel.mode {
                                PanelMode::Tools => panel.toggle_current(),
                                PanelMode::Model => {
                                    panel.select_current();
                                    let msg = panel.save(runtime);
                                    panel.deactivate();
                                    print_above_input(&mut stdout, &print_system_raw(&msg), state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                    continue;
                                }
                            }
                        }
                        KeyCode::Enter => {
                            let msg = panel.save(runtime);
                            panel.deactivate();
                            print_above_input(&mut stdout, &print_system_raw(&msg), state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                            continue;
                        }
                        KeyCode::Esc => {
                            panel.deactivate();
                        }
                        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                            raw_println!();
                            raw_println!("{}", print_system_raw("Goodbye."));
                            return Ok(());
                        }
                        _ => {}
                    }
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    continue;
                }

                // ── Stats panel keyboard intercept ──
                if stats_panel.active {
                    match (code, modifiers) {
                        (KeyCode::Tab, m) if !m.contains(KeyModifiers::SHIFT) => {
                            let next = stats_panel.tab.next();
                            stats_panel.switch_tab(next, runtime);
                        }
                        (KeyCode::Right, _) => {
                            let next = stats_panel.tab.next();
                            stats_panel.switch_tab(next, runtime);
                        }
                        (KeyCode::BackTab, _) | (KeyCode::Left, _) => {
                            let prev = stats_panel.tab.prev();
                            stats_panel.switch_tab(prev, runtime);
                        }
                        (KeyCode::Up, _) => stats_panel.scroll_up(),
                        (KeyCode::Down, _) => stats_panel.scroll_down(),
                        (KeyCode::Esc, _) => stats_panel.deactivate(),
                        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                            raw_println!();
                            raw_println!("{}", print_system_raw("Goodbye."));
                            return Ok(());
                        }
                        _ => {}
                    }
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    continue;
                }

                // ── Status bar navigation keyboard intercept ──
                if status_nav.active {
                    match code {
                        KeyCode::Left => status_nav.move_left(),
                        KeyCode::Right => status_nav.move_right(),
                        KeyCode::Enter => {
                            let action = state.status_bar_enter_action(status_nav.segment, runtime);
                            status_nav.deactivate();
                            match action {
                                StatusBarAction::ShowCompletion(buf) => {
                                    editor.set_buffer(&buf);
                                    completion.update_from_buffer(&editor.buffer, state, runtime);
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                    continue;
                                }
                                StatusBarAction::OpenConversation { thread_id, project_a_tag } => {
                                    let needs_project_switch = project_a_tag
                                        .as_ref()
                                        .map(|a| state.current_project.as_ref() != Some(a))
                                        .unwrap_or(false);

                                    if needs_project_switch {
                                        if let Some(a_tag) = &project_a_tag {
                                            subscribe_to_project(runtime, a_tag);
                                            state.switch_project(a_tag.clone(), runtime);
                                            auto_select_agent(state, runtime);
                                        }
                                    }

                                    let title = {
                                        let store = runtime.data_store();
                                        let store_ref = store.borrow();
                                        store_ref.get_thread_by_id(&thread_id)
                                            .map(|t| t.title.clone())
                                            .unwrap_or_default()
                                    };
                                    let mut output = vec![print_system_raw(&format!("Opened: {title}"))];
                                    output.extend(open_conversation(state, runtime, &thread_id, true, util::MESSAGES_TO_LOAD));

                                    if needs_project_switch {
                                        execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0)).ok();
                                        completion.input_area_drawn = false;
                                        let (_, rows) = terminal::size().unwrap_or((80, 24));
                                        let content_rows = output.len() as u16;
                                        let start_row = rows.saturating_sub(5 + content_rows);
                                        execute!(stdout, cursor::MoveTo(0, start_row)).ok();
                                    } else {
                                        clear_input_area(&mut stdout, &mut completion);
                                        completion.input_area_drawn = false;
                                    }
                                    for l in &output {
                                        raw_println!("{}", l);
                                    }
                                    update_delegation_bar(state, runtime);
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                    continue;
                                }
                                StatusBarAction::OpenStats => {
                                    stats_panel.activate(runtime);
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                    continue;
                                }
                            }
                        }
                        KeyCode::Esc => {
                            status_nav.deactivate();
                        }
                        _ => {
                            status_nav.deactivate();
                        }
                    }
                    if status_nav.active {
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                        continue;
                    }
                    if code == KeyCode::Esc {
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                        continue;
                    }
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                }

                match (code, modifiers) {
                    // Ctrl+C → quit
                    (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                        raw_println!();
                        raw_println!("{}", print_system_raw("Goodbye."));
                        return Ok(());
                    }

                    // Enter
                    (KeyCode::Enter, _) => {
                        if state.delegation_bar.focused {
                            let target = state.delegation_bar.selected_entry()
                                .map(|e| e.thread_id.clone());
                            if let Some(thread_id) = target {
                                let result = navigate_to_delegation(state, runtime, &thread_id);
                                if let CommandResult::ClearScreen(lines) = result {
                                    execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0)).ok();
                                    completion.input_area_drawn = false;
                                    let (_, rows) = terminal::size().unwrap_or((80, 24));
                                    let content_rows = lines.len() as u16;
                                    let start = rows.saturating_sub(5 + content_rows);
                                    execute!(stdout, cursor::MoveTo(0, start)).ok();
                                    for l in &lines {
                                        raw_println!("{}", l);
                                    }
                                    update_delegation_bar(state, runtime);
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                }
                            }
                            continue;
                        }
                        if completion.visible {
                            if let Some(action) = completion.accept() {
                                match action {
                                    ItemAction::ReplaceFull(text) => {
                                        editor.set_buffer(&text);
                                        completion.update_from_buffer(&editor.buffer, state, runtime);
                                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                        continue;
                                    }
                                    ItemAction::Submit(value) => {
                                        if editor.buffer.starts_with('@') {
                                            editor.set_buffer(&format!("/agent {value}"));
                                        } else {
                                            let cmd_part = match editor.buffer.find(' ') {
                                                Some(pos) => editor.buffer[..pos].to_string(),
                                                None => editor.buffer.clone(),
                                            };
                                            editor.set_buffer(&format!("{cmd_part} {value}"));
                                        }
                                    }
                                }
                            }
                        }
                        {
                            clear_input_area(&mut stdout, &mut completion);
                            completion.input_area_drawn = false;
                            let line = editor.submit();
                            raw_println!();

                            if line.trim().is_empty() {
                                redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                continue;
                            }

                            let result = if line.starts_with('/') {
                                let (cmd, arg) = match line.find(' ') {
                                    Some(pos) => (&line[..pos], Some(line[pos + 1..].trim())),
                                    None => (line.as_str(), None),
                                };
                                match cmd {
                                    "/project" | "/p" => CommandResult::Lines(handle_project_command(arg, state, runtime)),
                                    "/agent" | "/a" => CommandResult::Lines(handle_agent_command(arg, state, runtime)),
                                    "/new" | "/n" => handle_new_command(arg.unwrap_or(""), state, runtime),
                                    "/conversations" | "/c" | "/open" | "/o" => handle_open_command(arg, state, runtime),
                                    "/config" => handle_config_command(arg, state, runtime, &mut panel),
                                    "/model" | "/m" => handle_model_command(arg, state, runtime, &mut panel),
                                    "/active" => handle_active_command(arg, state, runtime),
                                    "/stats" => {
                                        stats_panel.activate(runtime);
                                        CommandResult::Lines(vec![])
                                    }
                                    "/boot" | "/b" => CommandResult::Lines(handle_boot_command(arg, runtime)),
                                    "/status" | "/s" => CommandResult::Lines(handle_status_command(state, runtime)),
                                    "/help" | "/h" => CommandResult::Lines(vec![print_help_raw()]),
                                    "/quit" | "/q" => {
                                        raw_println!("{}", print_system_raw("Goodbye."));
                                        return Ok(());
                                    }
                                    _ => CommandResult::Lines(vec![print_error_raw(&format!("Unknown command: {cmd}. Type /help for commands."))]),
                                }
                            } else {
                                CommandResult::Lines(handle_send_message(&line, state, runtime))
                            };

                            match result {
                                CommandResult::Lines(output_lines) => {
                                    for l in &output_lines {
                                        raw_println!("{}", l);
                                    }
                                    if !state.streaming_in_progress {
                                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                    }
                                }
                                CommandResult::ShowCompletion(buf) => {
                                    editor.set_buffer(&buf);
                                    completion.update_from_buffer(&editor.buffer, state, runtime);
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                }
                                CommandResult::ClearScreen(lines) => {
                                    execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0)).ok();
                                    completion.input_area_drawn = false;
                                    let (_, rows) = terminal::size().unwrap_or((80, 24));
                                    let content_rows = lines.len() as u16;
                                    let start = rows.saturating_sub(5 + content_rows);
                                    execute!(stdout, cursor::MoveTo(0, start)).ok();
                                    for l in &lines {
                                        raw_println!("{}", l);
                                    }
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                }
                            }
                        }
                    }

                    // Tab → apply selected completion to buffer, or trigger menu
                    (KeyCode::Tab, m) if !m.contains(KeyModifiers::SHIFT) => {
                        if completion.visible && !completion.items.is_empty() {
                            let item = &completion.items[completion.selected];
                            match &item.action {
                                ItemAction::ReplaceFull(text) => {
                                    editor.set_buffer(text);
                                }
                                ItemAction::Submit(value) => {
                                    let cmd_part = match editor.buffer.find(' ') {
                                        Some(pos) => editor.buffer[..pos].to_string(),
                                        None => editor.buffer.clone(),
                                    };
                                    editor.set_buffer(&format!("{cmd_part} {value}"));
                                }
                            }
                            completion.select_next();
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                        } else if editor.buffer.starts_with('/') || editor.buffer.starts_with('@') {
                            completion.update_from_buffer(&editor.buffer, state, runtime);
                            if !completion.items.is_empty() {
                                let item = &completion.items[completion.selected];
                                match &item.action {
                                    ItemAction::ReplaceFull(text) => {
                                        editor.set_buffer(text);
                                    }
                                    ItemAction::Submit(value) => {
                                        let cmd_part = match editor.buffer.find(' ') {
                                            Some(pos) => editor.buffer[..pos].to_string(),
                                            None => editor.buffer.clone(),
                                        };
                                        editor.set_buffer(&format!("{cmd_part} {value}"));
                                    }
                                }
                                completion.select_next();
                            }
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                        }
                    }

                    // Shift+Tab → prev completion
                    (KeyCode::BackTab, _) => {
                        if completion.visible && !completion.items.is_empty() {
                            completion.select_prev();
                            let item = &completion.items[completion.selected];
                            match &item.action {
                                ItemAction::ReplaceFull(text) => editor.set_buffer(text),
                                ItemAction::Submit(value) => {
                                    let cmd = match editor.buffer.find(' ') {
                                        Some(p) => editor.buffer[..p].to_string(),
                                        None => editor.buffer.clone(),
                                    };
                                    editor.set_buffer(&format!("{cmd} {value}"));
                                }
                            }
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                        }
                    }

                    // Escape
                    (KeyCode::Esc, _) => {
                        if state.delegation_bar.focused {
                            state.delegation_bar.unfocus();
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                        } else if editor.selected_attachment.is_some() {
                            editor.selected_attachment = None;
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                        } else if completion.visible {
                            completion.hide();
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                        } else if !state.conversation_stack.is_empty() {
                            if let Some(CommandResult::ClearScreen(lines)) = pop_conversation_stack(state, runtime) {
                                execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0)).ok();
                                completion.input_area_drawn = false;
                                let (_, rows) = terminal::size().unwrap_or((80, 24));
                                let content_rows = lines.len() as u16;
                                let start = rows.saturating_sub(5 + content_rows);
                                execute!(stdout, cursor::MoveTo(0, start)).ok();
                                for l in &lines {
                                    raw_println!("{}", l);
                                }
                                update_delegation_bar(state, runtime);
                                redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                            }
                        }
                    }

                    // Up arrow
                    (KeyCode::Up, _) => {
                        if state.delegation_bar.focused {
                            state.delegation_bar.unfocus();
                        } else if editor.selected_attachment.is_some() {
                            editor.selected_attachment = None;
                        } else if completion.visible {
                            completion.select_prev();
                        } else if editor.buffer.is_empty() && state.delegation_bar.has_content() && !state.delegation_bar.focused {
                            state.delegation_bar.focus();
                        } else {
                            editor.history_up();
                        }
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Down arrow
                    (KeyCode::Down, _) => {
                        if state.delegation_bar.focused {
                            state.delegation_bar.unfocus();
                        } else if !completion.visible && editor.selected_attachment.is_none()
                            && editor.cursor == editor.buffer.len() && editor.has_attachments()
                        {
                            editor.selected_attachment = Some(0);
                        } else if completion.visible {
                            completion.select_next();
                        } else if editor.buffer.is_empty() && !completion.visible
                            && editor.selected_attachment.is_none() && !panel.active
                        {
                            let segments = state.status_bar_segments(runtime);
                            status_nav.segment_count = segments.len();
                            status_nav.activate();
                        } else {
                            editor.history_down();
                        }
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Backspace
                    (KeyCode::Backspace, m) if !m.contains(KeyModifiers::ALT) => {
                        if let Some(idx) = editor.selected_attachment {
                            editor.remove_attachment(idx);
                        } else if let Some(att_idx) = editor.marker_before_cursor() {
                            editor.selected_attachment = Some(att_idx);
                        } else {
                            editor.delete_back();
                        }
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Delete
                    (KeyCode::Delete, _) => {
                        if let Some(idx) = editor.selected_attachment {
                            editor.remove_attachment(idx);
                        } else {
                            editor.delete_forward();
                        }
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Left
                    (KeyCode::Left, m) if !m.contains(KeyModifiers::ALT) => {
                        if state.delegation_bar.focused {
                            state.delegation_bar.select_prev();
                        } else if let Some(idx) = editor.selected_attachment {
                            editor.selected_attachment = Some(idx.saturating_sub(1));
                        } else {
                            editor.move_left();
                        }
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Right
                    (KeyCode::Right, m) if !m.contains(KeyModifiers::ALT) => {
                        if state.delegation_bar.focused {
                            state.delegation_bar.select_next();
                        } else if let Some(idx) = editor.selected_attachment {
                            let max = editor.attachments.len().saturating_sub(1);
                            editor.selected_attachment = Some((idx + 1).min(max));
                        } else {
                            editor.move_right();
                        }
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Home / Ctrl+A
                    (KeyCode::Home, _) => {
                        editor.move_home();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }
                    (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_home();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // End / Ctrl+E
                    (KeyCode::End, _) => {
                        editor.move_end();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }
                    (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_end();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Ctrl+W → delete word back
                    (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.delete_word_back();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Ctrl+U → clear line
                    (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.set_buffer("");
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Ctrl+K → kill to end of line
                    (KeyCode::Char('k'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.kill_to_end();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Ctrl+D → delete forward (or quit on empty)
                    (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) => {
                        if editor.buffer.is_empty() {
                            raw_println!();
                            raw_println!("{}", print_system_raw("Goodbye."));
                            return Ok(());
                        }
                        editor.delete_forward();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Ctrl+B → move left
                    (KeyCode::Char('b'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_left();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Ctrl+F → move right
                    (KeyCode::Char('f'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_right();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Ctrl+L → clear screen
                    (KeyCode::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => {
                        execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0)).ok();
                        completion.input_area_drawn = false;
                        let (_, rows) = terminal::size().unwrap_or((80, 24));
                        execute!(stdout, cursor::MoveTo(0, rows.saturating_sub(5))).ok();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Alt+B / Alt+Left → word left
                    (KeyCode::Char('b'), m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_left();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }
                    (KeyCode::Left, m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_left();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Alt+F / Alt+Right → word right
                    (KeyCode::Char('f'), m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_right();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }
                    (KeyCode::Right, m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_right();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Alt+D → delete word forward
                    (KeyCode::Char('d'), m) if m.contains(KeyModifiers::ALT) => {
                        editor.delete_word_forward();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Alt+Backspace → delete word back
                    (KeyCode::Backspace, m) if m.contains(KeyModifiers::ALT) => {
                        editor.delete_word_back();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Regular character
                    (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) => {
                        state.delegation_bar.unfocus();
                        editor.selected_attachment = None;
                        editor.insert_char(c);
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    // Ctrl+V → clipboard paste (image or text)
                    (KeyCode::Char('v'), m) if m.contains(KeyModifiers::CONTROL) => {
                        handle_clipboard_paste(&mut editor, keys, upload_tx.clone(), &mut stdout, state, runtime, &mut completion, &panel, &status_nav, &stats_panel);
                    }

                    _ => {}
                }
                } // end Event::Key
                    _ => {}
                } // end match event
            }

            // Upload result channel
            Some(result) = upload_rx.recv() => {
                match result {
                    UploadResult::Success(url) => {
                        let id = editor.add_image_attachment(url);
                        let msg = print_system_raw(&format!("Image uploaded → [Image #{}]", id));
                        print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }
                    UploadResult::Error(err) => {
                        let msg = print_error_raw(&format!("Image upload failed: {}", err));
                        print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                    }
                }
            }

            note_keys = runtime.next_note_keys() => {
                if let Some(note_keys) = note_keys {
                    match runtime.process_note_keys(&note_keys) {
                        Ok(events) => {
                            for event in &events {
                                if let Some(text) = handle_core_event(event, state, runtime) {
                                    if state.streaming_in_progress {
                                        raw_println!("{}", text);
                                    } else {
                                        print_above_input(&mut stdout, &text, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let msg = print_error_raw(&format!("Event processing error: {e}"));
                            print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                        }
                    }
                }
            }

            _ = tick.tick() => {
                drain_data_changes(data_rx, state, runtime, &mut stdout, &editor, &mut completion, &mut panel, &status_nav, &stats_panel);
                state.wave_frame = state.wave_frame.wrapping_add(1);
                update_delegation_bar(state, runtime);
                let agents_active = state.has_active_agents(runtime);
                if (state.is_animating() || agents_active || state.delegation_bar.has_content()) && !state.streaming_in_progress {
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel);
                }
            }
        }
    }

    Ok(())
}

fn drain_data_changes(
    data_rx: &Receiver<DataChange>,
    state: &mut ReplState,
    runtime: &CoreRuntime,
    stdout: &mut Stdout,
    editor: &LineEditor,
    completion: &mut CompletionMenu,
    panel: &mut ConfigPanel,
    status_nav: &StatusBarNav,
    stats_panel: &StatsPanel,
) {
    loop {
        match data_rx.try_recv() {
            Ok(DataChange::LocalStreamChunk {
                conversation_id,
                text_delta,
                is_finish,
                ..
            }) => {
                if state.current_conversation.as_deref() != Some(&conversation_id) {
                    continue;
                }

                if let Some(delta) = &text_delta {
                    if !state.streaming_in_progress {
                        state.streaming_in_progress = true;
                        state.stream_buffer.clear();

                        if panel.active {
                            panel.deactivate();
                        }

                        clear_input_area(stdout, completion);
                        completion.input_area_drawn = false;

                        let is_consecutive = state.last_displayed_pubkey.as_deref() == state.current_agent.as_deref()
                            && state.current_agent.is_some();

                        if is_consecutive {
                            let agent_name = state.agent_display();
                            let indent = " ".repeat(agent_name.len() + 3);
                            write!(stdout, "{indent}").ok();
                        } else {
                            let agent_name = state.agent_display();
                            write!(stdout, "{BRIGHT_GREEN}{agent_name} ›{RESET} ").ok();
                        }
                    }
                    write!(stdout, "{delta}").ok();
                    stdout.flush().ok();
                    state.stream_buffer.push_str(delta);
                }

                if is_finish && state.streaming_in_progress {
                    let raw_lines = state.stream_buffer.lines().count().max(1);
                    queue!(stdout, cursor::MoveUp(raw_lines as u16 - 1)).ok();
                    write!(stdout, "\r").ok();
                    for _ in 0..raw_lines {
                        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
                        write!(stdout, "\r\n").ok();
                    }
                    queue!(stdout, cursor::MoveUp(raw_lines as u16)).ok();
                    write!(stdout, "\r").ok();

                    let agent_name = state.agent_display();
                    let is_consecutive = state.last_displayed_pubkey.as_deref() == state.current_agent.as_deref()
                        && state.current_agent.is_some();

                    if is_consecutive {
                        let indent = " ".repeat(agent_name.len() + 3);
                        let colored = colorize_markdown(&state.stream_buffer);
                        for line in colored.lines() {
                            write!(stdout, "{indent}{line}\r\n").ok();
                        }
                    } else {
                        let colored = print_agent_message_raw(&agent_name, &state.stream_buffer);
                        for line in colored.lines() {
                            write!(stdout, "{line}\r\n").ok();
                        }
                    }
                    stdout.flush().ok();

                    if let Some(ref pk) = state.current_agent {
                        state.last_displayed_pubkey = Some(pk.clone());
                    }

                    raw_println!("{}", print_separator_raw());
                    state.streaming_in_progress = false;
                    state.stream_buffer.clear();

                    redraw_input(stdout, state, runtime, editor, completion, panel, status_nav, stats_panel);
                }
            }
            Ok(DataChange::ProjectStatus { json }) => {
                let kind = serde_json::from_str::<serde_json::Value>(&json)
                    .ok()
                    .and_then(|v| v.get("kind")?.as_u64());

                let store = runtime.data_store();
                let mut store_ref = store.borrow_mut();
                store_ref.handle_status_event_json(&json);

                let has_pending = store_ref.trust.has_pending_approvals();
                let mut needs_redraw = kind == Some(24133) || has_pending;

                drop(store_ref);

                if state.current_project.is_none()
                    && auto_select_project(state, runtime) {
                    needs_redraw = true;
                }

                if needs_redraw && !state.streaming_in_progress {
                    redraw_input(stdout, state, runtime, editor, completion, panel, status_nav, stats_panel);
                }
            }
            Ok(_) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let nsec = resolve_nsec(&args)?;
    let secret_key = SecretKey::parse(&nsec)?;
    let keys = Keys::new(secret_key);
    let user_pubkey = get_current_pubkey(&keys);

    let config = CoreConfig::default();
    let mut runtime = CoreRuntime::new(config)?;
    let handle = runtime.handle();

    let data_rx = runtime
        .take_data_rx()
        .ok_or_else(|| anyhow::anyhow!("Core runtime already has active data receiver"))?;

    runtime
        .data_store()
        .borrow_mut()
        .apply_authenticated_user(user_pubkey.clone());

    let keys_for_repl = keys.clone();

    // Connect to relays
    println!("{DIM}Connecting...{RESET}");
    let (response_tx, response_rx) = std::sync::mpsc::channel();
    handle.send(NostrCommand::Connect {
        keys,
        user_pubkey: user_pubkey.clone(),
        relay_urls: vec![],
        response_tx: Some(response_tx),
    })?;

    match response_rx.recv_timeout(std::time::Duration::from_secs(15)) {
        Ok(Ok(())) => {
            println!("{GREEN}Connected.{RESET}");
        }
        Ok(Err(e)) => {
            anyhow::bail!("Connection failed: {e}");
        }
        Err(_) => {
            anyhow::bail!("Connection timed out after 15s");
        }
    }

    println!();
    println!("{WHITE_BOLD}tenex-repl{RESET} {DIM}— type /help for commands{RESET}");
    println!();

    let mut state = ReplState::new(user_pubkey);

    // Install panic hook to restore terminal
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            cursor::Show,
            crossterm::event::DisableBracketedPaste
        );
        default_panic(info);
    }));

    // Enable raw mode + bracketed paste
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), crossterm::event::EnableBracketedPaste)?;

    let result = run_repl(&mut runtime, &data_rx, &mut state, &keys_for_repl).await;

    // Restore terminal
    terminal::disable_raw_mode()?;
    execute!(
        io::stdout(),
        cursor::Show,
        crossterm::event::DisableBracketedPaste
    )?;

    runtime.shutdown();
    result
}
