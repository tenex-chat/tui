use crate::ui::components::{
    modal_area, render_modal_background, render_modal_header, render_modal_items, ModalItem,
    ModalSize,
};
use crate::ui::modal::MessageAction;
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Render the message actions modal
pub fn render_message_actions_modal(
    f: &mut Frame,
    selected_index: usize,
    has_trace: bool,
    area: Rect,
) {
    // Calculate dynamic height based on content
    let item_count = if has_trace { 4 } else { 3 };
    let content_height = (item_count as u16 + 4).min(12); // +4 for header, padding
    let height_percent = (content_height as f32 / area.height as f32).min(0.4);

    let size = ModalSize {
        max_width: 45,
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
    let remaining = render_modal_header(f, inner_area, "Message Actions", "esc");

    // Build items
    let items: Vec<ModalItem> = MessageAction::ALL
        .iter()
        .enumerate()
        .filter(|(_, action)| has_trace || !matches!(action, MessageAction::OpenTrace))
        .map(|(i, action)| {
            ModalItem::new(action.label())
                .with_shortcut(&action.hotkey().to_string())
                .selected(i == selected_index)
        })
        .collect();

    render_modal_items(f, remaining, &items);

    // Render hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("enter select · esc cancel")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

/// Render the view raw event modal
pub fn render_view_raw_event_modal(f: &mut Frame, json: &str, scroll_offset: usize, area: Rect) {
    let size = ModalSize {
        max_width: (area.width as f32 * 0.85) as u16,
        height_percent: 0.85,
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
    let remaining = render_modal_header(f, inner_area, "Raw Event", "esc");

    // Render JSON with syntax highlighting (simple version - just muted for keys)
    let content_area = Rect::new(
        remaining.x + 2,
        remaining.y,
        remaining.width.saturating_sub(4),
        remaining.height,
    );

    let lines: Vec<Line> = json
        .lines()
        .skip(scroll_offset)
        .take(content_area.height as usize)
        .map(|line| {
            // Simple syntax highlighting for JSON
            if line.trim().starts_with('"') && line.contains(':') {
                // Key line
                let parts: Vec<&str> = line.splitn(2, ':').collect();
                if parts.len() == 2 {
                    Line::from(vec![
                        Span::styled(parts[0], Style::default().fg(theme::ACCENT_PRIMARY)),
                        Span::styled(":", Style::default().fg(theme::TEXT_MUTED)),
                        Span::styled(parts[1], Style::default().fg(theme::TEXT_PRIMARY)),
                    ])
                } else {
                    Line::from(Span::styled(line, Style::default().fg(theme::TEXT_PRIMARY)))
                }
            } else {
                Line::from(Span::styled(line, Style::default().fg(theme::TEXT_PRIMARY)))
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, content_area);

    // Render hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints =
        Paragraph::new("↑↓ scroll · esc close").style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

/// Render the hotkey help modal (Ctrl+T+?)
pub fn render_hotkey_help_modal(f: &mut Frame, area: Rect) {
    let size = ModalSize {
        max_width: 60,
        height_percent: 0.75,
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
    let remaining = render_modal_header(f, inner_area, "Keyboard Shortcuts", "esc");

    // Define hotkey sections
    let sections = vec![
        ("Prefix Commands (Ctrl+T + key)", vec![
            ("e", "Expand to full-screen editor"),
            ("m", "Toggle LLM metadata display"),
            ("t", "Toggle todo sidebar"),
            ("v", "Toggle vim mode"),
            ("A", "Toggle show archived"),
            ("?", "Show this help"),
        ]),
        ("Navigation", vec![
            ("Alt+0", "Go to home/dashboard"),
            ("Alt+1-9", "Jump to tab 1-9"),
            ("Alt+Tab", "Cycle through recent tabs"),
            ("Alt+/", "Open tab picker"),
            ("Tab", "Next tab (Chat) / Switch panel (Home)"),
            ("Shift+Tab", "Previous tab"),
            ("↑/↓", "Navigate messages/items"),
            ("Enter", "Open item / Enter subthread"),
            ("Esc", "Back / Exit subthread"),
        ]),
        ("Chat View", vec![
            ("i", "Enter edit mode"),
            ("@", "Open agent selector"),
            ("%", "Open branch selector"),
            ("t", "Toggle todo sidebar"),
            ("o", "Open first image"),
            ("/", "Open message actions"),
            ("x", "Close current tab"),
        ]),
        ("Home View", vec![
            ("p", "Open projects modal"),
            ("n", "New thread in project"),
            ("f", "Cycle time filter"),
            ("/", "Search threads"),
            ("Space", "Toggle project visibility"),
        ]),
        ("Input Mode", vec![
            ("Enter", "Send message"),
            ("Shift/Alt+Enter", "New line"),
            ("Ctrl+A/E", "Line start/end"),
            ("Ctrl+K/U", "Kill to end/start of line"),
            ("Ctrl+W", "Delete word backward"),
            ("Ctrl+D", "Delete char at cursor"),
            ("Ctrl+Z", "Undo"),
            ("Ctrl+Shift+Z", "Redo"),
            ("Ctrl+C/X", "Copy/Cut selection"),
            ("Home/End", "Line start/end"),
            ("Alt+←/→", "Word left/right"),
            ("Alt+Backspace", "Delete word backward"),
            ("Shift+←/→", "Extend selection"),
        ]),
    ];

    // Render content
    let content_area = Rect::new(
        remaining.x + 2,
        remaining.y,
        remaining.width.saturating_sub(4),
        remaining.height,
    );

    let mut lines: Vec<Line> = Vec::new();
    for (section_title, hotkeys) in sections {
        // Section header
        lines.push(Line::from(Span::styled(
            section_title,
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(ratatui::style::Modifier::BOLD),
        )));

        // Hotkeys in this section
        for (key, description) in hotkeys {
            lines.push(Line::from(vec![
                Span::styled(format!("  {:12}", key), Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(description, Style::default().fg(theme::TEXT_PRIMARY)),
            ]));
        }

        lines.push(Line::from("")); // Blank line between sections
    }

    // Truncate to fit
    let visible_lines: Vec<Line> = lines.into_iter().take(content_area.height as usize).collect();
    let paragraph = Paragraph::new(visible_lines);
    f.render_widget(paragraph, content_area);

    // Render hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("Press any key to close")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}
