use crate::ui::components::{
    render_chat_sidebar, render_modal_items, render_tab_bar, ConversationMetadata, Modal,
    ModalItem, ModalSize,
};
use crate::ui::format::truncate_with_ellipsis;
use crate::ui::layout;
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

use super::{actions, input, messages};

pub fn render_chat(f: &mut Frame, app: &mut App, area: Rect) {
    // Fill entire area with app background (pure black)
    let bg_block = Block::default().style(Style::default().bg(theme::BG_APP));
    f.render_widget(bg_block, area);

    let all_messages = app.messages();

    // Aggregate todo state from all messages
    let todo_state = aggregate_todo_state(&all_messages);

    // Calculate total LLM runtime across all messages in the conversation
    let total_llm_runtime_ms: u64 = all_messages
        .iter()
        .filter_map(|msg| {
            msg.llm_metadata
                .iter()
                .find(|(key, _)| key == "runtime")
                .and_then(|(_, value)| value.parse::<u64>().ok())
        })
        .sum();

    // Build conversation metadata from selected thread, including work status
    let metadata = app
        .selected_thread
        .as_ref()
        .map(|thread| {
            // Get working agent names from 24133 events
            let working_agents = {
                let store = app.data_store.borrow();
                let agent_pubkeys = store.get_working_agents(&thread.id);

                // Get project a_tag to look up agent names from project status
                let project_a_tag = store.find_project_for_thread(&thread.id);

                // Resolve pubkeys to agent names via project status
                agent_pubkeys
                    .iter()
                    .map(|pubkey| {
                        // Look up agent name from project status
                        project_a_tag.as_ref()
                            .and_then(|a_tag| store.get_project_status(a_tag))
                            .and_then(|status| {
                                status.agents.iter()
                                    .find(|a| a.pubkey == *pubkey)
                                    .map(|a| a.name.clone())
                            })
                            .unwrap_or_else(|| {
                                // Fallback to short pubkey if agent not found
                                format!("{}...", &pubkey[..8.min(pubkey.len())])
                            })
                    })
                    .collect()
            };
            ConversationMetadata {
                title: Some(thread.title.clone()),
                status_label: thread.status_label.clone(),
                status_current_activity: thread.status_current_activity.clone(),
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
    let chunks = build_layout(area, input_height, has_attachments, has_status, has_tabs);

    // Split messages area horizontally - always show sidebar when visible
    let (messages_area_raw, sidebar_area) =
        split_messages_area(chunks[0], app.todo_sidebar_visible);

    // Add horizontal padding to messages area for breathing room (consistent with Home)
    let messages_area = layout::with_content_padding(messages_area_raw);

    messages::render_messages_panel(f, app, messages_area, &all_messages);

    // Render chat sidebar (work indicator + todos + metadata)
    if let Some(sidebar) = sidebar_area {
        render_chat_sidebar(f, &todo_state, &metadata, app.spinner_char(), sidebar);
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

    // Render nudge selector modal if showing
    if let ModalState::NudgeSelector(ref state) = app.modal_state {
        super::super::render_nudge_selector(f, app, area, state);
    }

    // Render chat actions modal if showing (Ctrl+T /)
    if let ModalState::ChatActions(ref state) = app.modal_state {
        render_chat_actions_modal(f, area, state);
    }

    // Render agent settings modal if showing
    if let ModalState::AgentSettings(ref state) = app.modal_state {
        render_agent_settings_modal(f, area, state);
    }

    // Render draft navigator modal if showing
    if let ModalState::DraftNavigator(ref state) = app.modal_state {
        super::super::render_draft_navigator(f, area, state);
    }

    // Command palette overlay (Ctrl+T)
    if let ModalState::CommandPalette(ref state) = app.modal_state {
        super::super::render_command_palette(f, area, state);
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
    // Tab bar now uses 2 lines (title + project)
    match (has_attachments, has_status, has_tabs) {
        (true, true, true) => Layout::vertical([
            Constraint::Min(0),            // Messages
            Constraint::Length(1),         // Status line
            Constraint::Length(1),         // Attachments line
            Constraint::Length(input_height), // Input (includes context)
            Constraint::Length(2),         // Tab bar (2 lines)
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
            Constraint::Length(2),         // Tab bar (2 lines)
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
            Constraint::Length(2),         // Tab bar (2 lines)
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
            Constraint::Length(2),         // Tab bar (2 lines)
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
    let hints = Paragraph::new("ctrl+s save · esc cancel")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);

    // Show cursor in the modal (offset by content area position)
    let (cursor_row, cursor_col) = editor.cursor_position();
    f.set_cursor_position((
        editor_area.x + cursor_col as u16,
        editor_area.y + cursor_row as u16,
    ));
}

fn render_agent_selector(f: &mut Frame, app: &App, area: Rect) {
    let agents = app.filtered_agents();
    let all_agents = app.available_agents();
    let selector_index = app.agent_selector_index();
    let selector_filter = app.agent_selector_filter();

    // Calculate dynamic height based on content
    // +7 accounts for: header (2) + search (2) + vertical padding (3)
    let item_count = agents.len().max(1);
    let content_height = (item_count as u16 + 7).min(20);
    let height_percent = (content_height as f32 / area.height as f32).min(0.6);

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

    let popup_area = Modal::new("Select Agent")
        .size(ModalSize {
            max_width: 55,
            height_percent,
        })
        .search(selector_filter, "Search agents...")
        .render(f, area, |f, content_area| {
            render_modal_items(f, content_area, &items);
        });

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
    // +7 accounts for: header (2) + search (2) + vertical padding (3)
    let item_count = branches.len().max(1);
    let content_height = (item_count as u16 + 7).min(20);
    let height_percent = (content_height as f32 / area.height as f32).min(0.6);

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

    let popup_area = Modal::new("Select Branch")
        .size(ModalSize {
            max_width: 55,
            height_percent,
        })
        .search(selector_filter, "Search branches...")
        .render(f, area, |f, content_area| {
            render_modal_items(f, content_area, &items);
        });

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

/// Render the agent settings modal
fn render_agent_settings_modal(
    f: &mut Frame,
    area: Rect,
    state: &crate::ui::modal::AgentSettingsState,
) {
    use crate::ui::modal::AgentSettingsFocus;
    use ratatui::style::Modifier;

    // Calculate size based on content
    let model_count = state.available_models.len();
    let tool_item_count = state.visible_item_count();
    let content_height = (model_count.max(tool_item_count) + 6) as u16;
    let total_height = content_height.min(30);
    let height_percent = (total_height as f32 / area.height as f32).min(0.8);

    let title = format!("{} Settings", state.agent_name);
    let (popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 80,
            height_percent,
        })
        .render_frame(f, area);

    // Two-column layout: Model on left, Tools on right
    let content_width = content_area.width.saturating_sub(4);
    let left_width = content_width / 3;
    let right_width = content_width - left_width - 1; // -1 for separator

    let left_area = Rect::new(content_area.x + 2, content_area.y, left_width, content_area.height.saturating_sub(2));
    let right_area = Rect::new(content_area.x + 3 + left_width, content_area.y, right_width, content_area.height.saturating_sub(2));

    // Render Model section
    let model_header_style = if state.focus == AgentSettingsFocus::Model {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    let model_header = Paragraph::new("Model").style(model_header_style);
    f.render_widget(model_header, left_area);

    let model_list_area = Rect::new(left_area.x, left_area.y + 1, left_area.width, left_area.height.saturating_sub(1));
    if state.available_models.is_empty() {
        let no_models = Paragraph::new("No models available")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(no_models, model_list_area);
    } else {
        for (i, model) in state.available_models.iter().enumerate() {
            if i as u16 >= model_list_area.height {
                break;
            }
            let is_selected = i == state.model_index;
            let prefix = if is_selected { "● " } else { "○ " };
            let style = if is_selected && state.focus == AgentSettingsFocus::Model {
                Style::default().fg(theme::ACCENT_PRIMARY)
            } else if is_selected {
                Style::default().fg(theme::TEXT_PRIMARY)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            };
            let model_text = Paragraph::new(format!("{}{}", prefix, model)).style(style);
            let item_area = Rect::new(model_list_area.x, model_list_area.y + i as u16, model_list_area.width, 1);
            f.render_widget(model_text, item_area);
        }
    }

    // Render Tools section header
    let tools_header_style = if state.focus == AgentSettingsFocus::Tools {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    let tools_header = Paragraph::new("Tools (space toggle, a toggle all)")
        .style(tools_header_style);
    f.render_widget(tools_header, right_area);

    // Render tool groups
    let tools_list_area = Rect::new(right_area.x, right_area.y + 1, right_area.width, right_area.height.saturating_sub(1));

    if state.tool_groups.is_empty() {
        let no_tools = Paragraph::new("No tools available")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(no_tools, tools_list_area);
    } else {
        let mut y_offset: u16 = 0;
        let mut cursor_pos: usize = 0;
        let visible_height = tools_list_area.height as usize;

        for group in &state.tool_groups {
            if y_offset as usize >= visible_height {
                break;
            }

            let is_cursor_on_group = cursor_pos == state.tools_cursor;
            let is_single_tool = group.tools.len() == 1;

            // Group header or single tool
            if is_single_tool {
                // Single tool - show as checkbox
                let tool = &group.tools[0];
                let is_checked = state.selected_tools.contains(tool);
                let prefix = if is_checked { "[x] " } else { "[ ] " };
                let style = if is_cursor_on_group && state.focus == AgentSettingsFocus::Tools {
                    Style::default().fg(theme::ACCENT_PRIMARY)
                } else if is_checked {
                    Style::default().fg(theme::TEXT_PRIMARY)
                } else {
                    Style::default().fg(theme::TEXT_MUTED)
                };
                let text = Paragraph::new(format!("{}{}", prefix, tool)).style(style);
                let item_area = Rect::new(tools_list_area.x, tools_list_area.y + y_offset, tools_list_area.width, 1);
                f.render_widget(text, item_area);
            } else {
                // Multi-tool group - show as expandable
                let is_fully = group.is_fully_selected(&state.selected_tools);
                let is_partial = group.is_partially_selected(&state.selected_tools);
                let expand_icon = if group.expanded { "▼ " } else { "▶ " };
                let check_icon = if is_fully { "[x] " } else if is_partial { "[-] " } else { "[ ] " };

                let style = if is_cursor_on_group && state.focus == AgentSettingsFocus::Tools {
                    Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
                } else if is_fully {
                    Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::BOLD)
                };
                let text = Paragraph::new(format!("{}{}{} ({})", expand_icon, check_icon, group.name, group.tools.len())).style(style);
                let item_area = Rect::new(tools_list_area.x, tools_list_area.y + y_offset, tools_list_area.width, 1);
                f.render_widget(text, item_area);
            }

            y_offset += 1;
            cursor_pos += 1;

            // Render expanded tools
            if group.expanded && !is_single_tool {
                for tool in &group.tools {
                    if y_offset as usize >= visible_height {
                        break;
                    }

                    let is_cursor_on_tool = cursor_pos == state.tools_cursor;
                    let is_checked = state.selected_tools.contains(tool);
                    let prefix = if is_checked { "  [x] " } else { "  [ ] " };

                    let style = if is_cursor_on_tool && state.focus == AgentSettingsFocus::Tools {
                        Style::default().fg(theme::ACCENT_PRIMARY)
                    } else if is_checked {
                        Style::default().fg(theme::TEXT_PRIMARY)
                    } else {
                        Style::default().fg(theme::TEXT_MUTED)
                    };

                    // Show just the method name for MCP tools
                    let display_name = if tool.starts_with("mcp__") {
                        tool.split("__").last().unwrap_or(tool)
                    } else {
                        tool.as_str()
                    };

                    let text = Paragraph::new(format!("{}{}", prefix, display_name)).style(style);
                    let item_area = Rect::new(tools_list_area.x, tools_list_area.y + y_offset, tools_list_area.width, 1);
                    f.render_widget(text, item_area);

                    y_offset += 1;
                    cursor_pos += 1;
                }
            }
        }
    }

    // Render hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("tab switch · ↑↓ navigate · space toggle · a toggle all · enter save · esc cancel")
        .style(Style::default().fg(theme::TEXT_MUTED));
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
    let (current_match, total_matches) = app.chat_search()
        .map(|s| (s.current_match, s.total_matches))
        .unwrap_or((0, 0));

    let query_display = if search_query.is_empty() {
        Span::styled("Type to search...", Style::default().fg(theme::TEXT_MUTED))
    } else {
        Span::styled(search_query.clone(), Style::default().fg(theme::TEXT_PRIMARY))
    };

    let match_info = if total_matches > 0 {
        format!(
            " [{}/{}]",
            current_match + 1,
            total_matches
        )
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
