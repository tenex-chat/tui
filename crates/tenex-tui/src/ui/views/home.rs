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
use crate::ui::{theme, App, HomeTab, NewThreadField};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph},
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

    // New thread modal overlay
    if app.showing_new_thread_modal {
        render_new_thread_modal(f, app, area);
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
    let calc_card_height = |item: &HierarchicalThread| -> u16 {
        let is_compact = item.depth > 0;
        let has_activity = item.thread.status_current_activity.is_some();
        if is_compact {
            2
        } else if has_activity {
            5
        } else {
            4
        }
    };

    // Calculate scroll offset to keep selected item visible
    let selected_idx = app.selected_recent_index;
    let mut scroll_offset: u16 = 0;

    // Calculate cumulative height up to and including selected item
    let mut height_before_selected: u16 = 0;
    let mut selected_height: u16 = 0;
    for (i, item) in hierarchy.iter().enumerate() {
        let h = calc_card_height(item);
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
        let h = calc_card_height(item);

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

fn render_conversation_card(
    app: &App,
    thread: &Thread,
    a_tag: &str,
    is_selected: bool,
    depth: usize,
    has_children: bool,
    child_count: usize,
    is_collapsed: bool,
) -> ListItem<'static> {
    // Use compact mode for nested threads (depth > 0)
    let is_compact = depth > 0;

    // Indentation based on nesting level
    let indent = card::INDENT_UNIT.repeat(depth);

    // Single borrow to extract all needed data
    let (project_name, author_name, author_pubkey, preview, timestamp) = {
        let store = app.data_store.borrow();
        let project_name = store.get_project_name(a_tag);

        // Get last message info without cloning the entire vector
        let messages = store.get_messages(&thread.id);
        let (author_name, author_pubkey, preview, timestamp) = if let Some(msg) = messages.last() {
            let name = store.get_profile_name(&msg.pubkey);
            let preview: String = msg.content.chars().take(80).collect();
            let preview = preview.replace('\n', " ");
            (name, msg.pubkey.clone(), preview, msg.created_at)
        } else {
            ("".to_string(), thread.pubkey.clone(), "No messages yet".to_string(), thread.last_activity)
        };

        (project_name, author_name, author_pubkey, preview, timestamp)
    };

    // Get avatar color and initial for the author
    let avatar_color = theme::user_color(&author_pubkey);
    let avatar_initial = author_name
        .chars()
        .next()
        .or_else(|| author_pubkey.chars().next())
        .unwrap_or('?')
        .to_uppercase()
        .next()
        .unwrap_or('?');

    // Helper to create avatar spans - colored background with initial centered
    let avatar_bg_style = Style::default().bg(avatar_color).fg(
        if theme::is_light_color(avatar_color) { ratatui::style::Color::Black } else { ratatui::style::Color::White }
    );
    let avatar_plain_style = Style::default().bg(avatar_color);

    let time_str = format_relative_time(timestamp);

    // Card styling - left border color is deterministic based on project a_tag
    let project_indicator_color = theme::project_color(a_tag);
    let border_char = card::BORDER_GLYPH;
    let border_style = Style::default().fg(project_indicator_color);

    let title_style = if is_selected {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    // Collapse/expand indicator
    let collapse_indicator = if has_children {
        if is_collapsed {
            card::COLLAPSE_CLOSED.to_string() // Collapsed - point right
        } else {
            card::COLLAPSE_OPEN.to_string() // Expanded - point down
        }
    } else if depth > 0 {
        card::COLLAPSE_LEAF.to_string() // Nested leaf node
    } else {
        String::new()
    };

    if is_compact {
        // COMPACT MODE: 2 lines for nested threads (with smaller avatar)
        // Line 1: Avatar with initial + content
        let mut line1_spans = vec![
            Span::styled(format!("{} ", avatar_initial), avatar_bg_style.add_modifier(Modifier::BOLD)),
            Span::styled(indent.clone(), Style::default()),
            Span::styled(collapse_indicator, Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(border_char, border_style),
        ];

        // Add status indicator if present
        if let Some(ref status_label) = thread.status_label {
            let symbol = status_label_to_symbol(status_label);
            line1_spans.push(Span::styled(
                format!("{} ", symbol),
                Style::default().fg(theme::ACCENT_WARNING),
            ));
        }

        // Title (truncated more for compact view)
        line1_spans.push(Span::styled(
            truncate_with_ellipsis(&thread.title, 40),
            title_style,
        ));

        // Collapsed indicator showing child count
        if is_collapsed && child_count > 0 {
            line1_spans.push(Span::styled(
                format!("{}+{}", card::SPACER, child_count),
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }

        // Time
        line1_spans.push(Span::styled(card::SPACER, Style::default()));
        line1_spans.push(Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)));

        // Spacing line with avatar continuation
        let spacing_spans = vec![
            Span::styled(card::SPACER, avatar_plain_style),
            Span::styled(indent.clone(), Style::default()),
            Span::styled(card::SPACER, Style::default()),  // Space for collapse indicator
            Span::styled(border_char, border_style),
        ];
        let lines = vec![Line::from(line1_spans), Line::from(spacing_spans)];

        let item = ListItem::new(lines);
        if is_selected {
            item.style(Style::default().bg(theme::BG_SELECTED))
        } else {
            item
        }
    } else {
        // FULL MODE: For root threads (4-5 lines with avatar column)

        // Line 1: Avatar top + Collapse indicator + Status label (if present) + Title + time
        let mut line1_spans = vec![
            Span::styled(card::SPACER, avatar_plain_style),  // Avatar row 1 (solid color)
            Span::styled(indent.clone(), Style::default()),
        ];

        if !collapse_indicator.is_empty() {
            line1_spans.push(Span::styled(collapse_indicator, Style::default().fg(theme::TEXT_MUTED)));
        }

        line1_spans.push(Span::styled(border_char, border_style));

        // Add status label with symbol if present
        if let Some(ref status_label) = thread.status_label {
            let symbol = status_label_to_symbol(status_label);
            line1_spans.push(Span::styled(
                format!("[{} {}] ", symbol, status_label),
                Style::default().fg(theme::ACCENT_WARNING),
            ));
        }

        line1_spans.push(Span::styled(
            truncate_with_ellipsis(&thread.title, 60),
            title_style,
        ));

        // Add time on the right (we'll pad later in rendering)
        line1_spans.push(Span::styled(card::SPACER, Style::default()));
        line1_spans.push(Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)));

        // Line 2: Avatar with initial + Project + agent + nested count
        let mut line2_spans = vec![
            Span::styled(format!("{} ", avatar_initial), avatar_bg_style.add_modifier(Modifier::BOLD)),  // Avatar row 2 (initial)
            Span::styled(indent.clone(), Style::default()),
            Span::styled(card::SPACER, Style::default()),  // Space for collapse indicator
            Span::styled(border_char, border_style),
            Span::styled(card::BULLET, Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(project_name, Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(card::SPACER, Style::default()),
            Span::styled(format!("@{}", author_name), Style::default().fg(theme::ACCENT_SPECIAL)),
        ];

        // Add nested conversations indicator
        if has_children && child_count > 0 {
            line2_spans.push(Span::styled(
                format!("{}{} nested", card::SPACER, child_count),
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }

        // Line 3: Avatar continuation + Preview
        let line3_spans = vec![
            Span::styled(card::SPACER, avatar_plain_style),  // Avatar row 3 (solid color)
            Span::styled(indent.clone(), Style::default()),
            Span::styled(card::SPACER, Style::default()),  // Space for collapse indicator
            Span::styled(border_char, border_style),
            Span::styled(
                truncate_with_ellipsis(&preview, 70),
                Style::default().fg(theme::TEXT_MUTED),
            ),
        ];

        // Build lines list
        let mut lines = vec![
            Line::from(line1_spans),
            Line::from(line2_spans),
            Line::from(line3_spans),
        ];

        // Line 4: Current activity (if present) OR spacing
        if let Some(ref activity) = thread.status_current_activity {
            let activity_spans = vec![
                Span::styled(card::SPACER, avatar_plain_style),  // Avatar row 4 (solid color)
                Span::styled(indent.clone(), Style::default()),
                Span::styled(card::SPACER, Style::default()),  // Space for collapse indicator
                Span::styled(border_char, border_style),
                Span::styled(card::ACTIVITY_GLYPH, Style::default().fg(theme::ACCENT_PRIMARY)),
                Span::styled(
                    truncate_with_ellipsis(activity, 70),
                    Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM),
                ),
            ];
            lines.push(Line::from(activity_spans));

            // Final line: Avatar bottom + Spacing line with border
            lines.push(Line::from(vec![
                Span::styled(card::SPACER, Style::default()),  // No avatar on line 5
                Span::styled(indent.clone(), Style::default()),
                Span::styled(card::SPACER, Style::default()),  // Space for collapse indicator
                Span::styled(border_char, border_style),
            ]));
        } else {
            // Final line: Avatar bottom + Spacing line with border
            lines.push(Line::from(vec![
                Span::styled(card::SPACER, avatar_plain_style),  // Avatar row 4 (solid color)
                Span::styled(indent.clone(), Style::default()),
                Span::styled(card::SPACER, Style::default()),  // Space for collapse indicator
                Span::styled(border_char, border_style),
            ]));
        }

        let item = ListItem::new(lines);
        if is_selected {
            item.style(Style::default().bg(theme::BG_SELECTED))
        } else {
            item
        }
    }
}

/// Render card content
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

    // Extract data
    let (project_name, author_name, preview, timestamp) = {
        let store = app.data_store.borrow();
        let project_name = store.get_project_name(a_tag);
        let messages = store.get_messages(&thread.id);
        let (author_name, preview, timestamp) = if let Some(msg) = messages.last() {
            let name = store.get_profile_name(&msg.pubkey);
            let preview: String = msg.content.chars().take(80).collect();
            let preview = preview.replace('\n', " ");
            (name, preview, msg.created_at)
        } else {
            ("".to_string(), "No messages yet".to_string(), thread.last_activity)
        };
        (project_name, author_name, preview, timestamp)
    };

    let time_str = format_relative_time(timestamp);
    let project_indicator_color = theme::project_color(a_tag);
    let border_char = card::BORDER_GLYPH;
    let border_style = Style::default().fg(project_indicator_color);

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

    let mut lines: Vec<Line> = Vec::new();

    if is_compact {
        // COMPACT: single content line + spacing
        let mut line1 = vec![
            Span::styled(indent.clone(), Style::default()),
            Span::styled(collapse_indicator, Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(border_char, border_style),
        ];
        if let Some(ref status_label) = thread.status_label {
            line1.push(Span::styled(format!("{} ", status_label_to_symbol(status_label)), Style::default().fg(theme::ACCENT_WARNING)));
        }
        line1.push(Span::styled(truncate_with_ellipsis(&thread.title, 40), title_style));
        if is_collapsed && child_count > 0 {
            line1.push(Span::styled(
                format!("{}+{}", card::SPACER, child_count),
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
        line1.push(Span::styled(card::SPACER, Style::default()));
        line1.push(Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)));
        lines.push(Line::from(line1));

        // Spacing line
        lines.push(Line::from(vec![
            Span::styled(indent, Style::default()),
            Span::styled(card::SPACER, Style::default()),
            Span::styled(border_char, border_style),
        ]));
    } else {
        // FULL MODE: 4-5 lines
        // Line 1: Title + time
        let mut line1 = vec![Span::styled(indent.clone(), Style::default())];
        if !collapse_indicator.is_empty() {
            line1.push(Span::styled(collapse_indicator, Style::default().fg(theme::TEXT_MUTED)));
        }
        line1.push(Span::styled(border_char, border_style));
        if let Some(ref status_label) = thread.status_label {
            line1.push(Span::styled(format!("[{} {}] ", status_label_to_symbol(status_label), status_label), Style::default().fg(theme::ACCENT_WARNING)));
        }
        line1.push(Span::styled(truncate_with_ellipsis(&thread.title, 60), title_style));
        line1.push(Span::styled(card::SPACER, Style::default()));
        line1.push(Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)));
        lines.push(Line::from(line1));

        // Line 2: Project + author
        let mut line2 = vec![
            Span::styled(indent.clone(), Style::default()),
            Span::styled(card::SPACER, Style::default()),
            Span::styled(border_char, border_style),
            Span::styled(card::BULLET, Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(project_name, Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(card::SPACER, Style::default()),
            Span::styled(format!("@{}", author_name), Style::default().fg(theme::ACCENT_SPECIAL)),
        ];
        if has_children && child_count > 0 {
            line2.push(Span::styled(
                format!("{}{} nested", card::SPACER, child_count),
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
        lines.push(Line::from(line2));

        // Line 3: Preview
        lines.push(Line::from(vec![
            Span::styled(indent.clone(), Style::default()),
            Span::styled(card::SPACER, Style::default()),
            Span::styled(border_char, border_style),
            Span::styled(truncate_with_ellipsis(&preview, 70), Style::default().fg(theme::TEXT_MUTED)),
        ]));

        // Line 4: Activity or spacing
        if let Some(ref activity) = thread.status_current_activity {
            lines.push(Line::from(vec![
                Span::styled(indent.clone(), Style::default()),
                Span::styled(card::SPACER, Style::default()),
                Span::styled(border_char, border_style),
                Span::styled(card::ACTIVITY_GLYPH, Style::default().fg(theme::ACCENT_PRIMARY)),
                Span::styled(truncate_with_ellipsis(activity, 70), Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)),
            ]));
            // Line 5: spacing
            lines.push(Line::from(vec![
                Span::styled(indent, Style::default()),
                Span::styled(card::SPACER, Style::default()),
                Span::styled(border_char, border_style),
            ]));
        } else {
            // Line 4: spacing
            lines.push(Line::from(vec![
                Span::styled(indent, Style::default()),
                Span::styled(card::SPACER, Style::default()),
                Span::styled(border_char, border_style),
            ]));
        }
    }

    let mut style = Style::default();
    if is_selected {
        style = style.bg(theme::BG_SELECTED);
    }

    let paragraph = Paragraph::new(lines).style(style);
    f.render_widget(paragraph, area);
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

    // Card styling - left border color is deterministic based on project a_tag
    let project_indicator_color = theme::project_color(&item.project_a_tag);
    let border_char = card::BORDER_GLYPH;
    let border_style = Style::default().fg(project_indicator_color);

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
        Span::styled(border_char, border_style),
        unread_indicator,
        Span::styled(truncate_with_ellipsis(&item.title, 55), title_style),
        Span::styled(card::SPACER, Style::default()),
        Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)),
    ];

    // Line 2: Type + Project + Author
    let line2_spans = vec![
        Span::styled(border_char, border_style),
        Span::styled(type_str, Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" in ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(project_name, Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled(" by ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(author_name, Style::default().fg(theme::ACCENT_SPECIAL)),
    ];

    // Line 3: Spacing line with border
    let line3_spans = vec![Span::styled(border_char, border_style)];

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
    let time_indicator = if app.time_filter.is_some() { " ✓" } else { "" };
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

fn render_new_thread_modal(f: &mut Frame, app: &App, area: Rect) {
    let popup_width = 80.min(area.width.saturating_sub(4));
    let popup_height = (area.height as f32 * 0.8) as u16;
    let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    f.render_widget(Clear, popup_area);

    let modal_chunks = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Min(5),
        Constraint::Length(1),
    ])
    .split(popup_area);

    let title = Paragraph::new(" New Thread")
        .style(Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
        );
    f.render_widget(title, modal_chunks[0]);

    render_new_thread_project_field(f, app, modal_chunks[1]);
    render_new_thread_agent_field(f, app, modal_chunks[2]);
    render_new_thread_content_field(f, app, modal_chunks[3]);

    let can_submit = app.can_submit_new_thread();
    let submit_hint = if can_submit { "Enter send" } else { "" };
    let hints = format!("Tab next field · {} · Esc cancel", submit_hint);
    let hints = Paragraph::new(hints)
        .style(Style::default().fg(theme::TEXT_MUTED))
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
        );
    f.render_widget(hints, modal_chunks[4]);
}

fn render_new_thread_project_field(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.new_thread_modal_focus == NewThreadField::Project;
    let border_color = if is_focused { theme::ACCENT_WARNING } else { theme::TEXT_MUTED };
    let projects = app.new_thread_filtered_projects();

    let display_text = if is_focused {
        if app.new_thread_project_filter.is_empty() {
            " Type to filter...".to_string()
        } else {
            format!(" {}", app.new_thread_project_filter)
        }
    } else {
        app.new_thread_selected_project
            .as_ref()
            .map(|p| format!(" {}{}", card::BULLET, p.name))
            .unwrap_or_else(|| " Select project...".to_string())
    };

    let text_style = if is_focused {
        if app.new_thread_project_filter.is_empty() {
            Style::default().fg(theme::TEXT_MUTED)
        } else {
            Style::default().fg(theme::ACCENT_WARNING)
        }
    } else if app.new_thread_selected_project.is_some() {
        Style::default().fg(theme::ACCENT_SUCCESS)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let project_field = Paragraph::new(display_text)
        .style(text_style)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(" Project ", Style::default().fg(border_color))),
        );
    f.render_widget(project_field, area);

    if is_focused && !projects.is_empty() {
        let dropdown_height = (projects.len() as u16 + 2).min(8);
        let dropdown_area = Rect::new(
            area.x + 1,
            area.y + area.height,
            area.width.saturating_sub(2),
            dropdown_height,
        );

        if dropdown_area.y + dropdown_area.height < f.area().height {
            f.render_widget(Clear, dropdown_area);

            let items: Vec<ListItem> = projects
                .iter()
                .enumerate()
                .map(|(i, p)| {
                    let is_selected = i == app.new_thread_project_index;
                    let style = if is_selected {
                        Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::TEXT_PRIMARY)
                    };
                    let prefix = if is_selected { card::COLLAPSE_CLOSED } else { card::SPACER };
                    ListItem::new(Line::from(Span::styled(format!("{}{}", prefix, p.name), style)))
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT_WARNING)),
            );
            f.render_widget(list, dropdown_area);
        }
    }
}

