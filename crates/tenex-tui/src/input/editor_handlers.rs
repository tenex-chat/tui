//! Text editor keyboard event handlers.
//!
//! Handles input for the chat editor including vim mode support.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::nostr::NostrCommand;
use crate::ui::app::VimMode;
use crate::ui::{App, InputMode};

/// Handle key events for the chat editor (rich text editing)
pub(super) fn handle_chat_editor_key(app: &mut App, key: KeyEvent) {
    // If vim mode is enabled, dispatch based on mode
    if app.vim_enabled {
        match app.vim_mode {
            VimMode::Normal => {
                handle_vim_normal_mode(app, key);
                return;
            }
            VimMode::Insert => {
                // Esc exits insert mode
                if key.code == KeyCode::Esc {
                    app.vim_enter_normal();
                    app.save_chat_draft();
                    return;
                }
                // Otherwise fall through to normal editing
            }
        }
    }

    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);
    let has_alt = modifiers.contains(KeyModifiers::ALT);
    let has_shift = modifiers.contains(KeyModifiers::SHIFT);

    match code {
        // Shift+Enter or Alt+Enter = newline
        // Also handle Ctrl+J which is what iTerm2/macOS sends for Shift+Enter
        KeyCode::Enter if has_shift || has_alt => {
            app.chat_editor.insert_newline();
            app.save_chat_draft();
        }
        KeyCode::Char('j') | KeyCode::Char('J') if has_ctrl => {
            app.chat_editor.insert_newline();
            app.save_chat_draft();
        }
        // Enter = send message or create new thread
        KeyCode::Enter => {
            handle_send_message(app);
        }
        // Esc = exit input mode
        KeyCode::Esc => {
            app.save_chat_draft();
            app.input_mode = InputMode::Normal;
            // Set selection to last item so Up arrow works intuitively
            let count = app.display_item_count();
            app.selected_message_index = count.saturating_sub(1);
        }
        // Tab = cycle focus between input and attachments
        KeyCode::Tab if app.chat_editor.has_attachments() => {
            app.chat_editor.cycle_focus();
            if app.chat_editor.get_focused_attachment().is_some() {
                app.open_attachment_modal();
            }
        }
        // Up = cycle through message history (when input is empty)
        KeyCode::Up if app.chat_editor.text.is_empty() && !app.chat_editor.has_attachments() => {
            app.history_prev();
        }
        // Down = cycle forward through message history (when browsing)
        KeyCode::Down if app.is_browsing_history() => {
            app.history_next();
        }
        // Up = focus attachments (when there are any)
        KeyCode::Up
            if app.chat_editor.has_attachments() && app.chat_editor.focused_attachment.is_none() =>
        {
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
        // Ctrl+N = open nudge selector
        KeyCode::Char('n') if has_ctrl => {
            app.open_nudge_selector();
        }
        // Ctrl+A = move to beginning of visual line
        KeyCode::Char('a') if has_ctrl => {
            app.chat_editor
                .move_to_visual_line_start(app.chat_input_wrap_width);
        }
        // Ctrl+E = move to end of visual line
        KeyCode::Char('e') if has_ctrl => {
            app.chat_editor
                .move_to_visual_line_end(app.chat_input_wrap_width);
        }
        // Ctrl+K = kill to end of line
        KeyCode::Char('k') if has_ctrl => {
            app.chat_editor.kill_to_line_end();
            app.save_chat_draft();
        }
        // Ctrl+U = kill to beginning of line
        KeyCode::Char('u') if has_ctrl => {
            app.chat_editor.kill_to_line_start();
            app.save_chat_draft();
        }
        // Ctrl+W = delete word backward
        KeyCode::Char('w') if has_ctrl => {
            app.chat_editor.delete_word_backward();
            app.save_chat_draft();
        }
        // Ctrl+D = delete character at cursor
        KeyCode::Char('d') if has_ctrl => {
            app.chat_editor.delete_char_at();
            app.save_chat_draft();
        }
        // Ctrl+Shift+Z = redo
        KeyCode::Char('z') if has_ctrl && has_shift => {
            app.chat_editor.redo();
            app.save_chat_draft();
        }
        // Ctrl+Z = undo
        KeyCode::Char('z') if has_ctrl => {
            app.chat_editor.undo();
            app.save_chat_draft();
        }
        // Ctrl+C = copy selection
        KeyCode::Char('c') if has_ctrl => {
            if let Some(selected) = app.chat_editor.selected_text() {
                use arboard::Clipboard;
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(selected);
                }
            }
        }
        // Ctrl+X = cut selection
        KeyCode::Char('x') if has_ctrl => {
            if let Some(selected) = app.chat_editor.selected_text() {
                use arboard::Clipboard;
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(selected);
                }
                app.chat_editor.delete_selection();
                app.save_chat_draft();
            }
        }
        // Shift+Alt+Left = word left extend selection
        KeyCode::Left if has_alt && has_shift => {
            app.chat_editor.move_word_left_extend_selection();
        }
        // Shift+Alt+Right = word right extend selection
        KeyCode::Right if has_alt && has_shift => {
            app.chat_editor.move_word_right_extend_selection();
        }
        // Alt+Left = word left
        KeyCode::Left if has_alt => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_word_left();
        }
        // Alt+Right = word right
        KeyCode::Right if has_alt => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_word_right();
        }
        // Shift+Left = extend selection left
        KeyCode::Left if has_shift => {
            app.chat_editor.move_left_extend_selection();
        }
        // Shift+Right = extend selection right
        KeyCode::Right if has_shift => {
            app.chat_editor.move_right_extend_selection();
        }
        // Basic navigation (clears selection)
        KeyCode::Left => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_left();
        }
        KeyCode::Right => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_right();
        }
        // Home = move to beginning of line
        KeyCode::Home => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_to_line_start();
        }
        // End = move to end of line
        KeyCode::End => {
            app.chat_editor.clear_selection();
            app.chat_editor.move_to_line_end();
        }
        // Alt+Backspace = delete word backward
        KeyCode::Backspace if has_alt => {
            app.chat_editor.delete_word_backward();
            app.save_chat_draft();
        }
        KeyCode::Backspace => {
            if app.chat_editor.focused_attachment.is_some() {
                app.chat_editor.delete_focused_attachment();
            } else {
                app.chat_editor.delete_char_before();
            }
            app.save_chat_draft();
        }
        KeyCode::Delete => {
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
        // Up/Down = move by visual lines (for wrapped text navigation)
        KeyCode::Up => {
            app.chat_editor.move_up_visual(app.chat_input_wrap_width);
        }
        KeyCode::Down => {
            app.chat_editor.move_down_visual(app.chat_input_wrap_width);
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

/// Handle key events for vim normal mode in chat editor
fn handle_vim_normal_mode(app: &mut App, key: KeyEvent) {
    let code = key.code;

    match code {
        // Ctrl+J is Line Feed (ASCII 10), same as Shift+Enter on iTerm2/macOS
        KeyCode::Char('j') | KeyCode::Char('J')
            if key.modifiers.contains(KeyModifiers::CONTROL) =>
        {
            app.chat_editor.insert_newline();
            app.save_chat_draft();
        }

        // Mode switching
        KeyCode::Char('i') => {
            app.vim_enter_insert();
        }
        KeyCode::Char('a') => {
            app.vim_enter_append();
        }
        KeyCode::Char('A') => {
            app.chat_editor.move_to_line_end();
            app.vim_enter_insert();
        }
        KeyCode::Char('I') => {
            app.chat_editor.move_to_line_start();
            app.vim_enter_insert();
        }
        KeyCode::Char('o') => {
            app.chat_editor.move_to_line_end();
            app.chat_editor.insert_newline();
            app.vim_enter_insert();
            app.save_chat_draft();
        }
        KeyCode::Char('O') => {
            app.chat_editor.move_to_line_start();
            app.chat_editor.insert_newline();
            app.chat_editor.move_up();
            app.vim_enter_insert();
            app.save_chat_draft();
        }

        // Movement
        KeyCode::Char('h') | KeyCode::Left => {
            app.chat_editor.move_left();
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.chat_editor.move_right();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            app.chat_editor.move_down_visual(app.chat_input_wrap_width);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            app.chat_editor.move_up_visual(app.chat_input_wrap_width);
        }
        KeyCode::Char('w') => {
            app.chat_editor.move_word_right();
        }
        KeyCode::Char('b') => {
            app.chat_editor.move_word_left();
        }
        KeyCode::Char('0') => {
            app.chat_editor.move_to_line_start();
        }
        KeyCode::Char('$') => {
            app.chat_editor.move_to_line_end();
        }

        // Editing
        KeyCode::Char('x') => {
            app.chat_editor.delete_char_at();
            app.save_chat_draft();
        }
        KeyCode::Char('X') => {
            app.chat_editor.delete_char_before();
            app.save_chat_draft();
        }
        KeyCode::Char('u') => {
            app.chat_editor.undo();
            app.save_chat_draft();
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.chat_editor.redo();
            app.save_chat_draft();
        }
        KeyCode::Char('D') => {
            app.chat_editor.kill_to_line_end();
            app.save_chat_draft();
        }

        // Esc in normal mode exits editing mode
        KeyCode::Esc => {
            app.save_chat_draft();
            app.input_mode = InputMode::Normal;
            // Set selection to last item so Up arrow works intuitively
            let count = app.display_item_count();
            app.selected_message_index = count.saturating_sub(1);
        }

        // Shift+Enter or Alt+Enter = newline (even in normal mode)
        KeyCode::Enter
            if key.modifiers.contains(KeyModifiers::SHIFT)
                || key.modifiers.contains(KeyModifiers::ALT) =>
        {
            app.chat_editor.insert_newline();
            app.save_chat_draft();
        }

        _ => {}
    }
}

/// Handle sending a message or creating a new thread
fn handle_send_message(app: &mut App) {
    let content = app.chat_editor.submit();
    if !content.is_empty() {
        // Save to message history for ↑/↓ navigation
        app.add_to_message_history(content.clone());
        app.exit_history_mode();

        if let (Some(ref core_handle), Some(ref project)) =
            (&app.core_handle, &app.selected_project)
        {
            let project_a_tag = project.a_tag();
            let agent_pubkey = app.selected_agent.as_ref().map(|a| a.pubkey.clone());
            let branch = app.selected_branch.clone();
            let nudge_ids = app.selected_nudge_ids.clone();

            if let Some(ref thread) = app.selected_thread {
                // Reply to existing thread
                let thread_id = thread.id.clone();
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
                    nudge_ids,
                    ask_author_pubkey: None,
                }) {
                    app.set_status(&format!("Failed to publish message: {}", e));
                } else {
                    app.delete_chat_draft();
                    app.selected_nudge_ids.clear();
                }
            } else {
                // Create new thread (kind:1)
                let title = content.lines().next().unwrap_or("New Thread").to_string();

                let draft_id = app
                    .find_draft_tab(&project_a_tag)
                    .map(|(_, id)| id.to_string());

                if let Err(e) = core_handle.send(NostrCommand::PublishThread {
                    project_a_tag: project_a_tag.clone(),
                    title,
                    content,
                    agent_pubkey,
                    branch,
                    nudge_ids,
                }) {
                    app.set_status(&format!("Failed to create thread: {}", e));
                } else {
                    app.pending_new_thread_project = Some(project_a_tag.clone());
                    app.pending_new_thread_draft_id = draft_id;
                    app.selected_nudge_ids.clear();
                }
            }
        }
    }
}
