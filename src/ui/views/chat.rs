use crate::models::Message;
use crate::ui::markdown::render_markdown;
use crate::ui::tool_calls::{parse_message_content, render_tool_calls_group, MessageContent};
use crate::ui::{App, InputMode};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};
use std::collections::HashMap;
use tracing::info_span;

/// Get any streaming content for the current thread
fn get_streaming_content(app: &App) -> Option<(String, String)> {
    let thread = app.selected_thread.as_ref()?;

    // Check if there's streaming content for this thread
    // Streaming deltas reference the message they're responding to,
    // which could be the thread root or any message in the thread
    for msg_id in app.streaming_accumulator.pending_messages() {
        // Check if this message_id is the thread root
        if msg_id == thread.id {
            if let Some(content) = app.streaming_accumulator.get_content(msg_id) {
                let agent_name = app
                    .selected_agent
                    .as_ref()
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| "Agent".to_string());
                return Some((agent_name, content));
            }
        }

        // Check if this message_id is any message in the current thread
        let messages = app.messages();
        for msg in &messages {
            if msg.id == msg_id {
                if let Some(content) = app.streaming_accumulator.get_content(msg_id) {
                    let agent_name = app
                        .selected_agent
                        .as_ref()
                        .map(|a| a.name.clone())
                        .unwrap_or_else(|| "Agent".to_string());
                    return Some((agent_name, content));
                }
            }
        }
    }

    None
}

