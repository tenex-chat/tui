use crate::ui::components::{Modal, ModalSize};
use crate::ui::modal::{DebugStatsState, DebugStatsTab};
use crate::ui::theme;
use crate::ui::App;
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use tenex_core::stats::{query_ndb_stats, query_project_thread_stats, NegentropySyncStatus};

fn kind_name(kind: u16) -> &'static str {
    match kind {
        1 => "Messages",
        513 => "Conv Metadata",
        4129 => "Agent Lessons",
        4199 => "Agent Defs",
        4201 => "Nudges",
        24010 => "Project Status",
        24133 => "Operations",
        30023 => "Long-form",
        31933 => "Projects",
        _ => "Unknown",
    }
}

/// Format a duration as a human-readable "time ago" string
fn format_time_ago(instant: std::time::Instant) -> String {
    let elapsed = instant.elapsed();
    if elapsed.as_secs() < 60 {
        format!("{}s ago", elapsed.as_secs())
    } else if elapsed.as_secs() < 3600 {
        format!("{}m ago", elapsed.as_secs() / 60)
    } else {
        format!("{}h ago", elapsed.as_secs() / 3600)
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

/// Render the tab bar at the top of the modal
fn render_tab_bar(active_tab: DebugStatsTab) -> Vec<Line<'static>> {
    let mut spans = Vec::new();
    spans.push(Span::raw("  "));

    for (i, tab) in DebugStatsTab::ALL.iter().enumerate() {
        if i > 0 {
            spans.push(Span::styled(" │ ", Style::default().fg(theme::BORDER_INACTIVE)));
        }

        let label = format!("[{}] {}", i + 1, tab.label());
        if *tab == active_tab {
            spans.push(Span::styled(
                label,
                Style::default()
                    .fg(theme::ACCENT_PRIMARY)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            spans.push(Span::styled(label, Style::default().fg(theme::TEXT_MUTED)));
        }
    }

    vec![
        Line::from(spans),
        Line::from(Span::styled(
            "  ─────────────────────────────────────────",
            Style::default().fg(theme::BORDER_INACTIVE),
        )),
        Line::from(""),
    ]
}

/// Render the Events tab content
fn render_events_tab(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    // Get network stats from event_stats
    let network_stats = app.event_stats.snapshot();

    // Get cache stats from nostrdb
    let cache_stats = query_ndb_stats(&app.db.ndb);

    // Header
    lines.push(Line::from(vec![Span::styled(
        "═══ Network Events Received ═══",
        Style::default().fg(theme::ACCENT_PRIMARY),
    )]));
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
        lines.push(Line::from(format!("  Total: {}", network_stats.total)));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Network stats by project
    lines.push(Line::from(vec![Span::styled(
        "═══ Network Events by Project ═══",
        Style::default().fg(theme::ACCENT_PRIMARY),
    )]));
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
                Span::styled(format!("({})", total), Style::default().fg(theme::TEXT_MUTED)),
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
    lines.push(Line::from(vec![Span::styled(
        "═══ NostrDB Cache ═══",
        Style::default().fg(theme::ACCENT_SUCCESS),
    )]));
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

    lines
}

/// Render the Subscriptions tab content with sidebar
fn render_subscriptions_tab(app: &App, state: &DebugStatsState, content_width: u16) -> Vec<Line<'static>> {
    use std::collections::HashMap;

    let mut lines: Vec<Line> = Vec::new();

    // Get subscription stats
    let sub_stats = app.subscription_stats.snapshot();

    // Header
    lines.push(Line::from(vec![Span::styled(
        "═══ Active Subscriptions ═══",
        Style::default().fg(theme::ACCENT_PRIMARY),
    )]));
    lines.push(Line::from(""));

    if sub_stats.count() == 0 {
        lines.push(Line::from(Span::styled(
            "  No active subscriptions",
            Style::default().fg(theme::TEXT_MUTED),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Subscriptions will appear here after connecting.",
            Style::default().fg(theme::TEXT_MUTED),
        )));
        return lines;
    }

    // Summary
    lines.push(Line::from(vec![
        Span::styled("  Total: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{} subscriptions", sub_stats.count()),
            Style::default().fg(theme::TEXT_PRIMARY),
        ),
        Span::styled(" • ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{} events received", sub_stats.total_events()),
            Style::default().fg(theme::ACCENT_SUCCESS),
        ),
    ]));
    lines.push(Line::from(""));

    // Calculate layout
    let sidebar_width = 28usize;
    let separator_width = 3usize;
    let content_start = sidebar_width + separator_width;
    let right_width = (content_width as usize).saturating_sub(content_start);

    // Build project list for sidebar
    let mut global_subs: Vec<(&String, &tenex_core::stats::SubscriptionInfo)> = Vec::new();
    let mut by_project: HashMap<String, Vec<(&String, &tenex_core::stats::SubscriptionInfo)>> =
        HashMap::new();

    for (sub_id, info) in sub_stats.by_events_received() {
        if let Some(ref a_tag) = info.project_a_tag {
            by_project
                .entry(a_tag.clone())
                .or_default()
                .push((sub_id, info));
        } else {
            global_subs.push((sub_id, info));
        }
    }

    // Sort projects by event count (descending), then by name (ascending) for stability
    let mut projects: Vec<_> = by_project.into_iter().collect();
    projects.sort_by(|a, b| {
        let total_a: u64 = a.1.iter().map(|(_, i)| i.events_received).sum();
        let total_b: u64 = b.1.iter().map(|(_, i)| i.events_received).sum();
        match total_b.cmp(&total_a) {
            std::cmp::Ordering::Equal => a.0.cmp(&b.0), // Secondary sort by name
            other => other,
        }
    });

    // Build sidebar items: [All, Global, Project1, Project2, ...]
    let mut sidebar_items: Vec<(String, Option<String>, usize, u64)> = Vec::new(); // (display_name, a_tag, sub_count, event_count)

    // "All" option
    let total_events: u64 = sub_stats.subscriptions.values().map(|i| i.events_received).sum();
    sidebar_items.push(("All".to_string(), None, sub_stats.count(), total_events));

    // Global subscriptions (if any)
    if !global_subs.is_empty() {
        let global_events: u64 = global_subs.iter().map(|(_, i)| i.events_received).sum();
        sidebar_items.push(("Global".to_string(), Some("__global__".to_string()), global_subs.len(), global_events));
    }

    // Projects
    for (a_tag, subs) in &projects {
        let project_name = format_project_name(a_tag);
        let event_count: u64 = subs.iter().map(|(_, i)| i.events_received).sum();
        sidebar_items.push((project_name, Some(a_tag.clone()), subs.len(), event_count));
    }

    // Get selected filter
    let selected_filter = state.sub_project_filters
        .get(state.sub_selected_filter_index)
        .cloned()
        .flatten();

    // Filter subscriptions based on selection
    let filtered_subs: Vec<(&String, &tenex_core::stats::SubscriptionInfo)> = match &selected_filter {
        None => sub_stats.by_events_received(), // All
        Some(f) if f == "__global__" => global_subs.clone(),
        Some(f) => projects.iter()
            .find(|(a, _)| a == f)
            .map(|(_, subs)| subs.clone())
            .unwrap_or_default(),
    };

    // Build combined lines (sidebar + content side by side)
    let sidebar_header = format!("{:^width$}", "─ Projects ─", width = sidebar_width);
    let content_header = format!("{:^width$}", "─ Subscriptions ─", width = right_width);

    lines.push(Line::from(vec![
        Span::styled(sidebar_header, Style::default().fg(theme::BORDER_INACTIVE)),
        Span::styled(" │ ", Style::default().fg(theme::BORDER_INACTIVE)),
        Span::styled(content_header, Style::default().fg(theme::BORDER_INACTIVE)),
    ]));

    // Calculate how many lines we need
    let sidebar_lines = sidebar_items.len();
    let content_lines_per_sub = 3; // description, kinds, id
    let content_lines = filtered_subs.len() * content_lines_per_sub + filtered_subs.len(); // +1 for spacing
    let max_lines = sidebar_lines.max(content_lines);

    for i in 0..max_lines {
        let mut spans: Vec<Span> = Vec::new();

        // Sidebar column
        if i < sidebar_items.len() {
            let (name, a_tag, count, events) = &sidebar_items[i];
            let is_selected = match (&selected_filter, a_tag) {
                (None, None) => true, // "All" selected
                (Some(sel), Some(tag)) => sel == tag,
                _ => false,
            };
            let is_focused = state.sub_sidebar_focused &&
                state.sub_selected_filter_index < state.sub_project_filters.len() &&
                match (&state.sub_project_filters[state.sub_selected_filter_index], a_tag) {
                    (None, None) => true,
                    (Some(sel), Some(tag)) => sel == tag,
                    _ => false,
                };

            let prefix = if is_focused { "▸ " } else { "  " };
            let name_display = if name.len() > 16 {
                format!("{}...", &name[..13])
            } else {
                name.clone()
            };

            let sidebar_text = format!("{}{:<16} {:>3} {:>5}", prefix, name_display, count, events);
            let style = if is_focused {
                Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(theme::TEXT_PRIMARY)
            } else {
                Style::default().fg(theme::TEXT_MUTED)
            };
            spans.push(Span::styled(
                format!("{:<width$}", sidebar_text, width = sidebar_width),
                style,
            ));
        } else {
            spans.push(Span::styled(
                " ".repeat(sidebar_width),
                Style::default(),
            ));
        }

        // Separator
        spans.push(Span::styled(" │ ", Style::default().fg(theme::BORDER_INACTIVE)));

        // Content column
        let sub_index = i / (content_lines_per_sub + 1);
        let sub_line = i % (content_lines_per_sub + 1);

        if sub_index < filtered_subs.len() {
            let (sub_id, info) = &filtered_subs[sub_index];
            let content_text = match sub_line {
                0 => {
                    // Description line
                    let desc = if info.description.len() > right_width.saturating_sub(15) {
                        format!("{}...", &info.description[..right_width.saturating_sub(18).min(info.description.len())])
                    } else {
                        info.description.clone()
                    };
                    format!("{:<width$} {:>6} events", desc, info.events_received, width = right_width.saturating_sub(14))
                }
                1 => {
                    // Kinds line
                    let kinds_str: String = info.kinds.iter()
                        .map(|k| format!("{}", k))
                        .collect::<Vec<_>>()
                        .join(",");
                    format!("  kinds: {}", kinds_str)
                }
                2 => {
                    // ID line
                    let short_id = if sub_id.len() > 24 {
                        format!("{}...", &sub_id[..24])
                    } else {
                        (*sub_id).clone()
                    };
                    format!("  id: {}", short_id)
                }
                _ => String::new(), // Spacing line
            };

            let style = match sub_line {
                0 => Style::default().fg(theme::TEXT_PRIMARY),
                1 => Style::default().fg(theme::TEXT_MUTED),
                2 => Style::default().fg(theme::BORDER_INACTIVE),
                _ => Style::default(),
            };

            spans.push(Span::styled(content_text, style));
        }

        lines.push(Line::from(spans));
    }

    // Help text
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  ↑↓", Style::default().fg(theme::TEXT_MUTED)),
        Span::raw(" select • "),
        Span::styled("←→", Style::default().fg(theme::TEXT_MUTED)),
        Span::raw(" switch panes"),
    ]));

    lines
}

