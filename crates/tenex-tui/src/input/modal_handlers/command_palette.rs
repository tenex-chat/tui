use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::input::commands;
use crate::ui::{App, ModalState, View};

pub(super) fn handle_command_palette_key(app: &mut App, key: KeyEvent) {
    // Get available commands before matching on state (to avoid mutable borrow conflict)
    let commands = commands::available_commands(app);
    let cmd_count = commands.len();
    let modifiers = key.modifiers;
    let has_modifiers = modifiers.contains(KeyModifiers::ALT)
        || modifiers.contains(KeyModifiers::CONTROL)
        || modifiers.contains(KeyModifiers::SHIFT);

    if let ModalState::CommandPalette(ref mut state) = app.modal_state {
        match key.code {
            KeyCode::Esc => {
                app.modal_state = ModalState::None;
            }
            // Only trigger tab navigation for plain Left/Right (no modifiers)
            // Alt+Left/Right should NOT switch tabs - just close the palette
            KeyCode::Left if !has_modifiers => {
                app.modal_state = ModalState::None;
                app.prev_tab();
            }
            KeyCode::Right if !has_modifiers => {
                app.modal_state = ModalState::None;
                app.next_tab();
            }
            // Alt+Left/Right closes palette without side effects
            // (user probably meant to do word navigation, which doesn't apply in palette context)
            KeyCode::Left | KeyCode::Right if has_modifiers => {
                app.modal_state = ModalState::None;
            }
            KeyCode::Up => {
                state.move_up();
            }
            KeyCode::Down => {
                state.move_down(cmd_count);
            }
            KeyCode::Enter => {
                if let Some(cmd) = commands.get(state.selected_index) {
                    let cmd_key = cmd.key;
                    app.modal_state = ModalState::None;
                    commands::execute_command(app, cmd_key);
                }
            }
            KeyCode::Char(c) => {
                // First try to execute a matching command
                // If no command matches, 'n' and 'p' are fallback for tab navigation
                let has_command = commands.iter().any(|cmd| cmd.key == c);
                if has_command {
                    app.modal_state = ModalState::None;
                    commands::execute_command(app, c);
                } else if c == 'n' {
                    // 'n' fallback: next tab
                    app.modal_state = ModalState::None;
                    app.next_tab();
                } else if c == 'p' {
                    // 'p' fallback: prev tab
                    app.modal_state = ModalState::None;
                    app.prev_tab();
                } else {
                    // No matching command and not a tab navigation key - just close palette
                    app.modal_state = ModalState::None;
                }
            }
            _ => {}
        }
    }
}

pub(super) fn handle_tab_modal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.close_tab_modal(),
        KeyCode::Up => {
            if app.tab_modal_index() > 0 {
                app.set_tab_modal_index(app.tab_modal_index() - 1);
            }
        }
        KeyCode::Down => {
            if app.tab_modal_index() + 1 < app.open_tabs().len() {
                app.set_tab_modal_index(app.tab_modal_index() + 1);
            }
        }
        KeyCode::Enter => {
            let idx = app.tab_modal_index();
            app.close_tab_modal();
            if idx < app.open_tabs().len() {
                app.switch_to_tab(idx);
                app.view = View::Chat;
            }
        }
        KeyCode::Char('x') => {
            if !app.open_tabs().is_empty() {
                let idx = app.tab_modal_index();
                app.close_tab_at(idx);
                if app.open_tabs().is_empty() {
                    app.close_tab_modal();
                }
            }
        }
        KeyCode::Char('1') => {
            app.close_tab_modal();
            app.go_home();
        }
        KeyCode::Char(c) if ('2'..='9').contains(&c) => {
            let tab_index = (c as usize) - ('2' as usize);
            app.close_tab_modal();
            if tab_index < app.open_tabs().len() {
                app.switch_to_tab(tab_index);
                app.view = View::Chat;
            }
        }
        _ => {}
    }
}
