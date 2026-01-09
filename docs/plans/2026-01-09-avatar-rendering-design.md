# Avatar Rendering in Thread List

## Overview

Display user avatars in the thread list to the left of each conversation card. Avatars are fetched from Nostr profile `picture` URLs, cached to disk, and rendered using terminal graphics protocols with halfblock fallback.

## Architecture

```
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Profile Load   │────▶│  Avatar Fetcher  │────▶│   Disk Cache    │
│  (from Nostr)   │     │  (background)    │     │ ~/.cache/tenex/ │
└─────────────────┘     └──────────────────┘     └─────────────────┘
                                                          │
                                                          ▼
┌─────────────────┐     ┌──────────────────┐     ┌─────────────────┐
│  Thread List    │◀────│  Avatar Renderer │◀────│  Memory Cache   │
│  (home.rs)      │     │  (ratatui-image) │     │  (decoded imgs) │
└─────────────────┘     └──────────────────┘     └─────────────────┘
```

## Decisions

| Decision | Choice | Rationale |
|----------|--------|-----------|
| Placement | Left of card | 2-char column before the colored border indicator |
| Size | 2x4 cells | Matches card height, ~4x8 pixel resolution with halfblocks |
| Protocol | Auto-detect + fallback | Kitty/iTerm2/Sixel where supported, halfblocks everywhere else |
| Caching | Background prefetch + disk | Best UX, persists across sessions |
| Fallback | Colored initials | Uses existing `theme::user_color` with first initial |

## Dependencies

```toml
ratatui-image = "1"
image = "0.25"
```

## Module Structure

### `src/ui/avatars.rs` (new)

```rust
pub struct AvatarCache {
    // pubkey -> decoded image (None = no avatar or failed)
    images: HashMap<String, Option<DynamicImage>>,
    // pubkeys currently being fetched
    pending: HashSet<String>,
    // ratatui-image protocol picker
    picker: Picker,
}

impl AvatarCache {
    pub fn new() -> Self;
    pub fn get(&self, pubkey: &str) -> Option<&DynamicImage>;
    pub fn request_fetch(&mut self, pubkey: &str, url: &str);
    pub fn picker(&self) -> &Picker;
}

pub fn render_avatar(
    cache: &AvatarCache,
    pubkey: &str,
    display_name: &str,
    area: Rect,
    frame: &mut Frame,
);
```

## Cache Storage

- Directory: `~/.cache/tenex/avatars/`
- Filename: `{first-8-chars-of-pubkey}.png`
- Image size: 32x32 pixels (resized on save)

## Thread List Integration

### Current card layout:
```
│ [Status] Title                    2h ago
│ ● Project  @author  3 nested
│ Preview message text...
│
```

### New layout with avatar:
```
██│ [Status] Title                  2h ago
██│ ● Project  @author  3 nested
██│ Preview message text...
██│
```

The avatar occupies 2 characters width, 4 lines height. For compact cards (nested threads), avatar scales to 2x2.

## Fallback Rendering

When no avatar image is available:
1. Fill 2x4 character area with user's deterministic color from `theme::user_color(pubkey)`
2. Overlay first character of display name, centered

## File Changes

| File | Changes |
|------|---------|
| `crates/tenex-tui/Cargo.toml` | Add `ratatui-image`, `image` dependencies |
| `crates/tenex-tui/src/ui/avatars.rs` | New module: cache, fetcher, renderer |
| `crates/tenex-tui/src/ui/mod.rs` | Export avatars module |
| `crates/tenex-tui/src/ui/views/home.rs` | Integrate avatar rendering in `render_conversation_card` |
| `crates/tenex-tui/src/app.rs` | Add `AvatarCache` to `App` struct |
| `crates/tenex-tui/src/data_store.rs` | Trigger avatar fetch when profile with picture URL is received |

## Background Fetch Flow

1. Profile received with `picture` URL
2. Check memory cache → if present, done
3. Check disk cache → if present, load to memory, done
4. Add pubkey to `pending` set
5. Spawn async task:
   - Download image from URL
   - Decode and resize to 32x32
   - Save to disk cache
   - Load into memory cache
   - Remove from `pending`
   - Trigger UI refresh
