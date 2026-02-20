//! Stats tab view - displays LLM usage statistics including:
//! - Per-day runtime bar chart with proper Unicode bars
//! - Total running cost with metric cards
//! - Cost per project table
//! - Top conversations by runtime
//! - Per-day message counts (user vs all) bar chart
//! - Activity grid (GitHub-style) showing LLM activity by hour
//!
//! The Stats view has four subtabs:
//! - Chart: Shows the 14-day LLM runtime chart (full height)
//! - Rankings: Shows Cost by Project and Top Conversations tables side-by-side
//! - Messages: Shows message counts per day (current user vs all project messages)
//! - Activity: Shows GitHub-style activity grid for LLM usage per hour
//!
//! Uses a modern dashboard layout with bordered sections and theme colors.

use crate::ui::{card, format::truncate_with_ellipsis, theme, App, StatsSubtab};
use chrono::{Datelike, TimeZone, Utc};
use ratatui::{
    layout::{Alignment, Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Cell, Paragraph, Row, Table},
    Frame,
};

// Unicode bar characters for charts
const BAR_FULL: char = '█';
const BAR_SEVEN_EIGHTHS: char = '▉';
const BAR_THREE_QUARTERS: char = '▊';
const BAR_FIVE_EIGHTHS: char = '▋';
const BAR_HALF: char = '▌';
const BAR_THREE_EIGHTHS: char = '▍';
const BAR_QUARTER: char = '▎';
const BAR_EIGHTH: char = '▏';

// Layout constants for consistent width calculations
const CHART_LABEL_WIDTH: u16 = 22; // Space needed for date label + runtime display
const MIN_CHART_WIDTH: u16 = CHART_LABEL_WIDTH; // Minimum width for chart to render

// Messages chart needs extra space for "user/all" counts (e.g., "123/456")
const MESSAGES_COUNTS_WIDTH: u16 = 8; // Extra width for dual count display
const MESSAGES_LABEL_WIDTH: u16 = CHART_LABEL_WIDTH + MESSAGES_COUNTS_WIDTH; // Total label area for messages chart
const MIN_MESSAGES_CHART_WIDTH: u16 = MESSAGES_LABEL_WIDTH; // Minimum width before falling back to empty state

const TABLE_INSET: u16 = 2; // Padding inside table blocks
const TABLE_COST_COL_WIDTH: u16 = 10; // Width for cost/runtime columns
const TABLE_RUNTIME_COL_WIDTH: u16 = 12; // Width for runtime column

// Import shared constants for cost and chart windows
// COST_WINDOW_DAYS is used for cost calculations (shared with FFI)
// CHART_WINDOW_DAYS is used for chart rendering (runtime, messages)
use tenex_core::constants::{CHART_WINDOW_DAYS, COST_WINDOW_DAYS};

// Local alias for chart window (usize for iteration)
const STATS_WINDOW_DAYS: usize = CHART_WINDOW_DAYS;

// Number of items to show in ranking tables (expanded view with all vertical space)
const RANKINGS_TABLE_ROWS: usize = 20;

// Month names for date formatting
const MONTHS: [&str; 12] = [
    "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
];

// Activity grid constants
const HOURS_PER_DAY: usize = 24;
const DAYS_IN_WEEK: usize = 7;

/// View mode for the activity grid
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityViewMode {
    Week, // 7 days × 24 hours = 168 hours
}

impl ActivityViewMode {
    pub fn num_hours(&self) -> usize {
        match self {
            ActivityViewMode::Week => DAYS_IN_WEEK * HOURS_PER_DAY,
        }
    }

    pub fn num_days(&self) -> usize {
        match self {
            ActivityViewMode::Week => DAYS_IN_WEEK,
        }
    }
}

/// Visualization type for the activity grid
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActivityVisualization {
    Tokens, // Total tokens used per hour
}

