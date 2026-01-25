use crate::ui::card;
use crate::ui::format::truncate_with_ellipsis;
use crate::ui::theme;
use crate::ui::todo::{TodoState, TodoStatus};
use ratatui::{
    layout::Rect,
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};

/// A delegation item extracted from q-tags in conversation messages
#[derive(Debug, Clone)]
pub struct SidebarDelegation {
    /// Thread ID of the delegation (q-tag value)
    pub thread_id: String,
    /// Title of the delegated conversation
    pub title: String,
    /// Target agent name (or short pubkey if unknown)
    pub target: String,
}

/// A report reference extracted from a-tags in conversation messages
#[derive(Debug, Clone)]
pub struct SidebarReport {
    /// The a-tag coordinate (30023:pubkey:slug)
    pub a_tag: String,
    /// Report title
    pub title: String,
    /// Report slug (for display)
    pub slug: String,
}

/// Parsed report a-tag coordinate
/// Format: "30023:pubkey:slug" where slug may contain colons
#[derive(Debug, Clone)]
pub struct ReportCoordinate {
    pub kind: u32,
    pub pubkey: String,
    pub slug: String,
}

impl ReportCoordinate {
    /// Parse a report a-tag into its components
    /// Returns None if the format is invalid
    pub fn parse(a_tag: &str) -> Option<Self> {
        let parts: Vec<&str> = a_tag.split(':').collect();
        if parts.len() >= 3 {
            let kind = parts[0].parse::<u32>().ok()?;
            let pubkey = parts[1].to_string();
            // Slug may contain colons, so join the rest
            let slug = parts[2..].join(":");
            Some(ReportCoordinate { kind, pubkey, slug })
        } else {
            None
        }
    }
}

/// State for the interactive sidebar with delegations and reports
#[derive(Debug, Clone, Default)]
pub struct SidebarState {
    /// Whether the sidebar currently has focus
    pub focused: bool,
    /// Currently selected item index (across all selectable items)
    pub selected_index: usize,
    /// Delegations extracted from conversation
    pub delegations: Vec<SidebarDelegation>,
    /// Reports referenced in conversation (deduped)
    pub reports: Vec<SidebarReport>,
}

/// The type of item selected in the sidebar
#[derive(Debug, Clone)]
pub enum SidebarSelection {
    /// A delegation was selected (thread_id)
    Delegation(String),
    /// A report was selected (a_tag)
    Report(String),
}

impl SidebarState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Update the sidebar with new delegations and reports
    pub fn update(&mut self, delegations: Vec<SidebarDelegation>, reports: Vec<SidebarReport>) {
        self.delegations = delegations;
        self.reports = reports;
        // Reset selection if out of bounds
        let total = self.total_items();
        if total == 0 {
            self.selected_index = 0;
        } else if self.selected_index >= total {
            self.selected_index = total.saturating_sub(1);
        }
    }

    /// Total number of selectable items
    pub fn total_items(&self) -> usize {
        self.delegations.len() + self.reports.len()
    }

    /// Check if sidebar has any selectable items
    pub fn has_items(&self) -> bool {
        !self.delegations.is_empty() || !self.reports.is_empty()
    }

    /// Move selection up
    pub fn move_up(&mut self) {
        if self.selected_index > 0 {
            self.selected_index -= 1;
        }
    }

    /// Move selection down
    pub fn move_down(&mut self) {
        let max = self.total_items();
        if max > 0 && self.selected_index + 1 < max {
            self.selected_index += 1;
        }
    }

    /// Get the currently selected item
    pub fn selected_item(&self) -> Option<SidebarSelection> {
        self.item_at(self.selected_index)
    }

    /// Get the item at a specific global index
    /// Delegations come first (indices 0..delegations.len()),
    /// then reports (indices delegations.len()..total_items())
    pub fn item_at(&self, global_index: usize) -> Option<SidebarSelection> {
        let del_count = self.delegations.len();
        if global_index < del_count {
            // Delegation
            self.delegations.get(global_index)
                .map(|d| SidebarSelection::Delegation(d.thread_id.clone()))
        } else {
            // Report
            let report_idx = global_index - del_count;
            self.reports.get(report_idx)
                .map(|r| SidebarSelection::Report(r.a_tag.clone()))
        }
    }

    /// Check if a delegation at the given local index is currently selected
    #[inline]
    pub fn is_delegation_selected(&self, local_index: usize) -> bool {
        self.focused && self.selected_index == local_index
    }

    /// Check if a report at the given local index is currently selected
    #[inline]
    pub fn is_report_selected(&self, local_index: usize) -> bool {
        self.focused && self.selected_index == self.delegations.len() + local_index
    }

    /// Toggle focus state
    pub fn toggle_focus(&mut self) {
        self.focused = !self.focused;
    }

    /// Set focus state
    pub fn set_focused(&mut self, focused: bool) {
        self.focused = focused;
    }
}