/// Render the Negentropy tab content
fn render_negentropy_tab(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    // Get negentropy stats
    let neg_stats = app.negentropy_stats.snapshot();

    // Header
    lines.push(Line::from(vec![Span::styled(
        "═══ Negentropy Synchronization ═══",
        Style::default().fg(theme::ACCENT_PRIMARY),
    )]));
    lines.push(Line::from(""));

    // Status section
    let status_indicator = if neg_stats.enabled {
        if neg_stats.sync_in_progress {
            Span::styled("● SYNCING", Style::default().fg(theme::ACCENT_WARNING))
        } else {
            Span::styled("● ENABLED", Style::default().fg(theme::ACCENT_SUCCESS))
        }
    } else {
        Span::styled("○ DISABLED", Style::default().fg(theme::TEXT_MUTED))
    };

    lines.push(Line::from(vec![
        Span::styled("  Status: ", Style::default().fg(theme::TEXT_MUTED)),
        status_indicator,
    ]));

    // Last full sync cycle time
    if let Some(instant) = neg_stats.last_cycle_time() {
        lines.push(Line::from(vec![
            Span::styled("  Last cycle: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(format_time_ago(instant), Style::default().fg(theme::TEXT_PRIMARY)),
        ]));
    } else {
        lines.push(Line::from(vec![
            Span::styled("  Last cycle: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("Never", Style::default().fg(theme::TEXT_DIM)),
        ]));
    }

    // Last filter sync time (any individual filter)
    if let Some(instant) = neg_stats.last_filter_time() {
        lines.push(Line::from(vec![
            Span::styled("  Last filter: ", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled(format_time_ago(instant), Style::default().fg(theme::TEXT_DIM)),
        ]));
    }

    // Current interval
    lines.push(Line::from(vec![
        Span::styled("  Sync interval: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{}s", neg_stats.current_interval_secs),
            Style::default().fg(theme::TEXT_PRIMARY),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Statistics section
    lines.push(Line::from(vec![Span::styled(
        "═══ Sync Statistics ═══",
        Style::default().fg(theme::ACCENT_SUCCESS),
    )]));
    lines.push(Line::from(""));

    // Success/failure counts
    lines.push(Line::from(vec![
        Span::styled("  Successful syncs: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{}", neg_stats.successful_syncs),
            Style::default().fg(theme::ACCENT_SUCCESS),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::styled("  Failed syncs: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{}", neg_stats.failed_syncs),
            Style::default().fg(if neg_stats.failed_syncs > 0 {
                theme::ACCENT_ERROR
            } else {
                theme::TEXT_PRIMARY
            }),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::styled("  Unsupported (relay): ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{}", neg_stats.unsupported_syncs),
            Style::default().fg(theme::TEXT_DIM),
        ),
    ]));

    lines.push(Line::from(vec![
        Span::styled("  Events reconciled: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{}", neg_stats.total_events_reconciled),
            Style::default().fg(theme::ACCENT_PRIMARY),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Recent results section
    lines.push(Line::from(vec![Span::styled(
        "═══ Recent Sync Results ═══",
        Style::default().fg(theme::ACCENT_WARNING),
    )]));
    lines.push(Line::from(""));

    if neg_stats.recent_results.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No sync results yet",
            Style::default().fg(theme::TEXT_MUTED),
        )));
        if !neg_stats.enabled {
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "  Note: Negentropy sync is currently disabled.",
                Style::default().fg(theme::TEXT_DIM),
            )));
            lines.push(Line::from(Span::styled(
                "  Most relays don't support it yet.",
                Style::default().fg(theme::TEXT_DIM),
            )));
        }
    } else {
        // Table header
        lines.push(Line::from(vec![
            Span::styled("  Kind", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw("      "),
            Span::styled("Status", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw("      "),
            Span::styled("Events", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw("    "),
            Span::styled("Time", Style::default().fg(theme::TEXT_MUTED)),
        ]));
        lines.push(Line::from("  ─────────────────────────────────────────"));

        // Show last 15 results (most recent first)
        let results_to_show: Vec<_> = neg_stats.recent_results.iter().rev().take(15).collect();

        for result in results_to_show {
            let status_span = match result.status {
                NegentropySyncStatus::Ok => {
                    Span::styled("  OK  ", Style::default().fg(theme::ACCENT_SUCCESS))
                }
                NegentropySyncStatus::Unsupported => {
                    Span::styled("UNSUP ", Style::default().fg(theme::TEXT_DIM))
                }
                NegentropySyncStatus::Failed => {
                    Span::styled("FAIL  ", Style::default().fg(theme::ACCENT_ERROR))
                }
            };

            let events_str = if result.status == NegentropySyncStatus::Ok && result.events_received > 0 {
                format!("{:>5}", result.events_received)
            } else {
                "    -".to_string()
            };

            let time_str = format_time_ago(result.completed_at);

            lines.push(Line::from(vec![
                Span::styled(format!("  {:6}", result.kind_label), Style::default().fg(theme::TEXT_PRIMARY)),
                Span::raw("    "),
                status_span,
                Span::raw("    "),
                Span::styled(events_str, Style::default().fg(theme::ACCENT_PRIMARY)),
                Span::raw("    "),
                Span::styled(time_str, Style::default().fg(theme::TEXT_DIM)),
            ]));
        }
    }

    lines
}

/// Render the E-Tag Query tab content
fn render_e_tag_query_tab(state: &DebugStatsState) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    // Header
    lines.push(Line::from(vec![Span::styled(
        "═══ Query Events by E-Tag ═══",
        Style::default().fg(theme::ACCENT_PRIMARY),
    )]));
    lines.push(Line::from(""));

    // Input field
    let input_text: String = if state.e_tag_query_input.is_empty() {
        "(enter hex event ID)".to_string()
    } else {
        state.e_tag_query_input.clone()
    };
    lines.push(Line::from(vec![
        Span::styled("  Event ID: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            input_text,
            Style::default().fg(if state.e_tag_input_focused {
                theme::ACCENT_PRIMARY
            } else {
                theme::TEXT_PRIMARY
            }),
        ),
        if state.e_tag_input_focused {
            Span::styled("▌", Style::default().fg(theme::ACCENT_PRIMARY))
        } else {
            Span::raw("")
        },
    ]));
    lines.push(Line::from(""));

    // Results section
    if state.e_tag_query_results.is_empty() {
        if !state.e_tag_query_input.is_empty() {
            lines.push(Line::from(Span::styled(
                "  No events found with this e-tag",
                Style::default().fg(theme::TEXT_MUTED),
            )));
        } else {
            lines.push(Line::from(Span::styled(
                "  Enter an event ID and press Enter to search",
                Style::default().fg(theme::TEXT_MUTED),
            )));
        }
    } else {
        lines.push(Line::from(vec![Span::styled(
            format!("  Found {} events:", state.e_tag_query_results.len()),
            Style::default().fg(theme::ACCENT_SUCCESS),
        )]));
        lines.push(Line::from("  ────────────────────────────────────────"));
        lines.push(Line::from(""));

        for (i, result) in state.e_tag_query_results.iter().enumerate() {
            let is_selected = i == state.e_tag_selected_index;
            let prefix = if is_selected { "▸ " } else { "  " };

            // Event ID line - safely handle short IDs
            let event_id_display = if result.event_id.len() >= 16 {
                format!("{}...", &result.event_id[..16])
            } else {
                result.event_id.clone()
            };
            lines.push(Line::from(vec![
                Span::styled(
                    prefix,
                    Style::default().fg(if is_selected {
                        theme::ACCENT_PRIMARY
                    } else {
                        theme::TEXT_PRIMARY
                    }),
                ),
                Span::styled("ID: ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(event_id_display, Style::default().fg(theme::TEXT_PRIMARY)),
            ]));

            // Kind and timestamp
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("Kind: ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(
                    format!("{}", result.kind),
                    Style::default().fg(theme::ACCENT_SUCCESS),
                ),
                Span::styled(" • ", Style::default().fg(theme::BORDER_INACTIVE)),
                Span::styled("Created: ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(
                    format!("{}", result.created_at),
                    Style::default().fg(theme::TEXT_PRIMARY),
                ),
            ]));

            // Pubkey - safely handle short pubkeys
            let pubkey_display = if result.pubkey.len() >= 16 {
                format!("{}...", &result.pubkey[..16])
            } else {
                result.pubkey.clone()
            };
            lines.push(Line::from(vec![
                Span::raw("    "),
                Span::styled("Pubkey: ", Style::default().fg(theme::TEXT_MUTED)),
                Span::styled(pubkey_display, Style::default().fg(theme::TEXT_PRIMARY)),
            ]));

            // Content preview
            if !result.content_preview.is_empty() {
                let preview = result.content_preview.replace('\n', " ");
                let preview = if preview.len() > 50 {
                    format!("{}...", &preview[..50])
                } else {
                    preview
                };
                lines.push(Line::from(vec![
                    Span::raw("    "),
                    Span::styled("Content: ", Style::default().fg(theme::TEXT_MUTED)),
                    Span::styled(preview, Style::default().fg(theme::TEXT_DIM)),
                ]));
            }

            lines.push(Line::from(""));
        }
    }

    lines
}

/// Render the Data Store tab content - shows thread counts and visibility state
fn render_data_store_tab(app: &App) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    let store = app.data_store.borrow();

    // Header: Visibility state
    lines.push(Line::from(vec![Span::styled(
        "═══ Visibility State ═══",
        Style::default().fg(theme::ACCENT_PRIMARY),
    )]));
    lines.push(Line::from(""));

    // Show visible_projects
    lines.push(Line::from(vec![
        Span::styled("  Visible projects: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{}", app.visible_projects.len()),
            Style::default().fg(theme::ACCENT_SUCCESS),
        ),
    ]));

    if app.visible_projects.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  ⚠ EMPTY - No conversations will be shown in Conversations tab!",
            Style::default().fg(theme::ACCENT_ERROR),
        )]));
    } else {
        for a_tag in &app.visible_projects {
            let project_name = format_project_name(a_tag);
            lines.push(Line::from(vec![
                Span::raw("    ✓ "),
                Span::styled(project_name, Style::default().fg(theme::TEXT_PRIMARY)),
            ]));
        }
    }

    lines.push(Line::from(""));

    // Time filter state
    lines.push(Line::from(vec![
        Span::styled("  Time filter: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            app.home.time_filter.as_ref().map(|tf| tf.label()).unwrap_or("None"),
            Style::default().fg(theme::TEXT_PRIMARY),
        ),
    ]));

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Thread counts by project
    lines.push(Line::from(vec![Span::styled(
        "═══ Threads in Memory (by Project) ═══",
        Style::default().fg(theme::ACCENT_SUCCESS),
    )]));
    lines.push(Line::from(""));

    let total_threads: usize = store.threads_by_project.values().map(|v| v.len()).sum();
    lines.push(Line::from(vec![
        Span::styled("  Total threads in memory: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{}", total_threads),
            Style::default().fg(theme::ACCENT_PRIMARY),
        ),
    ]));
    lines.push(Line::from(""));

    if store.threads_by_project.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  ⚠ No threads loaded in memory!",
            Style::default().fg(theme::ACCENT_ERROR),
        )]));
    } else {
        // Sort projects by thread count (descending)
        let mut project_counts: Vec<(&String, usize)> = store
            .threads_by_project
            .iter()
            .map(|(a_tag, threads)| (a_tag, threads.len()))
            .collect();
        project_counts.sort_by(|a, b| b.1.cmp(&a.1));

        lines.push(Line::from(vec![
            Span::styled("  Project", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw("                    "),
            Span::styled("Indexed", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw("  "),
            Span::styled("Loaded", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw("  "),
            Span::styled("Vis", Style::default().fg(theme::TEXT_MUTED)),
        ]));
        lines.push(Line::from("  ─────────────────────────────────────────────"));

        for (a_tag, loaded_count) in &project_counts {
            let project_name = format_project_name(a_tag);
            let is_visible = app.visible_projects.contains(*a_tag);
            let index_count = store.get_thread_root_count(a_tag);
            let display_name = if project_name.len() > 20 {
                format!("{}...", &project_name[..17])
            } else {
                project_name
            };

            let visibility_indicator = if is_visible {
                Span::styled("  ✓", Style::default().fg(theme::ACCENT_SUCCESS))
            } else {
                Span::styled("  ✗", Style::default().fg(theme::TEXT_DIM))
            };

            // Highlight if indexed != loaded (potential issue)
            let index_style = if index_count != *loaded_count {
                Style::default().fg(theme::ACCENT_WARNING)
            } else {
                Style::default().fg(theme::ACCENT_PRIMARY)
            };

            lines.push(Line::from(vec![
                Span::styled(format!("  {:<22}", display_name), Style::default().fg(theme::TEXT_PRIMARY)),
                Span::styled(format!("{:>7}", index_count), index_style),
                Span::styled(format!("{:>7}", loaded_count), Style::default().fg(theme::ACCENT_PRIMARY)),
                visibility_indicator,
            ]));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Known projects count
    lines.push(Line::from(vec![Span::styled(
        "═══ Known Projects ═══",
        Style::default().fg(theme::ACCENT_WARNING),
    )]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  Projects loaded: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{}", store.projects.len()),
            Style::default().fg(theme::ACCENT_PRIMARY),
        ),
    ]));

    lines.push(Line::from(""));

    // Show recent_threads result to understand filtering
    drop(store); // Release borrow before calling recent_threads
    let recent = app.recent_threads();
    lines.push(Line::from(vec![Span::styled(
        "═══ Recents Tab Output ═══",
        Style::default().fg(theme::ACCENT_PRIMARY),
    )]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  recent_threads() returned: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{} threads", recent.len()),
            Style::default().fg(if recent.is_empty() { theme::ACCENT_ERROR } else { theme::ACCENT_SUCCESS }),
        ),
    ]));

    if recent.is_empty() && !app.visible_projects.is_empty() {
        lines.push(Line::from(vec![Span::styled(
            "  ⚠ Visible projects selected but no threads returned!",
            Style::default().fg(theme::ACCENT_WARNING),
        )]));
        lines.push(Line::from(vec![Span::styled(
            "    Possible causes: time filter too restrictive, all threads archived,",
            Style::default().fg(theme::TEXT_DIM),
        )]));
        lines.push(Line::from(vec![Span::styled(
            "    or threads_by_project doesn't have entries for visible projects",
            Style::default().fg(theme::TEXT_DIM),
        )]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Direct database query section - find what's actually in the DB
    lines.push(Line::from(vec![Span::styled(
        "═══ Raw Database Query (Direct) ═══",
        Style::default().fg(theme::ACCENT_ERROR),
    )]));
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Querying kind:1 events with a-tags directly from nostrdb...",
        Style::default().fg(theme::TEXT_DIM),
    )]));
    lines.push(Line::from(""));

    // Get project a-tags for the query
    let store = app.data_store.borrow();
    let project_a_tags: Vec<String> = store.projects.iter().map(|p| p.a_tag()).collect();
    drop(store); // Release borrow before query

    // Query the database directly for each project
    let db_stats = query_project_thread_stats(&app.db.ndb, &project_a_tags);

    lines.push(Line::from(vec![
        Span::styled("  Project", Style::default().fg(theme::TEXT_MUTED)),
        Span::raw("               "),
        Span::styled("DB", Style::default().fg(theme::TEXT_MUTED)),
        Span::raw("     "),
        Span::styled("Threads", Style::default().fg(theme::TEXT_MUTED)),
        Span::raw("  "),
        Span::styled("Msgs", Style::default().fg(theme::TEXT_MUTED)),
        Span::raw("  "),
        Span::styled("InMem", Style::default().fg(theme::TEXT_MUTED)),
    ]));
    lines.push(Line::from("  ─────────────────────────────────────────────────────"));

    let store = app.data_store.borrow();
    for info in &db_stats {
        let display_name = if info.name.len() > 18 {
            format!("{}...", &info.name[..15])
        } else {
            info.name.clone()
        };

        // Get in-memory count for comparison
        let in_mem_count = store
            .threads_by_project
            .get(&info.a_tag)
            .map(|v| v.len())
            .unwrap_or(0);

        // Highlight discrepancy if DB has more threads than memory
        let in_mem_style = if info.threads_count > in_mem_count && in_mem_count < 100 {
            Style::default().fg(theme::ACCENT_ERROR) // Problem: should have more
        } else {
            Style::default().fg(theme::TEXT_DIM)
        };

        lines.push(Line::from(vec![
            Span::styled(format!("  {:<18}", display_name), Style::default().fg(theme::TEXT_PRIMARY)),
            Span::styled(format!("{:>6}", info.raw_db_kind1_count), Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled(format!("{:>9}", info.threads_count), Style::default().fg(theme::ACCENT_SUCCESS)),
            Span::styled(format!("{:>6}", info.messages_count), Style::default().fg(theme::TEXT_DIM)),
            Span::styled(format!("{:>7}", in_mem_count), in_mem_style),
        ]));
    }
    drop(store);

    // Add legend
    lines.push(Line::from(""));
    lines.push(Line::from(vec![Span::styled(
        "  Legend: DB=total kind:1, Threads=root events, Msgs=replies, InMem=loaded",
        Style::default().fg(theme::TEXT_DIM),
    )]));

    lines
}

