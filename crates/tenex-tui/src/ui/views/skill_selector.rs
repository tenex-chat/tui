use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::SkillSelectorState;
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
    Frame,
};

/// Render the skill selector modal
pub fn render_skill_selector(f: &mut Frame, app: &App, area: Rect, state: &SkillSelectorState) {
    // Header with selection count
    let selected_count = state.selected_skill_ids.len();
    let title = if selected_count > 0 {
        format!("Select Skills ({} selected)", selected_count)
    } else {
        "Select Skills".to_string()
    };

    let (popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 70,
            height_percent: 0.7,
        })
        .search(&state.selector.filter, "Search skills...")
        .render_frame(f, area);

    // Get filtered skills
    let skills = app.filtered_skills();

    // List area
    let list_area = Rect::new(
        content_area.x,
        content_area.y + 1,
        content_area.width,
        content_area.height.saturating_sub(5),
    );

    if skills.is_empty() {
        let msg = if state.selector.filter.is_empty() {
            "No skills available."
        } else {
            "No skills match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state.selector.index.min(skills.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let items: Vec<ListItem> = skills
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, skill)| {
                let is_cursor = i == selected_index;
                let is_selected = state.selected_skill_ids.contains(&skill.id);
                let border_color = theme::user_color(&skill.pubkey);

                let mut spans = vec![];

                // Checkbox
                let checkbox = if is_selected { "[✓] " } else { "[ ] " };
                let checkbox_style = if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_MUTED)
                };
                spans.push(Span::styled(checkbox, checkbox_style));

                // Left border indicator
                if is_cursor {
                    spans.push(Span::styled("▌", Style::default().fg(border_color)));
                } else {
                    spans.push(Span::styled("│", Style::default().fg(border_color)));
                }

                // Skill title with / prefix
                let title_style = if is_cursor {
                    Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };
                spans.push(Span::styled(format!("/{}", skill.title), title_style));

                // File attachment indicator (📎N) - shows when skill has file attachments
                if !skill.file_ids.is_empty() {
                    let file_indicator = format!(" \u{1F4CE}{}", skill.file_ids.len());
                    spans.push(Span::styled(file_indicator, Style::default().fg(theme::ACCENT_WARNING)));
                }

                // Description preview (character-safe truncation)
                if !skill.description.is_empty() {
                    let char_count = skill.description.chars().count();
                    let desc_preview = if char_count > 40 {
                        let truncated: String = skill.description.chars().take(37).collect();
                        format!(" - {}...", truncated)
                    } else {
                        format!(" - {}", skill.description)
                    };
                    spans.push(Span::styled(desc_preview, Style::default().fg(theme::TEXT_MUTED)));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        let list = List::new(items);
        f.render_widget(list, list_area);

        // Show content preview of cursor-selected skill
        if let Some(skill) = skills.get(selected_index) {
            let preview_area = Rect::new(
                content_area.x,
                list_area.y + list_area.height,
                content_area.width,
                2,
            );
            let content_preview = skill.content_preview(content_area.width as usize * 2);
            let preview = Paragraph::new(content_preview)
                .style(Style::default().fg(theme::TEXT_DIM));
            f.render_widget(preview, preview_area);
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
        Span::styled("Space", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" toggle", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled(" confirm", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
    ];

    let hints = Paragraph::new(Line::from(hint_spans));
    f.render_widget(hints, hints_area);
}
