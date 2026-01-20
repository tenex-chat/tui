use crate::ui::card;
use crate::ui::{theme, App, View};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::Style,
    text::{Line, Span},
    widgets::Paragraph,
    Frame,
};
use unicode_width::UnicodeWidthStr;

/// Truncate string to fit within a display width, adding ellipsis when truncated.
/// Returns (truncated_string, actual_display_width).
fn truncate_to_width(s: &str, max_width: usize) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }

    let current_width = s.width();
    if current_width <= max_width {
        return (s.to_string(), current_width);
    }

    if max_width <= 3 {
        let dots = ".".repeat(max_width);
        return (dots.clone(), dots.width());
    }

    // Build truncated string char by char, tracking display width
    let mut result = String::new();
    let mut width = 0;
    let target_width = max_width - 3; // Reserve space for "..."

    for c in s.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width + char_width > target_width {
            break;
        }
        result.push(c);
        width += char_width;
    }

    result.push_str("...");
    (result.clone(), result.width())
}

/// Truncate string to fit within a display width without ellipsis.
/// Returns (truncated_string, actual_display_width).
fn truncate_plain_to_width(s: &str, max_width: usize) -> (String, usize) {
    if max_width == 0 {
        return (String::new(), 0);
    }

    let current_width = s.width();
    if current_width <= max_width {
        return (s.to_string(), current_width);
    }

    let mut result = String::new();
    let mut width = 0;

    for c in s.chars() {
        let char_width = unicode_width::UnicodeWidthChar::width(c).unwrap_or(0);
        if width + char_width > max_width {
            break;
        }
        result.push(c);
        width += char_width;
    }

    (result.clone(), result.width())
}

/// Represents a single tab's display data with pre-calculated widths.
/// Both lines use the SAME total_width, ensuring perfect alignment.
struct TabDisplay {
    // Title line components
    shortcut: String,
    shortcut_style: Style,
    indicator: String,
    indicator_style: Style,
    title: String,
    title_style: Style,

    // Project line components
    project: String,
    project_style: Style,

    // Shared width info - this is the key to alignment!
    // Total width = shortcut_width + indicator_width (1) + title_width
    shortcut_width: usize,
    title_width: usize,
}

impl TabDisplay {
    /// Generate spans for the title line
    fn title_spans(&self) -> Vec<Span<'static>> {
        vec![
            Span::styled(self.shortcut.clone(), self.shortcut_style),
            Span::styled(self.indicator.clone(), self.indicator_style),
            Span::styled(self.title.clone(), self.title_style),
        ]
    }

    /// Generate spans for the project line, using the SAME widths as title line
    fn project_spans(&self) -> Vec<Span<'static>> {
        // Pad to match shortcut + indicator width
        let prefix_width = self.shortcut_width + 1; // +1 for indicator
        let prefix = " ".repeat(prefix_width);

        // Pad project to match title width
        let project_width = self.project.width();
        let padding_needed = self.title_width.saturating_sub(project_width);
        let padded_project = format!("{}{}", self.project, " ".repeat(padding_needed));

        vec![
            Span::styled(prefix, Style::default()),
            Span::styled(padded_project, self.project_style),
        ]
    }
}

/// Renders a two-line tab bar:
/// - Line 1: Tab numbers and titles
/// - Line 2: Project names aligned under each tab
pub fn render_tab_bar(f: &mut Frame, app: &App, area: Rect) {
    // Split area into two lines
    let lines = Layout::vertical([Constraint::Length(1), Constraint::Length(1)]).split(area);

    let data_store = app.data_store.borrow();

    // Build both lines simultaneously
    let mut title_spans: Vec<Span> = Vec::new();
    let mut project_spans: Vec<Span> = Vec::new();

    // === Home Tab (always first, Option+1) ===
    let home_active = app.view == View::Home;

    // Home shortcut: "⌥1 "
    let home_shortcut = "⌥1 ";
    let home_shortcut_width = home_shortcut.width();
    let home_shortcut_style = if home_active {
        Style::default().fg(theme::ACCENT_PRIMARY)
    } else {
        Style::default().fg(theme::TEXT_MUTED)
    };

    // Home has no indicator (use space)
    let home_indicator = " ";

    // Home title
    let home_title = "Home";
    let home_title_width = home_title.width();
    let home_title_style = if home_active {
        Style::default()
            .fg(theme::ACCENT_SUCCESS)
            .add_modifier(ratatui::style::Modifier::BOLD)
    } else {
        theme::tab_inactive()
    };

    // Title line for Home
    title_spans.push(Span::styled(home_shortcut, home_shortcut_style));
    title_spans.push(Span::raw(home_indicator));
    title_spans.push(Span::styled(home_title, home_title_style));

    // Project line for Home (empty, but same width!)
    let home_prefix = " ".repeat(home_shortcut_width + 1); // +1 for indicator
    let home_project_pad = " ".repeat(home_title_width);
    project_spans.push(Span::styled(home_prefix, Style::default()));
    project_spans.push(Span::styled(home_project_pad, Style::default()));

    // Separator after home
    if !app.open_tabs().is_empty() {
        title_spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
        project_spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
    }

    // === Conversation Tabs ===
    let max_title_width = 14;

    for (i, tab) in app.open_tabs().iter().enumerate() {
        let is_active = i == app.active_tab_index();
        let tab_num = i + 2; // Start from 2 since 1 is Home

        // Build shortcut string and measure its actual width
        let shortcut = if tab_num <= 9 {
            format!("⌥{} ", tab_num)
        } else {
            "   ".to_string()
        };
        let shortcut_width = shortcut.width();

        let shortcut_style = if is_active {
            Style::default().fg(theme::ACCENT_PRIMARY)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        // Indicator (always 1 display cell)
        let (indicator, indicator_style) = if tab.is_draft() {
            ("+".to_string(), Style::default().fg(theme::ACCENT_SUCCESS))
        } else if tab.has_unread && !is_active {
            (
                card::BULLET.to_string(),
                Style::default().fg(theme::ACCENT_ERROR),
            )
        } else {
            (" ".to_string(), Style::default())
        };

        // Title - truncate and get actual display width
        let (title, title_width) = truncate_to_width(&tab.thread_title, max_title_width);
        let title_style = if is_active {
            theme::tab_active()
        } else if tab.has_unread {
            theme::tab_unread()
        } else {
            theme::tab_inactive()
        };

        // Project name - truncate to same max width
        let project_name = data_store.get_project_name(&tab.project_a_tag);
        let (project, _project_width) = truncate_plain_to_width(&project_name, max_title_width);
        let project_style = if is_active {
            Style::default().fg(theme::ACCENT_SUCCESS)
        } else {
            Style::default().fg(theme::TEXT_MUTED)
        };

        // Create tab display with shared width info
        let tab_display = TabDisplay {
            shortcut,
            shortcut_style,
            shortcut_width,
            indicator,
            indicator_style,
            title,
            title_style,
            title_width,
            project,
            project_style,
        };

        // Add spans from the same TabDisplay, ensuring alignment
        title_spans.extend(tab_display.title_spans());
        project_spans.extend(tab_display.project_spans());

        // Separator between tabs
        if i < app.open_tabs().len() - 1 {
            title_spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
            project_spans.push(Span::styled(" │ ", Style::default().fg(theme::TEXT_MUTED)));
        }
    }

    // Render both lines
    let title_line = Line::from(title_spans);
    let project_line = Line::from(project_spans);

    f.render_widget(Paragraph::new(title_line), lines[0]);
    f.render_widget(Paragraph::new(project_line), lines[1]);
}
