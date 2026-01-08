use crate::models::AskQuestion;
use crate::ui::app::AskModalState;
use crate::ui::ask_input::InputMode;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame,
};

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
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else if is_answered {
            Style::default().fg(Color::Green)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        spans.push(Span::styled(title, style));

        if i < input_state.questions.len() - 1 {
            spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        }
    }

    // Add Submit tab at the end
    if input_state.is_complete() {
        spans.push(Span::styled(" │ ", Style::default().fg(Color::DarkGray)));
        spans.push(Span::styled(
            "Submit",
            Style::default().fg(Color::Green).add_modifier(Modifier::BOLD),
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
        .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
        .wrap(Wrap { trim: true });
    f.render_widget(question_widget, layout[0]);

    // Custom input mode
    if input_state.mode == InputMode::CustomInput {
        let custom_text = format!("  {}", input_state.custom_input);
        let cursor_pos = input_state.custom_cursor + 2;

        let input_widget = Paragraph::new(custom_text)
            .style(Style::default().fg(Color::Yellow))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                    .title(" Type your answer ")
            );

        f.render_widget(input_widget, layout[1]);
        f.set_cursor_position((
            layout[1].x + cursor_pos as u16 + 1,
            layout[1].y + 1,
        ));
    } else if suggestions.is_empty() {
        // No suggestions - show help text
        let help = Paragraph::new(" Press 'c' to type custom answer ")
            .style(Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))
            .alignment(Alignment::Center);
        f.render_widget(help, layout[1]);
    } else {
        // Show numbered options with selection indicator
        let items: Vec<ListItem> = suggestions
            .iter()
            .enumerate()
            .map(|(i, suggestion)| {
                let is_selected = i == input_state.selected_option_index;
                let marker = if is_selected { "❯ " } else { "  " };

                let style = if is_selected {
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                ListItem::new(Line::from(vec![
                    Span::raw(marker),
                    Span::styled(format!("{}. ", i + 1), style),
                    Span::styled(suggestion, style),
                ]))
            })
            .collect();

        // Add "Type something" option at the end
        let custom_idx = suggestions.len();
        let is_custom_selected = custom_idx == input_state.selected_option_index;
        let custom_marker = if is_custom_selected { "❯ " } else { "  " };
        let custom_style = if is_custom_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let mut all_items = items;
        all_items.push(ListItem::new(Line::from(vec![
            Span::raw(custom_marker),
            Span::styled("Type something", custom_style),
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
        .style(Style::default().fg(Color::White).add_modifier(Modifier::BOLD))
        .wrap(Wrap { trim: true });
    f.render_widget(question_widget, layout[0]);

    // Custom input mode
    if input_state.mode == InputMode::CustomInput {
        let custom_text = format!("  {}", input_state.custom_input);
        let cursor_pos = input_state.custom_cursor + 2;

        let input_widget = Paragraph::new(custom_text)
            .style(Style::default().fg(Color::Yellow))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::Green))
                    .title(" Type your answer ")
            );

        f.render_widget(input_widget, layout[1]);
        f.set_cursor_position((
            layout[1].x + cursor_pos as u16 + 1,
            layout[1].y + 1,
        ));
    } else if options.is_empty() {
        let help = Paragraph::new(" Press 'c' to type custom answer ")
            .style(Style::default().fg(Color::DarkGray).add_modifier(Modifier::ITALIC))
            .alignment(Alignment::Center);
        f.render_widget(help, layout[1]);
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
                    Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };

                ListItem::new(Line::from(vec![
                    Span::raw(marker),
                    Span::styled(format!("{}. ", i + 1), style),
                    Span::styled(checkbox, style),
                    Span::styled(option, style),
                ]))
            })
            .collect();

        // Add "Type something" option
        let custom_idx = options.len();
        let is_custom_selected = custom_idx == input_state.selected_option_index;
        let custom_marker = if is_custom_selected { "❯ " } else { "  " };
        let custom_style = if is_custom_selected {
            Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::DarkGray)
        };

        let mut all_items = items;
        all_items.push(ListItem::new(Line::from(vec![
            Span::raw(custom_marker),
            Span::styled("Type something", custom_style),
        ])));

        let list = List::new(all_items);
        f.render_widget(list, layout[1]);
    }
}

/// Render help bar showing keyboard shortcuts
fn render_help_bar(f: &mut Frame, input_state: &crate::ui::ask_input::AskInputState, area: Rect) {
    let help_text = if input_state.mode == InputMode::CustomInput {
        vec![
            Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" to submit · "),
            Span::styled("Esc", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" to cancel"),
        ]
    } else if input_state.is_multi_select() {
        vec![
            Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" to select · "),
            Span::styled("Space", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
            Span::raw(" for multi-select · "),
            Span::styled("Tab/Arrow keys", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" to navigate · "),
            Span::styled("Esc", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" to cancel"),
        ]
    } else {
        vec![
            Span::styled("Enter", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
            Span::raw(" to select · "),
            Span::styled("Tab/Arrow keys", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
            Span::raw(" to navigate · "),
            Span::styled("Esc", Style::default().fg(Color::Red).add_modifier(Modifier::BOLD)),
            Span::raw(" to cancel"),
        ]
    };

    let help_paragraph = Paragraph::new(Line::from(help_text))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray))
        )
        .alignment(Alignment::Center);

    f.render_widget(help_paragraph, area);
}