/// Render the Stats tab content with a dashboard layout featuring subtabs
pub fn render_stats(f: &mut Frame, app: &App, area: Rect) {
    // Get today's runtime first (requires mutable borrow)
    let today_runtime = app
        .data_store
        .borrow_mut()
        .statistics
        .get_today_unique_runtime();

    // Get stats data from the data store (immutable borrow)
    let data_store = app.data_store.borrow();

    // 1. Runtime by day (last STATS_WINDOW_DAYS days)
    let runtime_by_day = data_store.statistics.get_runtime_by_day(STATS_WINDOW_DAYS);

    // 2. Total cost (past COST_WINDOW_DAYS)
    // Use saturating_sub for safe arithmetic in case of clock skew
    let now_secs = Utc::now().timestamp() as u64;
    let cost_window_start = now_secs.saturating_sub(COST_WINDOW_DAYS * 24 * 60 * 60);
    let total_cost = data_store.get_total_cost_since(cost_window_start);

    // 3. Cost by project
    let cost_by_project = data_store.get_cost_by_project();

    // 4. Top conversations by runtime (expanded for Rankings view)
    let top_conversations = data_store.get_top_conversations_by_runtime(RANKINGS_TABLE_ROWS);

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

    // 5. Messages by day (user vs all)
    let (user_messages_by_day, all_messages_by_day) =
        data_store.get_messages_by_day(STATS_WINDOW_DAYS);

    // Note: Don't drop data_store yet - Activity tab needs it

    // Dashboard Layout with Subtabs:
    // ┌─────────────────────────────────────────────────────────────┐
    // │  [Total Cost]  [24h Runtime]  [Avg (14d)]                  │ <- Metric Cards Row
    // ├─────────────────────────────────────────────────────────────┤
    // │  [Chart] [Rankings]                                         │ <- Subtab Navigation
    // ├─────────────────────────────────────────────────────────────┤
    // │                                                             │
    // │  Content Area (changes based on active subtab)              │
    // │                                                             │
    // └─────────────────────────────────────────────────────────────┘

    // Calculate adaptive heights using helper function
    let metric_cards_height = 5u16;
    let subtab_nav_height = 3u16;
    let content_height = area
        .height
        .saturating_sub(metric_cards_height + subtab_nav_height);

    let vertical_chunks = Layout::vertical([
        Constraint::Length(metric_cards_height), // Metric cards
        Constraint::Length(subtab_nav_height),   // Subtab navigation
        Constraint::Min(content_height),         // Content area
    ])
    .split(area);

    // 1. Render Metric Cards Row (always visible)
    render_metric_cards(
        f,
        total_cost,
        today_runtime,
        &runtime_by_day,
        vertical_chunks[0],
    );

    // 2. Render Subtab Navigation
    render_subtab_navigation(f, app.stats_subtab, vertical_chunks[1]);

    // 3. Render Content based on active subtab
    match app.stats_subtab {
        StatsSubtab::Chart => {
            // Full-height chart view
            render_runtime_chart(f, &runtime_by_day, vertical_chunks[2]);
        }
        StatsSubtab::Rankings => {
            // Side-by-side tables view
            let table_chunks =
                Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                    .split(vertical_chunks[2]);

            render_cost_by_project_table(f, &cost_by_project, table_chunks[0]);
            render_top_conversations_table(f, &top_conv_with_titles, table_chunks[1]);
        }
        StatsSubtab::Messages => {
            // Messages per day bar chart (user vs all)
            render_messages_chart(
                f,
                &user_messages_by_day,
                &all_messages_by_day,
                vertical_chunks[2],
            );
        }
        StatsSubtab::Activity => {
            // GitHub-style activity grid (default to week view, tokens visualization)
            render_activity_grid(
                f,
                &data_store,
                ActivityViewMode::Week,
                ActivityVisualization::Tokens,
                vertical_chunks[2],
            );
        }
    }

    drop(data_store); // Release borrow
}

