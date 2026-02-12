use crate::ui::app::InputContextFocus;
use crate::ui::card;
use crate::ui::layout;
use crate::ui::notifications::NotificationLevel;
use crate::ui::{theme, App, InputMode, ModalState};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Maximum number of visible content lines before scrolling kicks in
pub(crate) const MAX_VISIBLE_LINES: usize = 15;

pub(crate) fn input_height(app: &App) -> u16 {
    // +5 = 1 for padding top, 1 for context line at bottom, 2 for half-block borders, 1 extra
    let line_count = app.chat_editor().line_count().max(1);
    // Allow up to MAX_VISIBLE_LINES (15) content lines, then scroll
    // Min height 6 (1 line + chrome), max height 20 (15 lines + chrome)
    (line_count as u16 + 5).clamp(6, MAX_VISIBLE_LINES as u16 + 5)
}

pub(crate) fn has_attachments(app: &App) -> bool {
    !app.chat_editor().attachments.is_empty() || !app.chat_editor().image_attachments.is_empty()
}

pub(crate) fn has_status(_app: &App) -> bool {
    // Transient notifications should only appear in the bottom status bar.
    // The upper status bar (above input) was causing duplicate notifications.
    // Always return false to disable the upper status line.
    false
}

pub(crate) fn render_status_line(f: &mut Frame, app: &App, area: Rect) {
    if let Some(notification) = app.current_notification() {
        // Choose color based on notification level
        let color = match notification.level {
            NotificationLevel::Info => theme::ACCENT_PRIMARY,
            NotificationLevel::Success => theme::ACCENT_SUCCESS,
            NotificationLevel::Warning => theme::ACCENT_WARNING,
            NotificationLevel::Error => theme::ACCENT_ERROR,
        };

        let status_line = Line::from(vec![
            Span::styled(
                format!("{} ", notification.level.icon()),
                Style::default().fg(color),
            ),
            Span::styled(
                notification.message.as_str(),
                Style::default()
                    .fg(color)
                    .add_modifier(ratatui::style::Modifier::BOLD),
            ),
        ]);
        let status = Paragraph::new(status_line);
        f.render_widget(status, area);
    }
}