/// Metadata about the current conversation (from kind:513 events)
#[derive(Debug, Clone, Default)]
pub struct ConversationMetadata {
    pub title: Option<String>,
    pub status_label: Option<String>,
    pub status_current_activity: Option<String>,
    /// Brief summary/description of the conversation
    pub summary: Option<String>,
    /// Agent names currently working on this conversation (from kind:24133)
    pub working_agents: Vec<String>,
    /// Aggregate LLM runtime across all messages in milliseconds
    pub total_llm_runtime_ms: u64,
}

impl ConversationMetadata {
    pub fn has_content(&self) -> bool {
        self.title.is_some() || self.status_label.is_some() || self.status_current_activity.is_some() || self.summary.is_some() || self.total_llm_runtime_ms > 0
    }

    pub fn is_busy(&self) -> bool {
        !self.working_agents.is_empty()
    }

    /// Format the total LLM runtime as a human-readable string
    pub fn formatted_runtime(&self) -> Option<String> {
        if self.total_llm_runtime_ms == 0 {
            return None;
        }
        let seconds = self.total_llm_runtime_ms as f64 / 1000.0;
        if seconds >= 3600.0 {
            // Show as hours and minutes for longer runtimes
            let hours = (seconds / 3600.0).floor();
            let mins = ((seconds % 3600.0) / 60.0).floor();
            Some(format!("{:.0}h{:.0}m", hours, mins))
        } else if seconds >= 60.0 {
            // Show as minutes and seconds
            let mins = (seconds / 60.0).floor();
            let secs = seconds % 60.0;
            Some(format!("{:.0}m{:.0}s", mins, secs))
        } else {
            Some(format!("{:.1}s", seconds))
        }
    }
}

/// Render the conversation sidebar on the right side of the chat.
/// Shows summary (first), work indicator (if busy), todos (if any), delegations, reports, and metadata below.
pub fn render_chat_sidebar(
    f: &mut Frame,
    todo_state: &TodoState,
    metadata: &ConversationMetadata,
    sidebar_state: &SidebarState,
    spinner_char: char,
    area: Rect,
) {
    let mut lines: Vec<Line> = Vec::new();
    // Horizontal padding: 2 chars on each side
    let h_padding = 2;
    let content_width = (area.width as usize).saturating_sub(h_padding * 2);

    // Track whether we've rendered any section (for separator logic)
    let mut has_content = false;

    // === SUMMARY SECTION (FIRST) ===
    let has_summary = metadata.summary.is_some();
    if has_summary {
        render_summary_section(&mut lines, metadata, content_width, h_padding);
        has_content = true;
    }

    // === WORK INDICATOR SECTION ===
    if metadata.is_busy() {
        if has_content {
            lines.push(Line::from(""));
        }
        render_work_indicator_section(&mut lines, metadata, spinner_char, content_width, h_padding);
        has_content = true;
    }

    // === TODOS SECTION ===
    if todo_state.has_todos() {
        if has_content {
            lines.push(Line::from(""));
        }
        render_todos_section(&mut lines, todo_state, content_width, h_padding);
        has_content = true;
    }

    // === DELEGATIONS SECTION ===
    if !sidebar_state.delegations.is_empty() {
        if has_content {
            lines.push(Line::from(""));
        }
        render_delegations_section(&mut lines, sidebar_state, content_width, h_padding);
        has_content = true;
    }

    // === REPORTS SECTION ===
    if !sidebar_state.reports.is_empty() {
        if has_content {
            lines.push(Line::from(""));
        }
        render_reports_section(&mut lines, sidebar_state, content_width, h_padding);
        has_content = true;
    }

    // === METADATA SECTION ===
    if metadata.has_content() {
        if has_content {
            lines.push(Line::from(""));
        }
        render_metadata_section(&mut lines, metadata, content_width, h_padding);
        // has_content = true; // Not needed, this is the last section
    }

    // === EMPTY STATE ===
    if lines.is_empty() {
        let padding = " ".repeat(h_padding);
        lines.push(Line::from(Span::styled(
            format!("{}No active tasks", padding),
            theme::text_muted(),
        )));
    }


    let sidebar = Paragraph::new(lines)
        .style(Style::default().bg(theme::BG_SIDEBAR));

    f.render_widget(sidebar, area);

    // Draw focus indicator on left edge if focused
    if sidebar_state.focused && area.width > 0 {
        let focus_indicator = Paragraph::new("│".repeat(area.height as usize))
            .style(Style::default().fg(theme::ACCENT_PRIMARY));
        let indicator_area = Rect::new(area.x, area.y, 1, area.height);
        f.render_widget(focus_indicator, indicator_area);
    }
}

