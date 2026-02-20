use crate::models::Message;
use crate::ui::markdown::render_markdown;
use crate::ui::theme;
use crate::ui::tool_calls::{parse_message_content, render_tool_line, MessageContent, ToolCall};
use crate::ui::views::render_inline_ask_lines;
use crate::ui::{App, InputMode};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use std::collections::HashMap;

use super::cards::{
    author_line, author_line_with_recipient, bottom_half_block_line, dot_line, llm_metadata_line,
    markdown_lines, pad_line, reasoning_author_line, reasoning_dot_line, reasoning_lines,
    top_half_block_line,
};
use super::grouping::{group_messages, DisplayItem};

/// Result of computing message background styling policy.
/// Centralizes the decision of whether a message gets a "card" appearance.
struct MessageBackgroundPolicy {
    /// The background color to use for this message
    bg: ratatui::style::Color,
    /// Whether this message should render with card styling (half-block borders, padding)
    /// True for: search matches, selected messages, or messages with p-tags (recipients)
    has_card_bg: bool,
}

/// Compute the background styling policy for a message.
///
/// # Background Priority (highest to lowest):
/// 1. Current search match → BG_SEARCH_CURRENT (card styling)
/// 2. Any search match → BG_SEARCH_MATCH (card styling)
/// 3. Selected message → BG_SELECTED (card styling)
/// 4. Has p-tags (directed to recipients) → BG_CARD (card styling)
/// 5. Default → BG_APP (no card styling)
///
/// Card styling means the message gets visual "card" treatment with
/// top/bottom half-block borders and full-width background padding.
fn compute_message_background_policy(
    has_p_tags: bool,
    is_selected: bool,
    has_search_match: bool,
    is_current_search: bool,
    card_bg: ratatui::style::Color,
    card_bg_selected: ratatui::style::Color,
) -> MessageBackgroundPolicy {
    let bg = if is_current_search {
        theme::BG_SEARCH_CURRENT
    } else if has_search_match {
        theme::BG_SEARCH_MATCH
    } else if is_selected {
        card_bg_selected
    } else if has_p_tags {
        card_bg
    } else {
        theme::BG_APP
    };

    let has_card_bg = has_p_tags || is_selected || has_search_match || is_current_search;

    MessageBackgroundPolicy { bg, has_card_bg }
}

/// Render markdown content with a custom indicator and padding.
/// Used for ask event context rendering in both answered and unanswered paths.
fn render_markdown_with_indicator(
    content: &str,
    indicator: &str,
    indicator_color: ratatui::style::Color,
    pad: &str,
    bg: ratatui::style::Color,
    content_width: usize,
) -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    let context_rendered = render_markdown(content);
    for md_line in context_rendered.iter() {
        let mut spans = vec![
            Span::styled(
                indicator.to_string(),
                Style::default().fg(indicator_color).bg(bg),
            ),
            Span::styled(pad.to_string(), Style::default().bg(bg)),
        ];
        for span in md_line.spans.iter() {
            let mut new_span = span.clone();
            new_span.style = new_span.style.bg(bg);
            spans.push(new_span);
        }
        let line_len: usize = md_line
            .spans
            .iter()
            .map(|s| s.content.chars().count())
            .sum();
        pad_line(&mut spans, 1 + pad.len() + line_len, content_width, bg);
        lines.push(Line::from(spans));
    }
    lines
}

