use crate::ui::components::{
    render_modal_items, render_modal_sections, Modal, ModalItem, ModalSection, ModalSize,
};
use crate::ui::format::truncate_with_ellipsis;
use crate::ui::modal::{ConversationAction, ConversationActionsState, ProjectActionsState};
use crate::ui::{card, theme, App, View};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, ListState, Paragraph},
    Frame,
};

/// Shared helper for rendering project selector modals
/// Used by both `render_projects_modal` and `render_composer_project_selector`
fn render_project_selector_modal_inner(
    f: &mut Frame,
    app: &App,
    area: Rect,
    title: &str,
    filter: &str,
    selected_index: usize,
    hints_text: &str,
) {
    let (popup_area, remaining) = Modal::new(title)
        .size(ModalSize {
            max_width: 65,
            height_percent: 0.7,
        })
        .search(filter, "Search projects...")
        .render_frame(f, area);

    // Build sections
    let data_store = app.data_store.borrow();
    let (online_projects, offline_projects) = app.filtered_projects();

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
    let hints = Paragraph::new(hints_text).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

pub fn render_projects_modal(f: &mut Frame, app: &App, area: Rect) {
    render_project_selector_modal_inner(
        f,
        app,
        area,
        "Switch Project",
        app.projects_modal_filter(),
        app.projects_modal_index(),
        "↑↓ navigate · enter select · esc close",
    );
}

/// Render the composer project selector modal (for changing project in new conversations)
/// This is used when starting a new conversation and wanting to change the a-tag
pub fn render_composer_project_selector(f: &mut Frame, app: &App, area: Rect) {
    render_project_selector_modal_inner(
        f,
        app,
        area,
        "Select Project for New Conversation",
        app.composer_project_selector_filter(),
        app.composer_project_selector_index(),
        "↑↓ navigate · enter select · esc cancel",
    );
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
    let home_marker = if home_active {
        card::BULLET
    } else {
        card::SPACER
    };
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

        let active_marker = if is_active {
            card::BULLET
        } else {
            card::SPACER
        };
        let text = format!("{}{} · {}", active_marker, project_name, title_display);

        // Tab number is i+2 (since 1 is Home)
        let shortcut = if i + 2 <= 9 {
            format!("{}", i + 2)
        } else {
            String::new()
        };

        items.push(
            ModalItem::new(text)
                .with_shortcut(shortcut)
                .selected(is_selected),
        );
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

        let placeholder = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
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
                            (
                                "T",
                                format!("{} - {}", title, truncate_with_ellipsis(excerpt, 30)),
                            )
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
                        (
                            "M",
                            format!("{} - {}", title, truncate_with_ellipsis(excerpt, 35)),
                        )
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
pub(super) fn render_project_actions_modal(f: &mut Frame, area: Rect, state: &ProjectActionsState) {
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

pub(super) fn render_conversation_actions_modal(
    f: &mut Frame,
    area: Rect,
    state: &ConversationActionsState,
) {
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
