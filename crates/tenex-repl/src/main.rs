use anyhow::Result;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute};
use futures::StreamExt;
use nostr_sdk::prelude::*;
use std::io::{self, Stdout, Write};
use std::sync::mpsc::Receiver;
use tenex_core::config::CoreConfig;
use tenex_core::models::InputMode as AskInputMode;
use tenex_core::nostr::{get_current_pubkey, DataChange, NostrCommand};
use tenex_core::runtime::CoreRuntime;

// ANSI color codes
pub(crate) const CYAN: &str = "\x1b[36m";
pub(crate) const GREEN: &str = "\x1b[32m";
pub(crate) const BRIGHT_GREEN: &str = "\x1b[1;32m";
pub(crate) const ACCENT: &str = "\x1b[38;2;255;193;7m";
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
mod history;
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
use completion::CompletionMenu;
use panels::{ConfigPanel, PanelMode, StatusBarNav, StatsPanel, StatusBarAction, NudgeSkillPanel, NudgeSkillMode};
use state::ReplState;
use format::{print_separator_raw, print_error_raw, print_system_raw, print_help_raw};
use render::{redraw_input, clear_input_area, print_above_input, update_delegation_bar, apply_clear_screen};
use commands::{
    CommandResult, handle_project_command, handle_agent_command, handle_open_command,
    handle_active_command, handle_new_command, handle_send_message, handle_boot_command,
    handle_status_command, handle_config_command, handle_model_command, handle_core_event,
    navigate_to_delegation, pop_conversation_stack, auto_select_project,
    handle_status_bar_open, UploadResult, try_upload_image_file,
    handle_clipboard_paste, handle_bunker_command, maybe_open_ask_modal,
};

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

fn history_entry_label(entry: &history::HistoryEntry) -> String {
    let first_line = entry.content.lines().next().unwrap_or("");
    let truncated: String = first_line.chars().take(50).collect();
    if first_line.chars().count() > 50 {
        format!("{truncated}...")
    } else {
        truncated
    }
}

fn history_entry_description(entry: &history::HistoryEntry, runtime: &CoreRuntime) -> String {
    let project_name = entry.project_atag.as_ref().map(|atag| {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        store_ref.get_projects().iter()
            .find(|p| p.a_tag() == *atag)
            .map(|p| p.title.clone())
            .unwrap_or_else(|| "?".to_string())
    }).unwrap_or_else(|| "global".to_string());
    let age = history::relative_time(entry.updated_at);
    let source_icon = match entry.source.as_str() {
        "draft" => "\u{270e}",
        "sent" => "\u{2192}",
        "kind1" => "\u{26a1}",
        _ => "",
    };
    format!("{source_icon} {project_name} \u{00b7} {age}")
}

