use crate::models::{InboxEventType, InboxItem, Thread};
use crate::ui::components::{
    modal_area, render_modal_background, render_modal_header, render_modal_items,
    render_modal_overlay, render_modal_search, render_modal_sections, render_tab_bar, ModalItem,
    ModalSection, ModalSize,
};
use crate::ui::card;
use crate::ui::modal::ModalState;
use crate::ui::format::{format_relative_time, status_label_to_symbol, truncate_with_ellipsis};
use crate::ui::views::home_helpers::build_thread_hierarchy;
pub use crate::ui::views::home_helpers::HierarchicalThread;
use crate::ui::{theme, App, HomeTab};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};

pub fn render_home(f: &mut Frame, app: &App, area: Rect) {
    // Fill entire area with app background (pure black)
    let bg_block = Block::default().style(Style::default().bg(theme::BG_APP));
    f.render_widget(bg_block, area);

    let has_tabs = !app.open_tabs.is_empty();

    // Layout: Header tabs | Main area | Help bar | Optional tab bar
    let chunks = if has_tabs {
        Layout::vertical([
            Constraint::Length(2), // Tab header
            Constraint::Min(0),    // Main area (sidebar + content)
            Constraint::Length(1), // Help bar
            Constraint::Length(1), // Open tabs bar
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Length(2), // Tab header
            Constraint::Min(0),    // Main area (sidebar + content)
            Constraint::Length(1), // Help bar
        ])
        .split(area)
    };

    // Render tab header
    render_tab_header(f, app, chunks[0]);

    // Split main area into content and sidebar (sidebar on RIGHT)
    let main_chunks = Layout::horizontal([
        Constraint::Min(0),     // Content
        Constraint::Length(42), // Sidebar (fixed width, on RIGHT)
    ])
    .split(chunks[1]);

    // Render content based on active tab (with left and right padding)
    let content_area = main_chunks[0];
    let padded_content = Rect::new(
        content_area.x + 2, // 2-char left padding
        content_area.y,
        content_area.width.saturating_sub(4), // 2 left + 2 right padding (gap before sidebar)
        content_area.height,
    );
    match app.home_panel_focus {
        HomeTab::Recent => render_recent_with_feed(f, app, padded_content),
        HomeTab::Inbox => render_inbox_cards(f, app, padded_content),
    }

    // Render sidebar on the right
    render_project_sidebar(f, app, main_chunks[1]);

    // Single consolidated help bar
    render_help_bar(f, app, chunks[2]);

    // Open tabs bar (if tabs exist)
    if has_tabs {
        render_tab_bar(f, app, chunks[3]);
    }

    // Projects modal overlay
    if matches!(app.modal_state, ModalState::ProjectsModal { .. }) {
        render_projects_modal(f, app, area);
    }

    // Project settings modal overlay
    if let ModalState::ProjectSettings(ref state) = app.modal_state {
        super::render_project_settings(f, app, area, state);
    }

    // Tab modal overlay (Alt+/)
    if app.showing_tab_modal {
        render_tab_modal(f, app, area);
    }

    // Search modal overlay (/)
    if app.showing_search_modal {
        render_search_modal(f, app, area);
    }
}

fn render_tab_header(f: &mut Frame, app: &App, area: Rect) {
    let inbox_count = app.inbox_items().iter().filter(|i| !i.is_read).count();

    let tab_style = |tab: HomeTab| {
        if app.home_panel_focus == tab {
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        }
    };

    // Build tab spans
    let mut spans = vec![
        Span::styled("  TENEX", Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)),
        Span::styled("    ", Style::default()),
        Span::styled("Recent", tab_style(HomeTab::Recent)),
        Span::styled("   ", Style::default()),
        Span::styled("Inbox", tab_style(HomeTab::Inbox)),
    ];

    if inbox_count > 0 {
        spans.push(Span::styled(
            format!(" ({})", inbox_count),
            Style::default().fg(theme::ACCENT_ERROR),
        ));
    }

    let header_line = Line::from(spans);

    // Second line: tab indicator underline
    let accent = Style::default().fg(theme::ACCENT_PRIMARY);
    let blank = Style::default();

    let indicator_spans = vec![
        Span::styled("         ", blank), // Padding for "  TENEX  "
        Span::styled(if app.home_panel_focus == HomeTab::Recent { "──────" } else { "      " },
            if app.home_panel_focus == HomeTab::Recent { accent } else { blank }),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Inbox { "─────" } else { "     " },
            if app.home_panel_focus == HomeTab::Inbox { accent } else { blank }),
        Span::styled(if inbox_count > 0 { "    " } else { "" }, blank), // account for count
    ];
    let indicator_line = Line::from(indicator_spans);

    let header = Paragraph::new(vec![header_line, indicator_line]);
    f.render_widget(header, area);
}