fn render_new_thread_agent_field(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.new_thread_modal_focus == NewThreadField::Agent;
    let border_color = if is_focused { theme::ACCENT_WARNING } else { theme::TEXT_MUTED };
    let agents = app.new_thread_filtered_agents();

    let display_text = if is_focused {
        if app.new_thread_agent_filter.is_empty() {
            " Type to filter...".to_string()
        } else {
            format!(" {}", app.new_thread_agent_filter)
        }
    } else {
        app.new_thread_selected_agent
            .as_ref()
            .map(|a| format!(" @{}", a.name))
            .unwrap_or_else(|| " Select agent...".to_string())
    };

    let text_style = if is_focused {
        if app.new_thread_agent_filter.is_empty() {
            Style::default().fg(theme::TEXT_MUTED)
        } else {
            Style::default().fg(theme::ACCENT_WARNING)
        }
    } else if app.new_thread_selected_agent.is_some() {
        Style::default().fg(theme::ACCENT_SPECIAL)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let agent_field = Paragraph::new(display_text)
        .style(text_style)
        .block(
            Block::default()
                .borders(Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(border_color))
                .title(Span::styled(" Agent ", Style::default().fg(border_color))),
        );
    f.render_widget(agent_field, area);

    if is_focused && !agents.is_empty() {
        let dropdown_height = (agents.len() as u16 + 2).min(8);
        let dropdown_area = Rect::new(
            area.x + 1,
            area.y + area.height,
            area.width.saturating_sub(2),
            dropdown_height,
        );

        if dropdown_area.y + dropdown_area.height < f.area().height {
            f.render_widget(Clear, dropdown_area);

            let items: Vec<ListItem> = agents
                .iter()
                .enumerate()
                .map(|(i, a)| {
                    let is_selected = i == app.new_thread_agent_index;
                    let style = if is_selected {
                        Style::default().fg(theme::ACCENT_SPECIAL).add_modifier(Modifier::BOLD)
                    } else {
                        Style::default().fg(theme::TEXT_PRIMARY)
                    };
                    let prefix = if is_selected { card::COLLAPSE_CLOSED } else { card::SPACER };
                    ListItem::new(Line::from(Span::styled(format!("{}@{}", prefix, a.name), style)))
                })
                .collect();

            let list = List::new(items).block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT_WARNING)),
            );
            f.render_widget(list, dropdown_area);
        }
    }
}

