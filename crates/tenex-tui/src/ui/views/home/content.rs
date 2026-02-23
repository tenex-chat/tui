use crate::models::{InboxEventType, InboxItem, Thread};
use crate::ui::card;
use crate::ui::format::{format_relative_time, format_relative_time_short, truncate_with_ellipsis};
use crate::ui::views::home_helpers::build_thread_hierarchy;
use crate::ui::views::home_helpers::HierarchicalThread;
use crate::ui::{theme, App, HomeTab};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Cell, List, ListItem, ListState, Paragraph, Row, Table},
    Frame,
};
use unicode_width::UnicodeWidthStr;

/// Simple word-wrapping: split text into lines of at most `max_width` chars,
/// breaking on spaces when possible.
fn wrap_summary(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![];
    }
    let mut lines = Vec::new();
    let mut current_line = String::new();
    for word in text.split_whitespace() {
        if current_line.is_empty() {
            if word.len() > max_width {
                let mut remaining = word;
                while remaining.len() > max_width {
                    let (chunk, rest) = remaining.split_at(max_width);
                    lines.push(chunk.to_string());
                    remaining = rest;
                }
                current_line = remaining.to_string();
            } else {
                current_line = word.to_string();
            }
        } else if current_line.len() + 1 + word.len() <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            lines.push(current_line);
            current_line = word.to_string();
        }
    }
    if !current_line.is_empty() {
        lines.push(current_line);
    }
    lines
}

pub(super) fn render_conversations_with_feed(f: &mut Frame, app: &App, area: Rect) {
    render_conversations_cards(f, app, area, true);
}

fn render_conversations_cards(f: &mut Frame, app: &App, area: Rect, is_focused: bool) {
    let recent = app.recent_threads();

    if recent.is_empty() {
        let empty =
            Paragraph::new("No recent conversations").style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty, area);
        return;
    }

    // Get q-tag relationships for fallback parent-child detection
    let q_tag_relationships = app.data_store.borrow().get_q_tag_relationships();

    // Build hierarchical thread list (with default collapsed state from preferences)
    let default_collapsed = app.threads_default_collapsed();
    let hierarchy = build_thread_hierarchy(
        &recent,
        &app.collapsed_threads,
        &q_tag_relationships,
        default_collapsed,
    );

    // Helper to calculate card height
    // Full mode (depth=0):
    //   1 line (title) + summary lines (1-4) + 1 blank trailing = dynamic
    // Compact mode (depth>0): always 1 line
    let calc_card_height = |item: &HierarchicalThread,
                            _is_selected: bool,
                            _is_multi_selected: bool,
                            _next_is_selected: bool|
     -> u16 {
        if item.depth > 0 {
            return 1;
        }
        // Full mode: title(1) + summary lines (1-4) + trailing blank(1)
        // Height is constant regardless of selection to prevent layout jumping
        let summary_lines = if let Some(ref s) = item.thread.summary {
            let avail_width = area.width as usize;
            let wrapped = wrap_summary(s, avail_width.saturating_sub(8));
            (wrapped.len() as u16).min(4).max(1)
        } else {
            0
        };
        1 + summary_lines + 1
    };

    // Calculate scroll offset to keep selected item visible
    let selected_idx = app.current_selection();
    let mut scroll_offset: u16 = 0;

    // Calculate cumulative height up to and including selected item
    let mut height_before_selected: u16 = 0;
    let mut selected_height: u16 = 0;
    for (i, item) in hierarchy.iter().enumerate() {
        let item_is_selected = is_focused && i == selected_idx;
        let item_is_multi_selected = app.is_thread_multi_selected(&item.thread.id);
        let next_is_selected = is_focused && i + 1 == selected_idx;
        let h = calc_card_height(
            item,
            item_is_selected,
            item_is_multi_selected,
            next_is_selected,
        );
        if i < selected_idx {
            height_before_selected += h;
        } else if i == selected_idx {
            selected_height = h;
            break;
        }
    }

    // If selected item would be below visible area, scroll down
    let selected_bottom = height_before_selected + selected_height;
    if selected_bottom > area.height {
        scroll_offset = selected_bottom.saturating_sub(area.height);
    }

    // Render cards with scroll offset
    let mut y_offset: i32 = -(scroll_offset as i32);

    // Track parent a_tag for each depth level to suppress duplicate project names
    let mut parent_a_tag_stack: Vec<String> = Vec::new();

    for (i, item) in hierarchy.iter().enumerate() {
        let is_selected = is_focused && i == selected_idx;
        let is_multi_selected = app.is_thread_multi_selected(&item.thread.id);
        let next_is_selected = is_focused && i + 1 == selected_idx;
        let h = calc_card_height(item, is_selected, is_multi_selected, next_is_selected);

        // Maintain parent a_tag stack based on depth
        parent_a_tag_stack.truncate(item.depth);
        let parent_a_tag = parent_a_tag_stack.last().map(|s| s.as_str());

        // Skip items completely above visible area
        if y_offset + (h as i32) <= 0 {
            parent_a_tag_stack.push(item.a_tag.clone());
            y_offset += h as i32;
            continue;
        }

        // Stop if we're past the visible area
        if y_offset >= area.height as i32 {
            break;
        }

        // Calculate visible portion of card
        let render_y = y_offset.max(0) as u16;
        let visible_height = (h as i32 - (-y_offset).max(0))
            .min((area.height as i32) - render_y as i32)
            .max(0) as u16;

        if visible_height > 0 {
            let content_area = Rect::new(area.x, area.y + render_y, area.width, visible_height);
            let is_archived = app.is_thread_archived(&item.thread.id);

            render_card_content(
                f,
                app,
                &item.thread,
                &item.a_tag,
                parent_a_tag,
                is_selected,
                is_multi_selected,
                next_is_selected,
                item.depth,
                item.has_children,
                item.child_count,
                item.is_collapsed,
                is_archived,
                content_area,
            );
        }

        parent_a_tag_stack.push(item.a_tag.clone());
        y_offset += h as i32;
    }
}

