//! Text editor keyboard event handlers.
//!
//! Handles input for the chat editor including vim mode support.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::input::input_prefix;
use crate::nostr::NostrCommand;
use crate::ui::app::{InputContextFocus, VimMode};
use crate::ui::{App, InputMode};

/// Handle key events for the chat editor (rich text editing)
pub(super) fn handle_chat_editor_key(app: &mut App, key: KeyEvent) {
    // If context line is focused, handle navigation there first
    if app.input_context_focus.is_some() {
        handle_context_focus_key(app, key);
        return;
    }

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
            app.chat_editor_mut().insert_newline();
            app.save_chat_draft();
        }
        KeyCode::Char('j') | KeyCode::Char('J') if has_ctrl => {
            app.chat_editor_mut().insert_newline();
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
        KeyCode::Tab if app.chat_editor().has_attachments() => {
            app.chat_editor_mut().cycle_focus();
            if app.chat_editor().get_focused_attachment().is_some() {
                app.open_attachment_modal();
            }
        }
        // Up = cycle through message history (when input is empty)
        KeyCode::Up if app.chat_editor().text.is_empty() && !app.chat_editor().has_attachments() => {
            app.history_prev();
        }
        // Down = cycle forward through message history (when browsing)
        KeyCode::Down if app.is_browsing_history() => {
            app.history_next();
        }
        // Up = focus attachments (only when on first visual line and there are attachments)
        KeyCode::Up
            if app.chat_editor().has_attachments()
                && app.chat_editor().focused_attachment.is_none()
                && app.chat_editor().is_on_first_visual_line(app.chat_input_wrap_width) =>
        {
            app.chat_editor_mut().focus_attachments();
        }
        // Down = unfocus attachments (return to input)
        KeyCode::Down if app.chat_editor().focused_attachment.is_some() => {
            app.chat_editor_mut().unfocus_attachments();
        }
        // Left/Right = navigate between attachments when focused
        KeyCode::Left if app.chat_editor().focused_attachment.is_some() => {
            if let Some(idx) = app.chat_editor().focused_attachment {
                if idx > 0 {
                    app.chat_editor_mut().focused_attachment = Some(idx - 1);
                }
            }
        }
        KeyCode::Right if app.chat_editor().focused_attachment.is_some() => {
            let idx = app.chat_editor().focused_attachment;
            let total = app.chat_editor().total_attachments();
            if let Some(idx) = idx {
                if idx + 1 < total {
                    app.chat_editor_mut().focused_attachment = Some(idx + 1);
                }
            }
        }
        // Ctrl+N = open nudge selector
        KeyCode::Char('n') if has_ctrl => {
            app.open_nudge_selector();
        }
        // Ctrl+R = open history search
        KeyCode::Char('r') if has_ctrl => {
            app.open_history_search();
            // Trigger initial search to show recent messages
            app.update_history_search();
        }
        // Ctrl+A = move to beginning of visual line
        KeyCode::Char('a') if has_ctrl => {
            let wrap_width = app.chat_input_wrap_width;
            app.chat_editor_mut()
                .move_to_visual_line_start(wrap_width);
        }
        // Ctrl+E = move to end of visual line
        KeyCode::Char('e') if has_ctrl => {
            let wrap_width = app.chat_input_wrap_width;
            app.chat_editor_mut()
                .move_to_visual_line_end(wrap_width);
        }
        // Ctrl+K = kill to end of line
        KeyCode::Char('k') if has_ctrl => {
            app.chat_editor_mut().kill_to_line_end();
            app.save_chat_draft();
        }
        // Ctrl+U = clear entire input (can be restored with Ctrl+Z)
        KeyCode::Char('u') if has_ctrl => {
            app.chat_editor_mut().clear_input();
            app.save_chat_draft();
        }
        // Ctrl+W = delete word backward
        KeyCode::Char('w') if has_ctrl => {
            app.chat_editor_mut().delete_word_backward();
            app.save_chat_draft();
        }
        // Ctrl+D = delete character at cursor
        KeyCode::Char('d') if has_ctrl => {
            app.chat_editor_mut().delete_char_at();
            app.save_chat_draft();
        }
        // Ctrl+Shift+Z = redo
        KeyCode::Char('z') if has_ctrl && has_shift => {
            app.chat_editor_mut().redo();
            app.save_chat_draft();
        }
        // Ctrl+Z = undo
        KeyCode::Char('z') if has_ctrl => {
            app.chat_editor_mut().undo();
            app.save_chat_draft();
        }
        // Ctrl+C = copy selection
        KeyCode::Char('c') if has_ctrl => {
            if let Some(selected) = app.chat_editor_mut().selected_text() {
                use arboard::Clipboard;
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(selected);
                }
            }
        }
        // Ctrl+X = cut selection
        KeyCode::Char('x') if has_ctrl => {
            if let Some(selected) = app.chat_editor_mut().selected_text() {
                use arboard::Clipboard;
                if let Ok(mut clipboard) = Clipboard::new() {
                    let _ = clipboard.set_text(selected);
                }
                app.chat_editor_mut().delete_selection();
                app.save_chat_draft();
            }
        }
        // Shift+Alt+Left = word left extend selection
        KeyCode::Left if has_alt && has_shift => {
            app.chat_editor_mut().move_word_left_extend_selection();
        }
        // Shift+Alt+Right = word right extend selection
        KeyCode::Right if has_alt && has_shift => {
            app.chat_editor_mut().move_word_right_extend_selection();
        }
        // Alt+Left = word left
        KeyCode::Left if has_alt => {
            app.chat_editor_mut().clear_selection();
            app.chat_editor_mut().move_word_left();
        }
        // Alt+Right = word right
        KeyCode::Right if has_alt => {
            app.chat_editor_mut().clear_selection();
            app.chat_editor_mut().move_word_right();
        }
        // Shift+Left = extend selection left
        KeyCode::Left if has_shift => {
            app.chat_editor_mut().move_left_extend_selection();
        }
        // Shift+Right = extend selection right
        KeyCode::Right if has_shift => {
            app.chat_editor_mut().move_right_extend_selection();
        }
        // Basic navigation (clears selection)
        KeyCode::Left => {
            app.chat_editor_mut().clear_selection();
            app.chat_editor_mut().move_left();
        }
        KeyCode::Right => {
            app.chat_editor_mut().clear_selection();
            app.chat_editor_mut().move_right();
        }
        // Home = move to beginning of line
        KeyCode::Home => {
            app.chat_editor_mut().clear_selection();
            app.chat_editor_mut().move_to_line_start();
        }
        // End = move to end of line
        KeyCode::End => {
            app.chat_editor_mut().clear_selection();
            app.chat_editor_mut().move_to_line_end();
        }
        // Alt+Backspace = delete word backward
        KeyCode::Backspace if has_alt => {
            app.chat_editor_mut().delete_word_backward();
            app.save_chat_draft();
        }
        KeyCode::Backspace => {
            if app.chat_editor().focused_attachment.is_some() {
                app.chat_editor_mut().delete_focused_attachment();
            } else {
                app.chat_editor_mut().delete_char_before();
            }
            app.save_chat_draft();
        }
        KeyCode::Delete => {
            if app.chat_editor().focused_attachment.is_some() {
                app.chat_editor_mut().delete_focused_attachment();
            } else {
                app.chat_editor_mut().delete_char_at();
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
            let wrap_width = app.chat_input_wrap_width;
            app.chat_editor_mut().move_up_visual(wrap_width);
        }
        // Down on last line = focus context line (agent/model/branch), otherwise move down
        KeyCode::Down => {
            let wrap_width = app.chat_input_wrap_width;
            if app.chat_editor().is_on_last_visual_line(wrap_width) {
                // Focus the agent in the context line
                app.input_context_focus = Some(InputContextFocus::Agent);
            } else {
                app.chat_editor_mut().move_down_visual(wrap_width);
            }
        }
        KeyCode::PageUp => {
            app.scroll_up(20);
        }
        KeyCode::PageDown => {
            app.scroll_down(20);
        }
        // Regular character input - check for prefix triggers first
        KeyCode::Char(c) => {
            // Check if this is a prefix trigger (e.g., @ on empty input)
            if !input_prefix::try_handle_prefix(app, c) {
                // Not a prefix trigger, insert normally
                app.chat_editor_mut().insert_char(c);
                app.save_chat_draft();
            }
        }
        _ => {}
    }
}

