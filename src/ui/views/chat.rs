use crate::store::get_profile_name;
use crate::ui::{App, InputMode};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

pub fn render_chat(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(3),
    ])
    .split(area);

    // Messages area
    let thread_title = app
        .selected_thread
        .as_ref()
        .map(|t| t.title.clone())
        .unwrap_or_else(|| "Chat".to_string());

    if app.messages.is_empty() {
        let empty = Paragraph::new("No messages yet. Press 'i' to start typing.")
            .style(Style::default().fg(Color::DarkGray))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("{} (Esc to go back)", thread_title)),
            );
        f.render_widget(empty, chunks[0]);
    } else {
        let messages_text: Vec<Line> = app
            .messages
            .iter()
            .rev() // Show oldest first (reverse the vec)
            .flat_map(|msg| {
                let author = get_profile_name(&app.db.connection(), &msg.pubkey);
                vec![
                    Line::from(vec![
                        Span::styled(author, Style::default().fg(Color::Cyan)),
                        Span::styled(": ", Style::default().fg(Color::Cyan)),
                        Span::styled(
                            msg.content.clone(),
                            Style::default().fg(Color::White),
                        ),
                    ]),
                    Line::from(""),
                ]
            })
            .collect();

        let messages = Paragraph::new(messages_text)
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .title(format!("{} (Esc to go back)", thread_title)),
            )
            .wrap(Wrap { trim: false })
            .scroll((app.scroll_offset as u16, 0));

        f.render_widget(messages, chunks[0]);
    }

    // Input area
    let input_style = if app.input_mode == InputMode::Editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let input = Paragraph::new(app.input.as_str())
        .style(input_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(input_style)
                .title(if app.input_mode == InputMode::Editing {
                    "Type your message (Enter to send, Esc to cancel)"
                } else {
                    "Press 'i' to compose"
                }),
        );
    f.render_widget(input, chunks[1]);

    // Show cursor in input mode
    if app.input_mode == InputMode::Editing {
        f.set_cursor_position((
            chunks[1].x + app.cursor_position as u16 + 1,
            chunks[1].y + 1,
        ));
    }
}