/// Get the hierarchical thread list (used for navigation and selection)
pub fn get_hierarchical_threads(app: &App) -> Vec<HierarchicalThread> {
    let recent = app.recent_threads();
    let q_tag_relationships = app.data_store.borrow().get_q_tag_relationships();
    let default_collapsed = app.threads_default_collapsed();
    build_thread_hierarchy(
        &recent,
        &app.collapsed_threads,
        &q_tag_relationships,
        default_collapsed,
    )
}

// Fixed column widths for right-side alignment (all rows share these)
const RIGHT_TIME_W: usize = 5; // "4m", "12m", "5h"
const RIGHT_RECIP_W: usize = 20; // "@Architect-Orche..."
const RIGHT_PROJECT_W: usize = 15; // "TENEX Backend"
const RIGHT_COL_GAP: usize = 2;
const RIGHT_TOTAL_W: usize =
    RIGHT_COL_GAP + RIGHT_PROJECT_W + RIGHT_COL_GAP + RIGHT_RECIP_W + RIGHT_COL_GAP + RIGHT_TIME_W;

/// Render card content — "Minimal Clean" style.
///
/// Full mode (depth=0):
///   LINE 1: [collapse?] [title] [spinner?]  [ProjectName]  [@recipient]  [short time]
///   LINES 2+: [dot?] [summary text, word-wrapped, up to 4 lines]
///   + 1 blank trailing
///
/// Compact mode (depth>0):
///   LINE 1: [4-space indent per depth] [title]  [project_blank]  [@agent]  [short time]
///   All right columns are fixed-width for alignment.
fn render_card_content(
    f: &mut Frame,
    app: &App,
    thread: &Thread,
    a_tag: &str,
    parent_a_tag: Option<&str>,
    is_selected: bool,
    is_multi_selected: bool,
    _next_is_selected: bool,
    depth: usize,
    has_children: bool,
    child_count: usize,
    is_collapsed: bool,
    is_archived: bool,
    area: Rect,
) {
    let is_compact = depth > 0;

    // Check if this thread has an unsent draft
    let has_draft = app.has_draft_for_thread(&thread.id);

    // Extract data
    let (project_name, is_busy, first_recipient) = {
        let store = app.data_store.borrow();
        let project_name = store.get_project_name(a_tag);
        let is_busy = store.operations.is_event_busy(&thread.id);
        let first_recipient: Option<(String, String)> = thread
            .p_tags
            .first()
            .map(|pk| (store.get_profile_name(pk), pk.clone()));
        (project_name, is_busy, first_recipient)
    };

    let spinner_char = if is_busy {
        Some(app.spinner_char())
    } else {
        None
    };

    // Short time string — when collapsed with children, use effective_last_activity
    let display_timestamp = if is_collapsed && has_children {
        thread.effective_last_activity
    } else {
        thread.last_activity
    };
    let time_str = format_relative_time_short(display_timestamp);

    let mut lines: Vec<Line> = Vec::new();

    if is_compact {
        // COMPACT MODE: 1 line, no tree connectors, fixed right columns
        let indent = "    ".repeat(depth);

        let title_color = ratatui::style::Color::Rgb(85, 85, 85);
        let agent_color = ratatui::style::Color::Rgb(56, 56, 56);
        let time_color = ratatui::style::Color::Rgb(42, 42, 42);

        let title_style = if is_selected || is_multi_selected {
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(title_color)
        };

        // Fixed right columns: [project?]  [@recipient]  [time]
        // Show project name only when this delegation crosses into a different project.
        let cross_project = parent_a_tag.map_or(true, |p| p != a_tag);
        let project_text = if cross_project {
            truncate_with_ellipsis(&project_name, RIGHT_PROJECT_W)
        } else {
            String::new()
        };

        let recip_text = if let Some((name, _)) = first_recipient.as_ref() {
            format!("@{}", truncate_with_ellipsis(name, RIGHT_RECIP_W - 1))
        } else {
            String::new()
        };
        let time_text = truncate_with_ellipsis(&time_str, RIGHT_TIME_W);

        let project_col = format!("{:<w$}", project_text, w = RIGHT_PROJECT_W);
        let recip_col = format!("{:<w$}", recip_text, w = RIGHT_RECIP_W);
        let time_col = format!("{:>w$}", time_text, w = RIGHT_TIME_W);

        let nested_suffix = if is_collapsed && child_count > 0 {
            format!(" +{}", child_count)
        } else {
            String::new()
        };

        let total_width = area.width as usize;
        let indent_len = indent.len();
        let indicator_width = if thread.is_scheduled {
            " ⏰".width()
        } else {
            0
        } + if is_archived { " [arc]".width() } else { 0 }
            + if has_draft { " ✎".width() } else { 0 }
            + if spinner_char.is_some() { 2 } else { 0 };
        let title_max = total_width
            .saturating_sub(RIGHT_TOTAL_W + indent_len + nested_suffix.width() + indicator_width);
        let title_truncated = truncate_with_ellipsis(&thread.title, title_max);
        let title_len = title_truncated.width();
        let filler = total_width.saturating_sub(
            indent_len + title_len + nested_suffix.width() + RIGHT_TOTAL_W + indicator_width,
        );

        let mut line1 = Vec::new();
        if !indent.is_empty() {
            line1.push(Span::styled(indent, Style::default()));
        }
        line1.push(Span::styled(title_truncated, title_style));
        if thread.is_scheduled {
            line1.push(Span::styled(" ⏰", Style::default().fg(agent_color)));
        }
        if is_archived {
            line1.push(Span::styled(
                " [arc]",
                Style::default().fg(agent_color).add_modifier(Modifier::DIM),
            ));
        }
        if has_draft {
            line1.push(Span::styled(
                " ✎",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
        }
        if let Some(c) = spinner_char {
            line1.push(Span::styled(
                format!(" {}", c),
                Style::default().fg(theme::ACCENT_PRIMARY),
            ));
        }
        if !nested_suffix.is_empty() {
            line1.push(Span::styled(
                nested_suffix,
                Style::default().fg(agent_color),
            ));
        }
        line1.push(Span::styled(" ".repeat(filler), Style::default()));
        line1.push(Span::styled("  ", Style::default()));
        line1.push(Span::styled(
            project_col,
            Style::default().fg(theme::project_color(a_tag)),
        ));
        line1.push(Span::styled("  ", Style::default()));
        line1.push(Span::styled(recip_col, Style::default().fg(agent_color)));
        line1.push(Span::styled("  ", Style::default()));
        line1.push(Span::styled(time_col, Style::default().fg(time_color)));
        lines.push(Line::from(line1));
    } else {
        // FULL MODE (depth=0)
        let total_width = area.width as usize;

        let title_style = if is_selected || is_multi_selected {
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        // Collapse indicator (only for items with children)
        let collapse_indicator = if has_children {
            if is_collapsed {
                card::COLLAPSE_CLOSED
            } else {
                card::COLLAPSE_OPEN
            }
        } else {
            ""
        };
        let collapse_col_width = card::COLLAPSE_CLOSED.width();
        let collapse_len = collapse_indicator.width();
        let collapse_padding = collapse_col_width.saturating_sub(collapse_len);

        // Spinner suffix
        let spinner_suffix = spinner_char.map(|c| format!(" {}", c)).unwrap_or_default();

        // Fixed right columns: [Project]  [@Recipient]  [time]
        let project_text = truncate_with_ellipsis(&project_name, RIGHT_PROJECT_W);
        let recip_text = if let Some((name, _)) = first_recipient.as_ref() {
            format!("@{}", truncate_with_ellipsis(name, RIGHT_RECIP_W - 1))
        } else {
            String::new()
        };
        let time_text = truncate_with_ellipsis(&time_str, RIGHT_TIME_W);

        let project_col = format!("{:<w$}", project_text, w = RIGHT_PROJECT_W);
        let recip_col = format!("{:<w$}", recip_text, w = RIGHT_RECIP_W);
        let time_col = format!("{:>w$}", time_text, w = RIGHT_TIME_W);

        // Title area = total - collapse - right columns
        let main_col_width = total_width.saturating_sub(RIGHT_TOTAL_W + collapse_col_width);
        let title_max = main_col_width.saturating_sub(spinner_suffix.width());
        let title_truncated = truncate_with_ellipsis(&thread.title, title_max);

        // Build line1: [collapse] [title] [extras] [filler] [project] [recip] [time]
        let mut line1 = Vec::new();
        if !collapse_indicator.is_empty() {
            line1.push(Span::styled(
                collapse_indicator,
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
        if collapse_padding > 0 {
            line1.push(Span::styled(" ".repeat(collapse_padding), Style::default()));
        }
        line1.push(Span::styled(title_truncated.clone(), title_style));

        // Track extra chars added after title for filler calculation
        let mut extras_width = 0usize;
        if thread.is_scheduled {
            let s = " SCHED";
            extras_width += s.width();
            line1.push(Span::styled(s, Style::default().fg(theme::TEXT_MUTED)));
        }
        if is_archived {
            let s = " [arc]";
            extras_width += s.width();
            line1.push(Span::styled(
                s,
                Style::default()
                    .fg(theme::TEXT_MUTED)
                    .add_modifier(Modifier::DIM),
            ));
        }
        if has_draft {
            let s = " ✎";
            extras_width += s.width();
            line1.push(Span::styled(s, Style::default().fg(theme::ACCENT_WARNING)));
        }
        if !spinner_suffix.is_empty() {
            extras_width += spinner_suffix.width();
            line1.push(Span::styled(
                spinner_suffix,
                Style::default().fg(theme::ACCENT_PRIMARY),
            ));
        }

        let used = title_truncated.width() + extras_width;
        let filler = main_col_width.saturating_sub(used);
        line1.push(Span::styled(" ".repeat(filler), Style::default()));

        // Fixed-width right columns
        line1.push(Span::styled("  ", Style::default()));
        line1.push(Span::styled(
            project_col,
            Style::default().fg(theme::project_color(a_tag)),
        ));
        line1.push(Span::styled("  ", Style::default()));
        line1.push(Span::styled(
            recip_col,
            Style::default().fg(theme::TEXT_DIM),
        ));
        line1.push(Span::styled("  ", Style::default()));
        line1.push(Span::styled(
            time_col,
            Style::default().fg(theme::TEXT_MUTED),
        ));
        lines.push(Line::from(line1));

        // SUMMARY: word-wrapped, up to 4 lines
        let status_dot: Option<Style> = thread.status_label.as_ref().map(|s| {
            let color = match s.to_lowercase().as_str() {
                "done" | "complete" | "completed" | "finished" => theme::ACCENT_SUCCESS,
                "in progress" | "in-progress" | "working" | "active" => theme::ACCENT_WARNING,
                "blocked" | "waiting" | "paused" => theme::ACCENT_ERROR,
                "reviewing" | "review" | "in review" => theme::ACCENT_SPECIAL,
                _ => theme::TEXT_MUTED,
            };
            Style::default().fg(color)
        });

        let dot_width = if status_dot.is_some() { 2 } else { 0 };
        let summary_indent = collapse_col_width + dot_width;
        let summary_max_width = total_width.saturating_sub(summary_indent + 2);

        if let Some(ref summary) = thread.summary {
            let wrapped = wrap_summary(summary, summary_max_width);
            for (line_idx, line_text) in wrapped.iter().take(4).enumerate() {
                let mut summary_line = Vec::new();
                if line_idx == 0 {
                    summary_line.push(Span::styled(
                        " ".repeat(collapse_col_width),
                        Style::default(),
                    ));
                    if let Some(dot_style) = status_dot {
                        summary_line.push(Span::styled("● ", dot_style));
                    }
                } else {
                    summary_line.push(Span::styled(" ".repeat(summary_indent), Style::default()));
                }
                summary_line.push(Span::styled(
                    line_text.clone(),
                    Style::default().fg(theme::TEXT_MUTED),
                ));
                lines.push(Line::from(summary_line));
            }
        }

        // Trailing blank line (always, so height is constant)
        lines.push(Line::from(""));
    }

    // Rendering: just background color for selection, no half-block borders
    if is_selected || is_multi_selected {
        let content = Paragraph::new(lines).style(Style::default().bg(theme::BG_SELECTED));
        f.render_widget(content, area);
    } else {
        let paragraph = Paragraph::new(lines);
        f.render_widget(paragraph, area);
    }
}

pub(super) fn render_inbox_cards(f: &mut Frame, app: &App, area: Rect) {
    let inbox_items = app.inbox_items();

    if inbox_items.is_empty() {
        let empty_lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "No notifications",
                Style::default().fg(theme::TEXT_MUTED),
            )),
        ];
        let empty = Paragraph::new(empty_lines).alignment(ratatui::layout::Alignment::Center);
        f.render_widget(empty, area);
        return;
    }

    let selected_idx = app.current_selection();
    let items: Vec<ListItem> = inbox_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == selected_idx;
            let is_multi_selected = item
                .thread_id
                .as_ref()
                .map(|id| app.is_thread_multi_selected(id))
                .unwrap_or(false);
            render_inbox_card(app, item, is_selected, is_multi_selected)
        })
        .collect();

    // No block/border - just the list directly
    let list = List::new(items).highlight_style(Style::default());

    let mut state = ListState::default();
    state.select(Some(selected_idx));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_inbox_card(
    app: &App,
    item: &InboxItem,
    is_selected: bool,
    is_multi_selected: bool,
) -> ListItem<'static> {
    // Single borrow to extract all needed data
    let (project_name, author_name) = {
        let store = app.data_store.borrow();
        (
            store.get_project_name(&item.project_a_tag),
            store.get_profile_name(&item.author_pubkey),
        )
    };
    let time_str = format_relative_time(item.created_at);

    // Check if this is a "Waiting For You" item (Ask or Mention type = user was p-tagged)
    let is_waiting_for_user = matches!(
        item.event_type,
        InboxEventType::Ask | InboxEventType::Mention
    ) && !item.is_read;

    let type_str = match item.event_type {
        InboxEventType::Ask => "? Asked You",
        InboxEventType::Mention => "@ mentioned you",
    };

    let title_style = if is_selected {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else if is_waiting_for_user {
        // Waiting for user items get yellow/warning style
        Style::default()
            .fg(theme::ACCENT_WARNING)
            .add_modifier(Modifier::BOLD)
    } else if !item.is_read {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    // Indicator: @ for waiting items, • for unread, space otherwise
    let indicator = if is_waiting_for_user {
        Span::styled("@ ", Style::default().fg(theme::ACCENT_WARNING))
    } else if !item.is_read {
        Span::styled(card::BULLET, Style::default().fg(theme::ACCENT_PRIMARY))
    } else {
        Span::styled(card::SPACER, Style::default())
    };

    // Line 1: Title + time
    let line1_spans = vec![
        indicator,
        Span::styled(truncate_with_ellipsis(&item.title, 55), title_style),
        Span::styled(card::SPACER, Style::default()),
        Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)),
    ];

    // Line 2: Type + Project + Author (yellow for waiting items, muted otherwise)
    let type_style = if is_waiting_for_user {
        Style::default().fg(theme::ACCENT_WARNING)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let line2_spans = vec![
        Span::styled(type_str, type_style),
        Span::styled(" in ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            project_name,
            Style::default().fg(theme::project_color(&item.project_a_tag)),
        ),
        Span::styled(" by ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(author_name, Style::default().fg(theme::ACCENT_SPECIAL)),
    ];

    // Line 3: Spacing line
    let line3_spans = vec![Span::styled(card::SPACER, Style::default())];

    let list_item = ListItem::new(vec![
        Line::from(line1_spans),
        Line::from(line2_spans),
        Line::from(line3_spans),
    ]);

    if is_multi_selected {
        list_item.style(Style::default().bg(theme::BG_SELECTED))
    } else {
        list_item
    }
}

/// Render the reports list with search
pub(super) fn render_reports_list(f: &mut Frame, app: &App, area: Rect) {
    let reports = app.reports();

    // Layout: Search bar + List
    let chunks = Layout::vertical([
        Constraint::Length(2), // Search bar
        Constraint::Min(0),    // List
    ])
    .split(area);

    // Render search bar
    let search_style = if !app.report_search_filter.is_empty() {
        Style::default().fg(theme::TEXT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let search_text = if app.report_search_filter.is_empty() {
        "/ Search reports...".to_string()
    } else {
        format!("/ {}", app.report_search_filter)
    };

    let search_line = Paragraph::new(search_text).style(search_style);
    f.render_widget(search_line, chunks[0]);

    // Empty state
    if reports.is_empty() {
        let msg = if app.report_search_filter.is_empty() {
            "No reports found"
        } else {
            "No matching reports"
        };
        let empty = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty, chunks[1]);
        return;
    }

    // Render report cards
    let mut y_offset = 0u16;
    let selected_idx = app.current_selection();
    for (i, report) in reports.iter().enumerate() {
        let is_selected = i == selected_idx;
        let card_height = 3u16; // title, summary, spacing

        if y_offset + card_height > chunks[1].height {
            break;
        }

        let card_area = Rect::new(
            chunks[1].x,
            chunks[1].y + y_offset,
            chunks[1].width,
            card_height,
        );

        render_report_card(f, app, report, is_selected, card_area);
        y_offset += card_height;
    }
}

/// Render a single report card
fn render_report_card(
    f: &mut Frame,
    app: &App,
    report: &tenex_core::models::Report,
    is_selected: bool,
    area: Rect,
) {
    let store = app.data_store.borrow();
    let project_name = store.get_project_name(&report.project_a_tag);
    let author_name = store.get_profile_name(&report.author);
    drop(store);

    let time_str = crate::ui::format::format_relative_time(report.created_at);
    let reading_time = format!("{}m", report.reading_time_mins);

    let title_style = if is_selected {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    let bullet = if is_selected {
        card::BULLET
    } else {
        card::SPACER
    };

    // Line 1: Title + project + reading time + timestamp
    let title_max = area.width as usize - 30;
    let title = crate::ui::format::truncate_with_ellipsis(&report.title, title_max);

    let line1 = Line::from(vec![
        Span::styled(bullet, Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(title, title_style),
        Span::styled("  ", Style::default()),
        Span::styled(
            &project_name,
            Style::default().fg(theme::project_color(&report.project_a_tag)),
        ),
        Span::styled(
            format!("  {} · {}", reading_time, time_str),
            Style::default().fg(theme::TEXT_MUTED),
        ),
    ]);

    // Line 2: Summary + hashtags + author
    let summary_max = area.width as usize - 40;
    let summary = crate::ui::format::truncate_with_ellipsis(&report.summary, summary_max);
    let hashtags: String = report
        .hashtags
        .iter()
        .take(3)
        .map(|h| format!("#{} ", h))
        .collect();

    let line2 = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(summary, Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("  {}", hashtags.trim()),
            Style::default().fg(theme::ACCENT_WARNING),
        ),
        Span::styled(
            format!("  @{}", author_name),
            Style::default().fg(theme::ACCENT_SPECIAL),
        ),
    ]);

    // Line 3: Spacing
    let line3 = Line::from("");

    let content = Paragraph::new(vec![line1, line2, line3]);

    if is_selected {
        f.render_widget(content.style(Style::default().bg(theme::BG_SELECTED)), area);
    } else {
        f.render_widget(content, area);
    }
}

/// Render the Active Work tab showing currently active operations
pub(super) fn render_active_work(f: &mut Frame, app: &App, area: Rect) {
    let data_store = app.data_store.borrow();
    let operations = data_store.operations.get_all_active_operations();

    if operations.is_empty() {
        let empty_lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "No active work",
                Style::default().fg(theme::TEXT_MUTED),
            )),
            Line::from(Span::styled(
                "Operations will appear here when agents are working",
                Style::default().fg(theme::TEXT_MUTED),
            )),
        ];
        let empty = Paragraph::new(empty_lines).alignment(ratatui::layout::Alignment::Center);
        f.render_widget(empty, area);
        return;
    }

    let selected_idx = app.current_selection();
    let is_focused = app.home_panel_focus == HomeTab::ActiveWork && !app.sidebar_focused;

    // Build rows for the table with selection highlighting
    let mut rows: Vec<Row> = Vec::new();

    for (i, op) in operations.iter().enumerate() {
        let is_selected = is_focused && i == selected_idx;

        // Get agent names (comma-separated if multiple)
        let agent_names: Vec<String> = op
            .agent_pubkeys
            .iter()
            .map(|pk| data_store.get_profile_name(pk))
            .collect();
        let agent_str = if agent_names.is_empty() {
            "Unknown".to_string()
        } else {
            agent_names.join(", ")
        };

        // Get conversation title from pre-computed thread_id, or look up from event_id
        let conv_title = if let Some(ref thread_id) = op.thread_id {
            data_store
                .get_thread_by_id(thread_id)
                .map(|t| t.title.clone())
                .unwrap_or_else(|| truncate_with_ellipsis(&op.event_id, 12))
        } else {
            data_store
                .get_thread_info_for_event(&op.event_id)
                .map(|(_thread_id, title)| title)
                .unwrap_or_else(|| truncate_with_ellipsis(&op.event_id, 12))
        };

        // Get project name
        let project_name = data_store.get_project_name(&op.project_coordinate);

        // Calculate duration
        let duration = crate::ui::format::format_duration_since(op.created_at);

        // Set row style based on selection (use BG_SELECTED for consistency with other tabs)
        let row_style = if is_selected {
            Style::default().bg(theme::BG_SELECTED)
        } else {
            Style::default()
        };

        // Set text styles based on selection
        let agent_style = if is_selected {
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        let conv_style = if is_selected {
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        let project_style = if is_selected {
            Style::default()
                .fg(theme::ACCENT_SUCCESS)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::ACCENT_SUCCESS)
        };

        let duration_style = if is_selected {
            Style::default().fg(theme::TEXT_PRIMARY)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        // Add selection indicator
        let bullet = if is_selected {
            card::BULLET
        } else {
            card::SPACER
        };
        let agent_display = format!("{} {}", bullet, truncate_with_ellipsis(&agent_str, 18));

        rows.push(
            Row::new(vec![
                Cell::from(agent_display).style(agent_style),
                Cell::from(truncate_with_ellipsis(&conv_title, 30)).style(conv_style),
                Cell::from(truncate_with_ellipsis(&project_name, 20)).style(project_style),
                Cell::from(duration).style(duration_style),
            ])
            .style(row_style),
        );
    }

    drop(data_store);

    // Create header
    let header = Row::new(vec![
        Cell::from("Agent").style(
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Conversation").style(
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Project").style(
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Duration").style(
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .style(Style::default().bg(theme::BG_SECONDARY))
    .height(1);

    let widths = [
        Constraint::Length(22), // Agent
        Constraint::Min(20),    // Conversation (flexible)
        Constraint::Length(22), // Project
        Constraint::Length(12), // Duration
    ];

    let table = Table::new(rows, widths).header(header).column_spacing(2);

    f.render_widget(table, area);
}
