use crate::ui::notifications::NotificationLevel;
use crate::ui::{theme, App};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::Style,
    widgets::{Block, Borders, Paragraph},
    Frame,
};

#[derive(Debug, Clone, PartialEq)]
pub enum LoginStep {
    Nsec,
    Password,
    Unlock,
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
        LoginStep::Unlock => "Welcome back! Enter your password to unlock:",
    };
    let instruction_widget = Paragraph::new(instructions)
        .style(Style::default().fg(theme::TEXT_PRIMARY))
        .alignment(Alignment::Center);
    f.render_widget(instruction_widget, chunks[0]);

    // Input field
    let display_text = if *login_step == LoginStep::Nsec && !app.input.is_empty() {
        // Mask the nsec
        "*".repeat(app.input.len())
    } else if *login_step == LoginStep::Password && !app.input.is_empty() {
        "*".repeat(app.input.len())
    } else if *login_step == LoginStep::Unlock && !app.input.is_empty() {
        "*".repeat(app.input.len())
    } else {
        app.input.clone()
    };

    let input_widget = Paragraph::new(display_text)
        .style(Style::default().fg(theme::ACCENT_WARNING))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT_WARNING)),
        );
    f.render_widget(input_widget, chunks[1]);

    // Status notification
    if let Some(notification) = app.current_notification() {
        let color = match notification.level {
            NotificationLevel::Info => theme::ACCENT_PRIMARY,
            NotificationLevel::Success => theme::ACCENT_SUCCESS,
            NotificationLevel::Warning => theme::ACCENT_WARNING,
            NotificationLevel::Error => theme::ACCENT_ERROR,
        };
        let status = Paragraph::new(format!("{} {}", notification.level.icon(), notification.message))
            .style(Style::default().fg(color))
            .alignment(Alignment::Center);
        f.render_widget(status, chunks[2]);
    }
}