/// Handle key events when context line is focused (agent/model/branch selection)
fn handle_context_focus_key(app: &mut App, key: KeyEvent) {
    use crate::ui::ModalState;

    let focus = match app.input_context_focus {
        Some(f) => f,
        None => return,
    };

    match key.code {
        // Up or Esc = return to text input
        KeyCode::Up | KeyCode::Esc => {
            app.input_context_focus = None;
        }
        // Left = move to previous item (Nudge -> Branch -> Model -> Agent)
        KeyCode::Left => {
            app.input_context_focus = Some(match focus {
                InputContextFocus::Agent => InputContextFocus::Agent, // Already at leftmost
                InputContextFocus::Model => InputContextFocus::Agent,
                InputContextFocus::Branch => InputContextFocus::Model,
                InputContextFocus::Nudge => InputContextFocus::Branch,
            });
        }
        // Right = move to next item (Agent -> Model -> Branch -> Nudge)
        KeyCode::Right => {
            app.input_context_focus = Some(match focus {
                InputContextFocus::Agent => InputContextFocus::Model,
                InputContextFocus::Model => InputContextFocus::Branch,
                InputContextFocus::Branch => InputContextFocus::Nudge,
                InputContextFocus::Nudge => InputContextFocus::Nudge, // Already at rightmost
            });
        }
        // Enter = open the appropriate selector modal
        KeyCode::Enter => {
            match focus {
                InputContextFocus::Agent => {
                    app.input_context_focus = None;
                    app.open_agent_selector();
                }
                InputContextFocus::Model => {
                    // Open agent settings for the current agent to change the model
                    if let Some(agent) = &app.selected_agent {
                        if let Some(project) = &app.selected_project {
                            let (all_tools, all_models) = app
                                .data_store
                                .borrow()
                                .get_project_status(&project.a_tag())
                                .map(|status| {
                                    let tools = status.tools().iter().map(|s| s.to_string()).collect();
                                    let models = status.models().iter().map(|s| s.to_string()).collect();
                                    (tools, models)
                                })
                                .unwrap_or_default();

                            let settings_state = crate::ui::modal::AgentSettingsState::new(
                                agent.name.clone(),
                                agent.pubkey.clone(),
                                project.a_tag(),
                                agent.model.clone(),
                                agent.tools.clone(),
                                all_models,
                                all_tools,
                            );
                            app.input_context_focus = None;
                            app.modal_state = ModalState::AgentSettings(settings_state);
                        }
                    }
                }
                InputContextFocus::Branch => {
                    app.input_context_focus = None;
                    app.open_branch_selector();
                }
                InputContextFocus::Nudge => {
                    app.input_context_focus = None;
                    app.open_nudge_selector();
                }
            }
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
            app.chat_editor_mut().insert_newline();
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
            app.chat_editor_mut().move_to_line_end();
            app.vim_enter_insert();
        }
        KeyCode::Char('I') => {
            app.chat_editor_mut().move_to_line_start();
            app.vim_enter_insert();
        }
        KeyCode::Char('o') => {
            app.chat_editor_mut().move_to_line_end();
            app.chat_editor_mut().insert_newline();
            app.vim_enter_insert();
            app.save_chat_draft();
        }
        KeyCode::Char('O') => {
            app.chat_editor_mut().move_to_line_start();
            app.chat_editor_mut().insert_newline();
            app.chat_editor_mut().move_up();
            app.vim_enter_insert();
            app.save_chat_draft();
        }

        // Movement
        KeyCode::Char('h') | KeyCode::Left => {
            app.chat_editor_mut().move_left();
        }
        KeyCode::Char('l') | KeyCode::Right => {
            app.chat_editor_mut().move_right();
        }
        KeyCode::Char('j') | KeyCode::Down => {
            let wrap_width = app.chat_input_wrap_width;
            app.chat_editor_mut().move_down_visual(wrap_width);
        }
        KeyCode::Char('k') | KeyCode::Up => {
            let wrap_width = app.chat_input_wrap_width;
            app.chat_editor_mut().move_up_visual(wrap_width);
        }
        KeyCode::Char('w') => {
            app.chat_editor_mut().move_word_right();
        }
        KeyCode::Char('b') => {
            app.chat_editor_mut().move_word_left();
        }
        KeyCode::Char('0') => {
            app.chat_editor_mut().move_to_line_start();
        }
        KeyCode::Char('$') => {
            app.chat_editor_mut().move_to_line_end();
        }

        // Editing
        KeyCode::Char('x') => {
            app.chat_editor_mut().delete_char_at();
            app.save_chat_draft();
        }
        KeyCode::Char('X') => {
            app.chat_editor_mut().delete_char_before();
            app.save_chat_draft();
        }
        KeyCode::Char('u') => {
            app.chat_editor_mut().undo();
            app.save_chat_draft();
        }
        KeyCode::Char('r') if key.modifiers.contains(KeyModifiers::CONTROL) => {
            app.chat_editor_mut().redo();
            app.save_chat_draft();
        }
        KeyCode::Char('D') => {
            app.chat_editor_mut().kill_to_line_end();
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
            app.chat_editor_mut().insert_newline();
            app.save_chat_draft();
        }

        _ => {}
    }
}

