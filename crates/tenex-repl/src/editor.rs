pub(crate) enum AttachmentKind {
    Text { content: String },
    Image { url: String },
}

pub(crate) struct Attachment {
    pub(crate) id: usize,
    pub(crate) kind: AttachmentKind,
}

pub(crate) struct LineEditor {
    pub(crate) buffer: String,
    pub(crate) cursor: usize,
    pub(crate) history: Vec<String>,
    history_index: Option<usize>,
    saved_buffer: String,
    pub(crate) attachments: Vec<Attachment>,
    next_id: usize,
    pub(crate) selected_attachment: Option<usize>,
}

impl LineEditor {
    pub(crate) fn new() -> Self {
        Self {
            buffer: String::new(),
            cursor: 0,
            history: Vec::new(),
            history_index: None,
            saved_buffer: String::new(),
            attachments: Vec::new(),
            next_id: 1,
            selected_attachment: None,
        }
    }

    pub(crate) fn insert_char(&mut self, c: char) {
        self.buffer.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.history_index = None;
    }

    pub(crate) fn delete_back(&mut self) {
        if self.cursor > 0 {
            let prev = self.prev_char_boundary();
            self.buffer.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    pub(crate) fn delete_forward(&mut self) {
        if self.cursor < self.buffer.len() {
            let next = self.next_char_boundary();
            self.buffer.drain(self.cursor..next);
        }
    }

    pub(crate) fn kill_to_end(&mut self) {
        self.buffer.truncate(self.cursor);
    }

    pub(crate) fn delete_word_forward(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut pos = self.cursor;
        while pos < self.buffer.len() && self.buffer.as_bytes()[pos] == b' ' {
            pos += 1;
        }
        while pos < self.buffer.len() && self.buffer.as_bytes()[pos] != b' ' {
            pos += 1;
        }
        self.buffer.drain(self.cursor..pos);
    }

    pub(crate) fn move_word_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut pos = self.cursor;
        while pos > 0 && self.buffer.as_bytes()[pos - 1] == b' ' {
            pos -= 1;
        }
        while pos > 0 && self.buffer.as_bytes()[pos - 1] != b' ' {
            pos -= 1;
        }
        self.cursor = pos;
    }

    pub(crate) fn move_word_right(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut pos = self.cursor;
        while pos < self.buffer.len() && self.buffer.as_bytes()[pos] != b' ' {
            pos += 1;
        }
        while pos < self.buffer.len() && self.buffer.as_bytes()[pos] == b' ' {
            pos += 1;
        }
        self.cursor = pos;
    }

    pub(crate) fn delete_word_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut pos = self.cursor;
        while pos > 0 && self.buffer.as_bytes()[pos - 1] == b' ' {
            pos -= 1;
        }
        while pos > 0 && self.buffer.as_bytes()[pos - 1] != b' ' {
            pos -= 1;
        }
        self.buffer.drain(pos..self.cursor);
        self.cursor = pos;
    }

