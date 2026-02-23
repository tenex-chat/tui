use crate::ui::app::NudgeSkillSelectorItem;
use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::{BookmarkFilter, NudgeSkillSelectorState};
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{List, ListItem, Paragraph},
    Frame,
};

/// Render the unified nudge/skill selector modal.
pub fn render_nudge_skill_selector(
    f: &mut Frame,
    app: &App,
    area: Rect,
    state: &NudgeSkillSelectorState,
) {
    let selected_count = state.selected_nudge_ids.len() + state.selected_skill_ids.len();
    let filter_label = state.bookmark_filter.label();
    let title = if selected_count > 0 {
        format!(
            "Select Nudges/Skills [{}] ({} selected)",
            filter_label, selected_count
        )
    } else {
        format!("Select Nudges/Skills [{}]", filter_label)
    };

    let (popup_area, content_area) = Modal::new(&title)
        .size(ModalSize {
            max_width: 82,
            height_percent: 0.72,
        })
        .search(&state.selector.filter, "Search nudges and skills...")
        .render_frame(f, area);

    let items = app.filtered_nudge_skill_items();

    let list_area = Rect::new(
        content_area.x,
        content_area.y + 1,
        content_area.width,
        content_area.height.saturating_sub(5),
    );

    if items.is_empty() {
        let msg = if state.bookmark_filter == BookmarkFilter::BookmarkedOnly
            && state.selector.filter.is_empty()
        {
            "No bookmarked nudges or skills. Press Tab to show all, or 'b' on an item to bookmark."
        } else if state.selector.filter.is_empty() {
            "No nudges or skills available."
        } else {
            "No nudges or skills match your search."
        };
        let empty_msg = Paragraph::new(msg).style(Style::default().fg(theme::TEXT_MUTED));
        f.render_widget(empty_msg, list_area);
    } else {
        let visible_height = list_area.height as usize;
        let selected_index = state.selector.index.min(items.len().saturating_sub(1));

        let scroll_offset = if selected_index >= visible_height {
            selected_index - visible_height + 1
        } else {
            0
        };

        let rows: Vec<ListItem> = items
            .iter()
            .enumerate()
            .skip(scroll_offset)
            .take(visible_height)
            .map(|(i, item)| {
                let is_cursor = i == selected_index;
                let is_selected = is_item_selected(state, item);
                let is_bookmarked = app.is_bookmarked(item.id());
                let border_color = theme::user_color(item.pubkey());

                let mut spans = Vec::new();

                let checkbox = if is_selected { "[x] " } else { "[ ] " };
                let checkbox_style = if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_MUTED)
                };
                spans.push(Span::styled(checkbox, checkbox_style));

                // Bookmark star indicator
                if is_bookmarked {
                    spans.push(Span::styled(
                        "★ ",
                        Style::default()
                            .fg(Color::Yellow)
                            .add_modifier(Modifier::BOLD),
                    ));
                } else {
                    spans.push(Span::styled("  ", Style::default()));
                }

                if is_cursor {
                    spans.push(Span::styled("▌", Style::default().fg(border_color)));
                } else {
                    spans.push(Span::styled("│", Style::default().fg(border_color)));
                }

                let title_style = if is_cursor {
                    Style::default()
                        .fg(theme::ACCENT_PRIMARY)
                        .add_modifier(Modifier::BOLD)
                } else if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                } else {
                    Style::default().fg(theme::TEXT_PRIMARY)
                };

                let label = match item {
                    NudgeSkillSelectorItem::Nudge(_) => format!("/{}", item.title()),
                    NudgeSkillSelectorItem::Skill(_) => format!("skill/{}", item.title()),
                };
                spans.push(Span::styled(label, title_style));

                if let NudgeSkillSelectorItem::Skill(_) = item {
                    let file_count = item.skill_file_count();
                    if file_count > 0 {
                        spans.push(Span::styled(
                            format!(" [files:{}]", file_count),
                            Style::default().fg(theme::ACCENT_WARNING),
                        ));
                    }
                }

                if !item.description().is_empty() {
                    let desc = truncate_chars(item.description(), 40);
                    spans.push(Span::styled(
                        format!(" - {}", desc),
                        Style::default().fg(theme::TEXT_MUTED),
                    ));
                }

                ListItem::new(Line::from(spans))
            })
            .collect();

        f.render_widget(List::new(rows), list_area);

        if let Some(item) = items.get(selected_index) {
            let preview_area = Rect::new(
                content_area.x,
                list_area.y + list_area.height,
                content_area.width,
                2,
            );
            let preview = item.content_preview(content_area.width as usize * 2);
            let preview_widget =
                Paragraph::new(preview).style(Style::default().fg(theme::TEXT_DIM));
            f.render_widget(preview_widget, preview_area);
        }
    }

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
        Span::styled("b", Style::default().fg(Color::Yellow)),
        Span::styled(" bookmark", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Tab", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" filter", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Enter", Style::default().fg(theme::ACCENT_SUCCESS)),
        Span::styled(" confirm", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(" · ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" cancel", Style::default().fg(theme::TEXT_MUTED)),
    ];

    f.render_widget(Paragraph::new(Line::from(hint_spans)), hints_area);
}

fn is_item_selected(state: &NudgeSkillSelectorState, item: &NudgeSkillSelectorItem) -> bool {
    match item {
        NudgeSkillSelectorItem::Nudge(nudge) => {
            state.selected_nudge_ids.iter().any(|id| id == &nudge.id)
        }
        NudgeSkillSelectorItem::Skill(skill) => {
            state.selected_skill_ids.iter().any(|id| id == &skill.id)
        }
    }
}

fn truncate_chars(input: &str, max_chars: usize) -> String {
    let count = input.chars().count();
    if count <= max_chars {
        return input.to_string();
    }

    let mut truncated: String = input.chars().take(max_chars.saturating_sub(3)).collect();
    truncated.push_str("...");
    truncated
}
