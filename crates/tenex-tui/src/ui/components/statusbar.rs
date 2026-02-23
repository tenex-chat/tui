// Global status bar component displayed at the very bottom of the app
// Shows notifications on the left and cumulative unique runtime on the right

use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Style},
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
/// "Today" because this shows accumulated runtime from persistent Nostr data (RuntimeHierarchy).
const TODAY_LABEL: &str = "Today: ";

/// Minimum width for the runtime column.
/// Ensures column doesn't collapse below "Today: MM:SS " (13 chars) + 1 left padding + 3 buffer = 17
const RUNTIME_COLUMN_MIN_WIDTH: u16 = 17;

// Wave animation constants for active agent runtime display
/// Base green color (RGB) for the "Today:" runtime display when agents are active
const WAVE_BASE_COLOR_R: u8 = 106;
const WAVE_BASE_COLOR_G: u8 = 153;
const WAVE_BASE_COLOR_B: u8 = 85;

/// Phase speed multiplier - controls how fast the wave travels across the text
const WAVE_PHASE_SPEED: f32 = 0.3;

/// Wavelength - controls how many characters are in one wave cycle
/// Higher values = longer wavelength (smoother gradient)
const WAVE_WAVELENGTH: f32 = 0.8;

/// Wave period - used in the sine wave calculation
const WAVE_PERIOD: f32 = 12.0;

/// Format the full runtime label string (e.g., "Today: 05:32 ")
fn format_today_label(cumulative_runtime_ms: u64) -> String {
    format!("{}{} ", TODAY_LABEL, format_runtime(cumulative_runtime_ms))
}

/// Build a wave-animated runtime line with character-by-character color animation
///
/// Creates a traveling brightness wave effect across the runtime text by calculating
/// a sine wave for each character position. The wave creates a smooth gradient that
/// travels from left to right, making it clear that agents are actively working.
///
/// # Arguments
/// * `runtime_str` - The formatted runtime string to animate (e.g., "Today: 05:32 ")
/// * `padding` - Number of spaces to prepend for right-alignment
/// * `wave_offset` - Current animation frame offset (advances each frame)
///
/// # Returns
/// A Line containing spans with dynamically calculated colors for the wave effect
fn build_wave_runtime_line(
    runtime_str: &str,
    padding: usize,
    wave_offset: usize,
    active_agent_count: usize,
) -> Line<'static> {
    let mut spans = vec![Span::raw(" ".repeat(padding))];

    // Dynamic parameters based on active agent count
    // Speed: 1 agent = 0.3 (baseline), 10 agents = 3.0 (10x faster)
    let agent_count_clamped = active_agent_count.max(1).min(10) as f32;
    let speed_multiplier = 0.3 * agent_count_clamped;

    // Brightness amplitude: 1 agent = ±0.3 (baseline), 10 agents = ±0.6 (double brightness range)
    let brightness_amplitude = 0.3 + (0.3 * (agent_count_clamped - 1.0) / 9.0);

    // Create a smooth brightness wave that travels across the text
    for (i, ch) in runtime_str.chars().enumerate() {
        // Create a traveling sine wave
        // wave_offset moves the wave (scaled by agent count), i determines position along the string
        let phase = ((wave_offset as f32 * WAVE_PHASE_SPEED * speed_multiplier)
            + (i as f32 * WAVE_WAVELENGTH))
            * std::f32::consts::PI
            * 2.0
            / WAVE_PERIOD;

        // Sine wave gives us a value between -1 and 1
        let wave_value = phase.sin();

        // Map sine wave to brightness multiplier based on dynamic amplitude
        let brightness = 1.0 + (wave_value * brightness_amplitude);

        // Apply brightness to base green color
        let r = ((WAVE_BASE_COLOR_R as f32 * brightness).min(255.0).max(0.0)) as u8;
        let g = ((WAVE_BASE_COLOR_G as f32 * brightness).min(255.0).max(0.0)) as u8;
        let b = ((WAVE_BASE_COLOR_B as f32 * brightness).min(255.0).max(0.0)) as u8;

        let color = Color::Rgb(r, g, b);
        spans.push(Span::styled(ch.to_string(), Style::default().fg(color)));
    }

    Line::from(spans)
}

