use crate::models::{InboxEventType, InboxItem, Thread};
use crate::ui::card;
use crate::ui::format::{format_relative_time, status_label_to_symbol, truncate_with_ellipsis};
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
    // Full mode: title+recipient+project, summary+time, status+runtime (always 3 lines)
    // Compact mode: 2 lines (title+recipient+project, time)
    // Selected/multi-selected items add 2 lines for half-block borders (top + bottom)
    // and drop spacing line (borders provide visual separation)
    // next_is_selected: if true, this card doesn't need spacing (next card's top border provides it)
    let calc_card_height = |item: &HierarchicalThread,
                            is_selected: bool,
                            is_multi_selected: bool,
                            next_is_selected: bool|
     -> u16 {
        let is_compact = item.depth > 0;
        if is_compact {
            // Compact: 2 content lines + optional 2 border lines
            return if is_selected || is_multi_selected {
                4
            } else {
                2
            };
        }
        // Full mode:
        // Line 1: title + recipient + project (always)
        // Line 2: summary + relative time (always)
        // Line 3: status + runtime (always, even if empty for consistent layout)
        let mut lines = 3; // All 3 lines always present for consistent layout
                           // Spacing line only when card is not selected/multi-selected and next card is not selected
        if !is_selected && !is_multi_selected && !next_is_selected {
            lines += 1;
        }
        // Selected/multi-selected cards get 2 extra lines for half-block borders
        if is_selected || is_multi_selected {
            lines + 2
        } else {
            lines
        }
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

    for (i, item) in hierarchy.iter().enumerate() {
        let is_selected = is_focused && i == selected_idx;
        let is_multi_selected = app.is_thread_multi_selected(&item.thread.id);
        let next_is_selected = is_focused && i + 1 == selected_idx;
        let h = calc_card_height(item, is_selected, is_multi_selected, next_is_selected);

        // Skip items completely above visible area
        if y_offset + (h as i32) <= 0 {
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

/// Render card content in table-like format:
/// Full mode (depth=0):
///   Line 1: [title] [#] [recipient]               [project]
///   Line 2: [summary]                             [relative-last-activity]
///   Line 3: [current status]                      [cumulative llm runtime]
/// Compact mode (depth>0):
///   Line 1: [title] [#] [recipient]               [project]
///   Line 2: [empty]                               [time]
fn render_card_content(
    f: &mut Frame,
    app: &App,
    thread: &Thread,
    a_tag: &str,
    is_selected: bool,
    is_multi_selected: bool,
    next_is_selected: bool,
    depth: usize,
    has_children: bool,
    child_count: usize,
    is_collapsed: bool,
    is_archived: bool,
    area: Rect,
) {
    let is_compact = depth > 0;
    let indent = card::INDENT_UNIT.repeat(depth);
    let indent_len = indent.width();

    // Check if this thread has an unsent draft
    let has_draft = app.has_draft_for_thread(&thread.id);

    // Extract data - fetch what's needed for all modes
    let (project_name, is_busy, first_recipient, hierarchical_runtime) = {
        let store = app.data_store.borrow();
        let project_name = store.get_project_name(a_tag);
        let is_busy = store.operations.is_event_busy(&thread.id);
        // Get first recipient only (avoid allocating full Vec when we only need first)
        let first_recipient: Option<(String, String)> = thread
            .p_tags
            .first()
            .map(|pk| (store.get_profile_name(pk), pk.clone()));
        // Get hierarchical runtime (own + all children)
        let runtime = store.get_hierarchical_runtime(&thread.id);
        (project_name, is_busy, first_recipient, runtime)
    };

    // Spinner for busy threads (uses frame counter from App)
    let spinner_char = if is_busy {
        Some(app.spinner_char())
    } else {
        None
    };

    // Format relative time for last activity
    // When collapsed with children, show effective_last_activity (most recent in entire tree)
    // Otherwise show the conversation's own last_activity
    let display_timestamp = if is_collapsed && has_children {
        thread.effective_last_activity
    } else {
        thread.last_activity
    };
    let time_str = format_relative_time(display_timestamp);

    // Format runtime (similar to ConversationMetadata::formatted_runtime)
    let runtime_str = if hierarchical_runtime > 0 {
        let seconds = hierarchical_runtime as f64 / 1000.0;
        if seconds >= 3600.0 {
            let hours = (seconds / 3600.0).floor();
            let mins = ((seconds % 3600.0) / 60.0).floor();
            Some(format!("⏱ {:.0}h{:.0}m", hours, mins))
        } else if seconds >= 60.0 {
            let mins = (seconds / 60.0).floor();
            let secs = seconds % 60.0;
            Some(format!("⏱ {:.0}m{:.0}s", mins, secs))
        } else {
            Some(format!("⏱ {:.1}s", seconds))
        }
    } else {
        None
    };

    // Column widths for table layout
    // Right column: project (line 1) / time (line 2) / runtime (line 3)
    let right_col_width = 22;

    // Title style uses project color for determinism
    let title_style = if is_selected {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::project_color(a_tag))
    };

    let collapse_indicator = if has_children {
        if is_collapsed {
            card::COLLAPSE_CLOSED
        } else {
            card::COLLAPSE_OPEN
        }
    } else if depth > 0 {
        card::COLLAPSE_LEAF
    } else {
        ""
    };

    // All items reserve the same space for collapse indicator to keep titles aligned
    // COLLAPSE_CLOSED/OPEN use unicode chars that may be width-2, so use display width
    let collapse_col_width = card::COLLAPSE_CLOSED.width();
    let collapse_len = collapse_indicator.width();
    let collapse_padding = collapse_col_width.saturating_sub(collapse_len);

    let mut lines: Vec<Line> = Vec::new();

    // Calculate column widths for table layout (used by both compact and full mode)
    let total_width = area.width as usize;
    let fixed_cols_width = right_col_width + 2; // +2 for spacing
    let main_col_width =
        total_width.saturating_sub(fixed_cols_width + indent_len + collapse_col_width);

    // Status text with symbol (for line 3)
    let status_text = thread
        .status_label
        .as_ref()
        .map(|s| format!("{} {}", status_label_to_symbol(s), s))
        .unwrap_or_default();

    if is_compact {
        // COMPACT: 2 lines
        // LINE 1: [title] [spinner?] [#nested] [recipient]     [project]
        let spinner_suffix = spinner_char.map(|c| format!(" {}", c)).unwrap_or_default();
        let nested_suffix = if is_collapsed && child_count > 0 {
            format!(" +{}", child_count)
        } else {
            String::new()
        };
        // Build recipient suffix (first recipient only in compact mode)
        // Use flexible truncation - only truncate if name is very long
        let recipient_suffix = if let Some((name, _)) = first_recipient.as_ref() {
            let max_recipient_len = 25; // Reasonable max, only truncate if necessary
            format!(" @{}", truncate_with_ellipsis(name, max_recipient_len))
        } else {
            String::new()
        };
        let recipient_pubkey = first_recipient.as_ref().map(|(_, pk)| pk.clone());

        let title_max = main_col_width.saturating_sub(
            nested_suffix.width() + spinner_suffix.width() + recipient_suffix.width(),
        );
        let title_truncated = truncate_with_ellipsis(&thread.title, title_max);
        let title_display_len = title_truncated.width()
            + spinner_suffix.width()
            + nested_suffix.width()
            + recipient_suffix.width();
        let title_padding = main_col_width.saturating_sub(title_display_len);

        // Project (right column, right-aligned)
        let project_truncated =
            truncate_with_ellipsis(&project_name, right_col_width.saturating_sub(2));
        let project_display = format!("{}{}", card::BULLET_GLYPH, project_truncated);
        let project_len = project_display.width();
        let project_padding = right_col_width.saturating_sub(project_len);

        let mut line1 = Vec::new();
        // Add indent for nested items
        if !indent.is_empty() {
            line1.push(Span::styled(indent.clone(), Style::default()));
        }
        if !collapse_indicator.is_empty() {
            line1.push(Span::styled(
                collapse_indicator,
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
        if collapse_padding > 0 {
            line1.push(Span::styled(" ".repeat(collapse_padding), Style::default()));
        }
        line1.push(Span::styled(title_truncated, title_style));
        if thread.is_scheduled {
            line1.push(Span::styled(
                " ⏰ SCHED",
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
        if is_archived {
            line1.push(Span::styled(
                " [archived]",
                Style::default()
                    .fg(theme::TEXT_MUTED)
                    .add_modifier(Modifier::DIM),
            ));
        }
        if has_draft {
            line1.push(Span::styled(
                " ✎",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
        }
        if !spinner_suffix.is_empty() {
            line1.push(Span::styled(
                spinner_suffix,
                Style::default().fg(theme::ACCENT_PRIMARY),
            ));
        }
        if !nested_suffix.is_empty() {
            line1.push(Span::styled(
                nested_suffix,
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
        // Add recipient with deterministic color
        if !recipient_suffix.is_empty() {
            let color = recipient_pubkey
                .as_ref()
                .map(|pk| theme::user_color(pk))
                .unwrap_or(theme::TEXT_MUTED);
            line1.push(Span::styled(recipient_suffix, Style::default().fg(color)));
        }
        line1.push(Span::styled(" ".repeat(title_padding), Style::default()));
        line1.push(Span::styled(" ".repeat(project_padding), Style::default()));
        line1.push(Span::styled(
            project_display,
            Style::default().fg(theme::project_color(a_tag)),
        ));
        lines.push(Line::from(line1));

        // LINE 2: [empty main]                              [time]
        let time_len = time_str.width();
        let time_padding = right_col_width.saturating_sub(time_len);

        let mut line2 = Vec::new();
        // Add indent for nested items
        if !indent.is_empty() {
            line2.push(Span::styled(indent.clone(), Style::default()));
        }
        line2.push(Span::styled(
            " ".repeat(collapse_col_width),
            Style::default(),
        ));
        line2.push(Span::styled(" ".repeat(main_col_width), Style::default()));
        line2.push(Span::styled(" ".repeat(time_padding), Style::default()));
        line2.push(Span::styled(
            time_str,
            Style::default().fg(theme::TEXT_MUTED),
        ));
        lines.push(Line::from(line2));
    } else {
        // FULL MODE: Table-like layout
        // LINE 1: [title] [spinner?] [#nested] [recipient]     [project]
        // LINE 2: [summary]                                    [relative-last-activity]
        // LINE 3: [current status]                             [cumulative llm runtime]
        // LINE 4: spacing

        // Build recipient suffix for line 1
        // Use flexible truncation - only truncate if name is very long
        let recipient_suffix = if let Some((name, _)) = first_recipient.as_ref() {
            let max_recipient_len = 25; // Reasonable max, only truncate if necessary
            format!(" @{}", truncate_with_ellipsis(name, max_recipient_len))
        } else {
            String::new()
        };
        let recipient_pubkey = first_recipient.as_ref().map(|(_, pk)| pk.clone());

        // LINE 1: [title] [spinner?] [#nested] [recipient]     [project]
        let spinner_suffix = spinner_char.map(|c| format!(" {}", c)).unwrap_or_default();
        let nested_suffix = if has_children && child_count > 0 {
            format!(" {}", child_count)
        } else {
            String::new()
        };
        let title_max = main_col_width.saturating_sub(
            nested_suffix.width() + spinner_suffix.width() + recipient_suffix.width(),
        );
        let title_truncated = truncate_with_ellipsis(&thread.title, title_max);
        let title_display_len = title_truncated.width()
            + spinner_suffix.width()
            + nested_suffix.width()
            + recipient_suffix.width();
        let title_padding = main_col_width.saturating_sub(title_display_len);

        // Project for line 1 (right column, right-aligned)
        let project_truncated =
            truncate_with_ellipsis(&project_name, right_col_width.saturating_sub(2));
        let project_display = format!("{}{}", card::BULLET_GLYPH, project_truncated);
        let project_len = project_display.width();
        let project_padding = right_col_width.saturating_sub(project_len);

        let mut line1 = Vec::new();
        // Add indent for nested items
        if !indent.is_empty() {
            line1.push(Span::styled(indent.clone(), Style::default()));
        }
        if !collapse_indicator.is_empty() {
            line1.push(Span::styled(
                collapse_indicator,
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
        if collapse_padding > 0 {
            line1.push(Span::styled(" ".repeat(collapse_padding), Style::default()));
        }
        line1.push(Span::styled(title_truncated, title_style));
        if thread.is_scheduled {
            line1.push(Span::styled(
                " ⏰ SCHED",
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
        if is_archived {
            line1.push(Span::styled(
                " [archived]",
                Style::default()
                    .fg(theme::TEXT_MUTED)
                    .add_modifier(Modifier::DIM),
            ));
        }
        if has_draft {
            line1.push(Span::styled(
                " ✎",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
        }
        if !spinner_suffix.is_empty() {
            line1.push(Span::styled(
                spinner_suffix,
                Style::default().fg(theme::ACCENT_PRIMARY),
            ));
        }
        if !nested_suffix.is_empty() {
            line1.push(Span::styled(
                nested_suffix,
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
        // Add recipient with deterministic color
        if !recipient_suffix.is_empty() {
            let color = recipient_pubkey
                .as_ref()
                .map(|pk| theme::user_color(pk))
                .unwrap_or(theme::TEXT_MUTED);
            line1.push(Span::styled(recipient_suffix, Style::default().fg(color)));
        }
        line1.push(Span::styled(" ".repeat(title_padding), Style::default()));
        line1.push(Span::styled(" ".repeat(project_padding), Style::default()));
        line1.push(Span::styled(
            project_display,
            Style::default().fg(theme::project_color(a_tag)),
        ));
        lines.push(Line::from(line1));

        // LINE 2: [summary]                                    [relative-last-activity]
        let time_len = time_str.width();
        let time_padding = right_col_width.saturating_sub(time_len);
        let summary_max = main_col_width;

        let mut line2 = Vec::new();
        if !indent.is_empty() {
            line2.push(Span::styled(indent.clone(), Style::default()));
        }
        line2.push(Span::styled(
            " ".repeat(collapse_col_width),
            Style::default(),
        ));
        if let Some(ref summary) = thread.summary {
            let summary_truncated = truncate_with_ellipsis(summary, summary_max);
            let summary_len = summary_truncated.width();
            let summary_padding = main_col_width.saturating_sub(summary_len);
            line2.push(Span::styled(
                summary_truncated,
                Style::default().fg(theme::TEXT_MUTED),
            ));
            line2.push(Span::styled(" ".repeat(summary_padding), Style::default()));
        } else {
            line2.push(Span::styled(" ".repeat(main_col_width), Style::default()));
        }
        line2.push(Span::styled(" ".repeat(time_padding), Style::default()));
        line2.push(Span::styled(
            time_str,
            Style::default().fg(theme::TEXT_MUTED),
        ));
        lines.push(Line::from(line2));

        // LINE 3: [current status]                             [cumulative llm runtime]
        // Always render line 3 for consistent layout (even if status/runtime are empty)
        // Truncate runtime to fit within right_col_width to prevent overflow
        let runtime_display = runtime_str.clone().unwrap_or_default();
        let runtime_display = truncate_with_ellipsis(&runtime_display, right_col_width);
        let runtime_len = runtime_display.width();
        let runtime_padding = right_col_width.saturating_sub(runtime_len);

        let mut line3 = Vec::new();
        if !indent.is_empty() {
            line3.push(Span::styled(indent.clone(), Style::default()));
        }
        line3.push(Span::styled(
            " ".repeat(collapse_col_width),
            Style::default(),
        ));

        if !status_text.is_empty() {
            let status_max = main_col_width;
            let status_truncated = truncate_with_ellipsis(&status_text, status_max);
            let status_len = status_truncated.width();
            let status_padding = main_col_width.saturating_sub(status_len);
            line3.push(Span::styled(
                status_truncated,
                Style::default().fg(theme::ACCENT_WARNING),
            ));
            line3.push(Span::styled(" ".repeat(status_padding), Style::default()));
        } else {
            line3.push(Span::styled(" ".repeat(main_col_width), Style::default()));
        }

        line3.push(Span::styled(" ".repeat(runtime_padding), Style::default()));
        line3.push(Span::styled(
            runtime_display,
            Style::default().fg(theme::TEXT_MUTED),
        ));
        lines.push(Line::from(line3));

        // Spacing line (only when neither this nor next card is selected)
        if !is_selected && !is_multi_selected && !next_is_selected {
            lines.push(Line::from(""));
        }
    }

    if is_selected || is_multi_selected {
        // For selected/multi-selected cards, render half-block borders separately from content
        // This creates the visual effect of half-line padding
        let half_block_top = card::OUTER_HALF_BLOCK_BORDER
            .horizontal_bottom
            .repeat(area.width as usize); // ▄
        let half_block_bottom = card::OUTER_HALF_BLOCK_BORDER
            .horizontal_top
            .repeat(area.width as usize); // ▀

        // Top half-block line (fg=selection color, no bg - creates "growing down" effect)
        let top_area = Rect::new(area.x, area.y, area.width, 1);
        let top_line = Paragraph::new(Line::from(Span::styled(
            half_block_top,
            Style::default().fg(theme::BG_SELECTED),
        )));
        f.render_widget(top_line, top_area);

        // Content area (with selection background)
        let content_area = Rect::new(
            area.x,
            area.y + 1,
            area.width,
            area.height.saturating_sub(2),
        );
        let content = Paragraph::new(lines).style(Style::default().bg(theme::BG_SELECTED));
        f.render_widget(content, content_area);

        // Bottom half-block line (fg=selection color, no bg - creates "growing up" effect)
        let bottom_y = area.y + area.height.saturating_sub(1);
        let bottom_area = Rect::new(area.x, bottom_y, area.width, 1);
        let bottom_line = Paragraph::new(Line::from(Span::styled(
            half_block_bottom,
            Style::default().fg(theme::BG_SELECTED),
        )));
        f.render_widget(bottom_line, bottom_area);
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