pub fn render_chat(f: &mut Frame, app: &App, area: Rect) {
    let all_messages = app.messages();
    let _render_span = info_span!("render_chat", message_count = all_messages.len()).entered();

    // Calculate dynamic input height based on line count (min 3, max 10)
    let input_lines = app.chat_editor.line_count().max(1);
    let input_height = (input_lines as u16 + 2).clamp(3, 10); // +2 for borders

    // Check if we have attachments (paste or image)
    let has_attachments = !app.chat_editor.attachments.is_empty() || !app.chat_editor.image_attachments.is_empty();
    let has_status = app.status_message.is_some();

    // Build layout based on whether we have attachments and/or status
    let chunks = match (has_attachments, has_status) {
        (true, true) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Status line
            Constraint::Length(1),     // Context line
            Constraint::Length(1),     // Attachments line
            Constraint::Length(input_height), // Input
        ]).split(area),
        (true, false) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Context line
            Constraint::Length(1),     // Attachments line
            Constraint::Length(input_height), // Input
        ]).split(area),
        (false, true) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Status line
            Constraint::Length(1),     // Context line
            Constraint::Length(input_height), // Input
        ]).split(area),
        (false, false) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Context line
            Constraint::Length(input_height), // Input
        ]).split(area),
    };

    // Get thread_id first - needed for reply index filtering
    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());

    // Build reply index: parent_id -> Vec<&Message>
    // Skip messages that e-tag the thread root - those are siblings, not nested replies
    let mut replies_by_parent: HashMap<&str, Vec<&Message>> = HashMap::new();
    for msg in &all_messages {
        if let Some(ref parent_id) = msg.reply_to {
            // Only count as a reply if parent is NOT the thread root
            if Some(parent_id.as_str()) != thread_id {
                replies_by_parent.entry(parent_id.as_str()).or_default().push(msg);
            }
        }
    }
    let display_messages: Vec<&Message> = if let Some(ref root_id) = app.subthread_root {
        // Subthread view: show messages that reply directly to the root
        all_messages.iter()
            .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
            .collect()
    } else {
        // Main view: show messages with no parent or parent = thread root
        all_messages.iter()
            .filter(|m| {
                m.reply_to.is_none() || m.reply_to.as_deref() == thread_id
            })
            .collect()
    };

    // Build title with thread name and selected agent
    let thread_title = app
        .selected_thread
        .as_ref()
        .map(|t| t.title.clone())
        .unwrap_or_else(|| "Chat".to_string());

    let agent_name = app
        .selected_agent
        .as_ref()
        .map(|a| a.name.clone())
        .unwrap_or_else(|| "No agent".to_string());

    let title = if app.in_subthread() {
        format!("{} (subthread) | @{} (Esc to go back)", thread_title, agent_name)
    } else {
        format!("{} | @{} (@ to change, Esc to go back)", thread_title, agent_name)
    };

    // Messages area
    let mut messages_text: Vec<Line> = Vec::new();

    // If in subthread, render the root message first as a header
    if let Some(ref root_msg) = app.subthread_root_message {
        let author = app.data_store.borrow().get_profile_name(&root_msg.pubkey);
        messages_text.push(Line::from(Span::styled(
            format!("{} :", author),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));

        // Render root message content (truncated)
        let content_preview: String = root_msg.content.chars().take(200).collect();
        let markdown_lines = render_markdown(&content_preview);
        messages_text.extend(markdown_lines);
        if root_msg.content.len() > 200 {
            messages_text.push(Line::from(Span::styled("...", Style::default().fg(Color::DarkGray))));
        }

        // Separator
        messages_text.push(Line::from(Span::styled(
            "────────────────────────────────────────",
            Style::default().fg(Color::DarkGray),
        )));
        messages_text.push(Line::from(""));
    }

    // Render display messages
    {
        let _span = info_span!("render_messages").entered();
        for (i, msg) in display_messages.iter().enumerate() {
            let _msg_span = info_span!("render_message", index = i).entered();

            // Check if this message is selected (for navigation)
            let is_selected = i == app.selected_message_index && app.input_mode == InputMode::Normal;

            let author = {
                let _span = info_span!("get_profile_name").entered();
                app.data_store.borrow().get_profile_name(&msg.pubkey)
            };

            // Reasoning messages get muted italic style
            if msg.is_reasoning {
                let prefix = if is_selected { "> " } else { "" };
                let header_style = if is_selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::ITALIC)
                } else {
                    Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC)
                };
                messages_text.push(Line::from(Span::styled(
                    format!("{}{} (thinking):", prefix, author),
                    header_style,
                )));

                // Render reasoning content in muted italic style
                let muted_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
                for line in msg.content.lines() {
                    messages_text.push(Line::from(Span::styled(line.to_string(), muted_style)));
                }
            } else {
                // Normal message rendering
                let author_style = if is_selected {
                    Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::Cyan)
                };
                let prefix = if is_selected { "> " } else { "" };
                messages_text.push(Line::from(Span::styled(
                    format!("{}{} :", prefix, author),
                    author_style,
                )));

                let parsed = {
                    let _span = info_span!("parse_message_content").entered();
                    parse_message_content(&msg.content)
                };

                match parsed {
                    MessageContent::PlainText(text) => {
                        let _span = info_span!("render_markdown").entered();
                        let markdown_lines = render_markdown(&text);
                        messages_text.extend(markdown_lines);
                    }
                    MessageContent::Mixed {
                        text_parts,
                        tool_calls,
                    } => {
                        for text_part in text_parts {
                            if !text_part.trim().is_empty() {
                                let markdown_lines = render_markdown(&text_part);
                                messages_text.extend(markdown_lines);
                            }
                        }

                        if !tool_calls.is_empty() {
                            messages_text.push(Line::from(""));
                            let tool_call_lines = render_tool_calls_group(&tool_calls);
                            messages_text.extend(tool_call_lines);
                        }
                    }
                }
            }

            // Check if this message has replies and show preview
            if let Some(replies) = replies_by_parent.get(msg.id.as_str()) {
                if !replies.is_empty() {
                    // Find most recent reply
                    let most_recent = replies.iter().max_by_key(|r| r.created_at);
                    if let Some(recent) = most_recent {
                        let reply_author = app.data_store.borrow().get_profile_name(&recent.pubkey);
                        let preview: String = recent.content.chars().take(60).collect();
                        let preview = preview.replace('\n', " ");
                        let suffix = if recent.content.len() > 60 { "..." } else { "" };

                        messages_text.push(Line::from(vec![
                            Span::styled("  └→ ", Style::default().fg(Color::DarkGray)),
                            Span::styled(
                                format!("{}", replies.len()),
                                Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                            ),
                            Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                            Span::styled(
                                format!("{}: ", reply_author),
                                Style::default().fg(Color::Blue),
                            ),
                            Span::styled(
                                format!("{}{}", preview, suffix),
                                Style::default().fg(Color::DarkGray),
                            ),
                        ]));
                    }
                }
            }

            messages_text.push(Line::from(""));
            messages_text.push(Line::from(""));
        }
    }

    // Check for streaming content (agent typing)
    let streaming_content = {
        let _span = info_span!("get_streaming_content").entered();
        get_streaming_content(app)
    };
    if let Some((agent_name, content)) = streaming_content {
        messages_text.push(Line::from(Span::styled(
            format!("{} (typing...):", agent_name),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::ITALIC),
        )));

        let markdown_lines = render_markdown(&content);
        messages_text.extend(markdown_lines);

        messages_text.push(Line::from(""));
    }

    if messages_text.is_empty() {
        let empty = Paragraph::new("No messages yet. Press 'i' to start typing.")
            .style(Style::default().fg(Color::DarkGray))
            .block(Block::default().borders(Borders::ALL).title(title.clone()));
        f.render_widget(empty, chunks[0]);
    } else {
        let visible_height = chunks[0].height.saturating_sub(2) as usize; // -2 for borders
        let content_width = chunks[0].width.saturating_sub(2) as usize; // -2 for borders

        // Calculate actual line count after wrapping
        let total_lines: usize = messages_text
            .iter()
            .map(|line| {
                let line_len: usize = line.spans.iter().map(|s| s.content.len()).sum();
                if line_len == 0 {
                    1 // Empty lines still take one row
                } else {
                    (line_len + content_width - 1) / content_width.max(1) // Ceiling division
                }
            })
            .sum();

        let max_scroll = total_lines.saturating_sub(visible_height);

        // Use scroll_offset, clamped to max
        let scroll = app.scroll_offset.min(max_scroll);

        let messages = Paragraph::new(messages_text)
            .block(Block::default().borders(Borders::ALL).title(title.clone()))
            .wrap(Wrap { trim: false })
            .scroll((scroll as u16, 0));

        f.render_widget(messages, chunks[0]);
    }

    // Calculate chunk indices based on layout
    // Layout: [messages, (status?), context, (attachments?), input]
    let mut idx = 1; // Start after messages

    // Status line (if any)
    if has_status {
        if let Some(ref msg) = app.status_message {
            let status_line = Line::from(vec![
                Span::styled("⏳ ", Style::default().fg(Color::Yellow)),
                Span::styled(msg.as_str(), Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            ]);
            let status = Paragraph::new(status_line);
            f.render_widget(status, chunks[idx]);
        }
        idx += 1;
    }

    // Context line showing selected agent and branch
    let agent_display = app
        .selected_agent
        .as_ref()
        .map(|a| a.name.clone())
        .unwrap_or_else(|| "none".to_string());

    let branch_display = app
        .selected_branch
        .as_ref()
        .map(|b| format!(" on %{}", b))
        .unwrap_or_default();

    let context_line = Line::from(vec![
        Span::styled("→ @", Style::default().fg(Color::DarkGray)),
        Span::styled(agent_display, Style::default().fg(Color::Cyan)),
        Span::styled(branch_display, Style::default().fg(Color::Green)),
    ]);
    let context = Paragraph::new(context_line);
    f.render_widget(context, chunks[idx]);
    idx += 1;

    // Attachments line (if any)
    if has_attachments {
        let mut attachment_spans: Vec<Span> = vec![Span::styled("Attachments: ", Style::default().fg(Color::DarkGray))];
        let img_count = app.chat_editor.image_attachments.len();

        // Show image attachments (focus index 0..img_count)
        for (i, img) in app.chat_editor.image_attachments.iter().enumerate() {
            let is_focused = app.chat_editor.focused_attachment == Some(i);
            let style = if is_focused {
                Style::default().fg(Color::Black).bg(Color::Magenta).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Magenta)
            };
            attachment_spans.push(Span::styled(
                format!("[Image #{}] ", img.id),
                style,
            ));
        }

        // Show paste attachments (focus index img_count..)
        for (i, attachment) in app.chat_editor.attachments.iter().enumerate() {
            let is_focused = app.chat_editor.focused_attachment == Some(img_count + i);
            let style = if is_focused {
                Style::default().fg(Color::Black).bg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::Yellow)
            };
            attachment_spans.push(Span::styled(
                format!("[Paste #{}] ", attachment.id),
                style,
            ));
        }

        // Show hint based on what's focused
        if app.chat_editor.focused_attachment.is_some() {
            attachment_spans.push(Span::styled("(Backspace to delete, ↓ to exit)", Style::default().fg(Color::DarkGray)));
        } else {
            attachment_spans.push(Span::styled("(↑ to select)", Style::default().fg(Color::DarkGray)));
        }
        let attachments_line = Line::from(attachment_spans);
        let attachments = Paragraph::new(attachments_line);
        f.render_widget(attachments, chunks[idx]);
        idx += 1;
    }

    // Input area - use chat_editor instead of app.input
    let input_style = if app.input_mode == InputMode::Editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let input_area = chunks[idx];

    let input = Paragraph::new(app.chat_editor.text.as_str())
        .style(input_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(input_style)
                .title(if app.input_mode == InputMode::Editing {
                    "Enter to send, Ctrl+Enter for newline"
                } else {
                    "Press 'i' to compose"
                }),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(input, input_area);

    // Show cursor in input mode
    if app.input_mode == InputMode::Editing && !app.showing_attachment_modal {
        let (cursor_row, cursor_col) = app.chat_editor.cursor_position();
        f.set_cursor_position((
            input_area.x + cursor_col as u16 + 1,
            input_area.y + cursor_row as u16 + 1,
        ));
    }

    // Render agent selector popup if showing
    if app.showing_agent_selector {
        render_agent_selector(f, app, area);
    }

    // Render branch selector popup if showing
    if app.showing_branch_selector {
        render_branch_selector(f, app, area);
    }

    // Render attachment modal if showing
    if app.showing_attachment_modal {
        render_attachment_modal(f, app, area);
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

    // Render the modal content
    let modal = Paragraph::new(app.attachment_modal_editor.text.as_str())
        .style(Style::default().fg(Color::White))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Yellow))
                .title(title),
        )
        .wrap(Wrap { trim: false });

    f.render_widget(modal, popup_area);

    // Show cursor in the modal
    let (cursor_row, cursor_col) = app.attachment_modal_editor.cursor_position();
    f.set_cursor_position((
        popup_area.x + cursor_col as u16 + 1,
        popup_area.y + cursor_row as u16 + 1,
    ));
}

