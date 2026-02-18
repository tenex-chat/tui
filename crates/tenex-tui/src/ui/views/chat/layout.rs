use crate::ui::components::{
    render_chat_sidebar, render_modal_items, render_statusbar, render_tab_bar,
    ConversationMetadata, Modal, ModalItem, ModalSize,
};
use crate::ui::format::truncate_with_ellipsis;
use crate::ui::layout;
use crate::ui::modal::ChatActionsState;
use crate::ui::state::TabContentType;
use crate::ui::text_editor::TextEditor;
use crate::ui::theme;
use crate::ui::todo::aggregate_todo_state;
use crate::ui::views::{render_report_tab, render_tts_control};
use crate::ui::{App, ModalState};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::rc::Rc;

use super::{actions, input, messages};

pub fn render_chat(f: &mut Frame, app: &mut App, area: Rect) {
    // Fill entire area with app background (pure black)
    let bg_block = Block::default().style(Style::default().bg(theme::BG_APP));
    f.render_widget(bg_block, area);

    // Check active tab content type and dispatch to appropriate renderer
    let content_type = app
        .tabs
        .active_tab()
        .map(|t| t.content_type.clone())
        .unwrap_or(TabContentType::Conversation);

    match content_type {
        TabContentType::TTSControl => {
            render_tts_tab_layout(f, app, area);
            return;
        }
        TabContentType::Report { .. } => {
            render_report_tab_layout(f, app, area);
            return;
        }
        TabContentType::Conversation => {
            // Continue with normal conversation rendering below
        }
    }

    // Create top-level layout: header + content
    let main_chunks = Layout::vertical([
        Constraint::Length(layout::HEADER_HEIGHT_CHAT),
        Constraint::Min(0),
    ])
    .split(area);

    // Render header
    let chrome_color = if app.pending_quit {
        theme::ACCENT_ERROR
    } else {
        theme::ACCENT_PRIMARY
    };
    let title = app
        .selected_thread()
        .map(|t| t.title.clone())
        .or_else(|| {
            app.open_tabs()
                .get(app.active_tab_index())
                .map(|tab| tab.thread_title.clone())
        })
        .unwrap_or_else(|| "Chat".to_string());
    let padding = " ".repeat(layout::CONTENT_PADDING_H as usize);
    let header = Paragraph::new(format!("\n{}{}", padding, title)).style(
        Style::default()
            .fg(chrome_color)
            .add_modifier(ratatui::style::Modifier::BOLD),
    );
    f.render_widget(header, main_chunks[0]);

    // Content area (everything below header)
    let content_area = main_chunks[1];

    let all_messages = app.messages();

    // NOTE: Sidebar state is updated on data-change events (message arrival, tab switch)
    // not during render, to keep render pure. See update_sidebar_from_messages().

    // Aggregate todo state from all messages
    let todo_state = aggregate_todo_state(&all_messages);

    // Calculate total LLM runtime hierarchically (own + all children recursively)
    // This uses the RuntimeHierarchy in the data store for efficient recursive lookups
    let total_llm_runtime_ms: u64 = app
        .selected_thread()
        .map(|thread| {
            let store = app.data_store.borrow();
            store.get_hierarchical_runtime(&thread.id)
        })
        .unwrap_or(0);

    // Build conversation metadata from selected thread, including work status
    let metadata = app
        .selected_thread()
        .map(|thread| {
            // Get working agent names from 24133 events
            let working_agents = {
                let store = app.data_store.borrow();
                let agent_pubkeys = store.operations.get_working_agents(&thread.id);

                // Get project a_tag for potential fallback to project status
                let project_a_tag = store.find_project_for_thread(&thread.id);

                // Resolve pubkeys to agent names
                // Priority: 1) kind:0 profile, 2) agent slug from project status, 3) short pubkey
                agent_pubkeys
                    .iter()
                    .map(|pubkey| {
                        // Primary: Use kind:0 profile name
                        let profile_name = store.get_profile_name(pubkey);

                        // If profile name is just short pubkey, try project status as fallback
                        if profile_name.ends_with("...") {
                            project_a_tag
                                .as_ref()
                                .and_then(|a_tag| store.get_project_status(a_tag))
                                .and_then(|status| {
                                    status
                                        .agents
                                        .iter()
                                        .find(|a| a.pubkey == *pubkey)
                                        .map(|a| a.name.clone())
                                })
                                .unwrap_or(profile_name)
                        } else {
                            profile_name
                        }
                    })
                    .collect()
            };
            // Normalize summary: treat empty/whitespace-only as None
            let summary = thread
                .summary
                .as_ref()
                .filter(|s| !s.trim().is_empty())
                .cloned();

            ConversationMetadata {
                title: Some(thread.title.clone()),
                status_label: thread.status_label.clone(),
                status_current_activity: thread.status_current_activity.clone(),
                summary,
                working_agents,
                total_llm_runtime_ms,
            }
        })
        .unwrap_or_default();

    let input_height = input::input_height(app);
    let has_attachments = input::has_attachments(app);
    let has_status = input::has_status(app);
    let has_tabs = !app.open_tabs().is_empty();

    // Build layout based on whether we have attachments, status, and tabs
    // Context line is now INSIDE the input card, not separate
    let chunks = build_layout(
        content_area,
        input_height,
        has_attachments,
        has_status,
        has_tabs,
    );

    // Split messages area horizontally - always show sidebar when visible
    let (messages_area_raw, sidebar_area) =
        split_messages_area(chunks[0], app.todo_sidebar_visible);

    // Add horizontal padding to messages area for breathing room (consistent with Home)
    let messages_area = layout::with_content_padding(messages_area_raw);

    messages::render_messages_panel(f, app, messages_area, &all_messages);

    // Render chat sidebar (work indicator + todos + delegations + reports + metadata)
    if let Some(sidebar) = sidebar_area {
        render_chat_sidebar(
            f,
            &todo_state,
            &metadata,
            &app.sidebar_state,
            app.spinner_char(),
            sidebar,
        );
    }

    // Calculate chunk indices based on layout
    // Layout: [messages, (status?), (attachments?), input, (tabs?)]
    let mut idx = 1; // Start after messages

    // Status line (if any)
    if has_status {
        input::render_status_line(f, app, chunks[idx]);
        idx += 1;
    }

    // Attachments line (if any)
    if has_attachments {
        input::render_attachments_line(f, app, chunks[idx]);
        idx += 1;
    }

    // Input area - always show normal chat input (ask UI is inline with messages now)
    input::render_input_box(f, app, chunks[idx]);
    idx += 1;

    // Tab bar (if tabs are open)
    if has_tabs {
        render_tab_bar(f, app, chunks[idx]);
        idx += 1;
    }

    // Status bar at the very bottom (always visible)
    let (cumulative_runtime_ms, has_active_agents, active_agent_count) =
        app.data_store.borrow_mut().get_statusbar_runtime_ms();
    let audio_playing = app.audio_player.is_playing();
    render_statusbar(
        f,
        chunks[idx],
        app.current_notification(),
        cumulative_runtime_ms,
        has_active_agents,
        active_agent_count,
        app.wave_offset(),
        audio_playing,
    );

    // Render attachment modal if showing
    if app.is_attachment_modal_open() {
        render_attachment_modal(f, app, area);
    }

    // Render expanded editor modal if showing (Ctrl+E)
    if let ModalState::ExpandedEditor { ref editor } = app.modal_state {
        render_expanded_editor_modal(f, editor, area);
    }

    // Render tab modal if showing (Alt+/)
    if app.showing_tab_modal() {
        super::super::home::render_tab_modal(f, app, area);
    }

    // Render view raw event modal if showing
    if let ModalState::ViewRawEvent {
        json,
        scroll_offset,
        ..
    } = &app.modal_state
    {
        actions::render_view_raw_event_modal(f, json, *scroll_offset, area);
    }

    // Render hotkey help modal if showing
    if matches!(app.modal_state, ModalState::HotkeyHelp) {
        actions::render_hotkey_help_modal(f, area);
    }

    // Render report viewer modal if showing
    if let ModalState::ReportViewer(ref state) = app.modal_state {
        super::super::render_report_viewer(f, app, area, state);
    }

    // Render unified nudges/skills selector modal if showing
    if let ModalState::NudgeSkillSelector(ref state) = app.modal_state {
        super::super::render_nudge_skill_selector(f, app, area, state);
    }

    // Render chat actions modal if showing (Ctrl+T /)
    if let ModalState::ChatActions(ref state) = app.modal_state {
        render_chat_actions_modal(f, area, state);
    }

    // Render unified agent configuration modal if showing
    let filtered_agents = app.filtered_agents();
    if let ModalState::AgentConfig(ref mut state) = app.modal_state {
        render_agent_config_modal(f, area, state, &filtered_agents);
    }

    // Render draft navigator modal if showing
    if let ModalState::DraftNavigator(ref state) = app.modal_state {
        super::super::render_draft_navigator(f, area, state);
    }

    // Render history search modal if showing (Ctrl+R)
    if let ModalState::HistorySearch(ref state) = app.modal_state {
        super::super::render_history_search(f, area, state);
    }

    // Render backend approval modal if showing
    if let ModalState::BackendApproval(ref state) = app.modal_state {
        super::super::render_backend_approval_modal(f, area, state);
    }

    // Render projects modal if showing (Ctrl+T Shift+P from Chat view)
    if matches!(app.modal_state, ModalState::ProjectsModal { .. }) {
        super::super::render_projects_modal(f, app, area);
    }

    // Render composer project selector modal if showing (for changing project in new conversations)
    if matches!(app.modal_state, ModalState::ComposerProjectSelector { .. }) {
        super::super::render_composer_project_selector(f, app, area);
    }

    // Command palette overlay (Ctrl+T)
    if let ModalState::CommandPalette(ref state) = app.modal_state {
        super::super::render_command_palette(f, area, app, state.selected_index);
    }

    // Debug stats modal (Ctrl+T D)
    if let ModalState::DebugStats(ref state) = app.modal_state {
        super::super::render_debug_stats(f, area, app, state);
    }

    // Workspace manager modal
    if let ModalState::WorkspaceManager(ref state) = app.modal_state {
        let workspaces = app.preferences.borrow().workspaces().to_vec();
        let projects = app.data_store.borrow().get_projects().to_vec();
        let active_id = app
            .preferences
            .borrow()
            .active_workspace_id()
            .map(String::from);
        super::super::render_workspace_manager(
            f,
            area,
            state,
            &workspaces,
            &projects,
            active_id.as_deref(),
        );
    }

    // App settings modal
    if let ModalState::AppSettings(ref state) = app.modal_state {
        super::super::render_app_settings(f, app, area, state);
    }

    // Global search modal overlay (/)
    if app.showing_search_modal {
        super::super::home::render_search_modal(f, app, area);
    }

    // In-conversation search overlay (Ctrl+F) - per-tab isolated
    if app.is_chat_search_active() {
        render_chat_search_bar(f, app, area);
    }
}

