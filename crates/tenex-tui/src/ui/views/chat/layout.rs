use crate::ui::components::{
    modal_area, render_chat_sidebar, render_modal_background, render_modal_header,
    render_modal_items, render_modal_overlay, render_modal_search, render_tab_bar,
    ConversationMetadata, ModalItem, ModalSize,
};
use crate::ui::format::truncate_with_ellipsis;
use crate::ui::modal::ChatActionsState;
use crate::ui::theme;
use crate::ui::todo::aggregate_todo_state;
use crate::ui::text_editor::TextEditor;
use crate::ui::{App, ModalState};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::rc::Rc;
use tracing::info_span;

use super::{actions, input, messages};

pub fn render_chat(f: &mut Frame, app: &mut App, area: Rect) {
    // Fill entire area with app background (pure black)
    let bg_block = Block::default().style(Style::default().bg(theme::BG_APP));
    f.render_widget(bg_block, area);

    let all_messages = app.messages();
    let _render_span = info_span!("render_chat", message_count = all_messages.len()).entered();

    // Aggregate todo state from all messages
    let todo_state = aggregate_todo_state(&all_messages);

    // Build conversation metadata from selected thread
    let metadata = app
        .selected_thread
        .as_ref()
        .map(|thread| ConversationMetadata {
            title: Some(thread.title.clone()),
            status_label: thread.status_label.clone(),
            status_current_activity: thread.status_current_activity.clone(),
        })
        .unwrap_or_default();

    // Auto-open ask modal for first unanswered question (only when no modal is active)
    if matches!(app.modal_state, ModalState::None) {
        if let Some((msg_id, ask_event)) = app.has_unanswered_ask_event() {
            app.open_ask_modal(msg_id, ask_event);
        }
    }

    let input_height = input::input_height(app);
    let has_attachments = input::has_attachments(app);
    let has_status = input::has_status(app);
    let has_tabs = !app.open_tabs.is_empty();

    // Build layout based on whether we have attachments, status, and tabs
    // Context line is now INSIDE the input card, not separate
    let chunks = build_layout(area, input_height, has_attachments, has_status, has_tabs);

    // Split messages area horizontally - always show sidebar when visible
    let (messages_area_raw, sidebar_area) =
        split_messages_area(chunks[0], app.todo_sidebar_visible);

    // Add horizontal padding to messages area for breathing room
    let messages_area = with_horizontal_padding(messages_area_raw, 3);

    messages::render_messages_panel(f, app, messages_area, &all_messages);

    // Render chat sidebar (todos + metadata)
    if let Some(sidebar) = sidebar_area {
        render_chat_sidebar(f, &todo_state, &metadata, sidebar);
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
    }

    // Render agent selector popup if showing
    if matches!(app.modal_state, ModalState::AgentSelector { .. }) {
        render_agent_selector(f, app, area);
    }

    // Render branch selector popup if showing
    if matches!(app.modal_state, ModalState::BranchSelector { .. }) {
        render_branch_selector(f, app, area);
    }

    // Render attachment modal if showing
    if app.is_attachment_modal_open() {
        render_attachment_modal(f, app, area);
    }

    // Render expanded editor modal if showing (Ctrl+E)
    if let ModalState::ExpandedEditor { ref editor } = app.modal_state {
        render_expanded_editor_modal(f, editor, area);
    }

    // Render tab modal if showing (Alt+/)
    if app.showing_tab_modal {
        super::super::home::render_tab_modal(f, app, area);
    }

    // Render message actions modal if showing
    if let ModalState::MessageActions {
        selected_index,
        has_trace,
        ..
    } = &app.modal_state
    {
        actions::render_message_actions_modal(f, *selected_index, *has_trace, area);
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

    // Render nudge selector modal if showing
    if let ModalState::NudgeSelector(ref state) = app.modal_state {
        super::super::render_nudge_selector(f, app, area, state);
    }

    // Render chat actions modal if showing (Ctrl+T /)
    if let ModalState::ChatActions(ref state) = app.modal_state {
        render_chat_actions_modal(f, area, state);
    }
}

fn build_layout(
    area: Rect,
    input_height: u16,
    has_attachments: bool,
    has_status: bool,
    has_tabs: bool,
) -> Rc<[Rect]> {
    match (has_attachments, has_status, has_tabs) {
        (true, true, true) => Layout::vertical([
            Constraint::Min(0),            // Messages
            Constraint::Length(1),         // Status line
            Constraint::Length(1),         // Attachments line
            Constraint::Length(input_height), // Input (includes context)
            Constraint::Length(1),         // Tab bar
        ])
        .split(area),
        (true, true, false) => Layout::vertical([
            Constraint::Min(0),            // Messages
            Constraint::Length(1),         // Status line
            Constraint::Length(1),         // Attachments line
            Constraint::Length(input_height), // Input (includes context)
        ])
        .split(area),
        (true, false, true) => Layout::vertical([
            Constraint::Min(0),            // Messages
            Constraint::Length(1),         // Attachments line
            Constraint::Length(input_height), // Input (includes context)
            Constraint::Length(1),         // Tab bar
        ])
        .split(area),
        (true, false, false) => Layout::vertical([
            Constraint::Min(0),            // Messages
            Constraint::Length(1),         // Attachments line
            Constraint::Length(input_height), // Input (includes context)
        ])
        .split(area),
        (false, true, true) => Layout::vertical([
            Constraint::Min(0),            // Messages
            Constraint::Length(1),         // Status line
            Constraint::Length(input_height), // Input (includes context)
            Constraint::Length(1),         // Tab bar
        ])
        .split(area),
        (false, true, false) => Layout::vertical([
            Constraint::Min(0),            // Messages
            Constraint::Length(1),         // Status line
            Constraint::Length(input_height), // Input (includes context)
        ])
        .split(area),
        (false, false, true) => Layout::vertical([
            Constraint::Min(0),            // Messages
            Constraint::Length(input_height), // Input (includes context)
            Constraint::Length(1),         // Tab bar
        ])
        .split(area),
        (false, false, false) => Layout::vertical([
            Constraint::Min(0),            // Messages
            Constraint::Length(input_height), // Input (includes context)
        ])
        .split(area),
    }
}

fn split_messages_area(messages_area: Rect, show_sidebar: bool) -> (Rect, Option<Rect>) {
    if show_sidebar {
        let horiz = Layout::horizontal([Constraint::Min(40), Constraint::Length(30)])
            .split(messages_area);
        (horiz[0], Some(horiz[1]))
    } else {
        (messages_area, None)
    }
}

fn with_horizontal_padding(area: Rect, padding: u16) -> Rect {
    Rect {
        x: area.x + padding,
        y: area.y,
        width: area.width.saturating_sub(padding * 2),
        height: area.height,
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
    let title = if let Some(attachment) = app.chat_editor.get_focused_attachment() {
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
    let title = format!("Expanded Editor ({} lines, {} chars)", line_count, char_count);

    // Use consistent modal sizing (large modal for editing)
    let size = ModalSize {
        max_width: (area.width as f32 * 0.85) as u16,
        height_percent: 0.8,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    // Add vertical padding
    let inner_area = Rect::new(
        popup_area.x,
        popup_area.y + 1,
        popup_area.width,
        popup_area.height.saturating_sub(3),
    );

    // Render header with title and hint
    let remaining = render_modal_header(f, inner_area, &title, "esc");

    // Content area for the text editor
    let content_area = Rect::new(
        remaining.x + 2,
        remaining.y,
        remaining.width.saturating_sub(4),
        remaining.height.saturating_sub(2),
    );

    // Render the text content
    let text = Paragraph::new(editor.text.as_str())
        .style(Style::default().fg(theme::TEXT_PRIMARY))
        .wrap(Wrap { trim: false });
    f.render_widget(text, content_area);

    // Render hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("ctrl+s save · esc cancel")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);

    // Show cursor in the modal (offset by content area position)
    let (cursor_row, cursor_col) = editor.cursor_position();
    f.set_cursor_position((
        content_area.x + cursor_col as u16,
        content_area.y + cursor_row as u16,
    ));
}

fn render_agent_selector(f: &mut Frame, app: &App, area: Rect) {
    let agents = app.filtered_agents();
    let all_agents = app.available_agents();
    let selector_index = app.agent_selector_index();
    let selector_filter = app.agent_selector_filter();

    // Calculate dynamic height based on content
    let item_count = agents.len().max(1);
    let content_height = (item_count as u16 + 6).min(20); // +6 for header, search, hints
    let height_percent = (content_height as f32 / area.height as f32).min(0.6);

    let size = ModalSize {
        max_width: 55,
        height_percent,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    // Add vertical padding
    let inner_area = Rect::new(
        popup_area.x,
        popup_area.y + 1,
        popup_area.width,
        popup_area.height.saturating_sub(3),
    );

    // Render header
    let remaining = render_modal_header(f, inner_area, "Select Agent", "esc");

    // Render search
    let remaining = render_modal_search(f, remaining, selector_filter, "Search agents...");

    // Build items
    let items: Vec<ModalItem> = if agents.is_empty() {
        let msg = if all_agents.is_empty() {
            "No agents available"
        } else {
            "No matching agents"
        };
        vec![ModalItem::new(msg)]
    } else {
        agents
            .iter()
            .enumerate()
            .map(|(i, agent)| {
                let model_info = agent
                    .model
                    .as_ref()
                    .map(|m| m.to_string())
                    .unwrap_or_else(|| "unknown".to_string());

                ModalItem::new(&agent.name)
                    .with_shortcut(model_info)
                    .selected(i == selector_index)
            })
            .collect()
    };

    render_modal_items(f, remaining, &items);

    // Render hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("↑↓ navigate · enter select · esc cancel")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

fn render_branch_selector(f: &mut Frame, app: &App, area: Rect) {
    let branches = app.filtered_branches();
    let all_branches = app.available_branches();
    let selector_index = app.branch_selector_index();
    let selector_filter = app.branch_selector_filter();

    // Calculate dynamic height based on content
    let item_count = branches.len().max(1);
    let content_height = (item_count as u16 + 6).min(20);
    let height_percent = (content_height as f32 / area.height as f32).min(0.6);

    let size = ModalSize {
        max_width: 55,
        height_percent,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    let inner_area = Rect::new(
        popup_area.x,
        popup_area.y + 1,
        popup_area.width,
        popup_area.height.saturating_sub(3),
    );

    let remaining = render_modal_header(f, inner_area, "Select Branch", "esc");
    let remaining = render_modal_search(f, remaining, selector_filter, "Search branches...");

    let items: Vec<ModalItem> = if branches.is_empty() {
        let msg = if all_branches.is_empty() {
            "No branches available"
        } else {
            "No matching branches"
        };
        vec![ModalItem::new(msg)]
    } else {
        branches
            .iter()
            .enumerate()
            .map(|(i, branch)| ModalItem::new(branch).selected(i == selector_index))
            .collect()
    };

    render_modal_items(f, remaining, &items);

    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("↑↓ navigate · enter select · esc cancel")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);

    // Render ask modal overlay if open
    if let Some(modal_state) = app.ask_modal_state() {
        use crate::ui::views::render_ask_modal;

        // Create centered modal area (90% width, 85% height)
        let modal_width = (area.width * 90) / 100;
        let modal_height = (area.height * 85) / 100;
        let modal_x = area.x + (area.width.saturating_sub(modal_width)) / 2;
        let modal_y = area.y + (area.height.saturating_sub(modal_height)) / 2;
        let modal_area = Rect::new(modal_x, modal_y, modal_width, modal_height);

        render_ask_modal(f, modal_state, modal_area);
    }
}

/// Render the chat actions modal (Ctrl+T /)
fn render_chat_actions_modal(f: &mut Frame, area: Rect, state: &ChatActionsState) {
    render_modal_overlay(f, area);

    let actions = state.available_actions();
    let content_height = (actions.len() + 2) as u16;
    let total_height = content_height + 4;
    let height_percent = (total_height as f32 / area.height as f32).min(0.5);

    let size = ModalSize {
        max_width: 45,
        height_percent,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    let inner_area = Rect::new(
        popup_area.x,
        popup_area.y + 1,
        popup_area.width,
        popup_area.height.saturating_sub(2),
    );

    // Truncate title if too long
    let title = truncate_with_ellipsis(&state.thread_title, 35);
    let remaining = render_modal_header(f, inner_area, &title, "esc");

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

    render_modal_items(f, remaining, &items);

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