fn render_recent_with_feed(f: &mut Frame, app: &App, area: Rect) {
    render_recent_cards(f, app, area, true);
}

fn render_recent_cards(f: &mut Frame, app: &App, area: Rect, is_focused: bool) {
    let recent = app.recent_threads();

    if recent.is_empty() {
        let empty = Paragraph::new("No recent conversations")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty, area);
        return;
    }

    // Get q-tag relationships for fallback parent-child detection
    let q_tag_relationships = app.data_store.borrow().get_q_tag_relationships();

    // Build hierarchical thread list
    let hierarchy = build_thread_hierarchy(&recent, &app.collapsed_threads, &q_tag_relationships);

    // Helper to calculate card height
    // Full mode: 3 lines (title+project+status, summary+author+time, spacing) + optional activity
    // Compact mode: 2 lines (title, spacing)
    // Selected items add 2 lines for half-block borders (top + bottom)
    let calc_card_height = |item: &HierarchicalThread, is_selected: bool| -> u16 {
        let is_compact = item.depth > 0;
        let has_activity = item.thread.status_current_activity.is_some();
        let base = if is_compact {
            2
        } else if has_activity {
            4 // title row, summary row, activity row, spacing
        } else {
            3 // title row, summary row, spacing
        };
        if is_selected { base + 2 } else { base }
    };

    // Calculate scroll offset to keep selected item visible
    let selected_idx = app.selected_recent_index;
    let mut scroll_offset: u16 = 0;

    // Calculate cumulative height up to and including selected item
    let mut height_before_selected: u16 = 0;
    let mut selected_height: u16 = 0;
    for (i, item) in hierarchy.iter().enumerate() {
        let item_is_selected = is_focused && i == selected_idx;
        let h = calc_card_height(item, item_is_selected);
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
        let h = calc_card_height(item, is_selected);

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

            render_card_content(
                f,
                app,
                &item.thread,
                &item.a_tag,
                is_selected,
                item.depth,
                item.has_children,
                item.child_count,
                item.is_collapsed,
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
    build_thread_hierarchy(&recent, &app.collapsed_threads, &q_tag_relationships)
}

/// Render card content in table-like format:
/// [title] [#]            [project]       [status]
/// [summary]              [author]        [time]
fn render_card_content(
    f: &mut Frame,
    app: &App,
    thread: &Thread,
    a_tag: &str,
    is_selected: bool,
    depth: usize,
    has_children: bool,
    child_count: usize,
    is_collapsed: bool,
    area: Rect,
) {
    let is_compact = depth > 0;
    let indent = card::INDENT_UNIT.repeat(depth);
    let indent_len = indent.chars().count();

    // Extract data
    let (project_name, thread_author_name, preview, timestamp) = {
        let store = app.data_store.borrow();
        let project_name = store.get_project_name(a_tag);
        // Thread author is the person who created/started the thread
        let thread_author_name = store.get_profile_name(&thread.pubkey);
        let messages = store.get_messages(&thread.id);
        let (preview, timestamp) = if let Some(msg) = messages.last() {
            let preview: String = msg.content.chars().take(80).collect();
            let preview = preview.replace('\n', " ");
            (preview, msg.created_at)
        } else {
            ("No messages yet".to_string(), thread.last_activity)
        };
        (project_name, thread_author_name, preview, timestamp)
    };

    let time_str = format_relative_time(timestamp);

    // Column widths for table layout
    // Middle column: project (line 1) / author (line 2) - same width for alignment
    // Right column: status (line 1) / time (line 2) - same width for alignment
    let middle_col_width = 18;
    let right_col_width = 10;

    let title_style = if is_selected {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
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
    // COLLAPSE_CLOSED/OPEN are 2 chars ("▶ " / "▼ "), so use that as the standard width
    let collapse_col_width = card::COLLAPSE_CLOSED.chars().count();
    let collapse_len = collapse_indicator.chars().count();
    let collapse_padding = collapse_col_width.saturating_sub(collapse_len);

    let mut lines: Vec<Line> = Vec::new();

    // Calculate column widths for table layout (used by both compact and full mode)
    let total_width = area.width as usize;
    let fixed_cols_width = middle_col_width + right_col_width + 2; // +2 for spacing
    let main_col_width = total_width.saturating_sub(fixed_cols_width + indent_len + collapse_col_width);

    // Status text (for right column)
    let status_text = thread.status_label.as_ref()
        .map(|s| format!("{} {}", status_label_to_symbol(s), s))
        .unwrap_or_default();

    if is_compact {
        // COMPACT: 2 lines - same table layout as full mode
        // LINE 1: [title] [#nested]     [project]     [status]
        let nested_suffix = if is_collapsed && child_count > 0 {
            format!(" +{}", child_count)
        } else {
            String::new()
        };
        let title_max = main_col_width.saturating_sub(nested_suffix.chars().count());
        let title_truncated = truncate_with_ellipsis(&thread.title, title_max);
        let title_display_len = title_truncated.chars().count() + nested_suffix.chars().count();
        let title_padding = main_col_width.saturating_sub(title_display_len);

        // Project (middle column)
        let project_truncated = truncate_with_ellipsis(&project_name, middle_col_width.saturating_sub(2));
        let project_display = format!("{}{}", card::BULLET_GLYPH, project_truncated);
        let project_len = project_display.chars().count();
        let project_padding = middle_col_width.saturating_sub(project_len);

        // Status (right column, right-aligned)
        let status_truncated = truncate_with_ellipsis(&status_text, right_col_width);
        let status_len = status_truncated.chars().count();
        let status_padding = right_col_width.saturating_sub(status_len);

        let mut line1 = Vec::new();
        // Add indent for nested items
        if !indent.is_empty() {
            line1.push(Span::styled(indent.clone(), Style::default()));
        }
        if !collapse_indicator.is_empty() {
            line1.push(Span::styled(collapse_indicator, Style::default().fg(theme::TEXT_MUTED)));
        }
        if collapse_padding > 0 {
            line1.push(Span::styled(" ".repeat(collapse_padding), Style::default()));
        }
        line1.push(Span::styled(title_truncated, title_style));
        if !nested_suffix.is_empty() {
            line1.push(Span::styled(nested_suffix, Style::default().fg(theme::TEXT_MUTED)));
        }
        line1.push(Span::styled(" ".repeat(title_padding), Style::default()));
        line1.push(Span::styled(project_display, Style::default().fg(theme::ACCENT_SUCCESS)));
        line1.push(Span::styled(" ".repeat(project_padding), Style::default()));
        line1.push(Span::styled(" ".repeat(status_padding), Style::default()));
        line1.push(Span::styled(status_truncated, Style::default().fg(theme::ACCENT_WARNING)));
        lines.push(Line::from(line1));

        // LINE 2: [empty main]          [author]      [time]
        let author_display = format!("@{}", thread_author_name);
        let author_truncated = truncate_with_ellipsis(&author_display, middle_col_width.saturating_sub(1));
        let author_len = author_truncated.chars().count();
        let author_padding = middle_col_width.saturating_sub(author_len);

        let time_len = time_str.chars().count();
        let time_padding = right_col_width.saturating_sub(time_len);

        let mut line2 = Vec::new();
        // Add indent for nested items
        if !indent.is_empty() {
            line2.push(Span::styled(indent.clone(), Style::default()));
        }
        line2.push(Span::styled(" ".repeat(collapse_col_width), Style::default()));
        line2.push(Span::styled(" ".repeat(main_col_width), Style::default()));
        line2.push(Span::styled(author_truncated, Style::default().fg(theme::ACCENT_SPECIAL)));
        line2.push(Span::styled(" ".repeat(author_padding), Style::default()));
        line2.push(Span::styled(" ".repeat(time_padding), Style::default()));
        line2.push(Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)));
        lines.push(Line::from(line2));
    } else {
        // FULL MODE: Table-like layout (3 lines + optional activity + spacing)

        // LINE 1: [title] [#nested]     [project]     [status]
        // Build title with nested count
        let nested_suffix = if has_children && child_count > 0 {
            format!(" {}", child_count)
        } else {
            String::new()
        };
        let title_max = main_col_width.saturating_sub(nested_suffix.chars().count());
        let title_truncated = truncate_with_ellipsis(&thread.title, title_max);
        let title_with_nested = format!("{}{}", title_truncated, nested_suffix);
        let title_display_len = title_with_nested.chars().count();
        let title_padding = main_col_width.saturating_sub(title_display_len);

        // Project for line 1 (middle column)
        let project_truncated = truncate_with_ellipsis(&project_name, middle_col_width.saturating_sub(2));
        let project_display = format!("{}{}", card::BULLET_GLYPH, project_truncated);
        let project_len = project_display.chars().count();
        let project_padding = middle_col_width.saturating_sub(project_len);

        // Status for line 1 (right column, right-aligned)
        let status_truncated = truncate_with_ellipsis(&status_text, right_col_width);
        let status_len = status_truncated.chars().count();
        let status_padding = right_col_width.saturating_sub(status_len);

        let mut line1 = Vec::new();
        // Add indent for nested items
        if !indent.is_empty() {
            line1.push(Span::styled(indent.clone(), Style::default()));
        }
        if !collapse_indicator.is_empty() {
            line1.push(Span::styled(collapse_indicator, Style::default().fg(theme::TEXT_MUTED)));
        }
        if collapse_padding > 0 {
            line1.push(Span::styled(" ".repeat(collapse_padding), Style::default()));
        }
        line1.push(Span::styled(title_truncated, title_style));
        if !nested_suffix.is_empty() {
            line1.push(Span::styled(nested_suffix, Style::default().fg(theme::TEXT_MUTED)));
        }
        line1.push(Span::styled(" ".repeat(title_padding), Style::default()));
        line1.push(Span::styled(project_display, Style::default().fg(theme::ACCENT_SUCCESS)));
        line1.push(Span::styled(" ".repeat(project_padding), Style::default()));
        line1.push(Span::styled(" ".repeat(status_padding), Style::default()));
        line1.push(Span::styled(status_truncated, Style::default().fg(theme::ACCENT_WARNING)));
        lines.push(Line::from(line1));

        // LINE 2: [summary]            [author]      [time]
        let preview_max = main_col_width;
        let preview_truncated = truncate_with_ellipsis(&preview, preview_max);
        let preview_len = preview_truncated.chars().count();
        let preview_padding = main_col_width.saturating_sub(preview_len);

        // Author for line 2 (middle column) - show thread creator
        let author_display = format!("@{}", thread_author_name);
        let author_truncated = truncate_with_ellipsis(&author_display, middle_col_width.saturating_sub(1));
        let author_len = author_truncated.chars().count();
        let author_padding = middle_col_width.saturating_sub(author_len);

        // Time for line 2 (right column, right-aligned)
        let time_len = time_str.chars().count();
        let time_padding = right_col_width.saturating_sub(time_len);

        let mut line2 = Vec::new();
        // Add indent for nested items
        if !indent.is_empty() {
            line2.push(Span::styled(indent.clone(), Style::default()));
        }
        line2.push(Span::styled(" ".repeat(collapse_col_width), Style::default())); // Align with collapse indicator
        line2.push(Span::styled(preview_truncated, Style::default().fg(theme::TEXT_MUTED)));
        line2.push(Span::styled(" ".repeat(preview_padding), Style::default()));
        line2.push(Span::styled(author_truncated, Style::default().fg(theme::ACCENT_SPECIAL)));
        line2.push(Span::styled(" ".repeat(author_padding), Style::default()));
        line2.push(Span::styled(" ".repeat(time_padding), Style::default()));
        line2.push(Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)));
        lines.push(Line::from(line2));

        // Line 3: Activity (if present)
        if let Some(ref activity) = thread.status_current_activity {
            let mut line3 = Vec::new();
            // Add indent for nested items
            if !indent.is_empty() {
                line3.push(Span::styled(indent.clone(), Style::default()));
            }
            line3.push(Span::styled(" ".repeat(collapse_col_width), Style::default()));
            line3.push(Span::styled(card::ACTIVITY_GLYPH, Style::default().fg(theme::ACCENT_PRIMARY)));
            line3.push(Span::styled(truncate_with_ellipsis(activity, main_col_width.saturating_sub(3)), Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)));
            lines.push(Line::from(line3));
        }
        // Spacing line
        lines.push(Line::from(""));
    }

    if is_selected {
        // For selected cards, render half-block borders separately from content
        // This creates the visual effect of half-line padding
        let half_block_top = card::OUTER_HALF_BLOCK_BORDER.horizontal_bottom.repeat(area.width as usize); // ▄
        let half_block_bottom = card::OUTER_HALF_BLOCK_BORDER.horizontal_top.repeat(area.width as usize); // ▀

        // Top half-block line (fg=selection color, no bg - creates "growing down" effect)
        let top_area = Rect::new(area.x, area.y, area.width, 1);
        let top_line = Paragraph::new(Line::from(Span::styled(
            half_block_top,
            Style::default().fg(theme::BG_SELECTED),
        )));
        f.render_widget(top_line, top_area);

        // Content area (with selection background)
        let content_area = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(2));
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

