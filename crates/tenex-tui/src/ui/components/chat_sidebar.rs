use crate::ui::card;
use crate::ui::format::truncate_with_ellipsis;
use crate::ui::theme;
use crate::ui::todo::{TodoState, TodoStatus};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

/// Metadata about the current conversation (from kind:513 events)
#[derive(Debug, Clone, Default)]
pub struct ConversationMetadata {
    pub title: Option<String>,
    pub status_label: Option<String>,
    pub status_current_activity: Option<String>,
}

impl ConversationMetadata {
    pub fn has_content(&self) -> bool {
        self.title.is_some() || self.status_label.is_some() || self.status_current_activity.is_some()
    }
}

/// Render the conversation sidebar on the right side of the chat.
/// Shows todos (if any) at the top, metadata below.
pub fn render_chat_sidebar(
    f: &mut Frame,
    todo_state: &TodoState,
    metadata: &ConversationMetadata,
    area: Rect,
) {
    let mut lines: Vec<Line> = Vec::new();
    let content_width = (area.width as usize).saturating_sub(4);

    // === TODOS SECTION ===
    if todo_state.has_todos() {
        render_todos_section(&mut lines, todo_state, content_width);
    }

    // === METADATA SECTION ===
    if metadata.has_content() {
        // Add separator if we had todos
        if todo_state.has_todos() {
            lines.push(Line::from(""));
        }
        render_metadata_section(&mut lines, metadata, content_width);
    }

    // === EMPTY STATE ===
    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            "No active tasks",
            theme::text_muted(),
        )));
    }

    let sidebar = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::LEFT)
                .border_style(theme::border_inactive()),
        )
        .style(Style::default().bg(theme::BG_SIDEBAR));

    f.render_widget(sidebar, area);
}

fn render_todos_section(lines: &mut Vec<Line>, todo_state: &TodoState, content_width: usize) {
    // Header with count
    let completed = todo_state.completed_count();
    let total = todo_state.items.len();
    lines.push(Line::from(vec![
        Span::styled("TODOS ", theme::text_muted()),
        Span::styled(
            format!("{}/{}", completed, total),
            Style::default().fg(theme::TEXT_DIM),
        ),
    ]));

    // Progress bar
    let filled = if total > 0 {
        (completed * content_width) / total
    } else {
        0
    };
    let empty_bar = content_width.saturating_sub(filled);
    lines.push(Line::from(vec![
        Span::styled(
            "━".repeat(filled),
            Style::default().fg(theme::ACCENT_SUCCESS),
        ),
        Span::styled(
            "━".repeat(empty_bar),
            Style::default().fg(theme::PROGRESS_EMPTY),
        ),
    ]));
    lines.push(Line::from(""));

    // Active task highlight
    if let Some(active) = todo_state.in_progress_item() {
        lines.push(Line::from(Span::styled(
            "In Progress",
            theme::todo_in_progress(),
        )));
        lines.push(Line::from(Span::styled(
            format!("  {}", truncate_with_ellipsis(&active.title, content_width.saturating_sub(2))),
            theme::text_primary(),
        )));
        if let Some(ref desc) = active.description {
            lines.push(Line::from(Span::styled(
                format!("  {}", truncate_with_ellipsis(desc, content_width.saturating_sub(2))),
                theme::text_muted(),
            )));
        }
        lines.push(Line::from(""));
    }

    // Todo items
    for item in &todo_state.items {
        let (icon, icon_style) = match item.status {
            TodoStatus::Done => ("✓", theme::todo_done()),
            TodoStatus::InProgress => ("◐", theme::todo_in_progress()),
            TodoStatus::Pending => (card::HOLLOW_BULLET_GLYPH, theme::todo_pending()),
        };

        let title_style = if item.status == TodoStatus::Done {
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::CROSSED_OUT)
        } else {
            theme::text_primary()
        };

        let title = truncate_with_ellipsis(&item.title, content_width.saturating_sub(2));
        lines.push(Line::from(vec![
            Span::styled(format!("{} ", icon), icon_style),
            Span::styled(title, title_style),
        ]));
    }
}

fn render_metadata_section<'a>(
    lines: &mut Vec<Line<'a>>,
    metadata: &'a ConversationMetadata,
    content_width: usize,
) {
    // Section header
    let separator_width = content_width.saturating_sub(10);
    lines.push(Line::from(vec![
        Span::styled("─ ", theme::text_dim()),
        Span::styled("METADATA", theme::text_muted()),
        Span::styled(" ", theme::text_dim()),
        Span::styled("─".repeat(separator_width), theme::text_dim()),
    ]));

    // Title
    if let Some(ref title) = metadata.title {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled("title", theme::text_muted())));
        // Wrap title across multiple lines if needed
        for line in wrap_text(title, content_width) {
            lines.push(Line::from(Span::styled(line, theme::text_primary())));
        }
    }

    // Status label with color coding
    if let Some(ref status) = metadata.status_label {
        lines.push(Line::from(""));
        let status_style = match status.to_lowercase().as_str() {
            "completed" | "done" => theme::status_success(),
            "in progress" | "working" => theme::status_warning(),
            "blocked" | "failed" | "error" => theme::status_error(),
            _ => theme::text_primary(),
        };
        lines.push(Line::from(vec![
            Span::styled("status ", theme::text_muted()),
            Span::styled(status.clone(), status_style),
        ]));
    }

    // Current activity
    if let Some(ref activity) = metadata.status_current_activity {
        if metadata.status_label.is_none() {
            lines.push(Line::from(""));
        }
        for line in wrap_text(activity, content_width) {
            lines.push(Line::from(Span::styled(line, theme::text_muted())));
        }
    }
}

/// Wrap text to fit within the given width
fn wrap_text(text: &str, max_width: usize) -> Vec<String> {
    if max_width == 0 {
        return vec![];
    }

    let mut result = Vec::new();
    let mut current_line = String::new();

    for word in text.split_whitespace() {
        if current_line.is_empty() {
            if word.len() > max_width {
                // Word is too long, truncate it
                result.push(truncate_with_ellipsis(word, max_width));
            } else {
                current_line = word.to_string();
            }
        } else if current_line.len() + 1 + word.len() <= max_width {
            current_line.push(' ');
            current_line.push_str(word);
        } else {
            result.push(current_line);
            if word.len() > max_width {
                result.push(truncate_with_ellipsis(word, max_width));
                current_line = String::new();
            } else {
                current_line = word.to_string();
            }
        }
    }

    if !current_line.is_empty() {
        result.push(current_line);
    }

    result
}
