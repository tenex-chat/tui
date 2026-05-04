//! Unified create/edit project dialog with three tabs: Details, Agents, MCP Servers.

use crate::ui::app::fuzzy_matches;
use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::{
    ProjectDialogDetailsFocus, ProjectDialogMode, ProjectDialogState, ProjectDialogTab,
};
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use std::collections::HashMap;
use tenex_core::models::AgentInventoryItem;

fn short_pubkey(pubkey: &str) -> String {
    if pubkey.len() <= 16 {
        pubkey.to_string()
    } else {
        format!("{}…{}", &pubkey[..8], &pubkey[pubkey.len() - 8..])
    }
}

fn inventory_agent_for_pubkey(app: &App, agent_pubkey: &str) -> Option<AgentInventoryItem> {
    app.agent_inventory_item(agent_pubkey)
}

/// Get agents for the add-agent picker, sorted with selected first.
fn add_mode_agents_dialog(app: &App, state: &ProjectDialogState) -> Vec<AgentInventoryItem> {
    let filter = &state.add_agent_filter;
    let pending_positions: HashMap<&str, usize> = state
        .pending_agent_pubkeys
        .iter()
        .enumerate()
        .map(|(index, pubkey)| (pubkey.as_str(), index))
        .collect();

    let mut agents = app.agent_inventory_filtered_by(filter);
    agents.sort_by(|left, right| {
        let left_pending = pending_positions.get(left.pubkey.as_str()).copied();
        let right_pending = pending_positions.get(right.pubkey.as_str()).copied();
        let left_name = app.agent_display_name(&left.pubkey);
        let right_name = app.agent_display_name(&right.pubkey);

        match (left_pending, right_pending) {
            (Some(left_index), Some(right_index)) => left_index
                .cmp(&right_index)
                .then_with(|| left_name.cmp(&right_name))
                .then_with(|| left.pubkey.cmp(&right.pubkey)),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => left_name
                .cmp(&right_name)
                .then_with(|| left.pubkey.cmp(&right.pubkey)),
        }
    });
    agents
}

/// Available agent count for the add-agent picker (used for input bounds checks).
pub fn available_agent_count_dialog(app: &App, state: &ProjectDialogState) -> usize {
    add_mode_agents_dialog(app, state).len()
}

/// Get the agent pubkey at the given index in the add-agent picker.
pub fn get_agent_id_at_dialog_index(
    app: &App,
    state: &ProjectDialogState,
    index: usize,
) -> Option<String> {
    add_mode_agents_dialog(app, state)
        .into_iter()
        .nth(index)
        .map(|agent| agent.pubkey)
}

/// Available MCP tool count for the add-tool picker.
pub fn available_mcp_tool_count_dialog(app: &App, state: &ProjectDialogState) -> usize {
    let filter = &state.add_tool_filter;
    app.data_store
        .borrow()
        .content
        .get_mcp_tools()
        .into_iter()
        .filter(|t| !state.pending_mcp_tool_ids.contains(&t.id))
        .filter(|t| fuzzy_matches(&t.name, filter) || fuzzy_matches(&t.description, filter))
        .count()
}

/// Get the MCP tool ID at the given index in the add-tool picker.
pub fn get_mcp_tool_id_at_dialog_index(
    app: &App,
    state: &ProjectDialogState,
    index: usize,
) -> Option<String> {
    let filter = &state.add_tool_filter;
    app.data_store
        .borrow()
        .content
        .get_mcp_tools()
        .into_iter()
        .filter(|t| !state.pending_mcp_tool_ids.contains(&t.id))
        .filter(|t| fuzzy_matches(&t.name, filter) || fuzzy_matches(&t.description, filter))
        .nth(index)
        .map(|t| t.id.clone())
}

