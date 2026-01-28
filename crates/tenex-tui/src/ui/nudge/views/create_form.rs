//! Nudge create form - multi-step wizard for creating nudges

use crate::ui::components::{Modal, ModalSize};
use crate::ui::nudge::{NudgeFormFocus, NudgeFormState, NudgeFormStep, PermissionMode, ToolMode};
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

/// Render the permissions step (tool allow/deny/only)
fn render_permissions_step(f: &mut Frame, app: &App, area: Rect, state: &NudgeFormState) {
    use super::super::get_available_tools_from_statuses;

    let label = Paragraph::new("Tool Permissions (Optional)")
        .style(Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD));
    f.render_widget(label, Rect::new(area.x, area.y, area.width, 1));

    // Get available tools from project statuses using centralized helper
    let data_store = app.data_store.borrow();
    let available_tools = get_available_tools_from_statuses(&data_store.project_statuses);

    // Check if we're in add mode (showing tool list)
    let is_adding = state.permission_mode == PermissionMode::AddAllow
        || state.permission_mode == PermissionMode::AddDeny
        || state.permission_mode == PermissionMode::AddOnly;

    if is_adding {
        render_tool_selector(f, area, state, &available_tools);
    } else {
        // Render mode selector
        render_mode_selector(f, Rect::new(area.x, area.y + 2, area.width, 3), state);

        // Render mode-specific content below the selector
        let content_y = area.y + 6;
        let content_height = area.height.saturating_sub(8);

        if state.permissions.is_exclusive_mode() {
            // Exclusive mode: single column for only-tools
            render_exclusive_mode_content(f, Rect::new(area.x, content_y, area.width, content_height), state);
        } else {
            // Additive mode: two columns for allow/deny
            render_additive_mode_content(f, Rect::new(area.x, content_y, area.width, content_height), state);
        }
    }

    // Conflict warnings (only in Additive mode)
    if state.permissions.is_additive_mode() && !is_adding {
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

/// Render the mode selector (XOR choice between Additive and Exclusive)
fn render_mode_selector(f: &mut Frame, area: Rect, state: &NudgeFormState) {
    let is_additive = state.permissions.is_additive_mode();

    // Mode tabs
    let additive_style = if is_additive {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .bg(theme::ACCENT_PRIMARY)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let exclusive_style = if !is_additive {
        Style::default()
            .fg(theme::TEXT_PRIMARY)
            .bg(theme::ACCENT_WARNING)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    let mode_line = Line::from(vec![
        Span::styled(" Mode: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!(" [1] {} ", ToolMode::Additive.label()),
            additive_style,
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            format!(" [2] {} ", ToolMode::Exclusive.label()),
            exclusive_style,
        ),
    ]);
    f.render_widget(Paragraph::new(mode_line), Rect::new(area.x, area.y, area.width, 1));

    // Mode description
    let current_mode = if is_additive {
        ToolMode::Additive
    } else {
        ToolMode::Exclusive
    };
    let description = Paragraph::new(current_mode.description())
        .style(Style::default().fg(theme::TEXT_DIM));
    f.render_widget(description, Rect::new(area.x, area.y + 1, area.width, 1));
}

/// Render content for Additive mode (allow + deny columns)
fn render_additive_mode_content(f: &mut Frame, area: Rect, state: &NudgeFormState) {
    let col_width = (area.width / 2).saturating_sub(1);

    // Allow column
    let allow_area = Rect::new(area.x, area.y, col_width, area.height);
    render_permission_column(
        f,
        allow_area,
        "✓ Allow Tools",
        &state.permissions.allow_tools,
        theme::ACCENT_SUCCESS,
        false,
        state,
        "allow",
    );

    // Deny column
    let deny_area = Rect::new(
        area.x + col_width + 2,
        area.y,
        col_width,
        area.height,
    );
    render_permission_column(
        f,
        deny_area,
        "✗ Deny Tools",
        &state.permissions.deny_tools,
        theme::ACCENT_ERROR,
        false,
        state,
        "deny",
    );

    // Hint for selection mode
    if state.selecting_configured {
        let configured_count = state.get_configured_tools().len();
        let hint = format!("{} tools · ↑↓ select · x remove · Esc back", configured_count);
        let hint_para = Paragraph::new(hint).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(
            hint_para,
            Rect::new(area.x, area.y + area.height.saturating_sub(1), area.width, 1),
        );
    }
}

/// Render content for Exclusive mode (only-tools list)
fn render_exclusive_mode_content(f: &mut Frame, area: Rect, state: &NudgeFormState) {
    let border_color = if state.selecting_configured {
        theme::ACCENT_PRIMARY
    } else {
        theme::ACCENT_WARNING
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(border_color))
        .title(" ⚡ Exact Tools (overrides everything) ");

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Bail out gracefully if width is insufficient
    if inner.width < 6 || inner.height == 0 {
        return;
    }

    if state.permissions.only_tools.is_empty() {
        let empty_msg = Paragraph::new("No tools specified - agent will have NO tools!")
            .style(Style::default().fg(theme::ACCENT_ERROR));
        f.render_widget(empty_msg, inner);
    } else {
        // Show tools as a vertical list for better selection UX
        for (i, tool) in state.permissions.only_tools.iter().take(inner.height as usize).enumerate() {
            let y = inner.y.saturating_add(i as u16);
            let is_selected = state.selecting_configured && i == state.configured_tool_index;

            let style = if is_selected {
                Style::default().fg(theme::TEXT_PRIMARY).bg(theme::ACCENT_WARNING)
            } else {
                Style::default().fg(theme::ACCENT_WARNING)
            };

            // Safe width calculation with saturating_sub
            // Use char-safe truncation to avoid UTF-8 boundary panic
            let max_tool_len = inner.width.saturating_sub(4) as usize;
            let char_count = tool.chars().count();
            let display = if char_count > max_tool_len {
                let truncated: String = tool.chars().take(max_tool_len.saturating_sub(1)).collect();
                format!("• {}…", truncated)
            } else {
                format!("• {}", tool)
            };

            let line = Paragraph::new(display).style(style);
            f.render_widget(line, Rect::new(inner.x, y, inner.width, 1));
        }
    }

    // Tool count and selection hint
    let hint_text = if state.selecting_configured {
        format!("{} tools · ↑↓ select · x remove · Esc back", state.permissions.only_tools.len())
    } else {
        format!("{} tools selected", state.permissions.only_tools.len())
    };
    let count_para = Paragraph::new(hint_text).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(
        count_para,
        Rect::new(
            area.x,
            area.y + area.height.saturating_sub(1),
            area.width.saturating_sub(2),
            1,
        ),
    );
}

/// Render the tool selector when adding allow/deny/only tools
fn render_tool_selector(f: &mut Frame, area: Rect, state: &NudgeFormState, available_tools: &[String]) {
    let (mode_label, accent_color) = match state.permission_mode {
        PermissionMode::AddAllow => ("Allow", theme::ACCENT_SUCCESS),
        PermissionMode::AddDeny => ("Deny", theme::ACCENT_ERROR),
        PermissionMode::AddOnly => ("Only", theme::ACCENT_WARNING),
        PermissionMode::Browse => ("Browse", theme::TEXT_MUTED), // Shouldn't happen
    };

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
            let is_only = state.permissions.is_only(tool);

            let style = if is_selected {
                Style::default().fg(theme::TEXT_PRIMARY).bg(accent_color)
            } else if is_only {
                Style::default().fg(theme::ACCENT_WARNING)
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
            let prefix = if is_only {
                "⚡ "
            } else if is_allowed && is_denied {
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
    state: &NudgeFormState,
    column_type: &str, // "allow" or "deny"
) {
    let is_selecting_this = state.selecting_configured && !is_adding;

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(if is_adding || is_selecting_this {
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
        // Calculate which tool indices belong to this column
        let configured = state.get_configured_tools();
        let allow_count = state.permissions.allow_tools.len();

        for (i, tool) in tools.iter().take(inner.height as usize).enumerate() {
            // Determine if this tool is selected
            let global_idx = if column_type == "allow" {
                i
            } else {
                allow_count + i
            };

            let is_selected = state.selecting_configured
                && state.configured_tool_index < configured.len()
                && global_idx == state.configured_tool_index;

            let style = if is_selected {
                Style::default().fg(theme::TEXT_PRIMARY).bg(accent_color)
            } else {
                Style::default().fg(theme::TEXT_PRIMARY)
            };

            let line = Paragraph::new(format!(" • {}", tool)).style(style);
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

    // Render any validation errors first
    let error_height = render_validation_errors(f, Rect::new(area.x, y, area.width, 4), state);
    if error_height > 0 {
        y += error_height + 1; // Add spacing after errors
    }

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
    if state.permissions.is_exclusive_mode() {
        let only_count = state.permissions.only_tools.len();
        if only_count > 0 {
            let perms_line = Line::from(vec![
                Span::styled("Mode: ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled("EXCLUSIVE", Style::default().fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD)),
                Span::styled(" - ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(format!("{} exact tools", only_count), Style::default().fg(theme::ACCENT_WARNING)),
            ]);
            f.render_widget(Paragraph::new(perms_line), Rect::new(area.x, y, area.width, 1));
            y += 1;

            // Show the tools
            let tools_preview: String = state.permissions.only_tools.iter()
                .take(5)
                .cloned()
                .collect::<Vec<_>>()
                .join(", ");
            let suffix = if only_count > 5 { format!(" +{} more", only_count - 5) } else { String::new() };
            let tools_line = Line::from(vec![
                Span::styled("  Tools: ", Style::default().fg(theme::TEXT_DIM)),
                Span::styled(tools_preview, Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(suffix, Style::default().fg(theme::TEXT_DIM)),
            ]);
            f.render_widget(Paragraph::new(tools_line), Rect::new(area.x, y, area.width, 1));
            y += 1;
        } else {
            let warning_line = Line::from(vec![
                Span::styled("⚠ Mode: ", Style::default().fg(theme::ACCENT_ERROR)),
                Span::styled("EXCLUSIVE with NO tools", Style::default().fg(theme::ACCENT_ERROR).add_modifier(Modifier::BOLD)),
                Span::styled(" - agent will have no tools!", Style::default().fg(theme::ACCENT_ERROR)),
            ]);
            f.render_widget(Paragraph::new(warning_line), Rect::new(area.x, y, area.width, 1));
            y += 1;
        }
    } else {
        let allow_count = state.permissions.allow_tools.len();
        let deny_count = state.permissions.deny_tools.len();
        if allow_count > 0 || deny_count > 0 {
            let perms_line = Line::from(vec![
                Span::styled("Mode: ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled("ADDITIVE", Style::default().fg(theme::ACCENT_PRIMARY)),
                Span::styled(" - ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(format!("{} allowed", allow_count), Style::default().fg(theme::ACCENT_SUCCESS)),
                Span::styled(", ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(format!("{} denied", deny_count), Style::default().fg(theme::ACCENT_ERROR)),
            ]);
            f.render_widget(Paragraph::new(perms_line), Rect::new(area.x, y, area.width, 1));
            y += 1;
        }
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
            if state.permissions.is_exclusive_mode() {
                "1/2 switch mode · o add only-tool · x remove · Enter next · Backspace prev · Esc cancel"
            } else {
                "1/2 switch mode · a allow · d deny · x remove · Enter next · Backspace prev · Esc cancel"
            }
        }
        NudgeFormStep::Review => {
            let errors = state.get_submission_errors();
            if errors.is_empty() {
                "Enter submit · Backspace prev · Esc cancel"
            } else {
                // Show first validation error as hint
                "⚠ Fix errors above · Backspace prev · Esc cancel"
            }
        }
    };

    let hint_para = Paragraph::new(hints).style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hint_para, area);
}

/// Render validation errors in the review step
fn render_validation_errors(f: &mut Frame, area: Rect, state: &NudgeFormState) -> u16 {
    let errors = state.get_submission_errors();
    if errors.is_empty() {
        return 0;
    }

    let mut y = area.y;
    for error in &errors {
        let error_line = Line::from(vec![
            Span::styled("⚠ ", Style::default().fg(theme::ACCENT_ERROR)),
            Span::styled(error.as_str(), Style::default().fg(theme::ACCENT_ERROR)),
        ]);
        f.render_widget(Paragraph::new(error_line), Rect::new(area.x, y, area.width, 1));
        y += 1;
    }

    errors.len() as u16
}