/// Render the subtab navigation bar with pill-shaped tabs
fn render_subtab_navigation(f: &mut Frame, active_subtab: StatsSubtab, area: Rect) {
    let block = Block::default()
        .borders(Borders::BOTTOM)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.width < 20 || inner.height < 1 {
        return;
    }

    // Build the subtab pills
    let chart_active = active_subtab == StatsSubtab::Chart;
    let rankings_active = active_subtab == StatsSubtab::Rankings;
    let messages_active = active_subtab == StatsSubtab::Messages;
    let activity_active = active_subtab == StatsSubtab::Activity;

    let mut spans = vec![Span::raw(" ")];

    // Chart tab
    if chart_active {
        spans.push(Span::styled(
            " Chart ",
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .bg(theme::BG_TAB_ACTIVE)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::styled(
            " Chart ",
            Style::default()
                .fg(theme::TEXT_MUTED)
                .bg(theme::BG_SECONDARY),
        ));
    }

    spans.push(Span::raw("  "));

    // Rankings tab
    if rankings_active {
        spans.push(Span::styled(
            " Rankings ",
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .bg(theme::BG_TAB_ACTIVE)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::styled(
            " Rankings ",
            Style::default()
                .fg(theme::TEXT_MUTED)
                .bg(theme::BG_SECONDARY),
        ));
    }

    spans.push(Span::raw("  "));

    // Messages tab
    if messages_active {
        spans.push(Span::styled(
            " Messages ",
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .bg(theme::BG_TAB_ACTIVE)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::styled(
            " Messages ",
            Style::default()
                .fg(theme::TEXT_MUTED)
                .bg(theme::BG_SECONDARY),
        ));
    }

    spans.push(Span::raw("  "));

    // Activity tab
    if activity_active {
        spans.push(Span::styled(
            " Activity ",
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .bg(theme::BG_TAB_ACTIVE)
                .add_modifier(Modifier::BOLD),
        ));
    } else {
        spans.push(Span::styled(
            " Activity ",
            Style::default()
                .fg(theme::TEXT_MUTED)
                .bg(theme::BG_SECONDARY),
        ));
    }

    // Add hint for navigation
    spans.push(Span::raw("   "));
    spans.push(Span::styled(
        "h/l or ←/→ to switch",
        Style::default().fg(theme::TEXT_DIM),
    ));

    let paragraph = Paragraph::new(Line::from(spans));
    f.render_widget(paragraph, inner);
}

/// Render the top metric cards row
fn render_metric_cards(
    f: &mut Frame,
    total_cost: f64,
    today_runtime: u64,
    runtime_by_day: &[(u64, u64)],
    area: Rect,
) {
    // Calculate average daily runtime counting only non-zero days
    let non_zero_days: Vec<u64> = runtime_by_day
        .iter()
        .map(|(_, r)| *r)
        .filter(|r| *r > 0)
        .collect();

    let (avg_daily_runtime, active_days_count) = if non_zero_days.is_empty() {
        (0, 0)
    } else {
        let total: u64 = non_zero_days.iter().sum();
        (total / non_zero_days.len() as u64, non_zero_days.len())
    };

    let card_chunks = Layout::horizontal([
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
        Constraint::Ratio(1, 3),
    ])
    .split(area);

    // Card 1: Total Cost (past 2 weeks)
    render_metric_card(
        f,
        "Total Cost",
        &format!("${:.2}", total_cost),
        "past 2 weeks",
        theme::ACCENT_SUCCESS,
        card_chunks[0],
    );

    // Card 2: 24h Runtime
    render_metric_card(
        f,
        "24h Runtime",
        &format_runtime(today_runtime),
        "today",
        theme::ACCENT_PRIMARY,
        card_chunks[1],
    );

    // Card 3: Average Daily Runtime (counting only non-zero days)
    render_metric_card(
        f,
        &format!("Avg ({}d)", active_days_count),
        &format_runtime(avg_daily_runtime),
        "per day",
        theme::ACCENT_SPECIAL,
        card_chunks[2],
    );
}

/// Render a single metric card with bordered block
fn render_metric_card(
    f: &mut Frame,
    title: &str,
    value: &str,
    subtitle: &str,
    value_color: ratatui::style::Color,
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE))
        .title(format!(" {} ", title))
        .title_style(Style::default().fg(theme::TEXT_MUTED));

    let inner = block.inner(area);
    f.render_widget(block, area);

    if inner.height < 2 || inner.width < 4 {
        return;
    }

    let lines = vec![
        Line::from(vec![Span::styled(
            value,
            Style::default()
                .fg(value_color)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from(vec![Span::styled(
            subtitle,
            Style::default().fg(theme::TEXT_DIM),
        )]),
    ];

    let paragraph = Paragraph::new(lines).alignment(Alignment::Center);
    f.render_widget(paragraph, inner);
}

/// Render the runtime bar chart using Unicode block characters
fn render_runtime_chart(f: &mut Frame, runtime_by_day: &[(u64, u64)], area: Rect) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE))
        .title(format!(" LLM Runtime (Last {} Days) ", STATS_WINDOW_DAYS))
        .title_style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Width guard aligned with bar_max_width calculation
    if inner.height < 3 || inner.width < MIN_CHART_WIDTH {
        render_empty_state(f, "Insufficient space for chart", inner);
        return;
    }

    if runtime_by_day.is_empty() {
        render_empty_state(f, "No runtime data available", inner);
        return;
    }

    // Find max runtime for scaling
    let max_runtime = runtime_by_day.iter().map(|(_, r)| *r).max().unwrap_or(1);
    let bar_max_width = inner.width.saturating_sub(CHART_LABEL_WIDTH) as usize;

    // Build the chart data for last STATS_WINDOW_DAYS days
    let seconds_per_day: u64 = 86400;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let today_start = (now / seconds_per_day) * seconds_per_day;

    // Create a map for quick lookup
    let runtime_map: std::collections::HashMap<u64, u64> = runtime_by_day.iter().cloned().collect();

    // Generate last STATS_WINDOW_DAYS days with proper date labels
    // Order: newest (today) first, oldest last - ensures today is always visible
    // even if the chart area is too small to fit all days
    let mut lines: Vec<Line> = Vec::new();

    for i in 0..STATS_WINDOW_DAYS {
        let day_start = today_start - i as u64 * seconds_per_day;
        let runtime = runtime_map.get(&day_start).copied().unwrap_or(0);

        // Format date label using chrono for correct date calculation
        let day_label = format_day_label_from_timestamp(day_start, today_start);

        if runtime > 0 {
            let bar = create_unicode_bar(runtime, max_runtime, bar_max_width);
            let runtime_str = format_runtime(runtime);

            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:>7} ", day_label),
                    Style::default().fg(theme::TEXT_MUTED),
                ),
                Span::styled(bar, Style::default().fg(theme::ACCENT_PRIMARY)),
                Span::styled(
                    format!(" {}", runtime_str),
                    Style::default().fg(theme::TEXT_PRIMARY),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:>7} ", day_label),
                    Style::default().fg(theme::TEXT_MUTED),
                ),
                Span::styled(
                    format!("{} ", card::LIST_BULLET_GLYPH),
                    Style::default().fg(theme::TEXT_DIM),
                ),
                Span::styled("—", Style::default().fg(theme::TEXT_DIM)),
            ]));
        }
    }

    // Add padding at top if we have space
    let chart_area = Rect::new(
        inner.x + 1,
        inner.y + 1,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(2),
    );

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, chart_area);
}

