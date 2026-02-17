use crate::models::Thread;
use crate::ui::card;
use crate::ui::format::{format_relative_time, truncate_with_ellipsis};
use crate::ui::views::home_helpers::HierarchicalThread;
use crate::ui::{layout, theme, App, HomeTab};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Padding, Paragraph, Row, Table},
    Frame,
};
use unicode_width::UnicodeWidthStr;

pub(super) fn render_project_sidebar(f: &mut Frame, app: &App, area: Rect) {
    // If sidebar search is visible, add search input at top
    if app.sidebar_search.visible {
        let chunks = Layout::vertical([
            Constraint::Length(3), // Search input
            Constraint::Min(5),    // Projects list
        ])
        .split(area);

        render_sidebar_search_input(f, app, chunks[0]);
        render_projects_list(f, app, chunks[1]);
    } else {
        // Normal layout without search - just projects list
        render_projects_list(f, app, area);
    }
}

/// Render the sidebar search input
fn render_sidebar_search_input(f: &mut Frame, app: &App, area: Rect) {
    // Border block with title
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
        .title(Span::styled(
            " Search ",
            Style::default().fg(theme::ACCENT_PRIMARY),
        ));
    let inner = block.inner(area);
    f.render_widget(block, area);

    // Search query with cursor (cursor_pos is a char index, not byte index)
    let query = &app.sidebar_search.query;
    let cursor_pos = app.sidebar_search.cursor;
    let char_count = query.chars().count();

    // Build the search line with cursor indicator
    let mut spans = Vec::new();

    // Text before cursor
    if cursor_pos > 0 {
        let before: String = query.chars().take(cursor_pos).collect();
        spans.push(Span::styled(
            before,
            Style::default().fg(theme::TEXT_PRIMARY),
        ));
    }

    // Cursor (block character when focused)
    let cursor_char = if cursor_pos < char_count {
        query.chars().nth(cursor_pos).unwrap_or(' ')
    } else {
        ' '
    };
    spans.push(Span::styled(
        cursor_char.to_string(),
        Style::default().fg(theme::BG_APP).bg(theme::TEXT_PRIMARY),
    ));

    // Text after cursor
    if cursor_pos < char_count {
        let after: String = query.chars().skip(cursor_pos + 1).collect();
        spans.push(Span::styled(
            after,
            Style::default().fg(theme::TEXT_PRIMARY),
        ));
    }

    // Placeholder when empty (different hints for different tabs)
    if query.is_empty() {
        let placeholder = if app.home_panel_focus == HomeTab::Reports {
            "type to search..."
        } else {
            "type to search (use + for AND)..."
        };
        spans.push(Span::styled(
            placeholder,
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }

    let search_line = Paragraph::new(Line::from(spans));
    f.render_widget(search_line, inner);
}

/// Render the sidebar search results in the main content area
pub(super) fn render_sidebar_search_results(f: &mut Frame, app: &App, area: Rect) {
    // Delegate to appropriate renderer based on current tab
    if app.home_panel_focus == HomeTab::Reports {
        render_report_search_results(f, app, area);
    } else {
        render_conversation_search_results(f, app, area);
    }
}

/// Render conversation search results with hierarchical display
fn render_conversation_search_results(f: &mut Frame, app: &App, area: Rect) {
    use crate::ui::search::HierarchicalSearchItem;

    let results = &app.sidebar_search.hierarchical_results;
    let selected_idx = app
        .sidebar_search
        .selected_index
        .min(results.len().saturating_sub(1));
    let query = &app.sidebar_search.query;

    if results.is_empty() {
        let msg = "No matching conversations";
        let empty = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty, area);
        return;
    }

    // Helper to compute card height for a hierarchical item
    fn compute_item_height(item: &HierarchicalSearchItem) -> u16 {
        match item {
            HierarchicalSearchItem::ContextAncestor { .. } => {
                // Context ancestors are compact: just title line
                1
            }
            HierarchicalSearchItem::MatchedConversation {
                matching_messages, ..
            } => {
                // Title line + up to 3 matching message previews (each 2 lines: arrow prefix + content)
                let msg_lines = matching_messages.len().min(3) as u16 * 2;
                1 + msg_lines + 1 // title + messages + spacing
            }
        }
    }

    // Calculate available height (reserve 1 line for count at bottom)
    let available_height = area.height.saturating_sub(1);

    // Compute heights and scroll offset
    let mut cumulative_before_selected: u16 = 0;
    let mut heights_cache: Vec<u16> = Vec::with_capacity(selected_idx + 1);
    for i in 0..=selected_idx.min(results.len().saturating_sub(1)) {
        let h = compute_item_height(&results[i]);
        heights_cache.push(h);
        if i < selected_idx {
            cumulative_before_selected += h;
        }
    }
    let selected_height = if selected_idx < heights_cache.len() {
        heights_cache[selected_idx]
    } else {
        1
    };

    // Calculate scroll offset
    let scroll_offset = if cumulative_before_selected + selected_height <= available_height {
        0
    } else {
        let mut offset = 0;
        let mut height_sum: u16 = heights_cache.iter().sum();
        while height_sum > available_height && offset < selected_idx {
            height_sum -= heights_cache[offset];
            offset += 1;
        }
        offset
    };

    let store = app.data_store.borrow();
    let mut y_offset = 0u16;

    // Count actual matches (not context ancestors)
    let match_count = results.iter().filter(|r| !r.is_context_ancestor()).count();

    // Render items starting from scroll_offset
    for (i, item) in results.iter().enumerate().skip(scroll_offset) {
        let is_selected = i == selected_idx;
        let card_height = if i < heights_cache.len() {
            heights_cache[i]
        } else {
            compute_item_height(item)
        };

        if y_offset + card_height > available_height {
            break;
        }

        let card_area = Rect::new(area.x, area.y + y_offset, area.width, card_height);

        render_hierarchical_search_item(f, item, is_selected, card_area, &store, query);
        y_offset += card_height;
    }

    // Show result count at bottom
    let count_text = format!(
        "{} match{}",
        match_count,
        if match_count == 1 { "" } else { "es" }
    );
    let count_area = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1),
        area.width,
        1,
    );
    let count_line = Paragraph::new(count_text)
        .style(Style::default().fg(theme::TEXT_MUTED))
        .alignment(ratatui::layout::Alignment::Right);
    f.render_widget(count_line, count_area);
}

