use crate::models::AskQuestion;
use crate::ui::modal::AskModalState;
use crate::ui::ask_input::InputMode;
use crate::ui::theme;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

/// Generate Lines for inline ask UI to be rendered within message cards
/// Returns lines that can be appended to the message content
pub fn render_inline_ask_lines(
    modal_state: &AskModalState,
    indicator_color: Color,
    bg: Color,
    content_width: usize,
) -> Vec<Line<'static>> {
    let input_state = &modal_state.input_state;
    let mut lines: Vec<Line<'static>> = Vec::new();

    // Helper to pad line to full width
    let pad_to_width = |spans: &mut Vec<Span<'static>>, current_len: usize| {
        let pad = content_width.saturating_sub(current_len);
        if pad > 0 {
            spans.push(Span::styled(" ".repeat(pad), Style::default().bg(bg)));
        }
    };

    // === Tab bar (if multiple questions) ===
    if input_state.questions.len() > 1 {
        let mut tab_spans: Vec<Span<'static>> = vec![
            Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
            Span::styled(" ", Style::default().bg(bg)),
        ];
        let mut tab_len = 2;

        for (i, question) in input_state.questions.iter().enumerate() {
            let is_current = i == input_state.current_question_index;
            let is_answered = i < input_state.answers.len();

            let title = match question {
                AskQuestion::SingleSelect { title, .. } => title.clone(),
                AskQuestion::MultiSelect { title, .. } => title.clone(),
            };

            let style = if is_current {
                Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD).bg(bg)
            } else if is_answered {
                Style::default().fg(theme::ACCENT_SUCCESS).bg(bg)
            } else {
                Style::default().fg(theme::TEXT_MUTED).bg(bg)
            };

            tab_spans.push(Span::styled(title.clone(), style));
            tab_len += title.len();

            if i < input_state.questions.len() - 1 {
                tab_spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED).bg(bg)));
                tab_len += 3;
            }
        }

        pad_to_width(&mut tab_spans, tab_len);
        lines.push(Line::from(tab_spans));
    }

    // === Current question text ===
    if let Some(question) = input_state.current_question() {
        let question_text = match question {
            AskQuestion::SingleSelect { question, .. } => question.clone(),
            AskQuestion::MultiSelect { question, .. } => question.clone(),
        };

        let mut q_spans: Vec<Span<'static>> = vec![
            Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(question_text.clone(), Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD).bg(bg)),
        ];
        let q_len = 2 + question_text.len();
        pad_to_width(&mut q_spans, q_len);
        lines.push(Line::from(q_spans));

        // Empty line after question
        let mut empty_spans: Vec<Span<'static>> = vec![
            Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
            Span::styled(" ", Style::default().bg(bg)),
        ];
        pad_to_width(&mut empty_spans, 2);
        lines.push(Line::from(empty_spans));
    }

    // === Options ===
    if input_state.mode == InputMode::CustomInput {
        // Show custom input mode
        let custom_text = format!("  {}", input_state.custom_input);
        let cursor_indicator = "▌";
        let mut custom_spans: Vec<Span<'static>> = vec![
            Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
            Span::styled(" ", Style::default().bg(bg)),
            Span::styled(custom_text.clone(), Style::default().fg(theme::ACCENT_WARNING).bg(bg)),
            Span::styled(cursor_indicator.to_string(), Style::default().fg(theme::ACCENT_WARNING).bg(bg)),
        ];
        let custom_len = 2 + custom_text.len() + cursor_indicator.len();
        pad_to_width(&mut custom_spans, custom_len);
        lines.push(Line::from(custom_spans));
    } else if let Some(question) = input_state.current_question() {
        match question {
            AskQuestion::SingleSelect { suggestions, .. } => {
                // Render options
                for (i, suggestion) in suggestions.iter().enumerate() {
                    let is_selected = i == input_state.selected_option_index;
                    let marker = if is_selected { "❯" } else { " " };
                    let style = if is_selected {
                        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD).bg(bg)
                    } else {
                        Style::default().fg(theme::TEXT_PRIMARY).bg(bg)
                    };

                    let option_text = format!("{} {}. {}", marker, i + 1, suggestion);
                    let mut opt_spans: Vec<Span<'static>> = vec![
                        Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                        Span::styled(" ", Style::default().bg(bg)),
                        Span::styled(option_text.clone(), style),
                    ];
                    let opt_len = 2 + option_text.len();
                    pad_to_width(&mut opt_spans, opt_len);
                    lines.push(Line::from(opt_spans));
                }

                // Custom input option at the end
                let custom_idx = suggestions.len();
                let is_custom_selected = custom_idx == input_state.selected_option_index;
                let custom_marker = if is_custom_selected { "❯" } else { " " };
                let custom_style = if is_custom_selected {
                    Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD).bg(bg)
                } else {
                    Style::default().fg(theme::TEXT_MUTED).bg(bg)
                };

                let custom_option_text = format!("{} {}. Or type your own answer...", custom_marker, custom_idx + 1);
                let mut custom_opt_spans: Vec<Span<'static>> = vec![
                    Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                    Span::styled(" ", Style::default().bg(bg)),
                    Span::styled(custom_option_text.clone(), custom_style),
                ];
                let custom_opt_len = 2 + custom_option_text.len();
                pad_to_width(&mut custom_opt_spans, custom_opt_len);
                lines.push(Line::from(custom_opt_spans));
            }
            AskQuestion::MultiSelect { options, .. } => {
                // Render options with checkboxes
                for (i, option) in options.iter().enumerate() {
                    let is_selected = i == input_state.selected_option_index;
                    let is_checked = input_state.multi_select_state.get(i).copied().unwrap_or(false);

                    let marker = if is_selected { "❯" } else { " " };
                    let checkbox = if is_checked { "[✓]" } else { "[ ]" };
                    let style = if is_selected {
                        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD).bg(bg)
                    } else {
                        Style::default().fg(theme::TEXT_PRIMARY).bg(bg)
                    };

                    let option_text = format!("{} {}. {} {}", marker, i + 1, checkbox, option);
                    let mut opt_spans: Vec<Span<'static>> = vec![
                        Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                        Span::styled(" ", Style::default().bg(bg)),
                        Span::styled(option_text.clone(), style),
                    ];
                    let opt_len = 2 + option_text.len();
                    pad_to_width(&mut opt_spans, opt_len);
                    lines.push(Line::from(opt_spans));
                }

                // Custom input option
                let custom_idx = options.len();
                let is_custom_selected = custom_idx == input_state.selected_option_index;
                let custom_marker = if is_custom_selected { "❯" } else { " " };
                let custom_style = if is_custom_selected {
                    Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD).bg(bg)
                } else {
                    Style::default().fg(theme::TEXT_MUTED).bg(bg)
                };

                let custom_option_text = format!("{} {}. Or type your own answer...", custom_marker, custom_idx + 1);
                let mut custom_opt_spans: Vec<Span<'static>> = vec![
                    Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                    Span::styled(" ", Style::default().bg(bg)),
                    Span::styled(custom_option_text.clone(), custom_style),
                ];
                let custom_opt_len = 2 + custom_option_text.len();
                pad_to_width(&mut custom_opt_spans, custom_opt_len);
                lines.push(Line::from(custom_opt_spans));
            }
        }
    }

    // === Help bar ===
    let help_text = if input_state.mode == InputMode::CustomInput {
        "Enter submit · Esc cancel"
    } else if input_state.is_multi_select() {
        "Enter select · Space toggle · ↑↓ navigate · ← back · → skip · Esc cancel"
    } else {
        "Enter select · ↑↓ navigate · ← back · → skip · Esc cancel"
    };

    let mut help_spans: Vec<Span<'static>> = vec![
        Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
        Span::styled(" ", Style::default().bg(bg)),
        Span::styled(help_text.to_string(), Style::default().fg(theme::TEXT_MUTED).bg(bg)),
    ];
    let help_len = 2 + help_text.len();
    pad_to_width(&mut help_spans, help_len);
    lines.push(Line::from(help_spans));

    lines
}

