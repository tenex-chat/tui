// Centralized layout constants and utilities for consistent UI across all views
// All layout-related values should be defined here for maintainability

use ratatui::layout::Rect;

// =============================================================================
// PADDING CONSTANTS - Used for content spacing within views
// =============================================================================

/// Standard horizontal padding for main content areas (left + right)
/// Used in Home view, Chat view messages, etc.
pub const CONTENT_PADDING_H: u16 = 2;

/// Horizontal padding for modal content (left + right)
pub const MODAL_PADDING_H: u16 = 2;

// =============================================================================
// SIDEBAR CONSTANTS - Fixed widths for sidebar panels
// =============================================================================

/// Sidebar width for the home view (project list + filters)
pub const SIDEBAR_WIDTH_HOME: u16 = 42;

/// Sidebar width for the chat view (todos + metadata)
/// Increased from 30 to 45 (full 50% increase) to accommodate longer content.
/// User has accepted the trade-off of narrower message area on 80-column terminals.
/// Layout math: 80 - 45 - (2 * CONTENT_PADDING_H) = 80 - 45 - 4 = 31 columns
pub const SIDEBAR_WIDTH_CHAT: u16 = 45;

// =============================================================================
// CHROME CONSTANTS - Header/footer heights
// =============================================================================

/// Header height for chat view (title area)
pub const HEADER_HEIGHT_CHAT: u16 = 3;

/// Header height for other views
pub const HEADER_HEIGHT_DEFAULT: u16 = 1;

/// Footer height for chat view
pub const FOOTER_HEIGHT_CHAT: u16 = 2;

/// Footer height for other views (help bar)
pub const FOOTER_HEIGHT_DEFAULT: u16 = 1;

/// Tab bar height (top padding + title + project + bottom padding)
pub const TAB_BAR_HEIGHT: u16 = 4;

/// Status bar height (single line at very bottom of app)
pub const STATUSBAR_HEIGHT: u16 = 1;

// =============================================================================
// MODAL CONSTANTS - Sizing for popup modals
// =============================================================================

/// Default modal maximum width
pub const MODAL_DEFAULT_WIDTH: u16 = 70;

/// Default modal height as percentage of terminal
pub const MODAL_DEFAULT_HEIGHT_PERCENT: f32 = 0.7;

// =============================================================================
// LAYOUT HELPER FUNCTIONS
// =============================================================================

/// Apply horizontal padding to a Rect (reduces width and shifts x)
/// This is the single source of truth for horizontal padding logic
#[inline]
pub fn with_horizontal_padding(area: Rect, padding: u16) -> Rect {
    Rect {
        x: area.x + padding,
        y: area.y,
        width: area.width.saturating_sub(padding * 2),
        height: area.height,
    }
}

/// Apply content padding to a Rect (uses CONTENT_PADDING_H)
#[inline]
pub fn with_content_padding(area: Rect) -> Rect {
    with_horizontal_padding(area, CONTENT_PADDING_H)
}

/// Apply modal padding to a Rect (uses MODAL_PADDING_H)
#[inline]
pub fn with_modal_padding(area: Rect) -> Rect {
    with_horizontal_padding(area, MODAL_PADDING_H)
}
