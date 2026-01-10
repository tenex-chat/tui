use crate::ui::components::{
    modal_area, render_modal_background, render_modal_header, render_modal_overlay, ModalSize,
};
use crate::ui::modal::{AgentCreateStep, AgentFormFocus, CreateAgentState};
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Render the create/fork/clone agent modal
pub fn render_create_agent(f: &mut Frame, area: Rect, state: &CreateAgentState) {
    // Dim the background
    render_modal_overlay(f, area);

    let size = ModalSize {
        max_width: 80,
        height_percent: 0.85,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    let inner_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + 1,
        popup_area.width.saturating_sub(4),
        popup_area.height.saturating_sub(3),
    );

    // Header with mode and step indicator
    let step_indicator = match state.step {
        AgentCreateStep::Basics => format!("{} - Step 1/3: Basics", state.mode_label()),
        AgentCreateStep::Instructions => format!("{} - Step 2/3: Instructions", state.mode_label()),
        AgentCreateStep::Review => format!("{} - Step 3/3: Review", state.mode_label()),
    };
    let remaining = render_modal_header(f, inner_area, &step_indicator, "esc");

    match state.step {
        AgentCreateStep::Basics => {
            render_basics_step(f, remaining, state);
        }
        AgentCreateStep::Instructions => {
            render_instructions_step(f, remaining, state);
        }
        AgentCreateStep::Review => {
            render_review_step(f, remaining, state);
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
        AgentCreateStep::Basics => vec![
            Span::styled("Tab", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" switch field", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(" next step", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
        ],
        AgentCreateStep::Instructions => vec![
            Span::styled("Enter", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" new line", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Ctrl+Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(" next step", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Backspace", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" back (if empty)", Style::default().fg(theme::TEXT_MUTED)),
        ],
        AgentCreateStep::Review => vec![
            Span::styled("↑↓", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" scroll", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(" publish", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Backspace", Style::default().fg(theme::ACCENT_WARNING)),
            Span::styled(" back", Style::default().fg(theme::TEXT_MUTED)),
        ],
    };

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}

fn render_basics_step(f: &mut Frame, area: Rect, state: &CreateAgentState) {
    let mut y = area.y;

    // Name field
    let name_label_style = if state.focus == AgentFormFocus::Name {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let name_label = Paragraph::new(Line::from(vec![
        Span::styled("Name: ", name_label_style),
        Span::styled("*", Style::default().fg(theme::ACCENT_ERROR)),
    ]));
    f.render_widget(name_label, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Name input
    let name_border_color = if state.focus == AgentFormFocus::Name {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };
    let name_value = if state.name.is_empty() && state.focus == AgentFormFocus::Name {
        "Enter agent name...".to_string()
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
    let desc_label_style = if state.focus == AgentFormFocus::Description {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let desc_label = Paragraph::new(Line::from(vec![
        Span::styled("Description: ", desc_label_style),
        Span::styled("*", Style::default().fg(theme::ACCENT_ERROR)),
    ]));
    f.render_widget(desc_label, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Description input
    let desc_border_color = if state.focus == AgentFormFocus::Description {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };
    let desc_value = if state.description.is_empty() && state.focus == AgentFormFocus::Description {
        "Brief description of what this agent does...".to_string()
    } else if state.description.is_empty() {
        "(required)".to_string()
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

    // Role field
    let role_label_style = if state.focus == AgentFormFocus::Role {
        Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    let role_label = Paragraph::new(Line::from(vec![
        Span::styled("Role: ", role_label_style),
    ]));
    f.render_widget(role_label, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Role input
    let role_border_color = if state.focus == AgentFormFocus::Role {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };
    let role_input = Paragraph::new(Line::from(vec![
        Span::styled("│ ", Style::default().fg(role_border_color)),
        Span::styled(&state.role, Style::default().fg(theme::TEXT_PRIMARY)),
    ]));
    f.render_widget(role_input, Rect::new(area.x, y, area.width, 1));
    y += 2;

    // Version display (for fork mode)
    if state.source_id.is_some() {
        let version_label = Paragraph::new(Line::from(vec![
            Span::styled("Version: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(&state.version, Style::default().fg(theme::ACCENT_PRIMARY)),
        ]));
        f.render_widget(version_label, Rect::new(area.x, y, area.width, 1));
        y += 2;
    }

    // Show cursor in active field
    match state.focus {
        AgentFormFocus::Name => {
            f.set_cursor_position((area.x + 2 + state.name.len() as u16, area.y + 1));
        }
        AgentFormFocus::Description => {
            f.set_cursor_position((area.x + 2 + state.description.len() as u16, area.y + 4));
        }
        AgentFormFocus::Role => {
            f.set_cursor_position((area.x + 2 + state.role.len() as u16, area.y + 7));
        }
    }

    // Validation hint
    if state.name.trim().is_empty() || state.description.trim().is_empty() {
        let hint = Paragraph::new(Line::from(vec![
            Span::styled("* ", Style::default().fg(theme::ACCENT_ERROR)),
            Span::styled("Name and description are required", Style::default().fg(theme::TEXT_DIM)),
        ]));
        f.render_widget(hint, Rect::new(area.x, y, area.width, 1));
    }
}

fn render_instructions_step(f: &mut Frame, area: Rect, state: &CreateAgentState) {
    let mut y = area.y;

    // Instructions label
    let label = Paragraph::new(Line::from(vec![
        Span::styled("System Prompt / Instructions:", Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)),
    ]));
    f.render_widget(label, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Instructions editor area
    let editor_height = area.height.saturating_sub(4);
    let editor_area = Rect::new(area.x, y, area.width, editor_height);

    // Render border
    let border_line = Paragraph::new("─".repeat(area.width as usize))
        .style(Style::default().fg(theme::BORDER_INACTIVE));
    f.render_widget(border_line.clone(), Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Split instructions into lines and apply scroll
    let lines: Vec<&str> = state.instructions.lines().collect();
    let visible_height = editor_height.saturating_sub(2) as usize;
    let scroll = state.instructions_scroll.min(lines.len().saturating_sub(visible_height));

    for (i, line) in lines.iter().skip(scroll).take(visible_height).enumerate() {
        let line_num = scroll + i + 1;
        let line_content = Paragraph::new(Line::from(vec![
            Span::styled(format!("{:3} │ ", line_num), Style::default().fg(theme::TEXT_DIM)),
            Span::styled(*line, Style::default().fg(theme::TEXT_PRIMARY)),
        ]));
        f.render_widget(line_content, Rect::new(area.x, y + i as u16, area.width, 1));
    }

    // If empty, show placeholder
    if state.instructions.is_empty() {
        let placeholder = Paragraph::new(Line::from(vec![
            Span::styled("  1 │ ", Style::default().fg(theme::TEXT_DIM)),
            Span::styled("Enter system prompt here...", Style::default().fg(theme::TEXT_DIM)),
        ]));
        f.render_widget(placeholder, Rect::new(area.x, y, area.width, 1));
    }

    // Bottom border
    let bottom_y = editor_area.y + editor_height.saturating_sub(1);
    f.render_widget(border_line, Rect::new(area.x, bottom_y, area.width, 1));

    // Character count
    let char_count = format!("{} characters", state.instructions.len());
    let count_para = Paragraph::new(char_count).style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(count_para, Rect::new(area.x, bottom_y + 1, area.width, 1));

    // Position cursor
    let cursor_line = state.instructions[..state.instructions_cursor].matches('\n').count();
    let cursor_col = state.instructions[..state.instructions_cursor]
        .rfind('\n')
        .map(|pos| state.instructions_cursor - pos - 1)
        .unwrap_or(state.instructions_cursor);

    let visible_cursor_line = cursor_line.saturating_sub(scroll);
    if visible_cursor_line < visible_height {
        f.set_cursor_position((
            area.x + 6 + cursor_col as u16,
            y + visible_cursor_line as u16,
        ));
    }
}

fn render_review_step(f: &mut Frame, area: Rect, state: &CreateAgentState) {
    let mut y = area.y;

    // Title
    let title = Paragraph::new(Line::from(vec![
        Span::styled("Review Agent Definition", Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)),
    ]));
    f.render_widget(title, Rect::new(area.x, y, area.width, 1));
    y += 2;

    // Name
    let name_line = Paragraph::new(Line::from(vec![
        Span::styled("Name: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&state.name, Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
    ]));
    f.render_widget(name_line, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Description
    let desc_line = Paragraph::new(Line::from(vec![
        Span::styled("Description: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&state.description, Style::default().fg(theme::TEXT_PRIMARY)),
    ]));
    f.render_widget(desc_line, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Role
    let role_line = Paragraph::new(Line::from(vec![
        Span::styled("Role: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&state.role, Style::default().fg(theme::TEXT_PRIMARY)),
    ]));
    f.render_widget(role_line, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Version
    let version_line = Paragraph::new(Line::from(vec![
        Span::styled("Version: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&state.version, Style::default().fg(theme::TEXT_PRIMARY)),
    ]));
    f.render_widget(version_line, Rect::new(area.x, y, area.width, 1));
    y += 2;

    // Instructions preview
    let instructions_label = Paragraph::new(Line::from(vec![
        Span::styled("Instructions Preview:", Style::default().fg(theme::TEXT_MUTED)),
    ]));
    f.render_widget(instructions_label, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Instructions content with scroll
    let preview_height = area.height.saturating_sub(y - area.y + 2) as usize;
    let lines: Vec<&str> = state.instructions.lines().collect();
    let scroll = state.instructions_scroll.min(lines.len().saturating_sub(preview_height));

    for (i, line) in lines.iter().skip(scroll).take(preview_height).enumerate() {
        let line_para = Paragraph::new(*line).style(Style::default().fg(theme::TEXT_PRIMARY));
        f.render_widget(line_para, Rect::new(area.x + 2, y + i as u16, area.width.saturating_sub(2), 1));
    }

    // Scroll indicator if needed
    if lines.len() > preview_height {
        let indicator = format!("({}/{} lines)", scroll + 1, lines.len());
        let indicator_para = Paragraph::new(indicator).style(Style::default().fg(theme::TEXT_DIM));
        f.render_widget(indicator_para, Rect::new(area.x + area.width.saturating_sub(20), area.y, 20, 1));
    }
}
