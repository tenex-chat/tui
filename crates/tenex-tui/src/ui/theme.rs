// Centralized theme system for consistent UI styling
// All colors and styles are defined here - edit this file to change the look

use ratatui::style::{Color, Modifier, Style};

// =============================================================================
// COLOR PALETTE - Muted, sophisticated colors inspired by modern terminal UIs
// =============================================================================

/// App background - pure black for contrast
pub const BG_APP: Color = Color::Rgb(0, 0, 0);

/// Card/message background - very subtle lift from black
pub const BG_CARD: Color = Color::Rgb(18, 18, 18);

/// Selected item background - subtle highlight (like bg-neutral-800)
pub const BG_SELECTED: Color = Color::Rgb(32, 32, 32);

/// Active tab background - very subtle lift
pub const BG_TAB_ACTIVE: Color = Color::Rgb(28, 28, 32);

/// Search match highlight background - subtle yellow tint
pub const BG_SEARCH_MATCH: Color = Color::Rgb(60, 55, 30);

/// Current search match highlight - brighter yellow tint
pub const BG_SEARCH_CURRENT: Color = Color::Rgb(80, 70, 25);

/// Sidebar background - very dark, almost black
pub const BG_SIDEBAR: Color = Color::Rgb(12, 12, 12);

/// Dark background for secondary areas
pub const BG_SECONDARY: Color = Color::Rgb(23, 23, 23);

/// Input field background
pub const BG_INPUT: Color = Color::Rgb(18, 18, 18);

// -----------------------------------------------------------------------------
// Text Colors
// -----------------------------------------------------------------------------

/// Primary text - off-white for readability
pub const TEXT_PRIMARY: Color = Color::Rgb(220, 220, 220);

/// Secondary/muted text
pub const TEXT_MUTED: Color = Color::Rgb(128, 128, 128);

/// Dimmed text for hints, placeholders
pub const TEXT_DIM: Color = Color::Rgb(90, 90, 90);

/// White text - pure white for high-contrast indicators
pub const TEXT_WHITE: Color = Color::Rgb(255, 255, 255);

// -----------------------------------------------------------------------------
// Accent Colors - Muted, not harsh
// -----------------------------------------------------------------------------

/// Primary accent - muted blue (for interactive elements, focus)
pub const ACCENT_PRIMARY: Color = Color::Rgb(86, 156, 214);

/// Success/positive - muted green
pub const ACCENT_SUCCESS: Color = Color::Rgb(106, 153, 85);

/// Warning - muted amber/orange
pub const ACCENT_WARNING: Color = Color::Rgb(206, 145, 120);

/// Error - muted red
pub const ACCENT_ERROR: Color = Color::Rgb(244, 112, 112);

/// Special - muted purple (for agents, special content)
pub const ACCENT_SPECIAL: Color = Color::Rgb(169, 154, 203);

// -----------------------------------------------------------------------------
// Border/Indicator Colors
// -----------------------------------------------------------------------------

/// Active/focused border
pub const BORDER_ACTIVE: Color = Color::Rgb(100, 100, 100);

/// Inactive border
pub const BORDER_INACTIVE: Color = Color::Rgb(60, 60, 60);

/// Progress bar empty
pub const PROGRESS_EMPTY: Color = Color::Rgb(60, 60, 60);

// -----------------------------------------------------------------------------
// User Colors - Palette for deterministic user identification
// More muted than before
// -----------------------------------------------------------------------------

pub const USER_PALETTE: [Color; 8] = [
    Color::Rgb(86, 156, 214),  // Muted blue
    Color::Rgb(106, 153, 85),  // Muted green
    Color::Rgb(169, 154, 203), // Muted purple
    Color::Rgb(206, 145, 120), // Muted orange
    Color::Rgb(78, 154, 154),  // Muted teal
    Color::Rgb(180, 180, 120), // Muted yellow
    Color::Rgb(180, 100, 100), // Muted red
    Color::Rgb(140, 140, 170), // Muted lavender
];

/// Get a deterministic color for a user based on their pubkey
pub fn user_color(pubkey: &str) -> Color {
    let hash: usize = pubkey.bytes().map(|b| b as usize).sum();
    USER_PALETTE[hash % USER_PALETTE.len()]
}

/// Get a deterministic color for a project based on its a_tag
pub fn project_color(a_tag: &str) -> Color {
    let hash: usize = a_tag.bytes().map(|b| b as usize).sum();
    USER_PALETTE[hash % USER_PALETTE.len()]
}

// -----------------------------------------------------------------------------
// LLM Metadata Colors - For displaying token counts, model info, etc.
// -----------------------------------------------------------------------------