/// Render ask UI inline (replacing the input box) - Claude Code style
pub fn render_inline_ask_ui(f: &mut Frame, modal_state: &AskModalState, area: Rect) {
    let input_state = &modal_state.input_state;

    // Split into: tab bar (1 line), question area (rest), help bar (3 lines)
    let chunks = Layout::vertical([
        Constraint::Length(1),  // Tab bar
        Constraint::Min(3),     // Question content
        Constraint::Length(3),  // Help bar
    ]).split(area);

    // Render tab bar with question titles
    render_tab_bar(f, input_state, chunks[0]);

    // Render current question
    if let Some(question) = input_state.current_question() {
        render_question(f, input_state, question, chunks[1]);
    }

    // Render help bar
    render_help_bar(f, input_state, chunks[2]);
}

/// Render tab bar showing question titles (Feature | Practices | Detail | Submit)
fn render_tab_bar(f: &mut Frame, input_state: &crate::ui::ask_input::AskInputState, area: Rect) {
    let mut spans: Vec<Span> = Vec::new();

    // Add question tabs
    for (i, question) in input_state.questions.iter().enumerate() {
        let is_current = i == input_state.current_question_index;
        let is_answered = i < input_state.answers.len();

        let title = match question {
            AskQuestion::SingleSelect { title, .. } => title,
            AskQuestion::MultiSelect { title, .. } => title,
        };

        let style = if is_current {
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
        } else if is_answered {
            Style::default().fg(theme::ACCENT_SUCCESS)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        spans.push(Span::styled(title, style));

        if i < input_state.questions.len() - 1 {
            spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
        }
    }

    // Add Submit tab at the end
    if input_state.is_complete() {
        spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
        spans.push(Span::styled(
            "Submit",
            Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD),
        ));
    }

    let tab_line = Paragraph::new(Line::from(spans))
        .alignment(Alignment::Left);

    f.render_widget(tab_line, area);
}

