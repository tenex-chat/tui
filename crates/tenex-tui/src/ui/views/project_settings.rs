use crate::ui::app::fuzzy_matches;
use crate::ui::components::{
    modal_area, render_modal_background, render_modal_header, render_modal_overlay,
    render_modal_search, ModalSize,
};
use crate::ui::modal::ProjectSettingsState;
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

/// Render the project settings modal
pub fn render_project_settings(f: &mut Frame, app: &App, area: Rect, state: &ProjectSettingsState) {
    if state.in_add_mode {
        render_add_agent_mode(f, app, area, state);
    } else {
        render_main_settings(f, app, area, state);
    }
}

fn render_main_settings(f: &mut Frame, app: &App, area: Rect, state: &ProjectSettingsState) {
    let size = ModalSize {
        max_width: 70,
        height_percent: 0.7,
    };

    render_modal_overlay(f, area);
    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    let inner_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + 1,
        popup_area.width.saturating_sub(4),
        popup_area.height.saturating_sub(3),
    );

    // Header
    let title = format!("Settings: {}", state.project_name);
    let remaining = render_modal_header(f, inner_area, &title, "esc");

    // Agents section header
    let agents_header_area = Rect::new(remaining.x, remaining.y, remaining.width, 1);
    let agent_count = state.pending_agent_ids.len();
    let header_text = format!("Agents ({})", agent_count);
    let header = Paragraph::new(Line::from(vec![
        Span::styled(header_text, Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::ITALIC)),
    ]));
    f.render_widget(header, agents_header_area);

    // Agent list area
    let list_area = Rect::new(
        remaining.x,
        remaining.y + 2,
        remaining.width,
        remaining.height.saturating_sub(5),
    );

    if state.pending_agent_ids.is_empty() {
        let empty_msg = Paragraph::new("No agents assigned. Press 'a' to add agents.")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let items: Vec<ListItem> = state
            .pending_agent_ids
            .iter()
            .enumerate()
            .map(|(i, agent_id)| {
                let is_selected = i == state.selector_index;
                let is_pm = i == 0;

                // Try to get agent name from data store
                let agent_name = app
                    .data_store
                    .borrow()
                    .get_agent_definition(agent_id)
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| format!("{}...", &agent_id[..16.min(agent_id.len())]));

                let agent_role = app
                    .data_store
                    .borrow()
                    .get_agent_definition(agent_id)
                    .map(|a| a.role.clone())
                    .unwrap_or_else(|| "unknown".to_string());

                let author_pubkey = app
                    .data_store
                    .borrow()
                    .get_agent_definition(agent_id)
                    .map(|a| a.pubkey.clone());

                let mut spans = vec![];

                // Left border indicator using author color
                let border_color = author_pubkey
                    .as_ref()
                    .map(|pk| theme::user_color(pk))
                    .unwrap_or(theme::TEXT_MUTED);

                if is_selected {
                    spans.push(Span::styled("▌", Style::default().fg(border_color)));
                } else {
                    spans.push(Span::styled("│", Style::default().fg(border_color)));
                }

                // PM badge
                if is_pm {
                    spans.push(Span::styled("[PM] ", Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)));
                } else {
                    spans.push(Span::styled("     ", Style::default()));
                }

                // Agent name
                let name_style = if is_selected {
                    Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(format!("@{}", agent_name), name_style));

                // Role
                spans.push(Span::styled(
                    format!(" [{}]", agent_role),
                    Style::default().fg(theme::ACCENT_SPECIAL),
                ));

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);
    }

    // Hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let mut hint_spans = vec![
        Span::styled("a", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" add", Style::default().fg(theme::TEXT_MUTED)),
    ];

    if !state.pending_agent_ids.is_empty() {
        hint_spans.extend(vec![
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("d", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" remove", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("p", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" set PM", Style::default().fg(theme::TEXT_MUTED)),
        ]);
    }

    if state.has_changes() {
        hint_spans.extend(vec![
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(" save", Style::default().fg(theme::TEXT_MUTED)),
        ]);
    }

    hint_spans.extend(vec![
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)),
    ]);

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}

