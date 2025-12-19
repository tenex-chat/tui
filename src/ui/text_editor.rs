/// Rich text editor with multiline support and paste attachment handling
///
/// Features:
/// - Multiline input with dynamic height
/// - Ctrl+A: Move to beginning of line
/// - Ctrl+E: Move to end of line
/// - Ctrl+K: Kill from cursor to end of line
/// - Alt+Left/Right: Word jumping
/// - Large pastes become attachments

/// Represents a pasted attachment (large text that was pasted)
#[derive(Debug, Clone)]
pub struct PasteAttachment {
    pub id: usize,
    pub content: String,
}

impl PasteAttachment {
    pub fn line_count(&self) -> usize {
        self.content.lines().count().max(1)
    }

    pub fn char_count(&self) -> usize {
        self.content.len()
    }
}

/// Represents an uploaded image attachment
#[derive(Debug, Clone)]
pub struct ImageAttachment {
    pub id: usize,
    pub url: String,
}

/// Text editor state for rich editing
#[derive(Debug, Clone)]
pub struct TextEditor {
    /// The actual text content (can be multiline)
    pub text: String,
    /// Cursor position as byte offset
    pub cursor: usize,
    /// Attachments from large pastes
    pub attachments: Vec<PasteAttachment>,
    /// Next paste attachment ID
    next_attachment_id: usize,
    /// Image attachments (uploaded images)
    pub image_attachments: Vec<ImageAttachment>,
    /// Next image attachment ID
    next_image_id: usize,
    /// Currently focused attachment index (None = main input focused)
    pub focused_attachment: Option<usize>,
}

impl Default for TextEditor {
    fn default() -> Self {
        Self::new()
    }
}

impl TextEditor {
    pub fn new() -> Self {
        Self {
            text: String::new(),
            cursor: 0,
            attachments: Vec::new(),
            next_attachment_id: 1,
            image_attachments: Vec::new(),
            next_image_id: 1,
            focused_attachment: None,
        }
    }

    /// Check if text should become an attachment (>5 lines or >500 chars)
    fn should_be_attachment(text: &str) -> bool {
        let line_count = text.lines().count();
        let char_count = text.len();
        line_count > 5 || char_count > 500
    }

    /// Handle pasted text - may become attachment if large
    pub fn handle_paste(&mut self, text: &str) {
        if Self::should_be_attachment(text) {
            let attachment = PasteAttachment {
                id: self.next_attachment_id,
                content: text.to_string(),
            };
            self.next_attachment_id += 1;
            self.attachments.push(attachment);
        } else {
            // Insert at cursor position
            self.text.insert_str(self.cursor, text);
            self.cursor += text.len();
        }
    }

    /// Insert a single character at cursor
    pub fn insert_char(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert a newline at cursor
    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Delete character before cursor (backspace)
    pub fn delete_char_before(&mut self) {
        if self.cursor > 0 {
            // Find the previous character boundary
            let prev_boundary = self.text[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
            self.text.remove(prev_boundary);
            self.cursor = prev_boundary;
        }
    }

    /// Delete character at cursor (delete key)
    pub fn delete_char_at(&mut self) {
        if self.cursor < self.text.len() {
            self.text.remove(self.cursor);
        }
    }

    /// Move cursor left by one character
    pub fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.text[..self.cursor]
                .char_indices()
                .last()
                .map(|(i, _)| i)
                .unwrap_or(0);
        }
    }

    /// Move cursor right by one character
    pub fn move_right(&mut self) {
        if self.cursor < self.text.len() {
            self.cursor = self.text[self.cursor..]
                .char_indices()
                .nth(1)
                .map(|(i, _)| self.cursor + i)
                .unwrap_or(self.text.len());
        }
    }

    /// Move cursor to beginning of current line (Ctrl+A)
    pub fn move_to_line_start(&mut self) {
        // Find the previous newline or start of string
        self.cursor = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
    }