fn render_summary_section<'a>(
    lines: &mut Vec<Line<'a>>,
    metadata: &'a ConversationMetadata,
    content_width: usize,
    h_padding: usize,
) {
    let padding = " ".repeat(h_padding);

    if let Some(ref summary) = metadata.summary {
        for line in wrap_text(summary, content_width) {
            lines.push(Line::from(vec![
                Span::raw(padding.clone()),
                Span::styled(line, Style::default().fg(theme::TEXT_PRIMARY)),
            ]));
        }
    }
}

fn render_work_indicator_section(
    lines: &mut Vec<Line>,
    metadata: &ConversationMetadata,
    spinner_char: char,
    content_width: usize,
    h_padding: usize,
) {
    let padding = " ".repeat(h_padding);

    // Header with spinner
    lines.push(Line::from(vec![
        Span::raw(padding.clone()),
        Span::styled(
            format!("{} ", spinner_char),
            Style::default().fg(theme::ACCENT_PRIMARY),
        ),
        Span::styled("WORKING", Style::default().fg(theme::ACCENT_PRIMARY)),
    ]));

    // List working agents
    for agent_name in &metadata.working_agents {
        let display_name = truncate_with_ellipsis(agent_name, content_width.saturating_sub(2));
        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled(format!("  {}", display_name), theme::text_muted()),
        ]));
    }

    // Hint about stopping
    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::raw(padding.clone()),
        Span::styled("s", Style::default().fg(theme::ACCENT_WARNING)),
        Span::styled(" to stop", theme::text_muted()),
    ]));
}

fn render_todos_section(lines: &mut Vec<Line>, todo_state: &TodoState, content_width: usize, h_padding: usize) {
    let padding = " ".repeat(h_padding);

    let completed = todo_state.completed_count();
    let total = todo_state.items.len();

    // Progress bar
    let filled = if total > 0 {
        (completed * content_width) / total
    } else {
        0
    };
    let empty_bar = content_width.saturating_sub(filled);
    lines.push(Line::from(vec![
        Span::raw(padding.clone()),
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
        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled("In Progress", theme::todo_in_progress()),
        ]));
        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled(
                format!("  {}", truncate_with_ellipsis(&active.title, content_width.saturating_sub(2))),
                theme::text_primary(),
            ),
        ]));
        if let Some(ref desc) = active.description {
            lines.push(Line::from(vec![
                Span::raw(padding.clone()),
                Span::styled(
                    format!("  {}", truncate_with_ellipsis(desc, content_width.saturating_sub(2))),
                    theme::text_muted(),
                ),
            ]));
        }
        lines.push(Line::from(""));
    }

    // Todo items
    for item in &todo_state.items {
        let (icon, icon_style) = match item.status {
            TodoStatus::Done => (card::TODO_DONE_GLYPH, theme::todo_done()),
            TodoStatus::InProgress => (card::TODO_IN_PROGRESS_GLYPH, theme::todo_in_progress()),
            TodoStatus::Pending => (card::TODO_PENDING_GLYPH, theme::todo_pending()),
            TodoStatus::Skipped => (card::TODO_SKIPPED_GLYPH, theme::todo_skipped()),
        };

        let title_style = if matches!(item.status, TodoStatus::Done | TodoStatus::Skipped) {
            Style::default()
                .fg(theme::TEXT_MUTED)
                .add_modifier(Modifier::CROSSED_OUT)
        } else {
            theme::text_primary()
        };

        let title = truncate_with_ellipsis(&item.title, content_width.saturating_sub(2));
        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled(format!("{} ", icon), icon_style),
            Span::styled(title, title_style),
        ]));
    }
}