fn build_layout(
    area: Rect,
    input_height: u16,
    has_attachments: bool,
    has_status: bool,
    has_tabs: bool,
) -> Rc<[Rect]> {
    // All layouts end with statusbar at the very bottom
    match (has_attachments, has_status, has_tabs) {
        (true, true, true) => Layout::vertical([
            Constraint::Min(0),                           // Messages
            Constraint::Length(1),                        // Status line
            Constraint::Length(1),                        // Attachments line
            Constraint::Length(input_height),             // Input (includes context)
            Constraint::Length(layout::TAB_BAR_HEIGHT),   // Tab bar
            Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
        ])
        .split(area),
        (true, true, false) => Layout::vertical([
            Constraint::Min(0),                           // Messages
            Constraint::Length(1),                        // Status line
            Constraint::Length(1),                        // Attachments line
            Constraint::Length(input_height),             // Input (includes context)
            Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
        ])
        .split(area),
        (true, false, true) => Layout::vertical([
            Constraint::Min(0),                           // Messages
            Constraint::Length(1),                        // Attachments line
            Constraint::Length(input_height),             // Input (includes context)
            Constraint::Length(layout::TAB_BAR_HEIGHT),   // Tab bar
            Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
        ])
        .split(area),
        (true, false, false) => Layout::vertical([
            Constraint::Min(0),                           // Messages
            Constraint::Length(1),                        // Attachments line
            Constraint::Length(input_height),             // Input (includes context)
            Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
        ])
        .split(area),
        (false, true, true) => Layout::vertical([
            Constraint::Min(0),                           // Messages
            Constraint::Length(1),                        // Status line
            Constraint::Length(input_height),             // Input (includes context)
            Constraint::Length(layout::TAB_BAR_HEIGHT),   // Tab bar
            Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
        ])
        .split(area),
        (false, true, false) => Layout::vertical([
            Constraint::Min(0),                           // Messages
            Constraint::Length(1),                        // Status line
            Constraint::Length(input_height),             // Input (includes context)
            Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
        ])
        .split(area),
        (false, false, true) => Layout::vertical([
            Constraint::Min(0),                           // Messages
            Constraint::Length(input_height),             // Input (includes context)
            Constraint::Length(layout::TAB_BAR_HEIGHT),   // Tab bar
            Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
        ])
        .split(area),
        (false, false, false) => Layout::vertical([
            Constraint::Min(0),                           // Messages
            Constraint::Length(input_height),             // Input (includes context)
            Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
        ])
        .split(area),
    }
}