pub(crate) fn render_attachments_line(f: &mut Frame, app: &App, area: Rect) {
    let mut attachment_spans: Vec<Span> =
        vec![Span::styled("Attachments: ", Style::default().fg(theme::TEXT_MUTED))];
    let img_count = app.chat_editor().image_attachments.len();

    // Show image attachments (focus index 0..img_count)
    for (i, img) in app.chat_editor().image_attachments.iter().enumerate() {
        let is_focused = app.chat_editor().focused_attachment == Some(i);
        let style = if is_focused {
            Style::default()
                .fg(Color::Black)
                .bg(theme::ACCENT_SPECIAL)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            Style::default().fg(theme::ACCENT_SPECIAL)
        };
        attachment_spans.push(Span::styled(format!("[Image #{}] ", img.id), style));
    }

    // Show paste attachments (focus index img_count..)
    for (i, attachment) in app.chat_editor().attachments.iter().enumerate() {
        let is_focused = app.chat_editor().focused_attachment == Some(img_count + i);
        let style = if is_focused {
            Style::default()
                .fg(Color::Black)
                .bg(theme::ACCENT_WARNING)
                .add_modifier(ratatui::style::Modifier::BOLD)
        } else {
            Style::default().fg(theme::ACCENT_WARNING)
        };
        attachment_spans.push(Span::styled(format!("[Paste #{}] ", attachment.id), style));
    }

    // Show hint based on what's focused
    if app.chat_editor().focused_attachment.is_some() {
        attachment_spans.push(Span::styled(
            "(Backspace to delete, ↓ to exit)",
            Style::default().fg(theme::TEXT_MUTED),
        ));
    } else {
        attachment_spans.push(Span::styled(
            "(↑ to select)",
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }
    let attachments_line = Line::from(attachment_spans);
    let attachments = Paragraph::new(attachments_line);
    f.render_widget(attachments, area);
}

pub(crate) fn render_input_box(f: &mut Frame, app: &mut App, area: Rect) {
    // Update wrap width for visual line navigation - use consistent padding
    let input_padding = layout::CONTENT_PADDING_H as usize;
    let input_content_width_val = area.width.saturating_sub((1 + input_padding * 2) as u16) as usize;
    app.chat_input_wrap_width = input_content_width_val;
    // Normal chat input - deterministic color border based on user's pubkey
    let is_input_active =
        app.input_mode == InputMode::Editing && !matches!(app.modal_state, ModalState::AskModal(_));

    // Get user's deterministic color for the left border
    let user_color = app
        .data_store
        .borrow()
        .user_pubkey
        .as_ref()
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

    // Agent display with model info (no @ prefix)
    let (agent_display, agent_model_display) = app
        .selected_agent()
        .map(|a| {
            let model = a.model
                .as_ref()
                .map(|m| format!("({})", m))
                .unwrap_or_else(|| "(no model)".to_string());
            (a.name.clone(), model)
        })
        .unwrap_or_else(|| ("none".to_string(), String::new()));

    let project_display = app
        .selected_project
        .as_ref()
        .map(|p| format!(" {}", p.name))
        .unwrap_or_default();

    // Build input card with padding and context line at bottom
    let input_text = app.chat_editor().text.as_str();
    let input_content_width = input_content_width_val;

    // Calculate cursor's visual row and column with proper wrapping
    let cursor_pos = app.chat_editor().cursor;
    let before_cursor = &input_text[..cursor_pos.min(input_text.len())];

    // Count visual rows by iterating through all logical lines before cursor
    let mut cursor_visual_row = 0;
    let mut visual_col = 0;

    // Split text before cursor into logical lines (including partial last line)
    let logical_lines: Vec<&str> = before_cursor.split('\n').collect();
    for (i, line) in logical_lines.iter().enumerate() {
        let is_last_line = i == logical_lines.len() - 1;
        if is_last_line {
            // For the last line (where cursor is), calculate position within wrapped line
            let wrapped_rows = line.len() / input_content_width.max(1);
            cursor_visual_row += wrapped_rows;
            visual_col = line.len() % input_content_width.max(1);
        } else {
            // For complete lines, count all their visual rows (at least 1 for empty lines)
            let line_visual_rows = if line.is_empty() {
                1
            } else {
                (line.len() + input_content_width.max(1) - 1) / input_content_width.max(1)
            };
            cursor_visual_row += line_visual_rows;
        }
    }

    // Build all content lines first (without top padding)
    let mut content_lines: Vec<Line> = Vec::new();

    if input_text.is_empty() {
        // Placeholder text when empty
        let placeholder = if is_input_active { "Type your message..." } else { "" };
        let pad = input_content_width.saturating_sub(placeholder.len());
        content_lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
            Span::styled(" ".repeat(input_padding), Style::default().bg(input_bg)),
            Span::styled(
                placeholder,
                Style::default().fg(theme::TEXT_DIM).bg(input_bg),
            ),
            Span::styled(
                " ".repeat(pad + input_padding),
                Style::default().bg(input_bg),
            ),
        ]));
    } else {
        // Render each line of input with padding, wrapping long lines
        for line in input_text.lines() {
            // Wrap long lines to fit within content width
            let mut remaining = line;
            loop {
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
                content_lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
                    Span::styled(" ".repeat(input_padding), Style::default().bg(input_bg)),
                    Span::styled(
                        chunk.to_string(),
                        Style::default().fg(text_color).bg(input_bg),
                    ),
                    Span::styled(
                        " ".repeat(pad + input_padding),
                        Style::default().bg(input_bg),
                    ),
                ]));

                if rest.is_empty() {
                    break;
                }
                remaining = rest;
            }
        }
        // Handle case where input ends with newline
        if input_text.ends_with('\n') {
            content_lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
                Span::styled(
                    " ".repeat(area.width.saturating_sub(1) as usize),
                    Style::default().bg(input_bg),
                ),
            ]));
        }
    }

    // Calculate visible window for scrolling
    // Available height for content: area.height - 2 (half-block borders) - 1 (top padding) - 1 (context line)
    let available_content_lines = area.height.saturating_sub(4) as usize;
    let total_content_lines = content_lines.len();

    // Determine scroll offset to keep cursor visible
    let scroll_offset = if total_content_lines <= available_content_lines {
        0
    } else {
        // Keep cursor roughly centered, but clamp to valid range
        let half_visible = available_content_lines / 2;
        if cursor_visual_row < half_visible {
            0
        } else if cursor_visual_row + half_visible >= total_content_lines {
            total_content_lines.saturating_sub(available_content_lines)
        } else {
            cursor_visual_row.saturating_sub(half_visible)
        }
    };

    // Build final lines with scrolling applied
    let mut lines: Vec<Line> = Vec::new();

    // Top padding line
    lines.push(Line::from(vec![
        Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
        Span::styled(
            " ".repeat(area.width.saturating_sub(1) as usize),
            Style::default().bg(input_bg),
        ),
    ]));

    // Add visible content lines
    let visible_end = (scroll_offset + available_content_lines).min(total_content_lines);
    for line in content_lines.into_iter().skip(scroll_offset).take(visible_end - scroll_offset) {
        lines.push(line);
    }

    // Pad to fill available space if needed
    let target_height = area.height.saturating_sub(3) as usize; // -2 for borders, -1 for context line
    while lines.len() < target_height {
        lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
            Span::styled(
                " ".repeat(area.width.saturating_sub(1) as usize),
                Style::default().bg(input_bg),
            ),
        ]));
    }

    // Build nudge display string - always show "/" even if empty (for selection)
    // Uses per-tab isolated nudge selections
    let selected_nudge_ids = app.selected_nudge_ids();
    let nudge_display = if selected_nudge_ids.is_empty() {
        "/".to_string()
    } else {
        let nudge_titles: Vec<String> = selected_nudge_ids
            .iter()
            .filter_map(|id| app.data_store.borrow().get_nudge(id).map(|n| format!("/{}", n.title)))
            .collect();
        format!("[{}]", nudge_titles.join(", "))
    };

    // Context line at bottom: agent (model) branch project [nudges]
    // Add scroll indicator if we're scrolling
    let scroll_indicator = if total_content_lines > available_content_lines {
        let current_line = cursor_visual_row + 1;
        format!(" [{}/{}]", current_line, total_content_lines)
    } else {
        String::new()
    };

    // Get focus state for highlighting
    let context_focus = app.input_context_focus;

    // Style helper for focused items (highlighted with inverse colors)
    let focused_style = |base_fg: Color| -> Style {
        Style::default()
            .fg(Color::Black)
            .bg(base_fg)
            .add_modifier(Modifier::BOLD)
    };

    // Calculate context string for padding
    let nudge_str = format!(" {}", nudge_display);
    let context_str = format!("{} {}{}{}{}", agent_display, agent_model_display, project_display, nudge_str, scroll_indicator);
    let context_pad =
        area.width.saturating_sub(context_str.len() as u16 + (1 + input_padding * 2) as u16) as usize;

    // Build context line with highlighting based on focus
    let mut context_spans = vec![
        Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
        Span::styled(" ".repeat(input_padding), Style::default().bg(input_bg)),
    ];

    // Agent name (highlighted if focused)
    let agent_style = if context_focus == Some(InputContextFocus::Agent) {
        focused_style(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::ACCENT_PRIMARY).bg(input_bg)
    };
    context_spans.push(Span::styled(agent_display.clone(), agent_style));

    // Space separator
    context_spans.push(Span::styled(" ", Style::default().bg(input_bg)));

    // Model (highlighted if focused)
    let model_style = if context_focus == Some(InputContextFocus::Model) {
        focused_style(theme::TEXT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_DIM).bg(input_bg)
    };
    context_spans.push(Span::styled(agent_model_display.clone(), model_style));

    // Project (not selectable, always muted)
    context_spans.push(Span::styled(
        project_display.clone(),
        Style::default().fg(theme::TEXT_MUTED).bg(input_bg),
    ));

    // Nudge display (highlighted if focused) - always shown
    context_spans.push(Span::styled(" ", Style::default().bg(input_bg)));
    let nudge_style = if context_focus == Some(InputContextFocus::Nudge) {
        focused_style(theme::ACCENT_WARNING)
    } else {
        Style::default().fg(theme::ACCENT_WARNING).bg(input_bg)
    };
    context_spans.push(Span::styled(nudge_display, nudge_style));

    // Add scroll indicator if scrolling
    if !scroll_indicator.is_empty() {
        context_spans.push(Span::styled(
            scroll_indicator,
            Style::default().fg(theme::TEXT_MUTED).bg(input_bg),
        ));
    }

    context_spans.push(Span::styled(" ".repeat(context_pad.max(0)), Style::default().bg(input_bg)));
    lines.push(Line::from(context_spans));

    // Render with half-block borders for visual padding effect
    let half_block_top = card::OUTER_HALF_BLOCK_BORDER.horizontal_bottom.repeat(area.width as usize); // ▄
    let half_block_bottom = card::OUTER_HALF_BLOCK_BORDER.horizontal_top.repeat(area.width as usize); // ▀

    // Top half-block line (fg=input bg color, no bg - creates "growing down" effect)
    let top_area = Rect::new(area.x, area.y, area.width, 1);
    let top_line = Paragraph::new(Line::from(Span::styled(
        half_block_top,
        Style::default().fg(input_bg),
    )));
    f.render_widget(top_line, top_area);

    // Content area (with input background)
    let content_area = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(2));
    let input = Paragraph::new(lines).style(Style::default().bg(input_bg));
    f.render_widget(input, content_area);

    // Bottom half-block line (fg=input bg color, no bg - creates "growing up" effect)
    let bottom_y = area.y + area.height.saturating_sub(1);
    let bottom_area = Rect::new(area.x, bottom_y, area.width, 1);
    let bottom_line = Paragraph::new(Line::from(Span::styled(
        half_block_bottom,
        Style::default().fg(input_bg),
    )));
    f.render_widget(bottom_line, bottom_area);

    // Show cursor in input mode (but not when ask modal is active)
    // Cursor row is relative to the visible window now
    if is_input_active && !app.is_attachment_modal_open() {
        // Adjust cursor row for scroll offset
        let visible_cursor_row = cursor_visual_row.saturating_sub(scroll_offset);

        f.set_cursor_position((
            area.x + visual_col as u16 + (1 + input_padding) as u16, // +1 for "│" + padding
            area.y + visible_cursor_row as u16 + 2, // +2 for half-block top + top padding
        ));
    }
}
