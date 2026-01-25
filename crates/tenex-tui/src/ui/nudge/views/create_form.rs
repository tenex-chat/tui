//! Nudge create form - multi-step wizard for creating nudges

use crate::ui::components::{Modal, ModalSize};
use crate::ui::nudge::{NudgeFormFocus, NudgeFormState, NudgeFormStep, PermissionMode};
use crate::ui::theme;
use crate::ui::App;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
    Frame,
};

/// Render the nudge create form
pub fn render_nudge_create(f: &mut Frame, app: &App, area: Rect, state: &NudgeFormState) {
    let title = state.mode_label();

    let (_popup_area, content_area) = Modal::new(title)
        .size(ModalSize {
            max_width: 80,
            height_percent: 0.85,
        })
        .render_frame(f, area);

    // Render step indicator
    let step_area = Rect::new(content_area.x, content_area.y, content_area.width, 1);
    render_step_indicator(f, step_area, state.step);

    // Main content area (below step indicator)
    let main_area = Rect::new(
        content_area.x,
        content_area.y + 2,
        content_area.width,
        content_area.height.saturating_sub(5),
    );

    // Render current step content
    match state.step {
        NudgeFormStep::Basics => render_basics_step(f, main_area, state),
        NudgeFormStep::Content => render_content_step(f, main_area, state),
        NudgeFormStep::Permissions => render_permissions_step(f, app, main_area, state),
        NudgeFormStep::Review => render_review_step(f, main_area, state),
    }

    // Render navigation hints
    let hints_area = Rect::new(
        content_area.x,
        content_area.y + content_area.height.saturating_sub(2),
        content_area.width,
        1,
    );
    render_step_hints(f, hints_area, state);
}