fn split_messages_area(messages_area: Rect, show_sidebar: bool) -> (Rect, Option<Rect>) {
    if show_sidebar {
        let horiz = Layout::horizontal([
            Constraint::Min(40),
            Constraint::Length(layout::SIDEBAR_WIDTH_CHAT),
        ])
        .split(messages_area);
        (horiz[0], Some(horiz[1]))
    } else {
        (messages_area, None)
    }
}

fn render_attachment_modal(f: &mut Frame, app: &App, area: Rect) {
    // Large modal covering most of the screen
    let popup_width = (area.width as f32 * 0.8) as u16;
    let popup_height = (area.height as f32 * 0.8) as u16;
    let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the modal
    f.render_widget(Clear, popup_area);

    // Get attachment info for title
    let title = if let Some(attachment) = app.chat_editor().get_focused_attachment() {
        format!(
            "Paste #{} ({} lines, {} chars) - Ctrl+S save, Ctrl+D delete, Esc cancel",
            attachment.id,
            attachment.line_count(),
            attachment.char_count()
        )
    } else {
        "Attachment Editor".to_string()
    };

    // Get editor reference
    let editor = app.attachment_modal_editor();

    // Render the modal content
    let modal = Paragraph::new(editor.text.as_str())
        .style(Style::default().fg(theme::TEXT_PRIMARY))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT_WARNING))
                .title(title),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(modal, popup_area);

    // Show cursor in the modal
    let (cursor_row, cursor_col) = editor.cursor_position();
    f.set_cursor_position((
        popup_area.x + cursor_col as u16 + 1,
        popup_area.y + cursor_row as u16 + 1,
    ));
}

