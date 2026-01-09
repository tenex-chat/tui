use crate::models::Message;
use crate::ui::markdown::render_markdown;
use crate::ui::todo::{aggregate_todo_state, TodoState, TodoStatus};
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

/// Generate a deterministic color from a pubkey
/// Uses a simple hash to pick from a palette of distinct colors
fn color_from_pubkey(pubkey: &str) -> Color {
    // Palette of distinct, visually pleasing colors for the left indicator
    let colors = [
        Color::Rgb(86, 156, 214),   // Blue
        Color::Rgb(152, 195, 121),  // Green
        Color::Rgb(197, 134, 192),  // Purple
        Color::Rgb(206, 145, 120),  // Orange
        Color::Rgb(86, 212, 221),   // Cyan
        Color::Rgb(220, 220, 170),  // Yellow
        Color::Rgb(244, 112, 112),  // Red
        Color::Rgb(169, 154, 203),  // Lavender
    ];

    // Simple hash: sum of bytes modulo palette size
    let hash: usize = pubkey.bytes().map(|b| b as usize).sum();
    colors[hash % colors.len()]
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

    // Aggregate todo state from all messages
    let todo_state = aggregate_todo_state(&all_messages);

    // Check if we should show inline ask UI instead of normal input
    let should_show_ask_ui = if app.input_mode == InputMode::Editing {
        // Only auto-activate ask UI if in editing mode AND there's an unanswered ask
        if let Some((message_id, ask_event)) = app.has_unanswered_ask_event() {
            // Auto-activate ask UI if not already active
            if app.ask_modal_state.is_none() {
                app.open_ask_modal(message_id, ask_event);
            }
            true
        } else {
            false
        }
    } else {
        // Ask modal is already active (user manually opened it)
        app.ask_modal_state.is_some()
    };

    // Calculate dynamic input height based on what we're showing
    let input_height = if should_show_ask_ui {
        // Ask UI height: tab bar (1) + question header (2) + options + custom (n+1) + help (3)
        // Calculate based on current question's option count
        let option_count = app.ask_modal_state.as_ref()
            .and_then(|state| state.input_state.current_question())
            .map(|q| match q {
                crate::models::AskQuestion::SingleSelect { suggestions, .. } => suggestions.len() + 1, // +1 for custom
                crate::models::AskQuestion::MultiSelect { options, .. } => options.len() + 1, // +1 for custom
            })
            .unwrap_or(3);
        // tab(1) + header(2) + options(n) + help(3) = 6 + n, min 9, max 15
        (6 + option_count).clamp(9, 15) as u16
    } else {
        // Normal input: dynamic based on line count (min 3, max 10)
        let input_lines = app.chat_editor.line_count().max(1);
        (input_lines as u16 + 2).clamp(3, 10) // +2 for borders
    };

    // Check if we have attachments (paste or image) - only relevant for normal input
    let has_attachments = !should_show_ask_ui && (!app.chat_editor.attachments.is_empty() || !app.chat_editor.image_attachments.is_empty());
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

    // Split messages area horizontally if there are todos AND sidebar is visible
    let (messages_area_raw, sidebar_area) = if todo_state.has_todos() && app.todo_sidebar_visible {
        let horiz = Layout::horizontal([
            Constraint::Min(40),
            Constraint::Length(30),
        ]).split(chunks[0]);
        (horiz[0], Some(horiz[1]))
    } else {
        (chunks[0], None)
    };

    // Add horizontal padding to messages area
    let h_padding: u16 = 2;
    let messages_area = Rect {
        x: messages_area_raw.x + h_padding,
        y: messages_area_raw.y,
        width: messages_area_raw.width.saturating_sub(h_padding * 2),
        height: messages_area_raw.height,
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
        // Exclude the thread itself - it's rendered separately above
        all_messages.iter()
            .filter(|m| {
                // Skip the thread message - already rendered above
                if Some(m.id.as_str()) == thread_id {
                    return false;
                }
                m.reply_to.is_none() || m.reply_to.as_deref() == thread_id
            })
            .collect()
    };

    // Messages area
    let mut messages_text: Vec<Line> = Vec::new();

    // Left padding for message content
    let padding = "   ";
    let content_width = messages_area.width.saturating_sub(2) as usize;

    // Render the thread itself (kind:11) as the first message - same style as all other messages
    if !app.in_subthread() {
        if let Some(ref thread) = app.selected_thread {
            if !thread.content.trim().is_empty() {
                let author = {
                    let store = app.data_store.borrow();
                    store.get_profile_name(&thread.pubkey)
                };

                // Same card style as all messages - deterministic color from pubkey
                let indicator_color = color_from_pubkey(&thread.pubkey);
                let card_bg = Color::Rgb(30, 30, 30);

                // Author line with card background
                let author_line = format!("│ {}", author);
                let padding_needed = content_width.saturating_sub(author_line.len());
                messages_text.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(indicator_color).bg(card_bg)),
                    Span::styled(" ", Style::default().bg(card_bg)),
                    Span::styled(author, Style::default().fg(indicator_color).add_modifier(Modifier::BOLD).bg(card_bg)),
                    Span::styled(" ".repeat(padding_needed), Style::default().bg(card_bg)),
                ]));

                // Content with markdown
                let markdown_lines = render_markdown(&thread.content);
                for md_line in &markdown_lines {
                    let mut line_spans = vec![
                        Span::styled("│", Style::default().fg(indicator_color).bg(card_bg)),
                        Span::styled(" ", Style::default().bg(card_bg)),
                    ];
                    let mut line_len = 2; // "│ "
                    for span in &md_line.spans {
                        line_len += span.content.len();
                        let mut new_style = span.style;
                        new_style = new_style.bg(card_bg);
                        line_spans.push(Span::styled(span.content.clone(), new_style));
                    }
                    // Pad to full width
                    let pad = content_width.saturating_sub(line_len);
                    if pad > 0 {
                        line_spans.push(Span::styled(" ".repeat(pad), Style::default().bg(card_bg)));
                    }
                    messages_text.push(Line::from(line_spans));
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
                    let indicator_color = color_from_pubkey(pubkey);
                    let author = profile_cache.get(pubkey).cloned().unwrap_or_else(|| pubkey[..8.min(pubkey.len())].to_string());

                    // Show header if author changed
                    let show_header = prev_pubkey != Some(pubkey.as_str());
                    prev_pubkey = Some(pubkey.as_str());

                    if show_header {
                        messages_text.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(indicator_color)),
                            Span::styled(
                                author,
                                Style::default().fg(indicator_color).add_modifier(Modifier::BOLD),
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
                        Span::styled("│ ", Style::default().fg(indicator_color)),
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

                    let author = profile_cache.get(&msg.pubkey).cloned()
                        .unwrap_or_else(|| msg.pubkey[..8.min(msg.pubkey.len())].to_string());

                    // === OPENCODE-STYLE CARD ===
                    // - Left indicator line (deterministic color from pubkey)
                    // - Full-width shaded background
                    // - Author on first line, content below

                    let indicator_color = color_from_pubkey(&msg.pubkey);
                    let card_bg = Color::Rgb(30, 30, 30);
                    let card_bg_selected = Color::Rgb(45, 45, 45);
                    let bg = if is_selected { card_bg_selected } else { card_bg };

                    // Helper to pad line to full width
                    let pad_line = |spans: &mut Vec<Span>, current_len: usize| {
                        let pad = content_width.saturating_sub(current_len);
                        if pad > 0 {
                            spans.push(Span::styled(" ".repeat(pad), Style::default().bg(bg)));
                        }
                    };

                    // First line: indicator + author (padded to full width)
                    let author_len = 2 + author.len(); // "│ " + author
                    let mut author_spans = vec![
                        Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                        Span::styled(" ", Style::default().bg(bg)),
                        Span::styled(author.clone(), Style::default().fg(indicator_color).add_modifier(Modifier::BOLD).bg(bg)),
                    ];
                    pad_line(&mut author_spans, author_len);
                    messages_text.push(Line::from(author_spans));

                    // Content with markdown
                    let parsed = parse_message_content(&msg.content);
                    let content_text = match &parsed {
                        MessageContent::PlainText(text) => text.clone(),
                        MessageContent::Mixed { text_parts, .. } => text_parts.join("\n"),
                    };
                    let markdown_lines = render_markdown(&content_text);

                    // Content lines: indicator + content with background (padded)
                    for md_line in &markdown_lines {
                        let mut line_spans = vec![
                            Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                            Span::styled(" ", Style::default().bg(bg)),
                        ];
                        let mut line_len = 2; // "│ "
                        for span in &md_line.spans {
                            line_len += span.content.len();
                            let mut new_style = span.style;
                            new_style = new_style.bg(bg);
                            line_spans.push(Span::styled(span.content.clone(), new_style));
                        }
                        pad_line(&mut line_spans, line_len);
                        messages_text.push(Line::from(line_spans));
                    }

                    // Ask event indicator
                    if let Some(ref ask) = msg.ask_event {
                        let question_count = ask.questions.len();
                        let indicator_text = if question_count == 1 {
                            "❓ Question - Press 'i' to answer".to_string()
                        } else {
                            format!("❓ {} Questions - Press 'i' to answer", question_count)
                        };
                        let ask_len = 2 + indicator_text.len();
                        let mut ask_spans = vec![
                            Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                            Span::styled(" ", Style::default().bg(bg)),
                            Span::styled(indicator_text, Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD).bg(bg)),
                        ];
                        pad_line(&mut ask_spans, ask_len);
                        messages_text.push(Line::from(ask_spans));
                    }

                    // Tool calls
                    if let MessageContent::Mixed { tool_calls, .. } = &parsed {
                        for tool_call in tool_calls {
                            let icon = tool_icon(&tool_call.name);
                            let target = extract_target(tool_call).unwrap_or_default();
                            let tool_text = format!("{} {} {}", icon, tool_call.name.to_uppercase(), target);
                            let tool_len = 3 + tool_text.len(); // "│  " + content
                            let mut tool_spans = vec![
                                Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                                Span::styled("  ", Style::default().bg(bg)),
                                Span::styled(icon, Style::default().bg(bg)),
                                Span::styled(" ", Style::default().bg(bg)),
                                Span::styled(tool_call.name.to_uppercase(), Style::default().fg(Color::DarkGray).bg(bg)),
                                Span::styled(" ", Style::default().bg(bg)),
                                Span::styled(target, Style::default().fg(Color::Cyan).bg(bg)),
                            ];
                            pad_line(&mut tool_spans, tool_len);
                            messages_text.push(Line::from(tool_spans));
                        }
                    }

                    // Replies indicator
                    if let Some(replies) = replies_by_parent.get(msg.id.as_str()) {
                        if !replies.is_empty() {
                            let replies_text = format!("{} replies", replies.len());
                            let replies_len = 7 + replies_text.len(); // "│  └→ " + text
                            let mut replies_spans = vec![
                                Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                                Span::styled("  └→ ", Style::default().fg(Color::DarkGray).bg(bg)),
                                Span::styled(replies_text, Style::default().fg(Color::Magenta).bg(bg)),
                            ];
                            pad_line(&mut replies_spans, replies_len);
                            messages_text.push(Line::from(replies_spans));
                        }
                    }

                    prev_pubkey = Some(msg.pubkey.as_str());

                    // Empty line after card
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

    // Check for local streaming content (from Unix socket, not Nostr)
    // Clone the buffer to avoid borrowing app across the mutation below
    if let Some(buffer) = app.local_streaming_content().cloned() {
        if !buffer.text_content.is_empty() || !buffer.reasoning_content.is_empty() {
            // Render agent header with streaming indicator
            messages_text.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(Color::Magenta)),
                Span::styled(
                    "Agent",
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " (streaming)",
                    Style::default().fg(Color::Magenta).add_modifier(Modifier::ITALIC),
                ),
            ]));

            // Render reasoning content first if present (muted style)
            if !buffer.reasoning_content.is_empty() {
                let reasoning_style = Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC);
                for line in buffer.reasoning_content.lines() {
                    messages_text.push(Line::from(vec![
                        Span::styled("│ ", Style::default().fg(Color::Magenta)),
                        Span::styled(line.to_string(), reasoning_style),
                    ]));
                }
            }

            // Render text content with cursor indicator
            if !buffer.text_content.is_empty() {
                let markdown_lines = render_markdown(&buffer.text_content);
                for (i, line) in markdown_lines.iter().enumerate() {
                    let mut line_spans = vec![
                        Span::styled("│ ", Style::default().fg(Color::Magenta)),
                    ];
                    line_spans.extend(line.spans.clone());

                    // Add cursor indicator at the end of the last line
                    if i == markdown_lines.len() - 1 && !buffer.is_complete {
                        line_spans.push(Span::styled("▌", Style::default().fg(Color::Magenta)));
                    }
                    messages_text.push(Line::from(line_spans));
                }
            } else if !buffer.is_complete {
                // Show just cursor if we have no text yet but are streaming
                messages_text.push(Line::from(vec![
                    Span::styled("│ ", Style::default().fg(Color::Magenta)),
                    Span::styled("▌", Style::default().fg(Color::Magenta)),
                ]));
            }

            messages_text.push(Line::from(""));
        }
    }

    if messages_text.is_empty() {
        let empty = Paragraph::new("No messages yet. Press 'i' to start typing.")
            .style(Style::default().fg(Color::DarkGray));
        f.render_widget(empty, messages_area);
    } else {
        let visible_height = messages_area.height as usize;
        let content_width = messages_area.width as usize;

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
            .wrap(Wrap { trim: false })
            .scroll((scroll as u16, 0));

        f.render_widget(messages, messages_area);
    }

    // Render todo sidebar if there are todos
    if let Some(sidebar) = sidebar_area {
        render_todo_sidebar(f, &todo_state, sidebar);
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

    // Input area - show either ask UI or normal chat input
    let input_area = chunks[idx];

    if should_show_ask_ui {
        // Render inline ask UI (replacing input box)
        if let Some(ref modal_state) = app.ask_modal_state {
            use crate::ui::views::render_inline_ask_ui;
            render_inline_ask_ui(f, modal_state, input_area);
        }
    } else {
        // Normal chat input - deterministic color border based on user's pubkey
        let is_active = app.input_mode == InputMode::Editing;

        // Get user's deterministic color for the left border
        let user_color = app.data_store.borrow().user_pubkey.as_ref()
            .map(|pk| color_from_pubkey(pk))
            .unwrap_or(Color::Rgb(86, 156, 214)); // Fallback to blue

        let indicator_color = if is_active {
            user_color
        } else {
            Color::Rgb(60, 60, 60) // Dim when inactive
        };
        let text_color = if is_active {
            Color::White
        } else {
            Color::DarkGray
        };
        let input_bg = Color::Rgb(30, 30, 30);

        // Build input lines with left indicator and padding
        let input_text = app.chat_editor.text.as_str();
        let mut input_lines: Vec<Line> = Vec::new();
        let content_width = input_area.width.saturating_sub(3) as usize; // -3 for "│ " and padding

        if input_text.is_empty() {
            // Placeholder text when empty
            let placeholder = if is_active { "Type your message..." } else { "" };
            let pad = content_width.saturating_sub(placeholder.len());
            input_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(indicator_color).bg(input_bg)),
                Span::styled(" ", Style::default().bg(input_bg)),
                Span::styled(placeholder, Style::default().fg(Color::DarkGray).bg(input_bg)),
                Span::styled(" ".repeat(pad), Style::default().bg(input_bg)),
            ]));
        } else {
            // Render each line of input with indicator
            for line in input_text.lines() {
                let pad = content_width.saturating_sub(line.len());
                input_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(indicator_color).bg(input_bg)),
                    Span::styled(" ", Style::default().bg(input_bg)),
                    Span::styled(line.to_string(), Style::default().fg(text_color).bg(input_bg)),
                    Span::styled(" ".repeat(pad), Style::default().bg(input_bg)),
                ]));
            }
            // Handle case where input ends with newline
            if input_text.ends_with('\n') || input_lines.is_empty() {
                input_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(indicator_color).bg(input_bg)),
                    Span::styled(" ", Style::default().bg(input_bg)),
                    Span::styled(" ".repeat(content_width), Style::default().bg(input_bg)),
                ]));
            }
        }

        // Pad to fill the input area height with gray background
        while input_lines.len() < input_area.height as usize {
            input_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(indicator_color).bg(input_bg)),
                Span::styled(" ", Style::default().bg(input_bg)),
                Span::styled(" ".repeat(content_width), Style::default().bg(input_bg)),
            ]));
        }

        let input = Paragraph::new(input_lines)
            .style(Style::default().bg(input_bg));
        f.render_widget(input, input_area);

        // Show cursor in input mode
        if is_active && !app.showing_attachment_modal {
            let (cursor_row, cursor_col) = app.chat_editor.cursor_position();
            f.set_cursor_position((
                input_area.x + cursor_col as u16 + 2, // +2 for "│ "
                input_area.y + cursor_row as u16,
            ));
        }
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