    pub(crate) fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_char_boundary();
        }
    }

    pub(crate) fn move_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor = self.next_char_boundary();
        }
    }

    pub(crate) fn move_home(&mut self) {
        self.cursor = 0;
    }

    pub(crate) fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    pub(crate) fn insert_text(&mut self, s: &str) {
        self.buffer.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.history_index = None;
    }

    fn should_be_attachment(text: &str) -> bool {
        let line_count = text.lines().count();
        let char_count = text.len();
        line_count > 5 || char_count > 500
    }

    pub(crate) fn handle_paste(&mut self, text: &str) {
        if Self::should_be_attachment(text) {
            let id = self.next_id;
            self.attachments.push(Attachment {
                id,
                kind: AttachmentKind::Text { content: text.to_string() },
            });
            self.next_id += 1;
            let marker = format!("[Text Attachment {}]", id);
            self.insert_text(&marker);
        } else {
            let formatted = Self::smart_format_paste(text);
            self.insert_text(&formatted);
        }
    }

    fn smart_format_paste(text: &str) -> String {
        let trimmed = text.trim();

        if trimmed.starts_with("```") {
            return text.to_string();
        }

        if !trimmed.contains('\n') && trimmed.len() < 50 {
            return text.to_string();
        }

        if (trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
        {
            return format!("```json\n{}\n```", trimmed);
        }

        if let Some(lang) = Self::detect_code_language(trimmed) {
            return format!("```{}\n{}\n```", lang, trimmed);
        }

        text.to_string()
    }

    fn detect_code_language(text: &str) -> Option<&'static str> {
        if text.contains("fn ") && text.contains("->")
            || text.contains("impl ")
            || text.contains("pub struct ")
            || text.contains("use std::")
            || text.contains("#[derive(")
        {
            return Some("rust");
        }
        if text.contains("import ") && text.contains(" from ")
            || text.contains("export ")
            || text.contains("const ") && text.contains(" = ")
            || text.contains("function ")
            || text.contains("=> {")
        {
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
        if text.contains("def ") && text.contains(":")
            || text.contains("class ") && text.contains(":")
            || text.contains("if __name__")
        {
            return Some("python");
        }
        if text.contains("func ") && text.contains("package ")
            || text.contains("type ") && text.contains(" struct {")
        {
            return Some("go");
        }
        if text.starts_with("#!/bin/")
            || text.starts_with("$ ")
            || (text.contains("echo ") && text.contains("&&"))
        {
            return Some("bash");
        }
        if text.contains("<!DOCTYPE") || text.contains("<html") || text.contains("<div") {
            return Some("html");
        }
        if text.to_uppercase().contains("SELECT ")
            && (text.to_uppercase().contains(" FROM ")
                || text.to_uppercase().contains(" WHERE "))
        {
            return Some("sql");
        }
        None
    }

    pub(crate) fn add_image_attachment(&mut self, url: String) -> usize {
        let id = self.next_id;
        self.attachments.push(Attachment {
            id,
            kind: AttachmentKind::Image { url },
        });
        self.next_id += 1;
        let marker = format!("[Image #{}]", id);
        self.insert_text(&marker);
        id
    }

    pub(crate) fn build_full_content(&self) -> String {
        let mut content = self.buffer.clone();

        for att in &self.attachments {
            if let AttachmentKind::Image { ref url } = att.kind {
                let marker = format!("[Image #{}]", att.id);
                content = content.replace(&marker, url);
            }
        }

        let text_attachments: Vec<&Attachment> = self.attachments.iter()
            .filter(|a| matches!(a.kind, AttachmentKind::Text { .. }))
            .collect();
        if !text_attachments.is_empty() {
            if !content.is_empty() && !content.ends_with('\n') {
                content.push('\n');
            }
            content.push_str("\n----\n");
            for att in &text_attachments {
                if let AttachmentKind::Text { content: ref text } = att.kind {
                    content.push_str(&format!("-- Text Attachment {} --\n", att.id));
                    content.push_str(text);
                    if !text.ends_with('\n') {
                        content.push('\n');
                    }
                }
            }
        }

        content
    }

    pub(crate) fn has_attachments(&self) -> bool {
        !self.attachments.is_empty()
    }

    pub(crate) fn submit(&mut self) -> String {
        let content = if self.has_attachments() {
            self.build_full_content()
        } else {
            self.buffer.clone()
        };
        if !self.buffer.trim().is_empty() {
            self.history.push(self.buffer.clone());
        }
        self.buffer.clear();
        self.cursor = 0;
        self.history_index = None;
        self.attachments.clear();
        self.next_id = 1;
        self.selected_attachment = None;
        content
    }

    pub(crate) fn history_up(&mut self) {
        if self.history.is_empty() {
            return;
        }
        match self.history_index {
            None => {
                self.saved_buffer = self.buffer.clone();
                self.history_index = Some(self.history.len() - 1);
            }
            Some(0) => return,
            Some(idx) => {
                self.history_index = Some(idx - 1);
            }
        }
        let idx = self.history_index.unwrap();
        self.buffer = self.history[idx].clone();
        self.cursor = self.buffer.len();
    }

    pub(crate) fn history_down(&mut self) {
        match self.history_index {
            None => (),
            Some(idx) => {
                if idx + 1 >= self.history.len() {
                    self.history_index = None;
                    self.buffer = self.saved_buffer.clone();
                    self.cursor = self.buffer.len();
                    return;
                }
                self.history_index = Some(idx + 1);
                self.buffer = self.history[idx + 1].clone();
                self.cursor = self.buffer.len();
            }
        }
    }

    pub(crate) fn set_buffer(&mut self, s: &str) {
        self.buffer = s.to_string();
        self.cursor = self.buffer.len();
        self.history_index = None;
    }

    fn prev_char_boundary(&self) -> usize {
        let mut pos = self.cursor - 1;
        while !self.buffer.is_char_boundary(pos) {
            pos -= 1;
        }
        pos
    }

    fn next_char_boundary(&self) -> usize {
        let mut pos = self.cursor + 1;
        while pos < self.buffer.len() && !self.buffer.is_char_boundary(pos) {
            pos += 1;
        }
        pos
    }

    pub(crate) fn marker_for_attachment(att: &Attachment) -> String {
        match &att.kind {
            AttachmentKind::Text { .. } => format!("[Text Attachment {}]", att.id),
            AttachmentKind::Image { .. } => format!("[Image #{}]", att.id),
        }
    }

    pub(crate) fn marker_before_cursor(&self) -> Option<usize> {
        if self.cursor == 0 {
            return None;
        }
        let bytes = self.buffer.as_bytes();
        if bytes[self.cursor - 1] != b']' {
            return None;
        }
        let before = &self.buffer[..self.cursor];
        let bracket_start = before.rfind('[')?;
        let marker_text = &self.buffer[bracket_start..self.cursor];
        for (i, att) in self.attachments.iter().enumerate() {
            if Self::marker_for_attachment(att) == marker_text {
                return Some(i);
            }
        }
        None
    }

    pub(crate) fn remove_attachment(&mut self, idx: usize) {
        if idx >= self.attachments.len() {
            return;
        }
        let marker = Self::marker_for_attachment(&self.attachments[idx]);
        self.attachments.remove(idx);
        if let Some(pos) = self.buffer.find(&marker) {
            self.buffer.drain(pos..pos + marker.len());
            if self.cursor > pos {
                self.cursor = self.cursor.saturating_sub(marker.len()).max(pos);
            }
        }
        if self.attachments.is_empty() {
            self.selected_attachment = None;
        } else if let Some(sel) = self.selected_attachment {
            if sel >= self.attachments.len() {
                self.selected_attachment = Some(self.attachments.len() - 1);
            }
        }
    }
}
