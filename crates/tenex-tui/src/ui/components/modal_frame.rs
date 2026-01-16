use crate::ui::layout;
use crate::ui::theme;
use ratatui::{
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Clear, Paragraph, Widget},
    Frame,
};

/// Configuration for modal sizing
pub struct ModalSize {
    /// Maximum width in columns (will be capped by terminal width - 4)
    pub max_width: u16,
    /// Height as percentage of terminal height (0.0 - 1.0)
    pub height_percent: f32,
}

impl Default for ModalSize {
    fn default() -> Self {
        Self {
            max_width: layout::MODAL_DEFAULT_WIDTH,
            height_percent: layout::MODAL_DEFAULT_HEIGHT_PERCENT,
        }
    }
}

/// Calculate centered modal area
pub fn modal_area(terminal_area: Rect, size: &ModalSize) -> Rect {
    let popup_width = size.max_width.min(terminal_area.width.saturating_sub(4));
    let popup_height = (terminal_area.height as f32 * size.height_percent) as u16;
    let popup_x = terminal_area.x + (terminal_area.width.saturating_sub(popup_width)) / 2;
    let popup_y = terminal_area.y + (terminal_area.height.saturating_sub(popup_height)) / 2;
    Rect::new(popup_x, popup_y, popup_width, popup_height)
}

/// A widget that dims the existing content by applying a semi-transparent overlay effect
struct DimOverlay;

impl Widget for DimOverlay {
    fn render(self, area: Rect, buf: &mut Buffer) {
        // Apply dim modifier and darker foreground to existing cells to create fade effect
        // This preserves the content but makes it appear dimmed/faded
        for y in area.y..area.y + area.height {
            for x in area.x..area.x + area.width {
                if let Some(cell) = buf.cell_mut((x, y)) {
                    // Apply dim modifier to fade the text
                    cell.set_style(
                        Style::default()
                            .add_modifier(Modifier::DIM)
                            .bg(theme::BG_MODAL_OVERLAY),
                    );
                }
            }
        }
    }
}

/// Render dimmed overlay over the entire terminal area
/// This creates a semi-transparent fade effect behind modals by dimming existing content
pub fn render_modal_overlay(f: &mut Frame, terminal_area: Rect) {
    f.render_widget(DimOverlay, terminal_area);
}

/// Render the modal background (clears area and fills with modal bg color)
pub fn render_modal_background(f: &mut Frame, area: Rect) {
    f.render_widget(Clear, area);
    let bg_block = Block::default().style(Style::default().bg(theme::BG_MODAL));
    f.render_widget(bg_block, area);
}

/// Render modal header with title on left and hint on right
/// Returns the remaining area below the header
pub fn render_modal_header(f: &mut Frame, area: Rect, title: &str, hint: &str) -> Rect {
    // Header takes 2 lines (1 for content + 1 for spacing)
    let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(area);

    // Add horizontal padding using centralized layout
    let header_area = layout::with_modal_padding(chunks[0]);

    let title_span = Span::styled(title, theme::modal_title());
    let hint_span = Span::styled(hint, theme::modal_hint());

    // Calculate spacing between title and hint
    let title_len = title.len();
    let hint_len = hint.len();
    let available = header_area.width as usize;
    let spacing = available.saturating_sub(title_len + hint_len);

    let header_line = Line::from(vec![
        title_span,
        Span::raw(" ".repeat(spacing)),
        hint_span,
    ]);

    f.render_widget(Paragraph::new(header_line), header_area);

    chunks[1]
}

/// Render a search input field
/// Returns the remaining area below the search field
pub fn render_modal_search(
    f: &mut Frame,
    area: Rect,
    filter: &str,
    placeholder: &str,
) -> Rect {
    let chunks = Layout::vertical([Constraint::Length(2), Constraint::Min(0)]).split(area);

    let search_area = layout::with_modal_padding(chunks[0]);

    if filter.is_empty() {
        // Render placeholder with first letter highlighted
        let first_char = placeholder.chars().next().unwrap_or('S');
        let rest = &placeholder[first_char.len_utf8()..];

        let line = Line::from(vec![
            Span::styled(first_char.to_string(), theme::modal_search_active()),
            Span::styled(rest, theme::modal_search_placeholder()),
        ]);
        f.render_widget(Paragraph::new(line), search_area);
    } else {
        // Render active filter text
        let line = Line::from(Span::styled(filter, theme::modal_search_active()));
        f.render_widget(Paragraph::new(line), search_area);
    }

    chunks[1]
}