fn render_delegations_section(
    lines: &mut Vec<Line>,
    sidebar_state: &SidebarState,
    content_width: usize,
    h_padding: usize,
) {
    let padding = " ".repeat(h_padding);

    // Section header
    lines.push(Line::from(vec![
        Span::raw(padding.clone()),
        Span::styled("DELEGATIONS", theme::text_muted()),
        Span::styled(
            format!(" ({})", sidebar_state.delegations.len()),
            Style::default().fg(theme::TEXT_DIM),
        ),
    ]));

    // Render each delegation (only show the target/recipient)
    for (i, delegation) in sidebar_state.delegations.iter().enumerate() {
        let is_selected = sidebar_state.is_delegation_selected(i);

        // Selection indicator
        let indicator = if is_selected { "▸ " } else { "  " };
        let indicator_style = if is_selected {
            Style::default().fg(theme::ACCENT_PRIMARY)
        } else {
            Style::default()
        };

        // Target/recipient line only
        let target = truncate_with_ellipsis(&delegation.target, content_width.saturating_sub(4));
        let target_style = if is_selected {
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            theme::text_primary()
        };
        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled(indicator.to_string(), indicator_style),
            Span::styled(target, target_style),
        ]));
    }
}

fn render_reports_section(
    lines: &mut Vec<Line>,
    sidebar_state: &SidebarState,
    content_width: usize,
    h_padding: usize,
) {
    let padding = " ".repeat(h_padding);

    // Section header
    lines.push(Line::from(vec![
        Span::raw(padding.clone()),
        Span::styled("REPORTS", theme::text_muted()),
        Span::styled(
            format!(" ({})", sidebar_state.reports.len()),
            Style::default().fg(theme::TEXT_DIM),
        ),
    ]));

    // Render each report
    for (i, report) in sidebar_state.reports.iter().enumerate() {
        let is_selected = sidebar_state.is_report_selected(i);

        // Selection indicator
        let indicator = if is_selected { "▸ " } else { "  " };
        let indicator_style = if is_selected {
            Style::default().fg(theme::ACCENT_PRIMARY)
        } else {
            Style::default()
        };

        // Title line with document icon
        let title = truncate_with_ellipsis(&report.title, content_width.saturating_sub(6));
        let title_style = if is_selected {
            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::BOLD)
        } else {
            theme::text_primary()
        };
        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled(indicator.to_string(), indicator_style),
            Span::styled("📄 ", Style::default()),
            Span::styled(title, title_style),
        ]));
    }
}

fn render_metadata_section<'a>(
    lines: &mut Vec<Line<'a>>,
    metadata: &'a ConversationMetadata,
    content_width: usize,
    h_padding: usize,
) {
    let padding = " ".repeat(h_padding);

    // Total LLM runtime
    if let Some(ref runtime_str) = metadata.formatted_runtime() {
        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled("⏱ ", Style::default().fg(theme::ACCENT_PRIMARY)),
            Span::styled("Runtime: ", theme::text_muted()),
            Span::styled(runtime_str.clone(), Style::default().fg(theme::TEXT_PRIMARY)),
        ]));
    }

    // Status value with color coding (no label)
    if let Some(ref status) = metadata.status_label {
        let status_style = match status.to_lowercase().as_str() {
            "completed" | "done" => theme::status_success(),
            "in progress" | "working" => theme::status_warning(),
            "blocked" | "failed" | "error" => theme::status_error(),
            _ => theme::text_primary(),
        };
        lines.push(Line::from(vec![
            Span::raw(padding.clone()),
            Span::styled(status.clone(), status_style),
        ]));
    }

    // Current activity
    if let Some(ref activity) = metadata.status_current_activity {
        for line in wrap_text(activity, content_width) {
            lines.push(Line::from(vec![
                Span::raw(padding.clone()),
                Span::styled(line, theme::text_muted()),
            ]));
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
