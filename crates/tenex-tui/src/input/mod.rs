//! Input handling module - keyboard event processing for the TUI application.
//!
//! This module splits input handling into focused sub-modules:
//! - `modal_handlers`: Handlers for various modal dialogs
//! - `view_handlers`: Handlers for main view input (Home, Chat, etc.)
//! - `editor_handlers`: Handlers for text editing (chat editor, vim mode)
//! - `commands`: Command definitions and execution for the command palette

pub mod commands;
mod modal_handlers;
mod view_handlers;
mod editor_handlers;
pub mod input_prefix;

use anyhow::Result;
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::hotkeys::{resolve_hotkey, HotkeyId};
use crate::ui::modal::WorkspaceManagerState;
use crate::ui::views::login::LoginStep;
use crate::ui::{App, InputMode, ModalState, View};

// Re-export handlers for use in main input function
use modal_handlers::*;
use view_handlers::*;
use editor_handlers::*;

/// Main entry point for handling keyboard events.
/// Routes events to appropriate handlers based on current modal state, view, and input mode.
pub(crate) fn handle_key(
    app: &mut App,
    key: KeyEvent,
    login_step: &mut LoginStep,
    pending_nsec: &mut Option<String>,
) -> Result<()> {
    let code = key.code;
    let modifiers = key.modifiers;

    // =========================================================================
    // HIGH-PRIORITY GLOBAL HOTKEYS (work from anywhere, including within modals)
    // =========================================================================
    // These are resolved first using the centralized hotkey registry.
    // Only the highest-priority hotkeys should be here (e.g., Ctrl+T for command palette).

    // Check if we're in a text input context where single keys should not be intercepted
    // Check specifically for DebugStats on ETagQuery tab
    let in_debug_etag_input = matches!(
        &app.modal_state,
        ModalState::DebugStats(state) if state.active_tab == crate::ui::modal::DebugStatsTab::ETagQuery
    );
    let in_text_input_context = app.view == View::Login
        || app.input_mode == InputMode::Editing
        || in_debug_etag_input
        || matches!(
            app.modal_state,
            ModalState::CommandPalette(_)
                | ModalState::AgentSelector { .. }
                | ModalState::ProjectsModal { .. }
                | ModalState::CreateAgent(_)
                | ModalState::CreateProject(_)
                | ModalState::ProjectSettings(_)
                | ModalState::AgentSettings(_)
                | ModalState::ExpandedEditor { .. }
                | ModalState::AskModal(_)
        );

    // For text input contexts, only handle modified keys (Ctrl+, Alt+)
    // For non-text contexts, handle all hotkeys
    let should_check_hotkeys = if in_text_input_context {
        // Only intercept if there's a modifier (Ctrl or Alt)
        modifiers.contains(KeyModifiers::CONTROL) || modifiers.contains(KeyModifiers::ALT)
    } else {
        true
    };

    if should_check_hotkeys {
        let context = app.hotkey_context();
        if let Some(hotkey_id) = resolve_hotkey(code, modifiers, context) {
            match hotkey_id {
                // Ctrl+T opens command palette (always works, high priority)
                HotkeyId::CommandPalette => {
                    // Don't open another command palette if one is already open
                    if !matches!(app.modal_state, ModalState::CommandPalette(_)) {
                        app.open_command_palette();
                    }
                    return Ok(());
                }
                // Help modal (only if we're not in a text input context)
                HotkeyId::Help if !in_text_input_context => {
                    if !matches!(app.modal_state, ModalState::HotkeyHelp) {
                        app.modal_state = ModalState::HotkeyHelp;
                    }
                    return Ok(());
                }
                // Alt+M: Jump to notification thread (works from anywhere when notification has thread_id)
                HotkeyId::JumpToNotification => {
                    // Let jump_to_notification_thread() handle all validation and error feedback
                    app.jump_to_notification_thread();
                    return Ok(());
                }
                // Ctrl+Shift+T: Open workspace manager
                HotkeyId::WorkspaceManager => {
                    if !matches!(app.modal_state, ModalState::WorkspaceManager(_)) {
                        app.modal_state = ModalState::WorkspaceManager(WorkspaceManagerState::new());
                    }
                    return Ok(());
                }
                // Other global hotkeys are handled later in their respective sections
                _ => {}
            }
        }
    }

    // =========================================================================
    // GLOBAL AUDIO PLAYER CONTROLS (work from anywhere)
    // =========================================================================

    // Alt+S: Stop audio playback
    if code == KeyCode::Char('s') && modifiers.contains(KeyModifiers::ALT) {
        app.audio_player.stop();
        app.set_warning_status("Audio stopped");
        return Ok(());
    }

    // Alt+R: Replay last audio
    if code == KeyCode::Char('r') && modifiers.contains(KeyModifiers::ALT) {
        match app.audio_player.replay() {
            Ok(()) => {
                if let Some(name) = app.audio_player.current_audio_name() {
                    app.set_warning_status(&format!("Replaying: {}", name));
                } else {
                    app.set_warning_status("Replaying audio");
                }
            }
            Err(e) => {
                app.set_warning_status(&format!("Replay failed: {}", e));
            }
        }
        return Ok(());
    }

    // =========================================================================
    // MODAL HANDLERS - Process modal-specific input first
    // =========================================================================

    if handle_modal_input(app, key)? {
        return Ok(());
    }

    // =========================================================================
    // GLOBAL TAB NAVIGATION (works in all views except Login)
    // =========================================================================

    if app.view != View::Login {
        if handle_global_tab_navigation(app, key)? {
            return Ok(());
        }
    }

    // =========================================================================
    // VIEW-SPECIFIC HANDLERS
    // =========================================================================

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
        if handle_chat_normal_mode(app, key)? {
            return Ok(());
        }
    }

    // =========================================================================
    // INPUT MODE HANDLERS
    // =========================================================================

    match app.input_mode {
        InputMode::Normal => handle_normal_mode(app, key, login_step, pending_nsec)?,
        InputMode::Editing => handle_editing_mode(app, key, login_step, pending_nsec)?,
    }

    Ok(())
}