/// Render the messages bar chart showing user messages vs all messages per day
/// Uses side-by-side bars for each day: user messages in primary color, all messages in secondary
fn render_messages_chart(
    f: &mut Frame,
    user_messages_by_day: &[(u64, u64)],
    all_messages_by_day: &[(u64, u64)],
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE))
        .title(format!(" Messages (Last {} Days) ", STATS_WINDOW_DAYS))
        .title_style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Width guard aligned with bar_max_width calculation (uses MIN_MESSAGES_CHART_WIDTH for consistency)
    if inner.height < 3 || inner.width < MIN_MESSAGES_CHART_WIDTH {
        render_empty_state(f, "Insufficient space for chart", inner);
        return;
    }

    // Find max message count for scaling (across both user and all)
    let max_user = user_messages_by_day
        .iter()
        .map(|(_, c)| *c)
        .max()
        .unwrap_or(0);
    let max_all = all_messages_by_day
        .iter()
        .map(|(_, c)| *c)
        .max()
        .unwrap_or(0);
    let max_count = max_user.max(max_all);

    if max_count == 0 {
        render_empty_state(f, "No message data available", inner);
        return;
    }

    // Bar max width accounts for label + dual count display (uses same constant as width guard)
    let bar_max_width = inner.width.saturating_sub(MESSAGES_LABEL_WIDTH) as usize;

    // Build the chart data for last STATS_WINDOW_DAYS days
    let seconds_per_day: u64 = 86400;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let today_start = (now / seconds_per_day) * seconds_per_day;

    // Create maps for quick lookup
    let user_map: std::collections::HashMap<u64, u64> =
        user_messages_by_day.iter().cloned().collect();
    let all_map: std::collections::HashMap<u64, u64> =
        all_messages_by_day.iter().cloned().collect();

    // Generate last STATS_WINDOW_DAYS days with proper date labels
    // Order: newest (today) first, oldest last
    let mut lines: Vec<Line> = Vec::new();

    // Add legend at top
    lines.push(Line::from(vec![
        Span::styled("        ", Style::default()),
        Span::styled("█", Style::default().fg(theme::ACCENT_PRIMARY)),
        Span::styled(" You  ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("█", Style::default().fg(theme::ACCENT_SPECIAL)),
        Span::styled(" All", Style::default().fg(theme::TEXT_MUTED)),
    ]));
    lines.push(Line::from(vec![Span::raw("")])); // Spacer

    for i in 0..STATS_WINDOW_DAYS {
        let day_start = today_start - i as u64 * seconds_per_day;
        let user_count = user_map.get(&day_start).copied().unwrap_or(0);
        let all_count = all_map.get(&day_start).copied().unwrap_or(0);

        // Format date label using chrono for correct date calculation
        let day_label = format_day_label_from_timestamp(day_start, today_start);

        if all_count > 0 {
            // Build user bar with fractional blocks for precise representation
            let user_bar = create_unicode_bar(user_count, max_count, bar_max_width);
            let user_bar_actual_len = user_bar.chars().count();

            // Build all bar and extract the remainder portion (characters beyond user_bar length).
            // This ensures consistent bar building - we use the exact characters from all_bar
            // rather than generating new full blocks, avoiding overflow from fractional differences.
            let all_bar = create_unicode_bar(all_count, max_count, bar_max_width);

            // Extract remainder: skip the first user_bar_actual_len characters from all_bar
            // This gives us exactly the portion that extends beyond the user bar
            let all_bar_remainder: String = all_bar.chars().skip(user_bar_actual_len).collect();

            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:>7} ", day_label),
                    Style::default().fg(theme::TEXT_MUTED),
                ),
                Span::styled(user_bar, Style::default().fg(theme::ACCENT_PRIMARY)),
                Span::styled(
                    all_bar_remainder,
                    Style::default().fg(theme::ACCENT_SPECIAL),
                ),
                Span::styled(
                    format!(" {}/{}", user_count, all_count),
                    Style::default().fg(theme::TEXT_PRIMARY),
                ),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{:>7} ", day_label),
                    Style::default().fg(theme::TEXT_MUTED),
                ),
                Span::styled(
                    format!("{} ", card::LIST_BULLET_GLYPH),
                    Style::default().fg(theme::TEXT_DIM),
                ),
                Span::styled("—", Style::default().fg(theme::TEXT_DIM)),
            ]));
        }
    }

    // Add padding at top if we have space
    let chart_area = Rect::new(
        inner.x + 1,
        inner.y + 1,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(2),
    );

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, chart_area);
}

/// Create a Unicode horizontal bar with fractional precision
fn create_unicode_bar(value: u64, max_value: u64, max_width: usize) -> String {
    if max_value == 0 || max_width == 0 {
        return String::new();
    }

    let ratio = value as f64 / max_value as f64;
    let full_width = ratio * max_width as f64;
    let full_blocks = full_width.floor() as usize;
    let fraction = full_width - full_blocks as f64;

    let mut bar = String::new();

    // Add full blocks
    for _ in 0..full_blocks {
        bar.push(BAR_FULL);
    }

    // Add fractional block
    if fraction > 0.875 {
        bar.push(BAR_SEVEN_EIGHTHS);
    } else if fraction > 0.75 {
        bar.push(BAR_THREE_QUARTERS);
    } else if fraction > 0.625 {
        bar.push(BAR_FIVE_EIGHTHS);
    } else if fraction > 0.5 {
        bar.push(BAR_HALF);
    } else if fraction > 0.375 {
        bar.push(BAR_THREE_EIGHTHS);
    } else if fraction > 0.25 {
        bar.push(BAR_QUARTER);
    } else if fraction > 0.125 {
        bar.push(BAR_EIGHTH);
    } else if full_blocks == 0 && value > 0 {
        // Ensure at least a tiny indicator for non-zero values
        bar.push(BAR_EIGHTH);
    }

    bar
}

