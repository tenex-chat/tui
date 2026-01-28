use crate::models::{InboxEventType, InboxItem, Thread};
use crate::ui::components::{
    render_modal_items, render_modal_sections, render_statusbar, render_tab_bar, Modal, ModalItem,
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
    widgets::{Block, Borders, Cell, List, ListItem, ListState, Padding, Paragraph, Row, Table},
    Frame,
};
use unicode_width::UnicodeWidthStr;

pub fn render_home(f: &mut Frame, app: &App, area: Rect) {
    // Fill entire area with app background (pure black)
    let bg_block = Block::default().style(Style::default().bg(theme::BG_APP));
    f.render_widget(bg_block, area);

    let has_tabs = !app.open_tabs().is_empty();

    // Layout: Header tabs | Main area | Bottom padding | Optional tab bar | Statusbar
    let chunks = if has_tabs {
        Layout::vertical([
            Constraint::Length(2), // Tab header
            Constraint::Min(0),    // Main area (sidebar + content)
            Constraint::Length(1), // Bottom padding
            Constraint::Length(layout::TAB_BAR_HEIGHT), // Open tabs bar
            Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
        ])
        .split(area)
    } else {
        Layout::vertical([
            Constraint::Length(2), // Tab header
            Constraint::Min(0),    // Main area (sidebar + content)
            Constraint::Length(1), // Bottom padding
            Constraint::Length(layout::STATUSBAR_HEIGHT), // Global statusbar
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

    // If sidebar search is visible with a query, show search results instead
    if app.sidebar_search.visible && app.sidebar_search.has_query() {
        render_sidebar_search_results(f, app, padded_content);
    } else {
        match app.home_panel_focus {
            HomeTab::Conversations => render_conversations_with_feed(f, app, padded_content),
            HomeTab::Inbox => render_inbox_cards(f, app, padded_content),
            HomeTab::Reports => render_reports_list(f, app, padded_content),
            HomeTab::Feed => render_feed_cards(f, app, padded_content),
            HomeTab::ActiveWork => render_active_work(f, app, padded_content),
            HomeTab::Stats => super::render_stats(f, app, padded_content),
        }
    }

    // Render sidebar on the right
    render_project_sidebar(f, app, main_chunks[1]);

    // Bottom padding
    render_bottom_padding(f, chunks[2]);

    // Open tabs bar (if tabs exist)
    if has_tabs {
        render_tab_bar(f, app, chunks[3]);
    }

    // Status bar at the very bottom (always visible)
    // Uses today-only runtime filtering for the status bar display
    let statusbar_area = if has_tabs { chunks[4] } else { chunks[3] };
    let cumulative_runtime = app.data_store.borrow_mut().get_today_unique_runtime();
    render_statusbar(f, statusbar_area, app.current_notification(), cumulative_runtime);

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

    // Nudge CRUD modals
    if let ModalState::NudgeList(ref state) = app.modal_state {
        super::render_nudge_list(f, app, area, state);
    }
    if let ModalState::NudgeCreate(ref state) = app.modal_state {
        super::render_nudge_create(f, app, area, state);
    }
    if let ModalState::NudgeDetail(ref state) = app.modal_state {
        super::render_nudge_detail(f, app, area, state);
    }
    if let ModalState::NudgeDeleteConfirm(ref state) = app.modal_state {
        super::render_nudge_delete_confirm(f, app, area, state);
    }

    // Workspace manager modal
    if let ModalState::WorkspaceManager(ref state) = app.modal_state {
        let workspaces = app.preferences.borrow().workspaces().to_vec();
        let projects = app.data_store.borrow().get_projects().to_vec();
        let active_id = app.preferences.borrow().active_workspace_id().map(String::from);
        super::render_workspace_manager(f, area, state, &workspaces, &projects, active_id.as_deref());
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
    spans.push(Span::styled("Feed", tab_style(HomeTab::Feed)));
    spans.push(Span::styled("   ", Style::default()));
    spans.push(Span::styled("Active", tab_style(HomeTab::ActiveWork)));
    spans.push(Span::styled("   ", Style::default()));
    spans.push(Span::styled("Stats", tab_style(HomeTab::Stats)));

    let header_line = Line::from(spans);

    // Second line: tab indicator underline
    let accent = Style::default().fg(theme::ACCENT_PRIMARY);
    let blank = Style::default();

    // Calculate dynamic spacing for inbox count
    let inbox_count_width = if inbox_count > 0 {
        format!(" ({})", inbox_count).len()
    } else {
        0
    };

    let indicator_spans = vec![
        Span::styled("         ", blank), // Padding for "  TENEX  "
        Span::styled(if app.home_panel_focus == HomeTab::Conversations { "─────────────" } else { "             " },
            if app.home_panel_focus == HomeTab::Conversations { accent } else { blank }),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Inbox { "─────" } else { "     " },
            if app.home_panel_focus == HomeTab::Inbox { accent } else { blank }),
        Span::styled(" ".repeat(inbox_count_width), blank),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Reports { "───────" } else { "       " },
            if app.home_panel_focus == HomeTab::Reports { accent } else { blank }),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Feed { "────" } else { "    " },
            if app.home_panel_focus == HomeTab::Feed { accent } else { blank }),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::ActiveWork { "──────" } else { "      " },
            if app.home_panel_focus == HomeTab::ActiveWork { accent } else { blank }),
        Span::styled("   ", blank),
        Span::styled(if app.home_panel_focus == HomeTab::Stats { "─────" } else { "     " },
            if app.home_panel_focus == HomeTab::Stats { accent } else { blank }),
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
    // Full mode: title+recipient+project, summary+time, status+runtime (always 3 lines)
    // Compact mode: 2 lines (title+recipient+project, time)
    // Selected/multi-selected items add 2 lines for half-block borders (top + bottom)
    // and drop spacing line (borders provide visual separation)
    // next_is_selected: if true, this card doesn't need spacing (next card's top border provides it)
    let calc_card_height = |item: &HierarchicalThread, is_selected: bool, is_multi_selected: bool, next_is_selected: bool| -> u16 {
        let is_compact = item.depth > 0;
        if is_compact {
            // Compact: 2 content lines + optional 2 border lines
            return if is_selected || is_multi_selected { 4 } else { 2 };
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
        if is_selected || is_multi_selected { lines + 2 } else { lines }
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
        let h = calc_card_height(item, item_is_selected, item_is_multi_selected, next_is_selected);
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
    build_thread_hierarchy(&recent, &app.collapsed_threads, &q_tag_relationships, default_collapsed)
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
        let is_busy = store.is_event_busy(&thread.id);
        // Get first recipient only (avoid allocating full Vec when we only need first)
        let first_recipient: Option<(String, String)> = thread.p_tags.first()
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
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
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
    let main_col_width = total_width.saturating_sub(fixed_cols_width + indent_len + collapse_col_width);

    // Status text with symbol (for line 3)
    let status_text = thread.status_label.as_ref()
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
            nested_suffix.width() +
            spinner_suffix.width() +
            recipient_suffix.width()
        );
        let title_truncated = truncate_with_ellipsis(&thread.title, title_max);
        let title_display_len = title_truncated.width() +
            spinner_suffix.width() +
            nested_suffix.width() +
            recipient_suffix.width();
        let title_padding = main_col_width.saturating_sub(title_display_len);

        // Project (right column, right-aligned)
        let project_truncated = truncate_with_ellipsis(&project_name, right_col_width.saturating_sub(2));
        let project_display = format!("{}{}", card::BULLET_GLYPH, project_truncated);
        let project_len = project_display.width();
        let project_padding = right_col_width.saturating_sub(project_len);

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
        // Add recipient with deterministic color
        if !recipient_suffix.is_empty() {
            let color = recipient_pubkey.as_ref()
                .map(|pk| theme::user_color(pk))
                .unwrap_or(theme::TEXT_MUTED);
            line1.push(Span::styled(recipient_suffix, Style::default().fg(color)));
        }
        line1.push(Span::styled(" ".repeat(title_padding), Style::default()));
        line1.push(Span::styled(" ".repeat(project_padding), Style::default()));
        line1.push(Span::styled(project_display, Style::default().fg(theme::project_color(a_tag))));
        lines.push(Line::from(line1));

        // LINE 2: [empty main]                              [time]
        let time_len = time_str.width();
        let time_padding = right_col_width.saturating_sub(time_len);

        let mut line2 = Vec::new();
        // Add indent for nested items
        if !indent.is_empty() {
            line2.push(Span::styled(indent.clone(), Style::default()));
        }
        line2.push(Span::styled(" ".repeat(collapse_col_width), Style::default()));
        line2.push(Span::styled(" ".repeat(main_col_width), Style::default()));
        line2.push(Span::styled(" ".repeat(time_padding), Style::default()));
        line2.push(Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)));
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
            nested_suffix.width() +
            spinner_suffix.width() +
            recipient_suffix.width()
        );
        let title_truncated = truncate_with_ellipsis(&thread.title, title_max);
        let title_display_len = title_truncated.width() +
            spinner_suffix.width() +
            nested_suffix.width() +
            recipient_suffix.width();
        let title_padding = main_col_width.saturating_sub(title_display_len);

        // Project for line 1 (right column, right-aligned)
        let project_truncated = truncate_with_ellipsis(&project_name, right_col_width.saturating_sub(2));
        let project_display = format!("{}{}", card::BULLET_GLYPH, project_truncated);
        let project_len = project_display.width();
        let project_padding = right_col_width.saturating_sub(project_len);

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
        // Add recipient with deterministic color
        if !recipient_suffix.is_empty() {
            let color = recipient_pubkey.as_ref()
                .map(|pk| theme::user_color(pk))
                .unwrap_or(theme::TEXT_MUTED);
            line1.push(Span::styled(recipient_suffix, Style::default().fg(color)));
        }
        line1.push(Span::styled(" ".repeat(title_padding), Style::default()));
        line1.push(Span::styled(" ".repeat(project_padding), Style::default()));
        line1.push(Span::styled(project_display, Style::default().fg(theme::project_color(a_tag))));
        lines.push(Line::from(line1));

        // LINE 2: [summary]                                    [relative-last-activity]
        let time_len = time_str.width();
        let time_padding = right_col_width.saturating_sub(time_len);
        let summary_max = main_col_width;

        let mut line2 = Vec::new();
        if !indent.is_empty() {
            line2.push(Span::styled(indent.clone(), Style::default()));
        }
        line2.push(Span::styled(" ".repeat(collapse_col_width), Style::default()));
        if let Some(ref summary) = thread.summary {
            let summary_truncated = truncate_with_ellipsis(summary, summary_max);
            let summary_len = summary_truncated.width();
            let summary_padding = main_col_width.saturating_sub(summary_len);
            line2.push(Span::styled(summary_truncated, Style::default().fg(theme::TEXT_MUTED)));
            line2.push(Span::styled(" ".repeat(summary_padding), Style::default()));
        } else {
            line2.push(Span::styled(" ".repeat(main_col_width), Style::default()));
        }
        line2.push(Span::styled(" ".repeat(time_padding), Style::default()));
        line2.push(Span::styled(time_str, Style::default().fg(theme::TEXT_MUTED)));
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
        line3.push(Span::styled(" ".repeat(collapse_col_width), Style::default()));

        if !status_text.is_empty() {
            let status_max = main_col_width;
            let status_truncated = truncate_with_ellipsis(&status_text, status_max);
            let status_len = status_truncated.width();
            let status_padding = main_col_width.saturating_sub(status_len);
            line3.push(Span::styled(status_truncated, Style::default().fg(theme::ACCENT_WARNING)));
            line3.push(Span::styled(" ".repeat(status_padding), Style::default()));
        } else {
            line3.push(Span::styled(" ".repeat(main_col_width), Style::default()));
        }

        line3.push(Span::styled(" ".repeat(runtime_padding), Style::default()));
        line3.push(Span::styled(runtime_display, Style::default().fg(theme::TEXT_MUTED)));
        lines.push(Line::from(line3));

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

    // Check if this is a "Waiting For You" item (Mention type = user was p-tagged)
    let is_waiting_for_user = matches!(item.event_type, InboxEventType::Mention) && !item.is_read;

    let type_str = match item.event_type {
        InboxEventType::Mention => "@ mentioned you",
        InboxEventType::Reply => "Reply",
        InboxEventType::ThreadReply => "Thread reply",
    };

    let title_style = if is_selected {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else if is_waiting_for_user {
        // Waiting for user items get yellow/warning style
        Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)
    } else if !item.is_read {
        Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
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
    // Calculate flexible middle column width based on content
    let author_display = format!("@{}", author_name);
    let project_display_len = project_name.chars().count() + 1; // +1 for bullet glyph

    // Middle column should fit project with min 22 and max 30
    let middle_col_width = project_display_len.clamp(22, 30);

    let right_col_width = 14;
    let total_width = area.width as usize;
    let fixed_cols_width = middle_col_width + right_col_width + 2;
    let bullet_width = 2;
    let main_col_width = total_width.saturating_sub(fixed_cols_width + bullet_width);

    let mut lines: Vec<Line> = Vec::new();

    // LINE 1: [bullet] @author          [project]       [time]
    let author_truncated = truncate_with_ellipsis(&author_display, main_col_width);
    let author_len = author_truncated.chars().count();
    let author_padding = main_col_width.saturating_sub(author_len);

    let project_truncated = truncate_with_ellipsis(&project_name, middle_col_width.saturating_sub(1));
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

/// Render the Active Work tab showing currently active operations
fn render_active_work(f: &mut Frame, app: &App, area: Rect) {
    let data_store = app.data_store.borrow();
    let operations = data_store.get_all_active_operations();

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
        let empty = Paragraph::new(empty_lines)
            .alignment(ratatui::layout::Alignment::Center);
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
        let agent_names: Vec<String> = op.agent_pubkeys
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
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        let conv_style = if is_selected {
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        let project_style = if is_selected {
            Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::ACCENT_SUCCESS)
        };

        let duration_style = if is_selected {
            Style::default().fg(theme::TEXT_PRIMARY)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        // Add selection indicator
        let bullet = if is_selected { card::BULLET } else { card::SPACER };
        let agent_display = format!("{} {}", bullet, truncate_with_ellipsis(&agent_str, 18));

        rows.push(Row::new(vec![
            Cell::from(agent_display).style(agent_style),
            Cell::from(truncate_with_ellipsis(&conv_title, 30)).style(conv_style),
            Cell::from(truncate_with_ellipsis(&project_name, 20)).style(project_style),
            Cell::from(duration).style(duration_style),
        ]).style(row_style));
    }

    drop(data_store);

    // Create header
    let header = Row::new(vec![
        Cell::from("Agent").style(Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::BOLD)),
        Cell::from("Conversation").style(Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::BOLD)),
        Cell::from("Project").style(Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::BOLD)),
        Cell::from("Duration").style(Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::BOLD)),
    ])
    .style(Style::default().bg(theme::BG_SECONDARY))
    .height(1);

    let widths = [
        Constraint::Length(22),  // Agent
        Constraint::Min(20),     // Conversation (flexible)
        Constraint::Length(22),  // Project
        Constraint::Length(12),  // Duration
    ];

    let table = Table::new(rows, widths)
        .header(header)
        .column_spacing(2);

    f.render_widget(table, area);
}

