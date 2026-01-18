pub const INDENT_UNIT: &str = "  ";
pub const SPACER: &str = "  ";
pub const BULLET_GLYPH: &str = "\u{25cf}";
pub const HOLLOW_BULLET_GLYPH: &str = "\u{25cb}";
pub const BULLET: &str = "\u{25cf} ";
pub const HOLLOW_BULLET: &str = "\u{25cb} ";
pub const BULLET_ACTIVE: &str = "\u{25c9}";
pub const LIST_BULLET_GLYPH: &str = "\u{2022}";
pub const LIST_BULLET: &str = "\u{2022} ";
pub const CHECKBOX_ON: &str = "\u{25a0}";
pub const CHECKBOX_OFF: &str = "\u{25a1}";
pub const CHECKBOX_ON_PAD: &str = "\u{25a0} ";
pub const CHECKBOX_OFF_PAD: &str = "\u{25a1} ";
pub const COLLAPSE_OPEN: &str = "\u{25bc} ";
pub const COLLAPSE_CLOSED: &str = "\u{25b6} ";
pub const COLLAPSE_LEAF: &str = "\u{2514}\u{2500}";
pub const ACTIVITY_GLYPH: &str = "\u{27f3} ";
pub const CHECKMARK: &str = "\u{2713}";
pub const TODO_DONE_GLYPH: &str = CHECKMARK;
pub const TODO_IN_PROGRESS_GLYPH: &str = "\u{25d0}";
pub const TODO_PENDING_GLYPH: &str = HOLLOW_BULLET_GLYPH;
pub const TODO_SKIPPED_GLYPH: &str = "\u{2717}"; // ✗
pub const META_SEPARATOR: &str = " \u{2022} ";

// Half-block border characters (like lipgloss OuterHalfBlockBorder)
// Creates visually "half-height" borders for softer edges
use ratatui::symbols::border::Set;

pub const OUTER_HALF_BLOCK_BORDER: Set = Set {
    top_left: "\u{259B}",     // ▛
    top_right: "\u{259C}",    // ▜
    bottom_left: "\u{2599}",  // ▙
    bottom_right: "\u{259F}", // ▟
    vertical_left: "\u{258C}",  // ▌
    vertical_right: "\u{2590}", // ▐
    horizontal_top: "\u{2580}", // ▀
    horizontal_bottom: "\u{2584}", // ▄
};