/// Format a unix timestamp as a day label like "Jan 27" or "Today"/"Yesterday"
/// Uses chrono for correct date handling including leap years
///
/// # Arguments
/// * `day_start` - Unix timestamp (seconds) representing the start of the day
/// * `today_start` - Unix timestamp (seconds) representing the start of today
fn format_day_label_from_timestamp(day_start: u64, today_start: u64) -> String {
    // Calculate days difference using timestamps to determine Today/Yesterday
    let days_diff = (today_start.saturating_sub(day_start)) / 86400;

    match days_diff {
        0 => "Today".to_string(),
        1 => "Yest.".to_string(),
        _ => {
            // Use chrono to get correct month and day
            timestamp_to_month_day(day_start)
        }
    }
}

/// Convert a Unix timestamp to "Mon DD" format using chrono in UTC
/// Handles leap years and all edge cases correctly.
/// Uses UTC to match the UTC day bucketing in the data layer.
fn timestamp_to_month_day(timestamp: u64) -> String {
    // Convert to chrono DateTime in UTC for consistent handling with data bucketing
    let datetime = match Utc.timestamp_opt(timestamp as i64, 0) {
        chrono::LocalResult::Single(dt) => dt,
        _ => return "???".to_string(), // Invalid timestamp
    };

    let month_idx = (datetime.month() - 1) as usize; // month() is 1-indexed
    let day = datetime.day();

    format!("{} {:2}", MONTHS[month_idx], day)
}

/// Render the cost by project section as a proper table
fn render_cost_by_project_table(
    f: &mut Frame,
    cost_by_project: &[(String, String, f64)],
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE))
        .title(" Cost by Project ")
        .title_style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(area);
    f.render_widget(block, area);

    if cost_by_project.is_empty() {
        render_empty_state(f, "No cost data available", inner);
        return;
    }

    // Calculate available width for project name column
    // Total width - insets - cost column - column spacing
    let name_col_max_width = (inner.width as usize)
        .saturating_sub(TABLE_INSET as usize)
        .saturating_sub(TABLE_COST_COL_WIDTH as usize)
        .saturating_sub(1); // column spacing

    // Create table rows (use all available items up to RANKINGS_TABLE_ROWS)
    let rows: Vec<Row> = cost_by_project
        .iter()
        .take(RANKINGS_TABLE_ROWS)
        .enumerate()
        .map(|(idx, (_a_tag, name, cost))| {
            let truncated_name = truncate_with_ellipsis(name, name_col_max_width);
            let bg = if idx % 2 == 0 {
                theme::BG_APP
            } else {
                theme::BG_SECONDARY
            };

            Row::new(vec![
                Cell::from(truncated_name).style(Style::default().fg(theme::TEXT_PRIMARY)),
                Cell::from(format!("${:.2}", cost))
                    .style(Style::default().fg(theme::ACCENT_SUCCESS)),
            ])
            .style(Style::default().bg(bg))
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("Project").style(
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Cost").style(
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .style(Style::default().bg(theme::BG_SECONDARY))
    .height(1);

    let widths = [
        Constraint::Min(10),
        Constraint::Length(TABLE_COST_COL_WIDTH),
    ];

    let table = Table::new(rows, widths).header(header).column_spacing(1);

    // Use consistent inset calculation
    let table_area = Rect::new(
        inner.x + (TABLE_INSET / 2),
        inner.y,
        inner.width.saturating_sub(TABLE_INSET),
        inner.height,
    );

    f.render_widget(table, table_area);
}

/// Render the top conversations section as a proper table
fn render_top_conversations_table(
    f: &mut Frame,
    conversations: &[(String, String, u64)],
    area: Rect,
) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE))
        .title(" Top Conversations ")
        .title_style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(area);
    f.render_widget(block, area);

    if conversations.is_empty() {
        render_empty_state(f, "No conversation data available", inner);
        return;
    }

    // Calculate available width for title column
    // Total width - insets - runtime column - column spacing
    let title_col_max_width = (inner.width as usize)
        .saturating_sub(TABLE_INSET as usize)
        .saturating_sub(TABLE_RUNTIME_COL_WIDTH as usize)
        .saturating_sub(1); // column spacing

    // Create table rows
    let rows: Vec<Row> = conversations
        .iter()
        .enumerate()
        .map(|(idx, (_id, title, runtime))| {
            let truncated_title = truncate_with_ellipsis(title, title_col_max_width);
            let runtime_str = format_runtime(*runtime);
            let bg = if idx % 2 == 0 {
                theme::BG_APP
            } else {
                theme::BG_SECONDARY
            };

            Row::new(vec![
                Cell::from(truncated_title).style(Style::default().fg(theme::TEXT_PRIMARY)),
                Cell::from(runtime_str).style(Style::default().fg(theme::ACCENT_PRIMARY)),
            ])
            .style(Style::default().bg(bg))
        })
        .collect();

    let header = Row::new(vec![
        Cell::from("Conversation").style(
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        ),
        Cell::from("Runtime").style(
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::BOLD),
        ),
    ])
    .style(Style::default().bg(theme::BG_SECONDARY))
    .height(1);

    let widths = [
        Constraint::Min(10),
        Constraint::Length(TABLE_RUNTIME_COL_WIDTH),
    ];

    let table = Table::new(rows, widths).header(header).column_spacing(1);

    // Use consistent inset calculation
    let table_area = Rect::new(
        inner.x + (TABLE_INSET / 2),
        inner.y,
        inner.width.saturating_sub(TABLE_INSET),
        inner.height,
    );

    f.render_widget(table, table_area);
}

