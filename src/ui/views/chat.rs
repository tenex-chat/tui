use crate::models::Message;
use crate::ui::markdown::render_markdown;
use crate::ui::theme;
use crate::ui::todo::{aggregate_todo_state, TodoState, TodoStatus};
use crate::ui::tool_calls::{parse_message_content, MessageContent, tool_icon, extract_target};
use crate::ui::{App, InputMode, ModalState};
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

pub fn render_chat(f: &mut Frame, app: &mut App, area: Rect) {
    // Fill entire area with app background (pure black)
    let bg_block = Block::default().style(Style::default().bg(theme::BG_APP));
    f.render_widget(bg_block, area);

    let all_messages = app.messages();
    let _render_span = info_span!("render_chat", message_count = all_messages.len()).entered();

    // Aggregate todo state from all messages
    let todo_state = aggregate_todo_state(&all_messages);

    // Auto-open ask modal for first unanswered question (if not already open)
    if app.ask_modal_state.is_none() {
        if let Some((message_id, ask_event)) = app.has_unanswered_ask_event() {
            app.open_ask_modal(message_id, ask_event);
        }
    }

    // Calculate dynamic input height - always normal input now (ask UI is inline with messages)
    // +3 = 1 for padding top, 1 for context line at bottom
    let input_lines = app.chat_editor.line_count().max(1);
    let input_height = (input_lines as u16 + 3).clamp(4, 12);

    // Check if we have attachments (paste or image)
    let has_attachments = !app.chat_editor.attachments.is_empty() || !app.chat_editor.image_attachments.is_empty();
    let has_status = app.status_message.is_some();

    // Check if we have tabs to show
    let has_tabs = !app.open_tabs.is_empty();

    // Build layout based on whether we have attachments, status, and tabs
    // Context line is now INSIDE the input card, not separate
    let chunks = match (has_attachments, has_status, has_tabs) {
        (true, true, true) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Status line
            Constraint::Length(1),     // Attachments line
            Constraint::Length(input_height), // Input (includes context)
            Constraint::Length(1),     // Tab bar
        ]).split(area),
        (true, true, false) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Status line
            Constraint::Length(1),     // Attachments line
            Constraint::Length(input_height), // Input (includes context)
        ]).split(area),
        (true, false, true) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Attachments line
            Constraint::Length(input_height), // Input (includes context)
            Constraint::Length(1),     // Tab bar
        ]).split(area),
        (true, false, false) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Attachments line
            Constraint::Length(input_height), // Input (includes context)
        ]).split(area),
        (false, true, true) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Status line
            Constraint::Length(input_height), // Input (includes context)
            Constraint::Length(1),     // Tab bar
        ]).split(area),
        (false, true, false) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(1),     // Status line
            Constraint::Length(input_height), // Input (includes context)
        ]).split(area),
        (false, false, true) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(input_height), // Input (includes context)
            Constraint::Length(1),     // Tab bar
        ]).split(area),
        (false, false, false) => Layout::vertical([
            Constraint::Min(0),        // Messages
            Constraint::Length(input_height), // Input (includes context)
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

    // Add horizontal padding to messages area for breathing room
    let h_padding: u16 = 3;
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

    // Render the thread itself (kind:1) as the first message - same style as all other messages
    if !app.in_subthread() {
        if let Some(ref thread) = app.selected_thread {
            if !thread.content.trim().is_empty() {
                let author = {
                    let store = app.data_store.borrow();
                    store.get_profile_name(&thread.pubkey)
                };

                // Same card style as all messages - deterministic color from pubkey
                let indicator_color = theme::user_color(&thread.pubkey);
                let card_bg = theme::BG_CARD;

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
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD),
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
            messages_text.push(Line::from(Span::styled(format!("{}...", padding), Style::default().fg(theme::TEXT_MUTED))));
        }

        // Separator
        messages_text.push(Line::from(Span::styled(
            format!("{}────────────────────────────────────────", padding),
            Style::default().fg(theme::BORDER_INACTIVE),
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
                    let indicator_color = theme::user_color(pubkey);
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
                        Span::styled("▸ ", Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(
                            format!("{} actions", count),
                            Style::default().fg(theme::ACCENT_SPECIAL).add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(
                            first_action,
                            Style::default().fg(theme::TEXT_MUTED),
                        ),
                        Span::styled(" → ", Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(
                            last_action,
                            Style::default().fg(theme::TEXT_MUTED),
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

                    let indicator_color = theme::user_color(&msg.pubkey);
                    let card_bg = theme::BG_CARD;
                    let card_bg_selected = theme::BG_SELECTED;
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

                    // Ask event - only render if NOT answered by current user
                    if msg.ask_event.is_some() && !app.is_ask_answered_by_user(&msg.id) {
                        // Unanswered question - render full inline ask UI
                        // The modal should be auto-opened (handled at start of render)
                        if let Some(ref modal_state) = app.ask_modal_state {
                            if modal_state.message_id == msg.id {
                                let ask_lines = crate::ui::views::render_inline_ask_lines(
                                    modal_state,
                                    indicator_color,
                                    bg,
                                    content_width,
                                );
                                messages_text.extend(ask_lines);
                            }
                        }
                        // If modal not open for this message yet, it will be auto-opened
                        // on next render cycle (handled at start of render_chat)
                    }
                    // If answered, don't render anything special - answer shows as reply message

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
                                Span::styled(tool_call.name.to_uppercase(), Style::default().fg(theme::TEXT_MUTED).bg(bg)),
                                Span::styled(" ", Style::default().bg(bg)),
                                Span::styled(target, Style::default().fg(theme::ACCENT_PRIMARY).bg(bg)),
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
                                Span::styled("  └→ ", Style::default().fg(theme::TEXT_MUTED).bg(bg)),
                                Span::styled(replies_text, Style::default().fg(theme::ACCENT_SPECIAL).bg(bg)),
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

    // Check for local streaming content (from Unix socket, not Nostr)
    // Clone the buffer to avoid borrowing app across the mutation below
    if let Some(buffer) = app.local_streaming_content().cloned() {
        if !buffer.text_content.is_empty() || !buffer.reasoning_content.is_empty() {
            // Render agent header with streaming indicator
            messages_text.push(Line::from(vec![
                Span::styled("│ ", Style::default().fg(theme::ACCENT_SPECIAL)),
                Span::styled(
                    "Agent",
                    Style::default().fg(theme::ACCENT_SPECIAL).add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " (streaming)",
                    Style::default().fg(theme::ACCENT_SPECIAL).add_modifier(Modifier::ITALIC),
                ),
            ]));

            // Render reasoning content first if present (muted style)
            if !buffer.reasoning_content.is_empty() {
                let reasoning_style = Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::ITALIC);
                for line in buffer.reasoning_content.lines() {
                    messages_text.push(Line::from(vec![
                        Span::styled("│ ", Style::default().fg(theme::ACCENT_SPECIAL)),
                        Span::styled(line.to_string(), reasoning_style),
                    ]));
                }
            }

            // Render text content with cursor indicator
            if !buffer.text_content.is_empty() {
                let markdown_lines = render_markdown(&buffer.text_content);
                for (i, line) in markdown_lines.iter().enumerate() {
                    let mut line_spans = vec![
                        Span::styled("│ ", Style::default().fg(theme::ACCENT_SPECIAL)),
                    ];
                    line_spans.extend(line.spans.clone());

                    // Add cursor indicator at the end of the last line
                    if i == markdown_lines.len() - 1 && !buffer.is_complete {
                        line_spans.push(Span::styled("▌", Style::default().fg(theme::ACCENT_SPECIAL)));
                    }
                    messages_text.push(Line::from(line_spans));
                }
            } else if !buffer.is_complete {
                // Show just cursor if we have no text yet but are streaming
                messages_text.push(Line::from(vec![
                    Span::styled("│ ", Style::default().fg(theme::ACCENT_SPECIAL)),
                    Span::styled("▌", Style::default().fg(theme::ACCENT_SPECIAL)),
                ]));
            }

            messages_text.push(Line::from(""));
        }
    }

    if messages_text.is_empty() {
        let empty = Paragraph::new("No messages yet. Press 'i' to start typing.")
            .style(Style::default().fg(theme::TEXT_MUTED));
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
                Span::styled("⏳ ", Style::default().fg(theme::ACCENT_WARNING)),
                Span::styled(msg.as_str(), Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
            ]);
            let status = Paragraph::new(status_line);
            f.render_widget(status, chunks[idx]);
        }
        idx += 1;
    }

    // Agent/branch display (will be rendered inside input card)
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

    // Attachments line (if any)
    if has_attachments {
        let mut attachment_spans: Vec<Span> = vec![Span::styled("Attachments: ", Style::default().fg(theme::TEXT_MUTED))];
        let img_count = app.chat_editor.image_attachments.len();

        // Show image attachments (focus index 0..img_count)
        for (i, img) in app.chat_editor.image_attachments.iter().enumerate() {
            let is_focused = app.chat_editor.focused_attachment == Some(i);
            let style = if is_focused {
                Style::default().fg(Color::Black).bg(theme::ACCENT_SPECIAL).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::ACCENT_SPECIAL)
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
                Style::default().fg(Color::Black).bg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::ACCENT_WARNING)
            };
            attachment_spans.push(Span::styled(
                format!("[Paste #{}] ", attachment.id),
                style,
            ));
        }

        // Show hint based on what's focused
        if app.chat_editor.focused_attachment.is_some() {
            attachment_spans.push(Span::styled("(Backspace to delete, ↓ to exit)", Style::default().fg(theme::TEXT_MUTED)));
        } else {
            attachment_spans.push(Span::styled("(↑ to select)", Style::default().fg(theme::TEXT_MUTED)));
        }
        let attachments_line = Line::from(attachment_spans);
        let attachments = Paragraph::new(attachments_line);
        f.render_widget(attachments, chunks[idx]);
        idx += 1;
    }

    // Input area - always show normal chat input (ask UI is inline with messages now)
    let input_area = chunks[idx];

    // Normal chat input - deterministic color border based on user's pubkey
    let is_input_active = app.input_mode == InputMode::Editing && app.ask_modal_state.is_none();

    // Get user's deterministic color for the left border
    let user_color = app.data_store.borrow().user_pubkey.as_ref()
        .map(|pk| theme::user_color(pk))
        .unwrap_or(theme::ACCENT_PRIMARY); // Fallback to accent

    let input_indicator_color = if is_input_active {
        user_color
    } else {
        theme::BORDER_INACTIVE // Dim when inactive or ask modal is active
    };
    let text_color = if is_input_active {
        theme::TEXT_PRIMARY
    } else {
        theme::TEXT_MUTED
    };
    let input_bg = theme::BG_INPUT;

    // Build input card with padding and context line at bottom
    let input_text = app.chat_editor.text.as_str();
    let mut input_lines: Vec<Line> = Vec::new();
    let input_content_width = input_area.width.saturating_sub(5) as usize; // -5 for "│  " left and "  " right padding

    // Top padding line
    input_lines.push(Line::from(vec![
        Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
        Span::styled(" ".repeat(input_area.width.saturating_sub(1) as usize), Style::default().bg(input_bg)),
    ]));

    if input_text.is_empty() {
        // Placeholder text when empty
        let placeholder = if is_input_active { "Type your message..." } else { "" };
        let pad = input_content_width.saturating_sub(placeholder.len());
        input_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
            Span::styled("  ", Style::default().bg(input_bg)), // 2-char left padding
            Span::styled(placeholder, Style::default().fg(theme::TEXT_DIM).bg(input_bg)),
            Span::styled(" ".repeat(pad + 2), Style::default().bg(input_bg)), // +2 right padding
        ]));
    } else {
        // Render each line of input with padding
        for line in input_text.lines() {
            let pad = input_content_width.saturating_sub(line.len());
            input_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
                Span::styled("  ", Style::default().bg(input_bg)), // 2-char left padding
                Span::styled(line.to_string(), Style::default().fg(text_color).bg(input_bg)),
                Span::styled(" ".repeat(pad + 2), Style::default().bg(input_bg)), // +2 right padding
            ]));
        }
        // Handle case where input ends with newline
        if input_text.ends_with('\n') || input_lines.len() == 1 {
            input_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
                Span::styled(" ".repeat(input_area.width.saturating_sub(1) as usize), Style::default().bg(input_bg)),
            ]));
        }
    }

    // Reserve last line for context - pad middle lines to fill space
    let target_height = input_area.height.saturating_sub(1) as usize; // -1 for context line
    while input_lines.len() < target_height {
        input_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
            Span::styled(" ".repeat(input_area.width.saturating_sub(1) as usize), Style::default().bg(input_bg)),
        ]));
    }

    // Context line at bottom: @agent on %branch
    let context_str = format!("@{}{}", agent_display, branch_display);
    let context_pad = input_area.width.saturating_sub(context_str.len() as u16 + 4) as usize; // +4 for "│  " and " "
    input_lines.push(Line::from(vec![
        Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
        Span::styled("  ", Style::default().bg(input_bg)), // 2-char left padding
        Span::styled(format!("@{}", agent_display), Style::default().fg(theme::ACCENT_PRIMARY).bg(input_bg)),
        Span::styled(branch_display.clone(), Style::default().fg(theme::ACCENT_SUCCESS).bg(input_bg)),
        Span::styled(" ".repeat(context_pad), Style::default().bg(input_bg)),
    ]));

    let input = Paragraph::new(input_lines)
        .style(Style::default().bg(input_bg));
    f.render_widget(input, input_area);

    // Show cursor in input mode (but not when ask modal is active)
    // +1 for top padding line, +3 for "│  " prefix
    if is_input_active && !app.showing_attachment_modal {
        let (cursor_row, cursor_col) = app.chat_editor.cursor_position();
        f.set_cursor_position((
            input_area.x + cursor_col as u16 + 3, // +3 for "│  "
            input_area.y + cursor_row as u16 + 1, // +1 for top padding
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
    if matches!(app.modal_state, ModalState::BranchSelector { .. }) {
        render_branch_selector(f, app, area);
    }

    // Render attachment modal if showing
    if app.showing_attachment_modal {
        render_attachment_modal(f, app, area);
    }

    // Render tab modal if showing (Alt+/)
    if app.showing_tab_modal {
        super::home::render_tab_modal(f, app, area);
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
        vec![ListItem::new(msg).style(Style::default().fg(theme::TEXT_MUTED))]
    } else {
        agents
            .iter()
            .enumerate()
            .map(|(i, agent)| {
                let style = if i == app.agent_selector_index {
                    Style::default()
                        .fg(Color::Black)
                        .bg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
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
            .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
            .title(title),
    );

    f.render_widget(list, popup_area);
}

fn render_branch_selector(f: &mut Frame, app: &App, area: Rect) {
    let branches = app.filtered_branches();
    let all_branches = app.available_branches();
    let selector_index = app.branch_selector_index();
    let selector_filter = app.branch_selector_filter();

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
        vec![ListItem::new(msg).style(Style::default().fg(theme::TEXT_MUTED))]
    } else {
        branches
            .iter()
            .enumerate()
            .map(|(i, branch)| {
                let style = if i == selector_index {
                    Style::default()
                        .fg(Color::Black)
                        .bg(theme::ACCENT_SUCCESS)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };

                ListItem::new(branch.clone()).style(style)
            })
            .collect()
    };

    let title = if selector_filter.is_empty() {
        "Select Branch (type to filter)".to_string()
    } else {
        format!("Select Branch: {}", selector_filter)
    };

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT_SUCCESS))
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
            Style::default().fg(theme::ACCENT_PRIMARY)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };
        spans.push(Span::styled(format!("{}. ", i + 1), num_style));

        // Unread indicator (moved before project name)
        if tab.has_unread && !is_active {
            spans.push(Span::styled("● ", Style::default().fg(theme::ACCENT_ERROR)));
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
            Style::default().fg(theme::ACCENT_SUCCESS)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
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
            theme::tab_active()
        } else if tab.has_unread {
            theme::tab_unread()
        } else {
            theme::tab_inactive()
        };
        spans.push(Span::styled(title, title_style));

        // Separator between tabs
        if i < app.open_tabs.len() - 1 {
            spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
        }
    }

    // Add hint at the end
    spans.push(Span::styled("  ", Style::default()));
    spans.push(Span::styled("Tab:cycle x:close", Style::default().fg(theme::TEXT_MUTED)));

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
        Span::styled("Todo List ", theme::text_bold()),
        Span::styled(format!("{}/{}", completed, total), Style::default().fg(theme::TEXT_MUTED)),
    ]));

    // Progress bar
    let progress_width = (area.width as usize).saturating_sub(4);
    let filled = if total > 0 { (completed * progress_width) / total } else { 0 };
    let empty_bar = progress_width.saturating_sub(filled);
    lines.push(Line::from(vec![
        Span::styled("━".repeat(filled), Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled("━".repeat(empty_bar), Style::default().fg(theme::PROGRESS_EMPTY)),
    ]));
    lines.push(Line::from(""));

    // Active task highlight
    if let Some(active) = todo_state.in_progress_item() {
        lines.push(Line::from(Span::styled(
            "In Progress",
            theme::todo_in_progress(),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", truncate_str(&active.title, (area.width as usize).saturating_sub(4))),
            theme::text_primary(),
        )));
        if let Some(ref desc) = active.description {
            lines.push(Line::from(Span::styled(
                format!("  {}", truncate_str(desc, (area.width as usize).saturating_sub(4))),
                theme::text_muted(),
            )));
        }
        lines.push(Line::from(""));
    }

    // Todo items
    for item in &todo_state.items {
        let (icon, icon_style) = match item.status {
            TodoStatus::Done => ("✓", theme::todo_done()),
            TodoStatus::InProgress => ("◐", theme::todo_in_progress()),
            TodoStatus::Pending => ("○", theme::todo_pending()),
        };

        let title_style = if item.status == TodoStatus::Done {
            Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::CROSSED_OUT)
        } else {
            theme::text_primary()
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
                .border_style(theme::border_inactive()),
        )
        .style(Style::default().bg(theme::BG_SIDEBAR));

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
