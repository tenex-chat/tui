use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::DebugStatsState;
use crate::ui::theme;
use crate::ui::App;
use ratatui::{
    layout::Rect,
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use tenex_core::stats::query_ndb_stats;

fn kind_name(kind: u16) -> &'static str {
    match kind {
        1 => "Messages",
        513 => "Conv Metadata",
        4129 => "Agent Lessons",
        4199 => "Agent Defs",
        4201 => "Nudges",
        24010 => "Project Status",
        24133 => "Operations",
        31933 => "Projects",
        _ => "Unknown",
    }
}

fn format_project_name(a_tag: &str) -> String {
    if a_tag == "(global)" || a_tag.is_empty() {
        "(global)".to_string()
    } else {
        // Extract project name from a-tag: 31933:pubkey:name -> name
        a_tag.split(':').nth(2).unwrap_or(a_tag).to_string()
    }
}

pub fn render_debug_stats(f: &mut Frame, area: Rect, app: &App, state: &DebugStatsState) {
    // Get network stats from event_stats
    let network_stats = app.event_stats.snapshot();

    // Get cache stats from nostrdb
    let cache_stats = query_ndb_stats(&app.db.ndb);

    // Build content lines
    let mut lines: Vec<Line> = Vec::new();

    // Header
    lines.push(Line::from(vec![
        Span::styled(
            "═══ Network Events Received ═══",
            Style::default().fg(theme::ACCENT_PRIMARY),
        ),
    ]));
    lines.push(Line::from(""));

    // Network stats by kind
    let network_by_kind = network_stats.kinds_by_count();
    if network_by_kind.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No events received yet",
            Style::default().fg(theme::TEXT_MUTED),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Kind", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw("                  "),
            Span::styled("Count", Style::default().fg(theme::TEXT_MUTED)),
        ]));
        lines.push(Line::from("  ────────────────────────────"));

        for (kind, count) in &network_by_kind {
            let name = kind_name(*kind);
            lines.push(Line::from(format!(
                "  {:6} {:15} {:>6}",
                kind, name, count
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(format!(
            "  Total: {}",
            network_stats.total
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Network stats by project
    lines.push(Line::from(vec![
        Span::styled(
            "═══ Network Events by Project ═══",
            Style::default().fg(theme::ACCENT_PRIMARY),
        ),
    ]));
    lines.push(Line::from(""));

    let by_project = network_stats.by_project();
    if by_project.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No events received yet",
            Style::default().fg(theme::TEXT_MUTED),
        )));
    } else {
        let mut projects: Vec<_> = by_project.iter().collect();
        projects.sort_by(|a, b| {
            let total_a: u64 = a.1.values().sum();
            let total_b: u64 = b.1.values().sum();
            total_b.cmp(&total_a)
        });

        for (project_a_tag, kinds) in projects {
            let project_name = format_project_name(project_a_tag);
            let total: u64 = kinds.values().sum();
            lines.push(Line::from(vec![
                Span::styled(
                    format!("  {} ", project_name),
                    Style::default().fg(theme::TEXT_PRIMARY),
                ),
                Span::styled(
                    format!("({})", total),
                    Style::default().fg(theme::TEXT_MUTED),
                ),
            ]));

            let mut kind_list: Vec<_> = kinds.iter().collect();
            kind_list.sort_by(|a, b| b.1.cmp(a.1));
            for (kind, count) in kind_list {
                lines.push(Line::from(format!(
                    "    {:6} {:12} {:>5}",
                    kind,
                    kind_name(*kind),
                    count
                )));
            }
            lines.push(Line::from(""));
        }
    }

    lines.push(Line::from(""));

    // Cache stats header
    lines.push(Line::from(vec![
        Span::styled(
            "═══ NostrDB Cache ═══",
            Style::default().fg(theme::ACCENT_SUCCESS),
        ),
    ]));
    lines.push(Line::from(""));

    if cache_stats.is_empty() {
        lines.push(Line::from(Span::styled(
            "  Cache empty",
            Style::default().fg(theme::TEXT_MUTED),
        )));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Kind", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw("                  "),
            Span::styled("Cached", Style::default().fg(theme::TEXT_MUTED)),
        ]));
        lines.push(Line::from("  ────────────────────────────"));

        let mut cache_list: Vec<_> = cache_stats.iter().collect();
        cache_list.sort_by(|a, b| b.1.cmp(a.1));

        let total: u64 = cache_list.iter().map(|(_, c)| **c).sum();

        for (kind, count) in &cache_list {
            let name = kind_name(**kind);
            lines.push(Line::from(format!(
                "  {:6} {:15} {:>6}",
                kind, name, count
            )));
        }

        lines.push(Line::from(""));
        lines.push(Line::from(format!("  Total: {}", total)));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Footer
    lines.push(Line::from(Span::styled(
        "Press Esc to close",
        Style::default().fg(theme::TEXT_MUTED),
    )));

    // Calculate visible lines based on scroll
    let visible_lines: Vec<Line> = lines
        .into_iter()
        .skip(state.scroll_offset)
        .collect();

    Modal::new("Debug Stats")
        .size(ModalSize {
            max_width: 60,
            height_percent: 0.85,
        })
        .render(f, area, |f, content_area| {
            let paragraph = Paragraph::new(visible_lines)
                .style(Style::default().fg(theme::TEXT_PRIMARY));
            f.render_widget(paragraph, content_area);
        });
}