/// Handle sending a message or creating a new thread
fn handle_send_message(app: &mut App) {
    let content = app.chat_editor_mut().submit();
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
            // Per-tab isolated nudge selection
            let nudge_ids = app.selected_nudge_ids();

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
                    response_tx: None,
                }) {
                    app.set_status(&format!("Failed to publish message: {}", e));
                } else {
                    app.delete_chat_draft();
                    app.clear_selected_nudges();
                }
            } else {
                // Create new thread (kind:1)
                let title = content.lines().next().unwrap_or("New Thread").to_string();

                let draft_id = app
                    .find_draft_tab(&project_a_tag)
                    .map(|(_, id)| id.to_string());

                // Get reference_conversation_id from current tab (if set by "Reference conversation" command)
                let reference_conversation_id = app.tabs.active_tab()
                    .and_then(|tab| tab.reference_conversation_id.clone());

                if let Err(e) = core_handle.send(NostrCommand::PublishThread {
                    project_a_tag: project_a_tag.clone(),
                    title,
                    content,
                    agent_pubkey,
                    branch,
                    nudge_ids,
                    reference_conversation_id,
                    response_tx: None,
                }) {
                    app.set_status(&format!("Failed to create thread: {}", e));
                } else {
                    app.pending_new_thread_project = Some(project_a_tag.clone());
                    app.pending_new_thread_draft_id = draft_id;
                    app.clear_selected_nudges();
                    // Clear the reference_conversation_id after sending
                    if let Some(tab) = app.tabs.active_tab_mut() {
                        tab.reference_conversation_id = None;
                    }
                }
            }
        }
    }
}