/// Render the step indicator (1 ── 2 ── 3 ── 4)
fn render_step_indicator(f: &mut Frame, area: Rect, current_step: NudgeFormStep) {
    let mut spans = vec![];

    for (i, step) in NudgeFormStep::ALL.iter().enumerate() {
        if i > 0 {
            let connector_style = if step.index() <= current_step.index() {
                Style::default().fg(theme::ACCENT_PRIMARY)
            } else {
                Style::default().fg(theme::BORDER_INACTIVE)
            };
            spans.push(Span::styled(" ── ", connector_style));
        }

        let (label, style) = if *step == current_step {
            (
                format!("● {}", step.label()),
                Style::default()
                    .fg(theme::ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            )
        } else if step.index() < current_step.index() {
            (
                format!("✓ {}", step.label()),
                Style::default().fg(theme::ACCENT_SUCCESS),
            )
        } else {
            (
                format!("○ {}", step.label()),
                Style::default().fg(theme::TEXT_MUTED),
            )
        };

        spans.push(Span::styled(label, style));
    }

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}

/// Render the basics step (title, description, hashtags)
fn render_basics_step(f: &mut Frame, area: Rect, state: &NudgeFormState) {
    let mut y = area.y;

    // Title field
    render_text_field(
        f,
        Rect::new(area.x, y, area.width, 3),
        "Title",
        &state.title,
        "e.g., code-review",
        state.focus == NudgeFormFocus::Title,
    );
    y += 4;

    // Description field
    render_text_field(
        f,
        Rect::new(area.x, y, area.width, 3),
        "Description",
        &state.description,
        "Brief description (optional)",
        state.focus == NudgeFormFocus::Description,
    );
    y += 4;

    // Hashtags field
    let hashtags_focused = state.focus == NudgeFormFocus::Hashtags;
    let label_style = if hashtags_focused {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let label = Paragraph::new("Hashtags (optional)").style(label_style);
    f.render_widget(label, Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Render existing hashtags
    let tags_line: Vec<Span> = state
        .hashtags
        .iter()
        .map(|t| {
            Span::styled(
                format!(" #{} ", t),
                Style::default()
                    .fg(theme::TEXT_PRIMARY)
                    .bg(theme::BG_SELECTED),
            )
        })
        .collect();

    let border_color = if hashtags_focused {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };

    let mut input_spans = vec![Span::styled("│ ", Style::default().fg(border_color))];
    input_spans.extend(tags_line);

    // Current input
    let input_style = if hashtags_focused {
        Style::default().fg(theme::TEXT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };
    input_spans.push(Span::styled(&state.hashtag_input, input_style));

    if hashtags_focused && state.hashtag_input.is_empty() {
        input_spans.push(Span::styled(
            "type and press Space to add",
            Style::default().fg(theme::TEXT_DIM),
        ));
    }

    let input_line = Paragraph::new(Line::from(input_spans));
    f.render_widget(input_line, Rect::new(area.x, y, area.width, 1));

    // Cursor positioning for active field
    if hashtags_focused {
        let cursor_x = area.x + 2 + state.hashtags.iter().map(|t| t.len() + 4).sum::<usize>() as u16
            + state.hashtag_input.len() as u16;
        f.set_cursor_position((cursor_x, y));
    }
}

/// Render a text input field
fn render_text_field(
    f: &mut Frame,
    area: Rect,
    label: &str,
    value: &str,
    placeholder: &str,
    is_focused: bool,
) {
    let label_style = if is_focused {
        Style::default()
            .fg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let label_para = Paragraph::new(label).style(label_style);
    f.render_widget(label_para, Rect::new(area.x, area.y, area.width, 1));

    let border_color = if is_focused {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };

    let (display_text, text_style) = if value.is_empty() {
        (placeholder, Style::default().fg(theme::TEXT_DIM))
    } else {
        (value, Style::default().fg(theme::TEXT_PRIMARY))
    };

    let input = Paragraph::new(Line::from(vec![
        Span::styled("│ ", Style::default().fg(border_color)),
        Span::styled(display_text, text_style),
    ]));

    f.render_widget(input, Rect::new(area.x, area.y + 1, area.width, 1));

    // Set cursor position
    if is_focused {
        f.set_cursor_position((area.x + 2 + value.len() as u16, area.y + 1));
    }
}

/// Render the content step (multi-line editor)
fn render_content_step(f: &mut Frame, area: Rect, state: &NudgeFormState) {
    let label = Paragraph::new("Behavioral Instructions")
        .style(Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD));
    f.render_widget(label, Rect::new(area.x, area.y, area.width, 1));

    let hint = Paragraph::new("Enter the nudge content - instructions for agent behavior")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hint, Rect::new(area.x, area.y + 1, area.width, 1));

    // Editor area with border
    let editor_area = Rect::new(area.x, area.y + 3, area.width, area.height.saturating_sub(5));

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::ACCENT_PRIMARY))
        .title(" Content ");

    let inner_area = block.inner(editor_area);
    f.render_widget(block, editor_area);

    // Render content with line numbers
    let visible_height = inner_area.height as usize;
    let lines: Vec<&str> = state.content.lines().collect();
    let total_lines = lines.len().max(1);

    // Calculate scroll offset
    let scroll_offset = if state.content_cursor.0 >= state.content_scroll + visible_height {
        state.content_cursor.0 - visible_height + 1
    } else if state.content_cursor.0 < state.content_scroll {
        state.content_cursor.0
    } else {
        state.content_scroll
    };

    for (i, line_idx) in (scroll_offset..total_lines)
        .take(visible_height)
        .enumerate()
    {
        let y = inner_area.y + i as u16;
        let line_num = line_idx + 1;
        let content = lines.get(line_idx).unwrap_or(&"");

        // Line number
        let num_span = Span::styled(
            format!("{:3} │ ", line_num),
            Style::default().fg(theme::TEXT_MUTED),
        );

        // Content
        let content_style = if line_idx == state.content_cursor.0 {
            Style::default().fg(theme::TEXT_PRIMARY)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };
        let content_span = Span::styled(*content, content_style);

        let line = Paragraph::new(Line::from(vec![num_span, content_span]));
        f.render_widget(line, Rect::new(inner_area.x, y, inner_area.width, 1));
    }

    // Position cursor
    let cursor_y = inner_area.y + (state.content_cursor.0 - scroll_offset) as u16;
    let cursor_x = inner_area.x + 6 + state.content_cursor.1 as u16;
    if cursor_y < inner_area.y + inner_area.height {
        f.set_cursor_position((cursor_x, cursor_y));
    }

    // Character count
    let char_count = Paragraph::new(format!("{} characters", state.content.len()))
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(
        char_count,
        Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1),
    );
}

