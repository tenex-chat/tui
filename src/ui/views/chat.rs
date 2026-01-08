use crate::models::Message;
use crate::ui::markdown::render_markdown;
use crate::ui::tool_calls::{parse_message_content, MessageContent, tool_icon, extract_target};
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

/// Streaming content for display
struct StreamingContent {
    agent_name: String,
    content: String,
}

/// Check if a message looks like a short action/status message
fn is_action_message(content: &str) -> bool {
    let trimmed = content.trim();

    // Must be short (under 100 chars) and single line
    if trimmed.len() > 100 || trimmed.contains('\n') {
        return false;
    }

    // Common action patterns
    let action_patterns = [
        "Sending", "Executing", "Delegating", "Running", "Calling",
        "Reading", "Writing", "Editing", "Creating", "Deleting",
        "Searching", "Finding", "Checking", "Validating", "Processing",
        "Fetching", "Loading", "Saving", "Updating", "Installing",
        "Building", "Compiling", "Testing", "Deploying",
        "There is an existing", "Already", "Successfully", "Failed to",
        "Starting", "Finishing", "Completed", "Done",
    ];

    for pattern in action_patterns {
        if trimmed.starts_with(pattern) {
            return true;
        }
    }

    // Also detect messages that look like tool operations (short with specific verbs)
    let words: Vec<&str> = trimmed.split_whitespace().collect();
    if words.len() <= 6 {
        if let Some(first) = words.first() {
            // Check for -ing verbs
            if first.ends_with("ing") && first.len() > 4 {
                return true;
            }
        }
    }

    false
}

/// Grouped display item - either a single message or a collapsed group
enum DisplayItem<'a> {
    SingleMessage(&'a Message),
    ActionGroup {
        messages: Vec<&'a Message>,
        pubkey: String,
    },
}

/// Group consecutive action messages from the same author
fn group_messages<'a>(messages: &[&'a Message], user_pubkey: Option<&str>) -> Vec<DisplayItem<'a>> {
    let mut result = Vec::new();
    let mut current_group: Vec<&'a Message> = Vec::new();
    let mut group_pubkey: Option<String> = None;

    for msg in messages {
        let is_user = user_pubkey.map(|pk| pk == msg.pubkey.as_str()).unwrap_or(false);
        let is_action = !is_user && is_action_message(&msg.content);

        if is_action {
            // Check if we can add to current group
            if let Some(ref pk) = group_pubkey {
                if pk == &msg.pubkey {
                    current_group.push(msg);
                    continue;
                }
            }

            // Flush existing group if any
            if !current_group.is_empty() {
                if current_group.len() == 1 {
                    result.push(DisplayItem::SingleMessage(current_group[0]));
                } else {
                    result.push(DisplayItem::ActionGroup {
                        messages: current_group.clone(),
                        pubkey: group_pubkey.clone().unwrap(),
                    });
                }
                current_group.clear();
            }

            // Start new group
            group_pubkey = Some(msg.pubkey.clone());
            current_group.push(msg);
        } else {
            // Flush any existing group
            if !current_group.is_empty() {
                if current_group.len() == 1 {
                    result.push(DisplayItem::SingleMessage(current_group[0]));
                } else {
                    result.push(DisplayItem::ActionGroup {
                        messages: current_group.clone(),
                        pubkey: group_pubkey.clone().unwrap(),
                    });
                }
                current_group.clear();
                group_pubkey = None;
            }

            result.push(DisplayItem::SingleMessage(msg));
        }
    }

    // Flush final group
    if !current_group.is_empty() {
        if current_group.len() == 1 {
            result.push(DisplayItem::SingleMessage(current_group[0]));
        } else {
            result.push(DisplayItem::ActionGroup {
                messages: current_group,
                pubkey: group_pubkey.unwrap(),
            });
        }
    }

    result
}

