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
                    .map(|a| a.display_name().to_string())
                    .unwrap_or_else(|| "Agent".to_string());
                return Some((agent_name, content));
            }
        }

        // Check if this message_id is any message in the current thread
        for msg in &app.messages {
            if msg.id == msg_id {
                if let Some(content) = app.streaming_accumulator.get_content(msg_id) {
                    let agent_name = app
                        .selected_agent
                        .as_ref()
                        .map(|a| a.display_name().to_string())
                        .unwrap_or_else(|| "Agent".to_string());
                    return Some((agent_name, content));
                }
            }
        }
    }

    None
}

pub fn render_chat(f: &mut Frame, app: &App, area: Rect) {
    let _render_span = info_span!("render_chat", message_count = app.messages.len()).entered();

    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(area);

    // Build title with thread name and selected agent
    let thread_title = app
        .selected_thread
        .as_ref()
        .map(|t| t.title.clone())
        .unwrap_or_else(|| "Chat".to_string());

    let agent_name = app
        .selected_agent
        .as_ref()
        .map(|a| a.display_name().to_string())
        .unwrap_or_else(|| "No agent".to_string());

    let title = format!("{} | @{} (@ to change, Esc to go back)", thread_title, agent_name);

    // Messages area
    let mut messages_text: Vec<Line> = Vec::new();

    // Render regular messages
    {
        let _span = info_span!("render_messages").entered();
        for (i, msg) in app.messages.iter().enumerate() {
            let _msg_span = info_span!("render_message", index = i).entered();

            let author = {
                let _span = info_span!("get_profile_name").entered();
                app.get_profile_name(&msg.pubkey)
            };

            messages_text.push(Line::from(Span::styled(
                format!("{} :", author),
                Style::default().fg(Color::Cyan),
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

    // Input area
    let input_style = if app.input_mode == InputMode::Editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let input = Paragraph::new(app.input.as_str())
        .style(input_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(input_style)
                .title(if app.input_mode == InputMode::Editing {
                    "Type your message (Enter to send, Esc to cancel)"
                } else {
                    "Press 'i' to compose"
                }),
        );
    f.render_widget(input, chunks[1]);

    // Show cursor in input mode
    if app.input_mode == InputMode::Editing {
        f.set_cursor_position((
            chunks[1].x + app.cursor_position as u16 + 1,
            chunks[1].y + 1,
        ));
    }

    // Render agent selector popup if showing
    if app.showing_agent_selector {
        render_agent_selector(f, app, area);
    }
}

fn render_agent_selector(f: &mut Frame, app: &App, area: Rect) {
    let agents = app.available_agents();
    if agents.is_empty() {
        return;
    }

    // Calculate popup size and position (centered)
    let popup_width = 40.min(area.width.saturating_sub(4));
    let popup_height = (agents.len() as u16 + 2).min(area.height.saturating_sub(4));
    let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;

    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the area behind the popup
    f.render_widget(Clear, popup_area);

    // Build list items
    let items: Vec<ListItem> = agents
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
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
            .title("Select Agent (↑↓ Enter Esc)"),
    );

    f.render_widget(list, popup_area);
}
