use crate::ui::{App, InputMode};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Color, Style},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

#[derive(Debug, Clone, PartialEq)]
pub enum LoginStep {
    Nsec,
    Password,
}

pub fn render_login(f: &mut Frame, app: &App, area: Rect, login_step: &LoginStep) {
    let chunks = Layout::vertical([
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(0),
    ])
    .split(area);

    // Instructions
    let instructions = match login_step {
        LoginStep::Nsec => "Enter your nsec (private key) to login:",
        LoginStep::Password => "Enter a password to encrypt your key (optional, press Enter to skip):",
    };
    let instruction_widget = Paragraph::new(instructions)
        .style(Style::default().fg(Color::White))
        .alignment(Alignment::Center);
    f.render_widget(instruction_widget, chunks[0]);

    // Input field
    let input_style = if app.input_mode == InputMode::Editing {
        Style::default().fg(Color::Yellow)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let display_text = if *login_step == LoginStep::Nsec && !app.input.is_empty() {
        // Mask the nsec
        "*".repeat(app.input.len())
    } else if *login_step == LoginStep::Password && !app.input.is_empty() {
        "*".repeat(app.input.len())
    } else {
        app.input.clone()
    };

    let input_widget = Paragraph::new(display_text)
        .style(input_style)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(if app.input_mode == InputMode::Editing {
                    Style::default().fg(Color::Yellow)
                } else {
                    Style::default().fg(Color::DarkGray)
                })
                .title(if app.input_mode == InputMode::Editing {
                    "Editing (Esc to cancel, Enter to submit)"
                } else {
                    "Press 'i' to start typing"
                }),
        );
    f.render_widget(input_widget, chunks[1]);

    // Status
    if let Some(ref msg) = app.status_message {
        let status = Paragraph::new(msg.as_str())
            .style(Style::default().fg(Color::Red))
            .alignment(Alignment::Center);
        f.render_widget(status, chunks[2]);
    }
}
