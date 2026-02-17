use crate::ui::components::{render_modal_search, Modal, ModalSize};
use crate::ui::modal::{CreateProjectFocus, CreateProjectState, CreateProjectStep};
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
    Frame,
};

/// Render the create project modal
pub fn render_create_project(f: &mut Frame, app: &App, area: Rect, state: &CreateProjectState) {
    // Header with step indicator
    let step_indicator = match state.step {
        CreateProjectStep::Details => "Step 1/3: Details",
        CreateProjectStep::SelectAgents => "Step 2/3: Select Agents",
        CreateProjectStep::SelectTools => "Step 3/3: Select MCP Tools",
    };

    let (popup_area, content_area) = Modal::new(step_indicator)
        .size(ModalSize {
            max_width: 70,
            height_percent: 0.7,
        })
        .render_frame(f, area);

    // Content area with horizontal padding
    let inner_area = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        content_area.height.saturating_sub(2),
    );

    match state.step {
        CreateProjectStep::Details => {
            render_details_step(f, inner_area, state);
        }
        CreateProjectStep::SelectAgents => {
            render_agents_step(f, app, inner_area, state);
        }
        CreateProjectStep::SelectTools => {
            render_tools_step(f, app, inner_area, state);
        }
    }

    // Hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );

    let hint_spans = match state.step {
        CreateProjectStep::Details => vec![
            Span::styled("Tab", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" switch field", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(" next step", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
        ],
        CreateProjectStep::SelectAgents => vec![
            Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" navigate", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Space", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" toggle", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(" next", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Backspace", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" back", Style::default().fg(theme::TEXT_MUTED)),
        ],
        CreateProjectStep::SelectTools => vec![
            Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" navigate", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Space", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" toggle", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(" create", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Backspace", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" back", Style::default().fg(theme::TEXT_MUTED)),
        ],
    };

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}

