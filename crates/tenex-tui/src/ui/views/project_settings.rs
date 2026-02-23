use crate::ui::app::fuzzy_matches;
use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::{ProjectSettingsAddMode, ProjectSettingsFocus, ProjectSettingsState};
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};

/// Render the project settings modal
pub fn render_project_settings(
    f: &mut Frame,
    app: &App,
    area: Rect,
    state: &mut ProjectSettingsState,
) {
    match state.in_add_mode {
        Some(ProjectSettingsAddMode::Agent) => render_add_agent_mode(f, app, area, state),
        Some(ProjectSettingsAddMode::McpTool) => render_add_mcp_tool_mode(f, app, area, state),
        None => render_main_settings(f, app, area, state),
    }
}

/// Minimum width for side-by-side (horizontal) layout.
/// Below this, we switch to single-pane (vertical) layout showing only the focused pane.
const MIN_SIDE_BY_SIDE_WIDTH: u16 = 60;

fn render_main_settings(f: &mut Frame, app: &App, area: Rect, state: &mut ProjectSettingsState) {
    let title = format!("Settings: {}", state.project_name);

    let (popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 90,
            height_percent: 0.7,
        })
        .render_frame(f, area);

    // Content area with horizontal padding
    let remaining = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height,
    );

    // Calculate available height for content (reserve space for hints)
    let content_height = remaining.height.saturating_sub(3);

    let agents_focused = state.focus == ProjectSettingsFocus::Agents;
    let tools_focused = state.focus == ProjectSettingsFocus::Tools;

    // Determine layout mode based on available width
    let use_side_by_side = remaining.width >= MIN_SIDE_BY_SIDE_WIDTH;

    if use_side_by_side {
        // === HORIZONTAL LAYOUT (side-by-side panes) ===
        render_side_by_side_layout(
            f,
            app,
            remaining,
            content_height,
            state,
            popup_area,
            agents_focused,
            tools_focused,
        );
    } else {
        // === VERTICAL LAYOUT (single pane, narrow terminal fallback) ===
        render_single_pane_layout(
            f,
            app,
            remaining,
            content_height,
            state,
            popup_area,
            agents_focused,
            tools_focused,
        );
    }
}

/// Render horizontal side-by-side layout for agents and tools panes
fn render_side_by_side_layout(
    f: &mut Frame,
    app: &App,
    remaining: Rect,
    content_height: u16,
    state: &mut ProjectSettingsState,
    popup_area: Rect,
    agents_focused: bool,
    tools_focused: bool,
) {
    // Split horizontally: left = agents, right = tools
    let agents_width = remaining.width / 2;
    let tools_width = remaining
        .width
        .saturating_sub(agents_width)
        .saturating_sub(1); // -1 for gap

    // === Agents pane (left side) ===
    let agents_header_area = Rect::new(remaining.x, remaining.y, agents_width, 1);
    let agent_count = state.pending_agent_definition_ids.len();
    let header_text = format!("Agents ({})", agent_count);
    let header_style = if agents_focused {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme::ACCENT_WARNING)
            .add_modifier(Modifier::ITALIC)
    };
    let header = Paragraph::new(Line::from(vec![Span::styled(header_text, header_style)]));
    f.render_widget(header, agents_header_area);

    // Agent list area - compute height from actual layout
    let agents_list_height = content_height.saturating_sub(2) as usize;
    // Cache the computed visible height for use by input handlers
    state.set_visible_height(agents_list_height);
    let agents_list_area = Rect::new(
        remaining.x,
        remaining.y + 2,
        agents_width,
        agents_list_height as u16,
    );

    render_agents_list(
        f,
        app,
        agents_list_area,
        state,
        agents_focused,
        agents_list_height,
    );

    // === Tools pane (right side) ===
    let tools_x = remaining.x + agents_width + 1; // +1 for gap
    let tools_header_area = Rect::new(tools_x, remaining.y, tools_width, 1);
    let tool_count = state.pending_mcp_tool_ids.len();
    let tools_header_text = format!("MCP Tools ({})", tool_count);
    let tools_header_style = if tools_focused {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(theme::ACCENT_WARNING)
            .add_modifier(Modifier::ITALIC)
    };
    let tools_header = Paragraph::new(Line::from(vec![Span::styled(
        tools_header_text,
        tools_header_style,
    )]));
    f.render_widget(tools_header, tools_header_area);

    // MCP Tools list area
    let tools_list_height = content_height.saturating_sub(2) as usize;
    let tools_list_area = Rect::new(
        tools_x,
        remaining.y + 2,
        tools_width,
        tools_list_height as u16,
    );

    render_tools_list(
        f,
        app,
        tools_list_area,
        state,
        tools_focused,
        tools_list_height,
    );

    // Hints at bottom
    render_hints(f, popup_area, state, agents_focused, tools_focused);
}

