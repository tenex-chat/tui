mod content;
mod modals;
mod sidebar;

pub use crate::ui::views::home_helpers::HierarchicalThread;
pub use content::get_hierarchical_threads;
pub use modals::{render_projects_modal, render_search_modal, render_tab_modal};
pub use sidebar::{get_project_at_index, selectable_project_count};

use crate::ui::components::{render_statusbar, render_tab_bar};
use crate::ui::modal::ModalState;
use crate::ui::{layout, theme, App, HomeTab};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Paragraph},
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
        Constraint::Min(0),                             // Content
        Constraint::Length(layout::SIDEBAR_WIDTH_HOME), // Sidebar (fixed width, on RIGHT)
    ])
    .split(chunks[1]);

    // Render content based on active tab (with consistent padding)
    let content_area = main_chunks[0];
    let padded_content = layout::with_content_padding(content_area);

    // If sidebar search is visible with a query, show search results instead
    if app.sidebar_search.visible && app.sidebar_search.has_query() {
        sidebar::render_sidebar_search_results(f, app, padded_content);
    } else {
        match app.home_panel_focus {
            HomeTab::Conversations => content::render_conversations_with_feed(f, app, padded_content),
            HomeTab::Inbox => content::render_inbox_cards(f, app, padded_content),
            HomeTab::Reports => content::render_reports_list(f, app, padded_content),
            HomeTab::Feed => content::render_feed_cards(f, app, padded_content),
            HomeTab::ActiveWork => content::render_active_work(f, app, padded_content),
            HomeTab::Stats => super::render_stats(f, app, padded_content),
        }
    }

    // Render sidebar on the right
    sidebar::render_project_sidebar(f, app, main_chunks[1]);

    // Bottom padding
    sidebar::render_bottom_padding(f, chunks[2]);

    // Open tabs bar (if tabs exist)
    if has_tabs {
        render_tab_bar(f, app, chunks[3]);
    }

    // Status bar at the very bottom (always visible)
    let statusbar_area = if has_tabs { chunks[4] } else { chunks[3] };
    let (cumulative_runtime_ms, has_active_agents, active_agent_count) =
        app.data_store.borrow_mut().get_statusbar_runtime_ms();
    let audio_playing = app.audio_player.is_playing();
    render_statusbar(
        f,
        statusbar_area,
        app.current_notification(),
        cumulative_runtime_ms,
        has_active_agents,
        active_agent_count,
        app.wave_offset(),
        audio_playing,
    );

    // Projects modal overlay
    if matches!(app.modal_state, ModalState::ProjectsModal { .. }) {
        modals::render_projects_modal(f, app, area);
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
        modals::render_project_actions_modal(f, area, state);
    }

    // Conversation actions modal overlay
    if let ModalState::ConversationActions(ref state) = app.modal_state {
        modals::render_conversation_actions_modal(f, area, state);
    }

    // Report viewer modal overlay
    if let ModalState::ReportViewer(ref state) = app.modal_state {
        super::render_report_viewer(f, app, area, state);
    }

    // Tab modal overlay (Alt+/)
    if app.showing_tab_modal() {
        modals::render_tab_modal(f, app, area);
    }

    // Search modal overlay (/)
    if app.showing_search_modal {
        modals::render_search_modal(f, app, area);
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
        super::render_workspace_manager(
            f,
            area,
            state,
            &workspaces,
            &projects,
            active_id.as_deref(),
        );
    }

    // App settings modal
    if let ModalState::AppSettings(ref state) = app.modal_state {
        super::render_app_settings(f, app, area, state);
    }
}

/// Build a tab badge span and its display width for counts > 0
fn build_tab_badge(count: usize, color: ratatui::style::Color) -> (Option<Span<'static>>, usize) {
    if count > 0 {
        let text = format!(" ({})", count);
        let width = text.width();
        (Some(Span::styled(text, Style::default().fg(color))), width)
    } else {
        (None, 0)
    }
}

fn render_tab_header(f: &mut Frame, app: &App, area: Rect) {
    let inbox_count = app.inbox_items().iter().filter(|i| !i.is_read).count();
    let active_count = app.data_store.borrow().active_operations_count();

    let (inbox_badge, inbox_badge_width) = build_tab_badge(inbox_count, theme::ACCENT_ERROR);
    let (active_badge, active_badge_width) = build_tab_badge(active_count, theme::ACCENT_SUCCESS);

    let tab_style = |tab: HomeTab| {
        if app.home_panel_focus == tab {
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        }
    };

    let tabs = vec![
        (HomeTab::Conversations, "Conversations"),
        (HomeTab::Inbox, "Inbox"),
        (HomeTab::Reports, "Reports"),
        (HomeTab::Feed, "Feed"),
        (HomeTab::ActiveWork, "Active"),
        (HomeTab::Stats, "Stats"),
    ];

    let spans: Vec<Span<'static>> = tabs
        .iter()
        .flat_map(|(tab, label)| {
            let style = tab_style(*tab);
            let mut items = vec![Span::styled(format!(" {} ", label), style)];
            if *tab == HomeTab::Inbox {
                if let Some(badge) = inbox_badge.clone() {
                    items.push(badge);
                    items.push(Span::raw(" "));
                }
            }
            if *tab == HomeTab::ActiveWork {
                if let Some(badge) = active_badge.clone() {
                    items.push(badge);
                    items.push(Span::raw(" "));
                }
            }
            items
        })
        .collect();

    let tab_bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(theme::BG_APP));
    f.render_widget(tab_bar, area);

    let mut offset = 0usize;
    for (tab, label) in tabs {
        let mut width = label.width() + 2;
        if tab == HomeTab::Inbox {
            width += inbox_badge_width;
            if inbox_badge_width > 0 {
                width += 1;
            }
        }
        if tab == HomeTab::ActiveWork {
            width += active_badge_width;
            if active_badge_width > 0 {
                width += 1;
            }
        }

        if app.home_panel_focus == tab {
            let highlight_rect = Rect {
                x: area.x + offset as u16,
                y: area.y + 1,
                width: width as u16,
                height: 1,
            };
            let highlight = Block::default().style(Style::default().bg(theme::ACCENT_PRIMARY));
            f.render_widget(highlight, highlight_rect);
        }

        offset += width;
    }
}