/// Render an empty state message centered in the area
fn render_empty_state(f: &mut Frame, message: &str, area: Rect) {
    if area.height < 1 || area.width < message.len() as u16 {
        return;
    }

    let y_offset = area.height / 2;
    let centered_area = Rect::new(area.x, area.y + y_offset, area.width, 1);

    let paragraph = Paragraph::new(Line::from(vec![Span::styled(
        message,
        Style::default().fg(theme::TEXT_DIM),
    )]))
    .alignment(Alignment::Center);

    f.render_widget(paragraph, centered_area);
}

/// Format runtime in milliseconds to human-readable string
fn format_runtime(ms: u64) -> String {
    let seconds = ms / 1000;
    if seconds == 0 && ms > 0 {
        format!("{}ms", ms)
    } else if seconds == 0 {
        "0s".to_string()
    } else if seconds < 60 {
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

/// Render the activity grid (GitHub-style contribution graph)
fn render_activity_grid(
    f: &mut Frame,
    data_store: &tenex_core::store::AppDataStore,
    view_mode: ActivityViewMode,
    visualization: ActivityVisualization,
    area: Rect,
) {
    let (title, data) = match visualization {
        ActivityVisualization::Tokens => {
            let tokens_data = data_store
                .statistics
                .get_tokens_by_hour(view_mode.num_hours());
            ("LLM Token Usage", tokens_data)
        }
    };

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER_INACTIVE))
        .title(format!(" {} (Last {} days) ", title, view_mode.num_days()))
        .title_style(
            Style::default()
                .fg(theme::TEXT_PRIMARY)
                .add_modifier(Modifier::BOLD),
        );

    let inner = block.inner(area);
    f.render_widget(block, area);

    // Check minimum space requirements
    if inner.height < 10 || inner.width < 40 {
        render_empty_state(f, "Insufficient space for activity grid", inner);
        return;
    }

    // Build the activity grid
    // Layout: rows represent calendar days, columns represent hours of day (0-23)
    // Each cell shows activity for that hour on that specific calendar day
    // This matches GitHub's contribution graph layout (time = columns, sequence = rows)
    let num_days = view_mode.num_days();
    let seconds_per_day: u64 = 86400;
    let seconds_per_hour: u64 = 3600;
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    // Calculate today's day start (UTC)
    let today_start = (now / seconds_per_day) * seconds_per_day;

    // Calculate maximum value for color scaling
    let max_value = data.values().max().copied().unwrap_or(1).max(1);

    // Build grid data structure: [day_offset][hour_of_day] = value
    // day_offset 0 = today, 1 = yesterday, etc.
    let mut grid: Vec<Vec<u64>> = vec![vec![0; HOURS_PER_DAY]; num_days];

    // Populate grid with data using calendar day boundaries
    for (hour_start, value) in &data {
        // Calculate which calendar day this hour belongs to
        let day_start = (hour_start / seconds_per_day) * seconds_per_day;

        // Calculate how many days ago this was (0 = today, 1 = yesterday, etc.)
        let days_ago = ((today_start.saturating_sub(day_start)) / seconds_per_day) as usize;

        // Calculate hour-of-day (0-23) for this hour_start
        let seconds_since_day_start = hour_start - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as usize;

        // Store in grid if within range
        if days_ago < num_days && hour_of_day < HOURS_PER_DAY {
            grid[days_ago][hour_of_day] = *value;
        }
    }

    // Render the grid
    // Display format: rows are days (most recent on bottom), columns are hours (0-23 left to right)
    let mut lines: Vec<Line> = Vec::new();

    // Add legend at top
    lines.push(Line::from(vec![
        Span::styled("    ", Style::default()),
        Span::styled(
            "█ ",
            Style::default().fg(get_activity_color(max_value, max_value)),
        ),
        Span::styled("High  ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            "█ ",
            Style::default().fg(get_activity_color(max_value / 2, max_value)),
        ),
        Span::styled("Med  ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled(
            "█ ",
            Style::default().fg(get_activity_color(max_value / 4, max_value)),
        ),
        Span::styled("Low  ", Style::default().fg(theme::TEXT_MUTED)),
        Span::styled("█ ", Style::default().fg(get_activity_color(0, max_value))),
        Span::styled("None", Style::default().fg(theme::TEXT_MUTED)),
    ]));
    lines.push(Line::from(vec![Span::raw("")])); // Spacer

    // Add hour labels at top
    let mut hour_label_spans = vec![Span::styled("     ", Style::default())];
    for hour in 0..HOURS_PER_DAY {
        // Show label for every 3rd hour to avoid clutter
        if hour % 3 == 0 {
            hour_label_spans.push(Span::styled(
                format!("{:02}h ", hour),
                Style::default().fg(theme::TEXT_DIM),
            ));
        } else {
            hour_label_spans.push(Span::styled("    ", Style::default()));
        }
    }
    lines.push(Line::from(hour_label_spans));
    lines.push(Line::from(vec![Span::raw("")])); // Spacer

    // Render grid: each row is a day, each column is an hour
    // Days in reverse order (oldest first = top, newest = bottom)
    for day_offset in (0..num_days).rev() {
        // Day label
        let day_label = if day_offset == 0 {
            "Today".to_string()
        } else if day_offset == 1 {
            "Yest.".to_string()
        } else {
            // Calculate the actual date for this day
            let day_start = today_start - (day_offset as u64 * seconds_per_day);
            format_day_label_from_timestamp(day_start, today_start)
        };

        let mut line_spans = vec![Span::styled(
            format!("{:>5} ", day_label),
            Style::default().fg(theme::TEXT_MUTED),
        )];

        // Hours go left to right (00-23)
        for hour in 0..HOURS_PER_DAY {
            let value = grid[day_offset][hour];
            let color = get_activity_color(value, max_value);
            line_spans.push(Span::styled("█", Style::default().fg(color)));
            line_spans.push(Span::raw(" ")); // Space between blocks
        }

        lines.push(Line::from(line_spans));
    }

    let paragraph = Paragraph::new(lines);
    let chart_area = Rect::new(
        inner.x + 1,
        inner.y + 1,
        inner.width.saturating_sub(2),
        inner.height.saturating_sub(2),
    );
    f.render_widget(paragraph, chart_area);
}