/// Render the unified project dialog (Details / Agents / MCP Servers).
pub fn render_project_dialog(
    f: &mut Frame,
    app: &App,
    area: Rect,
    state: &mut ProjectDialogState,
) {
    // If we're in an add-sub-mode, render that as the primary content
    if state.in_add_agent_mode {
        render_add_agent_mode(f, app, area, state);
        return;
    }
    if state.in_add_tool_mode {
        render_add_mcp_tool_mode(f, app, area, state);
        return;
    }

    let title = match &state.mode {
        ProjectDialogMode::Creating => "New Project".to_string(),
        ProjectDialogMode::Editing { .. } => format!("Project Settings: {}", state.name),
    };

    let (popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 80,
            height_percent: 0.75,
        })
        .render_frame(f, area);

    // Padded content area
    let remaining = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height,
    );

    // Tab bar at top
    render_tab_bar(f, remaining, state);

    // Body content area (skip tab + blank line; reserve hints at bottom)
    let body_area = Rect::new(
        remaining.x,
        remaining.y + 2,
        remaining.width,
        remaining.height.saturating_sub(4),
    );

    match state.tab {
        ProjectDialogTab::Details => render_details_tab(f, body_area, state),
        ProjectDialogTab::Agents => render_agents_tab(f, app, body_area, state),
        ProjectDialogTab::McpServers => render_mcp_servers_tab(f, app, body_area, state),
    }

    // Hints at the bottom of the popup
    render_hints(f, popup_area, state);
}

fn render_tab_bar(f: &mut Frame, area: Rect, state: &ProjectDialogState) {
    let tab_area = Rect::new(area.x, area.y, area.width, 1);

    let mut spans = vec![];
    for (i, tab) in ProjectDialogTab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
        }

        let is_active = *tab == state.tab;
        let style = if is_active {
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        spans.push(Span::styled(tab.label(), style));
    }

    f.render_widget(Paragraph::new(Line::from(spans)), tab_area);
}