/// Render the Event Feed tab content - live feed of most recent events
fn render_event_feed_tab(app: &App, state: &DebugStatsState) -> Vec<Line<'static>> {
    let mut lines: Vec<Line> = Vec::new();

    // Get recent events from the feed
    let recent_events = app.event_feed.recent();

    // Header
    lines.push(Line::from(vec![Span::styled(
        "═══ Live Event Feed ═══",
        Style::default().fg(theme::ACCENT_PRIMARY),
    )]));
    lines.push(Line::from(""));

    if recent_events.is_empty() {
        lines.push(Line::from(Span::styled(
            "  No events received yet",
            Style::default().fg(theme::TEXT_MUTED),
        )));
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  Events will appear here as they're received from the network.",
            Style::default().fg(theme::TEXT_MUTED),
        )));
        return lines;
    }

    // Summary
    lines.push(Line::from(vec![
        Span::styled("  Total events in feed: ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            format!("{}", recent_events.len()),
            Style::default().fg(theme::ACCENT_SUCCESS),
        ),
    ]));
    lines.push(Line::from(""));

    // Table header
    lines.push(Line::from(vec![
        Span::styled("  Kind  ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("Event ID", Style::default().fg(theme::TEXT_MUTED)),
        Span::raw("                                                         "),
        Span::styled("Age", Style::default().fg(theme::TEXT_MUTED)),
    ]));
    lines.push(Line::from("  ────────────────────────────────────────────────────────────────────────────────"));

    // Show events (newest first)
    for (i, event) in recent_events.iter().enumerate() {
        let is_selected = i == state.event_feed_selected_index;
        let prefix = if is_selected { "▸ " } else { "  " };
        let kind_name = kind_name(event.kind);

        // Format age
        let age = format_time_ago(event.received_at);

        // Format event ID (full, no truncation)
        let event_id_display = &event.event_id;

        let style = if is_selected {
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme::TEXT_PRIMARY)
        };

        lines.push(Line::from(vec![
            Span::styled(prefix, style),
            Span::styled(
                format!("{:<6}", event.kind),
                if is_selected {
                    Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(theme::ACCENT_SUCCESS)
                },
            ),
            Span::raw(" "),
            Span::styled(event_id_display.clone(), style),
            Span::raw(" "),
            Span::styled(
                age,
                Style::default().fg(theme::TEXT_DIM),
            ),
        ]));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  ↑↓", Style::default().fg(theme::TEXT_MUTED)),
        Span::raw(" select • "),
        Span::styled("Enter", Style::default().fg(theme::TEXT_MUTED)),
        Span::raw(" view raw event"),
    ]));

    lines
}