pub const LLM_METADATA_PALETTE: [Color; 8] = [
    Color::Rgb(86, 156, 214),  // Blue - prompt
    Color::Rgb(106, 153, 85),  // Green - completion
    Color::Rgb(169, 154, 203), // Purple - total
    Color::Rgb(206, 145, 120), // Orange - model
    Color::Rgb(180, 100, 140), // Pink - ral
    Color::Rgb(78, 154, 154),  // Cyan - cached
    Color::Rgb(180, 180, 120), // Yellow - reasoning
    Color::Rgb(180, 100, 100), // Red - cost
];

/// Get a deterministic color for an LLM metadata key
pub fn llm_metadata_color(key: &str) -> Color {
    let hash: usize = key.bytes().map(|b| b as usize).sum();
    LLM_METADATA_PALETTE[hash % LLM_METADATA_PALETTE.len()]
}

// =============================================================================
// STYLE FUNCTIONS - Semantic styles for common UI patterns
// =============================================================================

// -----------------------------------------------------------------------------
// Text Styles
// -----------------------------------------------------------------------------

pub fn text_primary() -> Style {
    Style::default().fg(TEXT_PRIMARY)
}

pub fn text_muted() -> Style {
    Style::default().fg(TEXT_MUTED)
}


// -----------------------------------------------------------------------------
// Status Styles
// -----------------------------------------------------------------------------

pub fn status_success() -> Style {
    Style::default().fg(ACCENT_SUCCESS)
}

pub fn status_warning() -> Style {
    Style::default().fg(ACCENT_WARNING)
}

pub fn status_error() -> Style {
    Style::default().fg(ACCENT_ERROR)
}


// -----------------------------------------------------------------------------
// Tab/Navigation Styles
// -----------------------------------------------------------------------------

pub fn tab_active() -> Style {
    Style::default()
        .fg(TEXT_PRIMARY)
        .bg(BG_TAB_ACTIVE)
        .add_modifier(Modifier::BOLD)
}

pub fn tab_inactive() -> Style {
    Style::default().fg(TEXT_MUTED)
}

pub fn tab_unread() -> Style {
    // Unread activity (no mention) - white with bold
    Style::default()
        .fg(TEXT_WHITE)
        .add_modifier(Modifier::BOLD)
}

pub fn tab_waiting_for_user() -> Style {
    // User is mentioned - orange (warning color) with bold
    Style::default()
        .fg(ACCENT_WARNING)
        .add_modifier(Modifier::BOLD)
}

// -----------------------------------------------------------------------------
// Tool Call Styles
// -----------------------------------------------------------------------------

// -----------------------------------------------------------------------------
// Todo/Progress Styles
// -----------------------------------------------------------------------------

pub fn todo_done() -> Style {
    Style::default().fg(ACCENT_SUCCESS)
}

pub fn todo_in_progress() -> Style {
    Style::default().fg(ACCENT_PRIMARY)
}

pub fn todo_pending() -> Style {
    Style::default().fg(TEXT_DIM)
}

pub fn todo_skipped() -> Style {
    Style::default().fg(ACCENT_ERROR)
}

// -----------------------------------------------------------------------------
// Project/Agent Styles
// -----------------------------------------------------------------------------


// -----------------------------------------------------------------------------
// Modal Styles - Consistent command palette / popup modal styling
// -----------------------------------------------------------------------------

/// Modal background - slightly elevated from pure black
pub const BG_MODAL: Color = Color::Rgb(24, 24, 24);

/// Modal overlay - dims the background behind modals (semi-dark to create fade effect)
pub const BG_MODAL_OVERLAY: Color = Color::Rgb(10, 10, 12);

/// Modal title style
pub fn modal_title() -> Style {
    Style::default()
        .fg(TEXT_PRIMARY)
        .add_modifier(Modifier::BOLD)
}

/// Modal hint text (e.g., "esc" in corner)
pub fn modal_hint() -> Style {
    Style::default().fg(TEXT_MUTED)
}

/// Modal search input placeholder
pub fn modal_search_placeholder() -> Style {
    Style::default().fg(TEXT_DIM)
}

/// Modal search input active text
pub fn modal_search_active() -> Style {
    Style::default().fg(ACCENT_WARNING)
}

/// Modal section header (grouped items)
pub fn modal_section_header() -> Style {
    Style::default()
        .fg(ACCENT_WARNING)
        .add_modifier(Modifier::ITALIC)
}

/// Modal item normal state
pub fn modal_item() -> Style {
    Style::default().fg(TEXT_PRIMARY)
}

/// Modal item selected state - accent background with contrasting text
pub fn modal_item_selected() -> Style {
    Style::default()
        .fg(Color::Black)
        .bg(ACCENT_WARNING)
        .add_modifier(Modifier::BOLD)
}

/// Modal item shortcut/hint text (right-aligned)
pub fn modal_item_shortcut() -> Style {
    Style::default().fg(TEXT_MUTED)
}

/// Modal item shortcut when selected
pub fn modal_item_shortcut_selected() -> Style {
    Style::default().fg(Color::Black).bg(ACCENT_WARNING)
}

