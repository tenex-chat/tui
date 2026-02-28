use crate::RESET;
use crossterm::terminal;
use tenex_core::models::Thread;

pub(crate) fn strip_ansi(s: &str) -> String {
    let mut out = String::new();
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            while let Some(&next) = chars.peek() {
                chars.next();
                if next == 'm' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Get display name for a thread (summary or title), truncated to max_len.
pub(crate) fn thread_display_name(thread: &Thread, max_len: usize) -> String {
    let display = thread
        .summary
        .as_deref()
        .filter(|s| !s.is_empty())
        .unwrap_or(&thread.title);
    if display.len() > max_len {
        format!("{}...", &display[..max_len.saturating_sub(3)])
    } else {
        display.to_string()
    }
}

/// Render text with a wave of brightness sweeping left-to-right.
pub(crate) fn wave_colorize(text: &str, elapsed_ms: f64, palette: &[u8]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len().max(1) as f64;
    let progress = (elapsed_ms / ANIMATION_DURATION_F64).clamp(0.0, 1.0);
    let peak = progress * 1.4 - 0.2;

    let mut out = String::new();
    let pal_max = palette.len() - 1;

    for (i, ch) in chars.iter().enumerate() {
        let char_pos = i as f64 / len;
        let dist = (char_pos - peak).abs();
        let brightness = (-dist * dist * 20.0).exp();
        let idx = (brightness * pal_max as f64).round() as usize;
        let color = palette[idx.min(pal_max)];
        out.push_str(&format!("\x1b[38;5;{color}m{ch}"));
    }
    out.push_str(RESET);
    out
}

/// Format runtime in milliseconds to a human-readable string (HH:MM:SS or MM:SS)
pub(crate) fn format_runtime(total_ms: u64) -> String {
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

pub(crate) fn format_day_label(timestamp: u64) -> String {
    use chrono::{TimeZone, Datelike, Local};
    let dt = Local.timestamp_opt(timestamp as i64, 0)
        .single()
        .unwrap_or_else(Local::now);
    let weekday = match dt.weekday() {
        chrono::Weekday::Mon => "Mon",
        chrono::Weekday::Tue => "Tue",
        chrono::Weekday::Wed => "Wed",
        chrono::Weekday::Thu => "Thu",
        chrono::Weekday::Fri => "Fri",
        chrono::Weekday::Sat => "Sat",
        chrono::Weekday::Sun => "Sun",
    };
    let month = match dt.month() {
        1 => "Jan", 2 => "Feb", 3 => "Mar", 4 => "Apr",
        5 => "May", 6 => "Jun", 7 => "Jul", 8 => "Aug",
        9 => "Sep", 10 => "Oct", 11 => "Nov", 12 => "Dec",
        _ => "???",
    };
    format!("{} {} {:>2}", weekday, month, dt.day())
}

pub(crate) fn term_width() -> u16 {
    terminal::size().map(|(w, _)| w).unwrap_or(80)
}

pub(crate) const PROMPT_PREFIX_WIDTH: u16 = 4;
pub(crate) const HALF_BLOCK_LOWER: char = '▄';
pub(crate) const HALF_BLOCK_UPPER: char = '▀';

pub(crate) const TICK_INTERVAL_MS: u64 = 50;
pub(crate) const ANIMATION_DURATION_MS: u128 = 5000;
pub(crate) const ANIMATION_DURATION_F64: f64 = 5000.0;
pub(crate) const DELEGATION_STALENESS_SECS: u64 = 120;
pub(crate) const MESSAGES_TO_LOAD: usize = 10;