fn render_details_tab(f: &mut Frame, area: Rect, state: &ProjectDialogState) {
    let mut y = area.y;

    // Track positions where text inputs render so we can place a cursor there.
    let name_input_y;
    let desc_input_y;
    let repo_input_y;

    // ===== Name =====
    let name_label_style = if state.details_focus == ProjectDialogDetailsFocus::Name {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled("Name:", name_label_style)])),
        Rect::new(area.x, y, area.width, 1),
    );
    y += 1;

    let name_border = if state.details_focus == ProjectDialogDetailsFocus::Name {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };
    let name_value = if state.name.is_empty() {
        if state.details_focus == ProjectDialogDetailsFocus::Name {
            "Enter project name...".to_string()
        } else {
            "(required)".to_string()
        }
    } else {
        state.name.clone()
    };
    let name_value_style = if state.name.is_empty() {
        Style::default().fg(theme::TEXT_DIM)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("│ ", Style::default().fg(name_border)),
            Span::styled(name_value, name_value_style),
        ])),
        Rect::new(area.x, y, area.width, 1),
    );
    name_input_y = y;
    y += 2;

    // ===== Description =====
    let desc_label_style = if state.details_focus == ProjectDialogDetailsFocus::Description {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Description:",
            desc_label_style,
        )])),
        Rect::new(area.x, y, area.width, 1),
    );
    y += 1;

    let desc_border = if state.details_focus == ProjectDialogDetailsFocus::Description {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };
    let desc_value = if state.description.is_empty() {
        if state.details_focus == ProjectDialogDetailsFocus::Description {
            "Enter description (optional)...".to_string()
        } else {
            "(optional)".to_string()
        }
    } else {
        state.description.clone()
    };
    let desc_value_style = if state.description.is_empty() {
        Style::default().fg(theme::TEXT_DIM)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("│ ", Style::default().fg(desc_border)),
            Span::styled(desc_value, desc_value_style),
        ])),
        Rect::new(area.x, y, area.width, 1),
    );
    desc_input_y = y;
    y += 2;

    // ===== Repo URL =====
    let repo_label_style = if state.details_focus == ProjectDialogDetailsFocus::RepoUrl {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![Span::styled("Repo URL:", repo_label_style)])),
        Rect::new(area.x, y, area.width, 1),
    );
    y += 1;

    let repo_border = if state.details_focus == ProjectDialogDetailsFocus::RepoUrl {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };
    let repo_value = if state.repo_url.is_empty() {
        if state.details_focus == ProjectDialogDetailsFocus::RepoUrl {
            "https://github.com/owner/repo".to_string()
        } else {
            "(optional)".to_string()
        }
    } else {
        state.repo_url.clone()
    };
    let repo_value_style = if state.repo_url.is_empty() {
        Style::default().fg(theme::TEXT_DIM)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("│ ", Style::default().fg(repo_border)),
            Span::styled(repo_value, repo_value_style),
        ])),
        Rect::new(area.x, y, area.width, 1),
    );
    repo_input_y = y;
    y += 2;

    // ===== Private toggle =====
    let private_label_style = if state.details_focus == ProjectDialogDetailsFocus::Private {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let private_checkbox = if state.is_private { "[✓]" } else { "[ ]" };
    let private_value_style = if state.is_private {
        Style::default().fg(theme::ACCENT_SUCCESS)
    } else {
        Style::default().fg(theme::TEXT_DIM)
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Private: ", private_label_style),
            Span::styled(private_checkbox, private_value_style),
            Span::styled("  (Space to toggle)", Style::default().fg(theme::TEXT_DIM)),
        ])),
        Rect::new(area.x, y, area.width, 1),
    );
    y += 2;

    // Validation hint
    if state.name.trim().is_empty() {
        f.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("* ", Style::default().fg(theme::ACCENT_ERROR)),
                Span::styled(
                    "Project name is required",
                    Style::default().fg(theme::TEXT_DIM),
                ),
            ])),
            Rect::new(area.x, y, area.width, 1),
        );
    }

    // Cursor positioning for active text fields
    match state.details_focus {
        ProjectDialogDetailsFocus::Name => {
            f.set_cursor_position((area.x + 2 + state.name.len() as u16, name_input_y));
        }
        ProjectDialogDetailsFocus::Description => {
            f.set_cursor_position((area.x + 2 + state.description.len() as u16, desc_input_y));
        }
        ProjectDialogDetailsFocus::RepoUrl => {
            f.set_cursor_position((area.x + 2 + state.repo_url.len() as u16, repo_input_y));
        }
        ProjectDialogDetailsFocus::Private => {}
    }
}

fn render_agents_tab(f: &mut Frame, app: &App, area: Rect, state: &mut ProjectDialogState) {
    // Header line
    let header_text = format!("Agents ({})", state.pending_agent_pubkeys.len());
    let header = Paragraph::new(Line::from(vec![Span::styled(
        header_text,
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )]));
    f.render_widget(header, Rect::new(area.x, area.y, area.width, 1));

    // List area below header
    let visible_height = area.height.saturating_sub(2) as usize;
    state.cached_agents_visible_height = visible_height;
    let list_area = Rect::new(
        area.x,
        area.y + 2,
        area.width,
        visible_height as u16,
    );

    if state.pending_agent_pubkeys.is_empty() {
        let empty_msg = Paragraph::new("No agents assigned. Press 'a' to add.")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
        return;
    }

    let scroll_offset = state.agents_scroll_offset;
    let items: Vec<ListItem> = state
        .pending_agent_pubkeys
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(i, agent_pubkey)| {
            let is_selected = i == state.agents_selector_index;
            let is_pm = i == 0;
            let inventory_agent = inventory_agent_for_pubkey(app, agent_pubkey);

            let mut spans = vec![];

            let border_color = theme::user_color(agent_pubkey);

            if is_selected {
                spans.push(Span::styled("▌", Style::default().fg(border_color)));
            } else {
                spans.push(Span::styled("│", Style::default().fg(border_color)));
            }

            // PM badge
            if is_pm {
                spans.push(Span::styled(
                    "[PM] ",
                    Style::default()
                        .fg(theme::ACCENT_WARNING)
                        .add_modifier(Modifier::BOLD),
                ));
            } else {
                spans.push(Span::styled("     ", Style::default()));
            }

            let agent_name = inventory_agent
                .as_ref()
                .map(|agent| app.agent_display_name(&agent.pubkey))
                .unwrap_or_else(|| app.agent_display_name(agent_pubkey));
            let name_style = if is_selected {
                Style::default()
                    .fg(theme::ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT_PRIMARY)
            };
            spans.push(Span::styled(agent_name, name_style));

            spans.push(Span::styled(
                format!(" [{}]", short_pubkey(agent_pubkey)),
                Style::default().fg(theme::ACCENT_SPECIAL),
            ));

            if let Some(agent) = &inventory_agent {
                if agent.is_multi_backend {
                    spans.push(Span::styled(
                        format!(" [⚠ {} backends]", agent.backends.len()),
                        Style::default().fg(theme::ACCENT_ERROR),
                    ));
                }
            }

            ListItem::new(Line::from(spans))
        })
        .collect();

    f.render_widget(List::new(items), list_area);
}