/// Render the project sidebar with checkboxes for filtering
fn render_project_sidebar(f: &mut Frame, app: &App, area: Rect) {
    // If sidebar search is visible, add search input at top
    if app.sidebar_search.visible {
        let chunks = Layout::vertical([
            Constraint::Length(3), // Search input
            Constraint::Min(5),    // Projects list
            Constraint::Length(4), // Filter section
        ])
        .split(area);

        render_sidebar_search_input(f, app, chunks[0]);
        render_projects_list(f, app, chunks[1]);
        render_filters_section(f, app, chunks[2]);
    } else {
        // Normal layout without search
        let chunks = Layout::vertical([
            Constraint::Min(5),    // Projects list
            Constraint::Length(4), // Filter section
        ])
        .split(area);

        render_projects_list(f, app, chunks[0]);
        render_filters_section(f, app, chunks[1]);
    }
}

/// Render the sidebar search input
fn render_sidebar_search_input(f: &mut Frame, app: &App, area: Rect) {
    // Border block with title
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
        .title(Span::styled(" Search ", Style::default().fg(theme::ACCENT_PRIMARY)));
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
        spans.push(Span::styled(before, Style::default().fg(theme::TEXT_PRIMARY)));
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
        spans.push(Span::styled(after, Style::default().fg(theme::TEXT_PRIMARY)));
    }

    // Placeholder when empty (different hints for different tabs)
    if query.is_empty() {
        let placeholder = if app.home_panel_focus == HomeTab::Reports {
            "type to search..."
        } else {
            "type to search (use + for AND)..."
        };
        spans.push(Span::styled(placeholder, Style::default().fg(theme::TEXT_MUTED)));
    }

    let search_line = Paragraph::new(Line::from(spans));
    f.render_widget(search_line, inner);
}

