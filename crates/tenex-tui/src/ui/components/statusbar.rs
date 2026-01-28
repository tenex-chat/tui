// Global status bar component displayed at the very bottom of the app
// Shows notifications on the left and cumulative unique runtime on the right

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use unicode_segmentation::UnicodeSegmentation;
use unicode_width::UnicodeWidthStr;

use crate::ui::notifications::{Notification, NotificationLevel};
use crate::ui::theme;

/// Format runtime in milliseconds to a human-readable string (HH:MM:SS or MM:SS)
fn format_runtime(total_ms: u64) -> String {
    let total_seconds = total_ms / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;

    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

/// Label prefix for the runtime display.
/// "Session" because this is in-memory runtime that resets on application restart.
const SESSION_LABEL: &str = "Session: ";

/// Minimum width for the runtime column.
/// Ensures column doesn't collapse below "Session: MM:SS " (15 chars) + 1 left padding + 2 buffer = 18
const RUNTIME_COLUMN_MIN_WIDTH: u16 = 18;

/// Format the full runtime label string (e.g., "Session: 05:32 ")
fn format_session_label(cumulative_runtime_ms: u64) -> String {
    format!("{}{} ", SESSION_LABEL, format_runtime(cumulative_runtime_ms))
}

/// Calculate the width needed for the runtime string
fn calculate_runtime_width(cumulative_runtime_ms: u64) -> u16 {
    let runtime_str = format_session_label(cumulative_runtime_ms);
    // Add 1 for left padding, ensure minimum width
    (runtime_str.width() + 1).max(RUNTIME_COLUMN_MIN_WIDTH as usize) as u16
}

/// Render the status bar at the bottom of the screen
/// Shows notification (left/center) and cumulative runtime (right)
/// Uses fixed-width columns to prevent layout breakage from long notifications
///
/// ## Color Logic
/// - `has_active_agents = true`: "Session:" label shown in GREEN (agents working)
/// - `has_active_agents = false`: "Session:" label shown in RED (no agents working)
pub fn render_statusbar(
    f: &mut Frame,
    area: Rect,
    current_notification: Option<&Notification>,
    cumulative_runtime_ms: u64,
    has_active_agents: bool,
) {
    // Calculate dynamic width for runtime column based on actual content
    let runtime_column_width = calculate_runtime_width(cumulative_runtime_ms);

    // Split into columns: notification (flexible) | runtime (dynamic width based on content)
    let chunks = Layout::horizontal([
        Constraint::Min(0),                        // Notification (fills remaining space)
        Constraint::Length(runtime_column_width), // Runtime (dynamic width)
    ])
    .split(area);

    let notification_area = chunks[0];
    let runtime_area = chunks[1];

    // Render notification (left side) - truncate to fit available width
    let notification_paragraph = if let Some(notification) = current_notification {
        let (icon, color) = match notification.level {
            NotificationLevel::Info => ("\u{2139}", theme::ACCENT_PRIMARY),    // ℹ
            NotificationLevel::Success => ("\u{2713}", theme::ACCENT_SUCCESS), // ✓
            NotificationLevel::Warning => ("\u{26A0}", theme::ACCENT_WARNING), // ⚠
            NotificationLevel::Error => ("\u{2717}", theme::ACCENT_ERROR),     // ✗
        };

        // Calculate available width for message (account for icon + spaces)
        let icon_width = icon.width() + 2; // " icon " = icon + 2 spaces
        let available_for_message = (notification_area.width as usize).saturating_sub(icon_width);

        // Truncate message with ellipsis if needed
        let message = truncate_with_ellipsis(&notification.message, available_for_message);

        let spans = vec![
            Span::styled(format!(" {} ", icon), Style::default().fg(color)),
            Span::styled(message, Style::default().fg(color)),
        ];
        Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::BG_SIDEBAR))
    } else {
        Paragraph::new("").style(Style::default().bg(theme::BG_SIDEBAR))
    };

    f.render_widget(notification_paragraph, notification_area);

    // Render runtime (right side) - right-aligned within its fixed column
    // Color indicates agent activity: GREEN = agents working, RED = no agents working
    let runtime_color = if has_active_agents {
        theme::ACCENT_SUCCESS // Green - agents are actively working
    } else {
        theme::ACCENT_ERROR   // Red - no agents working
    };

    let runtime_str = format_session_label(cumulative_runtime_ms);
    let runtime_width = runtime_str.width();
    let padding = (runtime_area.width as usize).saturating_sub(runtime_width);
    let padded_runtime = format!("{}{}", " ".repeat(padding), runtime_str);

    let runtime_paragraph = Paragraph::new(padded_runtime)
        .style(Style::default().fg(runtime_color).bg(theme::BG_SIDEBAR));

    f.render_widget(runtime_paragraph, runtime_area);
}

/// Truncate a string to fit within max_width, adding ellipsis if needed.
/// Uses grapheme-aware truncation to avoid splitting emoji/combining characters.
/// Width-aware for all branches (handles CJK/emoji correctly even for small widths).
fn truncate_with_ellipsis(s: &str, max_width: usize) -> String {
    let width = s.width();
    if width <= max_width {
        return s.to_string();
    }

    if max_width == 0 {
        return String::new();
    }

    // For small widths (can't fit ellipsis), truncate grapheme-by-grapheme with width awareness
    if max_width <= 3 {
        let mut current_width = 0;
        let mut result = String::new();

        for grapheme in s.graphemes(true) {
            let grapheme_width = grapheme.width();
            if current_width + grapheme_width > max_width {
                break;
            }
            result.push_str(grapheme);
            current_width += grapheme_width;
        }

        return result;
    }

    // Normal case: truncate to leave room for "..."
    let target_width = max_width - 3;
    let mut current_width = 0;
    let mut result = String::new();

    for grapheme in s.graphemes(true) {
        let grapheme_width = grapheme.width();
        if current_width + grapheme_width > target_width {
            break;
        }
        result.push_str(grapheme);
        current_width += grapheme_width;
    }

    result.push_str("...");
    result
}