/// Render the permissions step (tool allow/deny)
fn render_permissions_step(f: &mut Frame, app: &App, area: Rect, state: &NudgeFormState) {
    let label = Paragraph::new("Tool Permissions (Optional)")
        .style(Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD));
    f.render_widget(label, Rect::new(area.x, area.y, area.width, 1));

    let hint = Paragraph::new("Add tools to allow or deny when this nudge is active")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hint, Rect::new(area.x, area.y + 1, area.width, 1));

    // Get available tools from project statuses
    let data_store = app.data_store.borrow();
    // Fix #5: Sort tool list for deterministic ordering
    let mut available_tools: Vec<String> = data_store
        .project_statuses
        .values()
        .flat_map(|s| s.all_tools.iter().cloned())
        .collect::<std::collections::HashSet<_>>()
        .into_iter()
        .collect();
    available_tools.sort();

    // Check if we're in add mode (showing tool list)
    let is_adding = state.permission_mode == PermissionMode::AddAllow
        || state.permission_mode == PermissionMode::AddDeny;

    if is_adding {
        // Fix #4: Render the actual tool list when in add mode
        render_tool_selector(f, area, state, &available_tools);
    } else {
        // Two columns: Allow (left) and Deny (right)
        let col_width = (area.width / 2).saturating_sub(1);

        // Allow column
        let allow_area = Rect::new(area.x, area.y + 3, col_width, area.height.saturating_sub(5));
        render_permission_column(
            f,
            allow_area,
            "✓ Allow Tools",
            &state.permissions.allow_tools,
            theme::ACCENT_SUCCESS,
            false,
        );

        // Deny column
        let deny_area = Rect::new(
            area.x + col_width + 2,
            area.y + 3,
            col_width,
            area.height.saturating_sub(5),
        );
        render_permission_column(
            f,
            deny_area,
            "✗ Deny Tools",
            &state.permissions.deny_tools,
            theme::ACCENT_ERROR,
            false,
        );
    }

    // Conflict warnings
    let conflicts = state.permissions.detect_conflicts();
    if !conflicts.is_empty() {
        let warning = format!(
            "⚠ Conflict: {} - deny will override allow",
            conflicts
                .iter()
                .map(|c| c.tool_name.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
        let warning_para = Paragraph::new(warning).style(Style::default().fg(theme::ACCENT_WARNING));
        f.render_widget(
            warning_para,
            Rect::new(area.x, area.y + area.height.saturating_sub(2), area.width, 1),
        );
    }

    // Tool count from available tools
    if !available_tools.is_empty() && !is_adding {
        let tool_count = format!("{} tools available from project statuses", available_tools.len());
        let count_para = Paragraph::new(tool_count).style(Style::default().fg(theme::TEXT_DIM));
        f.render_widget(
            count_para,
            Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1),
        );
    }
}