/// Render a single hierarchical search item
fn render_hierarchical_search_item(
    f: &mut Frame,
    item: &crate::ui::search::HierarchicalSearchItem,
    is_selected: bool,
    area: Rect,
    store: &std::cell::Ref<crate::store::AppDataStore>,
    _query: &str, // Kept for API compatibility; matched_terms from item is used instead
) {
    use crate::ui::search::HierarchicalSearchItem;

    let depth = item.depth();
    // Indentation: 2 spaces per depth level
    let indent = "  ".repeat(depth);
    let indent_width = depth * 2;

    match item {
        HierarchicalSearchItem::ContextAncestor {
            thread_title,
            project_a_tag,
            ..
        } => {
            // Context ancestors are dimmed and compact
            let title_max = (area.width as usize)
                .saturating_sub(indent_width)
                .saturating_sub(5)
                .max(10);
            let title = crate::ui::format::truncate_with_ellipsis(thread_title, title_max);

            let style = if is_selected {
                Style::default()
                    .fg(theme::TEXT_MUTED)
                    .bg(theme::BG_SELECTED)
            } else {
                Style::default()
                    .fg(theme::TEXT_MUTED)
                    .add_modifier(Modifier::DIM)
            };

            let line = Line::from(vec![
                Span::styled(&indent, Style::default()),
                Span::styled(title, style),
                Span::styled("  ", Style::default()),
                Span::styled(
                    store.get_project_name(project_a_tag),
                    Style::default()
                        .fg(theme::project_color(project_a_tag))
                        .add_modifier(Modifier::DIM),
                ),
            ]);

            let para = Paragraph::new(vec![line]);
            if is_selected {
                f.render_widget(para.style(Style::default().bg(theme::BG_SELECTED)), area);
            } else {
                f.render_widget(para, area);
            }
        }
        HierarchicalSearchItem::MatchedConversation {
            thread_title,
            project_a_tag,
            project_name,
            matching_messages,
            title_matched,
            content_matched,
            id_matched,
            matched_terms,
            ..
        } => {
            let mut lines: Vec<Line> = Vec::new();

            // Title line with match type indicator and highlighting
            // For multi-term search, show [+] indicator
            let is_multi_term = matched_terms.len() > 1;
            let type_indicator = if is_multi_term {
                "[+]" // Multi-term AND match
            } else if *id_matched {
                "[I]"
            } else if *title_matched {
                "[T]"
            } else if *content_matched {
                "[C]"
            } else {
                "[R]"
            };
            let type_color = if is_multi_term {
                theme::ACCENT_WARNING // Special color for multi-term matches
            } else if *id_matched {
                theme::TEXT_MUTED
            } else if *title_matched {
                theme::ACCENT_PRIMARY
            } else if *content_matched {
                theme::ACCENT_WARNING
            } else {
                theme::ACCENT_SUCCESS
            };

            let title_max = (area.width as usize)
                .saturating_sub(indent_width)
                .saturating_sub(30)
                .max(10);

            // Highlight matching text in title if title matched
            // For multi-term, highlight all matching terms
            let title_spans = if *title_matched {
                highlight_text_spans_multi(
                    thread_title,
                    matched_terms,
                    theme::TEXT_PRIMARY,
                    theme::ACCENT_PRIMARY,
                )
            } else {
                vec![Span::styled(
                    crate::ui::format::truncate_with_ellipsis(thread_title, title_max),
                    if is_selected {
                        Style::default()
                            .fg(theme::ACCENT_PRIMARY)
                            .add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::TEXT_PRIMARY)
                    },
                )]
            };

            let mut title_line_spans = vec![
                Span::styled(&indent, Style::default()),
                Span::styled(type_indicator, Style::default().fg(type_color)),
                Span::styled(" ", Style::default()),
            ];
            title_line_spans.extend(title_spans);
            title_line_spans.push(Span::styled("  ", Style::default()));
            title_line_spans.push(Span::styled(
                project_name.as_str(),
                Style::default().fg(theme::project_color(project_a_tag)),
            ));

            lines.push(Line::from(title_line_spans));

            // Matching message previews (up to 3)
            let message_indent = format!("{}  -> ", indent);
            let content_width = (area.width as usize)
                .saturating_sub(message_indent.len())
                .saturating_sub(2)
                .max(10);

            for msg in matching_messages.iter().take(3) {
                // Author line
                let author_name = store.get_profile_name(&msg.author_pubkey);
                let author_color = theme::user_color(&msg.author_pubkey);
                lines.push(Line::from(vec![
                    Span::styled(format!("{}  ", indent), Style::default()),
                    Span::styled("@", Style::default().fg(theme::TEXT_MUTED)),
                    Span::styled(author_name, Style::default().fg(author_color)),
                ]));

                // Message content with bracket highlighting (supports multi-term)
                let preview: String = msg
                    .content
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(content_width)
                    .collect();
                let highlighted_spans =
                    build_bracket_highlight_spans_multi(&preview, matched_terms, content_width);
                let mut content_line_spans = vec![Span::styled(
                    &message_indent,
                    Style::default().fg(theme::TEXT_MUTED),
                )];
                content_line_spans.extend(highlighted_spans);
                lines.push(Line::from(content_line_spans));
            }

            // Spacing line
            lines.push(Line::from(""));

            let para = Paragraph::new(lines);
            if is_selected {
                f.render_widget(para.style(Style::default().bg(theme::BG_SELECTED)), area);
            } else {
                f.render_widget(para, area);
            }
        }
    }
}