/// A modal section with a header and items
pub struct ModalSection {
    pub header: String,
    pub items: Vec<ModalItem>,
}

impl ModalSection {
    pub fn new(header: impl Into<String>) -> Self {
        Self {
            header: header.into(),
            items: Vec::new(),
        }
    }

    pub fn with_items(mut self, items: Vec<ModalItem>) -> Self {
        self.items = items;
        self
    }
}

/// A modal item with text and optional shortcut
pub struct ModalItem {
    pub text: String,
    pub shortcut: Option<String>,
    pub is_selected: bool,
}

impl ModalItem {
    pub fn new(text: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            shortcut: None,
            is_selected: false,
        }
    }

    pub fn with_shortcut(mut self, shortcut: impl Into<String>) -> Self {
        self.shortcut = Some(shortcut.into());
        self
    }

    pub fn selected(mut self, is_selected: bool) -> Self {
        self.is_selected = is_selected;
        self
    }
}

/// Render modal sections with items
pub fn render_modal_sections(f: &mut Frame, area: Rect, sections: &[ModalSection]) {
    let content_area = layout::with_modal_padding(area);

    let mut y_offset = 0u16;

    for (section_idx, section) in sections.iter().enumerate() {
        // Add spacing before sections (except first)
        if section_idx > 0 {
            y_offset += 1;
        }

        // Render section header
        if y_offset < content_area.height {
            let header_area = Rect::new(
                content_area.x,
                content_area.y + y_offset,
                content_area.width,
                1,
            );
            let header_line =
                Line::from(Span::styled(&section.header, theme::modal_section_header()));
            f.render_widget(Paragraph::new(header_line), header_area);
            y_offset += 1;
        }

        // Render items
        for item in &section.items {
            if y_offset >= content_area.height {
                break;
            }

            let item_area = Rect::new(
                content_area.x,
                content_area.y + y_offset,
                content_area.width,
                1,
            );

            render_modal_item(f, item_area, item);
            y_offset += 1;
        }
    }
}

/// Render a single modal item
fn render_modal_item(f: &mut Frame, area: Rect, item: &ModalItem) {
    let text_style = if item.is_selected {
        theme::modal_item_selected()
    } else {
        theme::modal_item()
    };

    let shortcut_style = if item.is_selected {
        theme::modal_item_shortcut_selected()
    } else {
        theme::modal_item_shortcut()
    };

    // If selected, fill the entire line with background color
    if item.is_selected {
        let bg_block = Block::default().style(Style::default().bg(theme::ACCENT_WARNING));
        f.render_widget(bg_block, area);
    }

    let shortcut_text = item.shortcut.as_deref().unwrap_or("");
    let text_len = item.text.len();
    let shortcut_len = shortcut_text.len();
    let available = area.width as usize;
    let spacing = available.saturating_sub(text_len + shortcut_len);

    let line = Line::from(vec![
        Span::styled(&item.text, text_style),
        Span::styled(" ".repeat(spacing), text_style),
        Span::styled(shortcut_text, shortcut_style),
    ]);

    f.render_widget(Paragraph::new(line), area);
}

/// Render a list of simple items (no sections)
pub fn render_modal_items(f: &mut Frame, area: Rect, items: &[ModalItem]) {
    let content_area = layout::with_modal_padding(area);

    for (idx, item) in items.iter().enumerate() {
        if idx as u16 >= content_area.height {
            break;
        }

        let item_area = Rect::new(
            content_area.x,
            content_area.y + idx as u16,
            content_area.width,
            1,
        );

        render_modal_item(f, item_area, item);
    }
}

/// Complete modal frame that combines all elements
/// This is a convenience function for simple command palette-style modals
pub fn render_command_modal(
    f: &mut Frame,
    terminal_area: Rect,
    title: &str,
    hint: &str,
    filter: &str,
    search_placeholder: &str,
    sections: &[ModalSection],
    size: ModalSize,
) {
    let area = modal_area(terminal_area, &size);
    render_modal_background(f, area);

    // Add vertical padding
    let inner_area = Rect::new(area.x, area.y + 1, area.width, area.height.saturating_sub(2));

    let remaining = render_modal_header(f, inner_area, title, hint);
    let remaining = render_modal_search(f, remaining, filter, search_placeholder);
    render_modal_sections(f, remaining, sections);
}