/// Render the tool selector when adding allow/deny tools
fn render_tool_selector(f: &mut Frame, area: Rect, state: &NudgeFormState, available_tools: &[String]) {
    let is_allow_mode = state.permission_mode == PermissionMode::AddAllow;
    let mode_label = if is_allow_mode { "Allow" } else { "Deny" };
    let accent_color = if is_allow_mode { theme::ACCENT_SUCCESS } else { theme::ACCENT_ERROR };

    // Title
    let title = format!("Select tool to {} (Esc to cancel)", mode_label.to_lowercase());
    let title_para = Paragraph::new(title)
        .style(Style::default().fg(accent_color).add_modifier(Modifier::BOLD));
    f.render_widget(title_para, Rect::new(area.x, area.y + 3, area.width, 1));

    // Filter input
    let filter_label = if state.tool_filter.is_empty() {
        "Type to filter...".to_string()
    } else {
        format!("Filter: {}", state.tool_filter)
    };
    let filter_style = if state.tool_filter.is_empty() {
        Style::default().fg(theme::TEXT_DIM)
    } else {
        Style::default().fg(theme::TEXT_PRIMARY)
    };
    let filter_para = Paragraph::new(filter_label).style(filter_style);
    f.render_widget(filter_para, Rect::new(area.x, area.y + 4, area.width, 1));

    // Tool list area
    let list_area = Rect::new(area.x, area.y + 6, area.width, area.height.saturating_sub(9));
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(accent_color))
        .title(format!(" Available Tools ({}) ", available_tools.len()));

    let inner = block.inner(list_area);
    f.render_widget(block, list_area);

    // Filter tools
    let filtered: Vec<&str> = state.filter_tools(available_tools);
    let visible_height = inner.height as usize;

    if filtered.is_empty() {
        let empty_msg = if state.tool_filter.is_empty() {
            "No tools available"
        } else {
            "No matching tools"
        };
        let empty_para = Paragraph::new(empty_msg).style(Style::default().fg(theme::TEXT_DIM));
        f.render_widget(empty_para, inner);
    } else {
        // Calculate scroll offset to keep selected item visible
        let scroll_offset = if state.tool_index >= state.tool_scroll + visible_height {
            state.tool_index.saturating_sub(visible_height - 1)
        } else if state.tool_index < state.tool_scroll {
            state.tool_index
        } else {
            state.tool_scroll
        };

        for (i, tool) in filtered.iter().skip(scroll_offset).take(visible_height).enumerate() {
            let actual_index = scroll_offset + i;
            let is_selected = actual_index == state.tool_index;
            let is_allowed = state.permissions.is_allowed(tool);
            let is_denied = state.permissions.is_denied(tool);

            let style = if is_selected {
                Style::default().fg(theme::TEXT_PRIMARY).bg(accent_color)
            } else if is_allowed && is_denied {
                // Conflict - highlight
                Style::default().fg(theme::ACCENT_WARNING)
            } else if is_allowed {
                Style::default().fg(theme::ACCENT_SUCCESS)
            } else if is_denied {
                Style::default().fg(theme::ACCENT_ERROR)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            };

            // Show status indicator
            let prefix = if is_allowed && is_denied {
                "⚠ "
            } else if is_allowed {
                "✓ "
            } else if is_denied {
                "✗ "
            } else {
                "  "
            };

            let line = Paragraph::new(format!("{}{}", prefix, tool)).style(style);
            f.render_widget(line, Rect::new(inner.x, inner.y + i as u16, inner.width, 1));
        }

        // Scroll indicator
        if filtered.len() > visible_height {
            let indicator = format!("{}/{}", state.tool_index + 1, filtered.len());
            let indicator_para = Paragraph::new(indicator).style(Style::default().fg(theme::TEXT_MUTED));
            f.render_widget(
                indicator_para,
                Rect::new(
                    inner.x + inner.width.saturating_sub(8),
                    list_area.y + list_area.height.saturating_sub(1),
                    8,
                    1,
                ),
            );
        }
    }

    // Hints
    let hints = "↑↓ navigate · Enter select · Esc cancel";
    let hints_para = Paragraph::new(hints).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(
        hints_para,
        Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1),
    );
}