fn render_inbox_cards(f: &mut Frame, app: &App, area: Rect) {
    let inbox_items = app.inbox_items();

    if inbox_items.is_empty() {
        let empty_lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "No notifications",
                Style::default().fg(theme::TEXT_MUTED),
            )),
        ];
        let empty = Paragraph::new(empty_lines)
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(empty, area);
        return;
    }

    let items: Vec<ListItem> = inbox_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == app.selected_inbox_index;
            render_inbox_card(app, item, is_selected)
        })
        .collect();

    // No block/border - just the list directly
    let list = List::new(items).highlight_style(Style::default());

    let mut state = ListState::default();
    state.select(Some(app.selected_inbox_index));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_inbox_card(app: &App, item: &InboxItem, is_selected: bool) -> ListItem<'static> {
    // Single borrow to extract all needed data
    let (project_name, author_name) = {
        let store = app.data_store.borrow();
        (store.get_project_name(&item.project_a_tag), store.get_profile_name(&item.author_pubkey))
    };
    let time_str = format_relative_time(item.created_at);

    let type_str = match item.event_type {
        InboxEventType::Mention => "Mention",
        InboxEventType::Reply => "Reply",
        InboxEventType::ThreadReply => "Thread reply",
    };

    let title_style = if is_selected {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else if !item.is_read {
        Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    // Unread indicator
    let unread_indicator = if !item.is_read {
        Span::styled(card::BULLET, Style::default().fg(theme::ACCENT_PRIMARY))
    } else {
        Span::styled(card::SPACER, Style::default())
    };

    // Line 1: Title + time
    let line1_spans = vec![
        unread_indicator,
        Span::styled(truncate_with_ellipsis(&item.title, 55), title_style),
        Span::styled(card::SPACER, Style::default()),
        Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)),
    ];

    // Line 2: Type + Project + Author
    let line2_spans = vec![
        Span::styled(type_str, Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" in ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(project_name, Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled(" by ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(author_name, Style::default().fg(theme::ACCENT_SPECIAL)),
    ];

    // Line 3: Spacing line
    let line3_spans = vec![Span::styled(card::SPACER, Style::default())];

    ListItem::new(vec![
        Line::from(line1_spans),
        Line::from(line2_spans),
        Line::from(line3_spans),
    ])
}

