use crate::models::{InboxEventType, InboxItem, Thread};
use crate::ui::views::chat::render_tab_bar;
use crate::ui::{theme, App, HomeTab, NewThreadField};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph},
    Frame,
};
use std::time::{SystemTime, UNIX_EPOCH};

/// Map status label to Unicode symbol
fn status_label_to_symbol(label: &str) -> &'static str {
    match label.to_lowercase().as_str() {
        "in progress" | "in-progress" | "working" | "active" => "🔧",
        "blocked" | "waiting" | "paused" => "🚧",
        "done" | "complete" | "completed" | "finished" => "✅",
        "reviewing" | "review" | "in review" => "👀",
        "testing" | "in testing" => "🧪",
        "planning" | "draft" | "design" => "📝",
        "urgent" | "critical" | "high priority" => "🔥",
        "bug" | "issue" | "error" => "🐛",
        "enhancement" | "feature" | "new" => "✨",
        "question" | "help needed" => "❓",
        _ => "📌",
    }
}

/// Format a timestamp as relative time (e.g., "2m ago", "1h ago")
fn format_relative_time(timestamp: u64) -> String {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let diff = now.saturating_sub(timestamp);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else {
        format!("{}w ago", diff / 604800)
    }
}

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
        Constraint::Length(20), // Sidebar (fixed width, on RIGHT)
    ])
    .split(chunks[1]);

    // Render content based on active tab (with left padding)
    let content_area = main_chunks[0];
    let padded_content = Rect::new(
        content_area.x + 2, // 2-char left padding
        content_area.y,
        content_area.width.saturating_sub(2),
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
    if app.showing_projects_modal {
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

    let items: Vec<ListItem> = recent
        .iter()
        .enumerate()
        .map(|(i, (thread, a_tag))| {
            let is_selected = is_focused && i == app.selected_recent_index;
            render_conversation_card(app, thread, a_tag, is_selected)
        })
        .collect();

    // No block/border - just the list directly
    let list = List::new(items).highlight_style(Style::default());

    let mut state = ListState::default();
    state.select(Some(app.selected_recent_index));
    f.render_stateful_widget(list, area, &mut state);
}

fn render_conversation_card(
    app: &App,
    thread: &Thread,
    a_tag: &str,
    is_selected: bool,
) -> ListItem<'static> {
    // Single borrow to extract all needed data
    let (project_name, author_name, preview, timestamp) = {
        let store = app.data_store.borrow();
        let project_name = store.get_project_name(a_tag);

        // Get last message info without cloning the entire vector
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

    // Card styling - left border color is deterministic based on project a_tag
    let project_indicator_color = theme::project_color(a_tag);
    let border_char = "│ ";
    let border_style = Style::default().fg(project_indicator_color);

    let title_style = if is_selected {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    // Line 1: Status label (if present) + Title + time
    let mut line1_spans = vec![Span::styled(border_char, border_style)];

    // Add status label with symbol if present
    if let Some(ref status_label) = thread.status_label {
        let symbol = status_label_to_symbol(status_label);
        line1_spans.push(Span::styled(
            format!("[{} {}] ", symbol, status_label),
            Style::default().fg(theme::ACCENT_WARNING),
        ));
    }

    line1_spans.push(Span::styled(
        truncate_string(&thread.title, 60),
        title_style,
    ));

    // Add time on the right (we'll pad later in rendering)
    let time_padding = "  ";
    line1_spans.push(Span::styled(time_padding, Style::default()));
    line1_spans.push(Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)));

    // Line 2: Project + agent
    let line2_spans = vec![
        Span::styled(border_char, border_style),
        Span::styled("● ", Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled(project_name, Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled("  ", Style::default()),
        Span::styled(format!("@{}", author_name), Style::default().fg(theme::ACCENT_SPECIAL)),
    ];

    // Line 3: Preview
    let mut line3_spans = vec![
        Span::styled(border_char, border_style),
        Span::styled(
            truncate_string(&preview, 70),
            Style::default().fg(theme::TEXT_MUTED),
        ),
    ];

    // Build lines list
    let mut lines = vec![
        Line::from(line1_spans),
        Line::from(line2_spans),
        Line::from(line3_spans),
    ];

    // Line 4: Current activity (if present)
    if let Some(ref activity) = thread.status_current_activity {
        let activity_spans = vec![
            Span::styled(border_char, border_style),
            Span::styled("⟳ ", Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled(
                truncate_string(activity, 70),
                Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM),
            ),
        ];
        lines.push(Line::from(activity_spans));
    }

    // Final line: Empty line for spacing
    lines.push(Line::from(vec![Span::raw("")]));

    let item = ListItem::new(lines);
    if is_selected {
        item.style(Style::default().bg(theme::BG_SELECTED))
    } else {
        item
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

    // Card styling - left border color is deterministic based on project a_tag
    let project_indicator_color = theme::project_color(&item.project_a_tag);
    let border_char = "│ ";
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
        Span::styled("● ", Style::default().fg(theme::ACCENT_PRIMARY))
    } else {
        Span::styled("  ", Style::default())
    };

    // Line 1: Title + time
    let line1_spans = vec![
        Span::styled(border_char, border_style),
        unread_indicator,
        Span::styled(truncate_string(&item.title, 55), title_style),
        Span::styled("  ", Style::default()),
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

    // Line 3: Empty for spacing
    let line3_spans = vec![Span::raw("")];

    ListItem::new(vec![
        Line::from(line1_spans),
        Line::from(line2_spans),
        Line::from(line3_spans),
    ])
}

/// Render the project sidebar with checkboxes for filtering
fn render_project_sidebar(f: &mut Frame, app: &App, area: Rect) {
    let (online_projects, offline_projects) = app.filtered_projects();

    let mut items: Vec<ListItem> = Vec::new();

    // Online section header
    if !online_projects.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("● ", Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled("Online", Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)),
        ])));
    }

    // Online projects
    for project in &online_projects {
        let a_tag = project.a_tag();
        let is_visible = app.visible_projects.is_empty() || app.visible_projects.contains(&a_tag);
        let checkbox = if is_visible { "[✓] " } else { "[ ] " };
        let name = truncate_string(&project.name, 14);
        items.push(ListItem::new(Line::from(vec![
            Span::styled(checkbox, Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::raw(name),
        ])));
    }

    // Offline section header
    if !offline_projects.is_empty() {
        items.push(ListItem::new(Line::from(vec![
            Span::styled("○ ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Offline", Style::default().fg(theme::TEXT_MUTED)),
        ])));
    }

    // Offline projects
    for project in &offline_projects {
        let a_tag = project.a_tag();
        let is_visible = app.visible_projects.is_empty() || app.visible_projects.contains(&a_tag);
        let checkbox = if is_visible { "[✓] " } else { "[ ] " };
        let name = truncate_string(&project.name, 14);
        items.push(ListItem::new(Line::from(vec![
            Span::styled(checkbox, Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(name, Style::default().fg(theme::TEXT_MUTED)),
        ])));
    }

    let list = List::new(items)
        .block(Block::default()
            .borders(Borders::NONE)
            .padding(Padding::new(2, 2, 1, 1))) // left, right, top, bottom
        .style(Style::default().bg(theme::BG_SIDEBAR));

    // Calculate selected index (accounting for headers)
    let selected_index = if app.sidebar_focused {
        let mut idx = app.sidebar_project_index;
        // Add 1 for online header if we have online projects
        if !online_projects.is_empty() {
            idx += 1;
        }
        // If we're past online projects, add 1 for offline header
        if idx > online_projects.len() && !offline_projects.is_empty() {
            idx += 1;
        }
        Some(idx)
    } else {
        None
    };

    let mut state = ListState::default();
    state.select(selected_index);

    f.render_stateful_widget(list, area, &mut state);
}

fn render_help_bar(f: &mut Frame, app: &App, area: Rect) {
    let hints = if app.sidebar_focused {
        "← back · ↑↓ navigate · Space toggle · Tab switch · q quit"
    } else {
        match app.home_panel_focus {
            HomeTab::Recent => "→ projects · ↑↓ navigate · Enter open · n new · Tab switch · q quit",
            HomeTab::Inbox => "→ projects · ↑↓ navigate · Enter open · r mark read · Tab switch · q quit",
        }
    };

    let help = Paragraph::new(hints).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(help, area);
}

fn truncate_string(s: &str, max_len: usize) -> String {
    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        format!("{}...", s.chars().take(max_len - 3).collect::<String>())
    }
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
    // Center the modal
    let popup_width = 60.min(area.width.saturating_sub(4));
    let popup_height = (area.height as f32 * 0.7) as u16;
    let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the background
    f.render_widget(Clear, popup_area);

    // Modal layout: title + filter + content + hints
    let modal_chunks = Layout::vertical([
        Constraint::Length(1), // Title
        Constraint::Length(3), // Filter input
        Constraint::Min(0),    // Content
        Constraint::Length(1), // Hints
    ])
    .split(popup_area);

    // Title bar
    let title = Paragraph::new("Switch Project")
        .style(Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
        );
    f.render_widget(title, modal_chunks[0]);

    // Filter input
    let filter_style = if app.project_filter.is_empty() {
        Style::default().fg(theme::TEXT_MUTED)
    } else {
        Style::default().fg(theme::ACCENT_WARNING)
    };

    let filter_text = if app.project_filter.is_empty() {
        "Type to filter projects..."
    } else {
        &app.project_filter
    };

    let filter_input = Paragraph::new(filter_text).style(filter_style).block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
    );
    f.render_widget(filter_input, modal_chunks[1]);

    // Render the project list
    let data_store = app.data_store.borrow();
    let (online_projects, offline_projects) = app.filtered_projects();

    let mut items: Vec<ListItem> = Vec::new();

    // Online projects section
    if !online_projects.is_empty() {
        items.push(ListItem::new(Line::from(Span::styled(
            format!("● ONLINE ({})", online_projects.len()),
            Style::default()
                .fg(theme::ACCENT_SUCCESS)
                .add_modifier(Modifier::BOLD),
        ))));

        for (idx, project) in online_projects.iter().enumerate() {
            let is_selected = idx == app.selected_project_index;
            let prefix = if is_selected { "  ▶ " } else { "    " };

            let owner_name = data_store.get_profile_name(&project.pubkey);
            let agent_count = data_store
                .get_project_status(&project.a_tag())
                .map(|s| s.agents.len())
                .unwrap_or(0);

            let style = if is_selected {
                Style::default()
                    .fg(theme::ACCENT_SUCCESS)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT_PRIMARY)
            };

            let content = vec![
                Line::from(Span::styled(format!("{}{}", prefix, project.name), style)),
                Line::from(Span::styled(
                    format!("      {} agent(s) · {}", agent_count, owner_name),
                    Style::default().fg(theme::TEXT_MUTED),
                )),
            ];

            items.push(ListItem::new(content));
        }
    }

    // Offline projects section (always shown)
    if !offline_projects.is_empty() {
        if !online_projects.is_empty() {
            items.push(ListItem::new(Line::from("")));
        }

        items.push(ListItem::new(Line::from(Span::styled(
            format!("○ OFFLINE ({})", offline_projects.len()),
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        ))));

        for (idx, project) in offline_projects.iter().enumerate() {
            let offset = online_projects.len();
            let is_selected = offset + idx == app.selected_project_index;
            let prefix = if is_selected { "  ▶ " } else { "    " };

            let owner_name = data_store.get_profile_name(&project.pubkey);

            let style = if is_selected {
                Style::default()
                    .fg(theme::ACCENT_WARNING)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            };

            let content = vec![
                Line::from(Span::styled(format!("{}{}", prefix, project.name), style)),
                Line::from(Span::styled(
                    format!("      {}", owner_name),
                    Style::default().fg(theme::TEXT_MUTED),
                )),
            ];

            items.push(ListItem::new(content));
        }
    }
    drop(data_store);

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
    );
    f.render_widget(list, modal_chunks[2]);

    // Hints
    let hints = Paragraph::new("↑↓ navigate · Enter select · Tab expand · Esc close")
        .style(Style::default().fg(theme::TEXT_MUTED))
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
        );
    f.render_widget(hints, modal_chunks[3]);
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
            .map(|p| format!(" ● {}", p.name))
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
                    let prefix = if is_selected { "▶ " } else { "  " };
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
                    let prefix = if is_selected { "▶ " } else { "  " };
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
    // Calculate modal dimensions
    let tab_count = app.open_tabs.len();
    let popup_width = 60.min(area.width.saturating_sub(4));
    // +3 for header, footer, and dashboard row
    let popup_height = (tab_count as u16 + 4).min(area.height.saturating_sub(4)).max(5);
    let popup_x = area.x + (area.width.saturating_sub(popup_width)) / 2;
    let popup_y = area.y + (area.height.saturating_sub(popup_height)) / 2;
    let popup_area = Rect::new(popup_x, popup_y, popup_width, popup_height);

    // Clear the background
    f.render_widget(Clear, popup_area);

    // Modal layout: header + content + hints
    let modal_chunks = Layout::vertical([
        Constraint::Length(1), // Title
        Constraint::Min(0),    // Content
        Constraint::Length(1), // Hints
    ])
    .split(popup_area);

    // Title bar
    let title = Paragraph::new(" Open Tabs")
        .style(Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD))
        .block(
            Block::default()
                .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
        );
    f.render_widget(title, modal_chunks[0]);

    // Build tab list
    let data_store = app.data_store.borrow();
    let mut items: Vec<ListItem> = Vec::new();

    // Dashboard row (always first)
    let dashboard_style = Style::default().fg(theme::TEXT_PRIMARY);
    items.push(ListItem::new(Line::from(vec![
        Span::styled("  0. ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Dashboard", dashboard_style),
    ])));

    // Tab rows
    for (i, tab) in app.open_tabs.iter().enumerate() {
        let is_selected = i == app.tab_modal_index;
        let is_active = i == app.active_tab_index;

        // Get project name
        let project_name = data_store.get_project_name(&tab.project_a_tag);

        // Build the line
        let prefix = if is_selected { ">" } else { " " };
        let active_marker = if is_active { "*" } else { " " };

        let line_style = if is_selected {
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
        } else if tab.has_unread {
            Style::default().fg(theme::ACCENT_WARNING)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        let title_display = truncate_string(&tab.thread_title, 35);

        items.push(ListItem::new(Line::from(vec![
            Span::styled(format!("{}{} {}. ", prefix, active_marker, i + 1),
                Style::default().fg(if is_selected { theme::ACCENT_PRIMARY } else { theme::TEXT_MUTED })),
            Span::styled(format!("{} ", project_name),
                Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled("| ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(title_display, line_style),
            if tab.has_unread {
                Span::styled(" [unread]", Style::default().fg(theme::ACCENT_WARNING))
            } else {
                Span::raw("")
            },
        ])));
    }
    drop(data_store);

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
    );
    f.render_widget(list, modal_chunks[1]);

    // Hints
    let hints = Paragraph::new("Up/Down select | Enter switch | x close tab | 0-9 jump | Esc cancel")
        .style(Style::default().fg(theme::TEXT_MUTED))
        .block(
            Block::default()
                .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY)),
        );
    f.render_widget(hints, modal_chunks[2]);
}