/// Render a permission column (allow or deny)
fn render_permission_column(
    f: &mut Frame,
    area: Rect,
    title: &str,
    tools: &[String],
    accent_color: ratatui::style::Color,
    is_adding: bool,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if is_adding {
            accent_color
        } else {
            theme::BORDER_INACTIVE
        }))
        .title(format!(" {} ", title));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if tools.is_empty() && !is_adding {
        let empty = Paragraph::new("(none)")
            .style(Style::default().fg(theme::TEXT_DIM));
        f.render_widget(empty, inner);
    } else {
        for (i, tool) in tools.iter().take(inner.height as usize).enumerate() {
            let line = Paragraph::new(format!(" • {}", tool))
                .style(Style::default().fg(theme::TEXT_PRIMARY));
            f.render_widget(line, Rect::new(inner.x, inner.y + i as u16, inner.width, 1));
        }
    }
}

/// Render the review step
fn render_review_step(f: &mut Frame, area: Rect, state: &NudgeFormState) {
    let label = Paragraph::new("Review Your Nudge")
        .style(Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD));
    f.render_widget(label, Rect::new(area.x, area.y, area.width, 1));

    let mut y = area.y + 2;

    // Title
    let title_line = Line::from(vec![
        Span::styled("Title: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(&state.title, Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD)),
    ]);
    f.render_widget(Paragraph::new(title_line), Rect::new(area.x, y, area.width, 1));
    y += 1;

    // Description
    if !state.description.is_empty() {
        let desc_line = Line::from(vec![
            Span::styled("Description: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(&state.description, Style::default().fg(theme::TEXT_MUTED)),
        ]);
        f.render_widget(Paragraph::new(desc_line), Rect::new(area.x, y, area.width, 1));
        y += 1;
    }

    // Hashtags
    if !state.hashtags.is_empty() {
        let tags: String = state.hashtags.iter().map(|t| format!("#{}", t)).collect::<Vec<_>>().join(" ");
        let tags_line = Line::from(vec![
            Span::styled("Tags: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(tags, Style::default().fg(theme::ACCENT_WARNING)),
        ]);
        f.render_widget(Paragraph::new(tags_line), Rect::new(area.x, y, area.width, 1));
        y += 1;
    }

    // Permissions summary
    let allow_count = state.permissions.allow_tools.len();
    let deny_count = state.permissions.deny_tools.len();
    if allow_count > 0 || deny_count > 0 {
        let perms_line = Line::from(vec![
            Span::styled("Permissions: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(format!("{} allowed", allow_count), Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(", ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(format!("{} denied", deny_count), Style::default().fg(theme::ACCENT_ERROR)),
        ]);
        f.render_widget(Paragraph::new(perms_line), Rect::new(area.x, y, area.width, 1));
        y += 1;
    }

    y += 1;

    // Content preview
    let content_label = Paragraph::new("Content:")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(content_label, Rect::new(area.x, y, area.width, 1));
    y += 1;

    let content_area = Rect::new(area.x, y, area.width, area.height.saturating_sub(y - area.y + 2));
    let content_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE));

    let content_inner = content_block.inner(content_area);
    f.render_widget(content_block, content_area);

    let content_para = Paragraph::new(&*state.content)
        .style(Style::default().fg(theme::TEXT_MUTED))
        .wrap(Wrap { trim: false });
    f.render_widget(content_para, content_inner);
}

/// Render step-specific hints
fn render_step_hints(f: &mut Frame, area: Rect, state: &NudgeFormState) {
    let hints = match state.step {
        NudgeFormStep::Basics => {
            "Tab cycle fields · Enter next step · Esc cancel"
        }
        NudgeFormStep::Content => {
            "Type to edit · ↑↓←→ navigate · Tab next · Shift+Tab prev · Esc cancel"
        }
        NudgeFormStep::Permissions => {
            "a add allow · d add deny · x remove · Enter next · Backspace prev · Esc cancel"
        }
        NudgeFormStep::Review => {
            if state.can_submit() {
                "Enter submit · Backspace prev · Esc cancel"
            } else {
                "Complete required fields first · Backspace prev · Esc cancel"
            }
        }
    };

    let hint_para = Paragraph::new(hints).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hint_para, area);
}