/// Render the sidebar search results in the main content area
fn render_sidebar_search_results(f: &mut Frame, app: &App, area: Rect) {
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
    let selected_idx = app.sidebar_search.selected_index.min(results.len().saturating_sub(1));
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
            HierarchicalSearchItem::MatchedConversation { matching_messages, .. } => {
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

        let card_area = Rect::new(
            area.x,
            area.y + y_offset,
            area.width,
            card_height,
        );

        render_hierarchical_search_item(f, item, is_selected, card_area, &store, query);
        y_offset += card_height;
    }

    // Show result count at bottom
    let count_text = format!("{} match{}", match_count, if match_count == 1 { "" } else { "es" });
    let count_area = Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1);
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
        HierarchicalSearchItem::ContextAncestor { thread_title, project_a_tag, .. } => {
            // Context ancestors are dimmed and compact
            let title_max = (area.width as usize).saturating_sub(indent_width).saturating_sub(5).max(10);
            let title = crate::ui::format::truncate_with_ellipsis(thread_title, title_max);

            let style = if is_selected {
                Style::default().fg(theme::TEXT_MUTED).bg(theme::BG_SELECTED)
            } else {
                Style::default().fg(theme::TEXT_MUTED).add_modifier(Modifier::DIM)
            };

            let line = Line::from(vec![
                Span::styled(&indent, Style::default()),
                Span::styled(title, style),
                Span::styled("  ", Style::default()),
                Span::styled(
                    store.get_project_name(project_a_tag),
                    Style::default().fg(theme::project_color(project_a_tag)).add_modifier(Modifier::DIM)
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
                "[+]"  // Multi-term AND match
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
                theme::ACCENT_WARNING  // Special color for multi-term matches
            } else if *id_matched {
                theme::TEXT_MUTED
            } else if *title_matched {
                theme::ACCENT_PRIMARY
            } else if *content_matched {
                theme::ACCENT_WARNING
            } else {
                theme::ACCENT_SUCCESS
            };

            let title_max = (area.width as usize).saturating_sub(indent_width).saturating_sub(30).max(10);

            // Highlight matching text in title if title matched
            // For multi-term, highlight all matching terms
            let title_spans = if *title_matched {
                highlight_text_spans_multi(thread_title, matched_terms, theme::TEXT_PRIMARY, theme::ACCENT_PRIMARY)
            } else {
                vec![Span::styled(
                    crate::ui::format::truncate_with_ellipsis(thread_title, title_max),
                    if is_selected {
                        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
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
            let content_width = (area.width as usize).saturating_sub(message_indent.len()).saturating_sub(2).max(10);

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
                let preview: String = msg.content.lines().next().unwrap_or("").chars().take(content_width).collect();
                let highlighted_spans = build_bracket_highlight_spans_multi(&preview, matched_terms, content_width);
                let mut content_line_spans = vec![
                    Span::styled(&message_indent, Style::default().fg(theme::TEXT_MUTED)),
                ];
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
        return vec![Span::styled(text.to_string(), Style::default().fg(theme::TEXT_MUTED))];
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
                Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD),
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
        spans.push(Span::styled(remaining, Style::default().fg(theme::TEXT_MUTED)));
    }

    if spans.is_empty() {
        vec![Span::styled(text.to_string(), Style::default().fg(theme::TEXT_MUTED))]
    } else {
        spans
    }
}

/// Build highlighted spans with [brackets] around matching text for multiple terms
/// Each term is highlighted where it appears in the text
fn build_bracket_highlight_spans_multi(text: &str, terms: &[String], _max_width: usize) -> Vec<Span<'static>> {
    if terms.is_empty() {
        return vec![Span::styled(text.to_string(), Style::default().fg(theme::TEXT_MUTED))];
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
            Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD),
        ));
        last_end = end;
    }

    // Add remaining text
    if last_end < text_len {
        let remaining: String = text_chars[last_end..].iter().collect();
        spans.push(Span::styled(remaining, Style::default().fg(theme::TEXT_MUTED)));
    }

    if spans.is_empty() {
        vec![Span::styled(text.to_string(), Style::default().fg(theme::TEXT_MUTED))]
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
        return vec![Span::styled(text.to_string(), Style::default().fg(normal_color))];
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
            Style::default().fg(highlight_color).add_modifier(Modifier::BOLD),
        ));
        last_end = end;
    }

    if last_end < text_len {
        let remaining: String = text_chars[last_end..].iter().collect();
        spans.push(Span::styled(remaining, Style::default().fg(normal_color)));
    }

    if spans.is_empty() {
        vec![Span::styled(text.to_string(), Style::default().fg(normal_color))]
    } else {
        spans
    }
}

