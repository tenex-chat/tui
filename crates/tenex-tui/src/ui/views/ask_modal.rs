use crate::models::AskQuestion;
use crate::ui::modal::AskModalState;
use crate::ui::ask_input::InputMode;
use crate::ui::theme;
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};

pub fn render_ask_modal(f: &mut Frame, modal_state: &AskModalState, area: Rect) {
    let input_state = &modal_state.input_state;

    let title_text = modal_state.ask_event.title.as_ref()
        .map(|t| format!(" {} ", t))
        .unwrap_or_else(|| " Answer Questions ".to_string());

    let block = Block::default()
        .title(title_text)
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
        .style(Style::default().bg(Color::Black));

    let inner = block.inner(area);
    f.render_widget(Clear, area);
    f.render_widget(block, area);

    let chunks = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(3),
        ])
        .split(inner);

    if let Some(question) = input_state.current_question() {
        let content_chunks = Layout::default()
            .direction(ratatui::layout::Direction::Vertical)
            .constraints([
                Constraint::Length(5),
                Constraint::Min(3),
                Constraint::Length(1),
            ])
            .split(chunks[0]);

        render_context_and_progress(f, modal_state, content_chunks[0]);
        render_question(f, input_state, question, content_chunks[1]);
        render_question_indicator(f, input_state, content_chunks[2]);
    }

    render_help_bar(f, input_state, chunks[1]);
}

fn render_context_and_progress(f: &mut Frame, modal_state: &AskModalState, area: Rect) {
    let input_state = &modal_state.input_state;

    let progress_text = format!(
        "Question {}/{}",
        input_state.current_question_index + 1,
        input_state.questions.len()
    );

    let context_text = if modal_state.ask_event.context.len() > 200 {
        format!("{}...", &modal_state.ask_event.context[..200])
    } else {
        modal_state.ask_event.context.clone()
    };

    let header = Line::from(vec![
        Span::styled(progress_text, Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
    ]);

    let context = Paragraph::new(context_text)
        .style(Style::default().fg(theme::TEXT_MUTED))
        .wrap(Wrap { trim: true });

    let layout = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(1),
        ])
        .split(area);

    f.render_widget(Paragraph::new(header), layout[0]);
    f.render_widget(context, layout[2]);
}

fn render_question(f: &mut Frame, input_state: &crate::ui::ask_input::AskInputState, question: &AskQuestion, area: Rect) {
    match question {
        AskQuestion::SingleSelect { title, question: q_text, suggestions } => {
            render_single_select(f, input_state, title, q_text, suggestions, area);
        }
        AskQuestion::MultiSelect { title, question: q_text, options } => {
            render_multi_select(f, input_state, title, q_text, options, area);
        }
    }
}