fn render_new_thread_content_field(f: &mut Frame, app: &App, area: Rect) {
    let is_focused = app.new_thread_modal_focus == NewThreadField::Content;
    let border_color = if is_focused { theme::ACCENT_PRIMARY } else { theme::TEXT_MUTED };

    let content = &app.new_thread_editor.text;
    let display_text = if content.is_empty() && !is_focused {
        " Enter your message..."
    } else if content.is_empty() {
        ""
    } else {
        content
    };

    let text_style = if content.is_empty() {
        Style::default().fg(theme::TEXT_MUTED)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    // Account for 1 char padding on left
    let inner_width = area.width.saturating_sub(4) as usize;
    let lines: Vec<Line> = display_text
        .lines()
        .flat_map(|line| {
            if line.is_empty() {
                vec![Line::from(" ")]
            } else {
                line.chars()
                    .collect::<Vec<_>>()
                    .chunks(inner_width.max(1))
                    .map(|chunk| {
                        Line::from(Span::styled(
                            format!(" {}", chunk.iter().collect::<String>()),
                            text_style,
                        ))
                    })
                    .collect::<Vec<_>>()
            }
        })
        .collect();

    let content_field = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT | Borders::TOP)
            .border_style(Style::default().fg(border_color))
            .title(Span::styled(" Message ", Style::default().fg(border_color))),
    );
    f.render_widget(content_field, area);

    if is_focused {
        // +2 for border and padding
        let cursor_x = area.x + 2 + (app.new_thread_editor.cursor % inner_width.max(1)) as u16;
        let cursor_y = area.y + 1 + (app.new_thread_editor.cursor / inner_width.max(1)) as u16;
        if cursor_y < area.y + area.height - 1 {
            f.set_cursor_position((cursor_x, cursor_y));
        }
    }
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