pub fn render_debug_stats(f: &mut Frame, area: Rect, app: &App, state: &DebugStatsState) {
    // Calculate modal content width (approximate based on max_width and area)
    let modal_width = 120u16.min(area.width.saturating_sub(4));
    let content_width = modal_width.saturating_sub(4); // Account for modal padding

    // Build content lines based on active tab
    let mut lines: Vec<Line> = Vec::new();

    // Add tab bar
    lines.extend(render_tab_bar(state.active_tab));

    // Add tab content
    match state.active_tab {
        DebugStatsTab::Events => {
            lines.extend(render_events_tab(app));
        }
        DebugStatsTab::Subscriptions => {
            lines.extend(render_subscriptions_tab(app, state, content_width));
        }
        DebugStatsTab::Negentropy => {
            lines.extend(render_negentropy_tab(app));
        }
        DebugStatsTab::ETagQuery => {
            lines.extend(render_e_tag_query_tab(state));
        }
        DebugStatsTab::DataStore => {
            lines.extend(render_data_store_tab(app));
        }
        DebugStatsTab::EventFeed => {
            lines.extend(render_event_feed_tab(app, state));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(""));

    // Footer
    let footer = match state.active_tab {
        DebugStatsTab::ETagQuery => vec![
            Span::styled("Tab", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("/", Style::default().fg(theme::BORDER_INACTIVE)),
            Span::styled("1-6", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" switch tabs • "),
            Span::styled("Enter", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" search • "),
            Span::styled("Esc", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" close"),
        ],
        DebugStatsTab::Subscriptions => vec![
            Span::styled("Tab", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("/", Style::default().fg(theme::BORDER_INACTIVE)),
            Span::styled("1-6", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" tabs • "),
            Span::styled("↑↓", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" select • "),
            Span::styled("Enter", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" filter • "),
            Span::styled("Esc", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" close"),
        ],
        DebugStatsTab::EventFeed => vec![
            Span::styled("Tab", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("/", Style::default().fg(theme::BORDER_INACTIVE)),
            Span::styled("1-6", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" tabs • "),
            Span::styled("↑↓", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" select • "),
            Span::styled("Enter", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" view • "),
            Span::styled("Esc", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" close"),
        ],
        _ => vec![
            Span::styled("Tab", Style::default().fg(theme::TEXT_MUTED)),
            Span::styled("/", Style::default().fg(theme::BORDER_INACTIVE)),
            Span::styled("1-6", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" switch tabs • "),
            Span::styled("Esc", Style::default().fg(theme::TEXT_MUTED)),
            Span::raw(" close"),
        ],
    };
    lines.push(Line::from(footer));

    // Calculate visible lines based on scroll
    let visible_lines: Vec<Line> = lines.into_iter().skip(state.scroll_offset).collect();

    Modal::new("Debug Stats")
        .size(ModalSize {
            max_width: 120,
            height_percent: 0.85,
        })
        .render(f, area, |f, content_area| {
            let paragraph =
                Paragraph::new(visible_lines).style(Style::default().fg(theme::TEXT_PRIMARY));
            f.render_widget(paragraph, content_area);
        });
}