/// Build highlighted spans with [brackets] around matching text
/// Uses char indices to avoid Unicode byte offset panics
fn build_bracket_highlight_spans(text: &str, query: &str, _max_width: usize) -> Vec<Span<'static>> {
    if query.is_empty() {
        return vec![Span::styled(
            text.to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        )];
    }

    let query_chars: Vec<char> = query.chars().collect();
    let query_char_count = query_chars.len();
    let text_chars: Vec<char> = text.chars().collect();
    let mut spans = Vec::new();
    let mut last_char_end = 0;

    // Find all matches using char indices (ASCII case-insensitive)
    let mut i = 0;
    while i <= text_chars.len().saturating_sub(query_char_count) {
        if chars_match_ascii_ignore_case(&text_chars, &query_chars, i) {
            // Add text before match
            if i > last_char_end {
                let before: String = text_chars[last_char_end..i].iter().collect();
                spans.push(Span::styled(before, Style::default().fg(theme::TEXT_MUTED)));
            }
            // Add [matched] text with brackets and highlighting
            let match_text: String = text_chars[i..i + query_char_count].iter().collect();
            spans.push(Span::styled(
                format!("[{}]", match_text),
                Style::default()
                    .fg(theme::ACCENT_WARNING)
                    .add_modifier(Modifier::BOLD),
            ));
            last_char_end = i + query_char_count;
            i = last_char_end;
        } else {
            i += 1;
        }
    }

    // Add remaining text
    if last_char_end < text_chars.len() {
        let remaining: String = text_chars[last_char_end..].iter().collect();
        spans.push(Span::styled(
            remaining,
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }

    if spans.is_empty() {
        vec![Span::styled(
            text.to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        )]
    } else {
        spans
    }
}

/// Build highlighted spans with [brackets] around matching text for multiple terms
/// Each term is highlighted where it appears in the text
fn build_bracket_highlight_spans_multi(
    text: &str,
    terms: &[String],
    _max_width: usize,
) -> Vec<Span<'static>> {
    if terms.is_empty() {
        return vec![Span::styled(
            text.to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        )];
    }

    // For single term, delegate to existing function
    if terms.len() == 1 {
        return build_bracket_highlight_spans(text, &terms[0], _max_width);
    }

    let text_chars: Vec<char> = text.chars().collect();
    let text_len = text_chars.len();

    // Find all match ranges for all terms
    let mut matches: Vec<(usize, usize)> = Vec::new(); // (start_char_idx, end_char_idx)

    for term in terms {
        let term_chars: Vec<char> = term.chars().collect();
        let term_len = term_chars.len();
        if term_len == 0 {
            continue;
        }

        let mut i = 0;
        while i <= text_len.saturating_sub(term_len) {
            if chars_match_ascii_ignore_case(&text_chars, &term_chars, i) {
                matches.push((i, i + term_len));
                i += term_len; // Skip past this match
            } else {
                i += 1;
            }
        }
    }

    // Sort matches by start position
    matches.sort_by_key(|(start, _)| *start);

    // Merge overlapping matches
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in matches {
        if let Some((_, last_end)) = merged.last_mut() {
            if start <= *last_end {
                // Overlapping or adjacent - extend the previous match
                *last_end = (*last_end).max(end);
            } else {
                merged.push((start, end));
            }
        } else {
            merged.push((start, end));
        }
    }

    // Build spans
    let mut spans = Vec::new();
    let mut last_end = 0;

    for (start, end) in merged {
        // Add text before match
        if start > last_end {
            let before: String = text_chars[last_end..start].iter().collect();
            spans.push(Span::styled(before, Style::default().fg(theme::TEXT_MUTED)));
        }
        // Add [matched] text with brackets and highlighting
        let match_text: String = text_chars[start..end].iter().collect();
        spans.push(Span::styled(
            format!("[{}]", match_text),
            Style::default()
                .fg(theme::ACCENT_WARNING)
                .add_modifier(Modifier::BOLD),
        ));
        last_end = end;
    }

    // Add remaining text
    if last_end < text_len {
        let remaining: String = text_chars[last_end..].iter().collect();
        spans.push(Span::styled(
            remaining,
            Style::default().fg(theme::TEXT_MUTED),
        ));
    }

    if spans.is_empty() {
        vec![Span::styled(
            text.to_string(),
            Style::default().fg(theme::TEXT_MUTED),
        )]
    } else {
        spans
    }
}

