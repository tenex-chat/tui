# UI Style Guidelines

This document defines the styling rules for the TENEX TUI. All UI code MUST use the centralized theme system.

## Core Principle

**Never use hardcoded colors.** All colors must come from `src/ui/theme.rs`.

## Required Import

Every file that renders UI must import the theme:

```rust
use crate::ui::theme;
```

## Color Usage

### DO NOT use raw Color values:

```rust
// WRONG - hardcoded colors
Style::default().fg(Color::Cyan)
Style::default().fg(Color::Rgb(86, 156, 214))
Color::DarkGray
```

### DO use theme constants:

```rust
// CORRECT - theme constants
Style::default().fg(theme::ACCENT_PRIMARY)
Style::default().fg(theme::TEXT_MUTED)
theme::BG_CARD
```

## Layout Rules

1. **Sidebars go on the RIGHT**, not left
2. **Sidebars should have `BG_SIDEBAR` background** (subtle gray, not black)
3. **App background is pure black** (`BG_APP` = #000000)
4. **NO full borders around sections** - Don't use `Borders::ALL` on content sections
5. **LEFT border indicators only** - Items that belong to something (project, user) get a left border with deterministic color
6. **Spacing from edges** - Content needs 2+ character padding from screen edges
7. **Selection color is subtle** - Use `BG_SELECTED` which is very dim gray, not bright

## Deterministic Colors

Items use deterministic colors for visual grouping - same entity = same color consistently:

- **Project items**: Use `theme::project_color(a_tag)` for left border indicator
- **User items**: Use `theme::user_color(pubkey)` for left border indicator

This creates visual grouping where all items belonging to the same project share the same left border color, making it easy to scan and identify related items.

```rust
// Project-owned item (task, event, etc)
let border_color = theme::project_color(&a_tag);
Block::default().borders(Borders::LEFT).border_style(Style::default().fg(border_color))

// User-owned item (message, profile, etc)
let border_color = theme::user_color(&pubkey);
Block::default().borders(Borders::LEFT).border_style(Style::default().fg(border_color))
```

## Color Mapping Reference

| Semantic Purpose | Theme Constant | Approximate Color |
|-----------------|----------------|-------------------|
| App background | `BG_APP` | Pure black (#000000) |
| Sidebar background | `BG_SIDEBAR` | Very dark gray (#171717) |
| Selected item background | `BG_SELECTED` | Subtle gray (#202020) - barely visible |
| Primary text | `TEXT_PRIMARY` | Off-white |
| Secondary/muted text | `TEXT_MUTED` | Gray |
| Hints, placeholders | `TEXT_DIM` | Dark gray |
| Focus, links, interactive | `ACCENT_PRIMARY` | Muted blue |
| Success, online, complete | `ACCENT_SUCCESS` | Muted green |
| Warnings, pending | `ACCENT_WARNING` | Muted orange |
| Errors, urgent | `ACCENT_ERROR` | Muted red |
| Special (agents, images) | `ACCENT_SPECIAL` | Muted purple |
| Card backgrounds | `BG_CARD` | Very dark gray |
| Secondary areas | `BG_SECONDARY` | Dark gray |
| Input backgrounds | `BG_INPUT` | Very dark gray |
| Active borders | `BORDER_ACTIVE` | Medium gray |
| Inactive borders | `BORDER_INACTIVE` | Dark gray |

**Note on `BG_SELECTED`**: This should be VERY subtle - a barely visible highlight that indicates selection without being visually jarring. The goal is to show which item is selected without drawing attention away from the content itself.

## User Colors

For deterministic user identification (same user = same color):

```rust
// CORRECT
let color = theme::user_color(&pubkey);

// WRONG - don't define your own color palettes
let colors = [Color::Rgb(86, 156, 214), ...];
```

## Style Functions

Use semantic style functions when available:

```rust
// Text styles
theme::text_primary()
theme::text_muted()
theme::text_bold()

// Border styles
theme::border_active()
theme::border_inactive()

// Tab styles
theme::tab_active()
theme::tab_inactive()
theme::tab_unread()

// Todo styles
theme::todo_done()
theme::todo_in_progress()
theme::todo_pending()

// Tool call styles
theme::tool_name()
theme::tool_target()

// Streaming indicators
theme::streaming_indicator()
theme::typing_indicator()
```

## Exceptions

The only acceptable use of raw `Color::` is:

1. `Color::Black` - for text on colored backgrounds (contrast)
2. When receiving a color as a function parameter (e.g., `indicator_color: Color`)

## Adding New Colors

If you need a new color:

1. **DO NOT** add a hardcoded color in your view file
2. **DO** add a new constant to `src/ui/theme.rs`
3. **DO** give it a semantic name (what it's for, not what color it is)
4. **DO** use a muted tone consistent with the existing palette

## Design Philosophy

- **Muted, not harsh**: No bright cyans, magentas, or saturated colors
- **Subtle indicators**: Left borders, not full borders everywhere
- **Dark backgrounds**: Cards use very dark grays, not black
- **Consistent hierarchy**: Primary text is brightest, secondary is muted, hints are dim
- **User distinction**: Users get consistent colors, but from a muted palette

## Modal Component

All modals should use the centralized modal component from `crate::ui::components`. This ensures consistent styling across all modal dialogs.

### Required Import

```rust
use crate::ui::components::{
    modal_area, render_modal_background, render_modal_header, render_modal_items,
    render_modal_search, render_modal_sections, ModalItem, ModalSection, ModalSize,
};
```

### Modal Structure

Modals follow this visual structure:

1. **No outer border** - Modals have a dark background (`BG_MODAL`) without visible borders
2. **Header** - Title on left, hint (e.g., "esc") on right
3. **Search** (optional) - First letter highlighted in accent color
4. **Sections** - Grouped items with italic orange headers
5. **Items** - Text with right-aligned shortcuts, selected item has accent background
6. **Hints** - Keyboard hints at the bottom

### Basic Usage

```rust
fn render_my_modal(f: &mut Frame, app: &App, area: Rect) {
    let size = ModalSize {
        max_width: 70,
        height_percent: 0.7,
    };

    let popup_area = modal_area(area, &size);
    render_modal_background(f, popup_area);

    let inner_area = Rect::new(
        popup_area.x,
        popup_area.y + 1,
        popup_area.width,
        popup_area.height.saturating_sub(3),
    );

    // Header with title and hint
    let remaining = render_modal_header(f, inner_area, "Modal Title", "esc");

    // Optional: Search input
    let remaining = render_modal_search(f, remaining, &filter, "Search...");

    // Build items
    let items: Vec<ModalItem> = data.iter().enumerate().map(|(i, item)| {
        ModalItem::new(&item.name)
            .with_shortcut("ctrl+x")
            .selected(i == selected_index)
    }).collect();

    render_modal_items(f, remaining, &items);

    // Render hints at bottom
    let hints_area = Rect::new(
        popup_area.x + 2,
        popup_area.y + popup_area.height.saturating_sub(2),
        popup_area.width.saturating_sub(4),
        1,
    );
    let hints = Paragraph::new("hint text").style(Style::default().fg(theme::TEXT_MUTED));
    f.render_widget(hints, hints_area);
}
```

### With Sections

```rust
let sections = vec![
    ModalSection::new("Online (3)").with_items(online_items),
    ModalSection::new("Offline (5)").with_items(offline_items),
];
render_modal_sections(f, remaining, &sections);
```

### Modal Style Functions

```rust
// Modal-specific theme functions
theme::modal_title()           // Bold primary text
theme::modal_hint()            // Muted hint text
theme::modal_search_placeholder()
theme::modal_search_active()
theme::modal_section_header()  // Italic orange for section headers
theme::modal_item()            // Normal item text
theme::modal_item_selected()   // Selected: black text on orange background
theme::modal_item_shortcut()   // Muted shortcut text
theme::modal_item_shortcut_selected()
```

## Pre-commit Validation

This repository has a pre-commit hook that validates style guideline compliance. Commits will be blocked if:

- Raw `Color::` values are used (except `Color::Black`)
- Colors are defined inline instead of using theme constants
- New color constants are added without semantic naming
