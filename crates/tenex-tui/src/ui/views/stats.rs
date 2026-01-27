//! Stats tab view - displays LLM usage statistics including:
//! - Per-day runtime bar chart
//! - Total running cost
//! - Cost per project table
//! - Top 10 longest conversations

use crate::ui::{format::truncate_with_ellipsis, theme, App};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// Render the Stats tab content
pub fn render_stats(f: &mut Frame, app: &App, area: Rect) {
    // Get stats data from the data store
    let data_store = app.data_store.borrow();

    // 1. Runtime by day (last 14 days)
    let runtime_by_day = data_store.get_runtime_by_day(14);

    // 2. Total cost
    let total_cost = data_store.get_total_cost();

    // 3. Cost by project
    let cost_by_project = data_store.get_cost_by_project();

    // 4. Top 10 conversations by runtime
    let top_conversations = data_store.get_top_conversations_by_runtime(10);

    // Get thread titles for the top conversations
    let top_conv_with_titles: Vec<(String, String, u64)> = top_conversations
        .iter()
        .map(|(id, runtime)| {
            let title = data_store
                .get_thread_by_id(id)
                .map(|t| t.title.clone())
                .unwrap_or_else(|| truncate_with_ellipsis(id, 12));
            (id.clone(), title, *runtime)
        })
        .collect();

    drop(data_store); // Release borrow

    // Calculate section heights dynamically
    // Total cost section: 3 lines (title + value + blank)
    // Runtime chart: 3 lines header + bar_height + 1 legend = ~12 lines
    // Cost by project: 2 header + n projects + 1 blank
    // Top conversations: 2 header + n conversations

    let total_cost_height = 3u16;
    let chart_height = 12u16;
    let project_count = cost_by_project.len().min(10) as u16;
    let project_section_height = 3 + project_count;
    let conv_count = top_conv_with_titles.len() as u16;
    let conv_section_height = 3 + conv_count;

    // If content fits, just render it; otherwise we'd need scrolling
    // For now, render what fits with vertical layout
    let sections = Layout::vertical([
        Constraint::Length(total_cost_height),
        Constraint::Length(chart_height),
        Constraint::Length(project_section_height),
        Constraint::Length(conv_section_height),
    ])
    .split(area);

    // 1. Total Cost Section
    render_total_cost_section(f, total_cost, sections[0]);

    // 2. Runtime Bar Chart Section
    render_runtime_chart_section(f, &runtime_by_day, sections[1]);

    // 3. Cost by Project Section
    render_cost_by_project_section(f, &cost_by_project, sections[2]);

    // 4. Top Conversations Section
    render_top_conversations_section(f, &top_conv_with_titles, sections[3]);
}

/// Render the total cost section with prominent display
fn render_total_cost_section(f: &mut Frame, total_cost: f64, area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Title line
    lines.push(Line::from(vec![
        Span::styled(
            "Total Running Cost",
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD),
        ),
    ]));

    // Cost value - prominently displayed
    let cost_str = format!("${:.2}", total_cost);
    lines.push(Line::from(vec![
        Span::styled(
            cost_str,
            Style::default().fg(theme::ACCENT_SUCCESS).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  all-time",
            Style::default().fg(theme::TEXT_MUTED),
        ),
    ]));

    // Blank line
    lines.push(Line::from(""));

    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