/// Render the project sidebar with checkboxes for filtering
fn render_project_sidebar(f: &mut Frame, app: &App, area: Rect) {
    // Split sidebar into projects list and filter section
    let chunks = Layout::vertical([
        Constraint::Min(5),    // Projects list
        Constraint::Length(4), // Filter section
    ])
    .split(area);

    render_projects_list(f, app, chunks[0]);
    render_filters_section(f, app, chunks[1]);
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
            Span::styled("Online", Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)),
        ])));
    }

    // Online projects - now empty = none (inverted)
    for (i, project) in online_projects.iter().enumerate() {
        let a_tag = project.a_tag();
        let is_visible = app.visible_projects.contains(&a_tag);
        let is_focused = selected_project_index == Some(i);

        let checkbox = if is_visible { card::CHECKBOX_ON_PAD } else { card::CHECKBOX_OFF_PAD };
        let focus_indicator = if is_focused { card::COLLAPSE_CLOSED } else { card::SPACER };
        let name = truncate_with_ellipsis(&project.name, 20); // Fits wider sidebar

        let checkbox_style = if is_focused {
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::ACCENT_PRIMARY)
        };

        let name_style = if is_focused {
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        let item = ListItem::new(Line::from(vec![
            Span::styled(focus_indicator, Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled(checkbox, checkbox_style),
            Span::styled(name, name_style),
        ]));

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

        let checkbox = if is_visible { card::CHECKBOX_ON_PAD } else { card::CHECKBOX_OFF_PAD };
        let focus_indicator = if is_focused { card::COLLAPSE_CLOSED } else { card::SPACER };
        let name = truncate_with_ellipsis(&project.name, 20); // Fits wider sidebar

        let checkbox_style = if is_focused {
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        let name_style = if is_focused {
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        let item = ListItem::new(Line::from(vec![
            Span::styled(focus_indicator, Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled(checkbox, checkbox_style),
            Span::styled(name, name_style),
        ]));

        let item = if is_focused {
            item.style(Style::default().bg(theme::BG_SELECTED))
        } else {
            item
        };

        items.push(item);
    }

    let list = List::new(items)
        .block(Block::default()
            .borders(Borders::NONE)
            .padding(Padding::new(2, 2, 1, 0))) // Reduced left padding to fit indicator
        .style(Style::default().bg(theme::BG_SIDEBAR));

    f.render_widget(list, area);
}

/// Render the filters section below projects
fn render_filters_section(f: &mut Frame, app: &App, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Separator line
    lines.push(Line::from(Span::styled(
        "─ Filters ─",
        Style::default().fg(theme::TEXT_MUTED),
    )));

    // "Only by me" filter
    let only_by_me_checkbox = if app.only_by_me { card::CHECKBOX_ON } else { card::CHECKBOX_OFF };
    let only_by_me_style = if app.only_by_me {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    lines.push(Line::from(vec![
        Span::styled("[m] ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(only_by_me_checkbox, only_by_me_style),
        Span::styled(" By me", only_by_me_style),
    ]));

    // Time filter
    let time_label = app.time_filter
        .map(|tf| tf.label())
        .unwrap_or("All");
    let time_style = if app.time_filter.is_some() {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let time_indicator = if app.time_filter.is_some() {
        format!(" {}", card::CHECKMARK)
    } else {
        String::new()
    };
    lines.push(Line::from(vec![
        Span::styled("[f] ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(format!("Time: {}{}", time_label, time_indicator), time_style),
    ]));

    let filter_widget = Paragraph::new(lines)
        .block(Block::default()
            .borders(Borders::NONE)
            .padding(Padding::new(4, 4, 0, 1))) // left, right, top, bottom
        .style(Style::default().bg(theme::BG_SIDEBAR));

    f.render_widget(filter_widget, area);
}

fn render_help_bar(f: &mut Frame, app: &App, area: Rect) {
    let hints = if app.sidebar_focused {
        "← back · ↑↓ navigate · Space toggle · m filter · f time · A agents · Tab switch · q quit"
    } else {
        match app.home_panel_focus {
            HomeTab::Recent => "→ projects · ↑↓ navigate · Space fold · Enter open · n new · m filter · f time · A agents · q quit",
            HomeTab::Inbox => "→ projects · ↑↓ navigate · Enter open · r mark read · m filter · f time · A agents · q quit",
        }
    };

    let help = Paragraph::new(hints).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(help, area);
}

/// Get the actual project at the given selection index
/// Returns (project, is_online)
pub fn get_project_at_index(app: &App, index: usize) -> Option<(crate::models::Project, bool)> {
    let (online_projects, offline_projects) = app.filtered_projects();

    if index < online_projects.len() {
        online_projects.get(index).map(|p| (p.clone(), true))
    } else {
        let offline_index = index - online_projects.len();
        offline_projects.get(offline_index).map(|p| (p.clone(), false))
    }
}

/// Get the total count of selectable projects
pub fn selectable_project_count(app: &App) -> usize {
    let (online, offline) = app.filtered_projects();
    online.len() + offline.len()
}

fn render_projects_modal(f: &mut Frame, app: &App, area: Rect) {
    // Dim the background
    render_modal_overlay(f, area);

    let size = ModalSize {
        max_width: 65,
        height_percent: 0.7,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    // Add vertical padding
    let inner_area = Rect::new(
        popup_area.x,
        popup_area.y + 1,
        popup_area.width,
        popup_area.height.saturating_sub(3), // Leave room for hints
    );

    // Render header
    let remaining = render_modal_header(f, inner_area, "Switch Project", "esc");

    // Render search
    let filter = app.projects_modal_filter();
    let remaining = render_modal_search(f, remaining, filter, "Search projects...");

    // Build sections
    let data_store = app.data_store.borrow();
    let (online_projects, offline_projects) = app.filtered_projects();
    let selected_index = app.projects_modal_index();

    let mut sections = Vec::new();

    // Online section
    if !online_projects.is_empty() {
        let online_items: Vec<ModalItem> = online_projects
            .iter()
            .enumerate()
            .map(|(idx, project)| {
                let is_selected = idx == selected_index;
                let owner_name = data_store.get_profile_name(&project.pubkey);
                let agent_count = data_store
                    .get_project_status(&project.a_tag())
                    .map(|s| s.agents.len())
                    .unwrap_or(0);

                ModalItem::new(&project.name)
                    .with_shortcut(format!("{} agents · {}", agent_count, owner_name))
                    .selected(is_selected)
            })
            .collect();

        sections.push(
            ModalSection::new(format!("Online ({})", online_projects.len()))
                .with_items(online_items),
        );
    }

    // Offline section
    if !offline_projects.is_empty() {
        let offline_items: Vec<ModalItem> = offline_projects
            .iter()
            .enumerate()
            .map(|(idx, project)| {
                let offset = online_projects.len();
                let is_selected = offset + idx == selected_index;
                let owner_name = data_store.get_profile_name(&project.pubkey);

                ModalItem::new(&project.name)
                    .with_shortcut(owner_name)
                    .selected(is_selected)
            })
            .collect();

        sections.push(
            ModalSection::new(format!("Offline ({})", offline_projects.len()))
                .with_items(offline_items),
        );
    }
    drop(data_store);

    // Render sections
    render_modal_sections(f, remaining, &sections);

    // Render hints at the bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("↑↓ navigate · enter select · esc close")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

/// Render the tab modal (Alt+/) showing all open tabs
pub fn render_tab_modal(f: &mut Frame, app: &App, area: Rect) {
    // Dim the background
    render_modal_overlay(f, area);

    // Calculate modal dimensions - dynamic based on tab count
    let tab_count = app.open_tabs.len();
    let content_height = (tab_count + 2) as u16; // +2 for header spacing
    let total_height = content_height + 4; // +4 for padding and hints
    let height_percent = (total_height as f32 / area.height as f32).min(0.7);

    let size = ModalSize {
        max_width: 70,
        height_percent,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    // Add vertical padding
    let inner_area = Rect::new(
        popup_area.x,
        popup_area.y + 1,
        popup_area.width,
        popup_area.height.saturating_sub(2),
    );

    // Render header with title and hint
    let remaining = render_modal_header(f, inner_area, "Open Tabs", "esc");

    // Build items list
    let data_store = app.data_store.borrow();
    let items: Vec<ModalItem> = app
        .open_tabs
        .iter()
        .enumerate()
        .map(|(i, tab)| {
            let is_selected = i == app.tab_modal_index;
            let is_active = i == app.active_tab_index;

            let project_name = data_store.get_project_name(&tab.project_a_tag);
            let title_display = truncate_with_ellipsis(&tab.thread_title, 30);

            let active_marker = if is_active { card::BULLET } else { card::SPACER };
            let text = format!("{}{} · {}", active_marker, project_name, title_display);

            ModalItem::new(text)
                .with_shortcut(format!("{}", i + 1))
                .selected(is_selected)
        })
        .collect();
    drop(data_store);

    // Render the items
    render_modal_items(f, remaining, &items);

    // Render hints at the bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("↑↓ navigate · enter switch · x close · 0-9 jump")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

/// Render the search modal (/) showing search results
pub fn render_search_modal(f: &mut Frame, app: &App, area: Rect) {
    use crate::ui::app::SearchMatchType;

    // Dim the background
    render_modal_overlay(f, area);

    let size = ModalSize {
        max_width: 80,
        height_percent: 0.8,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    // Add vertical padding
    let inner_area = Rect::new(
        popup_area.x,
        popup_area.y + 1,
        popup_area.width,
        popup_area.height.saturating_sub(3),
    );

    // Render header with title and hint
    let remaining = render_modal_header(f, inner_area, "Search", "esc");

    // Render search input
    let remaining = render_modal_search(f, remaining, &app.search_filter, "Search threads and messages...");

    // Get search results
    let results = app.search_results();

    if results.is_empty() {
        // Show placeholder or "no results" message
        let content_area = Rect::new(
            remaining.x + 2,
            remaining.y,
            remaining.width.saturating_sub(4),
            remaining.height,
        );

        let msg = if app.search_filter.is_empty() {
            "Type to search threads and messages"
        } else {
            "No results found"
        };

        let placeholder = Paragraph::new(msg)
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(placeholder, content_area);
    } else {
        // Build items list from results
        let items: Vec<ModalItem> = results
            .iter()
            .enumerate()
            .map(|(i, result)| {
                let is_selected = i == app.search_index;

                // Format the result line
                let (type_indicator, main_text) = match &result.match_type {
                    SearchMatchType::Thread => {
                        let title = truncate_with_ellipsis(&result.thread.title, 50);
                        if let Some(excerpt) = &result.excerpt {
                            ("T", format!("{} - {}", title, truncate_with_ellipsis(excerpt, 30)))
                        } else {
                            ("T", title)
                        }
                    }
                    SearchMatchType::ConversationId => {
                        let title = truncate_with_ellipsis(&result.thread.title, 30);
                        let id_preview = truncate_with_ellipsis(&result.thread.id, 20);
                        ("I", format!("{} ({})", title, id_preview))
                    }
                    SearchMatchType::Message { .. } => {
                        let title = truncate_with_ellipsis(&result.thread.title, 25);
                        let excerpt = result.excerpt.as_deref().unwrap_or("");
                        ("M", format!("{} - {}", title, truncate_with_ellipsis(excerpt, 35)))
                    }
                };

                let text = format!("[{}] {}", type_indicator, main_text);
                let project_display = truncate_with_ellipsis(&result.project_name, 15);

                ModalItem::new(text)
                    .with_shortcut(project_display)
                    .selected(is_selected)
            })
            .collect();

        // Render the items
        render_modal_items(f, remaining, &items);
    }

    // Render hints at the bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("↑↓ navigate · enter open · esc close")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}