fn history_search_items(entries: &[history::HistoryEntry], runtime: &CoreRuntime) -> Vec<completion::CompletionItem> {
    entries.iter().map(|e| {
        completion::CompletionItem {
            label: history_entry_label(e),
            description: history_entry_description(e, runtime),
            fill: e.content.clone(),
        }
    }).collect()
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
    let mut nudge_skill_panel = NudgeSkillPanel::new();
    let mut events = EventStream::new();
    let (upload_tx, mut upload_rx) = tokio::sync::mpsc::channel::<UploadResult>(8);
    let history_store = history::HistoryStore::open(&CoreConfig::default_data_dir())
        .map_err(|e| anyhow::anyhow!("Failed to open history database: {e}"))?;

    // Initial sync delay
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Process any initial events
    if let Some(note_keys) = runtime.poll_note_keys() {
        let _ = runtime.process_note_keys(&note_keys);
    }

    // Auto-select the first online project (if any)
    auto_select_project(state, runtime);

    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);

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
                            print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                        } else {
                            editor.handle_paste(&text);
                            if editor.has_attachments() {
                                let msg = format!("{DIM}(pasted as text attachment){RESET}");
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                            } else {
                                redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                continue;
                            }
                            KeyCode::Char('b') | KeyCode::Char('n') => {
                                let mut store_ref = store.borrow_mut();
                                let name = store_ref.get_profile_name(&backend_pk);
                                store_ref.add_blocked_backend(&backend_pk);
                                store_ref.save_cache();
                                drop(store_ref);
                                let msg = print_system_raw(&format!("Blocked backend: {}", name));
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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

                // ── Bunker approval prompt handling ──
                if !state.pending_bunker_requests.is_empty() {
                    match code {
                        KeyCode::Char('a') | KeyCode::Char('y') | KeyCode::Enter => {
                            if let Some(req) = state.pending_bunker_requests.pop_front() {
                                let _ = runtime.handle().send(NostrCommand::BunkerResponse {
                                    request_id: req.request_id,
                                    approved: true,
                                });
                                let msg = print_system_raw("Bunker request approved");
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                            }
                            continue;
                        }
                        KeyCode::Char('A') => {
                            if let Some(req) = state.pending_bunker_requests.pop_front() {
                                let _ = runtime.handle().send(NostrCommand::BunkerResponse {
                                    request_id: req.request_id.clone(),
                                    approved: true,
                                });
                                let _ = runtime.handle().send(NostrCommand::AddBunkerAutoApproveRule {
                                    requester_pubkey: req.requester_pubkey,
                                    event_kind: req.event_kind,
                                });
                                let msg = print_system_raw("Bunker request approved + auto-approve rule added");
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                            }
                            continue;
                        }
                        KeyCode::Char('r') | KeyCode::Char('n') => {
                            if let Some(req) = state.pending_bunker_requests.pop_front() {
                                let _ = runtime.handle().send(NostrCommand::BunkerResponse {
                                    request_id: req.request_id,
                                    approved: false,
                                });
                                let msg = print_system_raw("Bunker request rejected");
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                            }
                            continue;
                        }
                        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                            raw_println!();
                            raw_println!("{}", print_system_raw("Goodbye."));
                            return Ok(());
                        }
                        _ => {
                            continue; // swallow all other input while bunker prompt active
                        }
                    }
                }

                // ── Ask modal keyboard intercept ──
                if state.ask_modal.is_some() {
                    match code {
                        KeyCode::Up => {
                            if let Some(ref mut modal) = state.ask_modal {
                                modal.input_state.prev_option();
                            }
                        }
                        KeyCode::Down => {
                            if let Some(ref mut modal) = state.ask_modal {
                                modal.input_state.next_option();
                            }
                        }
                        KeyCode::Left => {
                            if let Some(ref mut modal) = state.ask_modal {
                                if modal.input_state.mode == AskInputMode::CustomInput {
                                    modal.input_state.move_cursor_left();
                                } else {
                                    modal.input_state.prev_question();
                                }
                            }
                        }
                        KeyCode::Right => {
                            if let Some(ref mut modal) = state.ask_modal {
                                if modal.input_state.mode == AskInputMode::CustomInput {
                                    modal.input_state.move_cursor_right();
                                } else {
                                    modal.input_state.skip_question();
                                }
                            }
                        }
                        KeyCode::Char(' ') => {
                            if let Some(ref mut modal) = state.ask_modal {
                                if modal.input_state.mode == AskInputMode::Selection {
                                    modal.input_state.toggle_multi_select();
                                } else {
                                    modal.input_state.insert_char(' ');
                                }
                            }
                        }
                        KeyCode::Enter => {
                            let should_submit = if let Some(ref mut modal) = state.ask_modal {
                                if modal.input_state.mode == AskInputMode::CustomInput {
                                    modal.input_state.submit_custom_answer();
                                } else {
                                    modal.input_state.select_current_option();
                                }
                                modal.input_state.is_complete()
                            } else {
                                false
                            };

                            if should_submit {
                                if let Some(modal) = state.ask_modal.take() {
                                    let response_text = modal.input_state.format_response();
                                    if let (Some(ref a_tag), Some(ref thread_id)) = (&state.current_project, &state.current_conversation) {
                                        let _ = runtime.handle().send(NostrCommand::PublishMessage {
                                            thread_id: thread_id.clone(),
                                            project_a_tag: a_tag.clone(),
                                            content: response_text,
                                            agent_pubkey: state.current_agent.clone(),
                                            reply_to: Some(modal.message_id),
                                            nudge_ids: Vec::new(),
                                            skill_ids: Vec::new(),
                                            ask_author_pubkey: Some(modal.ask_author_pubkey),
                                            response_tx: None,
                                        });
                                        let msg = print_system_raw("Response submitted");
                                        print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                    }
                                }
                            }
                        }
                        KeyCode::Esc => {
                            if let Some(ref mut modal) = state.ask_modal {
                                if modal.input_state.mode == AskInputMode::CustomInput {
                                    modal.input_state.cancel_custom_mode();
                                } else {
                                    state.ask_modal = None;
                                }
                            }
                        }
                        KeyCode::Backspace => {
                            if let Some(ref mut modal) = state.ask_modal {
                                if modal.input_state.mode == AskInputMode::CustomInput {
                                    modal.input_state.delete_char();
                                }
                            }
                        }
                        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                            raw_println!();
                            raw_println!("{}", print_system_raw("Goodbye."));
                            return Ok(());
                        }
                        KeyCode::Char(c) => {
                            if let Some(ref mut modal) = state.ask_modal {
                                if modal.input_state.mode == AskInputMode::CustomInput {
                                    modal.input_state.insert_char(c);
                                }
                            }
                        }
                        _ => {}
                    }
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    continue;
                }

                // ── Config panel keyboard intercept (4-mode) ──
                if panel.active {
                    match panel.mode {
                        PanelMode::Tools => match code {
                            KeyCode::Up => panel.move_up(),
                            KeyCode::Down => panel.move_down(),
                            KeyCode::Char(' ') => panel.toggle_current(),
                            KeyCode::Enter => {
                                let msg = panel.save(runtime);
                                panel.deactivate();
                                print_above_input(&mut stdout, &print_system_raw(&msg), state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                continue;
                            }
                            KeyCode::Char('@') => panel.switch_to_agent_select(runtime),
                            KeyCode::Char('-') => panel.switch_to_flag_select(),
                            KeyCode::Esc => panel.deactivate(),
                            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                                raw_println!();
                                raw_println!("{}", print_system_raw("Goodbye."));
                                return Ok(());
                            }
                            _ => {}
                        },
                        PanelMode::AgentSelect => match code {
                            KeyCode::Up => panel.move_up(),
                            KeyCode::Down => panel.move_down(),
                            KeyCode::Enter => {
                                if panel.resolve_selected_agent(runtime) {
                                    panel.rebuild_origin_command();
                                    panel.switch_to_tools();
                                }
                            }
                            KeyCode::Backspace => {
                                if panel.filter.is_empty() {
                                    panel.switch_to_tools();
                                } else {
                                    panel.filter.pop();
                                    panel.cursor = 0;
                                    panel.scroll_offset = 0;
                                }
                            }
                            KeyCode::Esc => panel.switch_to_tools(),
                            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                                raw_println!();
                                raw_println!("{}", print_system_raw("Goodbye."));
                                return Ok(());
                            }
                            KeyCode::Char(c) => {
                                panel.filter.push(c);
                                panel.cursor = 0;
                                panel.scroll_offset = 0;
                            }
                            _ => {}
                        },
                        PanelMode::FlagSelect => match code {
                            KeyCode::Up => panel.move_up(),
                            KeyCode::Down => panel.move_down(),
                            KeyCode::Enter | KeyCode::Char(' ') => {
                                match panel.cursor {
                                    0 => {
                                        // --model
                                        panel.switch_to_model_select(runtime);
                                    }
                                    1 => {
                                        // --set-pm
                                        panel.is_set_pm = !panel.is_set_pm;
                                        panel.rebuild_origin_command();
                                        panel.switch_to_tools();
                                    }
                                    2 => {
                                        // --global
                                        panel.is_global = !panel.is_global;
                                        panel.rebuild_origin_command();
                                        panel.switch_to_tools();
                                    }
                                    _ => {}
                                }
                            }
                            KeyCode::Esc | KeyCode::Backspace => panel.switch_to_tools(),
                            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                                raw_println!();
                                raw_println!("{}", print_system_raw("Goodbye."));
                                return Ok(());
                            }
                            _ => {}
                        },
                        PanelMode::ModelSelect => match code {
                            KeyCode::Up => panel.move_up(),
                            KeyCode::Down => panel.move_down(),
                            KeyCode::Enter | KeyCode::Char(' ') => {
                                if panel.select_current_model() {
                                    if panel.quick_save {
                                        let msg = panel.save_model_only(runtime);
                                        panel.deactivate();
                                        print_above_input(&mut stdout, &print_system_raw(&msg), state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                        continue;
                                    } else {
                                        panel.rebuild_origin_command();
                                        panel.switch_to_tools();
                                    }
                                }
                            }
                            KeyCode::Backspace => {
                                if panel.filter.is_empty() {
                                    if panel.quick_save {
                                        panel.deactivate();
                                    } else {
                                        panel.switch_to_tools();
                                    }
                                } else {
                                    panel.filter.pop();
                                    panel.cursor = 0;
                                    panel.scroll_offset = 0;
                                }
                            }
                            KeyCode::Esc => {
                                if panel.quick_save {
                                    panel.deactivate();
                                } else {
                                    panel.switch_to_tools();
                                }
                            }
                            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                                raw_println!();
                                raw_println!("{}", print_system_raw("Goodbye."));
                                return Ok(());
                            }
                            KeyCode::Char(c) => {
                                panel.filter.push(c);
                                panel.cursor = 0;
                                panel.scroll_offset = 0;
                            }
                            _ => {}
                        },
                    }
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    continue;
                }

                // ── Nudge/Skill panel keyboard intercept ──
                if nudge_skill_panel.active {
                    match code {
                        KeyCode::Up => nudge_skill_panel.move_up(),
                        KeyCode::Down => nudge_skill_panel.move_down(),
                        KeyCode::Char(' ') => nudge_skill_panel.toggle_current(),
                        KeyCode::Tab => {
                            let next = match nudge_skill_panel.mode {
                                NudgeSkillMode::Nudges => NudgeSkillMode::Skills,
                                NudgeSkillMode::Skills => NudgeSkillMode::Nudges,
                            };
                            nudge_skill_panel.switch_mode(next, runtime);
                        }
                        KeyCode::Enter => {
                            let (nudge_ids, skill_ids) = nudge_skill_panel.commit_selections(runtime);
                            state.selected_nudge_ids = nudge_ids;
                            state.selected_skill_ids = skill_ids;
                            nudge_skill_panel.deactivate();
                        }
                        KeyCode::Esc => {
                            nudge_skill_panel.deactivate();
                            editor.insert_char('$');
                        }
                        KeyCode::Backspace => {
                            if nudge_skill_panel.filter.is_empty() {
                                nudge_skill_panel.deactivate();
                            } else {
                                nudge_skill_panel.filter.pop();
                                nudge_skill_panel.cursor = 0;
                                nudge_skill_panel.scroll_offset = 0;
                            }
                        }
                        KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                            raw_println!();
                            raw_println!("{}", print_system_raw("Goodbye."));
                            return Ok(());
                        }
                        KeyCode::Char(c) => {
                            nudge_skill_panel.filter.push(c);
                            nudge_skill_panel.cursor = 0;
                            nudge_skill_panel.scroll_offset = 0;
                        }
                        _ => {}
                    }
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                    continue;
                                }
                                StatusBarAction::OpenConversation { thread_id, project_a_tag } => {
                                    if let CommandResult::ClearScreen(lines) = handle_status_bar_open(state, runtime, &thread_id, project_a_tag) {
                                        apply_clear_screen(&mut stdout, &lines, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                        maybe_open_ask_modal(state, runtime);
                                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                    }
                                    continue;
                                }
                                StatusBarAction::OpenStats => {
                                    stats_panel.activate(runtime);
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                        continue;
                    }
                    if code == KeyCode::Esc {
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                        continue;
                    }
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                }

                if state.search_mode {
                    match (code, modifiers) {
                        (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                            raw_println!();
                            raw_println!("{}", print_system_raw("Goodbye."));
                            return Ok(());
                        }
                        (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                            state.search_all_projects = !state.search_all_projects;
                            let results = if state.search_all_projects {
                                history_store.search_all(&editor.buffer, state.active_draft_id, 12)
                            } else {
                                history_store.search(&editor.buffer, state.current_project.as_deref(), state.active_draft_id, 12)
                            };
                            if let Ok(entries) = results {
                                completion.items = history_search_items(&entries, runtime);
                                completion.selected = 0;
                                completion.visible = !completion.items.is_empty();
                            }
                        }
                        (KeyCode::Enter, _) => {
                            if completion.visible && !completion.items.is_empty() {
                                let fill = completion.items[completion.selected].fill.clone();
                                editor.set_buffer(&fill);
                            } else {
                                editor.set_buffer(&state.pre_search_buffer);
                            }
                            state.search_mode = false;
                            state.search_all_projects = false;
                            completion.hide();
                        }
                        (KeyCode::Esc, _) => {
                            editor.set_buffer(&state.pre_search_buffer);
                            state.search_mode = false;
                            state.search_all_projects = false;
                            completion.hide();
                        }
                        (KeyCode::Up, _) => {
                            if completion.visible { completion.select_prev(); }
                        }
                        (KeyCode::Down, _) => {
                            if completion.visible { completion.select_next(); }
                        }
                        (KeyCode::Tab, _) => {
                            if completion.visible { completion.select_next(); }
                        }
                        (KeyCode::BackTab, _) => {
                            if completion.visible { completion.select_prev(); }
                        }
                        (KeyCode::Backspace, _) => {
                            editor.delete_back();
                            let results = if state.search_all_projects {
                                history_store.search_all(&editor.buffer, state.active_draft_id, 12)
                            } else {
                                history_store.search(&editor.buffer, state.current_project.as_deref(), state.active_draft_id, 12)
                            };
                            if let Ok(entries) = results {
                                completion.items = history_search_items(&entries, runtime);
                                completion.selected = 0;
                                completion.visible = !completion.items.is_empty();
                            }
                        }
                        (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) => {
                            editor.insert_char(c);
                            let results = if state.search_all_projects {
                                history_store.search_all(&editor.buffer, state.active_draft_id, 12)
                            } else {
                                history_store.search(&editor.buffer, state.current_project.as_deref(), state.active_draft_id, 12)
                            };
                            if let Ok(entries) = results {
                                completion.items = history_search_items(&entries, runtime);
                                completion.selected = 0;
                                completion.visible = !completion.items.is_empty();
                            }
                        }
                        _ => {}
                    }
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    continue;
                }

                match (code, modifiers) {
                    // Ctrl+C → quit
                    (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                        if !editor.buffer.is_empty() {
                            history_store.upsert_draft(state.active_draft_id, &editor.buffer, state.current_project.as_deref()).ok();
                        }
                        raw_println!();
                        raw_println!("{}", print_system_raw("Goodbye."));
                        return Ok(());
                    }

                    // Ctrl+R → reverse history search
                    (KeyCode::Char('r'), m) if m.contains(KeyModifiers::CONTROL) => {
                        state.search_mode = true;
                        state.search_all_projects = false;
                        state.pre_search_buffer = editor.buffer.clone();
                        editor.set_buffer("");
                        completion.items.clear();
                        completion.visible = false;
                        completion.selected = 0;
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Enter
                    (KeyCode::Enter, _) => {
                        if state.delegation_bar.focused {
                            let target = state.delegation_bar.selected_entry()
                                .map(|e| e.thread_id.clone());
                            if let Some(thread_id) = target {
                                let result = navigate_to_delegation(state, runtime, &thread_id);
                                if let CommandResult::ClearScreen(lines) = result {
                                    apply_clear_screen(&mut stdout, &lines, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                    maybe_open_ask_modal(state, runtime);
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                }
                            }
                            continue;
                        }
                        if completion.visible && !completion.items.is_empty() {
                            editor.set_buffer(&completion.items[completion.selected].fill);
                            completion.hide();
                        } else if completion.visible {
                            completion.hide();
                        }
                        {
                            clear_input_area(&mut stdout, &mut completion);
                            completion.input_area_drawn = false;
                            let line = editor.submit();
                            raw_println!();

                            if line.trim().is_empty() {
                                redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                continue;
                            }

                            let line = if line.starts_with('@') {
                                format!("/agent {}", line[1..].trim())
                            } else {
                                line
                            };

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
                                    "/bunker" => CommandResult::Lines(handle_bunker_command(arg, state, runtime)),
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
                                history_store.record_sent(
                                    &line,
                                    state.current_project.as_deref(),
                                    state.active_draft_id.take(),
                                ).ok();
                                state.draft_last_content.clear();
                                CommandResult::Lines(handle_send_message(&line, state, runtime))
                            };

                            match result {
                                CommandResult::Lines(output_lines) => {
                                    for l in &output_lines {
                                        raw_println!("{}", l);
                                    }
                                    if !state.streaming_in_progress {
                                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                    }
                                }
                                CommandResult::ShowCompletion(buf) => {
                                    editor.set_buffer(&buf);
                                    completion.update_from_buffer(&editor.buffer, state, runtime);
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                }
                                CommandResult::ClearScreen(lines) => {
                                    apply_clear_screen(&mut stdout, &lines, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                    maybe_open_ask_modal(state, runtime);
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                }
                            }
                        }
                    }

                    // Tab → apply selected completion to buffer, or trigger menu
                    (KeyCode::Tab, m) if !m.contains(KeyModifiers::SHIFT) => {
                        if !completion.visible {
                            if editor.buffer.starts_with('/') || editor.buffer.starts_with('@') {
                                completion.pre_completion_buffer = Some(editor.buffer.clone());
                                completion.update_from_buffer(&editor.buffer, state, runtime);
                            }
                        }
                        if completion.visible && !completion.items.is_empty() {
                            let fill = completion.items[completion.selected].fill.clone();
                            editor.set_buffer(&fill);
                            if fill.ends_with(' ') {
                                // Intermediate: refresh menu with next-level completions
                                completion.update_from_buffer(&editor.buffer, state, runtime);
                            } else {
                                // Terminal: cycle to next item
                                completion.select_next();
                            }
                        }
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Shift+Tab → prev completion
                    (KeyCode::BackTab, _) => {
                        if completion.visible && !completion.items.is_empty() {
                            completion.select_prev();
                            editor.set_buffer(&completion.items[completion.selected].fill);
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                        }
                    }

                    // Escape
                    (KeyCode::Esc, _) => {
                        if state.delegation_bar.focused {
                            state.delegation_bar.unfocus();
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                        } else if editor.selected_attachment.is_some() {
                            editor.selected_attachment = None;
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                        } else if completion.visible {
                            if let Some(ref saved) = completion.pre_completion_buffer {
                                editor.set_buffer(saved);
                            }
                            completion.hide();
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                        } else if !state.conversation_stack.is_empty() {
                            if let Some(CommandResult::ClearScreen(lines)) = pop_conversation_stack(state, runtime) {
                                apply_clear_screen(&mut stdout, &lines, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                maybe_open_ask_modal(state, runtime);
                                redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Delete
                    (KeyCode::Delete, _) => {
                        if let Some(idx) = editor.selected_attachment {
                            editor.remove_attachment(idx);
                        } else {
                            editor.delete_forward();
                        }
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Home / Ctrl+A
                    (KeyCode::Home, _) => {
                        editor.move_home();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }
                    (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_home();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // End / Ctrl+E
                    (KeyCode::End, _) => {
                        editor.move_end();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }
                    (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_end();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Ctrl+W → delete word back
                    (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.delete_word_back();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Ctrl+U → clear line
                    (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.set_buffer("");
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Ctrl+K → kill to end of line
                    (KeyCode::Char('k'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.kill_to_end();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Ctrl+B → move left
                    (KeyCode::Char('b'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_left();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Ctrl+F → move right
                    (KeyCode::Char('f'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_right();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Ctrl+L → clear screen
                    (KeyCode::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => {
                        execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0)).ok();
                        completion.input_area_drawn = false;
                        let (_, rows) = terminal::size().unwrap_or((80, 24));
                        execute!(stdout, cursor::MoveTo(0, rows.saturating_sub(5))).ok();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Alt+B / Alt+Left → word left
                    (KeyCode::Char('b'), m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_left();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }
                    (KeyCode::Left, m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_left();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Alt+F / Alt+Right → word right
                    (KeyCode::Char('f'), m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_right();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }
                    (KeyCode::Right, m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_right();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Alt+D → delete word forward
                    (KeyCode::Char('d'), m) if m.contains(KeyModifiers::ALT) => {
                        editor.delete_word_forward();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Alt+Backspace → delete word back
                    (KeyCode::Backspace, m) if m.contains(KeyModifiers::ALT) => {
                        editor.delete_word_back();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // $ → open nudge/skill selector
                    (KeyCode::Char('$'), m) if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) => {
                        state.delegation_bar.unfocus();
                        editor.selected_attachment = None;
                        nudge_skill_panel.activate(
                            runtime,
                            NudgeSkillMode::Nudges,
                            &state.selected_nudge_ids,
                            &state.selected_skill_ids,
                        );
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Regular character
                    (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) => {
                        state.delegation_bar.unfocus();
                        editor.selected_attachment = None;
                        editor.insert_char(c);
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    // Ctrl+V → clipboard paste (image or text)
                    (KeyCode::Char('v'), m) if m.contains(KeyModifiers::CONTROL) => {
                        handle_clipboard_paste(&mut editor, keys, upload_tx.clone(), &mut stdout, state, runtime, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }

                    _ => {}
                }
                } // end Event::Key
                    Event::Resize(_, _) => {
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }
                    _ => {}
                } // end match event
            }

            // Upload result channel
            Some(result) = upload_rx.recv() => {
                match result {
                    UploadResult::Success(url) => {
                        let id = editor.add_image_attachment(url);
                        let msg = print_system_raw(&format!("Image uploaded → [Image #{}]", id));
                        print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }
                    UploadResult::Error(err) => {
                        let msg = print_error_raw(&format!("Image upload failed: {}", err));
                        print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                    }
                }
            }

            note_keys = runtime.next_note_keys() => {
                if let Some(note_keys) = note_keys {
                    match runtime.process_note_keys(&note_keys) {
                        Ok(events) => {
                            for event in &events {
                                if let Some(text) = handle_core_event(event, state, runtime, &history_store) {
                                    if state.streaming_in_progress {
                                        raw_println!("{}", text);
                                    } else {
                                        print_above_input(&mut stdout, &text, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                                    }
                                }
                            }
                            // Auto-open ask modal if a new ask event arrived
                            if !events.is_empty() {
                                maybe_open_ask_modal(state, runtime);
                                redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                            }
                        }
                        Err(e) => {
                            let msg = print_error_raw(&format!("Event processing error: {e}"));
                            print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
                        }
                    }
                }
            }

            _ = tick.tick() => {
                drain_data_changes(data_rx, state, runtime, &mut stdout, &editor, &mut completion, &mut panel, &status_nav, &stats_panel, &nudge_skill_panel);
                state.wave_frame = state.wave_frame.wrapping_add(1);
                update_delegation_bar(state, runtime);

                if !editor.buffer.trim().is_empty() && editor.buffer != state.draft_last_content
                    && state.draft_last_saved.elapsed().as_secs() >= 1
                {
                    if let Ok(id) = history_store.upsert_draft(
                        state.active_draft_id,
                        &editor.buffer,
                        state.current_project.as_deref(),
                    ) {
                        state.active_draft_id = Some(id);
                        state.draft_last_content = editor.buffer.clone();
                        state.draft_last_saved = std::time::Instant::now();
                    }
                } else if editor.buffer.trim().is_empty() && state.active_draft_id.is_some() {
                    if let Some(id) = state.active_draft_id.take() {
                        history_store.delete_draft(id).ok();
                        state.draft_last_content.clear();
                    }
                }

                let agents_active = state.has_active_agents(runtime);
                if (state.is_animating() || agents_active || state.delegation_bar.has_content()) && !state.streaming_in_progress {
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion, &panel, &status_nav, &stats_panel, &nudge_skill_panel);
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
    nudge_skill_panel: &NudgeSkillPanel,
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
                        state.stream_finished_conv = None;

                        if panel.active {
                            panel.deactivate();
                        }

                        clear_input_area(stdout, completion);
                        completion.input_area_drawn = false;

                        let is_consecutive = state.last_displayed_pubkey.as_deref() == state.current_agent.as_deref()
                            && state.current_agent.is_some();

                        if is_consecutive {
                            write!(stdout, "  ").ok();
                        } else {
                            let agent_name = state.agent_display();
                            write!(stdout, "{BRIGHT_GREEN}{agent_name}{RESET}\r\n  ").ok();
                        }
                    }
                    write!(stdout, "{delta}").ok();
                    stdout.flush().ok();
                    state.stream_buffer.push_str(delta);
                }

                if is_finish && state.streaming_in_progress {
                    // Finish the streaming line and print separator.
                    // The raw streamed text stays on screen — no cursor
                    // rewrite (terminal line wrapping makes that fragile).
                    write!(stdout, "\r\n").ok();
                    stdout.flush().ok();

                    if let Some(ref pk) = state.current_agent {
                        state.last_displayed_pubkey = Some(pk.clone());
                    }

                    raw_println!("{}", print_separator_raw());
                    state.streaming_in_progress = false;
                    state.stream_buffer.clear();
                    state.stream_finished_conv = Some(conversation_id.clone());

                    redraw_input(stdout, state, runtime, editor, completion, panel, status_nav, stats_panel, nudge_skill_panel);
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
                    redraw_input(stdout, state, runtime, editor, completion, panel, status_nav, stats_panel, nudge_skill_panel);
                }
            }
            Ok(DataChange::BunkerSignRequest { request }) => {
                state.pending_bunker_requests.push_back(request);
                if !state.streaming_in_progress {
                    redraw_input(stdout, state, runtime, editor, completion, panel, status_nav, stats_panel, nudge_skill_panel);
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
