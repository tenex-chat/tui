use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::{App, HomeTab, ModalState};

pub(super) fn handle_sidebar_search_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);

    match code {
        // Ctrl+T opens command palette (where '/' can toggle search off)
        // This allows Ctrl+T + / to toggle search off
        KeyCode::Char('t') if has_ctrl => {
            // Open command palette - the '/' command will toggle search off
            app.open_command_palette();
        }
        // Esc clears the query first, then closes the search if already empty
        KeyCode::Esc => {
            if !app.sidebar_search.query.is_empty() {
                app.sidebar_search.query.clear();
                app.sidebar_search.cursor = 0;
                app.update_sidebar_search_results();
            } else {
                // Query is already empty, close the search
                app.sidebar_search.toggle();
            }
        }
        // Enter opens the selected result
        KeyCode::Enter => {
            app.open_selected_search_result();
        }
        // Up/Down navigate results
        KeyCode::Up => {
            app.sidebar_search.move_selection_up();
        }
        KeyCode::Down => {
            // Use appropriate move method based on current tab
            // Note: scroll offset adjustment happens in the renderer where we have real layout data
            if app.home_panel_focus == HomeTab::Reports {
                app.sidebar_search.move_selection_down_reports();
            } else {
                app.sidebar_search.move_selection_down();
            }
        }
        // Left/Right move cursor in query
        KeyCode::Left => {
            app.sidebar_search.move_cursor_left();
        }
        KeyCode::Right => {
            app.sidebar_search.move_cursor_right();
        }
        // Backspace deletes character
        KeyCode::Backspace => {
            app.sidebar_search.delete_char();
            app.update_sidebar_search_results();
        }
        // Character input
        KeyCode::Char(c) => {
            app.sidebar_search.insert_char(c);
            app.update_sidebar_search_results();
        }
        _ => {}
    }
}

pub(super) fn handle_search_modal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.showing_search_modal = false;
            app.search_filter.clear();
            app.search_index = 0;
        }
        KeyCode::Up => {
            if app.search_index > 0 {
                app.search_index -= 1;
            }
        }
        KeyCode::Down => {
            let count = app.search_results().len();
            if app.search_index + 1 < count {
                app.search_index += 1;
            }
        }
        KeyCode::Enter => {
            let results = app.search_results();
            if let Some(result) = results.get(app.search_index).cloned() {
                app.showing_search_modal = false;
                app.search_filter.clear();
                app.search_index = 0;
                app.open_thread_from_home(&result.thread, &result.project_a_tag);
            }
        }
        KeyCode::Char(c) => {
            app.search_filter.push(c);
            app.search_index = 0;
        }
        KeyCode::Backspace => {
            app.search_filter.pop();
            app.search_index = 0;
        }
        _ => {}
    }
}

pub(super) fn handle_chat_search_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => app.exit_chat_search(),
        KeyCode::Enter | KeyCode::Down => app.chat_search_next(),
        KeyCode::Up => app.chat_search_prev(),
        KeyCode::Char(c) => {
            // Per-tab isolated: update query on active tab
            if let Some(tab) = app.tabs.active_tab_mut() {
                tab.chat_search.query.push(c);
            }
            app.update_chat_search();
        }
        KeyCode::Backspace => {
            // Per-tab isolated: update query on active tab
            if let Some(tab) = app.tabs.active_tab_mut() {
                tab.chat_search.query.pop();
            }
            app.update_chat_search();
        }
        _ => {}
    }
}

pub(super) fn handle_history_search_key(app: &mut App, key: KeyEvent) {
    let code = key.code;
    let modifiers = key.modifiers;
    let has_ctrl = modifiers.contains(KeyModifiers::CONTROL);

    match code {
        // Close modal
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
        }

        // Navigate results
        KeyCode::Up | KeyCode::Char('k') if has_ctrl => {
            if let ModalState::HistorySearch(ref mut state) = app.modal_state {
                state.move_up();
            }
        }
        KeyCode::Down | KeyCode::Char('j') if has_ctrl => {
            if let ModalState::HistorySearch(ref mut state) = app.modal_state {
                state.move_down();
            }
        }
        // Arrow keys also navigate
        KeyCode::Up => {
            if let ModalState::HistorySearch(ref mut state) = app.modal_state {
                state.move_up();
            }
        }
        KeyCode::Down => {
            if let ModalState::HistorySearch(ref mut state) = app.modal_state {
                state.move_down();
            }
        }

        // Toggle all projects vs current project
        KeyCode::Tab => {
            if let ModalState::HistorySearch(ref mut state) = app.modal_state {
                state.toggle_all_projects();
                // Re-run search with new filter
                app.update_history_search();
            }
        }

        // Select entry and put content in input
        KeyCode::Enter => {
            let content = if let ModalState::HistorySearch(ref state) = app.modal_state {
                state.selected_entry().map(|e| e.content.clone())
            } else {
                None
            };

            if let Some(content) = content {
                app.modal_state = ModalState::None;
                // Set the content in the chat editor
                app.chat_editor_mut().set_text(&content);
            } else {
                app.modal_state = ModalState::None;
            }
        }

        // Search input - ignore control/alt modifiers to prevent Ctrl+R inserting "r"
        KeyCode::Char(c)
            if !key
                .modifiers
                .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
        {
            if let ModalState::HistorySearch(ref mut state) = app.modal_state {
                state.add_char(c);
            }
            app.update_history_search();
        }
        KeyCode::Backspace => {
            if let ModalState::HistorySearch(ref mut state) = app.modal_state {
                state.backspace();
            }
            app.update_history_search();
        }

        _ => {}
    }
}