/// Render the current question with options
fn render_question(
    f: &mut Frame,
    input_state: &crate::ui::ask_input::AskInputState,
    question: &AskQuestion,
    area: Rect,
) {
    match question {
        AskQuestion::SingleSelect { title, question: q_text, suggestions } => {
            render_single_select(f, input_state, title, q_text, suggestions, area);
        }
        AskQuestion::MultiSelect { title, question: q_text, options } => {
            render_multi_select(f, input_state, title, q_text, options, area);
        }
    }
}

/// Render single-select question with options
fn render_single_select(
    f: &mut Frame,
    input_state: &crate::ui::ask_input::AskInputState,
    _title: &str,
    question: &str,
    suggestions: &[String],
    area: Rect,
) {
    let layout = Layout::vertical([
        Constraint::Length(2),  // Question header
        Constraint::Min(3),     // Options
    ]).split(area);

    // Question text
    let question_widget = Paragraph::new(question)
        .style(Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD))
        .wrap(Wrap { trim: true });
    f.render_widget(question_widget, layout[0]);

    // Custom input mode - show the text input
    if input_state.mode == InputMode::CustomInput {
        let custom_text = format!("  {}", input_state.custom_input);
        let cursor_pos = input_state.custom_cursor + 2;

        let input_widget = Paragraph::new(custom_text)
            .style(Style::default().fg(theme::ACCENT_WARNING))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT_SUCCESS))
                    .title(" Type your answer ")
            );

        f.render_widget(input_widget, layout[1]);
        f.set_cursor_position((
            layout[1].x + cursor_pos as u16 + 1,
            layout[1].y + 1,
        ));
    } else {
        // Show numbered options with selection indicator
        let items: Vec<ListItem> = suggestions
            .iter()
            .enumerate()
            .map(|(i, suggestion)| {
                let is_selected = i == input_state.selected_option_index;
                let marker = if is_selected { "❯ " } else { "  " };

                let style = if is_selected {
                    Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };

                ListItem::new(Line::from(vec![
                    Span::raw(marker),
                    Span::styled(format!("{}. ", i + 1), style),
                    Span::styled(suggestion, style),
                ]))
            })
            .collect();

        // Add "Or type your own answer..." option at the end
        let custom_idx = suggestions.len();
        let is_custom_selected = custom_idx == input_state.selected_option_index;
        let custom_marker = if is_custom_selected { "❯ " } else { "  " };
        let custom_style = if is_custom_selected {
            Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        let mut all_items = items;
        all_items.push(ListItem::new(Line::from(vec![
            Span::raw(custom_marker),
            Span::styled("Or type your own answer...", custom_style),
        ])));

        let list = List::new(all_items);
        f.render_widget(list, layout[1]);
    }
}