fn render_agent_selector(f: &mut Frame, app: &App, area: Rect) {
    let agents = app.filtered_agents();
    let all_agents = app.available_agents();

    // Calculate popup size and position (centered)
    let popup_width = 40.min(area.width.saturating_sub(4));
    let item_count = agents.len().max(1);
    let popup_height = (item_count as u16 + 2).min(area.height.saturating_sub(4));
    let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup
    f.render_widget(Clear, popup_area);

    // Build list items
    let items: Vec<ListItem> = if agents.is_empty() {
        let msg = if all_agents.is_empty() {
            "No agents available (project offline?)"
        } else {
            "No matching agents"
        };
        vec![ListItem::new(msg).style(Style::default().fg(Color::DarkGray))]
    } else {
        agents
            .iter()
            .enumerate()
            .map(|(i, agent)| {
                let style = if i == app.agent_selector_index {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                let model_info = agent
                    .model
                    .as_ref()
                    .map(|m| format!(" ({})", m))
                    .unwrap_or_default();

                ListItem::new(format!("{}{}", agent.name, model_info)).style(style)
            })
            .collect()
    };

    let title = if app.selector_filter.is_empty() {
        "Select Agent (type to filter)".to_string()
    } else {
        format!("Select Agent: {}", app.selector_filter)
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title(title),
    );

    f.render_widget(list, popup_area);
}

fn render_branch_selector(f: &mut Frame, app: &App, area: Rect) {
    let branches = app.filtered_branches();
    let all_branches = app.available_branches();

    // Calculate popup size and position (centered)
    let popup_width = 40.min(area.width.saturating_sub(4));
    let item_count = branches.len().max(1);
    let popup_height = (item_count as u16 + 2).min(area.height.saturating_sub(4));
    let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup
    f.render_widget(Clear, popup_area);

    // Build list items
    let items: Vec<ListItem> = if branches.is_empty() {
        let msg = if all_branches.is_empty() {
            "No branches available (project offline?)"
        } else {
            "No matching branches"
        };
        vec![ListItem::new(msg).style(Style::default().fg(Color::DarkGray))]
    } else {
        branches
            .iter()
            .enumerate()
            .map(|(i, branch)| {
                let style = if i == app.branch_selector_index {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Green)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                ListItem::new(branch.clone()).style(style)
            })
            .collect()
    };

    let title = if app.selector_filter.is_empty() {
        "Select Branch (type to filter)".to_string()
    } else {
        format!("Select Branch: {}", app.selector_filter)
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Green))
            .title(title),
    );

    f.render_widget(list, popup_area);
}
