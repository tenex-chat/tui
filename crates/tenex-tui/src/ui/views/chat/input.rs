use crate::ui::{theme, App, InputMode, ModalState};
use ratatui::{
    layout::Rect,
    style::{Color, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub(crate) fn input_height(app: &App) -> u16 {
    // +3 = 1 for padding top, 1 for context line at bottom, 1 extra for visual breathing room
    let line_count = app.chat_editor.line_count().max(1);
    (line_count as u16 + 3).clamp(4, 12)
}

pub(crate) fn has_attachments(app: &App) -> bool {
    !app.chat_editor.attachments.is_empty() || !app.chat_editor.image_attachments.is_empty()
}

pub(crate) fn has_status(app: &App) -> bool {
    app.status_message.is_some()
}

pub(crate) fn render_status_line(f: &mut Frame, app: &App, area: Rect) {
    if let Some(ref msg) = app.status_message {
        let status_line = Line::from(vec![
            Span::styled("⏳ ", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(
                msg.as_str(),
                Style::default()
                    .fg(theme::ACCENT_WARNING)
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
    let img_count = app.chat_editor.image_attachments.len();

    // Show image attachments (focus index 0..img_count)
    for (i, img) in app.chat_editor.image_attachments.iter().enumerate() {
        let is_focused = app.chat_editor.focused_attachment == Some(i);
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
    for (i, attachment) in app.chat_editor.attachments.iter().enumerate() {
        let is_focused = app.chat_editor.focused_attachment == Some(img_count + i);
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
    if app.chat_editor.focused_attachment.is_some() {
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

pub(crate) fn render_input_box(f: &mut Frame, app: &App, area: Rect) {
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

    // Build input card with padding and context line at bottom
    let input_text = app.chat_editor.text.as_str();
    let mut lines: Vec<Line> = Vec::new();
    let input_content_width = area.width.saturating_sub(5) as usize; // -5 for "│  " left and "  " right padding

    // Top padding line
    lines.push(Line::from(vec![
        Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
        Span::styled(
            " ".repeat(area.width.saturating_sub(1) as usize),
            Style::default().bg(input_bg),
        ),
    ]));

    if input_text.is_empty() {
        // Placeholder text when empty
        let placeholder = if is_input_active { "Type your message..." } else { "" };
        let pad = input_content_width.saturating_sub(placeholder.len());
        lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
            Span::styled("  ", Style::default().bg(input_bg)), // 2-char left padding
            Span::styled(
                placeholder,
                Style::default().fg(theme::TEXT_DIM).bg(input_bg),
            ),
            Span::styled(
                " ".repeat(pad + 2),
                Style::default().bg(input_bg),
            ), // +2 right padding
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
                lines.push(Line::from(vec![
                    Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
                    Span::styled("  ", Style::default().bg(input_bg)), // 2-char left padding
                    Span::styled(
                        chunk.to_string(),
                        Style::default().fg(text_color).bg(input_bg),
                    ),
                    Span::styled(
                        " ".repeat(pad + 2),
                        Style::default().bg(input_bg),
                    ), // +2 right padding
                ]));
                remaining = rest;
            }
        }
        // Handle case where input ends with newline or is single line
        if input_text.ends_with('\n') || lines.len() == 1 {
            lines.push(Line::from(vec![
                Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
                Span::styled(
                    " ".repeat(area.width.saturating_sub(1) as usize),
                    Style::default().bg(input_bg),
                ),
            ]));
        }
    }

    // Reserve last line for context - pad middle lines to fill space
    let target_height = area.height.saturating_sub(1) as usize; // -1 for context line
    while lines.len() < target_height {
        lines.push(Line::from(vec![
            Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
            Span::styled(
                " ".repeat(area.width.saturating_sub(1) as usize),
                Style::default().bg(input_bg),
            ),
        ]));
    }

    // Context line at bottom: @agent on %branch
    let context_str = format!("@{}{}", agent_display, branch_display);
    let context_pad =
        area.width.saturating_sub(context_str.len() as u16 + 4) as usize; // +4 for "│  " and " "
    lines.push(Line::from(vec![
        Span::styled("│", Style::default().fg(input_indicator_color).bg(input_bg)),
        Span::styled("  ", Style::default().bg(input_bg)), // 2-char left padding
        Span::styled(
            format!("@{}", agent_display),
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .bg(input_bg),
        ),
        Span::styled(
            branch_display.clone(),
            Style::default().fg(theme::ACCENT_SUCCESS).bg(input_bg),
        ),
        Span::styled(" ".repeat(context_pad), Style::default().bg(input_bg)),
    ]));

    let input = Paragraph::new(lines).style(Style::default().bg(input_bg));
    f.render_widget(input, area);

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
        let visual_row =
            before_cursor.matches('\n').count() + col_in_last_line / input_content_width.max(1);
        let visual_col = col_in_last_line % input_content_width.max(1);

        f.set_cursor_position((
            area.x + visual_col as u16 + 3, // +3 for "│  "
            area.y + visual_row as u16 + 1, // +1 for top padding
        ));
    }
}