/// Get streaming sessions with content for the current thread
fn get_streaming_content(app: &App) -> Vec<StreamingContent> {
    let thread = match app.selected_thread.as_ref() {
        Some(t) => t,
        None => return Vec::new(),
    };

    let data_store = app.data_store.borrow();
    let sessions = data_store.streaming_with_content_for_thread(&thread.id);

    sessions
        .into_iter()
        .map(|session| {
            let agent_name = data_store.get_profile_name(&session.pubkey);
            StreamingContent {
                agent_name,
                content: session.content().to_string(),
            }
        })
        .collect()
}

/// Get typing indicators (agents streaming but no content yet)
fn get_typing_indicators(app: &App) -> Vec<String> {
    let thread = match app.selected_thread.as_ref() {
        Some(t) => t,
        None => return Vec::new(),
    };

    let data_store = app.data_store.borrow();
    let pubkeys = data_store.typing_indicators_for_thread(&thread.id);

    pubkeys
        .into_iter()
        .map(|pubkey| data_store.get_profile_name(pubkey))
        .collect()
}

pub fn render_chat(f: &mut Frame, app: &mut App, area: Rect) {
    let all_messages = app.messages();
    let _render_span = info_span!("render_chat", message_count = all_messages.len()).entered();

    // Calculate dynamic input height based on line count (min 3, max 10)
    let input_lines = app.chat_editor.line_count().max(1);
    let input_height = (input_lines as u16 + 2).clamp(3, 10); // +2 for borders

    // Check if we have attachments (paste or image)
    let has_attachments = !app.chat_editor.attachments.is_empty() || !app.chat_editor.image_attachments.is_empty();
    let has_status = app.status_message.is_some();

    // Check if we have tabs to show
    let has_tabs = !app.open_tabs.is_empty();

    // Build layout based on whether we have attachments, status, and tabs
    let chunks = match (has_attachments, has_status, has_tabs) {
        (true, true, true) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Status line
            Constraint::Length(1),     // Context line
            Constraint::Length(1),     // Attachments line
            Constraint::Length(input_height), // Input
            Constraint::Length(1),     // Tab bar
        ]).split(area),
        (true, true, false) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Status line
            Constraint::Length(1),     // Context line
            Constraint::Length(1),     // Attachments line
            Constraint::Length(input_height), // Input
        ]).split(area),
        (true, false, true) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Context line
            Constraint::Length(1),     // Attachments line
            Constraint::Length(input_height), // Input
            Constraint::Length(1),     // Tab bar
        ]).split(area),
        (true, false, false) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Context line
            Constraint::Length(1),     // Attachments line
            Constraint::Length(input_height), // Input
        ]).split(area),
        (false, true, true) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Status line
            Constraint::Length(1),     // Context line
            Constraint::Length(input_height), // Input
            Constraint::Length(1),     // Tab bar
        ]).split(area),
        (false, true, false) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Status line
            Constraint::Length(1),     // Context line
            Constraint::Length(input_height), // Input
        ]).split(area),
        (false, false, true) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Context line
            Constraint::Length(input_height), // Input
            Constraint::Length(1),     // Tab bar
        ]).split(area),
        (false, false, false) => Layout::vertical([
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

    // Left padding for message content
    let padding = "   ";
    let content_width = area.width.saturating_sub(4) as usize;

    // Render the thread itself (kind:11) as the first message
    // This is NOT in subthread mode - the thread content IS the first message
    if !app.in_subthread() {
        if let Some(ref thread) = app.selected_thread {
            if !thread.content.trim().is_empty() {
                // Single borrow for thread header
                let (user_pubkey, author) = {
                    let store = app.data_store.borrow();
                    (store.user_pubkey.clone(), store.get_profile_name(&thread.pubkey))
                };
                let is_user_thread = user_pubkey.as_ref().map(|pk| pk == &thread.pubkey).unwrap_or(false);

                if is_user_thread {
                    // User's thread - render as card
                    let card_border = Color::Rgb(48, 54, 61);
                    let label_color = Color::Green;

                    let top_border = "─".repeat(content_width.saturating_sub(2));
                    messages_text.push(Line::from(Span::styled(
                        format!("╭{}╮", top_border),
                        Style::default().fg(card_border),
                    )));

                    messages_text.push(Line::from(vec![
                        Span::styled("│ ", Style::default().fg(card_border)),
                        Span::styled(author, Style::default().fg(label_color).add_modifier(Modifier::BOLD)),
                    ]));

                    let markdown_lines = render_markdown(&thread.content);
                    for line in markdown_lines {
                        let mut line_spans = vec![
                            Span::styled("│ ", Style::default().fg(card_border)),
                        ];
                        line_spans.extend(line.spans);
                        messages_text.push(Line::from(line_spans));
                    }

                    let bottom_border = "─".repeat(content_width.saturating_sub(2));
                    messages_text.push(Line::from(Span::styled(
                        format!("╰{}╯", bottom_border),
                        Style::default().fg(card_border),
                    )));
                } else {
                    // Someone else's thread - render with left border
                    let border_color = Color::Cyan;

                    messages_text.push(Line::from(vec![
                        Span::styled("│ ", Style::default().fg(border_color)),
                        Span::styled(author, Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
                    ]));

                    let markdown_lines = render_markdown(&thread.content);
                    for line in markdown_lines {
                        let mut line_spans = vec![
                            Span::styled("│ ", Style::default().fg(border_color)),
                        ];
                        line_spans.extend(line.spans);
                        messages_text.push(Line::from(line_spans));
                    }
                }

                messages_text.push(Line::from(""));
            }
        }
    }

    // If in subthread, render the root message first as a header
    if let Some(ref root_msg) = app.subthread_root_message {
        let author = app.data_store.borrow().get_profile_name(&root_msg.pubkey);
        messages_text.push(Line::from(Span::styled(
            format!("{}{} :", padding, author),
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
        )));

        // Render root message content (truncated)
        let content_preview: String = root_msg.content.chars().take(200).collect();
        let markdown_lines = render_markdown(&content_preview);
        for line in markdown_lines {
            let mut padded_spans = vec![Span::raw(padding)];
            padded_spans.extend(line.spans);
            messages_text.push(Line::from(padded_spans));
        }
        if root_msg.content.len() > 200 {
            messages_text.push(Line::from(Span::styled(format!("{}...", padding), Style::default().fg(Color::DarkGray))));
        }

        // Separator
        messages_text.push(Line::from(Span::styled(
            format!("{}────────────────────────────────────────", padding),
            Style::default().fg(Color::DarkGray),
        )));
        messages_text.push(Line::from(""));
    }

    // Render display messages with card-style layout and action grouping
    {
        let _span = info_span!("render_messages").entered();

        // Collect all unique pubkeys and cache profile names with single borrow
        let (user_pubkey, profile_cache) = {
            let store = app.data_store.borrow();
            let user_pk = store.user_pubkey.clone();

            // Collect unique pubkeys from ALL messages (includes replies not in display)
            let mut pubkeys: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for msg in &all_messages {
                pubkeys.insert(&msg.pubkey);
            }

            // Build profile name cache
            let cache: std::collections::HashMap<String, String> = pubkeys
                .into_iter()
                .map(|pk| (pk.to_string(), store.get_profile_name(pk)))
                .collect();

            (user_pk, cache)
        };

        // Group consecutive action messages
        let grouped = group_messages(&display_messages, user_pubkey.as_deref());

        let mut prev_pubkey: Option<&str> = None;

        for (group_idx, item) in grouped.iter().enumerate() {
            match item {
                DisplayItem::ActionGroup { messages: action_msgs, pubkey } => {
                    // Render collapsed action group
                    let border_color = Color::Cyan;
                    let author = profile_cache.get(pubkey).cloned().unwrap_or_else(|| pubkey[..8.min(pubkey.len())].to_string());

                    // Show header if author changed
                    let show_header = prev_pubkey != Some(pubkey.as_str());
                    prev_pubkey = Some(pubkey.as_str());

                    if show_header {
                        messages_text.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(border_color)),
                            Span::styled(
                                author,
                                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    }

                    // Collapsed summary line
                    let count = action_msgs.len();
                    let first_action: String = action_msgs.first()
                        .map(|m| m.content.trim().chars().take(30).collect())
                        .unwrap_or_default();
                    let last_action: String = action_msgs.last()
                        .map(|m| m.content.trim().chars().take(30).collect())
                        .unwrap_or_default();

                    messages_text.push(Line::from(vec![
                        Span::styled("│ ", Style::default().fg(border_color)),
                        Span::styled("▸ ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            format!("{} actions", count),
                            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" · ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            first_action,
                            Style::default().fg(Color::DarkGray),
                        ),
                        Span::styled(" → ", Style::default().fg(Color::DarkGray)),
                        Span::styled(
                            last_action,
                            Style::default().fg(Color::DarkGray),
                        ),
                    ]));

                    messages_text.push(Line::from(""));
                }

                DisplayItem::SingleMessage(msg) => {
                    let _msg_span = info_span!("render_message", index = group_idx).entered();

                    // Check if this message is selected (for navigation)
                    let is_selected = group_idx == app.selected_message_index && app.input_mode == InputMode::Normal;

                    // Determine if this is from the user or an assistant
                    let is_user_message = user_pubkey.as_ref().map(|pk| pk == &msg.pubkey).unwrap_or(false);

                    // Check if we should show header (first message or different author)
                    let show_header = prev_pubkey != Some(msg.pubkey.as_str());
                    prev_pubkey = Some(msg.pubkey.as_str());

                    let author = profile_cache.get(&msg.pubkey).cloned()
                        .unwrap_or_else(|| msg.pubkey[..8.min(msg.pubkey.len())].to_string());

                    if is_user_message {
                        // USER MESSAGE: Card style with box borders
                        let card_border = if is_selected { Color::Yellow } else { Color::Rgb(48, 54, 61) };
                        let label_color = Color::Green;

                        // Top border of card
                        let top_border = "─".repeat(content_width.saturating_sub(2));
                        messages_text.push(Line::from(Span::styled(
                            format!("╭{}╮", top_border),
                            Style::default().fg(card_border),
                        )));

                        // Label line
                        messages_text.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(card_border)),
                            Span::styled(author.clone(), Style::default().fg(label_color).add_modifier(Modifier::BOLD)),
                        ]));

                        // Content
                        let parsed = parse_message_content(&msg.content);
                        match parsed {
                            MessageContent::PlainText(text) => {
                                let markdown_lines = render_markdown(&text);
                                for line in markdown_lines {
                                    let mut line_spans = vec![
                                        Span::styled("│ ", Style::default().fg(card_border)),
                                    ];
                                    line_spans.extend(line.spans);
                                    messages_text.push(Line::from(line_spans));
                                }
                            }
                            MessageContent::Mixed { text_parts, tool_calls: _ } => {
                                for text_part in text_parts {
                                    if !text_part.trim().is_empty() {
                                        let markdown_lines = render_markdown(&text_part);
                                        for line in markdown_lines {
                                            let mut line_spans = vec![
                                                Span::styled("│ ", Style::default().fg(card_border)),
                                            ];
                                            line_spans.extend(line.spans);
                                            messages_text.push(Line::from(line_spans));
                                        }
                                    }
                                }
                            }
                        }

                        // Bottom border of card
                        let bottom_border = "─".repeat(content_width.saturating_sub(2));
                        messages_text.push(Line::from(Span::styled(
                            format!("╰{}╯", bottom_border),
                            Style::default().fg(card_border),
                        )));

                    } else {
                        // ASSISTANT MESSAGE: Left border style
                        let border_color = if is_selected { Color::Yellow } else { Color::Cyan };

                        // Reasoning messages get muted italic style
                        if msg.is_reasoning {
                            if show_header {
                                messages_text.push(Line::from(vec![
                                    Span::styled("│ ", Style::default().fg(border_color)),
                                    Span::styled(
                                        format!("{} (thinking)", author),
                                        Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
                                    ),
                                ]));
                            }

                            let muted_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
                            for line in msg.content.lines() {
                                messages_text.push(Line::from(vec![
                                    Span::styled("│ ", Style::default().fg(border_color)),
                                    Span::styled(line.to_string(), muted_style),
                                ]));
                            }
                        } else {
                            // Normal assistant message
                            if show_header {
                                messages_text.push(Line::from(vec![
                                    Span::styled("│ ", Style::default().fg(border_color)),
                                    Span::styled(
                                        author.clone(),
                                        Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD),
                                    ),
                                ]));

                                // Show ask indicator if this is an ask event
                                if let Some(ref ask) = msg.ask_event {
                                    let question_count = ask.questions.len();
                                    let indicator_text = if question_count == 1 {
                                        " [❓ Question - Press Ctrl+R to answer] ".to_string()
                                    } else {
                                        format!(" [❓ {} Questions - Press Ctrl+R to answer] ", question_count)
                                    };

                                    messages_text.push(Line::from(vec![
                                        Span::styled("│ ", Style::default().fg(border_color)),
                                        Span::styled(
                                            indicator_text,
                                            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
                                        ),
                                    ]));
                                }
                            }

                            let parsed = {
                                let _span = info_span!("parse_message_content").entered();
                                parse_message_content(&msg.content)
                            };

                            match parsed {
                                MessageContent::PlainText(text) => {
                                    let markdown_lines = render_markdown(&text);
                                    for line in markdown_lines {
                                        let mut line_spans = vec![
                                            Span::styled("│ ", Style::default().fg(border_color)),
                                        ];
                                        line_spans.extend(line.spans);
                                        messages_text.push(Line::from(line_spans));
                                    }
                                }
                                MessageContent::Mixed { text_parts, tool_calls } => {
                                    // Render tool calls as compact blocks
                                    for tool_call in &tool_calls {
                                        let icon = tool_icon(&tool_call.name);
                                        let target = extract_target(tool_call).unwrap_or_default();

                                        messages_text.push(Line::from(vec![
                                            Span::styled("│ ", Style::default().fg(border_color)),
                                            Span::styled("┌─ ", Style::default().fg(Color::DarkGray)),
                                            Span::styled(icon, Style::default()),
                                            Span::styled(" ", Style::default()),
                                            Span::styled(
                                                tool_call.name.to_uppercase(),
                                                Style::default().fg(Color::DarkGray),
                                            ),
                                            Span::styled(" ", Style::default()),
                                            Span::styled(
                                                target,
                                                Style::default().fg(Color::Cyan),
                                            ),
                                        ]));

                                        if let Some(ref result) = tool_call.result {
                                            let (result_icon, result_color) = if result.contains("error") || result.contains("Error") || result.contains("failed") {
                                                ("✗", Color::Red)
                                            } else {
                                                ("✓", Color::Green)
                                            };

                                            let result_preview: String = result.lines().next().unwrap_or("").chars().take(60).collect();
                                            let suffix = if result.len() > 60 { "..." } else { "" };

                                            messages_text.push(Line::from(vec![
                                                Span::styled("│ ", Style::default().fg(border_color)),
                                                Span::styled("└─ ", Style::default().fg(Color::DarkGray)),
                                                Span::styled(result_icon, Style::default().fg(result_color)),
                                                Span::styled(" ", Style::default()),
                                                Span::styled(
                                                    format!("{}{}", result_preview, suffix),
                                                    Style::default().fg(Color::DarkGray),
                                                ),
                                            ]));
                                        }
                                    }

                                    for text_part in text_parts {
                                        if !text_part.trim().is_empty() {
                                            let markdown_lines = render_markdown(&text_part);
                                            for line in markdown_lines {
                                                let mut line_spans = vec![
                                                    Span::styled("│ ", Style::default().fg(border_color)),
                                                ];
                                                line_spans.extend(line.spans);
                                                messages_text.push(Line::from(line_spans));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }

                    // Check if this message has replies and show preview
                    if let Some(replies) = replies_by_parent.get(msg.id.as_str()) {
                        if !replies.is_empty() {
                            let most_recent = replies.iter().max_by_key(|r| r.created_at);
                            if let Some(recent) = most_recent {
                                let reply_author = profile_cache.get(&recent.pubkey).cloned()
                                    .unwrap_or_else(|| recent.pubkey[..8.min(recent.pubkey.len())].to_string());
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

                    // Blank line between messages
                    messages_text.push(Line::from(""));
                }
            }
        }
    }

    // Check for streaming content (agents actively streaming with content)
    let streaming_sessions = {
        let _span = info_span!("get_streaming_content").entered();
        get_streaming_content(app)
    };
    for streaming in &streaming_sessions {
        messages_text.push(Line::from(Span::styled(
            format!("{}{} (streaming...):", padding, streaming.agent_name),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::ITALIC),
        )));

        let markdown_lines = render_markdown(&streaming.content);
        for line in markdown_lines {
            let mut padded_spans = vec![Span::raw(padding)];
            padded_spans.extend(line.spans);
            messages_text.push(Line::from(padded_spans));
        }

        messages_text.push(Line::from(""));
    }

    // Check for typing indicators (agents streaming but no content yet)
    let typing_agents = {
        let _span = info_span!("get_typing_indicators").entered();
        get_typing_indicators(app)
    };
    for agent_name in &typing_agents {
        messages_text.push(Line::from(Span::styled(
            format!("{}{} is typing...", padding, agent_name),
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC),
        )));
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

        // Update max_scroll_offset so scroll methods work correctly
        app.max_scroll_offset = max_scroll;

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
        Span::raw(padding),
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
                .border_style(input_style),
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
    idx += 1;

    // Tab bar (if tabs are open)
    if has_tabs {
        render_tab_bar(f, app, chunks[idx]);
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

    // Render ask modal overlay if open
    if let Some(ref modal_state) = app.ask_modal_state {
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

pub fn render_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();

    // Borrow data_store once for all project name lookups
    let data_store = app.data_store.borrow();

    for (i, tab) in app.open_tabs.iter().enumerate() {
        let is_active = i == app.active_tab_index;

        // Tab number with period separator
        let num_style = if is_active {
            Style::default().fg(Color::Cyan)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(format!("{}. ", i + 1), num_style));

        // Unread indicator (moved before project name)
        if tab.has_unread && !is_active {
            spans.push(Span::styled("● ", Style::default().fg(Color::Red)));
        } else {
            spans.push(Span::raw("● "));
        }

        // Project name (truncated to 8 chars max)
        let project_name = data_store.get_project_name(&tab.project_a_tag);
        let max_project_len = 8;
        let project_display: String = if project_name.len() > max_project_len {
            project_name.chars().take(max_project_len).collect()
        } else {
            project_name
        };

        let project_style = if is_active {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(project_display, project_style));
        spans.push(Span::raw(" | "));

        // Tab title (truncated to fit remaining space)
        let max_title_len = 12;
        let title: String = if tab.thread_title.len() > max_title_len {
            format!("{}...", &tab.thread_title[..max_title_len - 3])
        } else {
            tab.thread_title.clone()
        };

        let title_style = if is_active {
            Style::default().fg(Color::White).add_modifier(Modifier::BOLD)
        } else if tab.has_unread {
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };
        spans.push(Span::styled(title, title_style));

        // Separator between tabs
        if i < app.open_tabs.len() - 1 {
            spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }
    }

    // Add hint at the end
    spans.push(Span::styled("  ", Style::default()));
    spans.push(Span::styled("Tab:cycle x:close", Style::default().fg(Color::DarkGray)));

    let tab_line = Line::from(spans);
    let tab_bar = Paragraph::new(tab_line);
    f.render_widget(tab_bar, area);
}
