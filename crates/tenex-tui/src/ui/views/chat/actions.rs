use crate::ui::components::{Modal, ModalSize};
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Render the view raw event modal
pub fn render_view_raw_event_modal(f: &mut Frame, json: &str, scroll_offset: usize, area: Rect) {
    let (popup_area, content_area) = Modal::new("Raw Event")
        .size(ModalSize {
            max_width: (area.width as f32 * 0.85) as u16,
            height_percent: 0.85,
        })
        .render_frame(f, area);

    // Render JSON with syntax highlighting (simple version - just muted for keys)
    let json_area = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height,
    );

    let lines: Vec<Line> = json
        .lines()
        .skip(scroll_offset)
        .take(json_area.height as usize)
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
    f.render_widget(paragraph, json_area);

    // Render hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("↑↓ scroll · y copy · esc close")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

/// Render the hotkey help modal (?)
pub fn render_hotkey_help_modal(f: &mut Frame, area: Rect) {
    let (popup_area, content_area) = Modal::new("Keyboard Shortcuts")
        .size(ModalSize {
            max_width: 60,
            height_percent: 0.75,
        })
        .render_frame(f, area);

    // Define hotkey sections
    // NOTE: These should eventually be auto-generated from the hotkey registry
    // For now, manually curated for clarity
    let sections = vec![
        (
            "Global",
            vec![
                ("Ctrl+T", "Open command palette"),
                ("?", "Show this help"),
                ("q", "Quit"),
            ],
        ),
        (
            "Navigation",
            vec![
                ("1", "Go to home/dashboard"),
                ("2-9", "Jump to tab 2-9 (Normal mode)"),
                ("Alt+1-9", "Jump to tab (any mode)"),
                ("Ctrl+T ←", "Previous tab (works everywhere)"),
                ("Ctrl+T →", "Next tab (works everywhere)"),
                ("Tab", "Next tab (Chat) / Switch panel (Home)"),
                ("Shift+Tab", "Previous tab"),
                ("↑/↓", "Navigate messages/items"),
                ("Enter", "Open item / Enter subthread"),
                ("Esc", "Back / Exit subthread"),
            ],
        ),
        (
            "Chat View (Normal)",
            vec![
                ("i", "Enter edit mode"),
                ("@", "Open agent configuration"),
                ("%", "Open branch selector"),
                ("Ctrl+T s", "Toggle sidebar"),
                ("o", "Open first image"),
                ("x", "Close current tab"),
                (".", "Stop agent"),
                ("y", "Copy content"),
                ("v", "View raw event"),
            ],
        ),
        (
            "Home View",
            vec![
                ("p", "Open projects modal"),
                ("n", "New thread in project"),
                ("f", "Cycle time filter"),
                ("/", "Enter search filter (Reports/Search tabs)"),
                ("Space", "Toggle project visibility (Sidebar)"),
            ],
        ),
        (
            "Input Mode",
            vec![
                ("Ctrl+Enter", "Send message"),
                ("Shift/Alt+Enter", "New line"),
                ("Ctrl+A/E", "Line start/end"),
                ("Ctrl+K/U", "Kill to end/start of line"),
                ("Ctrl+W", "Delete word backward"),
                ("Ctrl+D", "Delete char at cursor"),
                ("Ctrl+Z", "Undo"),
                ("Ctrl+Shift+Z", "Redo"),
                ("Ctrl+C/X", "Copy/Cut selection"),
                ("Ctrl+/", "Open [/] nudges/skills selector"),
                ("Ctrl+N", "Open [/] nudges/skills selector"),
                ("Alt+K", "Open [/] nudges/skills selector"),
                ("/ (empty input)", "Open [/] nudges/skills selector"),
                ("Home/End", "Line start/end"),
                ("Alt+←/→", "Word left/right"),
                ("Alt+Backspace", "Delete word backward"),
                ("Shift+←/→", "Extend selection"),
                ("Esc", "Exit edit mode"),
            ],
        ),
    ];

    // Render content
    let hotkeys_area = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height,
    );

    let mut lines: Vec<Line> = Vec::new();
    for (section_title, hotkeys) in sections {
        // Section header
        lines.push(Line::from(Span::styled(
            section_title,
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(ratatui::style::Modifier::BOLD),
        )));

        // Hotkeys in this section
        for (key, description) in hotkeys {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {:12}", key),
                    Style::default().fg(theme::TEXT_MUTED),
                ),
                Span::styled(description, Style::default().fg(theme::TEXT_PRIMARY)),
            ]));
        }

        lines.push(Line::from("")); // Blank line between sections
    }

    // Truncate to fit
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .take(hotkeys_area.height as usize)
        .collect();
    let paragraph = Paragraph::new(visible_lines);
    f.render_widget(paragraph, hotkeys_area);

    // Render hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints =
        Paragraph::new("Press any key to close").style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}