fn render_expanded_editor_modal(f: &mut Frame, editor: &TextEditor, area: Rect) {
    let line_count = editor.text.lines().count().max(1);
    let char_count = editor.text.len();
    let title = format!(
        "Expanded Editor ({} lines, {} chars)",
        line_count, char_count
    );

    let (popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: (area.width as f32 * 0.85) as u16,
            height_percent: 0.8,
        })
        .render_frame(f, area);

    // Content area for the text editor (with horizontal padding)
    let editor_area = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height.saturating_sub(2),
    );

    // Render the text content
    let text = Paragraph::new(editor.text.as_str())
        .style(Style::default().fg(theme::TEXT_PRIMARY))
        .wrap(Wrap { trim: false });
    f.render_widget(text, editor_area);

    // Render hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints =
        Paragraph::new("ctrl+s save · esc cancel").style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);

    // Show cursor in the modal (offset by content area position)
    let (cursor_row, cursor_col) = editor.cursor_position();
    f.set_cursor_position((
        editor_area.x + cursor_col as u16,
        editor_area.y + cursor_row as u16,
    ));
}

/// Render the chat actions modal (Ctrl+T /)
fn render_chat_actions_modal(f: &mut Frame, area: Rect, state: &ChatActionsState) {
    let actions = state.available_actions();
    let content_height = (actions.len() + 2) as u16;
    let total_height = content_height + 4;
    let height_percent = (total_height as f32 / area.height as f32).min(0.5);

    // Truncate title if too long
    let title = truncate_with_ellipsis(&state.thread_title, 35);

    let items: Vec<ModalItem> = actions
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let is_selected = i == state.selected_index;
            ModalItem::new(action.label())
                .with_shortcut(action.hotkey().to_string())
                .selected(is_selected)
        })
        .collect();

    let popup_area = Modal::new(&title)
        .size(ModalSize {
            max_width: 45,
            height_percent,
        })
        .render(f, area, |f, content_area| {
            render_modal_items(f, content_area, &items);
        });

    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("↑↓ navigate · enter select · esc close")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