/// Render the todo sidebar on the right side of the chat
fn render_todo_sidebar(f: &mut Frame, todo_state: &TodoState, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Header with count
    let completed = todo_state.completed_count();
    let total = todo_state.items.len();
    lines.push(Line::from(vec![
        Span::styled("Todo List ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::styled(format!("{}/{}", completed, total), Style::default().fg(Color::DarkGray)),
    ]));

    // Progress bar
    let progress_width = (area.width as usize).saturating_sub(4);
    let filled = if total > 0 { (completed * progress_width) / total } else { 0 };
    let empty_bar = progress_width.saturating_sub(filled);
    lines.push(Line::from(vec![
        Span::styled("━".repeat(filled), Style::default().fg(Color::Green)),
        Span::styled("━".repeat(empty_bar), Style::default().fg(Color::Rgb(60, 60, 60))),
    ]));
    lines.push(Line::from(""));

    // Active task highlight
    if let Some(active) = todo_state.in_progress_item() {
        lines.push(Line::from(Span::styled(
            "In Progress",
            Style::default().fg(Color::Rgb(86, 156, 214)),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", truncate_str(&active.title, (area.width as usize).saturating_sub(4))),
            Style::default().fg(Color::White),
        )));
        if let Some(ref desc) = active.description {
            lines.push(Line::from(Span::styled(
                format!("  {}", truncate_str(desc, (area.width as usize).saturating_sub(4))),
                Style::default().fg(Color::DarkGray),
            )));
        }
        lines.push(Line::from(""));
    }

    // Todo items
    for item in &todo_state.items {
        let (icon, icon_style) = match item.status {
            TodoStatus::Done => ("✓", Style::default().fg(Color::Green)),
            TodoStatus::InProgress => ("◐", Style::default().fg(Color::Rgb(86, 156, 214))),
            TodoStatus::Pending => ("○", Style::default().fg(Color::DarkGray)),
        };

        let title_style = if item.status == TodoStatus::Done {
            Style::default().fg(Color::DarkGray).add_modifier(Modifier::CROSSED_OUT)
        } else {
            Style::default().fg(Color::White)
        };

        let title = truncate_str(&item.title, (area.width as usize).saturating_sub(4));
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", icon), icon_style),
            Span::styled(title, title_style),
        ]));
    }

    let sidebar = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(Style::default().fg(Color::DarkGray)),
        )
        .style(Style::default().bg(Color::Rgb(38, 38, 38))); // Gray background like screenshot

    f.render_widget(sidebar, area);
}

/// Truncate a string to fit within max_width characters
fn truncate_str(s: &str, max_width: usize) -> String {
    if s.len() <= max_width {
        s.to_string()
    } else if max_width > 3 {
        format!("{}...", &s[..max_width - 3])
    } else {
        s.chars().take(max_width).collect()
    }
}