fn render_mcp_servers_tab(f: &mut Frame, app: &App, area: Rect, state: &mut ProjectDialogState) {
    // Header line
    let header_text = format!("MCP Servers ({})", state.pending_mcp_tool_ids.len());
    let header = Paragraph::new(Line::from(vec![Span::styled(
        header_text,
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD),
    )]));
    f.render_widget(header, Rect::new(area.x, area.y, area.width, 1));

    let visible_height = area.height.saturating_sub(2) as usize;
    let list_area = Rect::new(
        area.x,
        area.y + 2,
        area.width,
        visible_height as u16,
    );

    if state.pending_mcp_tool_ids.is_empty() {
        let empty_msg = Paragraph::new("No tools. Press 't' to add.")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
        return;
    }

    let scroll_offset = state.tools_scroll_offset;
    let items: Vec<ListItem> = state
        .pending_mcp_tool_ids
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(i, tool_id)| {
            let is_selected = i == state.tools_selector_index;

            let tool_name = app
                .data_store
                .borrow()
                .content
                .get_mcp_tool(tool_id)
                .map(|t| t.name.clone())
                .unwrap_or_else(|| format!("{}...", &tool_id[..16.min(tool_id.len())]));

            let mut spans = vec![];

            if is_selected {
                spans.push(Span::styled(
                    "▌",
                    Style::default().fg(theme::ACCENT_PRIMARY),
                ));
            } else {
                spans.push(Span::styled("│", Style::default().fg(theme::TEXT_MUTED)));
            }

            let name_style = if is_selected {
                Style::default()
                    .fg(theme::ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme::TEXT_PRIMARY)
            };
            spans.push(Span::styled(format!(" {}", tool_name), name_style));

            ListItem::new(Line::from(spans))
        })
        .collect();

    f.render_widget(List::new(items), list_area);
}