fn render_single_select(
    f: &mut Frame,
    input_state: &crate::ui::ask_input::AskInputState,
    title: &str,
    question: &str,
    suggestions: &[String],
    area: Rect,
) {
    let layout = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
        ])
        .split(area);

    let question_widget = Paragraph::new(vec![
        Line::from(Span::styled(title, Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::raw(question)),
    ])
    .wrap(Wrap { trim: true });

    f.render_widget(question_widget, layout[0]);

    if input_state.mode == InputMode::CustomInput {
        let custom_text = format!(" Custom answer: {} ", input_state.custom_input);
        let cursor_pos = input_state.custom_cursor + " Custom answer: ".len();

        let input_widget = Paragraph::new(custom_text)
            .style(Style::default().fg(theme::ACCENT_WARNING))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme::ACCENT_SUCCESS)));

        f.render_widget(input_widget, layout[1]);
        f.set_cursor_position((
            layout[1].x + cursor_pos as u16 + 1,
            layout[1].y + 1,
        ));
    } else if suggestions.is_empty() {
        let help = Paragraph::new(" Press 'c' to enter custom answer ")
            .style(Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::ITALIC))
            .alignment(Alignment::Center);
        f.render_widget(help, layout[1]);
    } else {
        let items: Vec<ListItem> = suggestions
            .iter()
            .enumerate()
            .map(|(i, suggestion)| {
                let marker = if i == input_state.selected_option_index {
                    "> "
                } else {
                    "  "
                };

                let style = if i == input_state.selected_option_index {
                    Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(vec![
                    Span::raw(marker),
                    Span::styled(suggestion, style),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().title(" Suggestions (or press 'c' for custom) ").borders(Borders::ALL));

        f.render_widget(list, layout[1]);
    }
}

fn render_multi_select(
    f: &mut Frame,
    input_state: &crate::ui::ask_input::AskInputState,
    title: &str,
    question: &str,
    options: &[String],
    area: Rect,
) {
    let layout = Layout::default()
        .direction(ratatui::layout::Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(3),
        ])
        .split(area);

    let question_widget = Paragraph::new(vec![
        Line::from(Span::styled(title, Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::raw(question)),
    ])
    .wrap(Wrap { trim: true });

    f.render_widget(question_widget, layout[0]);

    if input_state.mode == InputMode::CustomInput {
        let custom_text = format!(" Custom answer: {} ", input_state.custom_input);
        let cursor_pos = input_state.custom_cursor + " Custom answer: ".len();

        let input_widget = Paragraph::new(custom_text)
            .style(Style::default().fg(theme::ACCENT_WARNING))
            .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme::ACCENT_SUCCESS)));

        f.render_widget(input_widget, layout[1]);
        f.set_cursor_position((
            layout[1].x + cursor_pos as u16 + 1,
            layout[1].y + 1,
        ));
    } else if options.is_empty() {
        let help = Paragraph::new(" Press 'c' to enter custom answer ")
            .style(Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::ITALIC))
            .alignment(Alignment::Center);
        f.render_widget(help, layout[1]);
    } else {
        let items: Vec<ListItem> = options
            .iter()
            .enumerate()
            .map(|(i, option)| {
                let is_selected = i == input_state.selected_option_index;
                let is_checked = input_state.multi_select_state.get(i).copied().unwrap_or(false);

                let checkbox = if is_checked { "[x]" } else { "[ ]" };
                let marker = if is_selected { "> " } else { "  " };

                let style = if is_selected {
                    Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(vec![
                    Span::raw(marker),
                    Span::styled(checkbox, style),
                    Span::raw(" "),
                    Span::styled(option, style),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().title(" Options (Space to toggle, Enter when done, or 'c' for custom) ").borders(Borders::ALL));

        f.render_widget(list, layout[1]);
    }
}

fn render_question_indicator(f: &mut Frame, input_state: &crate::ui::ask_input::AskInputState, area: Rect) {
    let mut indicators = Vec::new();
    for (i, _) in input_state.questions.iter().enumerate() {
        let marker = if i < input_state.answers.len() {
            "●"
        } else if i == input_state.current_question_index {
            "◉"
        } else {
            "○"
        };

        let style = if i < input_state.answers.len() {
            Style::default().fg(theme::ACCENT_SUCCESS)
        } else if i == input_state.current_question_index {
            Style::default().fg(theme::ACCENT_PRIMARY)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        indicators.push(Span::styled(marker, style));
        indicators.push(Span::raw(" "));
    }

    let indicator_line = Paragraph::new(Line::from(indicators))
        .alignment(Alignment::Center);

    f.render_widget(indicator_line, area);
}

fn render_help_bar(f: &mut Frame, input_state: &crate::ui::ask_input::AskInputState, area: Rect) {
    let help_text = if input_state.mode == InputMode::CustomInput {
        vec![
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)),
            Span::raw(" submit  "),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_ERROR).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]
    } else {
        let nav_help = if input_state.is_multi_select() {
            vec![
                Span::styled("↑↓/jk", Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
                Span::raw(" navigate  "),
                Span::styled("Space", Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)),
                Span::raw(" toggle  "),
                Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)),
                Span::raw(" confirm  "),
            ]
        } else {
            vec![
                Span::styled("↑↓/jk", Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
                Span::raw(" navigate  "),
                Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)),
                Span::raw(" select  "),
            ]
        };

        let mut all_help = nav_help;
        all_help.extend(vec![
            Span::styled("c", Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)),
            Span::raw(" custom  "),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_ERROR).add_modifier(Modifier::BOLD)),
            Span::raw(" cancel"),
        ]);
        all_help
    };

    let help_paragraph = Paragraph::new(Line::from(help_text))
        .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(theme::BORDER_INACTIVE)))
        .alignment(Alignment::Center);

    f.render_widget(help_paragraph, area);
}
