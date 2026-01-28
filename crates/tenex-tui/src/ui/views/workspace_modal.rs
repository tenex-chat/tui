//! Workspace manager modal view for creating, editing, and switching workspaces.

use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::{WorkspaceFocus, WorkspaceMode, WorkspaceManagerState};
use crate::ui::theme;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph},
    Frame,
};
use tenex_core::models::{Project, Workspace};

/// Render the workspace manager modal
pub fn render_workspace_manager(
    f: &mut Frame,
    area: Rect,
    state: &WorkspaceManagerState,
    workspaces: &[Workspace],
    projects: &[Project],
    active_workspace_id: Option<&str>,
) {
    match state.mode {
        WorkspaceMode::List => {
            render_list_mode(f, area, state, workspaces, active_workspace_id);
        }
        WorkspaceMode::Create | WorkspaceMode::Edit => {
            render_edit_mode(f, area, state, projects);
        }
        WorkspaceMode::Delete => {
            render_delete_mode(f, area, state, workspaces);
        }
    }
}

fn render_list_mode(
    f: &mut Frame,
    area: Rect,
    state: &WorkspaceManagerState,
    workspaces: &[Workspace],
    active_workspace_id: Option<&str>,
) {
    let title = if workspaces.is_empty() {
        "Workspaces".to_string()
    } else {
        format!("Workspaces ({} total)", workspaces.len())
    };

    let (_popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 60,
            height_percent: 0.6,
        })
        .render_frame(f, area);

    // List area
    let list_area = Rect::new(
        content_area.x,
        content_area.y,
        content_area.width,
        content_area.height.saturating_sub(4),
    );

    if workspaces.is_empty() {
        let msg = "No workspaces defined. Press 'n' to create one.";
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        // Sort workspaces: pinned first, then by name
        let mut sorted_workspaces: Vec<&Workspace> = workspaces.iter().collect();
        sorted_workspaces.sort_by(|a, b| {
            match (a.pinned, b.pinned) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => a.name.cmp(&b.name),
            }
        });

        let visible_height = list_area.height as usize;
        let selected_index = state.selected_index.min(sorted_workspaces.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let items: Vec<ListItem> = sorted_workspaces
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, workspace)| {
                let is_selected = i == selected_index;
                let is_active = active_workspace_id == Some(workspace.id.as_str());

                let mut spans = vec![];

                // Selection indicator
                if is_selected {
                    spans.push(Span::styled("▌ ", Style::default().fg(theme::ACCENT_PRIMARY)));
                } else {
                    spans.push(Span::styled("  ", Style::default()));
                }

                // Pin icon
                if workspace.pinned {
                    spans.push(Span::styled("📌 ", Style::default()));
                }

                // Workspace name
                let name_style = if is_active {
                    Style::default()
                        .fg(theme::ACCENT_SUCCESS)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(&workspace.name, name_style));

                // Project count
                let count_style = Style::default().fg(theme::TEXT_MUTED);
                spans.push(Span::styled(
                    format!(" ({} projects)", workspace.project_ids.len()),
                    count_style,
                ));

                // Active indicator
                if is_active {
                    spans.push(Span::styled(" ✓", Style::default().fg(theme::ACCENT_SUCCESS)));
                }

                let line = Line::from(spans);
                let style = if is_selected {
                    Style::default().bg(theme::BG_SELECTED)
                } else {
                    Style::default()
                };

                ListItem::new(line).style(style)
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);
    }

    // Help text at bottom
    let help_area = Rect::new(
        content_area.x,
        content_area.y + content_area.height.saturating_sub(3),
        content_area.width,
        2,
    );
    let help_text = Line::from(vec![
        Span::styled("Enter", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" switch • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("n", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" new • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("e", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" edit • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("d", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" delete • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("p", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" pin • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Bksp", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" all projects", Style::default().fg(theme::TEXT_MUTED)),
    ]);
    f.render_widget(Paragraph::new(help_text), help_area);
}

fn render_edit_mode(
    f: &mut Frame,
    area: Rect,
    state: &WorkspaceManagerState,
    projects: &[Project],
) {
    let title = if state.mode == WorkspaceMode::Create {
        "Create Workspace"
    } else {
        "Edit Workspace"
    };

    let (_popup_area, content_area) = Modal::new(title)
        .size(ModalSize {
            max_width: 70,
            height_percent: 0.7,
        })
        .render_frame(f, area);

    // Name input area (top portion)
    let name_area = Rect::new(
        content_area.x,
        content_area.y,
        content_area.width,
        3,
    );

    let name_focused = state.focus == WorkspaceFocus::Name;
    let name_border_color = if name_focused {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };

    let name_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(name_border_color))
        .title(Span::styled(
            " Name ",
            Style::default().fg(if name_focused { theme::ACCENT_PRIMARY } else { theme::TEXT_MUTED }),
        ));

    let name_text = if state.editing_name.is_empty() && name_focused {
        Paragraph::new("Enter workspace name...")
            .style(Style::default().fg(theme::TEXT_MUTED))
            .block(name_block)
    } else {
        Paragraph::new(state.editing_name.as_str())
            .style(Style::default().fg(theme::TEXT_PRIMARY))
            .block(name_block)
    };
    f.render_widget(name_text, name_area);

    // Projects list area (rest of content)
    let projects_area = Rect::new(
        content_area.x,
        content_area.y + 4,
        content_area.width,
        content_area.height.saturating_sub(8),
    );

    let projects_focused = state.focus == WorkspaceFocus::Projects;
    let projects_border_color = if projects_focused {
        theme::ACCENT_PRIMARY
    } else {
        theme::BORDER_INACTIVE
    };

    let projects_block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(projects_border_color))
        .title(Span::styled(
            format!(" Projects ({} selected) ", state.editing_project_ids.len()),
            Style::default().fg(if projects_focused { theme::ACCENT_PRIMARY } else { theme::TEXT_MUTED }),
        ));

    let inner_area = projects_block.inner(projects_area);
    f.render_widget(projects_block, projects_area);

    // Render project list
    let visible_height = inner_area.height as usize;
    let selected_index = state.project_selector_index.min(projects.len().saturating_sub(1));

    let scroll_offset = if selected_index >= visible_height {
        selected_index - visible_height + 1
    } else {
        0
    };

    let items: Vec<ListItem> = projects
        .iter()
        .enumerate()
        .skip(scroll_offset)
        .take(visible_height)
        .map(|(i, project)| {
            let is_cursor = i == selected_index && projects_focused;
            let is_selected = state.editing_project_ids.contains(&project.a_tag());

            let mut spans = vec![];

            // Cursor indicator
            if is_cursor {
                spans.push(Span::styled("▌ ", Style::default().fg(theme::ACCENT_PRIMARY)));
            } else {
                spans.push(Span::styled("  ", Style::default()));
            }

            // Checkbox
            let checkbox = if is_selected { "[✓]" } else { "[ ]" };
            let checkbox_style = if is_selected {
                Style::default().fg(theme::ACCENT_SUCCESS)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            };
            spans.push(Span::styled(checkbox, checkbox_style));
            spans.push(Span::styled(" ", Style::default()));

            // Project name
            let name_style = if is_cursor {
                Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(theme::TEXT_PRIMARY)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            };
            spans.push(Span::styled(&project.name, name_style));

            let line = Line::from(spans);
            let style = if is_cursor {
                Style::default().bg(theme::BG_SELECTED)
            } else {
                Style::default()
            };

            ListItem::new(line).style(style)
        })
        .collect();

    let list = List::new(items);
    f.render_widget(list, inner_area);

    // Help text at bottom
    let help_area = Rect::new(
        content_area.x,
        content_area.y + content_area.height.saturating_sub(3),
        content_area.width,
        2,
    );
    let help_text = Line::from(vec![
        Span::styled("Tab", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" switch focus • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Space", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" toggle project • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled(" save • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
    ]);
    f.render_widget(Paragraph::new(help_text), help_area);
}

fn render_delete_mode(
    f: &mut Frame,
    area: Rect,
    state: &WorkspaceManagerState,
    workspaces: &[Workspace],
) {
    let workspace_name = workspaces
        .get(state.selected_index)
        .map(|w| w.name.as_str())
        .unwrap_or("Unknown");

    let title = "Delete Workspace?";

    let (_popup_area, content_area) = Modal::new(title)
        .size(ModalSize {
            max_width: 50,
            height_percent: 0.3,
        })
        .render_frame(f, area);

    // Confirmation message
    let msg_area = Rect::new(
        content_area.x,
        content_area.y + 1,
        content_area.width,
        3,
    );

    let msg = Paragraph::new(format!(
        "Are you sure you want to delete \"{}\"?\n\nThis cannot be undone.",
        workspace_name
    ))
    .style(Style::default().fg(theme::TEXT_PRIMARY))
    .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(msg, msg_area);

    // Help text at bottom
    let help_area = Rect::new(
        content_area.x,
        content_area.y + content_area.height.saturating_sub(2),
        content_area.width,
        1,
    );
    let help_text = Line::from(vec![
        Span::styled("Enter/d", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" confirm delete • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
    ]);
    f.render_widget(Paragraph::new(help_text), help_area);
}
