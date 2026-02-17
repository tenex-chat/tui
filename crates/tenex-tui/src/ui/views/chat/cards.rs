use crate::ui::{format::format_message_time, theme};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// Half-block characters for vertical padding
const LOWER_HALF_BLOCK: char = '▄';
const UPPER_HALF_BLOCK: char = '▀';

/// Create a top padding line using lower half blocks (creates bottom-half fill effect)
/// The ▄ character: fg fills bottom half, bg fills top half
/// We want: top = terminal bg (BG_APP), bottom = card color
pub(crate) fn top_half_block_line(
    indicator_color: Color,
    bg: Color,
    width: usize,
) -> Line<'static> {
    let mut spans = vec![
        // Indicator: bottom-half = indicator color, top-half = terminal bg
        Span::styled(
            LOWER_HALF_BLOCK.to_string(),
            Style::default().fg(indicator_color).bg(theme::BG_APP),
        ),
    ];
    // Rest of the line: bottom-half = card bg, top-half = terminal bg
    if width > 1 {
        let fill: String = std::iter::repeat(LOWER_HALF_BLOCK)
            .take(width - 1)
            .collect();
        spans.push(Span::styled(
            fill,
            Style::default().fg(bg).bg(theme::BG_APP),
        ));
    }
    Line::from(spans)
}

/// Create a bottom padding line using upper half blocks (creates top-half fill effect)
/// The ▀ character: fg fills top half, bg fills bottom half
/// We want: top = card color, bottom = terminal bg (BG_APP)
pub(crate) fn bottom_half_block_line(
    indicator_color: Color,
    bg: Color,
    width: usize,
) -> Line<'static> {
    let mut spans = vec![
        // Indicator: top-half = indicator color, bottom-half = terminal bg
        Span::styled(
            UPPER_HALF_BLOCK.to_string(),
            Style::default().fg(indicator_color).bg(theme::BG_APP),
        ),
    ];
    // Rest of the line: top-half = card bg, bottom-half = terminal bg
    if width > 1 {
        let fill: String = std::iter::repeat(UPPER_HALF_BLOCK)
            .take(width - 1)
            .collect();
        spans.push(Span::styled(
            fill,
            Style::default().fg(bg).bg(theme::BG_APP),
        ));
    }
    Line::from(spans)
}

/// Wrap spans to fit within max_width, splitting at word boundaries
fn wrap_spans(spans: &[Span], max_width: usize) -> Vec<Vec<Span<'static>>> {
    if max_width == 0 {
        return vec![vec![]];
    }

    let mut result: Vec<Vec<Span<'static>>> = Vec::new();
    let mut current_line: Vec<Span<'static>> = Vec::new();
    let mut current_width = 0usize;

    for span in spans {
        let style = span.style;
        let text = span.content.as_ref();

        // Handle empty spans
        if text.is_empty() {
            continue;
        }

        // Split text into words (preserving whitespace)
        let mut remaining = text;
        while !remaining.is_empty() {
            // Find next word boundary
            let (word, rest) = split_next_word(remaining);
            remaining = rest;

            let word_len = word.chars().count();

            // If word fits on current line, add it
            if current_width + word_len <= max_width {
                current_line.push(Span::styled(word.to_string(), style));
                current_width += word_len;
            } else if word_len > max_width {
                // Word is longer than max width, force break it
                let mut word_remaining = word;
                while !word_remaining.is_empty() {
                    let available = max_width.saturating_sub(current_width);
                    if available == 0 {
                        // Start new line
                        if !current_line.is_empty() {
                            result.push(current_line);
                            current_line = Vec::new();
                        }
                        current_width = 0;
                        continue;
                    }

                    let (chunk, rest) = split_at_char_boundary(word_remaining, available);
                    word_remaining = rest;

                    if !chunk.is_empty() {
                        current_line.push(Span::styled(chunk.to_string(), style));
                        current_width += chunk.chars().count();
                    }

                    if !word_remaining.is_empty() {
                        result.push(current_line);
                        current_line = Vec::new();
                        current_width = 0;
                    }
                }
            } else {
                // Word doesn't fit, start new line
                if !current_line.is_empty() {
                    result.push(current_line);
                    current_line = Vec::new();
                }
                // Skip leading whitespace on new line
                let trimmed = word.trim_start();
                if !trimmed.is_empty() {
                    current_line.push(Span::styled(trimmed.to_string(), style));
                    current_width = trimmed.chars().count();
                } else {
                    current_width = 0;
                }
            }
        }
    }

    // Don't forget the last line
    if !current_line.is_empty() {
        result.push(current_line);
    }

    // Ensure at least one empty line if input was empty
    if result.is_empty() {
        result.push(vec![]);
    }

    result
}

