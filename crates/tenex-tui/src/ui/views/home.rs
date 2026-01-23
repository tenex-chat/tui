use crate::models::{InboxEventType, InboxItem, Thread};
use crate::ui::components::{
    render_modal_items, render_modal_sections, render_tab_bar, Modal, ModalItem,
    ModalSection, ModalSize,
};
use crate::ui::card;
use crate::ui::modal::{ConversationAction, ConversationActionsState, ModalState, ProjectActionsState};
use crate::ui::format::{format_relative_time, status_label_to_symbol, truncate_with_ellipsis};
use crate::ui::views::home_helpers::build_thread_hierarchy;
pub use crate::ui::views::home_helpers::HierarchicalThread;
use crate::ui::{layout, theme, App, HomeTab, View};
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

    let has_tabs = !app.open_tabs().is_empty();

    // Layout: Header tabs | Main area | Bottom padding | Optional tab bar
    let chunks = if has_tabs {
        Layout::vertical([
            Constraint::Length(2), // Tab header
            Constraint::Min(0),    // Main area (sidebar + content)
            Constraint::Length(1), // Bottom padding
            Constraint::Length(layout::TAB_BAR_HEIGHT), // Open tabs bar
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Length(2), // Tab header
            Constraint::Min(0),    // Main area (sidebar + content)
            Constraint::Length(1), // Bottom padding
        ])
        .split(area)
    };

    // Render tab header
    render_tab_header(f, app, chunks[0]);

    // Split main area into content and sidebar (sidebar on RIGHT)
    let main_chunks = Layout::horizontal([
        Constraint::Min(0),                            // Content
        Constraint::Length(layout::SIDEBAR_WIDTH_HOME), // Sidebar (fixed width, on RIGHT)
    ])
    .split(chunks[1]);

    // Render content based on active tab (with consistent padding)
    let content_area = main_chunks[0];
    let padded_content = layout::with_content_padding(content_area);
    match app.home_panel_focus {
        HomeTab::Conversations => render_conversations_with_feed(f, app, padded_content),
        HomeTab::Inbox => render_inbox_cards(f, app, padded_content),
        HomeTab::Reports => render_reports_list(f, app, padded_content),
        HomeTab::Status => render_status_list(f, app, padded_content),
        HomeTab::Search => render_search_tab(f, app, padded_content),
        HomeTab::Feed => render_feed_cards(f, app, padded_content),
    }

    // Render sidebar on the right
    render_project_sidebar(f, app, main_chunks[1]);

    // Bottom padding
    render_bottom_padding(f, chunks[2]);

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

    // Create project modal overlay
    if let ModalState::CreateProject(ref state) = app.modal_state {
        super::render_create_project(f, app, area, state);
    }

    // Create agent modal overlay
    if let ModalState::CreateAgent(ref state) = app.modal_state {
        super::render_create_agent(f, area, state);
    }

    // Project actions modal overlay
    if let ModalState::ProjectActions(ref state) = app.modal_state {
        render_project_actions_modal(f, area, state);
    }

    // Conversation actions modal overlay
    if let ModalState::ConversationActions(ref state) = app.modal_state {
        render_conversation_actions_modal(f, area, state);
    }

    // Report viewer modal overlay
    if let ModalState::ReportViewer(ref state) = app.modal_state {
        super::render_report_viewer(f, app, area, state);
    }

    // Tab modal overlay (Alt+/)
    if app.showing_tab_modal() {
        render_tab_modal(f, app, area);
    }

    // Search modal overlay (/)
    if app.showing_search_modal {
        render_search_modal(f, app, area);
    }

    // Command palette overlay (Ctrl+T)
    if let ModalState::CommandPalette(ref state) = app.modal_state {
        super::render_command_palette(f, area, app, state.selected_index);
    }

    // Backend approval modal
    if let ModalState::BackendApproval(ref state) = app.modal_state {
        super::render_backend_approval_modal(f, area, state);
    }

    // Debug stats modal (Ctrl+T D)
    if let ModalState::DebugStats(ref state) = app.modal_state {
        super::render_debug_stats(f, area, app, state);
    }
}

fn render_tab_header(f: &mut Frame, app: &App, area: Rect) {
    let inbox_count = app.inbox_items().iter().filter(|i| !i.is_read).count();
    let status_count = app.status_threads().iter().filter(|(t, _)| t.status_current_activity.is_some()).count();

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
        Span::styled("Conversations", tab_style(HomeTab::Conversations)),
        Span::styled("   ", Style::default()),
        Span::styled("Inbox", tab_style(HomeTab::Inbox)),
    ];

    if inbox_count > 0 {
        spans.push(Span::styled(
            format!(" ({})", inbox_count),
            Style::default().fg(theme::ACCENT_ERROR),
        ));
    }

    spans.push(Span::styled("   ", Style::default()));
    spans.push(Span::styled("Reports", tab_style(HomeTab::Reports)));
    spans.push(Span::styled("   ", Style::default()));
    spans.push(Span::styled("Status", tab_style(HomeTab::Status)));

    if status_count > 0 {
        spans.push(Span::styled(
            format!(" ({})", status_count),
            Style::default().fg(theme::ACCENT_SUCCESS),
        ));
    }

    spans.push(Span::styled("   ", Style::default()));
    spans.push(Span::styled("Search", tab_style(HomeTab::Search)));
    spans.push(Span::styled("   ", Style::default()));
    spans.push(Span::styled("Feed", tab_style(HomeTab::Feed)));

    // Show archived mode indicator
    if app.show_archived {
        spans.push(Span::styled("   ", Style::default()));
        spans.push(Span::styled("[showing archived]", Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)));
    }

    // Show scheduled filter indicator
    if app.hide_scheduled {
        spans.push(Span::styled("   ", Style::default()));
        spans.push(Span::styled("[hiding scheduled]", Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)));
    }

    let header_line = Line::from(spans);

    // Second line: tab indicator underline
    let accent = Style::default().fg(theme::ACCENT_PRIMARY);
    let blank = Style::default();

    let indicator_spans = vec![
        Span::styled("         ", blank), // Padding for "  TENEX  "
        Span::styled(if app.home_panel_focus == HomeTab::Conversations { "─────────────" } else { "             " },
            if app.home_panel_focus == HomeTab::Conversations { accent } else { blank }),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Inbox { "─────" } else { "     " },
            if app.home_panel_focus == HomeTab::Inbox { accent } else { blank }),
        Span::styled(if inbox_count > 0 { "    " } else { "" }, blank),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Reports { "───────" } else { "       " },
            if app.home_panel_focus == HomeTab::Reports { accent } else { blank }),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Status { "──────" } else { "      " },
            if app.home_panel_focus == HomeTab::Status { accent } else { blank }),
        Span::styled(if status_count > 0 { "    " } else { "" }, blank),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Search { "──────" } else { "      " },
            if app.home_panel_focus == HomeTab::Search { accent } else { blank }),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Feed { "────" } else { "    " },
            if app.home_panel_focus == HomeTab::Feed { accent } else { blank }),
    ];
    let indicator_line = Line::from(indicator_spans);

    let header = Paragraph::new(vec![header_line, indicator_line]);
    f.render_widget(header, area);
}