/// Render multi-select question with checkboxes
fn render_multi_select(
    f: &mut Frame,
    input_state: &crate::ui::ask_input::AskInputState,
    _title: &str,
    question: &str,
    options: &[String],
    area: Rect,
) {
    let layout = Layout::vertical([
        Constraint::Length(2),  // Question header
        Constraint::Min(3),     // Options
    ]).split(area);

    // Question text
    let question_widget = Paragraph::new(question)
        .style(Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD))
        .wrap(Wrap { trim: true });
    f.render_widget(question_widget, layout[0]);

    // Custom input mode - show the text input
    if input_state.mode == InputMode::CustomInput {
        let custom_text = format!("  {}", input_state.custom_input);
        let cursor_pos = input_state.custom_cursor + 2;

        let input_widget = Paragraph::new(custom_text)
            .style(Style::default().fg(theme::ACCENT_WARNING))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT_SUCCESS))
                    .title(" Type your answer ")
            );

        f.render_widget(input_widget, layout[1]);
        f.set_cursor_position((
            layout[1].x + cursor_pos as u16 + 1,
            layout[1].y + 1,
        ));
    } else {
        // Show numbered options with checkboxes
        let items: Vec<ListItem> = options
            .iter()
            .enumerate()
            .map(|(i, option)| {
                let is_selected = i == input_state.selected_option_index;
                let is_checked = input_state.multi_select_state.get(i).copied().unwrap_or(false);

                let marker = if is_selected { "❯ " } else { "  " };
                let checkbox = if is_checked { "[✓] " } else { "[ ] " };

                let style = if is_selected {
                    Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };

                ListItem::new(Line::from(vec![
                    Span::raw(marker),
                    Span::styled(format!("{}. ", i + 1), style),
                    Span::styled(checkbox, style),
                    Span::styled(option, style),
                ]))
            })
            .collect();

        // Add "Or type your own answer..." option
        let custom_idx = options.len();
        let is_custom_selected = custom_idx == input_state.selected_option_index;
        let custom_marker = if is_custom_selected { "❯ " } else { "  " };
        let custom_style = if is_custom_selected {
            Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        let mut all_items = items;
        all_items.push(ListItem::new(Line::from(vec![
            Span::raw(custom_marker),
            Span::styled("Or type your own answer...", custom_style),
        ])));

        let list = List::new(all_items);
        f.render_widget(list, layout[1]);
    }
}

/// Render help bar showing keyboard shortcuts
fn render_help_bar(f: &mut Frame, input_state: &crate::ui::ask_input::AskInputState, area: Rect) {
    let help_text = if input_state.mode == InputMode::CustomInput {
        vec![
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)),
            Span::raw(" submit · "),
            Span::styled("Shift+Enter", Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::raw(" newline · "),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_ERROR).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]
    } else if input_state.is_multi_select() {
        vec![
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)),
            Span::raw(" select · "),
            Span::styled("Space", Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::raw(" toggle · "),
            Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
            Span::raw(" navigate · "),
            Span::styled("←", Style::default().fg(theme::ACCENT_SPECIAL).add_modifier(Modifier::BOLD)),
            Span::raw(" back · "),
            Span::styled("→", Style::default().fg(theme::ACCENT_SPECIAL).add_modifier(Modifier::BOLD)),
            Span::raw(" skip · "),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_ERROR).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]
    } else {
        vec![
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)),
            Span::raw(" select · "),
            Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
            Span::raw(" navigate · "),
            Span::styled("←", Style::default().fg(theme::ACCENT_SPECIAL).add_modifier(Modifier::BOLD)),
            Span::raw(" back · "),
            Span::styled("→", Style::default().fg(theme::ACCENT_SPECIAL).add_modifier(Modifier::BOLD)),
            Span::raw(" skip · "),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_ERROR).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]
    };

    let help_paragraph = Paragraph::new(Line::from(help_text))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::TEXT_MUTED))
        )
        .alignment(Alignment::Center);

    f.render_widget(help_paragraph, area);
}