/// Calculate the width needed for the runtime string
fn calculate_runtime_width(cumulative_runtime_ms: u64) -> u16 {
    let runtime_str = format_today_label(cumulative_runtime_ms);
    // Add 1 for left padding, ensure minimum width
    (runtime_str.width() + 1).max(RUNTIME_COLUMN_MIN_WIDTH as usize) as u16
}

/// Render the status bar at the bottom of the screen
/// Shows notification (left/center), optional audio indicator, and cumulative runtime (right)
/// Uses fixed-width columns to prevent layout breakage from long notifications
///
/// ## Color Logic
/// - `has_active_agents = true`: "Today:" label shown in GREEN with wave animation (agents working)
/// - `has_active_agents = false`: "Today:" label shown in RED (no agents working)
/// - `audio_playing = true`: Shows audio indicator icon
pub fn render_statusbar(
    f: &mut Frame,
    area: Rect,
    current_notification: Option<&Notification>,
    cumulative_runtime_ms: u64,
    has_active_agents: bool,
    active_agent_count: usize,
    wave_offset: usize,
    audio_playing: bool,
) {
    // Calculate dynamic width for runtime column based on actual content
    let runtime_column_width = calculate_runtime_width(cumulative_runtime_ms);

    // Audio indicator width: "🔊 " = 3 chars when playing, 0 when not
    let audio_indicator_width = if audio_playing { 4 } else { 0 };

    // Split into columns: notification (flexible) | audio indicator (optional) | runtime (dynamic width based on content)
    let constraints = if audio_playing {
        vec![
            Constraint::Min(0),                        // Notification (fills remaining space)
            Constraint::Length(audio_indicator_width), // Audio indicator
            Constraint::Length(runtime_column_width),  // Runtime (dynamic width)
        ]
    } else {
        vec![
            Constraint::Min(0),                       // Notification (fills remaining space)
            Constraint::Length(runtime_column_width), // Runtime (dynamic width)
        ]
    };
    let chunks = Layout::horizontal(constraints).split(area);

    let notification_area = chunks[0];
    let runtime_area = if audio_playing { chunks[2] } else { chunks[1] };

    // Render notification (left side) - truncate to fit available width
    let notification_paragraph = if let Some(notification) = current_notification {
        let (icon, color) = match notification.level {
            NotificationLevel::Info => ("\u{2139}", theme::ACCENT_PRIMARY), // ℹ
            NotificationLevel::Success => ("\u{2713}", theme::ACCENT_SUCCESS), // ✓
            NotificationLevel::Warning => ("\u{26A0}", theme::ACCENT_WARNING), // ⚠
            NotificationLevel::Error => ("\u{2717}", theme::ACCENT_ERROR),  // ✗
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

    // Render audio indicator (middle) if playing
    if audio_playing {
        let audio_area = chunks[1];
        // Animate the speaker icon based on wave_offset for a pulsing effect
        let audio_icon = if wave_offset % 4 < 2 { "🔊" } else { "🔈" };
        let audio_line = Line::from(Span::styled(
            format!("{} ", audio_icon),
            Style::default().fg(theme::ACCENT_SUCCESS),
        ));
        let audio_paragraph =
            Paragraph::new(audio_line).style(Style::default().bg(theme::BG_SIDEBAR));
        f.render_widget(audio_paragraph, audio_area);
    }

    // Render runtime (right side) - right-aligned within its fixed column
    // Color indicates agent activity: GREEN = agents working, RED = no agents working
    // When agents are active, apply a wave animation character by character
    let runtime_str = format_today_label(cumulative_runtime_ms);
    let runtime_width = runtime_str.width();
    let padding = (runtime_area.width as usize).saturating_sub(runtime_width);

    let runtime_line = if has_active_agents {
        // GREEN mode with wave animation
        build_wave_runtime_line(&runtime_str, padding, wave_offset, active_agent_count)
    } else {
        // RED mode - no animation
        let padded_runtime = format!("{}{}", " ".repeat(padding), runtime_str);
        Line::from(Span::styled(
            padded_runtime,
            Style::default().fg(theme::ACCENT_ERROR),
        ))
    };

    let runtime_paragraph =
        Paragraph::new(runtime_line).style(Style::default().bg(theme::BG_SIDEBAR));

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