pub(crate) fn render_messages_panel(
    f: &mut Frame,
    app: &mut App,
    messages_area: Rect,
    all_messages: &[Message],
) {
    // Get thread_id first - needed for reply index filtering
    let thread_id = app.selected_thread().map(|t| t.id.as_str());

    // Build reply index: parent_id -> Vec<&Message>
    // Skip messages that e-tag the thread root - those are siblings, not nested replies
    let mut replies_by_parent: HashMap<&str, Vec<&Message>> = HashMap::new();
    for msg in all_messages {
        if let Some(ref parent_id) = msg.reply_to {
            // Only count as a reply if parent is NOT the thread root
            if Some(parent_id.as_str()) != thread_id {
                replies_by_parent
                    .entry(parent_id.as_str())
                    .or_default()
                    .push(msg);
            }
        }
    }
    let subthread_root = app.subthread_root().cloned();
    let display_messages: Vec<&Message> = if let Some(ref root_id) = subthread_root {
        // Subthread view: show messages that reply directly to the root
        all_messages
            .iter()
            .filter(|m| m.reply_to.as_deref() == Some(root_id.as_str()))
            .collect()
    } else {
        // Main view: thread root + messages with no parent or parent = thread root
        all_messages
            .iter()
            .filter(|m| {
                // Include thread root (id == thread_id) + direct replies
                Some(m.id.as_str()) == thread_id
                    || m.reply_to.is_none()
                    || m.reply_to.as_deref() == thread_id
            })
            .collect()
    };

    // Messages area
    let mut messages_text: Vec<Line> = Vec::new();

    // Left padding for message content
    let padding = "   ";
    // Full width - wrapping is handled in cards.rs markdown_lines()
    let content_width = messages_area.width as usize;

    // If in subthread, render the root message first as a header
    if let Some(root_msg) = app.subthread_root_message() {
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

    // Track line positions for each display item (for scroll-to-selection)
    let mut item_line_starts: Vec<usize> = Vec::new();

    // Render display messages with card-style layout
    {
        // Collect all unique pubkeys and cache profile names with single borrow
        let profile_cache = {
            let store = app.data_store.borrow();

            // Collect unique pubkeys from ALL messages (includes replies not in display)
            let mut pubkeys: std::collections::HashSet<&str> = std::collections::HashSet::new();
            for msg in all_messages {
                pubkeys.insert(&msg.pubkey);
            }

            // Build profile name cache
            let cache: HashMap<String, String> = pubkeys
                .into_iter()
                .map(|pk| (pk.to_string(), store.get_profile_name(pk)))
                .collect();

            cache
        };

        // Convert messages to display items - each message is its own item
        let grouped = group_messages(&display_messages);

        // Reserve capacity for line tracking
        item_line_starts.reserve(grouped.len());

        for (group_idx, item) in grouped.iter().enumerate() {
            // Record the starting line for this item
            item_line_starts.push(messages_text.len());
            match item {
                DisplayItem::SingleMessage {
                    message: msg,
                    is_consecutive,
                    has_next_consecutive,
                } => {
                    // Check if this message is selected (for navigation)
                    let is_selected = group_idx == app.selected_message_index()
                        && app.input_mode == InputMode::Normal;

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

                    // Check if this message has a search match (per-tab isolated)
                    let has_search_match = app.message_has_search_match(&msg.id);
                    let is_current_search = app
                        .chat_search()
                        .filter(|s| s.active)
                        .and_then(|s| s.match_locations.get(s.current_match))
                        .map(|loc| loc.message_id == msg.id)
                        .unwrap_or(false);

                    // Check if this is a tool use message (has tool tag) or delegation (has q_tags)
                    let is_tool_use = msg.tool_name.is_some() || !msg.q_tags.is_empty();
                    // Check if message has p-tags (recipients) - only these get card background by default
                    let has_p_tags = !msg.p_tags.is_empty();

                    // Compute background policy once for the entire message rendering
                    let bg_policy = compute_message_background_policy(
                        has_p_tags,
                        is_selected,
                        has_search_match,
                        is_current_search,
                        card_bg,
                        card_bg_selected,
                    );
                    let bg = bg_policy.bg;

                    if is_tool_use {
                        // Tool use message: render muted tool line only, no card background
                        let tool_name = msg.tool_name.as_deref().unwrap_or("tool");

                        // Try to construct ToolCall from tool_args tag first (preferred)
                        let tool_call_from_args = msg.tool_args.as_ref().and_then(|args_json| {
                            serde_json::from_str::<serde_json::Value>(args_json)
                                .ok()
                                .map(|params| ToolCall {
                                    id: String::new(),
                                    name: tool_name.to_string(),
                                    parameters: params,
                                    result: None,
                                })
                        });

                        if let Some(tool_call) = tool_call_from_args {
                            // Render using the structured tool_args data
                            // Pass content as fallback for unknown tools
                            messages_text.push(render_tool_line(
                                &tool_call,
                                indicator_color,
                                Some(&msg.content),
                            ));
                        } else {
                            // Fallback: parse content for embedded JSON tool calls
                            let parsed = parse_message_content(&msg.content);

                            if let MessageContent::Mixed { tool_calls, .. } = &parsed {
                                for tool_call in tool_calls {
                                    messages_text.push(render_tool_line(
                                        tool_call,
                                        indicator_color,
                                        Some(&msg.content),
                                    ));
                                }
                            } else {
                                // Final fallback: show tool name with content preview
                                let content_preview: String =
                                    msg.content.chars().take(50).collect();
                                messages_text.push(Line::from(vec![
                                    Span::styled("│", Style::default().fg(indicator_color)),
                                    Span::raw("  "),
                                    Span::styled(
                                        format!("{}: {}", tool_name, content_preview),
                                        Style::default().fg(theme::TEXT_MUTED),
                                    ),
                                ]));
                            }
                        }
                    } else if msg.is_reasoning {
                        // Reasoning/thinking message: muted style, no background
                        if *is_consecutive {
                            messages_text.push(reasoning_dot_line(indicator_color));
                        } else {
                            messages_text.push(reasoning_author_line(&author, indicator_color));
                        }

                        // Content with markdown (muted style)
                        let parsed = parse_message_content(&msg.content);
                        let content_text = match &parsed {
                            MessageContent::PlainText(text) => text.clone(),
                            MessageContent::Mixed { text_parts, .. } => text_parts.join("\n"),
                        };
                        let rendered = render_markdown(&content_text);

                        // Content lines: muted style, no background
                        messages_text.extend(reasoning_lines(
                            &rendered,
                            indicator_color,
                            content_width,
                        ));
                    } else {
                        // Non-tool message: render full card with background

                        // Determine if this message starts a new visual card
                        // (first in group, or has p-tags which show full header breaking the visual grouping)
                        let starts_new_card = !is_consecutive || has_p_tags;

                        // Top half-block padding for new cards (only if message has card background)
                        if starts_new_card && bg_policy.has_card_bg {
                            messages_text.push(top_half_block_line(
                                indicator_color,
                                bg,
                                content_width,
                            ));
                        }

                        // First line: author header OR dot indicator for consecutive messages
                        // Always show header with recipient if message has p-tags
                        if has_p_tags {
                            // Resolve p-tag pubkeys to display names
                            let recipient_names: Vec<String> = {
                                let store = app.data_store.borrow();
                                msg.p_tags
                                    .iter()
                                    .map(|pk| store.get_profile_name(pk))
                                    .collect()
                            };
                            messages_text.push(author_line_with_recipient(
                                &author,
                                &recipient_names,
                                indicator_color,
                                bg,
                                content_width,
                            ));
                        } else if *is_consecutive {
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

                        // Tool calls from content (with tool-specific rendering, no background)
                        if let MessageContent::Mixed { tool_calls, .. } = &parsed {
                            for tool_call in tool_calls {
                                messages_text.push(render_tool_line(
                                    tool_call,
                                    indicator_color,
                                    Some(&msg.content),
                                ));
                            }
                        }
                    }

                    // Debug info: shown when selected/setting enabled with llm_metadata
                    let show_debug_for_llm =
                        (is_selected || app.show_llm_metadata()) && !msg.llm_metadata.is_empty();

                    if show_debug_for_llm {
                        // LLM metadata line (id + time + token info)
                        messages_text.push(llm_metadata_line(
                            &msg.id,
                            msg.created_at,
                            &msg.llm_metadata,
                            indicator_color,
                            bg,
                            content_width,
                        ));
                    }

                    // Replies indicator
                    if let Some(replies) = replies_by_parent.get(msg.id.as_str()) {
                        if !replies.is_empty() {
                            let replies_text = format!("{} replies", replies.len());
                            let replies_len = 7 + replies_text.len(); // "│  └→ " + text
                            let mut replies_spans = vec![
                                Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
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

                    // Bottom half-block padding (for messages that end a visual card)
                    // This includes: last in group, or p-tag messages (which form their own card)
                    let ends_card = !has_next_consecutive || has_p_tags;
                    if ends_card && bg_policy.has_card_bg {
                        // Only add half-block for non-tool, non-reasoning messages (those have card bg)
                        if !is_tool_use && !msg.is_reasoning {
                            messages_text.push(bottom_half_block_line(
                                indicator_color,
                                bg,
                                content_width,
                            ));
                        }
                    }
                }

                DisplayItem::DelegationPreview {
                    thread_id,
                    parent_pubkey,
                    is_consecutive,
                    has_next_consecutive,
                } => {
                    // Check if this delegation preview is selected
                    let is_selected = group_idx == app.selected_message_index()
                        && app.input_mode == InputMode::Normal;

                    let indicator_color = theme::user_color(parent_pubkey);
                    let card_bg = theme::BG_CARD;
                    let card_bg_selected = theme::BG_SELECTED;
                    let bg = if is_selected {
                        card_bg_selected
                    } else {
                        card_bg
                    };

                    let indicator = if *is_consecutive { "·  " } else { "│  " };

                    // Check if this is an ask event (q-tag pointing to ask event instead of thread)
                    // Like Svelte's DelegationPreview, inline ask events instead of showing delegation card
                    let ask_event_data = {
                        let store = app.data_store.borrow();
                        store.get_ask_event_by_id(thread_id)
                    };

                    if let Some((ask_event, ask_pubkey)) = ask_event_data {
                        // This is an ask event - render inline ask UI
                        let ask_answered = app.is_ask_answered_by_user(thread_id);

                        if ask_answered {
                            // Answered ask with visual hierarchy like Svelte
                            let title_text = ask_event
                                .title
                                .clone()
                                .unwrap_or_else(|| "Question".to_string());
                            let pad = "   "; // 3-space padding for content
                            let response_bg = theme::BG_CARD; // Distinct background for response box

                            // === QUESTION SECTION ===
                            // Title (bold, with padding)
                            let mut title_spans = vec![
                                Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                ),
                                Span::styled(pad, Style::default().bg(bg)),
                                Span::styled(
                                    title_text.clone(),
                                    Style::default()
                                        .fg(theme::TEXT_PRIMARY)
                                        .add_modifier(Modifier::BOLD)
                                        .bg(bg),
                                ),
                            ];
                            pad_line(
                                &mut title_spans,
                                1 + pad.len() + title_text.len(),
                                content_width,
                                bg,
                            );
                            messages_text.push(Line::from(title_spans));

                            // Context (with padding)
                            if !ask_event.context.is_empty() {
                                // Empty line after title
                                let mut empty1 = vec![Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                )];
                                pad_line(&mut empty1, 1, content_width, bg);
                                messages_text.push(Line::from(empty1));

                                let context_lines = render_markdown_with_indicator(
                                    &ask_event.context,
                                    indicator,
                                    indicator_color,
                                    pad,
                                    bg,
                                    content_width,
                                );
                                messages_text.extend(context_lines);
                            }

                            // Empty line before status
                            let mut empty2 = vec![Span::styled(
                                indicator,
                                Style::default().fg(indicator_color).bg(bg),
                            )];
                            pad_line(&mut empty2, 1, content_width, bg);
                            messages_text.push(Line::from(empty2));

                            // Status line: ✓ Response submitted
                            let status_text = "✓ Response submitted";
                            let mut status_spans = vec![
                                Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                ),
                                Span::styled(pad, Style::default().bg(bg)),
                                Span::styled(
                                    status_text,
                                    Style::default().fg(theme::ACCENT_SUCCESS).bg(bg),
                                ),
                            ];
                            pad_line(
                                &mut status_spans,
                                1 + pad.len() + status_text.len(),
                                content_width,
                                bg,
                            );
                            messages_text.push(Line::from(status_spans));

                            // === RESPONSE SECTION (boxed with different background) ===
                            if let Some(response) = app.get_user_response_to_ask(thread_id) {
                                // Empty line before response box
                                let mut empty3 = vec![Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                )];
                                pad_line(&mut empty3, 1, content_width, bg);
                                messages_text.push(Line::from(empty3));

                                // Top border of response box
                                let box_width = content_width.saturating_sub(4); // Account for indicator + padding
                                let top_border =
                                    format!("┌{}┐", "─".repeat(box_width.saturating_sub(2)));
                                let mut top_spans = vec![
                                    Span::styled(
                                        indicator,
                                        Style::default().fg(indicator_color).bg(bg),
                                    ),
                                    Span::styled(pad, Style::default().bg(bg)),
                                    Span::styled(
                                        top_border.clone(),
                                        Style::default().fg(theme::TEXT_MUTED).bg(bg),
                                    ),
                                ];
                                pad_line(
                                    &mut top_spans,
                                    1 + pad.len() + top_border.len(),
                                    content_width,
                                    bg,
                                );
                                messages_text.push(Line::from(top_spans));

                                // Response content with left border and background
                                let rendered = render_markdown(&response);
                                for md_line in rendered.iter() {
                                    let mut spans = vec![
                                        Span::styled(
                                            indicator,
                                            Style::default().fg(indicator_color).bg(bg),
                                        ),
                                        Span::styled(pad, Style::default().bg(bg)),
                                        Span::styled(
                                            "│ ",
                                            Style::default().fg(theme::TEXT_MUTED).bg(response_bg),
                                        ),
                                    ];
                                    let mut content_len = 0;
                                    for span in md_line.spans.iter() {
                                        let mut new_span = span.clone();
                                        new_span.style = new_span.style.bg(response_bg);
                                        content_len += new_span.content.len();
                                        spans.push(new_span);
                                    }
                                    // Pad inside the box and add right border
                                    let inner_pad = box_width.saturating_sub(4 + content_len);
                                    spans.push(Span::styled(
                                        " ".repeat(inner_pad),
                                        Style::default().bg(response_bg),
                                    ));
                                    spans.push(Span::styled(
                                        " │",
                                        Style::default().fg(theme::TEXT_MUTED).bg(response_bg),
                                    ));
                                    // Pad rest of line
                                    pad_line(&mut spans, content_width, content_width, bg);
                                    messages_text.push(Line::from(spans));
                                }

                                // Bottom border
                                let bottom_border =
                                    format!("└{}┘", "─".repeat(box_width.saturating_sub(2)));
                                let mut bottom_spans = vec![
                                    Span::styled(
                                        indicator,
                                        Style::default().fg(indicator_color).bg(bg),
                                    ),
                                    Span::styled(pad, Style::default().bg(bg)),
                                    Span::styled(
                                        bottom_border.clone(),
                                        Style::default().fg(theme::TEXT_MUTED).bg(bg),
                                    ),
                                ];
                                pad_line(
                                    &mut bottom_spans,
                                    1 + pad.len() + bottom_border.len(),
                                    content_width,
                                    bg,
                                );
                                messages_text.push(Line::from(bottom_spans));
                            }
                        } else {
                            // Render ask event title/context
                            let title_text = ask_event
                                .title
                                .clone()
                                .unwrap_or_else(|| "Question".to_string());
                            let agent_name = {
                                let store = app.data_store.borrow();
                                store.get_profile_name(&ask_pubkey)
                            };

                            // Header line with title
                            let mut header_spans = vec![
                                Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                ),
                                Span::styled(
                                    format!("@{} ", agent_name),
                                    Style::default().fg(theme::TEXT_MUTED).bg(bg),
                                ),
                                Span::styled(
                                    title_text.clone(),
                                    Style::default()
                                        .fg(theme::TEXT_PRIMARY)
                                        .add_modifier(Modifier::BOLD)
                                        .bg(bg),
                                ),
                            ];
                            let header_len = 1 + agent_name.len() + 2 + title_text.len();
                            pad_line(&mut header_spans, header_len, content_width, bg);
                            messages_text.push(Line::from(header_spans));

                            // Context (with proper markdown rendering, not truncated)
                            if !ask_event.context.is_empty() {
                                // Empty line after title
                                let mut empty1 = vec![Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                )];
                                pad_line(&mut empty1, 1, content_width, bg);
                                messages_text.push(Line::from(empty1));

                                let pad = "   "; // 3-space padding for content
                                let context_lines = render_markdown_with_indicator(
                                    &ask_event.context,
                                    indicator,
                                    indicator_color,
                                    pad,
                                    bg,
                                    content_width,
                                );
                                messages_text.extend(context_lines);
                            }

                            // Render inline ask UI if modal is open for this event
                            if let Some(modal_state) = app.ask_modal_state() {
                                if modal_state.message_id == *thread_id {
                                    let ask_lines = render_inline_ask_lines(
                                        modal_state,
                                        indicator_color,
                                        bg,
                                        content_width,
                                    );
                                    messages_text.extend(ask_lines);
                                }
                            }
                        }
                    } else {
                        // Not an ask event - show normal delegation card
                        // Get thread info from data store
                        // NOTE: We get branch from the delegated thread's root message,
                        // NOT from the delegation tool-call event (which has the wrong branch)
                        let (
                            title,
                            from_agent_name,
                            to_agent_name,
                            status,
                            activity,
                            is_busy,
                            thread_branch,
                        ) = {
                            let store = app.data_store.borrow();
                            // Check if any agents are working on this delegation
                            let is_busy = store.operations.is_event_busy(thread_id);
                            // Get the "from" agent name (who made the delegation call)
                            let from_name = store.get_profile_name(parent_pubkey);
                            if let Some(t) = store.get_thread_by_id(thread_id) {
                                let title = if t.title == "Untitled" || t.title.is_empty() {
                                    t.content.chars().take(50).collect::<String>()
                                } else {
                                    t.title.clone()
                                };
                                let messages = store.get_messages(thread_id);
                                let activity =
                                    t.status_current_activity.clone().unwrap_or_else(|| {
                                        messages
                                            .last()
                                            .map(|m| m.content.chars().take(60).collect())
                                            .unwrap_or_default()
                                    });
                                // Get branch from the root message of the delegated thread (by ID, not ordering)
                                let root_branch = messages
                                    .iter()
                                    .find(|m| m.id == *thread_id)
                                    .and_then(|m| m.branch.clone());
                                // Get the "to" agent name from the thread's p_tags (who was delegated TO)
                                // If no p_tags, fall back to the thread creator's pubkey
                                let to_name = if !t.p_tags.is_empty() {
                                    store.get_profile_name(&t.p_tags[0])
                                } else {
                                    store.get_profile_name(&t.pubkey)
                                };
                                (
                                    title,
                                    from_name,
                                    to_name,
                                    t.status_label.clone(),
                                    activity,
                                    is_busy,
                                    root_branch,
                                )
                            } else {
                                (
                                    format!("delegation: {}", &thread_id[..8.min(thread_id.len())]),
                                    from_name,
                                    String::new(),
                                    None,
                                    String::new(),
                                    is_busy,
                                    None,
                                )
                            }
                        };

                        if to_agent_name.is_empty() {
                            // Thread not found - show ID only
                            messages_text.push(Line::from(vec![
                                Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                ),
                                Span::styled(
                                    format!("→ {}", title),
                                    Style::default().fg(theme::TEXT_MUTED).bg(bg),
                                ),
                            ]));
                        } else {
                            // Delegation card with full box border
                            // Card inner width (excluding indicator and box borders)
                            let card_inner_width = content_width.saturating_sub(3 + 2 + 1); // indicator(3) + "│ "(2) + "│"(1)

                            // Top border: │  ┌─────────────────────┐
                            let top_border: String = "─".repeat(card_inner_width);
                            messages_text.push(Line::from(vec![
                                Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                ),
                                Span::styled(
                                    format!("┌{}┐", top_border),
                                    Style::default().fg(theme::BORDER_INACTIVE).bg(bg),
                                ),
                            ]));

                            // Title line: │  │ Title (Enter to open)                    │
                            let hint = if is_selected { " (Enter to open)" } else { "" };
                            let title_max_width = card_inner_width.saturating_sub(1 + hint.len()); // space + hint
                            let title_display: String =
                                title.chars().take(title_max_width).collect();
                            let title_padding = card_inner_width
                                .saturating_sub(title_display.chars().count() + hint.len());
                            let title_pad_str: String = " ".repeat(title_padding);
                            messages_text.push(Line::from(vec![
                                Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                ),
                                Span::styled(
                                    "│",
                                    Style::default().fg(theme::BORDER_INACTIVE).bg(bg),
                                ),
                                Span::styled(
                                    format!(" {}", title_display),
                                    Style::default()
                                        .fg(theme::TEXT_PRIMARY)
                                        .bg(bg)
                                        .add_modifier(Modifier::BOLD),
                                ),
                                Span::styled(hint, Style::default().fg(theme::TEXT_MUTED).bg(bg)),
                                Span::styled(title_pad_str, Style::default().bg(bg)),
                                Span::styled(
                                    "│",
                                    Style::default().fg(theme::BORDER_INACTIVE).bg(bg),
                                ),
                            ]));

                            // Agent and status line: │  │ @from -> @to · Done ·  branch        │
                            // Build agent line with proper coloring
                            let mut agent_spans = vec![
                                Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                ),
                                Span::styled(
                                    "│",
                                    Style::default().fg(theme::BORDER_INACTIVE).bg(bg),
                                ),
                                Span::styled(
                                    format!(" @{}", from_agent_name),
                                    Style::default().fg(theme::TEXT_MUTED).bg(bg),
                                ),
                                Span::styled(" → ", Style::default().fg(theme::TEXT_MUTED).bg(bg)),
                                Span::styled(
                                    format!("@{}", to_agent_name),
                                    Style::default().fg(theme::ACCENT_PRIMARY).bg(bg),
                                ),
                            ];
                            // Calculate content length for padding
                            let mut content_len =
                                1 + from_agent_name.len() + 4 + to_agent_name.len(); // " @from -> @to" = 1 + len + 4 + len
                            if is_busy {
                                agent_spans.push(Span::styled(
                                    " · ",
                                    Style::default().fg(theme::TEXT_MUTED).bg(bg),
                                ));
                                let working_text = format!("{} working...", app.spinner_char());
                                content_len += 3 + working_text.len();
                                agent_spans.push(Span::styled(
                                    working_text,
                                    Style::default().fg(theme::ACCENT_PRIMARY).bg(bg),
                                ));
                            } else if let Some(ref status_label) = status {
                                let status_color =
                                    if status_label == "done" || status_label == "Done" {
                                        theme::ACCENT_SUCCESS
                                    } else {
                                        theme::ACCENT_WARNING
                                    };
                                agent_spans.push(Span::styled(
                                    " · ",
                                    Style::default().fg(theme::TEXT_MUTED).bg(bg),
                                ));
                                content_len += 3 + status_label.len();
                                agent_spans.push(Span::styled(
                                    status_label.clone(),
                                    Style::default().fg(status_color).bg(bg),
                                ));
                            }
                            if let Some(ref branch_name) = thread_branch {
                                agent_spans.push(Span::styled(
                                    " · ",
                                    Style::default().fg(theme::TEXT_MUTED).bg(bg),
                                ));
                                let branch_display = format!(" {}", branch_name);
                                content_len += 3 + branch_display.len();
                                agent_spans.push(Span::styled(
                                    branch_display,
                                    Style::default().fg(theme::ACCENT_SPECIAL).bg(bg),
                                ));
                            }
                            let agent_padding = card_inner_width.saturating_sub(content_len);
                            let agent_pad_str: String = " ".repeat(agent_padding);
                            agent_spans.push(Span::styled(agent_pad_str, Style::default().bg(bg)));
                            agent_spans.push(Span::styled(
                                "│",
                                Style::default().fg(theme::BORDER_INACTIVE).bg(bg),
                            ));
                            messages_text.push(Line::from(agent_spans));

                            // Activity line (if present): │  │ Activity text...                │
                            if !activity.is_empty() {
                                let activity_max_width = card_inner_width.saturating_sub(1); // space prefix
                                let activity_display: String =
                                    activity.chars().take(activity_max_width).collect();
                                let activity_padding = card_inner_width
                                    .saturating_sub(1 + activity_display.chars().count());
                                let activity_pad_str: String = " ".repeat(activity_padding);
                                messages_text.push(Line::from(vec![
                                    Span::styled(
                                        indicator,
                                        Style::default().fg(indicator_color).bg(bg),
                                    ),
                                    Span::styled(
                                        "│",
                                        Style::default().fg(theme::BORDER_INACTIVE).bg(bg),
                                    ),
                                    Span::styled(
                                        format!(" {}", activity_display),
                                        Style::default()
                                            .fg(theme::TEXT_MUTED)
                                            .bg(bg)
                                            .add_modifier(Modifier::ITALIC),
                                    ),
                                    Span::styled(activity_pad_str, Style::default().bg(bg)),
                                    Span::styled(
                                        "│",
                                        Style::default().fg(theme::BORDER_INACTIVE).bg(bg),
                                    ),
                                ]));
                            }

                            // Bottom border: │  └─────────────────────┘
                            let bottom_border: String = "─".repeat(card_inner_width);
                            messages_text.push(Line::from(vec![
                                Span::styled(
                                    indicator,
                                    Style::default().fg(indicator_color).bg(bg),
                                ),
                                Span::styled(
                                    format!("└{}┘", bottom_border),
                                    Style::default().fg(theme::BORDER_INACTIVE).bg(bg),
                                ),
                            ]));
                        }
                    }

                    // Only add empty line if no next consecutive
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
                Span::styled("│  ", Style::default().fg(theme::ACCENT_SPECIAL)),
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
                let reasoning_style = Style::default()
                    .fg(theme::TEXT_MUTED)
                    .add_modifier(Modifier::ITALIC);
                for line in buffer.reasoning_content.lines() {
                    messages_text.push(Line::from(vec![
                        Span::styled("│  ", Style::default().fg(theme::ACCENT_SPECIAL)),
                        Span::styled(line.to_string(), reasoning_style),
                    ]));
                }
            }

            // Render text content with cursor indicator
            if !buffer.text_content.is_empty() {
                let markdown_lines = render_markdown(&buffer.text_content);
                for (i, line) in markdown_lines.iter().enumerate() {
                    let mut line_spans = vec![Span::styled(
                        "│  ",
                        Style::default().fg(theme::ACCENT_SPECIAL),
                    )];
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
                    Span::styled("│  ", Style::default().fg(theme::ACCENT_SPECIAL)),
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

        // Wrapping is pre-computed in markdown_lines(), so each line = one visual line
        let total_lines = messages_text.len();
        let max_scroll = total_lines.saturating_sub(visible_height);

        // Update max_scroll_offset so scroll methods work correctly
        app.max_scroll_offset = max_scroll;

        // Auto-scroll to keep selected message visible (when in Normal mode navigating)
        if app.input_mode == InputMode::Normal && !item_line_starts.is_empty() {
            let selected_idx = app.selected_message_index();
            if selected_idx < item_line_starts.len() {
                let selected_start = item_line_starts[selected_idx];
                // Calculate end line (start of next item, or end of all lines)
                let selected_end = if selected_idx + 1 < item_line_starts.len() {
                    item_line_starts[selected_idx + 1]
                } else {
                    total_lines
                };
                let item_height = selected_end.saturating_sub(selected_start);

                // Clamp current scroll_offset to max first (handles usize::MAX sentinel)
                let current_scroll = app.scroll_offset.min(max_scroll);

                // Check if message is taller than viewport
                if item_height >= visible_height {
                    // Message is taller than viewport - prioritize showing the top
                    // Only scroll if the top of the message is not visible
                    if selected_start < current_scroll {
                        // Scroll up to show top of message
                        app.scroll_offset = selected_start;
                    } else if selected_start >= current_scroll + visible_height {
                        // Message top is below viewport - scroll down to show top
                        app.scroll_offset = selected_start.min(max_scroll);
                    }
                    // If top is visible but bottom isn't, don't auto-scroll (prevents flicker)
                } else {
                    // Message fits in viewport - ensure entire message is visible
                    // If selected item is above visible area, scroll up to show it
                    if selected_start < current_scroll {
                        app.scroll_offset = selected_start;
                    }
                    // If selected item is below visible area, scroll down to show it
                    else if selected_end > current_scroll + visible_height {
                        // Scroll so the end of the selected item is at bottom of viewport
                        app.scroll_offset =
                            selected_end.saturating_sub(visible_height).min(max_scroll);
                    }
                }
            }
        }

        // Use scroll_offset, clamped to max
        let scroll = app.scroll_offset.min(max_scroll);

        // No Wrap - wrapping is handled in cards.rs markdown_lines()
        let messages = Paragraph::new(messages_text).scroll((scroll as u16, 0));

        f.render_widget(messages, messages_area);
    }
}