/// Highlight text spans for multiple search terms
/// Similar to highlight_text_spans but supports multiple terms
fn highlight_text_spans_multi(
    text: &str,
    terms: &[String],
    normal_color: ratatui::style::Color,
    highlight_color: ratatui::style::Color,
) -> Vec<Span<'static>> {
    if terms.is_empty() {
        return vec![Span::styled(
            text.to_string(),
            Style::default().fg(normal_color),
        )];
    }

    // For single term, delegate to existing function
    if terms.len() == 1 {
        return highlight_text_spans(text, &terms[0], normal_color, highlight_color);
    }

    let text_chars: Vec<char> = text.chars().collect();
    let text_len = text_chars.len();

    // Find all match ranges for all terms
    let mut matches: Vec<(usize, usize)> = Vec::new();

    for term in terms {
        let term_chars: Vec<char> = term.chars().collect();
        let term_len = term_chars.len();
        if term_len == 0 {
            continue;
        }

        let mut i = 0;
        while i <= text_len.saturating_sub(term_len) {
            if chars_match_ascii_ignore_case(&text_chars, &term_chars, i) {
                matches.push((i, i + term_len));
                i += term_len;
            } else {
                i += 1;
            }
        }
    }

    // Sort and merge overlapping matches
    matches.sort_by_key(|(start, _)| *start);
    let mut merged: Vec<(usize, usize)> = Vec::new();
    for (start, end) in matches {
        if let Some((_, last_end)) = merged.last_mut() {
            if start <= *last_end {
                *last_end = (*last_end).max(end);
            } else {
                merged.push((start, end));
            }
        } else {
            merged.push((start, end));
        }
    }

    // Build spans
    let mut spans = Vec::new();
    let mut last_end = 0;

    for (start, end) in merged {
        if start > last_end {
            let before: String = text_chars[last_end..start].iter().collect();
            spans.push(Span::styled(before, Style::default().fg(normal_color)));
        }
        let match_text: String = text_chars[start..end].iter().collect();
        spans.push(Span::styled(
            match_text,
            Style::default()
                .fg(highlight_color)
                .add_modifier(Modifier::BOLD),
        ));
        last_end = end;
    }

    if last_end < text_len {
        let remaining: String = text_chars[last_end..].iter().collect();
        spans.push(Span::styled(remaining, Style::default().fg(normal_color)));
    }

    if spans.is_empty() {
        vec![Span::styled(
            text.to_string(),
            Style::default().fg(normal_color),
        )]
    } else {
        spans
    }
}

