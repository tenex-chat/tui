use crate::ui::components::{render_modal_items, Modal, ModalItem, ModalSize};
use crate::ui::modal::{
    BunkerApprovalAction, BunkerApprovalState, BunkerAuditState, BunkerRulesState,
};
use crate::ui::{theme, App};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

pub fn render_bunker_approval_modal(f: &mut Frame, area: Rect, state: &BunkerApprovalState) {
    let (popup_area, content_area) = Modal::new("Bunker Signing Request")
        .size(ModalSize {
            max_width: 86,
            height_percent: 0.68,
        })
        .render_frame(f, area);

    let requester_short = short_pubkey(&state.request.requester_pubkey);
    let kind_label = state
        .request
        .event_kind
        .map(|k| k.to_string())
        .unwrap_or_else(|| "unknown".to_string());
    let preview = state
        .request
        .event_content
        .as_deref()
        .map(truncate_preview)
        .unwrap_or_else(|| "(no content)".to_string());

    let summary_area = Rect::new(
        content_area.x + 2,
        content_area.y,
        content_area.width.saturating_sub(4),
        3,
    );
    let summary = vec![
        Line::from(vec![
            Span::styled("Requester: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(requester_short, Style::default().fg(theme::ACCENT_SPECIAL)),
            Span::styled("   Kind: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(kind_label, Style::default().fg(theme::ACCENT_SPECIAL)),
        ]),
        Line::from(vec![
            Span::styled("Request ID: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(
                state.request.request_id.as_str(),
                Style::default().fg(theme::TEXT_DIM),
            ),
        ]),
        Line::from(vec![
            Span::styled("Preview: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(preview, Style::default().fg(theme::TEXT_PRIMARY)),
        ]),
    ];
    f.render_widget(Paragraph::new(summary), summary_area);

    let checkbox_label = format!(
        "{} Always approve this requester + kind",
        if state.always_approve { "[x]" } else { "[ ]" }
    );
    let mut actions: Vec<ModalItem> = vec![ModalItem::new(checkbox_label)
        .with_shortcut("space".to_string())
        .selected(state.selected_index == 0)];
    actions.extend(
        BunkerApprovalAction::ALL
            .iter()
            .enumerate()
            .map(|(idx, action)| {
                ModalItem::new(action.label())
                    .with_shortcut(action.hotkey().to_string())
                    .selected(idx + 1 == state.selected_index)
            }),
    );

    let actions_area = Rect::new(
        content_area.x,
        content_area.y + 4,
        content_area.width,
        content_area.height.saturating_sub(5),
    );
    render_modal_items(f, actions_area, &actions);

    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("a approve · r reject · space toggle always-approve · esc reject")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

pub fn render_bunker_rules_modal(f: &mut Frame, app: &App, area: Rect, state: &BunkerRulesState) {
    let (popup_area, content_area) = Modal::new("Bunker Auto-Approve Rules")
        .size(ModalSize {
            max_width: 86,
            height_percent: 0.68,
        })
        .render_frame(f, area);

    let rules = &app.bunker_auto_approve_rules;
    if rules.is_empty() {
        let empty_area = Rect::new(
            content_area.x + 2,
            content_area.y + 1,
            content_area.width.saturating_sub(4),
            1,
        );
        let empty = Paragraph::new("No rules configured").style(
            Style::default()
                .fg(theme::TEXT_DIM)
                .add_modifier(Modifier::ITALIC),
        );
        f.render_widget(empty, empty_area);
    } else {
        let items: Vec<ModalItem> = rules
            .iter()
            .enumerate()
            .map(|(idx, rule)| {
                let label = format!(
                    "{} · kind {}",
                    short_pubkey(&rule.requester_pubkey),
                    rule.event_kind
                        .map(|kind| kind.to_string())
                        .unwrap_or_else(|| "any".to_string())
                );
                ModalItem::new(label).selected(idx == state.selected_index)
            })
            .collect();
        render_modal_items(f, content_area, &items);
    }

    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("↑↓ navigate · d delete · esc back")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

pub fn render_bunker_audit_modal(f: &mut Frame, app: &App, area: Rect, state: &BunkerAuditState) {
    let (popup_area, content_area) = Modal::new("Bunker Session Audit")
        .size(ModalSize {
            max_width: 96,
            height_percent: 0.72,
        })
        .render_frame(f, area);

    if app.bunker_audit_entries.is_empty() {
        let empty_area = Rect::new(
            content_area.x + 2,
            content_area.y + 1,
            content_area.width.saturating_sub(4),
            1,
        );
        let empty = Paragraph::new("No audit entries in this bunker session").style(
            Style::default()
                .fg(theme::TEXT_DIM)
                .add_modifier(Modifier::ITALIC),
        );
        f.render_widget(empty, empty_area);
    } else {
        let items: Vec<ModalItem> = app
            .bunker_audit_entries
            .iter()
            .enumerate()
            .skip(state.scroll_offset)
            .map(|(idx, entry)| {
                let preview = entry
                    .event_content_preview
                    .as_deref()
                    .map(truncate_preview)
                    .unwrap_or_else(|| "(no preview)".to_string());
                let label = format!(
                    "{} · {} · kind {} · {}",
                    entry.decision,
                    short_pubkey(&entry.requester_pubkey),
                    entry
                        .event_kind
                        .map(|kind| kind.to_string())
                        .unwrap_or_else(|| "n/a".to_string()),
                    preview
                );
                ModalItem::new(label).selected(idx == state.selected_index)
            })
            .collect();
        render_modal_items(f, content_area, &items);
    }

    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("↑↓ navigate · r refresh · esc back")
        .style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}

fn short_pubkey(pubkey: &str) -> String {
    if pubkey.len() > 16 {
        format!("{}...{}", &pubkey[..8], &pubkey[pubkey.len() - 8..])
    } else {
        pubkey.to_string()
    }
}

fn truncate_preview(text: &str) -> String {
    const MAX: usize = 80;
    if text.chars().count() > MAX {
        let preview: String = text.chars().take(MAX).collect();
        format!("{}...", preview)
    } else {
        text.to_string()
    }
}