fn render_details_step(f: &mut Frame, area: Rect, state: &CreateProjectState) {
    let mut y = area.y;

    // Name field
    let name_label_style = if state.focus == CreateProjectFocus::Name {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let name_label = Paragraph::new(Line::from(vec![Span::styled("Name: ", name_label_style)]));
    f.render_widget(name_label, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Name input
    let name_border_color = if state.focus == CreateProjectFocus::Name {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };
    let name_value = if state.name.is_empty() && state.focus == CreateProjectFocus::Name {
        "Enter project name...".to_string()
    } else if state.name.is_empty() {
        "(required)".to_string()
    } else {
        state.name.clone()
    };
    let name_style = if state.name.is_empty() {
        Style::default().fg(theme::TEXT_DIM)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };
    let name_input = Paragraph::new(Line::from(vec![
        Span::styled("│ ", Style::default().fg(name_border_color)),
        Span::styled(name_value, name_style),
    ]));
    f.render_widget(name_input, Rect::new(area.x, y, area.width, 1));
    y += 2;

    // Description field
    let desc_label_style = if state.focus == CreateProjectFocus::Description {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let desc_label = Paragraph::new(Line::from(vec![Span::styled(
        "Description: ",
        desc_label_style,
    )]));
    f.render_widget(desc_label, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Description input
    let desc_border_color = if state.focus == CreateProjectFocus::Description {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };
    let desc_value =
        if state.description.is_empty() && state.focus == CreateProjectFocus::Description {
            "Enter description (optional)...".to_string()
        } else if state.description.is_empty() {
            "(optional)".to_string()
        } else {
            state.description.clone()
        };
    let desc_style = if state.description.is_empty() {
        Style::default().fg(theme::TEXT_DIM)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };
    let desc_input = Paragraph::new(Line::from(vec![
        Span::styled("│ ", Style::default().fg(desc_border_color)),
        Span::styled(desc_value, desc_style),
    ]));
    f.render_widget(desc_input, Rect::new(area.x, y, area.width, 1));
    y += 2;

    // Show cursor in active field
    if state.focus == CreateProjectFocus::Name {
        f.set_cursor_position((area.x + 2 + state.name.len() as u16, area.y + 1));
    } else if state.focus == CreateProjectFocus::Description {
        f.set_cursor_position((area.x + 2 + state.description.len() as u16, y - 2));
    }

    // Validation hint
    if state.name.trim().is_empty() {
        let hint = Paragraph::new(Line::from(vec![
            Span::styled("* ", Style::default().fg(theme::ACCENT_ERROR)),
            Span::styled(
                "Project name is required",
                Style::default().fg(theme::TEXT_DIM),
            ),
        ]));
        f.render_widget(hint, Rect::new(area.x, y, area.width, 1));
    }
}

fn render_agents_step(f: &mut Frame, app: &App, area: Rect, state: &CreateProjectState) {
    // Search bar
    let remaining = render_modal_search(f, area, &state.agent_selector.filter, "Search agents...");

    // Get filtered agents using the state's filter
    let filtered_agents = app.agent_definitions_filtered_by(&state.agent_selector.filter);

    // List area
    let list_area = Rect::new(
        remaining.x,
        remaining.y + 1,
        remaining.width,
        remaining.height.saturating_sub(3),
    );

    if filtered_agents.is_empty() {
        let msg = if state.agent_selector.filter.is_empty() {
            "No agents available."
        } else {
            "No agents match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state
            .agent_selector
            .index
            .min(filtered_agents.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let items: Vec<ListItem> = filtered_agents
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, agent)| {
                let is_cursor = i == selected_index;
                let is_selected = state.agent_ids.contains(&agent.id);

                let mut spans = vec![];

                // Checkbox
                let checkbox = if is_selected { "[✓] " } else { "[ ] " };
                let checkbox_style = if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_MUTED)
                };
                spans.push(Span::styled(checkbox, checkbox_style));

                // Cursor indicator
                if is_cursor {
                    spans.push(Span::styled(
                        "▌",
                        Style::default().fg(theme::ACCENT_PRIMARY),
                    ));
                } else {
                    spans.push(Span::styled(" ", Style::default()));
                }

                // Agent name
                let name_style = if is_cursor {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(agent.name.clone(), name_style));

                // Description preview
                if !agent.description.is_empty() {
                    let desc_preview = if agent.description.len() > 40 {
                        format!(" - {}...", &agent.description[..37])
                    } else {
                        format!(" - {}", agent.description)
                    };
                    spans.push(Span::styled(
                        desc_preview,
                        Style::default().fg(theme::TEXT_MUTED),
                    ));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);
    }

    // Show selected count
    let count_area = Rect::new(
        remaining.x,
        list_area.y + list_area.height,
        remaining.width,
        1,
    );
    let count_text = format!("{} agent(s) selected", state.agent_ids.len());
    let count = Paragraph::new(count_text).style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(count, count_area);
}

fn render_tools_step(f: &mut Frame, app: &App, area: Rect, state: &CreateProjectState) {
    // Search bar
    let remaining =
        render_modal_search(f, area, &state.tool_selector.filter, "Search MCP tools...");

    // Get filtered tools
    let filtered_tools = app.mcp_tools_filtered_by(&state.tool_selector.filter);

    // List area
    let list_area = Rect::new(
        remaining.x,
        remaining.y + 1,
        remaining.width,
        remaining.height.saturating_sub(3),
    );

    if filtered_tools.is_empty() {
        let msg = if state.tool_selector.filter.is_empty() {
            "No MCP tools available."
        } else {
            "No tools match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state
            .tool_selector
            .index
            .min(filtered_tools.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let items: Vec<ListItem> = filtered_tools
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, tool)| {
                let is_cursor = i == selected_index;
                let is_selected = state.mcp_tool_ids.contains(&tool.id);

                let mut spans = vec![];

                // Checkbox
                let checkbox = if is_selected { "[✓] " } else { "[ ] " };
                let checkbox_style = if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_MUTED)
                };
                spans.push(Span::styled(checkbox, checkbox_style));

                // Cursor indicator
                if is_cursor {
                    spans.push(Span::styled(
                        "▌",
                        Style::default().fg(theme::ACCENT_PRIMARY),
                    ));
                } else {
                    spans.push(Span::styled(" ", Style::default()));
                }

                // Tool name
                let name_style = if is_cursor {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(tool.name.clone(), name_style));

                // Description preview
                if !tool.description.is_empty() {
                    let desc_preview = if tool.description.len() > 40 {
                        format!(" - {}...", &tool.description[..37])
                    } else {
                        format!(" - {}", tool.description)
                    };
                    spans.push(Span::styled(
                        desc_preview,
                        Style::default().fg(theme::TEXT_MUTED),
                    ));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);
    }

    // Show selected count
    let count_area = Rect::new(
        remaining.x,
        list_area.y + list_area.height,
        remaining.width,
        1,
    );

    // Calculate total (manual + auto from agents)
    let total_tool_count = state.all_mcp_tool_ids(app).len();
    let manual_count = state.mcp_tool_ids.len();

    let count_text = if total_tool_count > manual_count {
        format!(
            "{} tool(s) selected ({} from agents)",
            total_tool_count,
            total_tool_count - manual_count
        )
    } else {
        format!("{} tool(s) selected", total_tool_count)
    };

    let count = Paragraph::new(count_text).style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(count, count_area);
}
