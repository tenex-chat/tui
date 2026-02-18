use crate::ui::card;
use crate::ui::state::TabContentType;
use crate::ui::{theme, App, View};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use unicode_width::UnicodeWidthStr;

/// Half-block characters for vertical padding
const LOWER_HALF_BLOCK: char = '▄';
const UPPER_HALF_BLOCK: char = '▀';

/// Truncate string to fit within a display width, adding ellipsis when truncated.
/// Returns (truncated_string, actual_display_width).
fn truncate_to_width(s: &str, max_width: usize) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }

    let current_width = s.width();
    if current_width <= max_width {
        return (s.to_string(), current_width);
    }

    if max_width <= 3 {
        let dots = ".".repeat(max_width);
        return (dots.clone(), dots.width());
    }

    // Build truncated string char by char, tracking display width
    let mut result = String::new();
    let mut width = 0;
    let target_width = max_width - 3; // Reserve space for "..."

    for c in s.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width + char_width > target_width {
            break;
        }
        result.push(c);
        width += char_width;
    }

    result.push_str("...");
    (result.clone(), result.width())
}

/// Truncate string to fit within a display width without ellipsis.
/// Returns (truncated_string, actual_display_width).
fn truncate_plain_to_width(s: &str, max_width: usize) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }

    let current_width = s.width();
    if current_width <= max_width {
        return (s.to_string(), current_width);
    }

    let mut result = String::new();
    let mut width = 0;

    for c in s.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width + char_width > max_width {
            break;
        }
        result.push(c);
        width += char_width;
    }

    (result.clone(), result.width())
}

/// Create a half-block span that matches the display width of the given text
fn half_block_span(text: &str, block_char: char, is_active: bool) -> Span<'static> {
    let width = text.width();
    let content: String = std::iter::repeat_n(block_char, width).collect();
    if is_active {
        Span::styled(content, Style::default().fg(theme::BG_TAB_ACTIVE))
    } else {
        // Explicitly set background to clear any previous content
        Span::styled(" ".repeat(width), Style::default().bg(theme::BG_APP))
    }
}

