//! Avatar rendering system for thread list
//!
//! Handles fetching, caching, and rendering user avatars in the terminal.
//! Uses ratatui-image with protocol auto-detection for best quality where supported,
//! falling back to halfblocks for universal compatibility.

use image::{DynamicImage, ImageReader, Rgba, RgbaImage};
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::Frame;
use ratatui_image::picker::Picker;
use ratatui_image::protocol::StatefulProtocol;
use ratatui_image::{Resize, StatefulImage};
use std::collections::{HashMap, HashSet};
use std::io::Cursor;
use std::path::PathBuf;
use std::sync::mpsc::{channel, Receiver, Sender};
use tokio::sync::mpsc as async_mpsc;

use crate::ui::theme;

/// Result of a background avatar fetch operation
pub enum AvatarFetchResult {
    Success { pubkey: String, image: DynamicImage },
    Failed { pubkey: String },
}

/// Avatar cache managing disk and memory storage
pub struct AvatarCache {
    /// Decoded images ready for rendering (None = no avatar or fetch failed)
    images: HashMap<String, Option<DynamicImage>>,
    /// Protocol states for ratatui-image rendering
    protocols: HashMap<String, StatefulProtocol>,
    /// Pubkeys currently being fetched (prevents duplicate requests)
    pending: HashSet<String>,
    /// Protocol picker for terminal capability detection
    picker: Option<Picker>,
    /// Channel to receive completed fetches
    fetch_rx: Receiver<AvatarFetchResult>,
    /// Sender for spawning fetch tasks
    fetch_tx: Sender<AvatarFetchResult>,
    /// Async sender for spawning fetch tasks from sync context
    async_fetch_tx: Option<async_mpsc::Sender<(String, String)>>,
    /// Cache directory path
    cache_dir: PathBuf,
}

impl AvatarCache {
    pub fn new() -> Self {
        let (fetch_tx, fetch_rx) = channel();

        // Initialize cache directory
        let cache_dir = dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("."))
            .join("tenex")
            .join("avatars");

        // Create cache directory if it doesn't exist
        let _ = std::fs::create_dir_all(&cache_dir);

        // Try to create a picker for protocol detection
        // This may fail in some terminal environments
        let picker = Picker::from_query_stdio().ok();

        Self {
            images: HashMap::new(),
            protocols: HashMap::new(),
            pending: HashSet::new(),
            picker,
            fetch_rx,
            fetch_tx,
            async_fetch_tx: None,
            cache_dir,
        }
    }

    /// Set up async fetch channel for background downloads
    pub fn setup_async_fetcher(&mut self) -> async_mpsc::Receiver<(String, String)> {
        let (tx, rx) = async_mpsc::channel(100);
        self.async_fetch_tx = Some(tx);
        rx
    }

    /// Check for completed avatar fetches and update cache
    pub fn poll_fetches(&mut self) {
        while let Ok(result) = self.fetch_rx.try_recv() {
            match result {
                AvatarFetchResult::Success { pubkey, image } => {
                    // Save to disk cache
                    self.save_to_disk(&pubkey, &image);
                    // Store in memory
                    self.images.insert(pubkey.clone(), Some(image));
                    self.pending.remove(&pubkey);
                }
                AvatarFetchResult::Failed { pubkey } => {
                    // Mark as failed (None) so we don't retry
                    self.images.insert(pubkey.clone(), None);
                    self.pending.remove(&pubkey);
                }
            }
        }
    }

    /// Get a cached image for a pubkey
    pub fn get(&self, pubkey: &str) -> Option<&DynamicImage> {
        self.images.get(pubkey).and_then(|opt| opt.as_ref())
    }

    /// Check if we have an image (or know there isn't one)
    pub fn is_resolved(&self, pubkey: &str) -> bool {
        self.images.contains_key(pubkey)
    }

    /// Check if a fetch is currently pending
    pub fn is_pending(&self, pubkey: &str) -> bool {
        self.pending.contains(pubkey)
    }

    /// Request an avatar fetch for a pubkey with picture URL
    pub fn request_fetch(&mut self, pubkey: &str, url: &str) {
        // Skip if already cached, pending, or resolved as no-avatar
        if self.images.contains_key(pubkey) || self.pending.contains(pubkey) {
            return;
        }

        // Check disk cache first
        if let Some(image) = self.load_from_disk(pubkey) {
            self.images.insert(pubkey.to_string(), Some(image));
            return;
        }

        // Mark as pending and spawn fetch
        self.pending.insert(pubkey.to_string());

        if let Some(ref tx) = self.async_fetch_tx {
            let _ = tx.try_send((pubkey.to_string(), url.to_string()));
        }
    }

    /// Get disk cache path for a pubkey
    fn cache_path(&self, pubkey: &str) -> PathBuf {
        let short_key = &pubkey[..8.min(pubkey.len())];
        self.cache_dir.join(format!("{}.png", short_key))
    }

    /// Load image from disk cache
    fn load_from_disk(&self, pubkey: &str) -> Option<DynamicImage> {
        let path = self.cache_path(pubkey);
        if path.exists() {
            ImageReader::open(&path)
                .ok()
                .and_then(|r| r.decode().ok())
        } else {
            None
        }
    }

    /// Save image to disk cache (resized to 32x32)
    fn save_to_disk(&self, pubkey: &str, image: &DynamicImage) {
        let path = self.cache_path(pubkey);
        let resized = image.resize_exact(32, 32, image::imageops::FilterType::Lanczos3);
        let _ = resized.save(&path);
    }

    /// Render an avatar at the given area
    /// Returns true if an image was rendered, false if fallback was used
    pub fn render_avatar(
        &mut self,
        f: &mut Frame,
        pubkey: &str,
        display_name: &str,
        area: Rect,
    ) -> bool {
        // Check if we have an image
        if let Some(image) = self.get(pubkey).cloned() {
            // Try to render with ratatui-image
            if let Some(ref picker) = self.picker {
                // Get or create protocol state for this pubkey
                let protocol = self.protocols.entry(pubkey.to_string()).or_insert_with(|| {
                    picker.new_resize_protocol(image.clone())
                });

                let stateful_image = StatefulImage::new(None).resize(Resize::Fit(None));
                f.render_stateful_widget(stateful_image, area, protocol);
                return true;
            }
        }

        // Fallback: render colored initials
        render_initials_fallback(f, pubkey, display_name, area);
        false
    }

    /// Get the sync sender for background fetch results
    pub fn fetch_result_sender(&self) -> Sender<AvatarFetchResult> {
        self.fetch_tx.clone()
    }
}

