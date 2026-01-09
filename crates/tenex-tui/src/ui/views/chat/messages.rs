use crate::models::Message;
use crate::ui::markdown::render_markdown;
use crate::ui::theme;
use crate::ui::tool_calls::{parse_message_content, render_tool_line, MessageContent};
use crate::ui::views::render_inline_ask_lines;
use crate::ui::{App, InputMode};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
    Frame,
};
use std::collections::{HashMap, HashSet};
use tracing::info_span;

use super::cards::{author_line, dot_line, markdown_lines, pad_line};
use super::grouping::{group_messages, DisplayItem};

pub(crate) fn render_messages_panel(
    f: &mut Frame,
    app: &mut App,
    messages_area: Rect,
    all_messages: &[Message],
) {
    // Get thread_id first - needed for reply index filtering
    let thread_id = app.selected_thread.as_ref().map(|t| t.id.as_str());

    // Build reply index: parent_id -> Vec<&Message>
    // Skip messages that e-tag the thread root - those are siblings, not nested replies
    let mut replies_by_parent: HashMap<&str, Vec<&Message>> = HashMap::new();
    for msg in all_messages {
        if let Some(ref parent_id) = msg.reply_to {
            // Only count as a reply if parent is NOT the thread root
            if Some(parent_id.as_str()) != thread_id {
                replies_by_parent.entry(parent_id.as_str()).or_default().push(msg);
            }
        }
    }
    let display_messages: Vec<&Message> = if let Some(ref root_id) = app.subthread_root {
        // Subthread view: show messages that reply directly to the root
        all_messages
            .iter()
            .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
            .collect()
    } else {
        // Main view: show messages with no parent or parent = thread root
        // Exclude the thread itself - it's rendered separately above
        all_messages
            .iter()
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

                messages_text.push(author_line(&author, indicator_color, card_bg, content_width));

                // Content with markdown
                let rendered = render_markdown(&thread.content);
                messages_text.extend(markdown_lines(
                    &rendered,
                    indicator_color,
                    card_bg,
                    content_width,
                ));

                messages_text.push(Line::from(""));
            }
        }
    }

    // If in subthread, render the root message first as a header
    if let Some(ref root_msg) = app.subthread_root_message {
        let author = app.data_store.borrow().get_profile_name(&root_msg.pubkey);
        messages_text.push(Line::from(Span::styled(
            format!("{}{} :", padding, author),
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
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
            messages_text.push(Line::from(Span::styled(
                format!("{}...", padding),
                Style::default().fg(theme::TEXT_MUTED),
            )));
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
            let mut pubkeys: HashSet<&str> = HashSet::new();
            for msg in all_messages {
                pubkeys.insert(&msg.pubkey);
            }

            // Build profile name cache
            let cache: HashMap<String, String> = pubkeys
                .into_iter()
                .map(|pk| (pk.to_string(), store.get_profile_name(pk)))
                .collect();

            (user_pk, cache)
        };

        // Group consecutive action messages
        let grouped = group_messages(&display_messages, user_pubkey.as_deref());

        for (group_idx, item) in grouped.iter().enumerate() {
            match item {
                DisplayItem::ActionGroup {
                    messages: action_msgs,
                    pubkey,
                    is_consecutive,
                    has_next_consecutive,
                } => {
                    // Render collapsed action group
                    let indicator_color = theme::user_color(pubkey);
                    let author = profile_cache
                        .get(pubkey)
                        .cloned()
                        .unwrap_or_else(|| pubkey[..8.min(pubkey.len())].to_string());

                    // Show header only if not consecutive (first in a sequence from this author)
                    if !is_consecutive {
                        messages_text.push(Line::from(vec![
                            Span::styled("│ ", Style::default().fg(indicator_color)),
                            Span::styled(
                                author,
                                Style::default()
                                    .fg(indicator_color)
                                    .add_modifier(Modifier::BOLD),
                            ),
                        ]));
                    }

                    // Collapsed summary line with dot indicator if consecutive
                    let count = action_msgs.len();
                    let first_action: String = action_msgs
                        .first()
                        .map(|m| m.content.trim().chars().take(30).collect())
                        .unwrap_or_default();
                    let last_action: String = action_msgs
                        .last()
                        .map(|m| m.content.trim().chars().take(30).collect())
                        .unwrap_or_default();

                    // Use dot indicator for consecutive messages, regular indicator otherwise
                    let indicator = if *is_consecutive { "· " } else { "│ " };
                    messages_text.push(Line::from(vec![
                        Span::styled(indicator, Style::default().fg(indicator_color)),
                        Span::styled("▸ ", Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(
                            format!("{} actions", count),
                            Style::default()
                                .fg(theme::ACCENT_SPECIAL)
                                .add_modifier(Modifier::BOLD),
                        ),
                        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(first_action, Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(" → ", Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(last_action, Style::default().fg(theme::TEXT_MUTED)),
                    ]));

                    // Only add blank line if no next consecutive (end of author group)
                    if !has_next_consecutive {
                        messages_text.push(Line::from(""));
                    }
                }

                DisplayItem::SingleMessage {
                    message: msg,
                    is_consecutive,
                    has_next_consecutive,
                } => {
                    let _msg_span = info_span!("render_message", index = group_idx).entered();

                    // Check if this message is selected (for navigation)
                    let is_selected =
                        group_idx == app.selected_message_index && app.input_mode == InputMode::Normal;

                    let author = profile_cache
                        .get(&msg.pubkey)
                        .cloned()
                        .unwrap_or_else(|| msg.pubkey[..8.min(msg.pubkey.len())].to_string());

                    // === OPENCODE-STYLE CARD ===
                    // - Left indicator line (deterministic color from pubkey)
                    // - Full-width shaded background
                    // - Author on first line (only for first in consecutive group), content below

                    let indicator_color = theme::user_color(&msg.pubkey);
                    let card_bg = theme::BG_CARD;
                    let card_bg_selected = theme::BG_SELECTED;
                    let bg = if is_selected { card_bg_selected } else { card_bg };

                    // First line: author header OR dot indicator for consecutive messages
                    if *is_consecutive {
                        messages_text.push(dot_line(indicator_color, bg, content_width));
                    } else {
                        messages_text.push(author_line(
                            &author,
                            indicator_color,
                            bg,
                            content_width,
                        ));
                    }

                    // Content with markdown
                    let parsed = parse_message_content(&msg.content);
                    let content_text = match &parsed {
                        MessageContent::PlainText(text) => text.clone(),
                        MessageContent::Mixed { text_parts, .. } => text_parts.join("\n"),
                    };
                    let rendered = render_markdown(&content_text);

                    // Content lines: indicator + content with background (padded)
                    messages_text.extend(markdown_lines(
                        &rendered,
                        indicator_color,
                        bg,
                        content_width,
                    ));

                    // Ask event - only render if NOT answered by current user
                    if msg.ask_event.is_some() && !app.is_ask_answered_by_user(&msg.id) {
                        // Unanswered question - render full inline ask UI
                        // The modal should be auto-opened (handled at start of render)
                        if let Some(modal_state) = app.ask_modal_state() {
                            if modal_state.message_id == msg.id {
                                let ask_lines = render_inline_ask_lines(
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

                    // Tool calls (with tool-specific rendering, no background)
                    if let MessageContent::Mixed { tool_calls, .. } = &parsed {
                        for tool_call in tool_calls {
                            messages_text.push(render_tool_line(tool_call, indicator_color));
                        }
                    }

                    // Replies indicator
                    if let Some(replies) = replies_by_parent.get(msg.id.as_str()) {
                        if !replies.is_empty() {
                            let replies_text = format!("{} replies", replies.len());
                            let replies_len = 7 + replies_text.len(); // "│  └→ " + text
                            let mut replies_spans = vec![
                                Span::styled(
                                    "│",
                                    Style::default().fg(indicator_color).bg(bg),
                                ),
                                Span::styled(
                                    "  └→ ",
                                    Style::default().fg(theme::TEXT_MUTED).bg(bg),
                                ),
                                Span::styled(
                                    replies_text,
                                    Style::default().fg(theme::ACCENT_SPECIAL).bg(bg),
                                ),
                            ];
                            pad_line(&mut replies_spans, replies_len, content_width, bg);
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
                    Style::default()
                        .fg(theme::ACCENT_SPECIAL)
                        .add_modifier(Modifier::BOLD),
                ),
                Span::styled(
                    " (streaming)",
                    Style::default()
                        .fg(theme::ACCENT_SPECIAL)
                        .add_modifier(Modifier::ITALIC),
                ),
            ]));

            // Render reasoning content first if present (muted style)
            if !buffer.reasoning_content.is_empty() {
                let reasoning_style =
                    Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::ITALIC);
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
                    let mut line_spans =
                        vec![Span::styled("│ ", Style::default().fg(theme::ACCENT_SPECIAL))];
                    line_spans.extend(line.spans.clone());

                    // Add cursor indicator at the end of the last line
                    if i == markdown_lines.len() - 1 && !buffer.is_complete {
                        line_spans.push(Span::styled(
                            "▌",
                            Style::default().fg(theme::ACCENT_SPECIAL),
                        ));
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
}