/// Renders a four-line tab bar with half-character vertical padding:
/// - Line 0: Top padding (lower half blocks for active tabs)
/// - Line 1: Tab numbers and titles
/// - Line 2: Project names aligned under each tab
/// - Line 3: Bottom padding (upper half blocks for active tabs)
pub fn render_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    // Split area into four lines
    let lines = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(area);

    let data_store = app.data_store.borrow();

    // Build all four lines simultaneously
    let mut top_spans: Vec<Span> = Vec::new();
    let mut title_spans: Vec<Span> = Vec::new();
    let mut project_spans: Vec<Span> = Vec::new();
    let mut bottom_spans: Vec<Span> = Vec::new();

    // Add leading padding (1 space, not part of any tab)
    top_spans.push(Span::raw(" "));
    title_spans.push(Span::raw(" "));
    project_spans.push(Span::raw(" "));
    bottom_spans.push(Span::raw(" "));

    // === Home Tab (always first, Option+1) ===
    let home_active = app.view == View::Home;

    // Home tab content strings
    let home_left_pad = " ";
    let home_shortcut = "⌥1 ";
    let home_indicator = " ";
    let home_title = "Home";
    let home_right_pad = " ";

    // Styles for home tab
    let home_bg = if home_active {
        Some(theme::BG_TAB_ACTIVE)
    } else {
        None
    };

    let home_shortcut_style = if home_active {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .bg(theme::BG_TAB_ACTIVE)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let home_indicator_style = Style::default().bg(home_bg.unwrap_or(theme::BG_APP));

    let home_title_style = if home_active {
        Style::default()
            .fg(theme::ACCENT_SUCCESS)
            .bg(theme::BG_TAB_ACTIVE)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        theme::tab_inactive()
    };

    let home_pad_style = Style::default().bg(home_bg.unwrap_or(theme::BG_APP));

    // Top padding line for Home
    top_spans.push(half_block_span(
        home_left_pad,
        LOWER_HALF_BLOCK,
        home_active,
    ));
    top_spans.push(half_block_span(
        home_shortcut,
        LOWER_HALF_BLOCK,
        home_active,
    ));
    top_spans.push(half_block_span(
        home_indicator,
        LOWER_HALF_BLOCK,
        home_active,
    ));
    top_spans.push(half_block_span(home_title, LOWER_HALF_BLOCK, home_active));
    top_spans.push(half_block_span(
        home_right_pad,
        LOWER_HALF_BLOCK,
        home_active,
    ));

    // Title line for Home
    title_spans.push(Span::styled(home_left_pad, home_pad_style));
    title_spans.push(Span::styled(home_shortcut, home_shortcut_style));
    title_spans.push(Span::styled(home_indicator, home_indicator_style));
    title_spans.push(Span::styled(home_title, home_title_style));
    title_spans.push(Span::styled(home_right_pad, home_pad_style));

    // Project line for Home (empty space with same widths)
    let home_prefix_width = home_left_pad.width() + home_shortcut.width() + home_indicator.width();
    let home_suffix_width = home_title.width() + home_right_pad.width();
    let home_prefix = " ".repeat(home_prefix_width);
    let home_suffix = " ".repeat(home_suffix_width);
    project_spans.push(Span::styled(home_prefix, home_pad_style));
    project_spans.push(Span::styled(home_suffix, home_pad_style));

    // Bottom padding line for Home
    bottom_spans.push(half_block_span(
        home_left_pad,
        UPPER_HALF_BLOCK,
        home_active,
    ));
    bottom_spans.push(half_block_span(
        home_shortcut,
        UPPER_HALF_BLOCK,
        home_active,
    ));
    bottom_spans.push(half_block_span(
        home_indicator,
        UPPER_HALF_BLOCK,
        home_active,
    ));
    bottom_spans.push(half_block_span(home_title, UPPER_HALF_BLOCK, home_active));
    bottom_spans.push(half_block_span(
        home_right_pad,
        UPPER_HALF_BLOCK,
        home_active,
    ));

    // Spacing after home
    if !app.open_tabs().is_empty() {
        let gap = "  ";
        top_spans.push(Span::raw(gap));
        title_spans.push(Span::raw(gap));
        project_spans.push(Span::raw(gap));
        bottom_spans.push(Span::raw(gap));
    }

    // === All Tabs (Conversations, TTS Control, Reports) ===
    let max_title_width = 18;

    for (i, tab) in app.open_tabs().iter().enumerate() {
        // Only mark tab as active if we're not on Home and this is the active tab index
        let is_active = !home_active && i == app.active_tab_index();
        let tab_num = i + 2; // Start from 2 since 1 is Home

        // Tab content strings
        let left_pad = " ";
        let shortcut = if tab_num <= 9 {
            format!("⌥{} ", tab_num)
        } else {
            "   ".to_string()
        };

        // Determine indicator and title based on content type
        let (indicator, indicator_fg, title, project) = match &tab.content_type {
            TabContentType::Conversation => {
                // Indicator priority (for inactive tabs):
                // 1. @ (waiting_for_user/mention) - red/orange (ACCENT_ERROR)
                // 2. • (agent working) - blue (ACCENT_PRIMARY)
                // 3. • (unread, no mention) - white (TEXT_WHITE)
                // 4. + (draft) - green (ACCENT_SUCCESS)
                // 5. none
                let (ind, ind_fg) = if tab.waiting_for_user && !is_active {
                    // User is mentioned - show @ with red/orange
                    ("@ ".to_string(), Some(theme::ACCENT_ERROR))
                } else if tab.is_agent_working && !is_active {
                    // Agent is working - show blue bullet
                    (card::BULLET.to_string(), Some(theme::ACCENT_PRIMARY))
                } else if tab.has_unread && !is_active {
                    // Unread activity but no mention - show white bullet
                    (card::BULLET.to_string(), Some(theme::TEXT_WHITE))
                } else if tab.is_draft() {
                    ("+".to_string(), Some(theme::ACCENT_SUCCESS))
                } else {
                    (" ".to_string(), None)
                };

                // Title - look up from store for real threads
                let thread_title = if tab.is_draft() {
                    tab.thread_title.clone()
                } else {
                    data_store
                        .get_thread_by_id(&tab.thread_id)
                        .map(|t| t.title.clone())
                        .unwrap_or_else(|| tab.thread_title.clone())
                };
                let (title, title_width) = truncate_to_width(&thread_title, max_title_width);

                // Project name
                let project_name = data_store.get_project_name(&tab.project_a_tag);
                let (proj, _) = truncate_plain_to_width(&project_name, title_width);

                (ind, ind_fg, title, proj)
            }
            TabContentType::TTSControl => {
                // TTS Control tab has special indicator
                let tts_active = tab
                    .tts_state
                    .as_ref()
                    .map(|s| s.is_active())
                    .unwrap_or(false);
                let tts_paused = tab.tts_state.as_ref().map(|s| s.is_paused).unwrap_or(false);

                let (ind, ind_fg) = if tts_paused {
                    ("⏸".to_string(), Some(theme::ACCENT_WARNING))
                } else if tts_active {
                    ("▶".to_string(), Some(theme::ACCENT_SUCCESS))
                } else {
                    ("♪".to_string(), Some(theme::TEXT_MUTED))
                };

                let (title, _) = truncate_to_width("TTS Control", max_title_width);
                let project = "Audio".to_string();

                (ind, ind_fg, title, project)
            }
            TabContentType::Report { a_tag, .. } => {
                // Report tab indicator
                let (ind, ind_fg) = ("📄".to_string(), Some(theme::ACCENT_PRIMARY));

                // Get report title from store using a_tag (handles slug collisions)
                let report_title = data_store
                    .reports
                    .get_report_by_a_tag(a_tag)
                    .map(|r| r.title.clone())
                    .unwrap_or_else(|| tab.thread_title.clone());
                let (title, title_width) = truncate_to_width(&report_title, max_title_width);

                // Get author name for project line
                let author = data_store
                    .reports
                    .get_report_by_a_tag(a_tag)
                    .map(|r| data_store.get_profile_name(&r.author))
                    .unwrap_or_else(|| "Report".to_string());
                let (proj, _) = truncate_plain_to_width(&format!("@{}", author), title_width);

                (ind, ind_fg, title, proj)
            }
        };

        let (_, title_width) = truncate_to_width(&title, max_title_width);

        let right_pad = " ";

        // Styles
        let tab_bg = if is_active {
            Some(theme::BG_TAB_ACTIVE)
        } else {
            None
        };

        let shortcut_style = if is_active {
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .bg(theme::BG_TAB_ACTIVE)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        let indicator_style = {
            let mut style = Style::default();
            if let Some(fg) = indicator_fg {
                style = style.fg(fg);
            }
            if let Some(bg) = tab_bg {
                style = style.bg(bg);
            }
            style
        };

        let title_style = if is_active {
            theme::tab_active()
        } else if tab.waiting_for_user {
            theme::tab_waiting_for_user()
        } else if tab.is_agent_working {
            theme::tab_agent_working()
        } else if tab.has_unread {
            theme::tab_unread()
        } else {
            theme::tab_inactive()
        };

        let project_style = if is_active {
            Style::default()
                .fg(theme::ACCENT_SUCCESS)
                .bg(theme::BG_TAB_ACTIVE)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        let pad_style = Style::default().bg(tab_bg.unwrap_or(theme::BG_APP));

        // Top padding line
        top_spans.push(half_block_span(left_pad, LOWER_HALF_BLOCK, is_active));
        top_spans.push(half_block_span(&shortcut, LOWER_HALF_BLOCK, is_active));
        top_spans.push(half_block_span(&indicator, LOWER_HALF_BLOCK, is_active));
        top_spans.push(half_block_span(&title, LOWER_HALF_BLOCK, is_active));
        top_spans.push(half_block_span(right_pad, LOWER_HALF_BLOCK, is_active));

        // Title line
        title_spans.push(Span::styled(left_pad, pad_style));
        title_spans.push(Span::styled(shortcut.clone(), shortcut_style));
        title_spans.push(Span::styled(indicator.clone(), indicator_style));
        title_spans.push(Span::styled(title.clone(), title_style));
        title_spans.push(Span::styled(right_pad, pad_style));

        // Project line - align project under title
        // The project line must have the same total width as the title line
        // Title line width = left_pad + shortcut + indicator + title + right_pad
        let prefix_width = left_pad.width() + shortcut.width() + indicator.width();
        let prefix = " ".repeat(prefix_width);
        // Project text area should match: title_width + right_pad width
        let project_area_width = title_width + right_pad.width();
        // Use unicode-aware padding (format! uses char count, not display width)
        let project_display_width = project.width();
        let padding_needed = project_area_width.saturating_sub(project_display_width);
        let project_padded = format!("{}{}", project, " ".repeat(padding_needed));
        project_spans.push(Span::styled(prefix, pad_style));
        project_spans.push(Span::styled(project_padded, project_style));

        // Bottom padding line
        bottom_spans.push(half_block_span(left_pad, UPPER_HALF_BLOCK, is_active));
        bottom_spans.push(half_block_span(&shortcut, UPPER_HALF_BLOCK, is_active));
        bottom_spans.push(half_block_span(&indicator, UPPER_HALF_BLOCK, is_active));
        bottom_spans.push(half_block_span(&title, UPPER_HALF_BLOCK, is_active));
        bottom_spans.push(half_block_span(right_pad, UPPER_HALF_BLOCK, is_active));

        // Spacing between tabs
        if i < app.open_tabs().len() - 1 {
            let gap = "  ";
            top_spans.push(Span::raw(gap));
            title_spans.push(Span::raw(gap));
            project_spans.push(Span::raw(gap));
            bottom_spans.push(Span::raw(gap));
        }
    }

    // Render all four lines
    f.render_widget(Paragraph::new(Line::from(top_spans)), lines[0]);
    f.render_widget(Paragraph::new(Line::from(title_spans)), lines[1]);
    f.render_widget(Paragraph::new(Line::from(project_spans)), lines[2]);
    f.render_widget(Paragraph::new(Line::from(bottom_spans)), lines[3]);
}
