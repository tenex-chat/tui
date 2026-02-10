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

// =============================================================================
// Modal Builder - Reusable modal component with guaranteed overlay
// =============================================================================

/// A reusable modal component with builder pattern.
///
/// This ensures consistent modal rendering across the app:
/// - Always renders the dimmed overlay behind the modal
/// - Always renders the modal background
/// - Provides optional header and search components
///
/// # Example
/// ```ignore
/// Modal::new("Select Agent")
///     .hint("esc")
///     .size(ModalSize { max_width: 60, height_percent: 0.5 })
///     .search(&filter, "Search agents...")
///     .render(f, area, |f, content_area| {
///         // Render your custom content here
///         render_modal_items(f, content_area, &items);
///     });
/// ```
pub struct Modal<'a> {
    title: &'a str,
    hint: &'a str,
    size: ModalSize,
    search: Option<(&'a str, &'a str)>, // (filter, placeholder)
}

impl<'a> Modal<'a> {
    /// Create a new modal with the given title
    pub fn new(title: &'a str) -> Self {
        Self {
            title,
            hint: "esc",
            size: ModalSize::default(),
            search: None,
        }
    }

    /// Set the hint text shown in the top-right corner (default: "esc")
    pub fn hint(mut self, hint: &'a str) -> Self {
        self.hint = hint;
        self
    }

    /// Set the modal size
    pub fn size(mut self, size: ModalSize) -> Self {
        self.size = size;
        self
    }

    /// Add a search input field to the modal
    pub fn search(mut self, filter: &'a str, placeholder: &'a str) -> Self {
        self.search = Some((filter, placeholder));
        self
    }

    /// Render the modal and call the provided closure with the content area.
    ///
    /// This handles:
    /// 1. Rendering the dimmed overlay over the entire terminal
    /// 2. Calculating and rendering the modal background
    /// 3. Rendering the header (title + hint)
    /// 4. Optionally rendering the search field
    /// 5. Calling your closure with the remaining content area
    ///
    /// Returns the popup area (useful for cursor positioning).
    pub fn render<F>(self, f: &mut Frame, terminal_area: Rect, content_fn: F) -> Rect
    where
        F: FnOnce(&mut Frame, Rect),
    {
        // 1. Always render the overlay first
        render_modal_overlay(f, terminal_area);

        // 2. Calculate and render modal background
        let popup_area = modal_area(terminal_area, &self.size);
        render_modal_background(f, popup_area);

        // 3. Add vertical padding for inner content
        let inner_area = Rect::new(
            popup_area.x,
            popup_area.y + 1,
            popup_area.width,
            popup_area.height.saturating_sub(2),
        );

        // 4. Render header
        let remaining = render_modal_header(f, inner_area, self.title, self.hint);

        // 5. Optionally render search
        let content_area = if let Some((filter, placeholder)) = self.search {
            render_modal_search(f, remaining, filter, placeholder)
        } else {
            remaining
        };

        // 6. Call the content function with remaining area
        content_fn(f, content_area);

        popup_area
    }

    /// Render the modal without a content function, returning areas for manual rendering.
    ///
    /// Returns (popup_area, content_area) for cases where you need more control.
    pub fn render_frame(self, f: &mut Frame, terminal_area: Rect) -> (Rect, Rect) {
        // 1. Always render the overlay first
        render_modal_overlay(f, terminal_area);

        // 2. Calculate and render modal background
        let popup_area = modal_area(terminal_area, &self.size);
        render_modal_background(f, popup_area);

        // 3. Add vertical padding for inner content
        let inner_area = Rect::new(
            popup_area.x,
            popup_area.y + 1,
            popup_area.width,
            popup_area.height.saturating_sub(2),
        );

        // 4. Render header
        let remaining = render_modal_header(f, inner_area, self.title, self.hint);

        // 5. Optionally render search
        let content_area = if let Some((filter, placeholder)) = self.search {
            render_modal_search(f, remaining, filter, placeholder)
        } else {
            remaining
        };

        (popup_area, content_area)
    }
}

// =============================================================================
// Internal helper functions (used by Modal struct)
// =============================================================================

/// Calculate centered modal area
fn modal_area(terminal_area: Rect, size: &ModalSize) -> Rect {
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
fn render_modal_overlay(f: &mut Frame, terminal_area: Rect) {
    f.render_widget(DimOverlay, terminal_area);
}

/// Render the modal background (clears area and fills with modal bg color)
fn render_modal_background(f: &mut Frame, area: Rect) {
    f.render_widget(Clear, area);
    let bg_block = Block::default().style(Style::default().bg(theme::BG_MODAL));
    f.render_widget(bg_block, area);
}

/// Render modal header with title on left and hint on right
/// Returns the remaining area below the header
fn render_modal_header(f: &mut Frame, area: Rect, title: &str, hint: &str) -> Rect {
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
    render_modal_items_with_scroll(f, area, items, 0);
}

/// Render a list of simple items with scroll offset support
pub fn render_modal_items_with_scroll(f: &mut Frame, area: Rect, items: &[ModalItem], scroll_offset: usize) {
    let content_area = layout::with_modal_padding(area);

    // Skip items before the scroll offset
    let visible_items = items.iter().skip(scroll_offset);

    for (idx, item) in visible_items.enumerate() {
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

/// Calculate the number of visible items in a modal content area.
/// This accounts for modal padding and returns the actual height available for list items.
/// Use this instead of hard-coded calculations like `modal_height.saturating_sub(9)`.
pub fn visible_items_in_content_area(content_area: Rect) -> usize {
    let padded = layout::with_modal_padding(content_area);
    padded.height as usize
}
