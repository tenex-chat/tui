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
    /// Undo stack: (text, cursor) snapshots
    undo_stack: Vec<(String, usize)>,
    /// Redo stack: (text, cursor) snapshots
    redo_stack: Vec<(String, usize)>,
    /// Selection anchor (start of selection, cursor is end)
    pub selection_anchor: Option<usize>,
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
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            selection_anchor: None,
        }
    }

    /// Push current state to undo stack (call before any mutation)
    fn push_undo_state(&mut self) {
        self.undo_stack.push((self.text.clone(), self.cursor));
        // New edit invalidates redo
        self.redo_stack.clear();
        // Limit undo stack size
        if self.undo_stack.len() > 100 {
            self.undo_stack.remove(0);
        }
    }

    /// Undo last change
    pub fn undo(&mut self) {
        if let Some((text, cursor)) = self.undo_stack.pop() {
            self.redo_stack.push((self.text.clone(), self.cursor));
            self.text = text;
            self.cursor = cursor;
        }
    }

    /// Redo last undone change
    pub fn redo(&mut self) {
        if let Some((text, cursor)) = self.redo_stack.pop() {
            self.undo_stack.push((self.text.clone(), self.cursor));
            self.text = text;
            self.cursor = cursor;
        }
    }

    /// Check if there's an active selection
    pub fn has_selection(&self) -> bool {
        self.selection_anchor.is_some()
    }

    /// Get selection range as (start, end) byte offsets
    pub fn selection_range(&self) -> Option<(usize, usize)> {
        self.selection_anchor.map(|anchor| {
            (anchor.min(self.cursor), anchor.max(self.cursor))
        })
    }

    /// Get selected text
    pub fn selected_text(&self) -> Option<String> {
        self.selection_range().map(|(start, end)| self.text[start..end].to_string())
    }

    /// Delete the selected text
    pub fn delete_selection(&mut self) {
        if let Some((start, end)) = self.selection_range() {
            if start < end {
                self.push_undo_state();
                self.text.drain(start..end);
                self.cursor = start;
                self.selection_anchor = None;
            }
        }
    }

    /// Select all text
    pub fn select_all(&mut self) {
        if !self.text.is_empty() {
            self.selection_anchor = Some(0);
            self.cursor = self.text.len();
        }
    }

    /// Clear selection without modifying text
    pub fn clear_selection(&mut self) {
        self.selection_anchor = None;
    }

    /// Move left extending selection (Shift+Left)
    pub fn move_left_extend_selection(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.move_left();
    }

    /// Move right extending selection (Shift+Right)
    pub fn move_right_extend_selection(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.move_right();
    }

    /// Move word left extending selection (Shift+Alt+Left)
    pub fn move_word_left_extend_selection(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.move_word_left();
    }

    /// Move word right extending selection (Shift+Alt+Right)
    pub fn move_word_right_extend_selection(&mut self) {
        if self.selection_anchor.is_none() {
            self.selection_anchor = Some(self.cursor);
        }
        self.move_word_right();
    }

    /// Check if text should become an attachment (>5 lines or >500 chars)
    fn should_be_attachment(text: &str) -> bool {
        let line_count = text.lines().count();
        let char_count = text.len();
        line_count > 5 || char_count > 500
    }

    /// Handle pasted text - may become attachment if large (replaces selection if any)
    /// Uses smart paste detection for JSON and code
    pub fn handle_paste(&mut self, text: &str) {
        self.push_undo_state();
        // Delete selection first if any
        if let Some((start, end)) = self.selection_range() {
            if start < end {
                self.text.drain(start..end);
                self.cursor = start;
            }
        }
        self.selection_anchor = None;
        if Self::should_be_attachment(text) {
            let attachment = PasteAttachment {
                id: self.next_attachment_id,
                content: text.to_string(),
            };
            self.next_attachment_id += 1;
            self.attachments.push(attachment);
        } else {
            // Apply smart paste detection for code/JSON
            let formatted = self.smart_format_paste(text);
            // Insert at cursor position
            self.text.insert_str(self.cursor, &formatted);
            self.cursor += formatted.len();
        }
    }

    /// Detect content type and wrap in appropriate markdown code block
    fn smart_format_paste(&self, text: &str) -> String {
        let trimmed = text.trim();

        // Skip if already in a code block
        if trimmed.starts_with("```") {
            return text.to_string();
        }

        // Skip short single-line text (likely just a word or phrase)
        if !trimmed.contains('\n') && trimmed.len() < 50 {
            return text.to_string();
        }

        // Detect JSON
        if Self::looks_like_json(trimmed) {
            return format!("```json\n{}\n```", trimmed);
        }

        // Detect various code patterns
        if let Some(lang) = Self::detect_code_language(trimmed) {
            return format!("```{}\n{}\n```", lang, trimmed);
        }

        text.to_string()
    }

    /// Check if text looks like JSON
    fn looks_like_json(text: &str) -> bool {
        let trimmed = text.trim();
        // Must start with { or [ and end with } or ]
        (trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
    }

    /// Detect programming language from code patterns
    fn detect_code_language(text: &str) -> Option<&'static str> {
        // Rust
        if text.contains("fn ") && text.contains("->")
            || text.contains("impl ")
            || text.contains("pub struct ")
            || text.contains("use std::")
            || text.contains("#[derive(")
        {
            return Some("rust");
        }

        // TypeScript/JavaScript
        if text.contains("import ") && text.contains(" from ")
            || text.contains("export ")
            || text.contains("const ") && text.contains(" = ")
            || text.contains("function ")
            || text.contains("=> {")
        {
            // Distinguish TypeScript from JavaScript
            if text.contains(": string")
                || text.contains(": number")
                || text.contains(": boolean")
                || text.contains("interface ")
                || text.contains("<T>")
            {
                return Some("typescript");
            }
            return Some("javascript");
        }

        // Python
        if text.contains("def ") && text.contains(":")
            || text.contains("import ")
                && !text.contains(" from \"")
                && !text.contains(" from '")
            || text.contains("class ") && text.contains(":")
            || text.contains("if __name__")
        {
            return Some("python");
        }

        // Go
        if text.contains("func ") && text.contains("package ")
            || text.contains("type ") && text.contains(" struct {")
        {
            return Some("go");
        }

        // Shell/Bash
        if text.starts_with("#!/bin/")
            || text.starts_with("$ ")
            || (text.contains("echo ") && text.contains("&&"))
        {
            return Some("bash");
        }

        // HTML
        if text.contains("<!DOCTYPE") || text.contains("<html") || text.contains("<div") {
            return Some("html");
        }

        // CSS
        if text.contains("{") && (text.contains("color:") || text.contains("display:")) {
            return Some("css");
        }

        // SQL
        if text.to_uppercase().contains("SELECT ")
            && (text.to_uppercase().contains(" FROM ")
                || text.to_uppercase().contains(" WHERE "))
        {
            return Some("sql");
        }

        None
    }

    /// Insert a single character at cursor (replaces selection if any)
    pub fn insert_char(&mut self, c: char) {
        self.push_undo_state();
        // Delete selection first if any
        if let Some((start, end)) = self.selection_range() {
            if start < end {
                self.text.drain(start..end);
                self.cursor = start;
            }
        }
        self.selection_anchor = None;

        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
    }

    /// Insert a newline at cursor
    pub fn insert_newline(&mut self) {
        self.insert_char('\n');
    }

    /// Delete character before cursor (backspace) - deletes selection if any
    pub fn delete_char_before(&mut self) {
        // If there's a selection, delete it instead
        if let Some((start, end)) = self.selection_range() {
            if start < end {
                self.push_undo_state();
                self.text.drain(start..end);
                self.cursor = start;
                self.selection_anchor = None;
                return;
            }
        }
        self.selection_anchor = None;
        if self.cursor > 0 {
            self.push_undo_state();
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

    /// Delete character at cursor (delete key) - deletes selection if any
    pub fn delete_char_at(&mut self) {
        // If there's a selection, delete it instead
        if let Some((start, end)) = self.selection_range() {
            if start < end {
                self.push_undo_state();
                self.text.drain(start..end);
                self.cursor = start;
                self.selection_anchor = None;
                return;
            }
        }
        self.selection_anchor = None;
        if self.cursor < self.text.len() {
            self.push_undo_state();
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
        if self.cursor < end {
            self.push_undo_state();
            self.text.drain(self.cursor..end);
        }
    }

    /// Kill from cursor to beginning of line (Ctrl+U)
    pub fn kill_to_line_start(&mut self) {
        let start = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        if start < self.cursor {
            self.push_undo_state();
            self.text.drain(start..self.cursor);
            self.cursor = start;
        }
    }

    /// Delete word backward (Ctrl+W / Alt+Backspace)
    pub fn delete_word_backward(&mut self) {
        if self.cursor == 0 {
            return;
        }

        let before = &self.text[..self.cursor];

        // Skip whitespace first
        let trimmed = before.trim_end();
        if trimmed.is_empty() {
            // Only whitespace before cursor, delete it all
            self.push_undo_state();
            self.text.drain(0..self.cursor);
            self.cursor = 0;
            return;
        }

        // Find start of the word
        let word_start = trimmed
            .rfind(|c: char| c.is_whitespace())
            .map(|i| i + 1)
            .unwrap_or(0);

        self.push_undo_state();
        self.text.drain(word_start..self.cursor);
        self.cursor = word_start;
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

    /// Move cursor up one line (preserving column position where possible)
    pub fn move_up(&mut self) {
        self.clear_selection();
        let (row, col) = self.cursor_position();
        if row == 0 {
            // Already at first line, move to start
            self.cursor = 0;
            return;
        }

        // Find the start of the previous line
        let lines: Vec<&str> = self.text.split('\n').collect();
        let prev_line = lines[row - 1];
        let prev_line_col = col.min(prev_line.len());

        // Calculate byte offset to that position
        let mut offset = 0;
        for (i, line) in lines.iter().enumerate() {
            if i == row - 1 {
                offset += prev_line_col;
                break;
            }
            offset += line.len() + 1; // +1 for newline
        }

        self.cursor = offset;
    }

    /// Move cursor down one line (preserving column position where possible)
    pub fn move_down(&mut self) {
        self.clear_selection();
        let (row, col) = self.cursor_position();
        let lines: Vec<&str> = self.text.split('\n').collect();

        if row >= lines.len().saturating_sub(1) {
            // Already at last line, move to end
            self.cursor = self.text.len();
            return;
        }

        // Find the start of the next line
        let next_line = lines[row + 1];
        let next_line_col = col.min(next_line.len());

        // Calculate byte offset to that position
        let mut offset = 0;
        for (i, line) in lines.iter().enumerate() {
            if i == row + 1 {
                offset += next_line_col;
                break;
            }
            offset += line.len() + 1; // +1 for newline
        }

        self.cursor = offset;
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

    /// Get visual cursor position accounting for line wrapping at given width
    pub fn visual_cursor_position(&self, wrap_width: usize) -> (usize, usize) {
        if wrap_width == 0 {
            return self.cursor_position();
        }
        let before_cursor = &self.text[..self.cursor];
        let last_line_start = before_cursor.rfind('\n').map(|i| i + 1).unwrap_or(0);
        let col_in_last_line = self.cursor - last_line_start;

        // Count visual rows from logical lines + wrapping within lines
        let mut visual_row = 0;
        for (i, line) in self.text.split('\n').enumerate() {
            let line_start = if i == 0 {
                0
            } else {
                self.text[..self.cursor]
                    .match_indices('\n')
                    .nth(i - 1)
                    .map(|(idx, _)| idx + 1)
                    .unwrap_or(0)
            };

            // Check if cursor is on this logical line
            if self.cursor >= line_start && self.cursor <= line_start + line.len() {
                // Cursor is on this line - add wrapped rows before cursor position
                visual_row += col_in_last_line / wrap_width;
                break;
            } else {
                // Add all visual rows from this logical line
                visual_row += if line.is_empty() {
                    1
                } else {
                    (line.len() + wrap_width - 1) / wrap_width
                };
            }
        }

        let visual_col = col_in_last_line % wrap_width;
        (visual_row, visual_col)
    }

    /// Move to beginning of visual line (accounting for wrap width)
    pub fn move_to_visual_line_start(&mut self, wrap_width: usize) {
        if wrap_width == 0 {
            self.move_to_line_start();
            return;
        }

        // Find start of current logical line
        let logical_line_start = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let col_in_line = self.cursor - logical_line_start;

        // Which visual line within this logical line are we on?
        let visual_line_in_logical = col_in_line / wrap_width;

        // Go to start of that visual line
        self.cursor = logical_line_start + (visual_line_in_logical * wrap_width);
    }

    /// Move to end of visual line (accounting for wrap width)
    pub fn move_to_visual_line_end(&mut self, wrap_width: usize) {
        if wrap_width == 0 {
            self.move_to_line_end();
            return;
        }

        // Find start and end of current logical line
        let logical_line_start = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let logical_line_end = self.text[self.cursor..]
            .find('\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.text.len());
        let logical_line_len = logical_line_end - logical_line_start;

        let col_in_line = self.cursor - logical_line_start;

        // Which visual line within this logical line are we on?
        let visual_line_in_logical = col_in_line / wrap_width;

        // Calculate end of this visual line
        let visual_line_end = ((visual_line_in_logical + 1) * wrap_width).min(logical_line_len);

        self.cursor = logical_line_start + visual_line_end;
    }

    /// Move up one visual line (accounting for wrap width)
    pub fn move_up_visual(&mut self, wrap_width: usize) {
        self.clear_selection();
        if wrap_width == 0 {
            self.move_up();
            return;
        }

        // Find current logical line
        let logical_line_start = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let col_in_line = self.cursor - logical_line_start;
        let visual_line_in_logical = col_in_line / wrap_width;
        let col_in_visual_line = col_in_line % wrap_width;

        if visual_line_in_logical > 0 {
            // Move up within the same logical line
            let target_col = ((visual_line_in_logical - 1) * wrap_width) + col_in_visual_line;
            self.cursor = logical_line_start + target_col;
        } else {
            // Need to move to previous logical line
            if logical_line_start == 0 {
                // Already at first line, move to start
                self.cursor = 0;
                return;
            }

            // Find previous logical line
            let prev_line_end = logical_line_start - 1; // The '\n' character
            let prev_line_start = self.text[..prev_line_end]
                .rfind('\n')
                .map(|i| i + 1)
                .unwrap_or(0);
            let prev_line_len = prev_line_end - prev_line_start;

            // How many visual lines does prev line have?
            let prev_visual_lines = if prev_line_len == 0 {
                1
            } else {
                (prev_line_len + wrap_width - 1) / wrap_width
            };

            // Go to last visual line of prev logical line, same column
            let last_visual_line_start = (prev_visual_lines - 1) * wrap_width;
            let target_col = (last_visual_line_start + col_in_visual_line).min(prev_line_len);
            self.cursor = prev_line_start + target_col;
        }
    }

    /// Move down one visual line (accounting for wrap width)
    pub fn move_down_visual(&mut self, wrap_width: usize) {
        self.clear_selection();
        if wrap_width == 0 {
            self.move_down();
            return;
        }

        // Find current logical line
        let logical_line_start = self.text[..self.cursor]
            .rfind('\n')
            .map(|i| i + 1)
            .unwrap_or(0);
        let logical_line_end = self.text[self.cursor..]
            .find('\n')
            .map(|i| self.cursor + i)
            .unwrap_or(self.text.len());
        let logical_line_len = logical_line_end - logical_line_start;

        let col_in_line = self.cursor - logical_line_start;
        let visual_line_in_logical = col_in_line / wrap_width;
        let col_in_visual_line = col_in_line % wrap_width;

        // How many visual lines does current logical line have?
        let current_visual_lines = if logical_line_len == 0 {
            1
        } else {
            (logical_line_len + wrap_width - 1) / wrap_width
        };

        if visual_line_in_logical < current_visual_lines - 1 {
            // Move down within the same logical line
            let target_col = ((visual_line_in_logical + 1) * wrap_width) + col_in_visual_line;
            self.cursor = logical_line_start + target_col.min(logical_line_len);
        } else {
            // Need to move to next logical line
            if logical_line_end >= self.text.len() {
                // Already at last line, move to end
                self.cursor = self.text.len();
                return;
            }

            // Find next logical line
            let next_line_start = logical_line_end + 1; // After the '\n'
            let next_line_end = self.text[next_line_start..]
                .find('\n')
                .map(|i| next_line_start + i)
                .unwrap_or(self.text.len());
            let next_line_len = next_line_end - next_line_start;

            // Go to first visual line of next logical line, same column
            let target_col = col_in_visual_line.min(next_line_len);
            self.cursor = next_line_start + target_col;
        }
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