/// Render the runtime bar chart using ASCII characters
fn render_runtime_chart_section(f: &mut Frame, runtime_by_day: &[(u64, u64)], area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Title
    lines.push(Line::from(vec![
        Span::styled(
            "LLM Runtime (Last 14 Days)",
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    if runtime_by_day.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("No runtime data available", Style::default().fg(theme::TEXT_MUTED)),
        ]));
    } else {
        // Find max runtime for scaling
        let max_runtime = runtime_by_day.iter().map(|(_, r)| *r).max().unwrap_or(1);
        let bar_max_width = area.width.saturating_sub(20) as usize; // Leave room for labels

        // Build the chart - ASCII bar chart
        // We'll show each day as a column of characters
        let seconds_per_day: u64 = 86400;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let today_start = (now / seconds_per_day) * seconds_per_day;

        // Create a map for quick lookup
        let runtime_map: std::collections::HashMap<u64, u64> = runtime_by_day.iter().cloned().collect();

        // Generate last 14 days
        let mut daily_values: Vec<(String, u64)> = Vec::new();
        for i in (0..14).rev() {
            let day_start = today_start - i * seconds_per_day;
            let runtime = runtime_map.get(&day_start).copied().unwrap_or(0);
            // Format as day abbreviation (just use day number for simplicity)
            let day_label = if i == 0 {
                "Today".to_string()
            } else if i == 1 {
                "Y'day".to_string()
            } else {
                format!("-{}d", i)
            };
            daily_values.push((day_label, runtime));
        }

        // Render horizontal bar chart (easier in TUI)
        for (label, runtime) in &daily_values {
            if *runtime > 0 {
                let bar_width = ((*runtime as f64 / max_runtime as f64) * bar_max_width as f64) as usize;
                let bar_width = bar_width.max(1); // At least 1 char if non-zero
                let bar: String = "#".repeat(bar_width);
                let runtime_str = format_runtime(*runtime);

                lines.push(Line::from(vec![
                    Span::styled(format!("{:>5} ", label), Style::default().fg(theme::TEXT_MUTED)),
                    Span::styled(bar, Style::default().fg(theme::ACCENT_PRIMARY)),
                    Span::styled(format!(" {}", runtime_str), Style::default().fg(theme::TEXT_PRIMARY)),
                ]));
            } else {
                lines.push(Line::from(vec![
                    Span::styled(format!("{:>5} ", label), Style::default().fg(theme::TEXT_MUTED)),
                    Span::styled("-", Style::default().fg(theme::TEXT_MUTED)),
                ]));
            }
        }
    }

    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

/// Render the cost by project table
fn render_cost_by_project_section(f: &mut Frame, cost_by_project: &[(String, String, f64)], area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Title
    lines.push(Line::from(vec![
        Span::styled(
            "Cost by Project",
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD),
        ),
    ]));
    lines.push(Line::from(""));

    if cost_by_project.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("No cost data available", Style::default().fg(theme::TEXT_MUTED)),
        ]));
    } else {
        // Table header
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<30} {:>10}", "Project", "Cost"),
                Style::default().fg(theme::TEXT_MUTED),
            ),
        ]));

        // Project rows (limit to 10)
        for (_a_tag, name, cost) in cost_by_project.iter().take(10) {
            let truncated_name = truncate_with_ellipsis(name, 28);

            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<30}", truncated_name),
                    Style::default().fg(theme::TEXT_PRIMARY),
                ),
                Span::styled(
                    format!("{:>10}", format!("${:.2}", cost)),
                    Style::default().fg(theme::ACCENT_SUCCESS),
                ),
            ]));
        }
    }

    lines.push(Line::from(""));

    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

/// Render the top conversations by runtime
fn render_top_conversations_section(f: &mut Frame, conversations: &[(String, String, u64)], area: Rect) {
    let mut lines: Vec<Line> = Vec::new();

    // Title
    lines.push(Line::from(vec![
        Span::styled(
            "Top 10 Longest Conversations",
            Style::default().fg(theme::TEXT_PRIMARY).add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            "  (includes delegated sub-conversations)",
            Style::default().fg(theme::TEXT_MUTED),
        ),
    ]));
    lines.push(Line::from(""));

    if conversations.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("No conversation data available", Style::default().fg(theme::TEXT_MUTED)),
        ]));
    } else {
        // Table header
        lines.push(Line::from(vec![
            Span::styled(
                format!("{:<40} {:>12}", "Conversation", "Runtime"),
                Style::default().fg(theme::TEXT_MUTED),
            ),
        ]));

        // Conversation rows
        for (_id, title, runtime) in conversations.iter() {
            let truncated_title = truncate_with_ellipsis(title, 38);

            let runtime_str = format_runtime(*runtime);

            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:<40}", truncated_title),
                    Style::default().fg(theme::TEXT_PRIMARY),
                ),
                Span::styled(
                    format!("{:>12}", runtime_str),
                    Style::default().fg(theme::ACCENT_PRIMARY),
                ),
            ]));
        }
    }

    let para = Paragraph::new(lines);
    f.render_widget(para, area);
}

/// Format runtime in milliseconds to human-readable string
fn format_runtime(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds < 60 {
        format!("{}s", seconds)
    } else if seconds < 3600 {
        let mins = seconds / 60;
        let secs = seconds % 60;
        if secs > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}m", mins)
        }
    } else {
        let hours = seconds / 3600;
        let mins = (seconds % 3600) / 60;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    }
}