fn render_hints(f: &mut Frame, popup_area: Rect, state: &ProjectDialogState) {
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let mut hint_spans: Vec<Span> = Vec::new();

    let push_sep = |spans: &mut Vec<Span>| {
        spans.push(Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)));
    };

    match state.tab {
        ProjectDialogTab::Details => {
            hint_spans.push(Span::styled(
                "Tab",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
            hint_spans.push(Span::styled(
                " cycle field",
                Style::default().fg(theme::TEXT_MUTED),
            ));
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled(
                "← →",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
            hint_spans.push(Span::styled(
                " switch tab",
                Style::default().fg(theme::TEXT_MUTED),
            ));
            if state.can_save() {
                push_sep(&mut hint_spans);
                hint_spans.push(Span::styled(
                    "Enter",
                    Style::default().fg(theme::ACCENT_SUCCESS),
                ));
                hint_spans.push(Span::styled(" save", Style::default().fg(theme::TEXT_MUTED)));
            }
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled(
                "Esc",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
            hint_spans.push(Span::styled(
                " cancel",
                Style::default().fg(theme::TEXT_MUTED),
            ));
        }
        ProjectDialogTab::Agents => {
            hint_spans.push(Span::styled(
                "↑↓",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
            hint_spans.push(Span::styled(
                " navigate",
                Style::default().fg(theme::TEXT_MUTED),
            ));
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled("a", Style::default().fg(theme::ACCENT_WARNING)));
            hint_spans.push(Span::styled(
                " add agent",
                Style::default().fg(theme::TEXT_MUTED),
            ));
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled("d", Style::default().fg(theme::ACCENT_WARNING)));
            hint_spans.push(Span::styled(" remove", Style::default().fg(theme::TEXT_MUTED)));
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled("p", Style::default().fg(theme::ACCENT_WARNING)));
            hint_spans.push(Span::styled(" set PM", Style::default().fg(theme::TEXT_MUTED)));
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled(
                "← →",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
            hint_spans.push(Span::styled(
                " switch tab",
                Style::default().fg(theme::TEXT_MUTED),
            ));
            if state.can_save() && state.has_changes() {
                push_sep(&mut hint_spans);
                hint_spans.push(Span::styled(
                    "Enter",
                    Style::default().fg(theme::ACCENT_SUCCESS),
                ));
                hint_spans.push(Span::styled(" save", Style::default().fg(theme::TEXT_MUTED)));
            }
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled(
                "Esc",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
            hint_spans.push(Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)));
        }
        ProjectDialogTab::McpServers => {
            hint_spans.push(Span::styled(
                "↑↓",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
            hint_spans.push(Span::styled(
                " navigate",
                Style::default().fg(theme::TEXT_MUTED),
            ));
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled("t", Style::default().fg(theme::ACCENT_WARNING)));
            hint_spans.push(Span::styled(
                " add tool",
                Style::default().fg(theme::TEXT_MUTED),
            ));
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled("d", Style::default().fg(theme::ACCENT_WARNING)));
            hint_spans.push(Span::styled(" remove", Style::default().fg(theme::TEXT_MUTED)));
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled(
                "← →",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
            hint_spans.push(Span::styled(
                " switch tab",
                Style::default().fg(theme::TEXT_MUTED),
            ));
            if state.can_save() && state.has_changes() {
                push_sep(&mut hint_spans);
                hint_spans.push(Span::styled(
                    "Enter",
                    Style::default().fg(theme::ACCENT_SUCCESS),
                ));
                hint_spans.push(Span::styled(" save", Style::default().fg(theme::TEXT_MUTED)));
            }
            push_sep(&mut hint_spans);
            hint_spans.push(Span::styled(
                "Esc",
                Style::default().fg(theme::ACCENT_WARNING),
            ));
            hint_spans.push(Span::styled(" close", Style::default().fg(theme::TEXT_MUTED)));
        }
    }

    f.render_widget(Paragraph::new(Line::from(hint_spans)), hints_area);
}

// =============================================================================
// Sub-modals: agent picker and MCP tool picker (overlays in their own modals)
// =============================================================================

fn render_add_agent_mode(f: &mut Frame, app: &App, area: Rect, state: &ProjectDialogState) {
    let title = format!("Project Agents ({})", state.pending_agent_pubkeys.len());
    let (popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 70,
            height_percent: 0.8,
        })
        .search(&state.add_agent_filter, "Search agents...")
        .render_frame(f, area);

    let remaining = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height,
    );

    let available_agents = add_mode_agents_dialog(app, state);

    let list_area = Rect::new(
        remaining.x,
        remaining.y + 1,
        remaining.width,
        remaining.height.saturating_sub(4),
    );

    if available_agents.is_empty() {
        let desc_area = Rect::new(
            remaining.x,
            list_area.y + list_area.height,
            remaining.width,
            2,
        );
        if state.pubkey_input_active {
            render_pubkey_input(f, desc_area, state);
        } else {
            let msg = if state.add_agent_filter.is_empty() {
                "No agents in catalog. Use ^A to add by pubkey."
            } else {
                "No agents match your search."
            };
            let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
            f.render_widget(empty_msg, list_area);
        }
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state
            .add_agent_index
            .min(available_agents.len().saturating_sub(1));

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
                let is_cursor = i == selected_index;
                let is_checked = state.pending_agent_pubkeys.contains(&agent.pubkey);
                let is_pm = state
                    .pending_agent_pubkeys
                    .first()
                    .is_some_and(|pubkey| pubkey == &agent.pubkey);

                let mut spans = vec![];

                if is_cursor {
                    spans.push(Span::styled(
                        "▌ ",
                        Style::default().fg(theme::ACCENT_PRIMARY),
                    ));
                } else {
                    spans.push(Span::styled("  ", Style::default()));
                }

                let checkbox = if is_checked { "[✓]" } else { "[ ]" };
                let checkbox_style = if is_checked {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_MUTED)
                };
                spans.push(Span::styled(checkbox, checkbox_style));
                spans.push(Span::styled(" ", Style::default()));

                if is_pm {
                    spans.push(Span::styled(
                        "[PM] ",
                        Style::default()
                            .fg(theme::ACCENT_WARNING)
                            .add_modifier(Modifier::BOLD),
                    ));
                }

                let name_style = if is_cursor {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else if is_checked {
                    Style::default().fg(theme::TEXT_PRIMARY)
                } else {
                    Style::default().fg(theme::TEXT_MUTED)
                };
                spans.push(Span::styled(
                    app.agent_display_name(&agent.pubkey),
                    name_style,
                ));
                let backend_count = agent.backends.len();
                let (backend_label, backend_label_style) = if agent.is_multi_backend {
                    (
                        format!(" [⚠ {} backends]", backend_count),
                        Style::default().fg(theme::ACCENT_ERROR),
                    )
                } else {
                    (
                        format!(" [{}]", app.agent_inventory_backend_label(agent)),
                        Style::default().fg(theme::TEXT_MUTED),
                    )
                };
                spans.push(Span::styled(backend_label, backend_label_style));

                let row_style = if is_cursor {
                    Style::default().bg(theme::BG_SELECTED)
                } else {
                    Style::default()
                };

                ListItem::new(Line::from(spans)).style(row_style)
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);

        let desc_area = Rect::new(
            remaining.x,
            list_area.y + list_area.height,
            remaining.width,
            2,
        );

        if state.pubkey_input_active {
            render_pubkey_input(f, desc_area, state);
        } else if let Some(agent) = available_agents.get(selected_index) {
            let status = if state.pending_agent_pubkeys.contains(&agent.pubkey) {
                if state
                    .pending_agent_pubkeys
                    .first()
                    .is_some_and(|pubkey| pubkey == &agent.pubkey)
                {
                    "Selected · PM"
                } else {
                    "Selected"
                }
            } else {
                "Not selected"
            };
            let backend_count = agent.backends.len();
            let backend_info = if agent.is_multi_backend {
                format!("⚠ {} backends have this agent", backend_count)
            } else {
                format!("Backend: {}", app.agent_inventory_backend_label(agent))
            };
            let desc_style = if agent.is_multi_backend {
                Style::default().fg(theme::ACCENT_ERROR)
            } else {
                Style::default().fg(theme::TEXT_DIM)
            };
            let desc = Paragraph::new(format!("{} · {}", status, backend_info))
                .style(desc_style)
                .block(Block::default().borders(Borders::NONE));
            f.render_widget(desc, desc_area);
        }
    }

    // Hints
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let hint_spans = if state.pubkey_input_active {
        vec![
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(" add pubkey", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
        ]
    } else {
        vec![
            Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" navigate", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Space", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" toggle", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("^A", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" add by pubkey", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" done", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" back", Style::default().fg(theme::TEXT_MUTED)),
        ]
    };

    f.render_widget(Paragraph::new(Line::from(hint_spans)), hints_area);
}

fn render_pubkey_input(f: &mut Frame, area: Rect, state: &ProjectDialogState) {
    if area.height == 0 {
        return;
    }

    let input_line_area = Rect::new(area.x, area.y, area.width, 1);
    let label = "npub or hex: ";
    let cursor = if (std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        / 500)
        % 2
        == 0
    {
        "█"
    } else {
        " "
    };
    f.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(label, Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(
                &state.pubkey_input,
                Style::default()
                    .fg(theme::ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(cursor, Style::default().fg(theme::ACCENT_PRIMARY)),
        ])),
        input_line_area,
    );

    if area.height >= 2 {
        let error_area = Rect::new(area.x, area.y + 1, area.width, 1);
        if let Some(ref error) = state.pubkey_input_error {
            f.render_widget(
                Paragraph::new(error.as_str()).style(Style::default().fg(theme::ACCENT_ERROR)),
                error_area,
            );
        }
    }
}