/// Render vertical single-pane layout (narrow terminal fallback)
fn render_single_pane_layout(
    f: &mut Frame,
    app: &App,
    remaining: Rect,
    content_height: u16,
    state: &mut ProjectSettingsState,
    popup_area: Rect,
    agents_focused: bool,
    tools_focused: bool,
) {
    let pane_width = remaining.width;
    let list_height = content_height.saturating_sub(2) as usize;
    // Cache the computed visible height for use by input handlers
    state.set_visible_height(list_height);

    // Show pane indicator at the top
    let indicator_text = if agents_focused {
        format!(
            "◀ Agents ({}) ▶ Tools",
            state.pending_agent_definition_ids.len()
        )
    } else {
        format!("◀ Agents   ▶ Tools ({})", state.pending_mcp_tool_ids.len())
    };
    let indicator = Paragraph::new(indicator_text).style(Style::default().fg(theme::TEXT_MUTED));
    let indicator_area = Rect::new(remaining.x, remaining.y, pane_width, 1);
    f.render_widget(indicator, indicator_area);

    // Header for current pane
    let header_area = Rect::new(remaining.x, remaining.y + 1, pane_width, 1);
    if agents_focused {
        let header_text = format!("Agents ({})", state.pending_agent_definition_ids.len());
        let header = Paragraph::new(header_text).style(
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        );
        f.render_widget(header, header_area);

        let list_area = Rect::new(remaining.x, remaining.y + 3, pane_width, list_height as u16);
        render_agents_list(f, app, list_area, state, true, list_height);
    } else {
        let header_text = format!("MCP Tools ({})", state.pending_mcp_tool_ids.len());
        let header = Paragraph::new(header_text).style(
            Style::default()
                .fg(theme::ACCENT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        );
        f.render_widget(header, header_area);

        let list_area = Rect::new(remaining.x, remaining.y + 3, pane_width, list_height as u16);
        render_tools_list(f, app, list_area, state, true, list_height);
    }

    // Hints at bottom
    render_hints(f, popup_area, state, agents_focused, tools_focused);
}

/// Render the agents list
fn render_agents_list(
    f: &mut Frame,
    app: &App,
    list_area: Rect,
    state: &ProjectSettingsState,
    show_selection: bool,
    visible_height: usize,
) {
    if state.pending_agent_definition_ids.is_empty() {
        let empty_msg = Paragraph::new("No agents. Press 'a' to add.")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let scroll_offset = state.agents_scroll_offset;
        let items: Vec<ListItem> = state
            .pending_agent_definition_ids
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, agent_id)| {
                let is_selected = show_selection && i == state.selector_index;
                let is_pm = i == 0;

                // Try to get agent name from data store
                let agent_name = app
                    .data_store
                    .borrow()
                    .content
                    .get_agent_definition(agent_id)
                    .map(|a| a.name.clone())
                    .unwrap_or_else(|| format!("{}...", &agent_id[..16.min(agent_id.len())]));

                let agent_role = app
                    .data_store
                    .borrow()
                    .content
                    .get_agent_definition(agent_id)
                    .map(|a| a.role.clone())
                    .unwrap_or_else(|| "unknown".to_string());

                let author_pubkey = app
                    .data_store
                    .borrow()
                    .content
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
                    spans.push(Span::styled(
                        "[PM] ",
                        Style::default()
                            .fg(theme::ACCENT_WARNING)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    spans.push(Span::styled("     ", Style::default()));
                }

                // Agent name
                let name_style = if is_selected {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(format!("@{}", agent_name), name_style));

                // Role (truncate if needed for side-by-side layout)
                // Use char-based truncation to avoid panics on non-ASCII characters
                let role_display = if agent_role.chars().count() > 10 {
                    format!(" [{}…]", agent_role.chars().take(9).collect::<String>())
                } else {
                    format!(" [{}]", agent_role)
                };
                spans.push(Span::styled(
                    role_display,
                    Style::default().fg(theme::ACCENT_SPECIAL),
                ));

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);
    }
}

/// Render the tools list
fn render_tools_list(
    f: &mut Frame,
    app: &App,
    list_area: Rect,
    state: &ProjectSettingsState,
    show_selection: bool,
    visible_height: usize,
) {
    if state.pending_mcp_tool_ids.is_empty() {
        let empty_msg = Paragraph::new("No tools. Press 't' to add.")
            .style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let scroll_offset = state.tools_scroll_offset;
        let tool_items: Vec<ListItem> = state
            .pending_mcp_tool_ids
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, tool_id)| {
                let is_selected = show_selection && i == state.tools_selector_index;

                // Try to get tool name from data store
                let tool_name = app
                    .data_store
                    .borrow()
                    .content
                    .get_mcp_tool(tool_id)
                    .map(|t| t.name.clone())
                    .unwrap_or_else(|| format!("{}...", &tool_id[..16.min(tool_id.len())]));

                let mut spans = vec![];

                // Left border indicator
                if is_selected {
                    spans.push(Span::styled(
                        "▌",
                        Style::default().fg(theme::ACCENT_PRIMARY),
                    ));
                } else {
                    spans.push(Span::styled("│", Style::default().fg(theme::TEXT_MUTED)));
                }

                // Tool name
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

        let tools_list = List::new(tool_items);
        f.render_widget(tools_list, list_area);
    }
}

/// Render the hints bar at the bottom
fn render_hints(
    f: &mut Frame,
    popup_area: Rect,
    state: &ProjectSettingsState,
    agents_focused: bool,
    tools_focused: bool,
) {
    // Hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let mut hint_spans = vec![
        Span::styled("←→", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" switch pane", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("a", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" add agent", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("t", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" add tool", Style::default().fg(theme::TEXT_MUTED)),
    ];

    // Show context-sensitive hints based on focus
    if agents_focused && !state.pending_agent_definition_ids.is_empty() {
        hint_spans.extend(vec![
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("d", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" remove", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("p", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" set PM", Style::default().fg(theme::TEXT_MUTED)),
        ]);
    } else if tools_focused && !state.pending_mcp_tool_ids.is_empty() {
        hint_spans.extend(vec![
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("d", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" remove", Style::default().fg(theme::TEXT_MUTED)),
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
    let (popup_area, content_area) = Modal::new("Add Agent")
        .size(ModalSize {
            max_width: 70,
            height_percent: 0.8,
        })
        .search(&state.add_filter, "Search agents...")
        .render_frame(f, area);

    // Content area with horizontal padding
    let remaining = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height,
    );

    // Get available agents (exclude already added)
    let filter = &state.add_filter;
    let available_agents: Vec<_> = app
        .data_store
        .borrow()
        .content
        .get_agent_definitions()
        .into_iter()
        .filter(|a| !state.pending_agent_definition_ids.contains(&a.id))
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
        let selected_index = state
            .add_index
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
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
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
            let desc_preview = if agent.description.chars().count() > 80 {
                format!(
                    "{}...",
                    agent.description.chars().take(77).collect::<String>()
                )
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
        .content
        .get_agent_definitions()
        .into_iter()
        .filter(|a| !state.pending_agent_definition_ids.contains(&a.id))
        .filter(|a| {
            fuzzy_matches(&a.name, filter)
                || fuzzy_matches(&a.description, filter)
                || fuzzy_matches(&a.role, filter)
        })
        .count()
}

/// Get the agent ID at the given index in add mode
pub fn get_agent_id_at_index(
    app: &App,
    state: &ProjectSettingsState,
    index: usize,
) -> Option<String> {
    let filter = &state.add_filter;
    app.data_store
        .borrow()
        .content
        .get_agent_definitions()
        .into_iter()
        .filter(|a| !state.pending_agent_definition_ids.contains(&a.id))
        .filter(|a| {
            fuzzy_matches(&a.name, filter)
                || fuzzy_matches(&a.description, filter)
                || fuzzy_matches(&a.role, filter)
        })
        .nth(index)
        .map(|a| a.id.clone())
}

fn render_add_mcp_tool_mode(f: &mut Frame, app: &App, area: Rect, state: &ProjectSettingsState) {
    let (popup_area, content_area) = Modal::new("Add MCP Tool")
        .size(ModalSize {
            max_width: 70,
            height_percent: 0.8,
        })
        .search(&state.add_filter, "Search MCP tools...")
        .render_frame(f, area);

    // Content area with horizontal padding
    let remaining = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height,
    );

    // Get available MCP tools (exclude already added)
    let filter = &state.add_filter;
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

    // List area
    let list_area = Rect::new(
        remaining.x,
        remaining.y + 1,
        remaining.width,
        remaining.height.saturating_sub(4),
    );

    if available_tools.is_empty() {
        let msg = if state.add_filter.is_empty() {
            "No available MCP tools found."
        } else {
            "No MCP tools match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state.add_index.min(available_tools.len().saturating_sub(1));

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

                // Left border
                if is_selected {
                    spans.push(Span::styled(
                        "▌",
                        Style::default().fg(theme::ACCENT_PRIMARY),
                    ));
                } else {
                    spans.push(Span::styled("│", Style::default().fg(theme::TEXT_MUTED)));
                }

                // Tool name
                let name_style = if is_selected {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(&tool.name, name_style));

                // Author
                let author_name = app.data_store.borrow().get_profile_name(&tool.pubkey);
                spans.push(Span::styled(
                    format!(" by {}", author_name),
                    Style::default().fg(theme::TEXT_MUTED),
                ));

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);

        // Show description of selected tool
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

/// Get count of available MCP tools for add mode (for bounds checking)
pub fn available_mcp_tool_count(app: &App, state: &ProjectSettingsState) -> usize {
    let filter = &state.add_filter;
    app.data_store
        .borrow()
        .content
        .get_mcp_tools()
        .into_iter()
        .filter(|t| !state.pending_mcp_tool_ids.contains(&t.id))
        .filter(|t| fuzzy_matches(&t.name, filter) || fuzzy_matches(&t.description, filter))
        .count()
}

/// Get the MCP tool ID at the given index in add mode
pub fn get_mcp_tool_id_at_index(
    app: &App,
    state: &ProjectSettingsState,
    index: usize,
) -> Option<String> {
    let filter = &state.add_filter;
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

// The visible height constant is now defined in modal.rs as part of ProjectSettingsState
// and is dynamically computed during render and cached in state.cached_visible_height