/// Render report search results
fn render_report_search_results(f: &mut Frame, app: &App, area: Rect) {
    let results = &app.sidebar_search.report_results;
    let selected_idx = app.sidebar_search.selected_index.min(results.len().saturating_sub(1));
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

        let card_area = Rect::new(
            area.x,
            area.y + y_offset,
            area.width,
            card_height,
        );

        render_report_search_result_card(f, report, is_selected, card_area, &query_lower);
        y_offset += card_height;
    }

    // Show result count at bottom
    let result_count = results.len();
    let count_text = format!("{} report{}", result_count, if result_count == 1 { "" } else { "s" });
    let count_area = Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1);
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
    let title_line = highlight_text_spans(&report.title, query, theme::TEXT_PRIMARY, theme::ACCENT_PRIMARY);
    let title_para = Paragraph::new(Line::from(title_line));
    let title_area = Rect::new(inner.x, inner.y, inner.width, 1);
    f.render_widget(title_para, title_area);

    // Line 2: Summary (truncated) with highlighting
    if inner.height > 1 {
        let summary: String = report.summary.chars().take(100).collect();
        let summary_line = highlight_text_spans(&summary, query, theme::TEXT_MUTED, theme::ACCENT_PRIMARY);
        let summary_para = Paragraph::new(Line::from(summary_line));
        let summary_area = Rect::new(inner.x, inner.y + 1, inner.width, 1);
        f.render_widget(summary_para, summary_area);
    }

    // Line 3: Hashtags
    if inner.height > 2 && !report.hashtags.is_empty() {
        let tags = report.hashtags.iter()
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
fn chars_match_ascii_ignore_case(text_chars: &[char], query_chars: &[char], start_idx: usize) -> bool {
    query_chars.iter().enumerate().all(|(i, qc)| {
        text_chars.get(start_idx + i).map_or(false, |tc| tc.eq_ignore_ascii_case(qc))
    })
}

/// Highlight matching text in a string with spans
fn highlight_text_spans(text: &str, query: &str, normal_color: ratatui::style::Color, highlight_color: ratatui::style::Color) -> Vec<Span<'static>> {
    if query.is_empty() {
        return vec![Span::styled(text.to_string(), Style::default().fg(normal_color))];
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
                Style::default().fg(highlight_color).add_modifier(Modifier::BOLD),
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
        vec![Span::styled(text.to_string(), Style::default().fg(normal_color))]
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
    let time_label = app.home.time_filter
        .map(|tf| tf.label())
        .unwrap_or("All");
    let time_style = if app.home.time_filter.is_some() {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let time_indicator = if app.home.time_filter.is_some() {
        format!(" {}", card::CHECKMARK)
    } else {
        String::new()
    };
    lines.push(Line::from(vec![
        Span::styled("[f] ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(format!("Time: {}{}", time_label, time_indicator), time_style),
    ]));

    // Scheduled filter
    let scheduled_style = if app.hide_scheduled {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let scheduled_label = if app.hide_scheduled { "Hidden" } else { "Visible" };
    let scheduled_indicator = if app.hide_scheduled {
        format!(" {}", card::CHECKMARK)
    } else {
        String::new()
    };
    lines.push(Line::from(vec![
        Span::styled("[S] ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(format!("Scheduled: {}{}", scheduled_label, scheduled_indicator), scheduled_style),
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

pub fn render_projects_modal(f: &mut Frame, app: &App, area: Rect) {
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
