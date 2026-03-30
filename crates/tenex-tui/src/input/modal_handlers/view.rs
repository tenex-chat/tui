use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::ui::{App, ModalState};
use tenex_core::stats::query_events_by_e_tag;

pub(super) fn handle_view_raw_event_modal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc => {
            app.modal_state = ModalState::None;
        }
        KeyCode::Char('y') => {
            if let ModalState::ViewRawEvent { ref json, .. } = app.modal_state {
                use arboard::Clipboard;
                match Clipboard::new() {
                    Ok(mut clipboard) => {
                        if clipboard.set_text(json).is_ok() {
                            app.set_warning_status("Raw event copied to clipboard");
                        } else {
                            app.set_warning_status("Failed to copy to clipboard");
                        }
                    }
                    Err(_) => {
                        app.set_warning_status("Failed to access clipboard");
                    }
                }
            }
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

pub(super) fn handle_hotkey_help_modal_key(app: &mut App, key: KeyEvent) {
    match key.code {
        KeyCode::Esc | KeyCode::Enter | KeyCode::Char('?') | KeyCode::Char('q') => {
            app.modal_state = ModalState::None;
        }
        _ => {
            app.modal_state = ModalState::None;
        }
    }
}

pub(super) fn handle_debug_stats_modal_key(app: &mut App, key: KeyEvent) {
    use crate::ui::modal::DebugStatsTab;

    let has_ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

    // Check if we're on the E-Tag Query tab - always accept input there
    let is_e_tag_query_tab = matches!(
        &app.modal_state,
        ModalState::DebugStats(state) if state.active_tab == DebugStatsTab::ETagQuery
    );

    if is_e_tag_query_tab {
        match key.code {
            KeyCode::Esc => {
                if let ModalState::DebugStats(ref mut state) = app.modal_state {
                    if !state.e_tag_query_input.is_empty() {
                        // Clear input first
                        state.e_tag_query_input.clear();
                        state.e_tag_query_results.clear();
                    } else {
                        app.modal_state = ModalState::None;
                    }
                }
            }
            KeyCode::Enter => {
                // Execute the query
                if let ModalState::DebugStats(ref mut state) = app.modal_state {
                    if !state.e_tag_query_input.is_empty() {
                        let results = query_events_by_e_tag(&app.db.ndb, &state.e_tag_query_input);
                        state.e_tag_query_results = results;
                        state.e_tag_selected_index = 0;
                    }
                }
            }
            KeyCode::Backspace => {
                if let ModalState::DebugStats(ref mut state) = app.modal_state {
                    state.e_tag_query_input.pop();
                }
            }
            // Ctrl+V to paste from clipboard
            KeyCode::Char('v') if has_ctrl => {
                if let ModalState::DebugStats(ref mut state) = app.modal_state {
                    if let Ok(mut clipboard) = arboard::Clipboard::new() {
                        if let Ok(text) = clipboard.get_text() {
                            // Filter to only hex characters and limit to 64
                            let hex_only: String = text
                                .chars()
                                .filter(|c| c.is_ascii_hexdigit())
                                .take(64 - state.e_tag_query_input.len())
                                .map(|c| c.to_ascii_lowercase())
                                .collect();
                            state.e_tag_query_input.push_str(&hex_only);
                        }
                    }
                }
            }
            // Ctrl+A to clear
            KeyCode::Char('a') if has_ctrl => {
                if let ModalState::DebugStats(ref mut state) = app.modal_state {
                    state.e_tag_query_input.clear();
                }
            }
            KeyCode::Char(c) => {
                // Accept hex characters directly - this is the main input handler
                if c.is_ascii_hexdigit() {
                    if let ModalState::DebugStats(ref mut state) = app.modal_state {
                        if state.e_tag_query_input.len() < 64 {
                            state.e_tag_query_input.push(c.to_ascii_lowercase());
                        }
                    }
                }
            }
            KeyCode::Tab => {
                if let ModalState::DebugStats(ref mut state) = app.modal_state {
                    if key.modifiers.contains(KeyModifiers::SHIFT) {
                        state.prev_tab();
                    } else {
                        state.next_tab();
                    }
                }
            }
            KeyCode::Up | KeyCode::Down => {
                // Navigate results if we have any
                if let ModalState::DebugStats(ref mut state) = app.modal_state {
                    if !state.e_tag_query_results.is_empty() {
                        if key.code == KeyCode::Up {
                            state.e_tag_selected_index =
                                state.e_tag_selected_index.saturating_sub(1);
                        } else if state.e_tag_selected_index + 1 < state.e_tag_query_results.len() {
                            state.e_tag_selected_index += 1;
                        }
                    }
                }
            }
            _ => {}
        }
        return;
    }

    // Get subscription stats snapshot upfront (before mutable borrow)
    let sub_stats_snapshot = app.subscription_stats.snapshot();

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('D') => {
            app.modal_state = ModalState::None;
        }
        // Tab navigation: Tab key cycles through tabs
        KeyCode::Tab => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                if key.modifiers.contains(KeyModifiers::SHIFT) {
                    state.prev_tab();
                } else {
                    state.next_tab();
                }
                if state.active_tab == DebugStatsTab::Subscriptions {
                    state.update_project_filters(&sub_stats_snapshot);
                }
            }
        }
        // Number keys for direct tab access
        KeyCode::Char('1') => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                state.switch_tab(DebugStatsTab::Events);
            }
        }
        KeyCode::Char('2') => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                state.switch_tab(DebugStatsTab::Subscriptions);
                state.update_project_filters(&sub_stats_snapshot);
            }
        }
        KeyCode::Char('3') => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                state.switch_tab(DebugStatsTab::Negentropy);
            }
        }
        KeyCode::Char('4') => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                state.switch_tab(DebugStatsTab::ETagQuery);
            }
        }
        KeyCode::Char('5') => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                state.switch_tab(DebugStatsTab::DataStore);
            }
        }
        // Left/Right arrows - switch tabs or panes depending on context
        KeyCode::Left | KeyCode::Char('h') => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                if state.active_tab == DebugStatsTab::Subscriptions && !state.sub_sidebar_focused {
                    // Switch from content to sidebar
                    state.sub_sidebar_focused = true;
                } else {
                    state.prev_tab();
                    if state.active_tab == DebugStatsTab::Subscriptions {
                        state.update_project_filters(&sub_stats_snapshot);
                    }
                }
            }
        }
        KeyCode::Right | KeyCode::Char('l') => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                if state.active_tab == DebugStatsTab::Subscriptions && state.sub_sidebar_focused {
                    // Switch from sidebar to content (no-op for now, sidebar is main navigation)
                    state.sub_sidebar_focused = false;
                } else {
                    state.next_tab();
                    if state.active_tab == DebugStatsTab::Subscriptions {
                        state.update_project_filters(&sub_stats_snapshot);
                    }
                }
            }
        }
        // Up/Down navigation
        KeyCode::Up | KeyCode::Char('k') => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                match state.active_tab {
                    DebugStatsTab::ETagQuery if !state.e_tag_query_results.is_empty() => {
                        state.e_tag_selected_index = state.e_tag_selected_index.saturating_sub(1);
                    }
                    DebugStatsTab::Subscriptions if state.sub_sidebar_focused => {
                        state.sub_selected_filter_index =
                            state.sub_selected_filter_index.saturating_sub(1);
                    }
                    _ => {
                        state.scroll_offset = state.scroll_offset.saturating_sub(1);
                    }
                }
            }
        }
        KeyCode::Down | KeyCode::Char('j') => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                match state.active_tab {
                    DebugStatsTab::ETagQuery if !state.e_tag_query_results.is_empty() => {
                        if state.e_tag_selected_index + 1 < state.e_tag_query_results.len() {
                            state.e_tag_selected_index += 1;
                        }
                    }
                    DebugStatsTab::Subscriptions if state.sub_sidebar_focused => {
                        if state.sub_selected_filter_index + 1 < state.sub_project_filters.len() {
                            state.sub_selected_filter_index += 1;
                        }
                    }
                    _ => {
                        state.scroll_offset = state.scroll_offset.saturating_add(1);
                    }
                }
            }
        }
        // Enter to select filter on subscriptions tab or view event on event feed tab
        KeyCode::Enter => {
            if let ModalState::DebugStats(ref state) = app.modal_state {
                if state.active_tab == DebugStatsTab::Subscriptions {
                    // Selection is immediate via sub_selected_filter_index, Enter just confirms
                    if let ModalState::DebugStats(ref mut state) = app.modal_state {
                        state.sub_sidebar_focused = false;
                    }
                }
            }
        }
        KeyCode::PageUp => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                state.scroll_offset = state.scroll_offset.saturating_sub(10);
            }
        }
        KeyCode::PageDown => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                state.scroll_offset = state.scroll_offset.saturating_add(10);
            }
        }
        // Focus input on E-Tag Query tab
        KeyCode::Char('i') | KeyCode::Char('/') => {
            if let ModalState::DebugStats(ref mut state) = app.modal_state {
                if state.active_tab == DebugStatsTab::ETagQuery {
                    state.e_tag_input_focused = true;
                }
            }
        }
        _ => {}
    }
}