impl Default for AvatarCache {
    fn default() -> Self {
        Self::new()
    }
}

/// Render a colored initials fallback when no avatar is available
pub fn render_initials_fallback(
    f: &mut Frame,
    pubkey: &str,
    display_name: &str,
    area: Rect,
) {
    use ratatui::text::{Line, Span};
    use ratatui::widgets::Paragraph;
    use ratatui::style::{Modifier, Style};

    // Get deterministic color from pubkey
    let color = theme::user_color(pubkey);

    // Get first character of display name (or pubkey if empty)
    let initial = display_name
        .chars()
        .next()
        .or_else(|| pubkey.chars().next())
        .unwrap_or('?')
        .to_uppercase()
        .next()
        .unwrap_or('?');

    // Create filled block with initial
    // Using half-block characters to create a colored square
    let (r, g, b) = color_to_rgb(color);
    let bg_color = Color::Rgb(r, g, b);
    let fg_color = if is_light_color(r, g, b) {
        Color::Black
    } else {
        Color::White
    };

    // Build lines for the avatar area (2 wide x 4 tall)
    let mut lines: Vec<Line> = Vec::new();

    // For a 2x4 cell area, we fill with background color
    // and center the initial vertically
    let half = area.height / 2;

    for row in 0..area.height {
        if row == half {
            // Center row: show the initial
            lines.push(Line::from(vec![
                Span::styled(
                    format!("{} ", initial),
                    Style::default().fg(fg_color).bg(bg_color).add_modifier(Modifier::BOLD),
                ),
            ]));
        } else {
            // Other rows: solid background
            lines.push(Line::from(vec![
                Span::styled(
                    "  ",
                    Style::default().bg(bg_color),
                ),
            ]));
        }
    }

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, area);
}

/// Convert ratatui Color to RGB tuple
fn color_to_rgb(color: Color) -> (u8, u8, u8) {
    match color {
        Color::Rgb(r, g, b) => (r, g, b),
        _ => (128, 128, 128), // Default gray for non-RGB colors
    }
}

/// Check if a color is light (for text contrast)
fn is_light_color(r: u8, g: u8, b: u8) -> bool {
    // Simple luminance calculation
    let luminance = (0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32) / 255.0;
    luminance > 0.5
}

/// Background task to fetch avatar images
pub async fn fetch_avatar(
    url: String,
    result_tx: Sender<AvatarFetchResult>,
    pubkey: String,
) {
    // Fetch the image
    let result = match reqwest::get(&url).await {
        Ok(response) => {
            if response.status().is_success() {
                match response.bytes().await {
                    Ok(bytes) => {
                        // Try to decode the image
                        match ImageReader::new(Cursor::new(bytes))
                            .with_guessed_format()
                            .ok()
                            .and_then(|r| r.decode().ok())
                        {
                            Some(image) => {
                                // Resize to 32x32 for consistency
                                let resized = image.resize_exact(
                                    32,
                                    32,
                                    image::imageops::FilterType::Lanczos3,
                                );
                                AvatarFetchResult::Success {
                                    pubkey,
                                    image: resized,
                                }
                            }
                            None => AvatarFetchResult::Failed { pubkey },
                        }
                    }
                    Err(_) => AvatarFetchResult::Failed { pubkey },
                }
            } else {
                AvatarFetchResult::Failed { pubkey }
            }
        }
        Err(_) => AvatarFetchResult::Failed { pubkey },
    };

    let _ = result_tx.send(result);
}

/// Create a simple colored square image for fallback
#[allow(dead_code)]
pub fn create_colored_square(color: Color, size: u32) -> DynamicImage {
    let (r, g, b) = color_to_rgb(color);
    let mut img = RgbaImage::new(size, size);

    for pixel in img.pixels_mut() {
        *pixel = Rgba([r, g, b, 255]);
    }

    DynamicImage::ImageRgba8(img)
}
