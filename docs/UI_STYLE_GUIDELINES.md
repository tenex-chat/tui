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

## Color Mapping Reference

| Semantic Purpose | Theme Constant | Approximate Color |
|-----------------|----------------|-------------------|
| Primary text | `TEXT_PRIMARY` | Off-white |
| Secondary/muted text | `TEXT_MUTED` | Gray |
| Hints, placeholders | `TEXT_DIM` | Dark gray |
| Focus, links, interactive | `ACCENT_PRIMARY` | Muted blue |
| Success, online, complete | `ACCENT_SUCCESS` | Muted green |
| Warnings, pending | `ACCENT_WARNING` | Muted orange |
| Errors, urgent | `ACCENT_ERROR` | Muted red |
| Special (agents, images) | `ACCENT_SPECIAL` | Muted purple |
| Card backgrounds | `BG_CARD` | Very dark gray |
| Selected item background | `BG_SELECTED` | Slightly lighter dark |
| Secondary areas | `BG_SECONDARY` | Dark gray |
| Input backgrounds | `BG_INPUT` | Very dark gray |
| Active borders | `BORDER_ACTIVE` | Medium gray |
| Inactive borders | `BORDER_INACTIVE` | Dark gray |

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

## Pre-commit Validation

This repository has a pre-commit hook that validates style guideline compliance. Commits will be blocked if:

- Raw `Color::` values are used (except `Color::Black`)
- Colors are defined inline instead of using theme constants
- New color constants are added without semantic naming