/// Render the unified 3-column agent selection + configuration modal.
fn render_agent_config_modal(
    f: &mut Frame,
    area: Rect,
    state: &mut crate::ui::modal::AgentConfigState,
    agents: &[crate::models::ProjectAgent],
) {
    use crate::ui::components::visible_items_in_content_area;
    use crate::ui::modal::AgentConfigFocus;
    use ratatui::style::Modifier;

    state.selector.clamp_index(agents.len());

    let model_count = state
        .settings
        .as_ref()
        .map(|s| s.available_models.len())
        .unwrap_or(1);
    let tools_count = state
        .settings
        .as_ref()
        .map(|s| s.visible_item_count())
        .unwrap_or(1);
    let content_height = (agents.len().max(model_count).max(tools_count) as u16 + 8).min(32);
    let height_percent = (content_height as f32 / area.height as f32).min(0.86);
    let compact_modal = area.width < 96;

    let title = state
        .settings
        .as_ref()
        .map(|s| {
            if compact_modal {
                format!(
                    "Agent Config · {}",
                    truncate_with_ellipsis(&s.agent_name, 24)
                )
            } else {
                format!("Agent Configuration · {}", s.agent_name)
            }
        })
        .unwrap_or_else(|| {
            if compact_modal {
                "Agent Config".to_string()
            } else {
                "Agent Configuration".to_string()
            }
        });

    let (popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 112,
            height_percent,
        })
        .render_frame(f, area);

    let horizontal_inset = if content_area.width >= 80 { 2 } else { 1 };
    let inner = Rect::new(
        content_area.x + horizontal_inset,
        content_area.y,
        content_area.width.saturating_sub(horizontal_inset * 2),
        content_area.height.saturating_sub(1),
    );

    if inner.width < 9 || inner.height < 4 {
        let message_area = Rect::new(
            popup_area.x + 1,
            popup_area.y + popup_area.height / 2,
            popup_area.width.saturating_sub(2),
            1,
        );
        f.render_widget(
            Paragraph::new("Terminal too small for agent modal")
                .style(Style::default().fg(theme::TEXT_MUTED)),
            message_area,
        );
        return;
    }

    let column_gap = if inner.width >= 54 { 1 } else { 0 };
    let column_constraints = if inner.width >= 78 {
        [
            Constraint::Percentage(34),
            Constraint::Percentage(29),
            Constraint::Percentage(37),
        ]
    } else {
        [
            Constraint::Percentage(34),
            Constraint::Percentage(33),
            Constraint::Percentage(33),
        ]
    };
    let columns = Layout::horizontal(column_constraints)
        .spacing(column_gap)
        .split(inner);

    let agents_area = columns[0];
    let model_area = columns[1];
    let tools_area = columns[2];

    // Subtle column differentiation
    f.render_widget(
        Block::default().style(Style::default().bg(theme::BG_SECONDARY)),
        agents_area,
    );
    f.render_widget(
        Block::default().style(Style::default().bg(theme::BG_MODAL)),
        model_area,
    );
    f.render_widget(
        Block::default().style(Style::default().bg(theme::BG_CARD)),
        tools_area,
    );

    let agents_header_style = if state.focus == AgentConfigFocus::Agents {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    let model_header_style = if state.focus == AgentConfigFocus::Model {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    let tools_header_style = if state.focus == AgentConfigFocus::Tools {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };

    f.render_widget(
        Paragraph::new("Agents").style(agents_header_style),
        agents_area,
    );
    f.render_widget(
        Paragraph::new("Model").style(model_header_style),
        model_area,
    );
    let tools_header = if tools_area.width < 24 {
        "Tools"
    } else {
        "Tools (space toggle, a toggle all)"
    };
    f.render_widget(
        Paragraph::new(tools_header).style(tools_header_style),
        tools_area,
    );

    let agents_search_area = Rect::new(agents_area.x, agents_area.y + 1, agents_area.width, 1);
    let search_placeholder = if agents_search_area.width < 18 {
        "Search..."
    } else {
        "Search agents..."
    };
    if state.selector.filter.is_empty() {
        let first_char = search_placeholder.chars().next().unwrap_or('S');
        let rest = &search_placeholder[first_char.len_utf8()..];
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled(
                    first_char.to_string(),
                    Style::default().fg(theme::ACCENT_WARNING),
                ),
                Span::styled(rest, Style::default().fg(theme::TEXT_DIM)),
            ])),
            agents_search_area,
        );
    } else {
        let display_filter =
            truncate_with_ellipsis(&state.selector.filter, agents_search_area.width as usize);
        f.render_widget(
            Paragraph::new(display_filter).style(Style::default().fg(theme::ACCENT_WARNING)),
            agents_search_area,
        );
    }

    let agents_list_area = Rect::new(
        agents_area.x,
        agents_area.y + 2,
        agents_area.width,
        agents_area.height.saturating_sub(2),
    );
    let visible_agents = visible_items_in_content_area(agents_list_area).max(1);
    state.selector.adjust_scroll(visible_agents);

    if agents.is_empty() {
        f.render_widget(
            Paragraph::new("No matching agents").style(Style::default().fg(theme::TEXT_MUTED)),
            agents_list_area,
        );
    } else {
        let row_padding = if agents_list_area.width > 9 { 1 } else { 0 };
        let agents_rows_area = Rect::new(
            agents_list_area.x + row_padding,
            agents_list_area.y,
            agents_list_area.width.saturating_sub(row_padding * 2),
            agents_list_area.height,
        );

        for (row, (idx, agent)) in agents
            .iter()
            .enumerate()
            .skip(state.selector.scroll_offset)
            .take(visible_agents)
            .enumerate()
        {
            if row as u16 >= agents_rows_area.height {
                break;
            }

            let row_area = Rect::new(
                agents_rows_area.x,
                agents_rows_area.y + row as u16,
                agents_rows_area.width,
                1,
            );

            let is_selected = idx == state.selector.index;
            if is_selected {
                f.render_widget(
                    Block::default().style(Style::default().bg(theme::ACCENT_WARNING)),
                    row_area,
                );
            }

            let content_width = row_area.width as usize;
            if content_width == 0 {
                continue;
            }

            let name_style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(theme::ACCENT_WARNING)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT_PRIMARY)
            };
            let pm_prefix = if agent.is_pm { "[PM] " } else { "" };
            let available_name_width = content_width.saturating_sub(pm_prefix.chars().count());
            let name_text = truncate_with_ellipsis(&agent.name, available_name_width.max(1));
            let pm_style = if is_selected {
                Style::default()
                    .fg(Color::Black)
                    .bg(theme::ACCENT_WARNING)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
                    .fg(theme::ACCENT_WARNING)
                    .add_modifier(Modifier::BOLD)
            };

            let line = Line::from(vec![
                Span::styled(pm_prefix.to_string(), pm_style),
                Span::styled(name_text, name_style),
            ]);
            f.render_widget(Paragraph::new(line), row_area);
        }
    }

    let model_list_area = Rect::new(
        model_area.x,
        model_area.y + 1,
        model_area.width,
        model_area.height.saturating_sub(1),
    );
    if let Some(settings) = state.settings.as_ref() {
        if settings.available_models.is_empty() {
            f.render_widget(
                Paragraph::new("No models available").style(Style::default().fg(theme::TEXT_MUTED)),
                model_list_area,
            );
        } else {
            for (i, model) in settings.available_models.iter().enumerate() {
                if i as u16 >= model_list_area.height {
                    break;
                }
                let is_selected = i == settings.model_index;
                let prefix = if is_selected { "● " } else { "○ " };
                let style = if is_selected && state.focus == AgentConfigFocus::Model {
                    Style::default().fg(theme::ACCENT_PRIMARY)
                } else if is_selected {
                    Style::default().fg(theme::TEXT_PRIMARY)
                } else {
                    Style::default().fg(theme::TEXT_MUTED)
                };
                let row = Rect::new(
                    model_list_area.x,
                    model_list_area.y + i as u16,
                    model_list_area.width,
                    1,
                );
                f.render_widget(
                    Paragraph::new(format!("{}{}", prefix, model)).style(style),
                    row,
                );
            }
        }
    } else {
        f.render_widget(
            Paragraph::new("Select an agent").style(Style::default().fg(theme::TEXT_MUTED)),
            model_list_area,
        );
    }

    let tools_list_area = Rect::new(
        tools_area.x,
        tools_area.y + 1,
        tools_area.width,
        tools_area.height.saturating_sub(1),
    );
    if let Some(settings) = state.settings.as_mut() {
        let visible_height = tools_list_area.height as usize;
        settings.adjust_tools_scroll(visible_height.max(1));

        let mut y_offset: u16 = 0;
        let mut cursor_pos: usize = 0;
        let scroll_offset = settings.tools_scroll;

        for group in &settings.tool_groups {
            if y_offset as usize >= visible_height {
                break;
            }

            let is_cursor_on_group = cursor_pos == settings.tools_cursor;
            let is_single_tool = group.tools.len() == 1;

            if cursor_pos >= scroll_offset {
                if is_single_tool {
                    let tool = &group.tools[0];
                    let is_checked = settings.selected_tools.contains(tool);
                    let prefix = if is_checked { "[x] " } else { "[ ] " };
                    let style = if is_cursor_on_group && state.focus == AgentConfigFocus::Tools {
                        Style::default().fg(theme::ACCENT_PRIMARY)
                    } else if is_checked {
                        Style::default().fg(theme::TEXT_PRIMARY)
                    } else {
                        Style::default().fg(theme::TEXT_MUTED)
                    };
                    let row = Rect::new(
                        tools_list_area.x,
                        tools_list_area.y + y_offset,
                        tools_list_area.width,
                        1,
                    );
                    f.render_widget(
                        Paragraph::new(format!("{}{}", prefix, tool)).style(style),
                        row,
                    );
                } else {
                    let is_fully = group.is_fully_selected(&settings.selected_tools);
                    let is_partial = group.is_partially_selected(&settings.selected_tools);
                    let expand_icon = if group.expanded { "▼ " } else { "▶ " };
                    let check_icon = if is_fully {
                        "[x] "
                    } else if is_partial {
                        "[-] "
                    } else {
                        "[ ] "
                    };
                    let style = if is_cursor_on_group && state.focus == AgentConfigFocus::Tools {
                        Style::default()
                            .fg(theme::ACCENT_PRIMARY)
                            .add_modifier(Modifier::BOLD)
                    } else if is_fully {
                        Style::default()
                            .fg(theme::TEXT_PRIMARY)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default()
                            .fg(theme::TEXT_MUTED)
                            .add_modifier(Modifier::BOLD)
                    };
                    let row = Rect::new(
                        tools_list_area.x,
                        tools_list_area.y + y_offset,
                        tools_list_area.width,
                        1,
                    );
                    f.render_widget(
                        Paragraph::new(format!(
                            "{}{}{} ({})",
                            expand_icon,
                            check_icon,
                            group.name,
                            group.tools.len()
                        ))
                        .style(style),
                        row,
                    );
                }
                y_offset += 1;
            }
            cursor_pos += 1;

            if group.expanded && !is_single_tool {
                for tool in &group.tools {
                    if y_offset as usize >= visible_height {
                        break;
                    }
                    if cursor_pos >= scroll_offset {
                        let is_cursor_on_tool = cursor_pos == settings.tools_cursor;
                        let is_checked = settings.selected_tools.contains(tool);
                        let prefix = if is_checked { "  [x] " } else { "  [ ] " };
                        let style = if is_cursor_on_tool && state.focus == AgentConfigFocus::Tools {
                            Style::default().fg(theme::ACCENT_PRIMARY)
                        } else if is_checked {
                            Style::default().fg(theme::TEXT_PRIMARY)
                        } else {
                            Style::default().fg(theme::TEXT_MUTED)
                        };
                        let display_name = if tool.starts_with("mcp__") {
                            tool.split("__").last().unwrap_or(tool)
                        } else {
                            tool.as_str()
                        };
                        let row = Rect::new(
                            tools_list_area.x,
                            tools_list_area.y + y_offset,
                            tools_list_area.width,
                            1,
                        );
                        f.render_widget(
                            Paragraph::new(format!("{}{}", prefix, display_name)).style(style),
                            row,
                        );
                        y_offset += 1;
                    }
                    cursor_pos += 1;
                }
            }
        }

        // Bottom-row PM toggle
        if cursor_pos >= scroll_offset && (y_offset as usize) < visible_height {
            let is_cursor_on_pm = cursor_pos == settings.tools_cursor;
            let is_checked = settings.is_pm;
            let prefix = if is_checked { "[x] " } else { "[ ] " };
            let style = if is_cursor_on_pm && state.focus == AgentConfigFocus::Tools {
                Style::default()
                    .fg(theme::ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else if is_checked {
                Style::default()
                    .fg(theme::ACCENT_WARNING)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            };
            let row = Rect::new(
                tools_list_area.x,
                tools_list_area.y + y_offset,
                tools_list_area.width,
                1,
            );
            f.render_widget(
                Paragraph::new(format!("{}Set as PM", prefix)).style(style),
                row,
            );
        }
    } else {
        f.render_widget(
            Paragraph::new("Select an agent").style(Style::default().fg(theme::TEXT_MUTED)),
            tools_list_area,
        );
    }

    let hints_area = Rect::new(
        popup_area.x + 1,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(2),
        1,
    );
    let hints_text = if popup_area.width < 90 {
        "←→/tab shift+tab · ↑↓ · space/a · enter save · esc"
    } else {
        "←→/tab/shift+tab switch · ↑↓ navigate · space toggle · a toggle all · enter save · esc cancel"
    };
    let hints = Paragraph::new(hints_text).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

/// Render the in-conversation search bar (floating at top)
fn render_chat_search_bar(f: &mut Frame, app: &App, area: Rect) {
    use ratatui::text::{Line, Span};

    // Position at the top of the messages area, 3 lines tall
    let search_area = Rect::new(
        area.x + 2,
        area.y + 1,
        area.width.saturating_sub(4).min(60),
        3,
    );

    // Background
    let bg_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_ACTIVE))
        .style(Style::default().bg(theme::BG_CARD));
    f.render_widget(bg_block, search_area);

    // Inner area for content
    let inner = Rect::new(
        search_area.x + 1,
        search_area.y + 1,
        search_area.width.saturating_sub(2),
        1,
    );

    // Build search line (per-tab isolated)
    let search_query = app.chat_search_query();
    let (current_match, total_matches) = app
        .chat_search()
        .map(|s| (s.current_match, s.total_matches))
        .unwrap_or((0, 0));

    let query_display = if search_query.is_empty() {
        Span::styled("Type to search...", Style::default().fg(theme::TEXT_MUTED))
    } else {
        Span::styled(
            search_query.clone(),
            Style::default().fg(theme::TEXT_PRIMARY),
        )
    };

    let match_info = if total_matches > 0 {
        format!(" [{}/{}]", current_match + 1, total_matches)
    } else if !search_query.is_empty() {
        " [0 matches]".to_string()
    } else {
        String::new()
    };

    let line = Line::from(vec![
        Span::styled("🔍 ", Style::default().fg(theme::ACCENT_PRIMARY)),
        query_display,
        Span::styled(match_info, Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(
            "  (Enter: next · ↑↓: navigate · Esc: close)",
            Style::default().fg(theme::TEXT_MUTED),
        ),
    ]);

    let search_text = Paragraph::new(line);
    f.render_widget(search_text, inner);
}

// =============================================================================
// TTS TAB LAYOUT
// =============================================================================

/// Render the TTS control tab with shared chrome (tab bar, statusbar)
fn render_tts_tab_layout(f: &mut Frame, app: &mut App, area: Rect) {
    // Layout: Tab bar | Content | Statusbar
    let chunks = Layout::vertical([
        Constraint::Length(layout::TAB_BAR_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(layout::STATUSBAR_HEIGHT),
    ])
    .split(area);

    // Render tab bar
    render_tab_bar(f, app, chunks[0]);

    // Render TTS control content
    render_tts_control(f, app, chunks[1]);

    // Render statusbar
    let (cumulative_runtime_ms, has_active_agents, active_agent_count) =
        app.data_store.borrow_mut().get_statusbar_runtime_ms();
    let audio_playing = app.audio_player.is_playing();
    render_statusbar(
        f,
        chunks[2],
        app.current_notification(),
        cumulative_runtime_ms,
        has_active_agents,
        active_agent_count,
        app.wave_offset(),
        audio_playing,
    );
}

// =============================================================================
// REPORT TAB LAYOUT
// =============================================================================

/// Render the report tab with shared chrome (tab bar, statusbar)
fn render_report_tab_layout(f: &mut Frame, app: &mut App, area: Rect) {
    // Layout: Tab bar | Content | Statusbar
    let chunks = Layout::vertical([
        Constraint::Length(layout::TAB_BAR_HEIGHT),
        Constraint::Min(0),
        Constraint::Length(layout::STATUSBAR_HEIGHT),
    ])
    .split(area);

    // Render tab bar
    render_tab_bar(f, app, chunks[0]);

    // Render report tab content
    render_report_tab(f, app, chunks[1]);

    // Render statusbar
    let (cumulative_runtime_ms, has_active_agents, active_agent_count) =
        app.data_store.borrow_mut().get_statusbar_runtime_ms();
    let audio_playing = app.audio_player.is_playing();
    render_statusbar(
        f,
        chunks[2],
        app.current_notification(),
        cumulative_runtime_ms,
        has_active_agents,
        active_agent_count,
        app.wave_offset(),
        audio_playing,
    );
}
