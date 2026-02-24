use ratatui::{
    layout::{Constraint, Layout},
    style::Style,
    widgets::{Block, Paragraph},
    Frame,
};

use crate::ui;
use crate::ui::components::render_statusbar;
use crate::ui::layout;
use crate::ui::modal::ModalState;
use crate::ui::views::login::{render_login, LoginStep};
use crate::ui::{App, InputMode, View};

pub(crate) fn render(f: &mut Frame, app: &mut App, login_step: &LoginStep) {
    // Fill entire frame with app background (pure black)
    let bg_block = Block::default().style(Style::default().bg(ui::theme::BG_APP));
    f.render_widget(bg_block, f.area());

    // Home and Chat views have their own chrome - give them full area
    if app.view == View::Home {
        ui::views::render_home(f, app, f.area());
        return;
    }
    if app.view == View::Chat {
        ui::views::render_chat(f, app, f.area());
        return;
    }

    // Chrome height varies by view - using centralized layout constants
    let (header_height, footer_height) = match app.view {
        View::Chat => (layout::HEADER_HEIGHT_CHAT, layout::FOOTER_HEIGHT_CHAT),
        _ => (layout::HEADER_HEIGHT_DEFAULT, layout::FOOTER_HEIGHT_DEFAULT),
    };

    let chunks = Layout::vertical([
        Constraint::Length(header_height),
        Constraint::Min(0),
        Constraint::Length(footer_height),
        Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
    ])
    .split(f.area());

    // Determine chrome color based on pending_quit state
    let chrome_color = if app.pending_quit {
        ui::theme::ACCENT_ERROR
    } else {
        ui::theme::ACCENT_PRIMARY
    };

    // Header
    let title: String = match app.view {
        View::Login => "TENEX - Login".to_string(),
        View::Home => "TENEX - Home".to_string(), // Won't reach here
        View::Chat => app
            .selected_thread()
            .map(|t| t.title.clone())
            .or_else(|| {
                app.open_tabs()
                    .get(app.active_tab_index())
                    .map(|tab| tab.thread_title.clone())
            })
            .unwrap_or_else(|| "Chat".to_string()),
        View::LessonViewer => "TENEX - Lesson".to_string(),
        View::AgentBrowser => "TENEX - Agent Definitions".to_string(),
    };

    // Apply consistent padding to header
    let padding = " ".repeat(layout::CONTENT_PADDING_H as usize);
    if app.view == View::Chat {
        let header = Paragraph::new(format!("\n{}{}", padding, title)).style(
            Style::default()
                .fg(chrome_color)
                .add_modifier(ratatui::style::Modifier::BOLD),
        );
        f.render_widget(header, chunks[0]);
    } else {
        let header = Paragraph::new(format!("{}{}", padding, title))
            .style(Style::default().fg(chrome_color));
        f.render_widget(header, chunks[0]);
    }

    // Main content
    match app.view {
        View::Login => render_login(f, app, chunks[1], login_step),
        View::Home => {} // Won't reach here
        View::Chat => ui::views::render_chat(f, app, chunks[1]),
        View::LessonViewer => {
            if let Some(ref lesson_id) = app.viewing_lesson_id.clone() {
                if let Some(lesson) = app.data_store.borrow().content.get_lesson(lesson_id) {
                    ui::views::render_lesson_viewer(f, app, chunks[1], lesson);
                }
            }
        }
        View::AgentBrowser => ui::views::render_agent_browser(f, app, chunks[1]),
    }

    // Footer - show quit warning if pending, otherwise normal hints
    let (footer_text, footer_style) = if app.pending_quit {
        (
            "⚠ Press Ctrl+C again to quit".to_string(),
            Style::default().fg(ui::theme::ACCENT_ERROR),
        )
    } else {
        let text = match (&app.view, &app.input_mode) {
            (View::Login, InputMode::Editing) => format!("> {}", "*".repeat(app.input.len())),
            (View::Chat, InputMode::Normal) => {
                // Check if selected item (thread or delegation) has active operations
                let is_busy = app
                    .get_stop_target_thread_id()
                    .map(|id| app.data_store.borrow().operations.is_event_busy(&id))
                    .unwrap_or(false);
                if is_busy {
                    "Ctrl+T commands · . stop".to_string()
                } else {
                    "Ctrl+T commands".to_string()
                }
            }
            (_, InputMode::Normal) => "Ctrl+T commands · q quit".to_string(),
            _ => String::new(), // Chat/Threads editing has its own input box
        };
        (text, Style::default().fg(ui::theme::TEXT_MUTED))
    };

    // Apply consistent padding to footer (same as content areas)
    let formatted_footer = format!(
        "{}{}",
        " ".repeat(layout::CONTENT_PADDING_H as usize),
        footer_text
    );
    let footer = Paragraph::new(formatted_footer).style(footer_style);
    f.render_widget(footer, chunks[2]);

    // Status bar at the very bottom (always visible)
    let (cumulative_runtime_ms, has_active_agents, active_agent_count) =
        app.data_store.borrow_mut().get_statusbar_runtime_ms();
    let audio_playing = app.audio_player.is_playing();
    render_statusbar(
        f,
        chunks[3],
        app.current_notification(),
        cumulative_runtime_ms,
        has_active_agents,
        active_agent_count,
        app.wave_offset(),
        audio_playing,
    );

    // Global modal overlays - render AppSettings modal for views that don't handle it themselves
    // (Home, Chat, and AgentBrowser handle modals in their own render functions)
    if matches!(app.view, View::Login | View::LessonViewer) {
        match &app.modal_state {
            ModalState::AppSettings(state) => {
                ui::views::render_app_settings(f, app, f.area(), state)
            }
            ModalState::BunkerApproval(state) => {
                ui::views::render_bunker_approval_modal(f, f.area(), state)
            }
            ModalState::BunkerRules(state) => {
                ui::views::render_bunker_rules_modal(f, app, f.area(), state)
            }
            ModalState::BunkerAudit(state) => {
                ui::views::render_bunker_audit_modal(f, app, f.area(), state)
            }
            _ => {}
        }
    }
}
