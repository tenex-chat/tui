use crate::ui::card;
use crate::ui::markdown::render_markdown;
use crate::ui::modal::ModalState;
use crate::ui::{theme, App};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Wrap},
    Frame,
};
use tenex_core::models::AgentDefinition;

/// Render the agent browser view - list or detail mode
pub fn render_agent_browser(f: &mut Frame, app: &App, area: Rect) {
    f.render_widget(Clear, area);

    if app.agent_browser_in_detail {
        if let Some(ref id) = app.viewing_agent_id {
            let agent = app.data_store.borrow()
                .get_agent_definition(id)
                .cloned();
            if let Some(agent) = agent {
                render_agent_detail(f, app, area, &agent);
            }
        }
    } else {
        render_agent_list(f, app, area);
    }

    // Render create agent modal overlay
    if let ModalState::CreateAgent(ref state) = app.modal_state {
        super::render_create_agent(f, area, state);
    }

    // Command palette overlay (Ctrl+T)
    if let ModalState::CommandPalette(ref state) = app.modal_state {
        super::render_command_palette(f, area, app, state.selected_index);
    }
}

/// Render the agent list with search filter
fn render_agent_list(f: &mut Frame, app: &App, area: Rect) {
    let chunks = Layout::vertical([
        Constraint::Length(3), // Header + search
        Constraint::Min(0),    // List
        Constraint::Length(2), // Footer
    ])
    .split(area);

    // Header with search
    render_list_header(f, app, chunks[0]);

    // Agent list
    render_list_content(f, app, chunks[1]);

    // Footer with hints
    render_list_footer(f, chunks[2]);
}

fn render_list_header(f: &mut Frame, app: &App, area: Rect) {
    let agents = app.filtered_agent_definitions();
    let count = agents.len();

    let title_line = vec![
        Span::styled("🤖 ", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled("Agent Definitions", Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)),
        Span::styled(format!(" ({} agents)", count), Style::default().fg(theme::TEXT_MUTED)),
    ];

    let search_line = if app.agent_browser_filter.is_empty() {
        vec![
            Span::styled("Type to search...", Style::default().fg(theme::TEXT_MUTED)),
        ]
    } else {
        vec![
            Span::styled("Search: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(&app.agent_browser_filter, Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled("▌", Style::default().fg(theme::ACCENT_PRIMARY)),
        ]
    };

    let header = Paragraph::new(vec![
        Line::from(title_line),
        Line::from(search_line),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
    );

    f.render_widget(header, area);
}

fn render_list_content(f: &mut Frame, app: &App, area: Rect) {
    let agents = app.filtered_agent_definitions();

    if agents.is_empty() {
        let empty_msg = if app.agent_browser_filter.is_empty() {
            "No agent definitions found. Agents will appear here once subscribed from the network."
        } else {
            "No agents match your search."
        };

        let paragraph = Paragraph::new(empty_msg)
            .style(Style::default().fg(theme::TEXT_MUTED))
            .block(
                Block::default()
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
            );
        f.render_widget(paragraph, area);
        return;
    }

    let visible_height = area.height.saturating_sub(2) as usize;
    let selected_index = app.agent_browser_index.min(agents.len().saturating_sub(1));

    // Calculate scroll to keep selected item visible
    let scroll_offset = if selected_index >= visible_height {
        selected_index - visible_height + 1
    } else {
        0
    };

    let items: Vec<ListItem> = agents
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(idx, agent)| {
            let is_selected = idx == selected_index;
            create_agent_list_item(app, agent, is_selected)
        })
        .collect();

    let list = List::new(items)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
        );

    f.render_widget(list, area);
}

fn create_agent_list_item(app: &App, agent: &AgentDefinition, is_selected: bool) -> ListItem<'static> {
    let author_name = app.data_store.borrow().get_profile_name(&agent.pubkey);

    // Build the line: [role] name - description (author)
    let mut spans = vec![];

    // Selection indicator
    if is_selected {
        spans.push(Span::styled(card::COLLAPSE_CLOSED, Style::default().fg(theme::ACCENT_PRIMARY)));
    } else {
        spans.push(Span::styled(card::SPACER, Style::default()));
    }

    // Role badge
    spans.push(Span::styled(
        format!("[{}] ", agent.role),
        Style::default().fg(theme::ACCENT_SPECIAL),
    ));

    // Name
    let name_style = if is_selected {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };
    spans.push(Span::styled(agent.name.clone(), name_style));

    // Description preview (truncated)
    if !agent.description.is_empty() {
        let preview = agent.description_preview(40);
        spans.push(Span::styled(" - ", Style::default().fg(theme::TEXT_MUTED)));
        spans.push(Span::styled(preview, Style::default().fg(theme::TEXT_DIM)));
    }

    // Author
    spans.push(Span::styled(" (", Style::default().fg(theme::TEXT_MUTED)));
    spans.push(Span::styled(author_name, Style::default().fg(theme::ACCENT_SUCCESS)));
    spans.push(Span::styled(")", Style::default().fg(theme::TEXT_MUTED)));

    ListItem::new(Line::from(spans))
}