/// Get color for activity cell based on value (GitHub-style gradient)
/// Uses different shades of green to represent activity levels
fn get_activity_color(value: u64, max_value: u64) -> ratatui::style::Color {
    if value == 0 {
        // No activity - dim gray
        theme::TEXT_DIM
    } else if max_value == 0 {
        // Edge case: avoid division by zero
        theme::ACCENT_SUCCESS
    } else {
        let ratio = value as f64 / max_value as f64;
        if ratio >= 0.75 {
            // High activity - bright green
            ratatui::style::Color::Rgb(34, 197, 94) // green-500
        } else if ratio >= 0.5 {
            // Medium-high activity - medium green
            ratatui::style::Color::Rgb(74, 222, 128) // green-400
        } else if ratio >= 0.25 {
            // Medium-low activity - light green
            ratatui::style::Color::Rgb(134, 239, 172) // green-300
        } else {
            // Low activity - very light green
            ratatui::style::Color::Rgb(187, 247, 208) // green-200
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_runtime() {
        assert_eq!(format_runtime(0), "0s");
        assert_eq!(format_runtime(500), "500ms");
        assert_eq!(format_runtime(1000), "1s");
        assert_eq!(format_runtime(30000), "30s");
        assert_eq!(format_runtime(60000), "1m");
        assert_eq!(format_runtime(90000), "1m 30s");
        assert_eq!(format_runtime(3600000), "1h");
        assert_eq!(format_runtime(5400000), "1h 30m");
    }

    #[test]
    fn test_create_unicode_bar() {
        // Full bar
        let bar = create_unicode_bar(100, 100, 10);
        assert_eq!(bar.chars().count(), 10);

        // Half bar
        let bar = create_unicode_bar(50, 100, 10);
        assert!(bar.chars().count() >= 5);

        // Minimal bar for small value
        let bar = create_unicode_bar(1, 1000, 10);
        assert!(!bar.is_empty()); // Should have at least an eighth block

        // Zero bar
        let bar = create_unicode_bar(0, 100, 10);
        assert!(bar.is_empty());
    }

    #[test]
    fn test_format_day_label_from_timestamp() {
        let seconds_per_day: u64 = 86400;

        // Test Today
        let today_start: u64 = 1706313600; // Jan 27, 2024 00:00:00 UTC
        assert_eq!(
            format_day_label_from_timestamp(today_start, today_start),
            "Today"
        );

        // Test Yesterday
        let yesterday_start = today_start - seconds_per_day;
        assert_eq!(
            format_day_label_from_timestamp(yesterday_start, today_start),
            "Yest."
        );

        // Test 2 days ago - should show actual date in "Mon DD" format
        let two_days_ago = today_start - (2 * seconds_per_day);
        let label = format_day_label_from_timestamp(two_days_ago, today_start);
        // Now that we use UTC consistently, we can verify the exact date
        assert_eq!(label, "Jan 25", "Two days before Jan 27 should be Jan 25");
    }

    #[test]
    fn test_timestamp_to_month_day() {
        // Now that we use UTC consistently, we can verify exact dates

        // Test Jan 27, 2024 00:00:00 UTC
        let result = timestamp_to_month_day(1706313600);
        assert_eq!(result, "Jan 27", "1706313600 should be Jan 27 UTC");

        // Test Feb 29, 2024 00:00:00 UTC (leap day)
        let result2 = timestamp_to_month_day(1709164800);
        assert_eq!(
            result2, "Feb 29",
            "1709164800 should be Feb 29 UTC (leap day)"
        );

        // Verify these are different dates
        assert_ne!(
            result, result2,
            "Different timestamps should produce different dates"
        );
    }

    #[test]
    fn test_timestamp_to_month_day_format_consistency() {
        // Test that various timestamps all produce consistent format
        let timestamps = [
            1704067200, // Dec 31, 2023 UTC
            1706313600, // Jan 27, 2024 UTC
            1709078400, // Feb 28, 2024 UTC
            1709164800, // Feb 29, 2024 UTC (leap day)
            1709251200, // Mar 1, 2024 UTC
            1677542400, // Feb 28, 2023 UTC
            1677628800, // Mar 1, 2023 UTC
        ];

        for ts in timestamps {
            let result = timestamp_to_month_day(ts);

            // Verify format: "Mon DD" where DD may have leading space
            assert!(
                result.len() >= 5,
                "Timestamp {} produced too short result: '{}'",
                ts,
                result
            );

            let month_part = &result[..3];
            assert!(
                MONTHS.contains(&month_part),
                "Timestamp {} produced invalid month '{}' in result '{}'",
                ts,
                month_part,
                result
            );

            // Verify day part is numeric
            let day_part = result[4..].trim();
            let day_num: u32 = day_part.parse().unwrap_or_else(|_| {
                panic!(
                    "Timestamp {} produced non-numeric day '{}' in result '{}'",
                    ts, day_part, result
                )
            });
            assert!(
                (1..=31).contains(&day_num),
                "Day {} out of range for timestamp {}",
                day_num,
                ts
            );
        }
    }

    #[test]
    fn test_timestamp_to_month_day_leap_year_handling() {
        // Test that Feb 29 timestamps in leap years produce valid dates
        // (either Feb 29 in the local timezone or Feb 28/Mar 1 depending on offset)
        let leap_day_timestamp = 1709164800; // Feb 29, 2024 00:00:00 UTC

        let result = timestamp_to_month_day(leap_day_timestamp);
        let month_part = &result[..3];

        // Should be Feb or Mar (depending on local timezone offset)
        assert!(
            month_part == "Feb" || month_part == "Mar",
            "Leap day timestamp should produce Feb or Mar, got: '{}'",
            result
        );
    }

    #[test]
    fn test_width_guard_matches_bar_calculation() {
        // Verify that MIN_CHART_WIDTH and CHART_LABEL_WIDTH are consistent
        // If width is MIN_CHART_WIDTH, bar_max_width should be >= 0
        let bar_max_width = MIN_CHART_WIDTH.saturating_sub(CHART_LABEL_WIDTH);
        // With MIN_CHART_WIDTH == CHART_LABEL_WIDTH, bar_max_width will be 0
        // This is the minimum case - any smaller width would be rejected
        assert_eq!(bar_max_width, 0);

        // One pixel wider should give 1 character of bar space
        let bar_max_width_plus_one = (MIN_CHART_WIDTH + 1).saturating_sub(CHART_LABEL_WIDTH);
        assert_eq!(bar_max_width_plus_one, 1);
    }

    #[test]
    fn test_bar_composition_no_overflow() {
        // Verify that combining user_bar with all_bar remainder never exceeds max_width
        // This tests the fix for the bar composition overflow issue
        let max_width = 20;

        // Test various user/all combinations that could cause overflow
        let test_cases = [
            (10, 100),  // Small user, large all
            (50, 100),  // Half user, full all
            (99, 100),  // Almost equal
            (1, 100),   // Tiny user, full all
            (0, 100),   // Zero user (edge case)
            (100, 100), // Equal values
            (33, 77),   // Random values
            (17, 89),   // Values that produce fractional blocks
        ];

        for (user_count, all_count) in test_cases {
            let user_bar = create_unicode_bar(user_count, all_count, max_width);
            let all_bar = create_unicode_bar(all_count, all_count, max_width);

            let user_bar_len = user_bar.chars().count();
            let all_bar_len = all_bar.chars().count();

            // Extract remainder the same way render_messages_chart does
            let all_bar_remainder: String = all_bar.chars().skip(user_bar_len).collect();
            let remainder_len = all_bar_remainder.chars().count();

            let combined_len = user_bar_len + remainder_len;

            assert!(
                combined_len <= max_width,
                "Bar overflow for user={}, all={}: user_bar_len={}, remainder_len={}, combined={} > max_width={}",
                user_count, all_count, user_bar_len, remainder_len, combined_len, max_width
            );

            // Also verify combined equals all_bar length (no gaps)
            assert_eq!(
                combined_len, all_bar_len,
                "Bar gap for user={}, all={}: combined={} != all_bar_len={}",
                user_count, all_count, combined_len, all_bar_len
            );
        }
    }

    #[test]
    fn test_messages_chart_width_guard() {
        // Verify MIN_MESSAGES_CHART_WIDTH is properly defined
        // and bar_max_width calculation is consistent
        let bar_max_width = MIN_MESSAGES_CHART_WIDTH.saturating_sub(MESSAGES_LABEL_WIDTH);
        assert_eq!(
            bar_max_width, 0,
            "At minimum width, bar area should be zero"
        );

        // Verify MESSAGES_LABEL_WIDTH includes both label and counts
        assert_eq!(
            MESSAGES_LABEL_WIDTH,
            CHART_LABEL_WIDTH + MESSAGES_COUNTS_WIDTH,
            "Messages label width should be chart label + counts width"
        );
    }
}