/// Split text at the next word boundary, returning (word_with_trailing_space, rest)
fn split_next_word(text: &str) -> (&str, &str) {
    if text.is_empty() {
        return ("", "");
    }

    // Find end of current word (including trailing whitespace)
    let mut end = 0;
    let mut in_word = false;
    let mut found_space_after_word = false;

    for (i, c) in text.char_indices() {
        if c.is_whitespace() {
            if in_word {
                found_space_after_word = true;
            }
            end = i + c.len_utf8();
        } else {
            if found_space_after_word {
                // We've found a non-space after seeing space after word
                return (&text[..end], &text[end..]);
            }
            in_word = true;
            end = i + c.len_utf8();
        }
    }

    (text, "")
}

/// Split string at character boundary, respecting UTF-8
fn split_at_char_boundary(text: &str, max_chars: usize) -> (&str, &str) {
    let mut char_count = 0;
    for (i, _) in text.char_indices() {
        if char_count >= max_chars {
            return (&text[..i], &text[i..]);
        }
        char_count += 1;
    }
    (text, "")
}

pub(crate) fn pad_line(spans: &mut Vec<Span>, current_len: usize, width: usize, bg: Color) {
    let pad = width.saturating_sub(current_len);
    if pad > 0 {
        spans.push(Span::styled(" ".repeat(pad), Style::default().bg(bg)));
    }
}

pub(crate) fn author_line(
    author: &str,
    indicator_color: Color,
    bg: Color,
    width: usize,
) -> Line<'static> {
    let mut spans = vec![
        Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
        Span::styled("  ", Style::default().bg(bg)), // 2 spaces for consistent padding
        Span::styled(
            author.to_string(),
            Style::default()
                .fg(indicator_color)
                .add_modifier(Modifier::BOLD)
                .bg(bg),
        ),
    ];
    let current_len = 3 + author.chars().count(); // "│  " + author
    pad_line(&mut spans, current_len, width, bg);
    Line::from(spans)
}

pub(crate) fn dot_line(indicator_color: Color, bg: Color, width: usize) -> Line<'static> {
    let mut spans = vec![
        Span::styled("·", Style::default().fg(indicator_color).bg(bg)),
        Span::styled("  ", Style::default().bg(bg)), // 2 spaces for consistent padding
    ];
    pad_line(&mut spans, 3, width, bg);
    Line::from(spans)
}

/// Prefix width: "│  " = 3 characters
const PREFIX_WIDTH: usize = 3;

pub(crate) fn markdown_lines(
    markdown_lines: &[Line],
    indicator_color: Color,
    bg: Color,
    width: usize,
) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(PREFIX_WIDTH);
    let mut out = Vec::new();

    for md_line in markdown_lines {
        // Wrap the content spans first
        let wrapped = wrap_spans(&md_line.spans, content_width);

        for wrapped_line in wrapped {
            let mut spans = vec![
                Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
                Span::styled("  ", Style::default().bg(bg)), // 2 spaces padding
            ];
            let mut line_len = PREFIX_WIDTH;

            for span in wrapped_line {
                line_len += span.content.chars().count();
                let mut new_style = span.style;
                new_style = new_style.bg(bg);
                spans.push(Span::styled(span.content.to_string(), new_style));
            }

            pad_line(&mut spans, line_len, width, bg);
            out.push(Line::from(spans));
        }
    }
    out
}

/// Format LLM metadata value (tokens > 1000 as "6k", cost with $, runtime in seconds)
fn format_llm_value(key: &str, value: &str) -> String {
    // Format tokens > 1000 as "14.5k"
    if key.contains("tokens") {
        if let Ok(num) = value.parse::<f64>() {
            if num >= 1000.0 {
                let formatted = format!("{:.1}k", num / 1000.0);
                // Remove trailing .0 (e.g., "6.0k" -> "6k")
                return formatted.trim_end_matches(".0k").to_string()
                    + if formatted.ends_with(".0k") { "k" } else { "" };
            }
        }
    }
    // Format cost with $ prefix
    if key == "cost-usd" {
        return format!("${}", value);
    }
    // Format runtime from ms to seconds
    if key == "runtime" {
        if let Ok(ms) = value.parse::<f64>() {
            let seconds = ms / 1000.0;
            if seconds >= 60.0 {
                // Show as minutes and seconds for longer runtimes
                let mins = (seconds / 60.0).floor();
                let secs = seconds % 60.0;
                return format!("{:.0}m{:.0}s", mins, secs);
            }
            return format!("{:.1}s", seconds);
        }
    }
    value.to_string()
}

/// Get display label for LLM metadata key
fn llm_label(key: &str) -> &str {
    match key {
        "reasoning-tokens" => "reasoning",
        "cached-input-tokens" => "cached",
        "completion-tokens" => "completion",
        "prompt-tokens" => "prompt",
        "total-tokens" => "total",
        "cost-usd" => "cost",
        "runtime" => "runtime",
        _ => key,
    }
}