    /// Move cursor to end of current line (Ctrl+E)
    pub fn move_to_line_end(&mut self) {
        // Find the next newline or end of string
        self.cursor = self.text[self.cursor..]
            .find('\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.text.len());
    }

    /// Kill from cursor to end of line (Ctrl+K)
    pub fn kill_to_line_end(&mut self) {
        let end = self.text[self.cursor..]
            .find('\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.text.len());
        self.text.drain(self.cursor..end);
    }

    /// Move cursor to previous word boundary (Alt+Left)
    pub fn move_word_left(&mut self) {
        if self.cursor == 0 {
            return;
        }

        // Skip any whitespace before cursor
        let before = &self.text[..self.cursor];
        let trimmed_end = before.trim_end();
        if trimmed_end.is_empty() {
            self.cursor = 0;
            return;
        }

        // Find the start of the word
        let word_end = trimmed_end.len();
        let word_start = trimmed_end
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);

        self.cursor = word_start.min(word_end);
    }

    /// Move cursor to next word boundary (Alt+Right)
    pub fn move_word_right(&mut self) {
        if self.cursor >= self.text.len() {
            return;
        }

        let after = &self.text[self.cursor..];

        // Skip current word (non-whitespace)
        let word_end = after
            .find(|c: char| c.is_whitespace())
            .unwrap_or(after.len());

        // Skip whitespace after word
        let next_word = after[word_end..]
            .find(|c: char| !c.is_whitespace())
            .map(|i| word_end + i)
            .unwrap_or(after.len());

        self.cursor += next_word;
    }

    /// Clear all content
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
        self.attachments.clear();
        self.image_attachments.clear();
        self.focused_attachment = None;
    }

    /// Add an image attachment and return its ID
    pub fn add_image_attachment(&mut self, url: String) -> usize {
        let id = self.next_image_id;
        self.next_image_id += 1;
        self.image_attachments.push(ImageAttachment { id, url });
        id
    }

    /// Get the number of lines in the input
    pub fn line_count(&self) -> usize {
        self.text.lines().count().max(1)
    }

    /// Get cursor position as (row, col) for rendering
    pub fn cursor_position(&self) -> (usize, usize) {
        let before_cursor = &self.text[..self.cursor];
        let row = before_cursor.matches('\n').count();
        let col = before_cursor
            .rfind('\n')
            .map(|i| self.cursor - i - 1)
            .unwrap_or(self.cursor);
        (row, col)
    }

    /// Total number of attachments (images + pastes)
    pub fn total_attachments(&self) -> usize {
        self.image_attachments.len() + self.attachments.len()
    }

    /// Check if any attachments exist
    pub fn has_attachments(&self) -> bool {
        !self.image_attachments.is_empty() || !self.attachments.is_empty()
    }

    /// Focus the first attachment (called on Up arrow)
    pub fn focus_attachments(&mut self) {
        if self.has_attachments() {
            self.focused_attachment = Some(0);
        }
    }

    /// Unfocus attachments (return to text input)
    pub fn unfocus_attachments(&mut self) {
        self.focused_attachment = None;
    }

    /// Cycle focus: main input -> attachments -> back to main input
    /// Index spans: 0..image_attachments.len() for images, then paste attachments
    pub fn cycle_focus(&mut self) {
        let total = self.total_attachments();
        if total == 0 {
            return;
        }

        match self.focused_attachment {
            None => {
                self.focused_attachment = Some(0);
            }
            Some(idx) => {
                if idx + 1 < total {
                    self.focused_attachment = Some(idx + 1);
                } else {
                    self.focused_attachment = None;
                }
            }
        }
    }

    /// Check if focused attachment is an image (vs paste)
    #[allow(dead_code)]
    pub fn is_focused_image(&self) -> bool {
        self.focused_attachment
            .map(|idx| idx < self.image_attachments.len())
            .unwrap_or(false)
    }

    /// Get focused paste attachment for editing (only paste attachments are editable)
    pub fn get_focused_attachment(&self) -> Option<&PasteAttachment> {
        self.focused_attachment.and_then(|idx| {
            let img_count = self.image_attachments.len();
            if idx >= img_count {
                self.attachments.get(idx - img_count)
            } else {
                None // Image attachment, not editable
            }
        })
    }