fn render_conversations_with_feed(f: &mut Frame, app: &App, area: Rect) {
    render_conversations_cards(f, app, area, true);
}

fn render_conversations_cards(f: &mut Frame, app: &App, area: Rect, is_focused: bool) {
    let recent = app.recent_threads();

    if recent.is_empty() {
        let empty = Paragraph::new("No recent conversations")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty, area);
        return;
    }

    // Get q-tag relationships for fallback parent-child detection
    let q_tag_relationships = app.data_store.borrow().get_q_tag_relationships();

    // Build hierarchical thread list (with default collapsed state from preferences)
    let default_collapsed = app.threads_default_collapsed();
    let hierarchy = build_thread_hierarchy(&recent, &app.collapsed_threads, &q_tag_relationships, default_collapsed);

    // Helper to calculate card height
    // Full mode: 4 lines (title, summary, activity, reply) + spacing, but some may be hidden
    // Compact mode: 2 lines (title, spacing)
    // Selected items add 2 lines for half-block borders (top + bottom)
    // next_is_selected: if true, this card doesn't need spacing (next card's top border provides it)
    let calc_card_height = |item: &HierarchicalThread, is_selected: bool, next_is_selected: bool| -> u16 {
        let is_compact = item.depth > 0;
        if is_compact {
            return if is_selected { 4 } else { 2 }; // 2 lines + optional borders
        }
        // Full mode: title + summary + activity (if present) + reply + spacing
        let has_summary = item.thread.summary.is_some();
        let has_activity = item.thread.status_current_activity.is_some();
        // Line 1: title, Line 2: summary (or skip), Line 3: activity (or skip), Line 4: reply, Line 5: spacing
        let mut lines = 1; // title always
        if has_summary { lines += 1; } // summary
        if has_activity { lines += 1; } // activity
        lines += 1; // reply preview always
        if !is_selected && !next_is_selected {
            lines += 1; // spacing (only when neither this nor next card is selected)
        }
        if is_selected { lines + 2 } else { lines }
    };

    // Calculate scroll offset to keep selected item visible
    let selected_idx = app.current_selection();
    let mut scroll_offset: u16 = 0;

    // Calculate cumulative height up to and including selected item
    let mut height_before_selected: u16 = 0;
    let mut selected_height: u16 = 0;
    for (i, item) in hierarchy.iter().enumerate() {
        let item_is_selected = is_focused && i == selected_idx;
        let next_is_selected = is_focused && i + 1 == selected_idx;
        let h = calc_card_height(item, item_is_selected, next_is_selected);
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
        let next_is_selected = is_focused && i + 1 == selected_idx;
        let h = calc_card_height(item, is_selected, next_is_selected);

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
            let is_multi_selected = app.is_thread_multi_selected(&item.thread.id);

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
    build_thread_hierarchy(&recent, &app.collapsed_threads, &q_tag_relationships, default_collapsed)
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
    let indent_len = indent.chars().count();

    // Check if this thread has an unsent draft
    let has_draft = app.has_draft_for_thread(&thread.id);

    // Extract data
    let (project_name, thread_author_name, preview, timestamp, is_busy) = {
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
        // Check if any agents are working on this thread
        let is_busy = store.is_event_busy(&thread.id);
        (project_name, thread_author_name, preview, timestamp, is_busy)
    };

    // Spinner for busy threads (uses frame counter from App)
    let spinner_char = if is_busy {
        Some(app.spinner_char())
    } else {
        None
    };

    let time_str = format_relative_time(timestamp);

    // Column widths for table layout
    // Middle column: project (line 1) / author (line 2) - same width for alignment
    // Right column: status (line 1) / time (line 2) - same width for alignment
    let middle_col_width = 22;
    let right_col_width = 14;

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
        // LINE 1: [title] [spinner?] [#nested]     [project]     [status]
        let spinner_suffix = spinner_char.map(|c| format!(" {}", c)).unwrap_or_default();
        let nested_suffix = if is_collapsed && child_count > 0 {
            format!(" +{}", child_count)
        } else {
            String::new()
        };
        let title_max = main_col_width.saturating_sub(nested_suffix.chars().count() + spinner_suffix.chars().count());
        let title_truncated = truncate_with_ellipsis(&thread.title, title_max);
        let title_display_len = title_truncated.chars().count() + spinner_suffix.chars().count() + nested_suffix.chars().count();
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
        if thread.is_scheduled {
            line1.push(Span::styled(" ⏰ SCHED", Style::default().fg(theme::TEXT_MUTED)));
        }
        if is_archived {
            line1.push(Span::styled(" [archived]", Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)));
        }
        if has_draft {
            line1.push(Span::styled(" ✎", Style::default().fg(theme::ACCENT_WARNING)));
        }
        if !spinner_suffix.is_empty() {
            line1.push(Span::styled(spinner_suffix, Style::default().fg(theme::ACCENT_PRIMARY)));
        }
        if !nested_suffix.is_empty() {
            line1.push(Span::styled(nested_suffix, Style::default().fg(theme::TEXT_MUTED)));
        }
        line1.push(Span::styled(" ".repeat(title_padding), Style::default()));
        line1.push(Span::styled(project_display, Style::default().fg(theme::project_color(a_tag))));
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
        // FULL MODE: Table-like layout
        // LINE 1: [title] [spinner?] [#nested]     [project]     [status]
        // LINE 2: [summary] (if present)
        // LINE 3: [activity] (if present)
        // LINE 4: [reply preview]       [author]      [time]
        // LINE 5: spacing

        // LINE 1: [title] [spinner?] [#nested]     [project]     [status]
        let spinner_suffix = spinner_char.map(|c| format!(" {}", c)).unwrap_or_default();
        let nested_suffix = if has_children && child_count > 0 {
            format!(" {}", child_count)
        } else {
            String::new()
        };
        let title_max = main_col_width.saturating_sub(nested_suffix.chars().count() + spinner_suffix.chars().count());
        let title_truncated = truncate_with_ellipsis(&thread.title, title_max);
        let title_display_len = title_truncated.chars().count() + spinner_suffix.chars().count() + nested_suffix.chars().count();
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
        if thread.is_scheduled {
            line1.push(Span::styled(" ⏰ SCHED", Style::default().fg(theme::TEXT_MUTED)));
        }
        if is_archived {
            line1.push(Span::styled(" [archived]", Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)));
        }
        if has_draft {
            line1.push(Span::styled(" ✎", Style::default().fg(theme::ACCENT_WARNING)));
        }
        if !spinner_suffix.is_empty() {
            line1.push(Span::styled(spinner_suffix, Style::default().fg(theme::ACCENT_PRIMARY)));
        }
        if !nested_suffix.is_empty() {
            line1.push(Span::styled(nested_suffix, Style::default().fg(theme::TEXT_MUTED)));
        }
        line1.push(Span::styled(" ".repeat(title_padding), Style::default()));
        line1.push(Span::styled(project_display, Style::default().fg(theme::project_color(a_tag))));
        line1.push(Span::styled(" ".repeat(project_padding), Style::default()));
        line1.push(Span::styled(" ".repeat(status_padding), Style::default()));
        line1.push(Span::styled(status_truncated, Style::default().fg(theme::ACCENT_WARNING)));
        lines.push(Line::from(line1));

        // LINE 2: [summary] (if present) - from metadata summary tag
        if let Some(ref summary) = thread.summary {
            let mut line_summary = Vec::new();
            if !indent.is_empty() {
                line_summary.push(Span::styled(indent.clone(), Style::default()));
            }
            line_summary.push(Span::styled(" ".repeat(collapse_col_width), Style::default()));
            let summary_truncated = truncate_with_ellipsis(summary, main_col_width + middle_col_width + right_col_width);
            line_summary.push(Span::styled(summary_truncated, Style::default().fg(theme::TEXT_MUTED)));
            lines.push(Line::from(line_summary));
        }

        // LINE 3: Activity (if present)
        if let Some(ref activity) = thread.status_current_activity {
            let mut line_activity = Vec::new();
            if !indent.is_empty() {
                line_activity.push(Span::styled(indent.clone(), Style::default()));
            }
            line_activity.push(Span::styled(" ".repeat(collapse_col_width), Style::default()));
            line_activity.push(Span::styled(card::ACTIVITY_GLYPH, Style::default().fg(theme::ACCENT_PRIMARY)));
            line_activity.push(Span::styled(truncate_with_ellipsis(activity, main_col_width.saturating_sub(3)), Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)));
            lines.push(Line::from(line_activity));
        }

        // LINE 4: [reply preview]       [author]      [time]
        let preview_max = main_col_width;
        let preview_truncated = truncate_with_ellipsis(&preview, preview_max);
        let preview_len = preview_truncated.chars().count();
        let preview_padding = main_col_width.saturating_sub(preview_len);

        // Author (middle column) - show thread creator
        let author_display = format!("@{}", thread_author_name);
        let author_truncated = truncate_with_ellipsis(&author_display, middle_col_width.saturating_sub(1));
        let author_len = author_truncated.chars().count();
        let author_padding = middle_col_width.saturating_sub(author_len);

        // Time (right column, right-aligned)
        let time_len = time_str.chars().count();
        let time_padding = right_col_width.saturating_sub(time_len);

        let mut line_reply = Vec::new();
        if !indent.is_empty() {
            line_reply.push(Span::styled(indent.clone(), Style::default()));
        }
        line_reply.push(Span::styled(" ".repeat(collapse_col_width), Style::default())); // Align with collapse indicator
        line_reply.push(Span::styled(preview_truncated, Style::default().fg(theme::TEXT_MUTED)));
        line_reply.push(Span::styled(" ".repeat(preview_padding), Style::default()));
        line_reply.push(Span::styled(author_truncated, Style::default().fg(theme::ACCENT_SPECIAL)));
        line_reply.push(Span::styled(" ".repeat(author_padding), Style::default()));
        line_reply.push(Span::styled(" ".repeat(time_padding), Style::default()));
        line_reply.push(Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)));
        lines.push(Line::from(line_reply));

        // Spacing line (only when neither this nor next card is selected)
        if !is_selected && !is_multi_selected && !next_is_selected {
            lines.push(Line::from(""));
        }
    }

    if is_selected || is_multi_selected {
        // For selected/multi-selected cards, render half-block borders separately from content
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

    let selected_idx = app.current_selection();
    let items: Vec<ListItem> = inbox_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let is_selected = i == selected_idx;
            let is_multi_selected = item.thread_id.as_ref()
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

fn render_inbox_card(app: &App, item: &InboxItem, is_selected: bool, is_multi_selected: bool) -> ListItem<'static> {
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
    let indicator = if !item.is_read {
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

    // Line 2: Type + Project + Author
    let line2_spans = vec![
        Span::styled(type_str, Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" in ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(project_name, Style::default().fg(theme::project_color(&item.project_a_tag))),
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
fn render_reports_list(f: &mut Frame, app: &App, area: Rect) {
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
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    let bullet = if is_selected { card::BULLET } else { card::SPACER };

    // Line 1: Title + project + reading time + timestamp
    let title_max = area.width as usize - 30;
    let title = crate::ui::format::truncate_with_ellipsis(&report.title, title_max);

    let line1 = Line::from(vec![
        Span::styled(bullet, Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(title, title_style),
        Span::styled("  ", Style::default()),
        Span::styled(&project_name, Style::default().fg(theme::project_color(&report.project_a_tag))),
        Span::styled(format!("  {} · {}", reading_time, time_str), Style::default().fg(theme::TEXT_MUTED)),
    ]);

    // Line 2: Summary + hashtags + author
    let summary_max = area.width as usize - 40;
    let summary = crate::ui::format::truncate_with_ellipsis(&report.summary, summary_max);
    let hashtags: String = report.hashtags.iter().take(3).map(|h| format!("#{} ", h)).collect();

    let line2 = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(summary, Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(format!("  {}", hashtags.trim()), Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(format!("  @{}", author_name), Style::default().fg(theme::ACCENT_SPECIAL)),
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

/// Render the Status tab - conversations with status metadata
fn render_status_list(f: &mut Frame, app: &App, area: Rect) {
    let status_threads = app.status_threads();

    if status_threads.is_empty() {
        let empty = Paragraph::new("No conversations with status updates")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty, area);
        return;
    }

    let selected_idx = app.current_selection();
    let mut y_offset = 0u16;

    for (i, (thread, a_tag)) in status_threads.iter().enumerate() {
        let is_selected = i == selected_idx;
        let is_multi_selected = app.is_thread_multi_selected(&thread.id);
        let card_height = 4u16; // title, summary, activity, spacing

        if y_offset + card_height > area.height {
            break;
        }

        let card_area = Rect::new(
            area.x,
            area.y + y_offset,
            area.width,
            card_height,
        );

        render_status_card(f, app, thread, a_tag, is_selected, is_multi_selected, card_area);
        y_offset += card_height;
    }
}

/// Render a single status card with table-aligned columns
/// Layout matches Conversations tab:
/// [bullet] [title]                    [project]       [status]
///          [summary/activity]         [author]        [time]
fn render_status_card(
    f: &mut Frame,
    app: &App,
    thread: &Thread,
    a_tag: &str,
    is_selected: bool,
    is_multi_selected: bool,
    area: Rect,
) {
    let store = app.data_store.borrow();
    let project_name = store.get_project_name(a_tag);
    let author_name = store.get_profile_name(&thread.pubkey);
    drop(store);

    let time_str = crate::ui::format::format_relative_time(thread.last_activity);

    let title_style = if is_selected {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    let has_activity = thread.status_current_activity.is_some();

    // Column widths for table layout (matching Conversations tab)
    let middle_col_width = 22;
    let right_col_width = 14;
    let total_width = area.width as usize;
    let fixed_cols_width = middle_col_width + right_col_width + 2; // +2 for spacing
    let bullet_width = 2; // "● " or "  "
    let main_col_width = total_width.saturating_sub(fixed_cols_width + bullet_width);

    // Status text for right column
    let status_text = thread.status_label.as_ref()
        .map(|s| format!("{} {}", status_label_to_symbol(s), s))
        .unwrap_or_default();

    let mut lines: Vec<Line> = Vec::new();

    // LINE 1: [bullet] [title]          [project]       [status]
    let bullet = if is_selected || has_activity { card::BULLET } else { card::SPACER };
    let bullet_color = if has_activity { theme::ACCENT_SUCCESS } else { theme::ACCENT_PRIMARY };

    let title_max = main_col_width;
    let title_truncated = truncate_with_ellipsis(&thread.title, title_max);
    let title_len = title_truncated.chars().count();
    let title_padding = main_col_width.saturating_sub(title_len);

    // Project (middle column)
    let project_truncated = truncate_with_ellipsis(&project_name, middle_col_width.saturating_sub(2));
    let project_display = format!("{}{}", card::BULLET_GLYPH, project_truncated);
    let project_len = project_display.chars().count();
    let project_padding = middle_col_width.saturating_sub(project_len);

    // Status (right column, right-aligned)
    let status_truncated = truncate_with_ellipsis(&status_text, right_col_width);
    let status_len = status_truncated.chars().count();
    let status_padding = right_col_width.saturating_sub(status_len);

    let line1 = Line::from(vec![
        Span::styled(bullet, Style::default().fg(bullet_color)),
        Span::styled(title_truncated, title_style),
        Span::styled(" ".repeat(title_padding), Style::default()),
        Span::styled(project_display, Style::default().fg(theme::project_color(a_tag))),
        Span::styled(" ".repeat(project_padding), Style::default()),
        Span::styled(" ".repeat(status_padding), Style::default()),
        Span::styled(status_truncated, Style::default().fg(theme::ACCENT_WARNING)),
    ]);
    lines.push(line1);

    // LINE 2: Summary or activity indicator  [author]        [time]
    let line2_content = if let Some(ref activity) = thread.status_current_activity {
        // Show activity with pulsing indicator
        let indicator = if app.frame_counter % 4 < 2 { "◉" } else { "○" };
        let activity_max = main_col_width.saturating_sub(3); // Space for indicator
        let activity_text = truncate_with_ellipsis(activity, activity_max);
        format!("{} {}", indicator, activity_text)
    } else if let Some(ref summary) = thread.summary {
        truncate_with_ellipsis(summary, main_col_width)
    } else {
        String::new()
    };

    let line2_style = if thread.status_current_activity.is_some() {
        Style::default().fg(theme::ACCENT_SUCCESS)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let line2_len = line2_content.chars().count();
    let line2_padding = main_col_width.saturating_sub(line2_len);

    // Author (middle column)
    let author_display = format!("@{}", author_name);
    let author_truncated = truncate_with_ellipsis(&author_display, middle_col_width.saturating_sub(1));
    let author_len = author_truncated.chars().count();
    let author_padding = middle_col_width.saturating_sub(author_len);

    // Time (right column, right-aligned)
    let time_len = time_str.chars().count();
    let time_padding = right_col_width.saturating_sub(time_len);

    let line2 = Line::from(vec![
        Span::styled("  ", Style::default()), // Align with title (after bullet)
        Span::styled(line2_content, line2_style),
        Span::styled(" ".repeat(line2_padding), Style::default()),
        Span::styled(author_truncated, Style::default().fg(theme::ACCENT_SPECIAL)),
        Span::styled(" ".repeat(author_padding), Style::default()),
        Span::styled(" ".repeat(time_padding), Style::default()),
        Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)),
    ]);
    lines.push(line2);

    // LINE 3: Additional activity line if we have both summary and activity
    if thread.status_current_activity.is_some() && thread.summary.is_some() {
        if let Some(ref summary) = thread.summary {
            let summary_max = main_col_width + middle_col_width + right_col_width;
            let summary_text = truncate_with_ellipsis(summary, summary_max);
            let line3 = Line::from(vec![
                Span::styled("  ", Style::default()),
                Span::styled(summary_text, Style::default().fg(theme::TEXT_MUTED)),
            ]);
            lines.push(line3);
        }
    }

    // LINE 4: Spacing
    lines.push(Line::from(""));

    let content = Paragraph::new(lines);

    if is_selected || is_multi_selected {
        f.render_widget(content.style(Style::default().bg(theme::BG_SELECTED)), area);
    } else {
        f.render_widget(content, area);
    }
}

/// Render the Feed tab - kind:1 events (text notes) from visible projects
fn render_feed_cards(f: &mut Frame, app: &App, area: Rect) {
    let feed_items = app.feed_items();

    if feed_items.is_empty() {
        let empty_lines = vec![
            Line::from(""),
            Line::from(Span::styled(
                "No feed items",
                Style::default().fg(theme::TEXT_MUTED),
            )),
            Line::from(Span::styled(
                "Select projects in the sidebar to see their messages",
                Style::default().fg(theme::TEXT_MUTED),
            )),
        ];
        let empty = Paragraph::new(empty_lines)
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(empty, area);
        return;
    }

    let selected_idx = app.current_selection();
    let mut y_offset = 0u16;

    for (i, item) in feed_items.iter().enumerate() {
        let is_selected = i == selected_idx;
        let is_multi_selected = app.is_thread_multi_selected(&item.thread_id);
        let card_height = 4u16; // author/project, content preview, thread title, spacing

        if y_offset + card_height > area.height {
            break;
        }

        let card_area = Rect::new(
            area.x,
            area.y + y_offset,
            area.width,
            card_height,
        );

        render_feed_card(f, app, item, is_selected, is_multi_selected, card_area);
        y_offset += card_height;
    }
}

/// Render a single feed card
fn render_feed_card(
    f: &mut Frame,
    app: &App,
    item: &crate::ui::app::FeedItem,
    is_selected: bool,
    is_multi_selected: bool,
    area: Rect,
) {
    let store = app.data_store.borrow();
    let author_name = store.get_profile_name(&item.pubkey);
    let project_name = store.get_project_name(&item.project_a_tag);
    drop(store);

    let time_str = crate::ui::format::format_relative_time(item.created_at);

    let title_style = if is_selected {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    let bullet = if is_selected { card::BULLET } else { card::SPACER };
    let bullet_color = theme::ACCENT_PRIMARY;

    // Column widths for table layout
    let middle_col_width = 22;
    let right_col_width = 14;
    let total_width = area.width as usize;
    let fixed_cols_width = middle_col_width + right_col_width + 2;
    let bullet_width = 2;
    let main_col_width = total_width.saturating_sub(fixed_cols_width + bullet_width);

    let mut lines: Vec<Line> = Vec::new();

    // LINE 1: [bullet] @author          [project]       [time]
    let author_display = format!("@{}", author_name);
    let author_truncated = truncate_with_ellipsis(&author_display, main_col_width);
    let author_len = author_truncated.chars().count();
    let author_padding = main_col_width.saturating_sub(author_len);

    let project_truncated = truncate_with_ellipsis(&project_name, middle_col_width.saturating_sub(2));
    let project_display = format!("{}{}", card::BULLET_GLYPH, project_truncated);
    let project_len = project_display.chars().count();
    let project_padding = middle_col_width.saturating_sub(project_len);

    let time_len = time_str.chars().count();
    let time_padding = right_col_width.saturating_sub(time_len);

    let line1 = Line::from(vec![
        Span::styled(bullet, Style::default().fg(bullet_color)),
        Span::styled(author_truncated, Style::default().fg(theme::user_color(&item.pubkey))),
        Span::styled(" ".repeat(author_padding), Style::default()),
        Span::styled(project_display, Style::default().fg(theme::project_color(&item.project_a_tag))),
        Span::styled(" ".repeat(project_padding), Style::default()),
        Span::styled(" ".repeat(time_padding), Style::default()),
        Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)),
    ]);
    lines.push(line1);

    // LINE 2: Content preview
    let content_preview: String = item.content.chars().take(200).collect();
    let content_preview = content_preview.replace('\n', " ");
    let content_max = main_col_width + middle_col_width + right_col_width;
    let content_truncated = truncate_with_ellipsis(&content_preview, content_max);

    let line2 = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(content_truncated, title_style),
    ]);
    lines.push(line2);

    // LINE 3: Thread title (as context for where this message is from)
    let thread_prefix = "in: ";
    let thread_max = main_col_width + middle_col_width + right_col_width - thread_prefix.len();
    let thread_truncated = truncate_with_ellipsis(&item.thread_title, thread_max);

    let line3 = Line::from(vec![
        Span::styled("  ", Style::default()),
        Span::styled(thread_prefix, Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(thread_truncated, Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)),
    ]);
    lines.push(line3);

    // LINE 4: Spacing
    lines.push(Line::from(""));

    let content = Paragraph::new(lines);

    if is_selected || is_multi_selected {
        f.render_widget(content.style(Style::default().bg(theme::BG_SELECTED)), area);
    } else {
        f.render_widget(content, area);
    }
}

/// Render the Search tab content - full search with message display
fn render_search_tab(f: &mut Frame, app: &App, area: Rect) {
    use crate::ui::app::SearchMatchType;

    // Layout: Search bar + Results
    let chunks = Layout::vertical([
        Constraint::Length(2), // Search bar
        Constraint::Min(0),    // Results
    ])
    .split(area);

    // Render search bar
    let search_style = if !app.search_filter.is_empty() {
        Style::default().fg(theme::TEXT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let search_text = if app.search_filter.is_empty() {
        "/ Search threads and messages...".to_string()
    } else {
        format!("/ {}", app.search_filter)
    };

    let search_line = Paragraph::new(search_text).style(search_style);
    f.render_widget(search_line, chunks[0]);

    // Get search results
    let results = app.search_results();
    let selected_idx = app.current_selection();

    if results.is_empty() {
        // Show placeholder or "no results" message
        let msg = if app.search_filter.is_empty() {
            "Type to search threads and messages"
        } else {
            "No results found"
        };
        let empty = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty, chunks[1]);
        return;
    }

    // Render search results with full message content
    let mut y_offset = 0u16;
    let store = app.data_store.borrow();

    for (i, result) in results.iter().enumerate() {
        let is_selected = i == selected_idx;
        let is_multi_selected = app.is_thread_multi_selected(&result.thread.id);

        // Calculate card height based on content
        // Line 1: Title + project + time
        // Line 2: Match type indicator
        // Lines 3+: Message content (if message match)
        // Spacing line
        let base_height = 3u16;
        let content_lines = if let SearchMatchType::Message { ref message_id } = result.match_type {
            // Get the message and show more content
            let messages = store.get_messages(&result.thread.id);
            if let Some(msg) = messages.iter().find(|m| m.id == *message_id) {
                // Show up to 5 lines of content
                let content_preview: String = msg.content.chars().take(400).collect();
                let line_count = content_preview.lines().count().min(5) as u16;
                line_count.max(2)
            } else {
                2
            }
        } else {
            2
        };
        let card_height = base_height + content_lines;

        if y_offset + card_height > chunks[1].height {
            break;
        }

        let card_area = Rect::new(
            chunks[1].x,
            chunks[1].y + y_offset,
            chunks[1].width,
            card_height,
        );

        render_search_result_card(f, app, result, is_selected, is_multi_selected, card_area, &store);
        y_offset += card_height;
    }
}

/// Render a single search result card with full message content
fn render_search_result_card(
    f: &mut Frame,
    app: &App,
    result: &crate::ui::app::SearchResult,
    is_selected: bool,
    is_multi_selected: bool,
    area: Rect,
    store: &std::cell::Ref<crate::store::AppDataStore>,
) {
    use crate::ui::app::SearchMatchType;

    let time_str = crate::ui::format::format_relative_time(result.thread.last_activity);

    let title_style = if is_selected {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };

    let bullet = if is_selected { card::BULLET } else { card::SPACER };
    let bullet_color = theme::ACCENT_PRIMARY;

    // Line 1: Title + match type indicator + project + timestamp
    let title_max = area.width as usize - 40;
    let title = crate::ui::format::truncate_with_ellipsis(&result.thread.title, title_max);

    let (type_indicator, type_color) = match &result.match_type {
        SearchMatchType::Thread => ("[T]", theme::ACCENT_PRIMARY),
        SearchMatchType::ConversationId => ("[I]", theme::ACCENT_WARNING),
        SearchMatchType::Message { .. } => ("[M]", theme::ACCENT_SUCCESS),
    };

    let line1 = Line::from(vec![
        Span::styled(bullet, Style::default().fg(bullet_color)),
        Span::styled(type_indicator, Style::default().fg(type_color)),
        Span::styled(" ", Style::default()),
        Span::styled(title, title_style),
        Span::styled("  ", Style::default()),
        Span::styled(&result.project_name, Style::default().fg(theme::project_color(&result.project_a_tag))),
        Span::styled(format!("  {}", time_str), Style::default().fg(theme::TEXT_MUTED)),
    ]);

    let mut lines = vec![line1];

    // Line 2+: Show full message content if this is a message match
    match &result.match_type {
        SearchMatchType::Message { message_id } => {
            let messages = store.get_messages(&result.thread.id);
            if let Some(msg) = messages.iter().find(|m| m.id == *message_id) {
                // Get author name
                let author_name = store.get_profile_name(&msg.pubkey);
                let author_color = theme::user_color(&msg.pubkey);

                // Author line
                lines.push(Line::from(vec![
                    Span::styled("  @", Style::default().fg(theme::TEXT_MUTED)),
                    Span::styled(author_name, Style::default().fg(author_color)),
                    Span::styled(":", Style::default().fg(theme::TEXT_MUTED)),
                ]));

                // Message content (limited to a few lines)
                let content_preview: String = msg.content.chars().take(400).collect();
                let content_width = area.width as usize - 4;

                // Highlight the search term in the content
                let filter_lower = app.search_filter.to_lowercase();

                for line in content_preview.lines().take(4) {
                    let mut spans = vec![Span::styled("    ", Style::default())];

                    // Check if this line contains the search term
                    let line_lower = line.to_lowercase();
                    if let Some(match_start) = line_lower.find(&filter_lower) {
                        let match_end = match_start + app.search_filter.len();

                        // Before match
                        if match_start > 0 {
                            let before = &line[..match_start];
                            let truncated = crate::ui::format::truncate_with_ellipsis(before, content_width / 2);
                            spans.push(Span::styled(truncated, Style::default().fg(theme::TEXT_MUTED)));
                        }

                        // Highlighted match
                        let match_text = &line[match_start..match_end.min(line.len())];
                        spans.push(Span::styled(
                            match_text.to_string(),
                            Style::default().fg(theme::BG_APP).bg(theme::ACCENT_WARNING),
                        ));

                        // After match
                        if match_end < line.len() {
                            let after = &line[match_end..];
                            let truncated = crate::ui::format::truncate_with_ellipsis(after, content_width / 2);
                            spans.push(Span::styled(truncated, Style::default().fg(theme::TEXT_MUTED)));
                        }
                    } else {
                        // No match on this line, render normally
                        let truncated = crate::ui::format::truncate_with_ellipsis(line, content_width);
                        spans.push(Span::styled(truncated, Style::default().fg(theme::TEXT_MUTED)));
                    }

                    lines.push(Line::from(spans));
                }

                // Show "..." if content was truncated
                if content_preview.lines().count() > 4 || msg.content.len() > 400 {
                    lines.push(Line::from(vec![
                        Span::styled("    ...", Style::default().fg(theme::TEXT_MUTED)),
                    ]));
                }
            } else if let Some(excerpt) = &result.excerpt {
                // Fallback to excerpt if message not found
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(excerpt.clone(), Style::default().fg(theme::TEXT_MUTED)),
                ]));
            }
        }
        SearchMatchType::Thread => {
            // Show thread summary or content excerpt
            if let Some(ref summary) = result.thread.summary {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        crate::ui::format::truncate_with_ellipsis(summary, area.width as usize - 4),
                        Style::default().fg(theme::TEXT_MUTED),
                    ),
                ]));
            } else if let Some(excerpt) = &result.excerpt {
                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(excerpt.clone(), Style::default().fg(theme::TEXT_MUTED)),
                ]));
            }
        }
        SearchMatchType::ConversationId => {
            // Show the ID
            lines.push(Line::from(vec![
                Span::styled("  ID: ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(
                    crate::ui::format::truncate_with_ellipsis(&result.thread.id, 40),
                    Style::default().fg(theme::ACCENT_SPECIAL),
                ),
            ]));
        }
    }

    // Spacing line
    lines.push(Line::from(""));

    let content = Paragraph::new(lines);

    if is_selected || is_multi_selected {
        f.render_widget(content.style(Style::default().bg(theme::BG_SELECTED)), area);
    } else {
        f.render_widget(content, area);
    }
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
        let is_busy = app.data_store.borrow().is_project_busy(&a_tag);
        let is_archived = app.is_project_archived(&a_tag);

        let checkbox = if is_visible { card::CHECKBOX_ON_PAD } else { card::CHECKBOX_OFF_PAD };
        let focus_indicator = if is_focused { card::COLLAPSE_CLOSED } else { card::SPACER };
        // Reserve space for spinner (2 chars) and/or archived tag (10 chars)
        let name_max = match (is_busy, is_archived) {
            (true, true) => 8,   // Both spinner and archived
            (true, false) => 18, // Just spinner
            (false, true) => 10, // Just archived
            (false, false) => 20, // Neither
        };
        let name = truncate_with_ellipsis(&project.name, name_max);

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

        let mut spans = vec![
            Span::styled(focus_indicator, Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled(checkbox, checkbox_style),
            Span::styled(name, name_style),
        ];

        // Add archived tag if project is archived
        if is_archived {
            spans.push(Span::styled(" [archived]", Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)));
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
        let is_busy = app.data_store.borrow().is_project_busy(&a_tag);
        let is_archived = app.is_project_archived(&a_tag);

        let checkbox = if is_visible { card::CHECKBOX_ON_PAD } else { card::CHECKBOX_OFF_PAD };
        let focus_indicator = if is_focused { card::COLLAPSE_CLOSED } else { card::SPACER };
        // Reserve space for spinner (2 chars) and/or archived tag (10 chars)
        let name_max = match (is_busy, is_archived) {
            (true, true) => 8,   // Both spinner and archived
            (true, false) => 18, // Just spinner
            (false, true) => 10, // Just archived
            (false, false) => 20, // Neither
        };
        let name = truncate_with_ellipsis(&project.name, name_max);

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

        let mut spans = vec![
            Span::styled(focus_indicator, Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled(checkbox, checkbox_style),
            Span::styled(name, name_style),
        ];

        // Add archived tag if project is archived
        if is_archived {
            spans.push(Span::styled(" [archived]", Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)));
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

fn render_bottom_padding(f: &mut Frame, area: Rect) {
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
        offline_projects.get(offline_index).map(|p| (p.clone(), false))
    }
}

/// Get the total count of selectable projects
pub fn selectable_project_count(app: &App) -> usize {
    let (online, offline) = app.filtered_projects();
    online.len() + offline.len()
}

fn render_projects_modal(f: &mut Frame, app: &App, area: Rect) {
    let filter = app.projects_modal_filter();

    let (popup_area, remaining) = Modal::new("Switch Project")
        .size(ModalSize {
            max_width: 65,
            height_percent: 0.7,
        })
        .search(filter, "Search projects...")
        .render_frame(f, area);

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
    // Calculate modal dimensions - dynamic based on tab count (+1 for Home entry)
    let tab_count = app.open_tabs().len() + 1; // +1 for Home
    let content_height = (tab_count + 2) as u16; // +2 for header spacing
    let total_height = content_height + 4; // +4 for padding and hints
    let height_percent = (total_height as f32 / area.height as f32).min(0.7);

    let (popup_area, remaining) = Modal::new("Open Tabs")
        .size(ModalSize {
            max_width: 70,
            height_percent,
        })
        .render_frame(f, area);

    // Build items list - Home is always first (option 1)
    let data_store = app.data_store.borrow();
    let mut items: Vec<ModalItem> = Vec::with_capacity(app.open_tabs().len() + 1);

    // Home entry (option 1)
    let home_selected = app.tab_modal_index() == 0 && app.open_tabs().is_empty();
    let home_active = app.view == View::Home;
    let home_marker = if home_active { card::BULLET } else { card::SPACER };
    items.push(
        ModalItem::new(format!("{}Home (Dashboard)", home_marker))
            .with_shortcut("1".to_string())
            .selected(home_selected),
    );

    // Tab entries (options 2-9)
    for (i, tab) in app.open_tabs().iter().enumerate() {
        let is_selected = i == app.tab_modal_index();
        let is_active = i == app.active_tab_index() && app.view == View::Chat;

        let project_name = data_store.get_project_name(&tab.project_a_tag);
        // Look up title from store for real threads (gets kind:513 metadata title), use cached for drafts
        let thread_title = if tab.is_draft() {
            tab.thread_title.clone()
        } else {
            data_store
                .get_thread_by_id(&tab.thread_id)
                .map(|t| t.title.clone())
                .unwrap_or_else(|| tab.thread_title.clone())
        };
        let title_display = truncate_with_ellipsis(&thread_title, 30);

        let active_marker = if is_active { card::BULLET } else { card::SPACER };
        let text = format!("{}{} · {}", active_marker, project_name, title_display);

        // Tab number is i+2 (since 1 is Home)
        let shortcut = if i + 2 <= 9 {
            format!("{}", i + 2)
        } else {
            String::new()
        };

        items.push(ModalItem::new(text).with_shortcut(shortcut).selected(is_selected));
    }
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
    let hints = Paragraph::new("↑↓ navigate · enter switch · x close · 1=Home 2-9=tabs")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

/// Render the search modal (/) showing search results
pub fn render_search_modal(f: &mut Frame, app: &App, area: Rect) {
    use crate::ui::app::SearchMatchType;

    let (popup_area, remaining) = Modal::new("Search")
        .size(ModalSize {
            max_width: 80,
            height_percent: 0.8,
        })
        .search(&app.search_filter, "Search threads and messages...")
        .render_frame(f, area);

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

/// Render the project actions modal (boot, settings)
fn render_project_actions_modal(f: &mut Frame, area: Rect, state: &ProjectActionsState) {
    let actions = state.available_actions();
    let content_height = (actions.len() + 2) as u16;
    let total_height = content_height + 4;
    let height_percent = (total_height as f32 / area.height as f32).min(0.5);

    let (popup_area, remaining) = Modal::new(&state.project_name)
        .size(ModalSize {
            max_width: 40,
            height_percent,
        })
        .render_frame(f, area);

    let items: Vec<ModalItem> = actions
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let is_selected = i == state.selected_index;
            ModalItem::new(action.label(state.is_archived))
                .with_shortcut(action.hotkey().to_string())
                .selected(is_selected)
        })
        .collect();

    render_modal_items(f, remaining, &items);

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

fn render_conversation_actions_modal(f: &mut Frame, area: Rect, state: &ConversationActionsState) {
    let actions = ConversationAction::ALL;
    let content_height = (actions.len() + 2) as u16;
    let total_height = content_height + 4;
    let height_percent = (total_height as f32 / area.height as f32).min(0.5);

    // Truncate title if too long
    let title = truncate_with_ellipsis(&state.thread_title, 35);

    let (popup_area, remaining) = Modal::new(&title)
        .size(ModalSize {
            max_width: 45,
            height_percent,
        })
        .render_frame(f, area);

    let items: Vec<ModalItem> = actions
        .iter()
        .enumerate()
        .map(|(i, action)| {
            let is_selected = i == state.selected_index;
            ModalItem::new(action.label(state.is_archived))
                .with_shortcut(action.hotkey().to_string())
                .selected(is_selected)
        })
        .collect();

    render_modal_items(f, remaining, &items);

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