/// Handle global tab navigation with Alt key (works in all views except Login)
fn handle_global_tab_navigation(app: &mut App, key: KeyEvent) -> Result<bool> {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_alt = modifiers.contains(KeyModifiers::ALT);

    // macOS Option key produces special characters instead of Alt+key
    // Handle these characters for various shortcuts
    match code {
        // Option+M on macOS produces 'µ' - jump to notification
        KeyCode::Char('µ') => {
            app.jump_to_notification_thread();
            return Ok(true);
        }
        // Option+1 on macOS produces '¡' - go to dashboard
        KeyCode::Char('¡') => {
            app.go_home();
            return Ok(true);
        }
        // Option+2..9 on macOS produces special chars - switch to tab
        // ™=2, £=3, ¢=4, ∞=5, §=6, ¶=7, •=8, ª=9
        KeyCode::Char(c) if matches!(c, '™' | '£' | '¢' | '∞' | '§' | '¶' | '•' | 'ª') => {
            let tab_index = match c {
                '™' => 0, // Option+2 -> tab 0
                '£' => 1, // Option+3 -> tab 1
                '¢' => 2, // Option+4 -> tab 2
                '∞' => 3, // Option+5 -> tab 3
                '§' => 4, // Option+6 -> tab 4
                '¶' => 5, // Option+7 -> tab 5
                '•' => 6, // Option+8 -> tab 6
                'ª' => 7, // Option+9 -> tab 7
                _ => return Ok(false),
            };
            if tab_index < app.open_tabs().len() {
                app.switch_to_tab(tab_index);
                app.view = View::Chat;
            }
            return Ok(true);
        }
        _ => {}
    }

    if has_alt {
        match code {
            // Alt+1 = go to dashboard (home) - always first tab
            KeyCode::Char('1') => {
                app.go_home();
                return Ok(true);
            }
            // Alt+2..9 = jump directly to tab N-1 (since 1 is Home)
            KeyCode::Char(c) if c >= '2' && c <= '9' => {
                let tab_index = (c as usize) - ('2' as usize); // '2' -> 0, '3' -> 1, etc.
                if tab_index < app.open_tabs().len() {
                    app.switch_to_tab(tab_index);
                    app.view = View::Chat;
                }
                return Ok(true);
            }
            _ => {}
        }
    }

    Ok(false)
}