/// Render markdown content for reasoning/thinking messages
/// Uses muted text color and no background
pub(crate) fn reasoning_lines(
    markdown_lines: &[Line],
    indicator_color: Color,
    width: usize,
) -> Vec<Line<'static>> {
    let content_width = width.saturating_sub(PREFIX_WIDTH);
    let mut out = Vec::new();
    let muted_style = Style::default()
        .fg(theme::TEXT_MUTED)
        .add_modifier(Modifier::ITALIC);

    for md_line in markdown_lines {
        // Wrap the content spans first
        let wrapped = wrap_spans(&md_line.spans, content_width);

        for wrapped_line in wrapped {
            let mut spans = vec![
                Span::styled("│", Style::default().fg(indicator_color)),
                Span::styled("  ", Style::default()), // 2 spaces padding, no bg
            ];

            for span in wrapped_line {
                // Apply muted style to all content
                spans.push(Span::styled(span.content.to_string(), muted_style));
            }

            // No padding to full width (no background fill needed)
            out.push(Line::from(spans));
        }
    }
    out
}

/// Render author line for reasoning/thinking messages (muted style, no bg)
pub(crate) fn reasoning_author_line(author: &str, indicator_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("│", Style::default().fg(indicator_color)),
        Span::styled("  ", Style::default()), // 2 spaces for consistent padding
        Span::styled(
            author.to_string(),
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::ITALIC),
        ),
        Span::styled(
            " (thinking)",
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::ITALIC),
        ),
    ])
}

/// Render dot line for consecutive reasoning messages (muted style, no bg)
pub(crate) fn reasoning_dot_line(indicator_color: Color) -> Line<'static> {
    Line::from(vec![
        Span::styled("·", Style::default().fg(indicator_color)),
        Span::styled("  ", Style::default()), // 2 spaces for consistent padding
    ])
}

/// Render author line with recipient showing "[from] -> [to]" format
pub(crate) fn author_line_with_recipient(
    author: &str,
    recipients: &[String],
    indicator_color: Color,
    bg: Color,
    width: usize,
) -> Line<'static> {
    let mut spans = vec![
        Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
        Span::styled("  ", Style::default().bg(bg)), // 2 spaces for consistent padding
        Span::styled(
            author.to_string(),
            Style::default()
                .fg(indicator_color)
                .add_modifier(Modifier::BOLD)
                .bg(bg),
        ),
    ];
    let mut current_len = 3 + author.chars().count(); // "│  " + author

    // Add " -> " arrow
    spans.push(Span::styled(
        " -> ",
        Style::default().fg(theme::TEXT_MUTED).bg(bg),
    ));
    current_len += 4;

    // Add recipient names
    for (i, recipient) in recipients.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(
                ", ",
                Style::default().fg(theme::TEXT_MUTED).bg(bg),
            ));
            current_len += 2;
        }
        spans.push(Span::styled(
            format!("@{}", recipient),
            Style::default().fg(theme::ACCENT_SPECIAL).bg(bg),
        ));
        current_len += 1 + recipient.len(); // "@" + name
    }

    pad_line(&mut spans, current_len, width, bg);
    Line::from(spans)
}

/// Render LLM metadata line (id, time, and token info) for a selected message
pub(crate) fn llm_metadata_line(
    message_id: &str,
    created_at: u64,
    llm_metadata: &[(String, String)],
    indicator_color: Color,
    bg: Color,
    width: usize,
) -> Line<'static> {
    let mut spans = vec![
        Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
        Span::styled("  ", Style::default().bg(bg)),
    ];
    let mut current_len = 3; // "│  "

    // Add message ID (first 12 chars)
    let id_short = &message_id[..12.min(message_id.len())];
    spans.push(Span::styled(
        format!("id:{}", id_short),
        Style::default().fg(theme::TEXT_MUTED).bg(bg),
    ));
    current_len += 3 + id_short.len(); // "id:" + id

    // Add message time
    let time_str = format_message_time(created_at);
    spans.push(Span::styled("  ", Style::default().bg(bg)));
    current_len += 2;
    spans.push(Span::styled(
        format!("@{}", time_str),
        Style::default().fg(theme::TEXT_MUTED).bg(bg),
    ));
    current_len += 1 + time_str.len(); // "@" + time

    // Add LLM metadata chips
    for (key, value) in llm_metadata {
        // Skip cost-usd if < 0.01
        if key == "cost-usd" {
            if let Ok(cost) = value.parse::<f64>() {
                if cost < 0.01 {
                    continue;
                }
            }
        }

        let label = llm_label(key);
        let formatted_value = format_llm_value(key, value);
        let color = theme::llm_metadata_color(key);

        // Add separator
        spans.push(Span::styled("  ", Style::default().bg(bg)));
        current_len += 2;

        // Add label with color
        spans.push(Span::styled(
            format!("{}:", label),
            Style::default().fg(color).bg(bg),
        ));
        current_len += label.len() + 1;

        // Add value
        spans.push(Span::styled(
            format!(" {}", formatted_value),
            Style::default().fg(theme::TEXT_PRIMARY).bg(bg),
        ));
        current_len += 1 + formatted_value.len();
    }

    pad_line(&mut spans, current_len, width, bg);
    Line::from(spans)
}
