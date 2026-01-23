use crate::nostr;
use crate::ui::App;

/// Result of a background image upload
pub(crate) enum UploadResult {
    Success(String), // URL
    Error(String),   // Error message
}

/// Handle clipboard paste - checks for images and uploads to Blossom
pub(crate) fn handle_clipboard_paste(
    app: &mut App,
    keys: &nostr_sdk::Keys,
    upload_tx: tokio::sync::mpsc::Sender<UploadResult>,
) {
    use arboard::Clipboard;

    let mut clipboard = match Clipboard::new() {
        Ok(c) => c,
        Err(_e) => {
            return;
        }
    };

    // Check for image in clipboard
    if let Ok(image) = clipboard.get_image() {
        app.set_status("Uploading image...");

        // Convert to PNG bytes
        let png_data = match image_to_png(&image) {
            Ok(data) => data,
            Err(e) => {
                app.set_status(&format!("Failed to convert image: {}", e));
                return;
            }
        };

        // Spawn background upload task
        let keys = keys.clone();
        tokio::spawn(async move {
            let result = match nostr::upload_image(&png_data, &keys, "image/png").await {
                Ok(url) => UploadResult::Success(url),
                Err(e) => UploadResult::Error(format!("Upload failed: {}", e)),
            };
            let _ = upload_tx.send(result).await;
        });
    } else if let Ok(text) = clipboard.get_text() {
        // Check if clipboard text is a file path to an image
        if !handle_image_file_paste(app, &text, keys, upload_tx) {
            // Fall back to regular text paste
            app.chat_editor_mut().handle_paste(&text);
            app.save_chat_draft();
        }
    }
}

/// Check if text is an image file path and upload it if so
/// Returns true if it was an image file that was handled, false otherwise
pub(crate) fn handle_image_file_paste(
    app: &mut App,
    text: &str,
    keys: &nostr_sdk::Keys,
    upload_tx: tokio::sync::mpsc::Sender<UploadResult>,
) -> bool {
    let path = text.trim();

    // Skip if empty or doesn't look like a file path
    if path.is_empty() {
        return false;
    }

    // Handle file:// URLs (common from some terminals/apps)
    let path = if let Some(file_path) = path.strip_prefix("file://") {
        urlencoded_decode(file_path)
    } else {
        // Handle backslash-escaped spaces (from terminal drag-and-drop)
        path.replace("\\ ", " ")
    };

    // Check if it's a valid path to an image file
    let path_obj = std::path::Path::new(&path);

    // Must have an image extension
    let extension = match path_obj.extension().and_then(|e| e.to_str()) {
        Some(ext) => ext.to_lowercase(),
        None => return false,
    };

    let mime_type = match extension.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        _ => return false,
    };

    // Check if file exists
    if !path_obj.exists() {
        return false;
    }

    // Read the file
    app.set_status("Uploading image...");
    let data = match std::fs::read(&path) {
        Ok(data) => data,
        Err(e) => {
            app.set_status(&format!("Failed to read file: {}", e));
            return true;
        }
    };

    // Spawn background upload task
    let keys = keys.clone();
    let mime_type = mime_type.to_string();
    tokio::spawn(async move {
        let result = match nostr::upload_image(&data, &keys, &mime_type).await {
            Ok(url) => UploadResult::Success(url),
            Err(e) => UploadResult::Error(format!("Upload failed: {}", e)),
        };
        let _ = upload_tx.send(result).await;
    });

    true
}

/// Simple URL decoding for file paths
fn urlencoded_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();

    while let Some(c) = chars.next() {
        if c == '%' {
            // Try to parse the next two characters as hex
            let mut hex = String::with_capacity(2);
            if let Some(&h1) = chars.peek() {
                hex.push(h1);
                chars.next();
            }
            if let Some(&h2) = chars.peek() {
                hex.push(h2);
                chars.next();
            }
            if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                result.push(byte as char);
            } else {
                // Invalid escape, keep original
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }

    result
}

/// Convert arboard ImageData to PNG bytes
fn image_to_png(image: &arboard::ImageData) -> anyhow::Result<Vec<u8>> {
    use std::io::Cursor;

    let width = image.width as u32;
    let height = image.height as u32;

    // arboard gives us RGBA bytes
    let mut png_data = Vec::new();
    {
        let mut encoder = png::Encoder::new(Cursor::new(&mut png_data), width, height);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&image.bytes)?;
    }

    Ok(png_data)
}