fn render_add_mcp_tool_mode(f: &mut Frame, app: &App, area: Rect, state: &ProjectDialogState) {
    let (popup_area, content_area) = Modal::new("Add MCP Tool")
        .size(ModalSize {
            max_width: 70,
            height_percent: 0.8,
        })
        .search(&state.add_tool_filter, "Search MCP tools...")
        .render_frame(f, area);

    let remaining = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height,
    );

    let filter = &state.add_tool_filter;
    let available_tools: Vec<_> = app
        .data_store
        .borrow()
        .content
        .get_mcp_tools()
        .into_iter()
        .filter(|t| !state.pending_mcp_tool_ids.contains(&t.id))
        .filter(|t| fuzzy_matches(&t.name, filter) || fuzzy_matches(&t.description, filter))
        .cloned()
        .collect();

    let list_area = Rect::new(
        remaining.x,
        remaining.y + 1,
        remaining.width,
        remaining.height.saturating_sub(4),
    );

    if available_tools.is_empty() {
        let msg = if state.add_tool_filter.is_empty() {
            "No available MCP tools found."
        } else {
            "No MCP tools match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state
            .add_tool_index
            .min(available_tools.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let items: Vec<ListItem> = available_tools
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, tool)| {
                let is_selected = i == selected_index;

                let mut spans = vec![];

                if is_selected {
                    spans.push(Span::styled(
                        "▌",
                        Style::default().fg(theme::ACCENT_PRIMARY),
                    ));
                } else {
                    spans.push(Span::styled("│", Style::default().fg(theme::TEXT_MUTED)));
                }

                let name_style = if is_selected {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(&tool.name, name_style));

                let author_name = app.data_store.borrow().get_profile_name(&tool.pubkey);
                spans.push(Span::styled(
                    format!(" by {}", author_name),
                    Style::default().fg(theme::TEXT_MUTED),
                ));

                ListItem::new(Line::from(spans))
            })
            .collect();

        f.render_widget(List::new(items), list_area);

        if let Some(tool) = available_tools.get(selected_index) {
            let desc_area = Rect::new(
                remaining.x,
                list_area.y + list_area.height,
                remaining.width,
                2,
            );
            let desc_preview = if tool.description.chars().count() > 80 {
                format!(
                    "{}...",
                    tool.description.chars().take(77).collect::<String>()
                )
            } else {
                tool.description.clone()
            };
            let desc = Paragraph::new(desc_preview)
                .style(Style::default().fg(theme::TEXT_DIM))
                .block(Block::default().borders(Borders::NONE));
            f.render_widget(desc, desc_area);
        }
    }

    // Hints
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

    f.render_widget(Paragraph::new(Line::from(hint_spans)), hints_area);
}