/// Render report search results
fn render_report_search_results(f: &mut Frame, app: &App, area: Rect) {
    let results = &app.sidebar_search.report_results;
    let selected_idx = app
        .sidebar_search
        .selected_index
        .min(results.len().saturating_sub(1));
    let query = &app.sidebar_search.query;

    if results.is_empty() {
        let msg = "No matching reports";
        let empty = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty, area);
        return;
    }

    // Card height is fixed at 4 lines for reports (Title + Summary + Spacing)
    let card_height = 4u16;
    // Available height (reserve 1 line for count at bottom)
    let available_height = area.height.saturating_sub(1);
    // Calculate how many items can be visible at once
    let visible_count = (available_height / card_height) as usize;

    // Compute scroll_offset to ensure selected item is visible
    // Guard: if available_height < card_height, visible_count is 0 - don't scroll
    let scroll_offset = if visible_count == 0 {
        0
    } else if selected_idx >= visible_count {
        selected_idx - visible_count + 1
    } else {
        0
    };

    let mut y_offset = 0u16;
    let query_lower = query.to_lowercase();

    // Render items starting from scroll_offset
    for (i, report) in results.iter().enumerate().skip(scroll_offset) {
        let is_selected = i == selected_idx;

        if y_offset + card_height > available_height {
            break;
        }

        let card_area = Rect::new(area.x, area.y + y_offset, area.width, card_height);

        render_report_search_result_card(f, report, is_selected, card_area, &query_lower);
        y_offset += card_height;
    }

    // Show result count at bottom
    let result_count = results.len();
    let count_text = format!(
        "{} report{}",
        result_count,
        if result_count == 1 { "" } else { "s" }
    );
    let count_area = Rect::new(
        area.x,
        area.y + area.height.saturating_sub(1),
        area.width,
        1,
    );
    let count_line = Paragraph::new(count_text)
        .style(Style::default().fg(theme::TEXT_MUTED))
        .alignment(ratatui::layout::Alignment::Right);
    f.render_widget(count_line, count_area);
}