    /// Get focused image attachment
    #[allow(dead_code)]
    pub fn get_focused_image(&self) -> Option<&ImageAttachment> {
        self.focused_attachment.and_then(|idx| {
            if idx < self.image_attachments.len() {
                self.image_attachments.get(idx)
            } else {
                None
            }
        })
    }

    /// Update focused attachment content (only for paste attachments)
    pub fn update_focused_attachment(&mut self, new_content: String) {
        if let Some(idx) = self.focused_attachment {
            let img_count = self.image_attachments.len();
            if idx >= img_count {
                if let Some(attachment) = self.attachments.get_mut(idx - img_count) {
                    attachment.content = new_content;
                }
            }
        }
    }

    /// Delete focused attachment (image or paste)
    pub fn delete_focused_attachment(&mut self) {
        if let Some(idx) = self.focused_attachment {
            let img_count = self.image_attachments.len();
            if idx < img_count {
                // Delete image attachment
                self.image_attachments.remove(idx);
            } else {
                // Delete paste attachment
                let paste_idx = idx - img_count;
                if paste_idx < self.attachments.len() {
                    self.attachments.remove(paste_idx);
                }
            }
            // Adjust focus
            let new_total = self.total_attachments();
            if new_total == 0 {
                self.focused_attachment = None;
            } else if idx >= new_total {
                self.focused_attachment = Some(new_total - 1);
            }
        }
    }

    /// Build the full message content including attachments
    /// Replaces [Image #N] and [Paste #N] markers with actual content
    pub fn build_full_content(&self) -> String {
        let mut content = self.text.clone();

        // Replace [Image #N] markers with actual URLs
        for img in &self.image_attachments {
            let marker = format!("[Image #{}]", img.id);
            content = content.replace(&marker, &img.url);
        }

        // Append paste attachments at the end
        for attachment in &self.attachments {
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str(&attachment.content);
        }

        content
    }

    /// Submit and clear the editor, returning the full content
    pub fn submit(&mut self) -> String {
        let content = self.build_full_content();
        self.clear();
        content
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_basic_editing() {
        let mut editor = TextEditor::new();
        editor.insert_char('h');
        editor.insert_char('e');
        editor.insert_char('l');
        editor.insert_char('l');
        editor.insert_char('o');
        assert_eq!(editor.text, "hello");
        assert_eq!(editor.cursor, 5);
    }

    #[test]
    fn test_multiline() {
        let mut editor = TextEditor::new();
        editor.insert_char('a');
        editor.insert_newline();
        editor.insert_char('b');
        assert_eq!(editor.text, "a\nb");
        assert_eq!(editor.line_count(), 2);
    }

    #[test]
    fn test_paste_becomes_attachment() {
        let mut editor = TextEditor::new();
        let long_text = "line1\nline2\nline3\nline4\nline5\nline6\n";
        editor.handle_paste(long_text);
        assert!(editor.text.is_empty());
        assert_eq!(editor.attachments.len(), 1);
        assert_eq!(editor.attachments[0].content, long_text);
    }

    #[test]
    fn test_small_paste_inline() {
        let mut editor = TextEditor::new();
        editor.handle_paste("hello");
        assert_eq!(editor.text, "hello");
        assert!(editor.attachments.is_empty());
    }

    #[test]
    fn test_word_navigation() {
        let mut editor = TextEditor::new();
        editor.text = "hello world test".to_string();
        editor.cursor = 0;

        editor.move_word_right();
        assert_eq!(editor.cursor, 6); // After "hello "

        editor.move_word_right();
        assert_eq!(editor.cursor, 12); // After "world "

        editor.move_word_left();
        assert_eq!(editor.cursor, 6);
    }

    #[test]
    fn test_line_navigation() {
        let mut editor = TextEditor::new();
        editor.text = "first line\nsecond line".to_string();
        editor.cursor = 15; // Middle of "second"

        editor.move_to_line_start();
        assert_eq!(editor.cursor, 11);

        editor.move_to_line_end();
        assert_eq!(editor.cursor, 22);
    }
}