fn render_list_footer(f: &mut Frame, area: Rect) {
    let help_spans = vec![
        Span::styled("↑/↓", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" navigate | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" view | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("n", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" new | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" back", Style::default().fg(theme::TEXT_MUTED)),
    ];

    let footer = Paragraph::new(vec![Line::from(help_spans)])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
        );

    f.render_widget(footer, area);
}

/// Render agent detail view
fn render_agent_detail(f: &mut Frame, app: &App, area: Rect, agent: &AgentDefinition) {
    let chunks = Layout::vertical([
        Constraint::Length(4), // Header
        Constraint::Min(0),    // Content (scrollable)
        Constraint::Length(2), // Footer
    ])
    .split(area);

    render_detail_header(f, app, agent, chunks[0]);
    render_detail_content(f, app, agent, chunks[1]);
    render_detail_footer(f, chunks[2]);
}

fn render_detail_header(f: &mut Frame, app: &App, agent: &AgentDefinition, area: Rect) {
    let author_name = app.data_store.borrow().get_profile_name(&agent.pubkey);

    let title_line = vec![
        Span::styled("🤖 ", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(&agent.name, Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)),
        Span::styled(" [", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&agent.role, Style::default().fg(theme::ACCENT_SPECIAL)),
        Span::styled("]", Style::default().fg(theme::TEXT_MUTED)),
    ];

    let meta_line = vec![
        Span::styled("by ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&author_name, Style::default().fg(theme::ACCENT_SUCCESS)),
        if let Some(ref version) = agent.version {
            Span::styled(
                format!("{}v{}", card::META_SEPARATOR, version),
                Style::default().fg(theme::TEXT_MUTED)
            )
        } else {
            Span::styled("", Style::default())
        },
        if let Some(ref model) = agent.model {
            Span::styled(
                format!("{}{}", card::META_SEPARATOR, model),
                Style::default().fg(theme::ACCENT_SPECIAL)
            )
        } else {
            Span::styled("", Style::default())
        },
    ];

    let desc_line = if !agent.description.is_empty() {
        vec![Span::styled(&agent.description, Style::default().fg(theme::TEXT_DIM))]
    } else {
        vec![Span::styled("No description", Style::default().fg(theme::TEXT_MUTED))]
    };

    let header = Paragraph::new(vec![
        Line::from(title_line),
        Line::from(meta_line),
        Line::from(desc_line),
    ])
    .block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
    );

    f.render_widget(header, area);
}

fn render_detail_content(f: &mut Frame, app: &App, agent: &AgentDefinition, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Instructions section
    if !agent.instructions.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("📝 Instructions", Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
        ]));
        lines.push(Line::from(""));

        // Render markdown instructions
        let md_lines = render_markdown(&agent.instructions);
        lines.extend(md_lines);
        lines.push(Line::from(""));
    }

    // Use criteria section
    if !agent.use_criteria.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("🎯 Use Criteria", Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
        ]));
        for criteria in &agent.use_criteria {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}{}", card::INDENT_UNIT, card::LIST_BULLET),
                Style::default().fg(theme::ACCENT_PRIMARY)
            ),
            Span::styled(criteria.clone(), Style::default().fg(theme::TEXT_DIM)),
        ]));
        }
        lines.push(Line::from(""));
    }

    // Tools section
    if !agent.tools.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("🔧 Tools", Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
        ]));
        for tool in &agent.tools {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}{}", card::INDENT_UNIT, card::LIST_BULLET),
                Style::default().fg(theme::ACCENT_PRIMARY)
            ),
            Span::styled(tool.clone(), Style::default().fg(theme::TEXT_DIM)),
        ]));
        }
        lines.push(Line::from(""));
    }

    // MCP servers section
    if !agent.mcp_servers.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("🔌 MCP Servers", Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
        ]));
        for server in &agent.mcp_servers {
        lines.push(Line::from(vec![
            Span::styled(
                format!("{}{}", card::INDENT_UNIT, card::LIST_BULLET),
                Style::default().fg(theme::ACCENT_PRIMARY)
            ),
            Span::styled(server.clone(), Style::default().fg(theme::TEXT_DIM)),
        ]));
        }
        lines.push(Line::from(""));
    }

    // Calculate scrolling
    let content_height = lines.len();
    let visible_height = area.height.saturating_sub(2) as usize;
    let max_scroll = content_height.saturating_sub(visible_height);
    let scroll_offset = app.scroll_offset.min(max_scroll);

    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(scroll_offset)
        .take(visible_height)
        .collect();

    let mut block_title = String::from("Details");
    if content_height > visible_height {
        let scroll_percent = if max_scroll > 0 {
            (scroll_offset * 100) / max_scroll
        } else {
            0
        };
        block_title = format!("Details ({}%)", scroll_percent);
    }

    let content = Paragraph::new(visible_lines)
        .wrap(Wrap { trim: false })
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(block_title)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
        );

    f.render_widget(content, area);
}

fn render_detail_footer(f: &mut Frame, area: Rect) {
    let help_spans = vec![
        Span::styled("j/k", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" scroll | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("f", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" fork | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("c", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" clone | ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" back", Style::default().fg(theme::TEXT_MUTED)),
    ];

    let footer = Paragraph::new(vec![Line::from(help_spans)])
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
        );

    f.render_widget(footer, area);
}