/// Render a single report search result card
fn render_report_search_result_card(
    f: &mut Frame,
    report: &tenex_core::models::Report,
    is_selected: bool,
    area: Rect,
    query: &str,
) {
    let bg = if is_selected {
        theme::BG_SELECTED
    } else {
        theme::BG_CARD
    };

    // Background
    let block = Block::default()
        .style(Style::default().bg(bg))
        .padding(Padding::horizontal(1));
    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height == 0 {
        return;
    }

    // Line 1: Title with highlighting
    let title_line = highlight_text_spans(
        &report.title,
        query,
        theme::TEXT_PRIMARY,
        theme::ACCENT_PRIMARY,
    );
    let title_para = Paragraph::new(Line::from(title_line));
    let title_area = Rect::new(inner.x, inner.y, inner.width, 1);
    f.render_widget(title_para, title_area);

    // Line 2: Summary (truncated) with highlighting
    if inner.height > 1 {
        let summary: String = report.summary.chars().take(100).collect();
        let summary_line =
            highlight_text_spans(&summary, query, theme::TEXT_MUTED, theme::ACCENT_PRIMARY);
        let summary_para = Paragraph::new(Line::from(summary_line));
        let summary_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
        f.render_widget(summary_para, summary_area);
    }

    // Line 3: Hashtags
    if inner.height > 2 && !report.hashtags.is_empty() {
        let tags = report
            .hashtags
            .iter()
            .take(5)
            .map(|t| format!("#{}", t))
            .collect::<Vec<_>>()
            .join(" ");
        let tags_para = Paragraph::new(tags).style(Style::default().fg(theme::ACCENT_SPECIAL));
        let tags_area = Rect::new(inner.x, inner.y + 2, inner.width, 1);
        f.render_widget(tags_para, tags_area);
    }
}

/// Check if query chars match at position in text chars using ASCII case-insensitive comparison
/// This avoids Unicode casefold expansion issues (e.g., Turkish İ → i̇)
fn chars_match_ascii_ignore_case(
    text_chars: &[char],
    query_chars: &[char],
    start_idx: usize,
) -> bool {
    query_chars.iter().enumerate().all(|(i, qc)| {
        text_chars
            .get(start_idx + i)
            .map_or(false, |tc| tc.eq_ignore_ascii_case(qc))
    })
}

