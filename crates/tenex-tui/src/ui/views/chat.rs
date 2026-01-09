use crate::models::Message;
use crate::ui::components::{
    modal_area, render_modal_background, render_modal_header, render_modal_items,
    render_modal_overlay, render_modal_search, render_tab_bar, render_todo_sidebar, ModalItem,
    ModalSize,
};
use crate::ui::markdown::render_markdown;
use crate::ui::theme;
use crate::ui::todo::aggregate_todo_state;
use crate::ui::tool_calls::{extract_target, parse_message_content, tool_icon, MessageContent};
use crate::ui::views::chat_grouping::{group_messages, DisplayItem};
use crate::ui::{App, InputMode, ModalState};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
    Frame,
};
use std::collections::HashMap;
use tracing::info_span;

pub fn render_chat(f: &mut Frame, app: &mut App, area: Rect) {
    // Fill entire area with app background (pure black)
    let bg_block = Block::default().style(Style::default().bg(theme::BG_APP));
    f.render_widget(bg_block, area);

    let all_messages = app.messages();
    let _render_span = info_span!("render_chat", message_count = all_messages.len()).entered();

    // Aggregate todo state from all messages
    let todo_state = aggregate_todo_state(&all_messages);

    // Auto-open ask modal for first unanswered question (only when no modal is active)
    if matches!(app.modal_state, ModalState::None) {
        if let Some((msg_id, ask_event)) = app.has_unanswered_ask_event() {
            app.open_ask_modal(msg_id, ask_event);
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

        for (group_idx, item) in grouped.iter().enumerate() {
            match item {
                DisplayItem::ActionGroup { messages: action_msgs, pubkey, is_consecutive, has_next_consecutive } => {
                    // Render collapsed action group
                    let indicator_color = theme::user_color(pubkey);
                    let author = profile_cache.get(pubkey).cloned().unwrap_or_else(|| pubkey[..8.min(pubkey.len())].to_string());

                    // Show header only if not consecutive (first in a sequence from this author)
                    if !is_consecutive {
                        messages_text.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(indicator_color)),
                            Span::styled(
                                author,
                                Style::default().fg(indicator_color).add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    }

                    // Collapsed summary line with dot indicator if consecutive
                    let count = action_msgs.len();
                    let first_action: String = action_msgs.first()
                        .map(|m| m.content.trim().chars().take(30).collect())
                        .unwrap_or_default();
                    let last_action: String = action_msgs.last()
                        .map(|m| m.content.trim().chars().take(30).collect())
                        .unwrap_or_default();

                    // Use dot indicator for consecutive messages, regular indicator otherwise
                    let indicator = if *is_consecutive { "· " } else { "│ " };
                    messages_text.push(Line::from(vec![
                        Span::styled(indicator, Style::default().fg(indicator_color)),
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

                    // Only add blank line if no next consecutive (end of author group)
                    if !has_next_consecutive {
                        messages_text.push(Line::from(""));
                    }
                }

                DisplayItem::SingleMessage { message: msg, is_consecutive, has_next_consecutive } => {
                    let _msg_span = info_span!("render_message", index = group_idx).entered();

                    // Check if this message is selected (for navigation)
                    let is_selected = group_idx == app.selected_message_index && app.input_mode == InputMode::Normal;

                    let author = profile_cache.get(&msg.pubkey).cloned()
                        .unwrap_or_else(|| msg.pubkey[..8.min(msg.pubkey.len())].to_string());

                    // === OPENCODE-STYLE CARD ===
                    // - Left indicator line (deterministic color from pubkey)
                    // - Full-width shaded background
                    // - Author on first line (only for first in consecutive group), content below

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

                    // First line: author header OR dot indicator for consecutive messages
                    if *is_consecutive {
                        // Consecutive message: show dot indicator instead of full author
                        let dot_len = 2; // "· "
                        let mut dot_spans = vec![
                            Span::styled("·", Style::default().fg(indicator_color).bg(bg)),
                            Span::styled(" ", Style::default().bg(bg)),
                        ];
                        pad_line(&mut dot_spans, dot_len);
                        messages_text.push(Line::from(dot_spans));
                    } else {
                        // First in sequence: show full author header
                        let author_len = 2 + author.len(); // "│ " + author
                        let mut author_spans = vec![
                            Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                            Span::styled(" ", Style::default().bg(bg)),
                            Span::styled(author.clone(), Style::default().fg(indicator_color).add_modifier(Modifier::BOLD).bg(bg)),
                        ];
                        pad_line(&mut author_spans, author_len);
                        messages_text.push(Line::from(author_spans));
                    }

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
                        if let Some(modal_state) = app.ask_modal_state() {
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

                    // Tool calls - muted text, no background
                    if let MessageContent::Mixed { tool_calls, .. } = &parsed {
                        for tool_call in tool_calls {
                            let icon = tool_icon(&tool_call.name);
                            let target = extract_target(tool_call).unwrap_or_default();
                            messages_text.push(Line::from(vec![
                                Span::styled("  ", Style::default()),
                                Span::styled(icon, Style::default().fg(theme::TEXT_MUTED)),
                                Span::styled(" ", Style::default()),
                                Span::styled(tool_call.name.to_uppercase(), Style::default().fg(theme::TEXT_MUTED)),
                                Span::styled(" ", Style::default()),
                                Span::styled(target, Style::default().fg(theme::TEXT_MUTED)),
                            ]));
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

                    // Only add empty line if no next consecutive (end of author group)
                    if !has_next_consecutive {
                        messages_text.push(Line::from(""));
                    }
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
    let is_input_active = app.input_mode == InputMode::Editing && !matches!(app.modal_state, ModalState::AskModal(_));

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
        // Render each line of input with padding, wrapping long lines
        for line in input_text.lines() {
            // Wrap long lines to fit within content width
            let mut remaining = line;
            while !remaining.is_empty() {
                let (chunk, rest) = if remaining.len() > input_content_width {
                    // Find a safe UTF-8 boundary
                    let mut split_at = input_content_width;
                    while split_at > 0 && !remaining.is_char_boundary(split_at) {
                        split_at -= 1;
                    }
                    if split_at == 0 {
                        split_at = remaining.len().min(input_content_width);
                    }
                    (&remaining[..split_at], &remaining[split_at..])
                } else {
                    (remaining, "")
                };

                let pad = input_content_width.saturating_sub(chunk.len());
                input_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
                    Span::styled("  ", Style::default().bg(input_bg)), // 2-char left padding
                    Span::styled(chunk.to_string(), Style::default().fg(text_color).bg(input_bg)),
                    Span::styled(" ".repeat(pad + 2), Style::default().bg(input_bg)), // +2 right padding
                ]));
                remaining = rest;
            }
        }
        // Handle case where input ends with newline or is single line
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
    if is_input_active && !app.is_attachment_modal_open() {
        // Calculate visual cursor position accounting for line wrapping
        let cursor_pos = app.chat_editor.cursor;
        let text = app.chat_editor.text.as_str();
        let before_cursor = &text[..cursor_pos.min(text.len())];

        // Handle cursor at end of text with trailing content
        let last_line_start = before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col_in_last_line = cursor_pos - last_line_start;
        let visual_row = before_cursor.matches('\n').count() + col_in_last_line / input_content_width.max(1);
        let visual_col = col_in_last_line % input_content_width.max(1);

        f.set_cursor_position((
            input_area.x + visual_col as u16 + 3, // +3 for "│  "
            input_area.y + visual_row as u16 + 1, // +1 for top padding
        ));
    }
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

fn render_agent_selector(f: &mut Frame, app: &App, area: Rect) {
    // Dim the background
    render_modal_overlay(f, area);

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
    // Dim the background
    render_modal_overlay(f, area);

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