fn render_add_agent_mode(f: &mut Frame, app: &App, area: Rect, state: &ProjectSettingsState) {
    let size = ModalSize {
        max_width: 70,
        height_percent: 0.8,
    };

    render_modal_overlay(f, area);
    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    let inner_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + 1,
        popup_area.width.saturating_sub(4),
        popup_area.height.saturating_sub(3),
    );

    // Header
    let remaining = render_modal_header(f, inner_area, "Add Agent", "esc");

    // Search
    let remaining = render_modal_search(f, remaining, &state.add_filter, "Search agents...");

    // Get available agents (exclude already added)
    let filter = &state.add_filter;
    let available_agents: Vec<_> = app
        .data_store
        .borrow()
        .get_agent_definitions()
        .into_iter()
        .filter(|a| !state.pending_agent_ids.contains(&a.id))
        .filter(|a| {
            fuzzy_matches(&a.name, filter)
                || fuzzy_matches(&a.description, filter)
                || fuzzy_matches(&a.role, filter)
        })
        .cloned()
        .collect();

    // List area
    let list_area = Rect::new(
        remaining.x,
        remaining.y + 1,
        remaining.width,
        remaining.height.saturating_sub(4),
    );

    if available_agents.is_empty() {
        let msg = if state.add_filter.is_empty() {
            "No available agents found."
        } else {
            "No agents match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state.add_index.min(available_agents.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let items: Vec<ListItem> = available_agents
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, agent)| {
                let is_selected = i == selected_index;
                let border_color = theme::user_color(&agent.pubkey);

                let mut spans = vec![];

                // Left border
                if is_selected {
                    spans.push(Span::styled("▌", Style::default().fg(border_color)));
                } else {
                    spans.push(Span::styled("│", Style::default().fg(border_color)));
                }

                // Agent name
                let name_style = if is_selected {
                    Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(format!("@{}", agent.name), name_style));

                // Role
                spans.push(Span::styled(
                    format!(" [{}]", agent.role),
                    Style::default().fg(theme::ACCENT_SPECIAL),
                ));

                // Author
                let author_name = app.data_store.borrow().get_profile_name(&agent.pubkey);
                spans.push(Span::styled(
                    format!(" by {}", author_name),
                    Style::default().fg(theme::TEXT_MUTED),
                ));

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);

        // Show description of selected agent
        if let Some(agent) = available_agents.get(selected_index) {
            let desc_area = Rect::new(
                remaining.x,
                list_area.y + list_area.height,
                remaining.width,
                2,
            );
            let desc_preview = if agent.description.len() > 80 {
                format!("{}...", &agent.description[..77])
            } else {
                agent.description.clone()
            };
            let desc = Paragraph::new(desc_preview)
                .style(Style::default().fg(theme::TEXT_DIM))
                .block(Block::default().borders(Borders::NONE));
            f.render_widget(desc, desc_area);
        }
    }

    // Hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let hint_spans = vec![
        Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" navigate", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" add", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" back", Style::default().fg(theme::TEXT_MUTED)),
    ];

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}

/// Get count of available agents for add mode (for bounds checking)
pub fn available_agent_count(app: &App, state: &ProjectSettingsState) -> usize {
    let filter = &state.add_filter;
    app.data_store
        .borrow()
        .get_agent_definitions()
        .into_iter()
        .filter(|a| !state.pending_agent_ids.contains(&a.id))
        .filter(|a| {
            fuzzy_matches(&a.name, filter)
                || fuzzy_matches(&a.description, filter)
                || fuzzy_matches(&a.role, filter)
        })
        .count()
}

/// Get the agent ID at the given index in add mode
pub fn get_agent_id_at_index(app: &App, state: &ProjectSettingsState, index: usize) -> Option<String> {
    let filter = &state.add_filter;
    app.data_store
        .borrow()
        .get_agent_definitions()
        .into_iter()
        .filter(|a| !state.pending_agent_ids.contains(&a.id))
        .filter(|a| {
            fuzzy_matches(&a.name, filter)
                || fuzzy_matches(&a.description, filter)
                || fuzzy_matches(&a.role, filter)
        })
        .nth(index)
        .map(|a| a.id.clone())
}