/// Highlight matching text in a string with spans
fn highlight_text_spans(
    text: &str,
    query: &str,
    normal_color: ratatui::style::Color,
    highlight_color: ratatui::style::Color,
) -> Vec<Span<'static>> {
    if query.is_empty() {
        return vec![Span::styled(
            text.to_string(),
            Style::default().fg(normal_color),
        )];
    }

    let query_chars: Vec<char> = query.chars().collect();
    let query_char_count = query_chars.len();
    let mut spans = Vec::new();
    let mut last_char_end = 0;

    // Build a char-indexed search by iterating through characters
    let chars: Vec<char> = text.chars().collect();

    let mut i = 0;
    while i <= chars.len().saturating_sub(query_char_count) {
        // Check if query matches at position i (ASCII case-insensitive)
        if chars_match_ascii_ignore_case(&chars, &query_chars, i) {
            // Add text before match
            if i > last_char_end {
                let before: String = chars[last_char_end..i].iter().collect();
                spans.push(Span::styled(before, Style::default().fg(normal_color)));
            }
            // Add highlighted match (from original text)
            let match_text: String = chars[i..i + query_char_count].iter().collect();
            spans.push(Span::styled(
                match_text,
                Style::default()
                    .fg(highlight_color)
                    .add_modifier(Modifier::BOLD),
            ));
            last_char_end = i + query_char_count;
            i = last_char_end;
        } else {
            i += 1;
        }
    }

    // Add remaining text
    if last_char_end < chars.len() {
        let remaining: String = chars[last_char_end..].iter().collect();
        spans.push(Span::styled(remaining, Style::default().fg(normal_color)));
    }

    if spans.is_empty() {
        vec![Span::styled(
            text.to_string(),
            Style::default().fg(normal_color),
        )]
    } else {
        spans
    }
}

/// Render the projects list with checkboxes
fn render_projects_list(f: &mut Frame, app: &App, area: Rect) {
    let (online_projects, offline_projects) = app.filtered_projects();

    let mut items: Vec<ListItem> = Vec::new();

    // Calculate which item index is selected (0-based, not accounting for headers)
    let selected_project_index = if app.sidebar_focused {
        Some(app.sidebar_project_index)
    } else {
        None
    };

    // Online section header
    if !online_projects.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(card::BULLET, Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(
                "Online",
                Style::default()
                    .fg(theme::ACCENT_SUCCESS)
                    .add_modifier(Modifier::BOLD),
            ),
        ])));
    }

    // Online projects - now empty = none (inverted)
    for (i, project) in online_projects.iter().enumerate() {
        let a_tag = project.a_tag();
        let is_visible = app.visible_projects.contains(&a_tag);
        let is_focused = selected_project_index == Some(i);
        let is_busy = app.data_store.borrow().operations.is_project_busy(&a_tag);
        let is_archived = app.is_project_archived(&a_tag);

        let checkbox = if is_visible {
            card::CHECKBOX_ON_PAD
        } else {
            card::CHECKBOX_OFF_PAD
        };
        let focus_indicator = if is_focused {
            card::COLLAPSE_CLOSED
        } else {
            card::SPACER
        };
        // Reserve space for spinner (2 chars) and/or archived tag (10 chars)
        let name_max = match (is_busy, is_archived) {
            (true, true) => 8,    // Both spinner and archived
            (true, false) => 18,  // Just spinner
            (false, true) => 10,  // Just archived
            (false, false) => 20, // Neither
        };
        let name = truncate_with_ellipsis(&project.name, name_max);

        let checkbox_style = if is_focused {
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::ACCENT_PRIMARY)
        };

        let name_style = if is_focused {
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        let mut spans = vec![
            Span::styled(focus_indicator, Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled(checkbox, checkbox_style),
            Span::styled(name, name_style),
        ];

        // Add archived tag if project is archived
        if is_archived {
            spans.push(Span::styled(
                " [archived]",
                Style::default()
                    .fg(theme::TEXT_MUTED)
                    .add_modifier(Modifier::DIM),
            ));
        }

        // Add spinner if project is busy
        if is_busy {
            spans.push(Span::styled(
                format!(" {}", app.spinner_char()),
                Style::default().fg(theme::ACCENT_PRIMARY),
            ));
        }

        let item = ListItem::new(Line::from(spans));

        let item = if is_focused {
            item.style(Style::default().bg(theme::BG_SELECTED))
        } else {
            item
        };

        items.push(item);
    }

    // Offline section header
    if !offline_projects.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled(card::HOLLOW_BULLET, Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Offline", Style::default().fg(theme::TEXT_MUTED)),
        ])));
    }

    // Offline projects - now empty = none (inverted)
    let online_count = online_projects.len();
    for (i, project) in offline_projects.iter().enumerate() {
        let a_tag = project.a_tag();
        let is_visible = app.visible_projects.contains(&a_tag);
        let is_focused = selected_project_index == Some(online_count + i);
        let is_busy = app.data_store.borrow().operations.is_project_busy(&a_tag);
        let is_archived = app.is_project_archived(&a_tag);

        let checkbox = if is_visible {
            card::CHECKBOX_ON_PAD
        } else {
            card::CHECKBOX_OFF_PAD
        };
        let focus_indicator = if is_focused {
            card::COLLAPSE_CLOSED
        } else {
            card::SPACER
        };
        // Reserve space for spinner (2 chars) and/or archived tag (10 chars)
        let name_max = match (is_busy, is_archived) {
            (true, true) => 8,    // Both spinner and archived
            (true, false) => 18,  // Just spinner
            (false, true) => 10,  // Just archived
            (false, false) => 20, // Neither
        };
        let name = truncate_with_ellipsis(&project.name, name_max);

        let checkbox_style = if is_focused {
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        let name_style = if is_focused {
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        let mut spans = vec![
            Span::styled(focus_indicator, Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled(checkbox, checkbox_style),
            Span::styled(name, name_style),
        ];

        // Add archived tag if project is archived
        if is_archived {
            spans.push(Span::styled(
                " [archived]",
                Style::default()
                    .fg(theme::TEXT_MUTED)
                    .add_modifier(Modifier::DIM),
            ));
        }

        // Add spinner if project is busy (unlikely for offline, but for consistency)
        if is_busy {
            spans.push(Span::styled(
                format!(" {}", app.spinner_char()),
                Style::default().fg(theme::ACCENT_PRIMARY),
            ));
        }

        let item = ListItem::new(Line::from(spans));

        let item = if is_focused {
            item.style(Style::default().bg(theme::BG_SELECTED))
        } else {
            item
        };

        items.push(item);
    }

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::NONE)
                .padding(Padding::new(2, 2, 1, 0)),
        ) // Reduced left padding to fit indicator
        .style(Style::default().bg(theme::BG_SIDEBAR));

    f.render_widget(list, area);
}

pub(super) fn render_bottom_padding(f: &mut Frame, area: Rect) {
    let padding = Paragraph::new("").style(Style::default().bg(theme::BG_APP));
    f.render_widget(padding, area);
}

/// Get the actual project at the given selection index
/// Returns (project, is_online)
pub fn get_project_at_index(app: &App, index: usize) -> Option<(crate::models::Project, bool)> {
    let (online_projects, offline_projects) = app.filtered_projects();

    if index < online_projects.len() {
        online_projects.get(index).map(|p| (p.clone(), true))
    } else {
        let offline_index = index - online_projects.len();
        offline_projects
            .get(offline_index)
            .map(|p| (p.clone(), false))
    }
}

/// Get the total count of selectable projects
pub fn selectable_project_count(app: &App) -> usize {
    let (online, offline) = app.filtered_projects();
    online.len() + offline.len()
}
