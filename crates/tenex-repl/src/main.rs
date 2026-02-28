use anyhow::Result;
use chrono::Local;
use clap::Parser;
use crossterm::event::{
    Event, EventStream, KeyCode, KeyEvent, KeyModifiers,
};
use crossterm::terminal::{self, ClearType};
use crossterm::{cursor, execute, queue};
use futures::StreamExt;
use nostr_sdk::prelude::*;
use std::collections::HashSet;
use std::io::{self, Stdout, Write};
use std::sync::mpsc::Receiver;
use std::time::Instant;
use tenex_core::config::CoreConfig;
use tenex_core::events::CoreEvent;
use tenex_core::models::{Message, Project, ProjectAgent, Thread};
use tenex_core::nostr::{get_current_pubkey, DataChange, NostrCommand};
use tenex_core::runtime::CoreRuntime;
use tenex_core::store::app_data_store::AppDataStore;

// ANSI color codes
const CYAN: &str = "\x1b[36m";
const GREEN: &str = "\x1b[32m";
const BRIGHT_GREEN: &str = "\x1b[1;32m";
const YELLOW: &str = "\x1b[33m";
const RED: &str = "\x1b[31m";
const WHITE_BOLD: &str = "\x1b[1;37m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";
// Background color for input box
const BG_INPUT: &str = "\x1b[48;5;234m";
const BG_HIGHLIGHT: &str = "\x1b[48;5;239m";

macro_rules! raw_println {
    () => {{
        write!(std::io::stdout(), "\r\n").ok();
        std::io::stdout().flush().ok();
    }};
    ($($arg:tt)*) => {{
        let s = format!($($arg)*);
        let mut out = std::io::stdout();
        for line in s.split('\n') {
            write!(out, "{}\r\n", line).ok();
        }
        out.flush().ok();
    }};
}

#[derive(Parser, Debug)]
#[command(name = "tenex-repl")]
#[command(about = "TENEX Shell-style REPL Chat Client")]
struct Args {
    /// nsec key for authentication (prefer TENEX_NSEC env var)
    #[arg(long)]
    nsec: Option<String>,
}

// ─── Attachment Types ───────────────────────────────────────────────────────

enum AttachmentKind {
    Text { content: String },
    Image { url: String },
}

struct Attachment {
    id: usize,
    kind: AttachmentKind,
}

// ─── Line Editor ────────────────────────────────────────────────────────────

struct LineEditor {
    buffer: String,
    cursor: usize,
    history: Vec<String>,
    history_index: Option<usize>,
    saved_buffer: String,
    attachments: Vec<Attachment>,
    next_id: usize,
    selected_attachment: Option<usize>,
}

impl LineEditor {
    fn new() -> Self {
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

    fn insert_char(&mut self, c: char) {
        self.buffer.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        self.history_index = None;
    }

    fn delete_back(&mut self) {
        if self.cursor > 0 {
            let prev = self.prev_char_boundary();
            self.buffer.drain(prev..self.cursor);
            self.cursor = prev;
        }
    }

    fn delete_forward(&mut self) {
        if self.cursor < self.buffer.len() {
            let next = self.next_char_boundary();
            self.buffer.drain(self.cursor..next);
        }
    }

    fn kill_to_end(&mut self) {
        self.buffer.truncate(self.cursor);
    }

    fn delete_word_forward(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut pos = self.cursor;
        // Skip whitespace
        while pos < self.buffer.len() && self.buffer.as_bytes()[pos] == b' ' {
            pos += 1;
        }
        // Delete until next whitespace or end
        while pos < self.buffer.len() && self.buffer.as_bytes()[pos] != b' ' {
            pos += 1;
        }
        self.buffer.drain(self.cursor..pos);
    }

    fn move_word_left(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut pos = self.cursor;
        // Skip whitespace
        while pos > 0 && self.buffer.as_bytes()[pos - 1] == b' ' {
            pos -= 1;
        }
        // Move past word
        while pos > 0 && self.buffer.as_bytes()[pos - 1] != b' ' {
            pos -= 1;
        }
        self.cursor = pos;
    }

    fn move_word_right(&mut self) {
        if self.cursor >= self.buffer.len() {
            return;
        }
        let mut pos = self.cursor;
        // Skip current word
        while pos < self.buffer.len() && self.buffer.as_bytes()[pos] != b' ' {
            pos += 1;
        }
        // Skip whitespace
        while pos < self.buffer.len() && self.buffer.as_bytes()[pos] == b' ' {
            pos += 1;
        }
        self.cursor = pos;
    }

    fn delete_word_back(&mut self) {
        if self.cursor == 0 {
            return;
        }
        let mut pos = self.cursor;
        // Skip trailing whitespace
        while pos > 0 && self.buffer.as_bytes()[pos - 1] == b' ' {
            pos -= 1;
        }
        // Delete until next whitespace or start
        while pos > 0 && self.buffer.as_bytes()[pos - 1] != b' ' {
            pos -= 1;
        }
        self.buffer.drain(pos..self.cursor);
        self.cursor = pos;
    }

    fn move_left(&mut self) {
        if self.cursor > 0 {
            self.cursor = self.prev_char_boundary();
        }
    }

    fn move_right(&mut self) {
        if self.cursor < self.buffer.len() {
            self.cursor = self.next_char_boundary();
        }
    }

    fn move_home(&mut self) {
        self.cursor = 0;
    }

    fn move_end(&mut self) {
        self.cursor = self.buffer.len();
    }

    fn insert_text(&mut self, s: &str) {
        self.buffer.insert_str(self.cursor, s);
        self.cursor += s.len();
        self.history_index = None;
    }

    fn should_be_attachment(text: &str) -> bool {
        let line_count = text.lines().count();
        let char_count = text.len();
        line_count > 5 || char_count > 500
    }

    fn handle_paste(&mut self, text: &str) {
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

        // Skip if already in a code block
        if trimmed.starts_with("```") {
            return text.to_string();
        }

        // Skip short single-line text
        if !trimmed.contains('\n') && trimmed.len() < 50 {
            return text.to_string();
        }

        // Detect JSON
        if (trimmed.starts_with('{') && trimmed.ends_with('}'))
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
        {
            return format!("```json\n{}\n```", trimmed);
        }

        // Detect code languages
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

    fn add_image_attachment(&mut self, url: String) -> usize {
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

    fn build_full_content(&self) -> String {
        let mut content = self.buffer.clone();

        // Replace [Image #N] markers with actual URLs
        for att in &self.attachments {
            if let AttachmentKind::Image { ref url } = att.kind {
                let marker = format!("[Image #{}]", att.id);
                content = content.replace(&marker, url);
            }
        }

        // Append text attachments at end with ---- separator
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

    fn has_attachments(&self) -> bool {
        !self.attachments.is_empty()
    }

    fn submit(&mut self) -> String {
        let content = if self.has_attachments() {
            self.build_full_content()
        } else {
            self.buffer.clone()
        };
        // Store raw buffer (without expansion) in history
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

    fn history_up(&mut self) {
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

    fn history_down(&mut self) {
        match self.history_index {
            None => return,
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

    fn set_buffer(&mut self, s: &str) {
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

    fn marker_for_attachment(att: &Attachment) -> String {
        match &att.kind {
            AttachmentKind::Text { .. } => format!("[Text Attachment {}]", att.id),
            AttachmentKind::Image { .. } => format!("[Image #{}]", att.id),
        }
    }

    /// Find attachment marker immediately before cursor. Returns attachment index if found.
    fn marker_before_cursor(&self) -> Option<usize> {
        if self.cursor == 0 {
            return None;
        }
        // Check if char before cursor is ']'
        let bytes = self.buffer.as_bytes();
        if bytes[self.cursor - 1] != b']' {
            return None;
        }
        // Find matching '['
        let before = &self.buffer[..self.cursor];
        let bracket_start = before.rfind('[')?;
        let marker_text = &self.buffer[bracket_start..self.cursor];
        // Match against known markers
        for (i, att) in self.attachments.iter().enumerate() {
            if Self::marker_for_attachment(att) == marker_text {
                return Some(i);
            }
        }
        None
    }

    /// Remove attachment at index: delete from Vec, remove marker from buffer, adjust cursor.
    fn remove_attachment(&mut self, idx: usize) {
        if idx >= self.attachments.len() {
            return;
        }
        let marker = Self::marker_for_attachment(&self.attachments[idx]);
        self.attachments.remove(idx);
        // Remove marker from buffer
        if let Some(pos) = self.buffer.find(&marker) {
            self.buffer.drain(pos..pos + marker.len());
            if self.cursor > pos {
                self.cursor = self.cursor.saturating_sub(marker.len()).max(pos);
            }
        }
        // Clamp or clear selected_attachment
        if self.attachments.is_empty() {
            self.selected_attachment = None;
        } else if let Some(sel) = self.selected_attachment {
            if sel >= self.attachments.len() {
                self.selected_attachment = Some(self.attachments.len() - 1);
            }
        }
    }
}

// ─── Completion Menu ────────────────────────────────────────────────────────

const COMMANDS: &[(&str, &str)] = &[
    ("/project", "list or switch project"),
    ("/agent", "list or switch agent"),
    ("/new", "new context [agent@project]"),
    ("/conversations", "browse/open conversations"),
    ("/boot", "boot an offline project"),
    ("/active", "active work across all projects"),
    ("/status", "show current context"),
    ("/help", "show commands"),
    ("/quit", "exit"),
];

#[derive(Clone)]
enum ItemAction {
    /// Replace entire buffer with this text
    ReplaceFull(String),
    /// Submit this as the command (fills buffer + submits)
    Submit(String),
}

#[derive(Clone)]
struct CompletionItem {
    label: String,
    description: String,
    action: ItemAction,
}

struct CompletionMenu {
    visible: bool,
    items: Vec<CompletionItem>,
    selected: usize,
    rendered_lines: u16,
    attachment_indicator_lines: u16,
    input_wrap_lines: u16,
    cursor_row: u16,
    input_area_drawn: bool,
}

/// Build completion items for threads in a project.
/// If `project_prefix` is Some, the submit value includes `@project` prefix.
fn thread_completion_items(
    store: &AppDataStore,
    a_tag: &str,
    filter: &str,
    project_prefix: Option<&str>,
) -> Vec<CompletionItem> {
    let mut threads: Vec<&Thread> = store.get_threads(a_tag).iter().collect();
    threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));
    let threads: Vec<&Thread> = threads.into_iter().take(20).collect();
    threads
        .iter()
        .enumerate()
        .filter(|(_, t)| {
            filter.is_empty()
                || t.title.to_lowercase().contains(filter)
                || t.summary
                    .as_ref()
                    .map(|s| s.to_lowercase().contains(filter))
                    .unwrap_or(false)
        })
        .map(|(i, t)| {
            let display = t
                .summary
                .as_deref()
                .filter(|s| !s.is_empty())
                .unwrap_or(&t.title);
            let display = if display.len() > 50 {
                format!("{}...", &display[..47])
            } else {
                display.to_string()
            };

            let mut desc_parts = Vec::new();
            let working = store.operations.get_working_agents(&t.id);
            if !working.is_empty() {
                let names: Vec<String> = working
                    .iter()
                    .map(|pk| store.get_profile_name(pk))
                    .collect();
                desc_parts.push(format!("⟡ {}", names.join(", ")));
            }
            if let Some(status) = &t.status_label {
                if !status.is_empty() {
                    desc_parts.push(status.clone());
                }
            }

            let value = match project_prefix {
                Some(proj) => format!("@{} {}", proj, i + 1),
                None => (i + 1).to_string(),
            };

            CompletionItem {
                label: display,
                description: desc_parts.join(" · "),
                action: ItemAction::Submit(value),
            }
        })
        .collect()
}

/// Build completion items for active work across all projects.
/// Shows currently busy conversations (agents working via 24133) first,
/// then recently active conversations, deduped.
fn active_completion_items(store: &AppDataStore, filter: &str) -> Vec<CompletionItem> {
    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut items: Vec<CompletionItem> = Vec::new();

    // 1. Currently busy conversations (agents actively working)
    let active_ops = store.operations.get_all_active_operations();
    for op in &active_ops {
        let thread_id = op.thread_id.as_deref().unwrap_or(&op.event_id);
        if !seen_ids.insert(thread_id.to_string()) {
            continue;
        }
        let Some(thread) = store.get_thread_by_id(thread_id) else {
            continue;
        };
        let project_name = store
            .get_project_a_tag_for_thread(thread_id)
            .and_then(|a| {
                store
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == a)
                    .map(|p| p.title.clone())
            })
            .unwrap_or_default();

        let display = thread
            .summary
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&thread.title);
        let display = if display.len() > 45 {
            format!("{}...", &display[..42])
        } else {
            display.to_string()
        };

        if !filter.is_empty()
            && !display.to_lowercase().contains(filter)
            && !project_name.to_lowercase().contains(filter)
        {
            continue;
        }

        let agent_names: Vec<String> = op
            .agent_pubkeys
            .iter()
            .map(|pk| store.get_profile_name(pk))
            .collect();

        let idx = items.len() + 1;
        items.push(CompletionItem {
            label: display,
            description: format!("⟡ {} · {}", agent_names.join(", "), project_name),
            action: ItemAction::Submit(idx.to_string()),
        });
    }

    // 2. Recently active conversations across all projects (by effective_last_activity)
    let mut recent_threads: Vec<(&Thread, String)> = Vec::new();
    for project in store.get_projects().iter().filter(|p| !p.is_deleted) {
        let a_tag = project.a_tag();
        for thread in store.get_threads(&a_tag) {
            if !seen_ids.contains(&thread.id) {
                recent_threads.push((thread, project.title.clone()));
            }
        }
    }
    recent_threads.sort_by(|a, b| b.0.effective_last_activity.cmp(&a.0.effective_last_activity));

    for (thread, project_name) in recent_threads.into_iter().take(15) {
        if !seen_ids.insert(thread.id.clone()) {
            continue;
        }

        let display = thread
            .summary
            .as_deref()
            .filter(|s| !s.is_empty())
            .unwrap_or(&thread.title);
        let display = if display.len() > 45 {
            format!("{}...", &display[..42])
        } else {
            display.to_string()
        };

        if !filter.is_empty()
            && !display.to_lowercase().contains(filter)
            && !project_name.to_lowercase().contains(filter)
        {
            continue;
        }

        let mut desc_parts = Vec::new();
        let working = store.operations.get_working_agents(&thread.id);
        if !working.is_empty() {
            let names: Vec<String> = working
                .iter()
                .map(|pk| store.get_profile_name(pk))
                .collect();
            desc_parts.push(format!("⟡ {}", names.join(", ")));
        }
        if let Some(status) = &thread.status_label {
            if !status.is_empty() {
                desc_parts.push(status.clone());
            }
        }
        desc_parts.push(project_name);

        let idx = items.len() + 1;
        items.push(CompletionItem {
            label: display,
            description: desc_parts.join(" · "),
            action: ItemAction::Submit(idx.to_string()),
        });
    }

    items
}

/// Build completion items for agents in a project.
/// If `project_prefix` is Some, the submit value includes `@project` prefix.
fn agent_completion_items(
    store: &AppDataStore,
    a_tag: &str,
    filter: &str,
    project_prefix: Option<&str>,
) -> Vec<CompletionItem> {
    let agents: Vec<&ProjectAgent> = store
        .get_online_agents(a_tag)
        .map(|a| a.iter().collect())
        .unwrap_or_default();
    agents
        .iter()
        .enumerate()
        .filter(|(_, a)| filter.is_empty() || a.name.to_lowercase().contains(filter))
        .map(|(i, a)| {
            let model = a.model.as_deref().unwrap_or("unknown");
            let pm = if a.is_pm { " [PM]" } else { "" };
            let value = match project_prefix {
                Some(proj) => format!("@{} {}", proj, i + 1),
                None => (i + 1).to_string(),
            };
            CompletionItem {
                label: format!("{}{pm}", a.name),
                description: model.to_string(),
                action: ItemAction::Submit(value),
            }
        })
        .collect()
}

/// Build completion items for project picker (used by @-prefix on various commands).
fn project_picker_items(runtime: &CoreRuntime, filter: &str, cmd: &str) -> Vec<CompletionItem> {
    let store = runtime.data_store();
    let store_ref = store.borrow();
    let projects: Vec<&Project> = store_ref
        .get_projects()
        .iter()
        .filter(|p| !p.is_deleted)
        .collect();
    let mut items: Vec<(bool, CompletionItem)> = projects
        .iter()
        .filter(|p| filter.is_empty() || p.title.to_lowercase().contains(filter))
        .map(|p| {
            let online = store_ref.is_project_online(&p.a_tag());
            let status = if online { "online" } else { "offline" };
            (online, CompletionItem {
                label: p.title.clone(),
                description: status.to_string(),
                action: ItemAction::ReplaceFull(format!("{cmd} @{} ", p.title)),
            })
        })
        .collect();
    items.sort_by(|a, b| b.0.cmp(&a.0));
    items.into_iter().map(|(_, item)| item).collect()
}

impl CompletionMenu {
    fn new() -> Self {
        Self {
            visible: false,
            items: Vec::new(),
            selected: 0,
            rendered_lines: 0,
            attachment_indicator_lines: 0,
            input_wrap_lines: 0,
            cursor_row: 0,
            input_area_drawn: false,
        }
    }

    /// Update completions based on the current buffer content.
    fn update_from_buffer(&mut self, buffer: &str, state: &ReplState, runtime: &CoreRuntime) {
        // Handle "@" at start of input — agent picker: current project first, then others
        if buffer.starts_with('@') {
            let filter = buffer[1..].to_lowercase();
            let store = runtime.data_store();
            let store_ref = store.borrow();
            let mut items = Vec::new();

            // Current project agents first
            if let Some(ref current_a_tag) = state.current_project {
                items.extend(agent_completion_items(&store_ref, current_a_tag, &filter, None));
            }

            // Agents from other projects
            for project in store_ref.get_projects().iter().filter(|p| !p.is_deleted) {
                let a_tag = project.a_tag();
                if state.current_project.as_deref() == Some(&a_tag) {
                    continue;
                }
                let other_items = agent_completion_items(&store_ref, &a_tag, &filter, Some(&project.title));
                items.extend(other_items);
            }

            self.items = items;
            self.selected = 0;
            self.visible = !self.items.is_empty();
            return;
        }

        if !buffer.starts_with('/') {
            self.hide();
            return;
        }

        let (cmd_part, arg_part) = match buffer.find(' ') {
            Some(pos) => (&buffer[..pos], Some(buffer[pos + 1..].trim_start())),
            None => (buffer, None),
        };

        match arg_part {
            None => {
                // Completing the command name
                let lower = cmd_part.to_lowercase();
                self.items = COMMANDS
                    .iter()
                    .filter(|(cmd, _)| cmd.starts_with(&lower))
                    .map(|(cmd, desc)| CompletionItem {
                        label: cmd.to_string(),
                        description: desc.to_string(),
                        action: ItemAction::ReplaceFull(format!("{cmd} ")),
                    })
                    .collect();
            }
            Some(arg) => {
                // Completing arguments for specific commands
                let filter = arg.to_lowercase();
                match cmd_part {
                    "/project" | "/p" => {
                        let store = runtime.data_store();
                        let store_ref = store.borrow();
                        let projects: Vec<&Project> = store_ref
                            .get_projects()
                            .iter()
                            .filter(|p| !p.is_deleted)
                            .collect();
                        let mut items: Vec<(bool, CompletionItem)> = projects
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| filter.is_empty() || p.title.to_lowercase().contains(&filter))
                            .map(|(i, p)| {
                                let online = store_ref.is_project_online(&p.a_tag());
                                let status = if online { "online" } else { "offline" };
                                (online, CompletionItem {
                                    label: p.title.clone(),
                                    description: status.to_string(),
                                    action: ItemAction::Submit((i + 1).to_string()),
                                })
                            })
                            .collect();
                        // Online projects first
                        items.sort_by(|a, b| b.0.cmp(&a.0));
                        self.items = items.into_iter().map(|(_, item)| item).collect();
                    }
                    "/agent" | "/a" => {
                        if let Some(at_pos) = arg.find('@') {
                            let after_at = &arg[at_pos + 1..];

                            if let Some(space_pos) = after_at.find(' ') {
                                // "@project filter" — show agents from that project
                                let project_part = after_at[..space_pos].trim();
                                let agent_filter = after_at[space_pos + 1..].trim().to_lowercase();

                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let lower_proj = project_part.to_lowercase();
                                if let Some(project) = store_ref.get_projects().iter()
                                    .filter(|p| !p.is_deleted)
                                    .find(|p| p.title.to_lowercase().contains(&lower_proj))
                                {
                                    let a_tag = project.a_tag();
                                    self.items = agent_completion_items(&store_ref, &a_tag, &agent_filter, Some(project_part));
                                }
                            } else {
                                // "@filter" — show project picker
                                let project_filter = after_at.to_lowercase();
                                self.items = project_picker_items(runtime, &project_filter, cmd_part);
                            }
                        } else if let Some(ref a_tag) = state.current_project {
                            let store = runtime.data_store();
                            let store_ref = store.borrow();
                            self.items = agent_completion_items(&store_ref, a_tag, &filter, None);
                        } else {
                            self.items.clear();
                        }
                    }
                    "/open" | "/o" | "/conversations" | "/c" => {
                        if let Some(at_pos) = arg.find('@') {
                            let after_at = &arg[at_pos + 1..];

                            if let Some(space_pos) = after_at.find(' ') {
                                // "@project filter" — show conversations from that project
                                let project_part = after_at[..space_pos].trim();
                                let conv_filter = after_at[space_pos + 1..].trim().to_lowercase();

                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let lower_proj = project_part.to_lowercase();
                                if let Some(project) = store_ref.get_projects().iter()
                                    .filter(|p| !p.is_deleted)
                                    .find(|p| p.title.to_lowercase().contains(&lower_proj))
                                {
                                    let a_tag = project.a_tag();
                                    self.items = thread_completion_items(&store_ref, &a_tag, &conv_filter, Some(project_part));
                                }
                            } else {
                                // "@filter" — show project picker
                                let project_filter = after_at.to_lowercase();
                                self.items = project_picker_items(runtime, &project_filter, cmd_part);
                            }
                        } else if let Some(ref a_tag) = state.current_project {
                            let store = runtime.data_store();
                            let store_ref = store.borrow();
                            self.items = thread_completion_items(&store_ref, a_tag, &filter, None);
                        } else {
                            self.items.clear();
                        }
                    }
                    "/active" => {
                        let store = runtime.data_store();
                        let store_ref = store.borrow();
                        self.items = active_completion_items(&store_ref, &filter);
                    }
                    "/new" | "/n" => {
                        if let Some(at_pos) = arg.find('@') {
                            let agent_part = arg[..at_pos].trim();
                            let after_at = &arg[at_pos + 1..];

                            if let Some(space_pos) = after_at.find(' ') {
                                // "@project agent_filter" — show agents in that project
                                let project_part = after_at[..space_pos].trim();
                                let agent_filter = after_at[space_pos + 1..].trim().to_lowercase();

                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let projects: Vec<&Project> = store_ref
                                    .get_projects()
                                    .iter()
                                    .filter(|p| !p.is_deleted)
                                    .collect();
                                let lower_proj = project_part.to_lowercase();
                                if let Some(project) = projects.iter().find(|p| p.title.to_lowercase().contains(&lower_proj)) {
                                    let a_tag = project.a_tag();
                                    if let Some(agents) = store_ref.get_online_agents(&a_tag) {
                                        self.items = agents
                                            .iter()
                                            .filter(|a| agent_filter.is_empty() || a.name.to_lowercase().contains(&agent_filter))
                                            .map(|a| {
                                                let model = a.model.as_deref().unwrap_or("unknown");
                                                let pm = if a.is_pm { " [PM]" } else { "" };
                                                CompletionItem {
                                                    label: format!("{}{pm}", a.name),
                                                    description: model.to_string(),
                                                    action: ItemAction::Submit(format!("{}@{}", a.name, project_part)),
                                                }
                                            })
                                            .collect();
                                    }
                                }
                            } else if !agent_part.is_empty() {
                                // "agent@filter" — show projects that have a matching agent
                                let project_filter = after_at.to_lowercase();
                                let agent_lower = agent_part.to_lowercase();

                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let projects: Vec<&Project> = store_ref
                                    .get_projects()
                                    .iter()
                                    .filter(|p| !p.is_deleted)
                                    .collect();

                                let mut items: Vec<(bool, CompletionItem)> = Vec::new();
                                for project in &projects {
                                    let a_tag = project.a_tag();
                                    if !project_filter.is_empty() && !project.title.to_lowercase().contains(&project_filter) {
                                        continue;
                                    }
                                    // Check if this project has an agent matching agent_part
                                    let has_agent = store_ref
                                        .get_online_agents(&a_tag)
                                        .map(|agents| agents.iter().any(|a| a.name.to_lowercase().contains(&agent_lower)))
                                        .unwrap_or(false);
                                    if !has_agent {
                                        continue;
                                    }
                                    let online = store_ref.is_project_online(&a_tag);
                                    let status = if online { "online" } else { "offline" };
                                    items.push((online, CompletionItem {
                                        label: project.title.clone(),
                                        description: status.to_string(),
                                        action: ItemAction::Submit(format!("{}@{}", agent_part, project.title)),
                                    }));
                                }
                                items.sort_by(|a, b| b.0.cmp(&a.0));
                                self.items = items.into_iter().map(|(_, item)| item).collect();
                            } else {
                                // "@filter" (no agent before @) — show all projects
                                let project_filter = after_at.to_lowercase();

                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let projects: Vec<&Project> = store_ref
                                    .get_projects()
                                    .iter()
                                    .filter(|p| !p.is_deleted)
                                    .collect();

                                let mut items: Vec<(bool, CompletionItem)> = projects
                                    .iter()
                                    .filter(|p| project_filter.is_empty() || p.title.to_lowercase().contains(&project_filter))
                                    .map(|p| {
                                        let online = store_ref.is_project_online(&p.a_tag());
                                        let status = if online { "online" } else { "offline" };
                                        (online, CompletionItem {
                                            label: p.title.clone(),
                                            description: status.to_string(),
                                            action: ItemAction::ReplaceFull(format!("/new @{} ", p.title)),
                                        })
                                    })
                                    .collect();
                                items.sort_by(|a, b| b.0.cmp(&a.0));
                                self.items = items.into_iter().map(|(_, item)| item).collect();
                            }
                        } else {
                            // No @ — show agents in current project, filtered by arg
                            if let Some(ref a_tag) = state.current_project {
                                let store = runtime.data_store();
                                let store_ref = store.borrow();
                                let agents: Vec<&ProjectAgent> = store_ref
                                    .get_online_agents(a_tag)
                                    .map(|a| a.iter().collect())
                                    .unwrap_or_default();
                                self.items = agents
                                    .iter()
                                    .filter(|a| filter.is_empty() || a.name.to_lowercase().contains(&filter))
                                    .map(|a| {
                                        let model = a.model.as_deref().unwrap_or("unknown");
                                        let pm = if a.is_pm { " [PM]" } else { "" };
                                        CompletionItem {
                                            label: format!("{}{pm}", a.name),
                                            description: model.to_string(),
                                            action: ItemAction::ReplaceFull(format!("/new {}", a.name)),
                                        }
                                    })
                                    .collect();
                            } else {
                                self.items.clear();
                            }
                        }
                    }
                    "/boot" | "/b" => {
                        let store = runtime.data_store();
                        let store_ref = store.borrow();
                        let projects: Vec<&Project> = store_ref
                            .get_projects()
                            .iter()
                            .filter(|p| !p.is_deleted)
                            .collect();
                        self.items = projects
                            .iter()
                            .enumerate()
                            .filter(|(_, p)| {
                                let online = store_ref.is_project_online(&p.a_tag());
                                !online && (filter.is_empty() || p.title.to_lowercase().contains(&filter))
                            })
                            .map(|(i, p)| CompletionItem {
                                label: p.title.clone(),
                                description: "offline".to_string(),
                                action: ItemAction::Submit((i + 1).to_string()),
                            })
                            .collect();
                    }
                    _ => {
                        self.items.clear();
                    }
                }
            }
        }

        self.visible = !self.items.is_empty();
        if self.selected >= self.items.len() {
            self.selected = 0;
        }
    }

    fn hide(&mut self) {
        self.visible = false;
        self.items.clear();
        self.selected = 0;
    }

    fn select_next(&mut self) {
        if !self.items.is_empty() {
            self.selected = (self.selected + 1) % self.items.len();
        }
    }

    fn select_prev(&mut self) {
        if !self.items.is_empty() {
            self.selected = if self.selected == 0 {
                self.items.len() - 1
            } else {
                self.selected - 1
            };
        }
    }

    fn accept(&mut self) -> Option<ItemAction> {
        if !self.visible || self.items.is_empty() {
            return None;
        }
        let action = self.items[self.selected].action.clone();
        self.hide();
        Some(action)
    }
}

// ─── REPL State ─────────────────────────────────────────────────────────────

struct ReplState {
    current_project: Option<String>,
    current_agent: Option<String>,
    current_agent_name: Option<String>,
    current_conversation: Option<String>,
    user_pubkey: String,
    streaming_in_progress: bool,
    stream_buffer: String,
    /// Track last displayed message pubkey for consecutive message dedup
    last_displayed_pubkey: Option<String>,
    /// Wave animation on project name when switching projects
    project_anim_start: Option<Instant>,
    project_anim_name: String,
    /// Frame counter for runtime wave animation (incremented every tick)
    wave_frame: u64,
}

impl ReplState {
    fn new(user_pubkey: String) -> Self {
        Self {
            current_project: None,
            current_agent: None,
            current_agent_name: None,
            current_conversation: None,
            user_pubkey,
            streaming_in_progress: false,
            stream_buffer: String::new(),
            last_displayed_pubkey: None,
            project_anim_start: None,
            project_anim_name: String::new(),
            wave_frame: 0,
        }
    }

    fn start_project_animation(&mut self, name: &str) {
        self.project_anim_start = Some(Instant::now());
        self.project_anim_name = name.to_string();
    }

    fn is_animating(&self) -> bool {
        self.project_anim_start
            .map(|t| t.elapsed().as_millis() < 5000)
            .unwrap_or(false)
    }

    fn has_active_agents(&self, runtime: &CoreRuntime) -> bool {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        store_ref.operations.has_active_agents()
    }

    fn project_display(&self, runtime: &CoreRuntime) -> String {
        match &self.current_project {
            Some(a_tag) => {
                let store = runtime.data_store();
                let store_ref = store.borrow();
                store_ref
                    .get_projects()
                    .iter()
                    .find(|p| p.a_tag() == *a_tag)
                    .map(|p| p.title.clone())
                    .unwrap_or_else(|| "unknown".to_string())
            }
            None => "no-project".to_string(),
        }
    }

    fn agent_display(&self) -> String {
        self.current_agent_name
            .clone()
            .unwrap_or_else(|| "no-agent".to_string())
    }

    fn switch_project(&mut self, a_tag: String, runtime: &CoreRuntime) {
        let name = {
            let store = runtime.data_store();
            let store_ref = store.borrow();
            store_ref.get_projects().iter()
                .find(|p| p.a_tag() == a_tag)
                .map(|p| p.title.clone())
                .unwrap_or_else(|| "project".to_string())
        };
        self.current_project = Some(a_tag);
        self.start_project_animation(&name);
    }

    fn status_bar_text(&self, runtime: &CoreRuntime) -> String {
        let project = self.project_display(runtime);
        let agent = self.agent_display();

        let project_rendered = if let Some(start) = self.project_anim_start {
            let elapsed_ms = start.elapsed().as_millis() as f64;
            if elapsed_ms < 5000.0 {
                wave_colorize(&project, elapsed_ms, &[44, 37, 73, 109, 117, 159])
            } else {
                format!("{CYAN}{project}{RESET}")
            }
        } else {
            format!("{CYAN}{project}{RESET}")
        };

        let mut text = format!("{project_rendered}{DIM}/{RESET}{GREEN}{agent}{RESET}");

        let store = runtime.data_store();
        let store_ref = store.borrow();

        // Show agents working on current conversation
        if let Some(ref conv_id) = self.current_conversation {
            let working_agents = store_ref.operations.get_working_agents(conv_id);
            if !working_agents.is_empty() {
                let names: Vec<String> = working_agents
                    .iter()
                    .map(|pk| store_ref.get_profile_name(pk))
                    .collect();
                text.push_str(&format!(
                    "  {YELLOW}⟡ {} working{RESET}",
                    names.join(", ")
                ));
            }
        }

        // Show agents working on other conversations
        for ops in store_ref.operations.get_all_active_operations() {
            let thread_id = ops.thread_id.as_deref().unwrap_or(&ops.event_id);
            if self.current_conversation.as_deref() == Some(thread_id) {
                continue;
            }
            let names: Vec<String> = ops
                .agent_pubkeys
                .iter()
                .map(|pk| store_ref.get_profile_name(pk))
                .collect();
            let title = store_ref
                .get_thread_by_id(thread_id)
                .and_then(|t| {
                    t.summary
                        .clone()
                        .filter(|s| !s.is_empty())
                        .or(Some(t.title.clone()))
                })
                .unwrap_or_else(|| format!("{}…", &thread_id[..thread_id.len().min(12)]));
            let title = if title.len() > 40 {
                format!("{}…", &title[..39])
            } else {
                title
            };
            text.push_str(&format!(
                "  {DIM}⚡ {} → \"{title}\"{RESET}",
                names.join(", ")
            ));
        }

        text
    }

    fn status_bar_plain_width(&self, runtime: &CoreRuntime) -> usize {
        let project = self.project_display(runtime);
        let agent = self.agent_display();
        let mut width = project.len() + 1 + agent.len();

        let store = runtime.data_store();
        let store_ref = store.borrow();

        if let Some(ref conv_id) = self.current_conversation {
            let working_agents = store_ref.operations.get_working_agents(conv_id);
            if !working_agents.is_empty() {
                let names: Vec<String> = working_agents
                    .iter()
                    .map(|pk| store_ref.get_profile_name(pk))
                    .collect();
                // "  ⟡ {names} working"
                width += 2 + 2 + names.join(", ").len() + " working".len();
            }
        }

        for ops in store_ref.operations.get_all_active_operations() {
            let thread_id = ops.thread_id.as_deref().unwrap_or(&ops.event_id);
            if self.current_conversation.as_deref() == Some(thread_id) {
                continue;
            }
            let names: Vec<String> = ops
                .agent_pubkeys
                .iter()
                .map(|pk| store_ref.get_profile_name(pk))
                .collect();
            let title = store_ref
                .get_thread_by_id(thread_id)
                .and_then(|t| {
                    t.summary
                        .clone()
                        .filter(|s| !s.is_empty())
                        .or(Some(t.title.clone()))
                })
                .unwrap_or_else(|| format!("{}…", &thread_id[..thread_id.len().min(12)]));
            let title = if title.len() > 40 {
                format!("{}…", &title[..39])
            } else {
                title
            };
            // "  ⚡ {names} → "{title}""
            width += 2 + 2 + names.join(", ").len() + " → \"".len() + title.len() + "\"".len();
        }

        width
    }
}

// ─── Terminal Drawing ───────────────────────────────────────────────────────

const PROMPT_PREFIX_WIDTH: u16 = 4;
// Half-block characters for visual padding around the input box
const HALF_BLOCK_LOWER: char = '▄'; // upper edge (fg colored, bg transparent)
const HALF_BLOCK_UPPER: char = '▀'; // lower edge

fn term_width() -> u16 {
    terminal::size().map(|(w, _)| w).unwrap_or(80)
}

/// Draw the full input area with half-block padding:
///   Line 0: ▄▄▄▄▄ (top edge, fg=input_bg)
///   Line 1: │ › input text │ (input line, bg=input_bg)
///   Line 2: ▀▀▀▀▀ (bottom edge, fg=input_bg) — OR completion items push this down
///   Line N: status bar (project/agent)
///
/// Cursor is positioned on Line 1 (the input line).
fn redraw_input(
    stdout: &mut Stdout,
    state: &ReplState,
    runtime: &CoreRuntime,
    editor: &LineEditor,
    completion: &mut CompletionMenu,
) {
    let width = term_width() as usize;

    // Clear all previously rendered lines
    clear_input_area(stdout, completion);

    // ── Top edge: lower half-blocks in input bg color ──
    let fg_input_bg = "\x1b[38;5;234m"; // fg = same color as BG_INPUT
    write!(stdout, "{fg_input_bg}{}{RESET}", HALF_BLOCK_LOWER.to_string().repeat(width)).ok();

    // ── Backend approval prompt (replaces normal input when pending) ──
    {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        let pending_info = store_ref.trust.pending_backend_approvals.first().map(|a| {
            let name = store_ref.get_profile_name(&a.backend_pubkey);
            let short_pk = if a.backend_pubkey.len() >= 12 {
                a.backend_pubkey[..12].to_string()
            } else {
                a.backend_pubkey.clone()
            };
            (name, short_pk)
        });
        drop(store_ref);
        if let Some((name, short_pk)) = pending_info {
            let prompt = format!("  New backend: {YELLOW}{name}{RESET}{BG_INPUT}{DIM} ({short_pk}…){RESET}{BG_INPUT}  {WHITE_BOLD}[a]{RESET}{BG_INPUT}pprove  {WHITE_BOLD}[b]{RESET}{BG_INPUT}lock");
            let visible_len = 2 + "New backend: ".len() + name.len() + 2 + short_pk.len() + 2 + 2 + "[a]pprove  [b]lock".len();
            let prompt_pad = width.saturating_sub(visible_len);
            write!(stdout, "\r\n{BG_INPUT}{prompt}{}{RESET}", " ".repeat(prompt_pad)).ok();
            completion.input_wrap_lines = 0;
            completion.attachment_indicator_lines = 0;
            completion.rendered_lines = 0;

            // Bottom edge + status bar
            write!(stdout, "\r\n{fg_input_bg}{}{RESET}", HALF_BLOCK_UPPER.to_string().repeat(width)).ok();
            let status_text = state.status_bar_text(runtime);
            let status_plain_width = state.status_bar_plain_width(runtime);
            let (rt_ms, rt_active, rt_count) = {
                let store = runtime.data_store();
                let store_ref = store.borrow();
                store_ref.get_statusbar_runtime_ms()
            };
            let (rt_ansi, rt_plain_width) =
                build_runtime_indicator(rt_ms, rt_active, rt_count, state.wave_frame);
            let left_used = 3 + status_plain_width;
            let gap = width.saturating_sub(left_used + rt_plain_width);
            write!(
                stdout,
                "\r\n{DIM}   {status_text}{RESET}{}{rt_ansi}",
                " ".repeat(gap)
            )
            .ok();

            queue!(stdout, cursor::MoveUp(2)).ok();
            queue!(stdout, cursor::MoveToColumn(visible_len as u16)).ok();
            stdout.flush().ok();
            completion.cursor_row = 0;
            completion.input_area_drawn = true;
            return;
        }
    }

    // ── Input line (dark background, full width) ──
    let input_visible_len = PROMPT_PREFIX_WIDTH as usize + editor.buffer.chars().count();
    let input_total_rows = if width > 0 { input_visible_len.saturating_sub(1) / width + 1 } else { 1 };
    let last_row_chars = if width > 0 { ((input_visible_len.saturating_sub(1)) % width) + 1 } else { input_visible_len };
    let pad = width.saturating_sub(last_row_chars);
    write!(stdout, "\r\n{BG_INPUT}{WHITE_BOLD}  › {RESET}{BG_INPUT}{}{}{RESET}",
        editor.buffer,
        " ".repeat(pad),
    ).ok();
    completion.input_wrap_lines = (input_total_rows as u16).saturating_sub(1);

    // ── Attachment strip (below input, if attachments exist) ──
    if editor.has_attachments() {
        write!(stdout, "\r\n{BG_INPUT}  📎 ").ok();
        let mut strip_chars: usize = 5; // "  📎 " = 5 visible chars (📎 counts as 2)
        for (i, att) in editor.attachments.iter().enumerate() {
            let label = match &att.kind {
                AttachmentKind::Text { content } => {
                    let lines = content.lines().count();
                    format!("Text Att. {} ({} line{})", att.id, lines, if lines == 1 { "" } else { "s" })
                }
                AttachmentKind::Image { .. } => format!("Image #{}", att.id),
            };
            let is_selected = editor.selected_attachment == Some(i);
            if is_selected {
                write!(stdout, " {WHITE_BOLD}{BG_HIGHLIGHT} {label} {RESET}{BG_INPUT}").ok();
            } else {
                write!(stdout, " {DIM} {label} {RESET}{BG_INPUT}").ok();
            }
            strip_chars += 2 + label.len() + 1; // " " + " label " spacing
        }
        let strip_pad = width.saturating_sub(strip_chars);
        write!(stdout, "{}{RESET}", " ".repeat(strip_pad)).ok();
        completion.attachment_indicator_lines = 1;
    } else {
        completion.attachment_indicator_lines = 0;
    }

    // ── Completion menu (if visible, between input and bottom edge) ──
    if completion.visible && !completion.items.is_empty() {
        let count = completion.items.len().min(12) as u16; // cap at 12 visible items
        for (i, item) in completion.items.iter().take(12).enumerate() {
            let label = &item.label;
            let desc = &item.description;
            write!(stdout, "\r\n").ok();
            queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
            let text = format!("  {label:<24} {desc}");
            let text_len = 2 + label.chars().count().max(24) + 1 + desc.chars().count();
            let item_pad = if width > text_len { width - text_len } else { 0 };
            if i == completion.selected {
                write!(stdout, "{BG_INPUT}{WHITE_BOLD}{text}{}{RESET}", " ".repeat(item_pad)).ok();
            } else {
                write!(stdout, "{BG_INPUT}{DIM}{text}{}{RESET}", " ".repeat(item_pad)).ok();
            }
        }
        completion.rendered_lines = count;
    } else {
        completion.rendered_lines = 0;
    }

    // ── Bottom edge: upper half-blocks in input bg color ──
    write!(stdout, "\r\n{fg_input_bg}{}{RESET}", HALF_BLOCK_UPPER.to_string().repeat(width)).ok();

    // ── Status bar (project/agent on left, runtime on right) ──
    let status_text = state.status_bar_text(runtime);
    let status_plain_width = state.status_bar_plain_width(runtime);
    let (runtime_ms, has_active, active_count) = {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        store_ref.get_statusbar_runtime_ms()
    };
    let (runtime_ansi, runtime_plain_width) =
        build_runtime_indicator(runtime_ms, has_active, active_count, state.wave_frame);
    let left_used = 3 + status_plain_width; // "   " prefix + status text
    let gap = width.saturating_sub(left_used + runtime_plain_width);
    write!(
        stdout,
        "\r\n{DIM}   {status_text}{RESET}{}{runtime_ansi}",
        " ".repeat(gap)
    )
    .ok();

    // ── Position cursor back on input line ──
    // Lines below the last input row: attachment_indicator + completion + bottom_edge + status_bar
    let lines_below = completion.attachment_indicator_lines + completion.rendered_lines + 2;
    // Figure out which terminal row the cursor is on within the (possibly wrapped) input
    let cursor_char_pos = PROMPT_PREFIX_WIDTH as usize + editor.buffer[..editor.cursor].chars().count();
    let cursor_row = if width > 0 && cursor_char_pos > 0 { (cursor_char_pos - 1) / width } else { 0 };
    let wrap_lines_after_cursor = (completion.input_wrap_lines as usize).saturating_sub(cursor_row);
    queue!(stdout, cursor::MoveUp(lines_below + wrap_lines_after_cursor as u16)).ok();
    let col = if width > 0 && cursor_char_pos > 0 { ((cursor_char_pos - 1) % width) + 1 } else { 0 };
    queue!(stdout, cursor::MoveToColumn(col as u16)).ok();
    stdout.flush().ok();

    completion.cursor_row = cursor_row as u16;
    completion.input_area_drawn = true;
}

/// Clear the entire input area (top edge + input + completion + bottom edge + status bar).
/// After clearing, cursor is at the position where top edge should be drawn.
fn clear_input_area(stdout: &mut Stdout, completion: &mut CompletionMenu) {
    if !completion.input_area_drawn {
        // First draw — nothing to clear, cursor is already where we want it
        return;
    }

    // Cursor is on `cursor_row` within the input. Move up to top edge (1 row above input row 0).
    let up_to_top = completion.cursor_row + 1;
    queue!(stdout, cursor::MoveUp(up_to_top), cursor::MoveToColumn(0)).ok();
    queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();

    // Clear: input rows (1 + wrap_lines) + attachment_indicator + completion lines + bottom edge + status bar
    let lines_below = 1 + completion.input_wrap_lines + completion.attachment_indicator_lines + completion.rendered_lines + 2;
    for _ in 0..lines_below {
        write!(stdout, "\r\n").ok();
        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
    }
    // Move back up to where top edge was
    queue!(stdout, cursor::MoveUp(lines_below)).ok();
    stdout.flush().ok();
    completion.rendered_lines = 0;
    completion.attachment_indicator_lines = 0;
    completion.input_wrap_lines = 0;
}

/// Render text with a wave of brightness sweeping left-to-right.
/// `palette` is a set of 256-color codes from dim to bright.
/// The wave peak moves across the characters over 5000ms.
fn wave_colorize(text: &str, elapsed_ms: f64, palette: &[u8]) -> String {
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len().max(1) as f64;
    // Wave peak position (0..1) sweeps across, then a second pass dims down
    let progress = (elapsed_ms / 5000.0).clamp(0.0, 1.0);
    let peak = progress * 1.4 - 0.2; // overshoot slightly for smooth entry/exit

    let mut out = String::new();
    let pal_max = palette.len() - 1;

    for (i, ch) in chars.iter().enumerate() {
        let char_pos = i as f64 / len;
        // Distance from wave peak, normalized
        let dist = (char_pos - peak).abs();
        // Brightness: 1.0 at peak, fading with distance; gaussian-ish
        let brightness = (-dist * dist * 20.0).exp();
        // Blend between dim (index 0) and bright (last index)
        let idx = (brightness * pal_max as f64).round() as usize;
        let color = palette[idx.min(pal_max)];
        out.push_str(&format!("\x1b[38;5;{color}m{ch}"));
    }
    out.push_str(RESET);
    out
}

/// Format runtime in milliseconds to a human-readable string (HH:MM:SS or MM:SS)
fn format_runtime(total_ms: u64) -> String {
    let total_seconds = total_ms / 1000;
    let hours = total_seconds / 3600;
    let minutes = (total_seconds % 3600) / 60;
    let seconds = total_seconds % 60;
    if hours > 0 {
        format!("{:02}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{:02}:{:02}", minutes, seconds)
    }
}

/// Build the runtime indicator string with wave animation when agents are active.
/// Returns (ansi_colored_string, plain_char_count).
fn build_runtime_indicator(
    cumulative_runtime_ms: u64,
    has_active_agents: bool,
    active_agent_count: usize,
    wave_frame: u64,
) -> (String, usize) {
    let label = format!("Today: {} ", format_runtime(cumulative_runtime_ms));
    let plain_width = label.len();

    if !has_active_agents {
        // Red when no agents are working
        return (format!("{RED}{label}{RESET}"), plain_width);
    }

    // Green wave animation when agents are active
    // Wave parameters matching the TUI
    let base_r: f32 = 106.0;
    let base_g: f32 = 153.0;
    let base_b: f32 = 85.0;
    let wave_phase_speed: f32 = 0.3;
    let wave_wavelength: f32 = 0.8;
    let wave_period: f32 = 12.0;

    let agent_count_clamped = active_agent_count.max(1).min(10) as f32;
    let speed_multiplier = 0.3 * agent_count_clamped;
    let brightness_amplitude = 0.3 + (0.3 * (agent_count_clamped - 1.0) / 9.0);
    let offset = (wave_frame / 2) as f32; // slow down: every 2 frames

    let mut out = String::new();
    for (i, ch) in label.chars().enumerate() {
        let phase = ((offset * wave_phase_speed * speed_multiplier)
            + (i as f32 * wave_wavelength))
            * std::f32::consts::PI
            * 2.0
            / wave_period;
        let wave_value = phase.sin();
        let brightness = 1.0 + (wave_value * brightness_amplitude);
        let r = (base_r * brightness).clamp(0.0, 255.0) as u8;
        let g = (base_g * brightness).clamp(0.0, 255.0) as u8;
        let b = (base_b * brightness).clamp(0.0, 255.0) as u8;
        out.push_str(&format!("\x1b[38;2;{r};{g};{b}m{ch}"));
    }
    out.push_str(RESET);

    (out, plain_width)
}

/// Print text above the input area, then redraw the input area.
fn print_above_input(
    stdout: &mut Stdout,
    text: &str,
    state: &ReplState,
    runtime: &CoreRuntime,
    editor: &LineEditor,
    completion: &mut CompletionMenu,
) {
    // Clear the entire input area
    clear_input_area(stdout, completion);
    completion.input_area_drawn = false;

    // Print the text
    for line in text.split('\n') {
        write!(stdout, "{}\r\n", line).ok();
    }
    stdout.flush().ok();

    // Redraw input area
    redraw_input(stdout, state, runtime, editor, completion);
}

// ─── Markdown Colorizer ─────────────────────────────────────────────────────

const BOLD_COLOR: &str = "\x1b[1;38;5;222m"; // #FFD787
const CODE_INLINE: &str = "\x1b[36;48;5;237m"; // cyan on dark bg
const CODE_BLOCK: &str = "\x1b[38;5;248m"; // light gray
const CODE_BLOCK_BAR: &str = "\x1b[38;5;240m"; // dim bar
const HEADING_COLOR: &str = "\x1b[1;38;5;222m"; // #FFD787
const LIST_MARKER: &str = "\x1b[36m"; // cyan

/// Colorize markdown content for terminal display.
fn colorize_markdown(content: &str) -> String {
    let mut out = String::new();
    let mut in_code_block = false;

    for line in content.lines() {
        if !out.is_empty() {
            out.push('\n');
        }

        // Code block fences
        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            out.push_str(&format!("{CODE_BLOCK_BAR}{}{RESET}", line));
            continue;
        }

        if in_code_block {
            out.push_str(&format!("{CODE_BLOCK_BAR}│{CODE_BLOCK} {}{RESET}", line));
            continue;
        }

        // Headings
        if let Some(rest) = line.strip_prefix("### ") {
            out.push_str(&format!("{HEADING_COLOR}### {rest}{RESET}"));
            continue;
        }
        if let Some(rest) = line.strip_prefix("## ") {
            out.push_str(&format!("{HEADING_COLOR}## {rest}{RESET}"));
            continue;
        }
        if let Some(rest) = line.strip_prefix("# ") {
            out.push_str(&format!("{HEADING_COLOR}# {rest}{RESET}"));
            continue;
        }

        // List items (- or * or numbered)
        let trimmed = line.trim_start();
        let leading_ws = &line[..line.len() - trimmed.len()];
        if trimmed.starts_with("- ") || trimmed.starts_with("* ") {
            let bullet = &trimmed[..2];
            let rest = &trimmed[2..];
            out.push_str(leading_ws);
            out.push_str(&format!("{LIST_MARKER}{bullet}{RESET}"));
            out.push_str(&colorize_inline(rest));
            continue;
        }
        // Numbered lists: "1. ", "2. ", etc.
        if let Some(dot_pos) = trimmed.find(". ") {
            if dot_pos <= 3 && trimmed[..dot_pos].chars().all(|c| c.is_ascii_digit()) {
                let num_part = &trimmed[..dot_pos + 2];
                let rest = &trimmed[dot_pos + 2..];
                out.push_str(leading_ws);
                out.push_str(&format!("{LIST_MARKER}{num_part}{RESET}"));
                out.push_str(&colorize_inline(rest));
                continue;
            }
        }

        out.push_str(&colorize_inline(line));
    }

    out
}

/// Colorize inline markdown: **bold**, `code`
fn colorize_inline(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
        // Bold: **text**
        if i + 1 < len && chars[i] == '*' && chars[i + 1] == '*' {
            if let Some(end) = find_closing(&chars, i + 2, &['*', '*']) {
                out.push_str(BOLD_COLOR);
                let inner: String = chars[i + 2..end].iter().collect();
                out.push_str(&inner);
                out.push_str(RESET);
                i = end + 2;
                continue;
            }
        }

        // Inline code: `text`
        if chars[i] == '`' {
            if let Some(end) = chars[i + 1..].iter().position(|&c| c == '`') {
                let end = i + 1 + end;
                out.push_str(CODE_INLINE);
                out.push(' ');
                let inner: String = chars[i + 1..end].iter().collect();
                out.push_str(&inner);
                out.push(' ');
                out.push_str(RESET);
                i = end + 1;
                continue;
            }
        }

        out.push(chars[i]);
        i += 1;
    }

    out
}

/// Find closing marker (e.g. ** ) starting from `start`.
fn find_closing(chars: &[char], start: usize, marker: &[char]) -> Option<usize> {
    let mlen = marker.len();
    if start + mlen > chars.len() {
        return None;
    }
    for i in start..chars.len() - mlen + 1 {
        if chars[i..i + mlen] == *marker {
            return Some(i);
        }
    }
    None
}

// ─── Attachment Parsing & Rendering ─────────────────────────────────────────

/// Parse message content into (body, attachments).
/// Splits on "\n----\n" and parses "-- Text Attachment N --" headers.
fn parse_message_attachments(content: &str) -> (String, Vec<(String, String)>) {
    let separator = "\n----\n";
    let Some(sep_pos) = content.find(separator) else {
        return (content.to_string(), Vec::new());
    };

    let body = content[..sep_pos].to_string();
    let attachment_section = &content[sep_pos + separator.len()..];

    let mut attachments: Vec<(String, String)> = Vec::new();
    let mut current_header = String::new();
    let mut current_content = String::new();

    for line in attachment_section.lines() {
        if line.starts_with("-- Text Attachment ") && line.ends_with(" --") {
            // Save previous attachment if any
            if !current_header.is_empty() {
                attachments.push((current_header.clone(), current_content.trim_end().to_string()));
            }
            current_header = line.to_string();
            current_content.clear();
        } else {
            if !current_content.is_empty() {
                current_content.push('\n');
            }
            current_content.push_str(line);
        }
    }
    if !current_header.is_empty() {
        attachments.push((current_header, current_content.trim_end().to_string()));
    }

    (body, attachments)
}

/// Render a single attachment as a bordered, truncated block.
fn render_attachment_block(header: &str, content: &str) -> String {
    let width = term_width() as usize;
    let box_width = width.min(60);
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let preview_count = 3.min(total);

    // Header line
    let label = header.trim_matches('-').trim();
    let line_info = format!(" ({} lines)", total);
    let header_text = format!(" {} {}", label, line_info);
    let header_pad = box_width.saturating_sub(header_text.len() + 2);
    let mut out = format!(
        "{DIM}╭─{}{}{RESET}",
        header_text,
        "─".repeat(header_pad)
    );

    // Preview lines
    for line in lines.iter().take(preview_count) {
        let truncated = if line.len() > box_width - 4 {
            format!("{}...", &line[..box_width.saturating_sub(7)])
        } else {
            line.to_string()
        };
        let pad = box_width.saturating_sub(truncated.len() + 4);
        out.push_str(&format!("\n{DIM}│{RESET} {CODE_BLOCK}{truncated}{RESET}{}", " ".repeat(pad)));
    }

    // "N more lines" indicator
    if total > preview_count {
        let more = format!("... ({} more lines)", total - preview_count);
        let pad = box_width.saturating_sub(more.len() + 4);
        out.push_str(&format!("\n{DIM}│ {more}{}{RESET}", " ".repeat(pad)));
    }

    // Bottom border
    out.push_str(&format!("\n{DIM}╰{}╯{RESET}", "─".repeat(box_width.saturating_sub(2))));

    out
}

/// Replace [Text Attachment N] markers in body with styled references.
fn style_attachment_markers(body: &str) -> String {
    let mut result = body.to_string();
    // Find all [Text Attachment N] patterns
    let mut i = 1;
    loop {
        let marker = format!("[Text Attachment {}]", i);
        if !result.contains(&marker) {
            break;
        }
        let styled = format!("{DIM}[📎 Text Attachment {i}]{RESET}");
        result = result.replace(&marker, &styled);
        i += 1;
    }
    result
}

/// Format content with attachment rendering (body + attachment blocks).
fn render_content_with_attachments(content: &str) -> String {
    let (body, attachments) = parse_message_attachments(content);
    let styled_body = style_attachment_markers(&body);
    let colored = colorize_markdown(&styled_body);

    if attachments.is_empty() {
        return colored;
    }

    let mut out = colored;
    for (header, att_content) in &attachments {
        out.push('\n');
        out.push_str(&render_attachment_block(header, att_content));
    }
    out
}

fn print_separator_raw() -> String {
    let time = Local::now().format("%H:%M").to_string();
    let line = "─".repeat(40);
    format!("{DIM}{line} {time}{RESET}")
}

fn print_user_message_raw(content: &str) -> String {
    let rendered = render_content_with_attachments(content);
    format!("{WHITE_BOLD}you ›{RESET} {rendered}")
}

fn print_agent_message_raw(agent_name: &str, content: &str) -> String {
    let rendered = render_content_with_attachments(content);
    let lines: Vec<&str> = rendered.lines().collect();
    if lines.is_empty() {
        return String::new();
    }
    let mut out = format!("{BRIGHT_GREEN}{agent_name} ›{RESET} {}", lines[0]);
    let indent = " ".repeat(agent_name.len() + 3);
    for line in &lines[1..] {
        out.push_str(&format!("\n{indent}{line}"));
    }
    out
}

fn print_error_raw(msg: &str) -> String {
    format!("{RED}error:{RESET} {msg}")
}

fn print_system_raw(msg: &str) -> String {
    format!("{YELLOW}{msg}{RESET}")
}

fn print_help_raw() -> String {
    format!(
        "{WHITE_BOLD}Commands:{RESET}\n\
         \x20 /project [name]      List projects or switch to one\n\
         \x20 /agent [@project] [n]  List/switch agents (@ for other project)\n\
         \x20 /new [agent@project]  Clear screen, new context\n\
         \x20 /conversations [@proj] Browse and open conversations\n\
         \x20 /boot [name]          Boot an offline project\n\
         \x20 /active              Active work across all projects\n\
         \x20 /status              Show current context\n\
         \x20 /help                Show this help\n\
         \x20 /quit                Exit"
    )
}

// ─── Tool Summary (ported from ConversationRenderPolicy.swift) ──────────────

const TOOL_DIM: &str = "\x1b[38;5;243m"; // muted gray for tool lines

fn is_tool_use(msg: &Message) -> bool {
    msg.tool_name.is_some() || !msg.q_tags.is_empty()
}

fn tool_summary(tool_name: Option<&str>, tool_args: Option<&str>, content: &str) -> String {
    let name = tool_name.unwrap_or("").to_lowercase();
    let args: serde_json::Value = tool_args
        .and_then(|s| serde_json::from_str(s).ok())
        .unwrap_or(serde_json::Value::Null);

    let get = |key: &str| -> Option<String> {
        args.get(key)
            .and_then(|v| v.as_str())
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };

    let truncate = |s: &str, max: usize| -> String {
        if s.len() <= max { s.to_string() } else { format!("{}...", &s[..max.saturating_sub(3)]) }
    };

    // TodoWrite
    if matches!(name.as_str(), "todo_write" | "todowrite" | "mcp__tenex__todo_write") {
        let count = args.get("todos").or(args.get("items"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        return format!("☑ {count} tasks");
    }

    match name.as_str() {
        "bash" | "execute_bash" | "shell" => {
            let target = get("description")
                .or_else(|| get("command").map(|c| truncate(&c, 50)));
            format!("$ {}", target.unwrap_or_default())
        }
        "ask" | "askuserquestion" => {
            let title = get("title").unwrap_or_else(|| "Question".into());
            format!("❓ {title}")
        }
        "read" | "file_read" | "fs_read" => {
            let target = extract_file_target(&args);
            format!("📖 {}", target.unwrap_or_default())
        }
        "write" | "file_write" | "fs_write" | "edit" | "str_replace_editor" | "fs_edit" => {
            let target = extract_file_target(&args);
            format!("✏️ {}", target.unwrap_or_default())
        }
        "glob" | "find" | "grep" | "search" | "web_search" | "websearch" | "fs_glob" | "fs_grep" => {
            let target = get("pattern").or_else(|| get("query"))
                .map(|s| format!("\"{}\"", truncate(&s, 30)));
            format!("🔍 {}", target.unwrap_or_default())
        }
        "task" | "agent" => {
            let desc = get("description").unwrap_or_else(|| "agent".into());
            format!("▶ {}", truncate(&desc, 40))
        }
        "change_model" => {
            let variant = get("variant").unwrap_or_else(|| "default".into());
            format!("🧠 → {variant}")
        }
        _ => {
            // Try description, then content fallback, then just the tool name
            if let Some(desc) = get("description") {
                truncate(&desc, 80)
            } else {
                let trimmed = content.trim();
                if !trimmed.is_empty() {
                    truncate(trimmed, 80)
                } else if !name.is_empty() {
                    name
                } else {
                    "tool".to_string()
                }
            }
        }
    }
}

fn extract_file_target(args: &serde_json::Value) -> Option<String> {
    for key in &["file_path", "path", "filePath", "file", "target"] {
        if let Some(val) = args.get(*key).and_then(|v| v.as_str()).filter(|s| !s.is_empty()) {
            let parts: Vec<&str> = val.split('/').filter(|s| !s.is_empty()).collect();
            if parts.len() > 2 {
                return Some(format!(".../{}",  parts[parts.len()-2..].join("/")));
            }
            return Some(val.to_string());
        }
    }
    None
}

fn print_tool_use_raw(msg: &Message) -> String {
    let summary = tool_summary(
        msg.tool_name.as_deref(),
        msg.tool_args.as_deref(),
        &msg.content,
    );
    format!("{TOOL_DIM}  {summary}{RESET}")
}

/// Format a message with all rendering rules:
/// - Tool use: muted single-line summary
/// - Consecutive dedup: skip header for same-author non-tool, non-ptag messages
/// - P-tags: show [from -> to] header
fn format_message(
    msg: &Message,
    store: &AppDataStore,
    user_pubkey: &str,
    last_pubkey: &mut Option<String>,
) -> Option<String> {
    if msg.is_reasoning {
        return None;
    }

    let is_user = msg.pubkey == user_pubkey;
    let has_p_tags = !msg.p_tags.is_empty();
    let is_tool = is_tool_use(msg);

    // Tool use messages — always render as muted summary
    if is_tool {
        *last_pubkey = Some(msg.pubkey.clone());
        return Some(print_tool_use_raw(msg));
    }

    // Determine if we should show the header
    let is_consecutive = !has_p_tags
        && last_pubkey.as_deref() == Some(&msg.pubkey);

    // Add blank line between message groups (when sender changes)
    let gap = if !is_consecutive && last_pubkey.is_some() { "\n" } else { "" };

    *last_pubkey = Some(msg.pubkey.clone());

    if is_user {
        if is_consecutive {
            let rendered = render_content_with_attachments(&msg.content);
            return Some(format!("      {}", rendered));
        }
        return Some(format!("{gap}{}", print_user_message_raw(&msg.content)));
    }

    // Agent message
    let name = store.get_profile_name(&msg.pubkey);

    let mut out = String::from(gap);

    if has_p_tags {
        // Colored envelope: [from → @to1, @to2]
        let recipients: Vec<String> = msg.p_tags.iter()
            .map(|pk| format!("@{}", store.get_profile_name(pk)))
            .collect();
        out.push_str(&format!(
            "{CYAN}[{BRIGHT_GREEN}{name}{CYAN} → {}{CYAN}]{RESET}",
            recipients.join(", ")
        ));
        out.push('\n');
        let colored = render_content_with_attachments(&msg.content);
        let indent = "  ";
        for line in colored.lines() {
            out.push_str(indent);
            out.push_str(line);
            out.push('\n');
        }
        // Remove trailing newline
        if out.ends_with('\n') {
            out.pop();
        }
    } else if is_consecutive {
        // Indent continuation (align with "agent_name › ")
        let indent = " ".repeat(name.len() + 3);
        let colored = render_content_with_attachments(&msg.content);
        for (i, line) in colored.lines().enumerate() {
            if i > 0 { out.push('\n'); }
            out.push_str(&indent);
            out.push_str(line);
        }
    } else {
        out.push_str(&print_agent_message_raw(&name, &msg.content));
    }

    Some(out)
}

// ─── Command Handlers ───────────────────────────────────────────────────────

fn handle_project_command(arg: Option<&str>, state: &mut ReplState, runtime: &CoreRuntime) -> Vec<String> {
    let mut output = Vec::new();
    let store = runtime.data_store();
    let store_ref = store.borrow();
    let projects: Vec<&Project> = store_ref
        .get_projects()
        .iter()
        .filter(|p| !p.is_deleted)
        .collect();

    match arg {
        None | Some("") => {
            if projects.is_empty() {
                output.push(print_system_raw("No projects found. Waiting for sync..."));
                return output;
            }
            output.push(format!("{WHITE_BOLD}Projects:{RESET}"));
            for (i, project) in projects.iter().enumerate() {
                let marker = if state.current_project.as_deref() == Some(&project.a_tag()) {
                    format!("{GREEN}*{RESET} ")
                } else {
                    "  ".to_string()
                };
                let online = if store_ref.is_project_online(&project.a_tag()) {
                    format!(" {GREEN}(online){RESET}")
                } else {
                    format!(" {DIM}(offline){RESET}")
                };
                output.push(format!("  {marker}{}: {}{online}", i + 1, project.title));
            }
        }
        Some(name) => {
            let matched = if let Ok(idx) = name.parse::<usize>() {
                projects.get(idx.saturating_sub(1)).copied()
            } else {
                let lower = name.to_lowercase();
                projects
                    .iter()
                    .find(|p| p.title.to_lowercase().contains(&lower))
                    .copied()
            };

            match matched {
                Some(project) => {
                    let a_tag = project.a_tag();
                    let title = project.title.clone();
                    drop(store_ref);

                    let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMessages {
                        project_a_tag: a_tag.clone(),
                    });
                    let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMetadata {
                        project_a_tag: a_tag.clone(),
                    });

                    state.switch_project(a_tag, runtime);
                    state.current_conversation = None;
                    state.last_displayed_pubkey = None;
                    auto_select_agent(state, runtime);
                    output.push(print_system_raw(&format!("Switched to project: {title}")));
                }
                None => output.push(print_error_raw(&format!("No project matching '{name}'"))),
            }
        }
    }
    output
}

/// Auto-select the first online project. Returns true if a project was selected.
fn auto_select_project(state: &mut ReplState, runtime: &CoreRuntime) -> bool {
    if state.current_project.is_some() {
        return false;
    }
    let store = runtime.data_store();
    let store_ref = store.borrow();
    let online_project = store_ref
        .get_projects()
        .iter()
        .filter(|p| !p.is_deleted)
        .find(|p| store_ref.is_project_online(&p.a_tag()))
        .map(|p| (p.a_tag(), p.title.clone()));
    drop(store_ref);

    if let Some((a_tag, title)) = online_project {
        let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMessages {
            project_a_tag: a_tag.clone(),
        });
        let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMetadata {
            project_a_tag: a_tag.clone(),
        });
        state.switch_project(a_tag, runtime);
        auto_select_agent(state, runtime);
        raw_println!("{}", print_system_raw(&format!("Auto-selected project: {title}")));
        true
    } else {
        false
    }
}

fn auto_select_agent(state: &mut ReplState, runtime: &CoreRuntime) {
    let Some(ref a_tag) = state.current_project else {
        return;
    };
    let store = runtime.data_store();
    let store_ref = store.borrow();
    if let Some(agents) = store_ref.get_online_agents(a_tag) {
        let agent = agents
            .iter()
            .find(|a| a.is_pm)
            .or_else(|| agents.first());
        if let Some(a) = agent {
            state.current_agent = Some(a.pubkey.clone());
            state.current_agent_name = Some(a.name.clone());
        }
    }
}

fn handle_agent_command(arg: Option<&str>, state: &mut ReplState, runtime: &CoreRuntime) -> Vec<String> {
    let mut output = Vec::new();

    // Check for @project prefix
    let agent_arg = if let Some(raw) = arg {
        if let Some(at_pos) = raw.find('@') {
            let after_at = raw[at_pos + 1..].trim();
            let (project_name, remainder) = match after_at.find(' ') {
                Some(sp) => (after_at[..sp].trim(), Some(after_at[sp + 1..].trim())),
                None => {
                    // Just "@project" with no agent index — list agents in that project
                    (after_at, None)
                }
            };

            // Switch project
            let store = runtime.data_store();
            let store_ref = store.borrow();
            let lower = project_name.to_lowercase();
            let matched = store_ref
                .get_projects()
                .iter()
                .filter(|p| !p.is_deleted)
                .find(|p| p.title.to_lowercase().contains(&lower))
                .map(|p| p.a_tag());
            drop(store_ref);

            match matched {
                Some(a_tag) => {
                    let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMessages {
                        project_a_tag: a_tag.clone(),
                    });
                    let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMetadata {
                        project_a_tag: a_tag.clone(),
                    });
                    state.switch_project(a_tag, runtime);
                    auto_select_agent(state, runtime);
                }
                None => {
                    output.push(print_error_raw(&format!("No project matching '{project_name}'")));
                    return output;
                }
            }

            remainder
        } else {
            arg
        }
    } else {
        arg
    };

    let Some(ref a_tag) = state.current_project else {
        output.push(print_error_raw("Select a project first with /project"));
        return output;
    };
    let store = runtime.data_store();
    let store_ref = store.borrow();
    let agents: Vec<&ProjectAgent> = store_ref
        .get_online_agents(a_tag)
        .map(|a| a.iter().collect())
        .unwrap_or_default();

    match agent_arg {
        None | Some("") => {
            if agents.is_empty() {
                output.push(print_system_raw("No online agents. Is the backend running?"));
                return output;
            }
            output.push(format!("{WHITE_BOLD}Online agents:{RESET}"));
            for (i, agent) in agents.iter().enumerate() {
                let marker = if state.current_agent.as_deref() == Some(&agent.pubkey) {
                    format!("{GREEN}*{RESET} ")
                } else {
                    "  ".to_string()
                };
                let model = agent.model.as_deref().unwrap_or("unknown model");
                let pm_badge = if agent.is_pm { " [PM]" } else { "" };
                output.push(format!(
                    "  {marker}{}: {}{pm_badge} ({DIM}{model}{RESET})",
                    i + 1,
                    agent.name
                ));
            }
        }
        Some(name) => {
            let matched = if let Ok(idx) = name.parse::<usize>() {
                agents.get(idx.saturating_sub(1)).copied()
            } else {
                let lower = name.to_lowercase();
                agents
                    .iter()
                    .find(|a| a.name.to_lowercase().contains(&lower))
                    .copied()
            };

            match matched {
                Some(agent) => {
                    state.current_agent = Some(agent.pubkey.clone());
                    state.current_agent_name = Some(agent.name.clone());
                    output.push(print_system_raw(&format!("Switched to agent: {}", agent.name)));
                }
                None => output.push(print_error_raw(&format!("No agent matching '{name}'"))),
            }
        }
    }
    output
}

fn handle_open_command(arg: Option<&str>, state: &mut ReplState, runtime: &CoreRuntime) -> CommandResult {
    let arg = arg.unwrap_or("");
    if arg.is_empty() {
        return CommandResult::ShowCompletion("/conversations ".to_string());
    }

    // Check for @project prefix
    let (project_switch, idx_str) = if let Some(at_pos) = arg.find('@') {
        let after_at = arg[at_pos + 1..].trim();
        match after_at.find(' ') {
            Some(sp) => (Some(after_at[..sp].trim()), after_at[sp + 1..].trim()),
            None => return CommandResult::ShowCompletion(format!("/conversations {arg}")),
        }
    } else {
        (None, arg.trim())
    };

    // Switch project if @project was specified
    if let Some(project_name) = project_switch {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        let lower = project_name.to_lowercase();
        let matched = store_ref
            .get_projects()
            .iter()
            .filter(|p| !p.is_deleted)
            .find(|p| p.title.to_lowercase().contains(&lower))
            .map(|p| p.a_tag());
        drop(store_ref);

        match matched {
            Some(a_tag) => {
                let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMessages {
                    project_a_tag: a_tag.clone(),
                });
                let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMetadata {
                    project_a_tag: a_tag.clone(),
                });
                state.switch_project(a_tag, runtime);
                auto_select_agent(state, runtime);
            }
            None => return CommandResult::Lines(vec![print_error_raw(&format!("No project matching '{project_name}'"))]),
        }
    }

    let Some(ref a_tag) = state.current_project else {
        return CommandResult::Lines(vec![print_error_raw("Select a project first with /project")]);
    };

    let Ok(idx) = idx_str.parse::<usize>() else {
        return CommandResult::Lines(vec![print_error_raw("Usage: /conversations <N> (number)")]);
    };

    let store = runtime.data_store();
    let store_ref = store.borrow();
    let mut threads: Vec<&Thread> = store_ref.get_threads(a_tag).iter().collect();
    threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));

    let Some(thread) = threads.get(idx.saturating_sub(1)) else {
        return CommandResult::Lines(vec![print_error_raw(&format!("No conversation at index {idx}"))]);
    };

    let id = thread.id.clone();
    let title = thread.title.clone();
    drop(store_ref);

    state.current_conversation = Some(id.clone());

    let mut output = Vec::new();
    output.push(print_system_raw(&format!("Opened: {title}")));

    let store = runtime.data_store();
    let store_ref = store.borrow();
    let messages = store_ref.get_messages(&id);

    if !messages.is_empty() {
        let mut last_pk: Option<String> = None;
        for msg in messages {
            if let Some(formatted) = format_message(msg, &store_ref, &state.user_pubkey, &mut last_pk) {
                output.push(formatted);
            }
        }
        state.last_displayed_pubkey = last_pk;
        output.push(print_separator_raw());
    }

    CommandResult::ClearScreen(output)
}

fn handle_active_command(arg: Option<&str>, state: &mut ReplState, runtime: &CoreRuntime) -> CommandResult {
    let arg = arg.unwrap_or("");
    if arg.is_empty() {
        return CommandResult::ShowCompletion("/active ".to_string());
    }

    let Ok(idx) = arg.trim().parse::<usize>() else {
        return CommandResult::Lines(vec![print_error_raw("Usage: /active <N> (number)")]);
    };

    // Rebuild the same ordered list as active_completion_items to find the right thread
    let store = runtime.data_store();
    let store_ref = store.borrow();

    let mut seen_ids: HashSet<String> = HashSet::new();
    let mut ordered_threads: Vec<(String, Option<String>)> = Vec::new(); // (thread_id, project_a_tag)

    // 1. Currently busy conversations first
    let active_ops = store_ref.operations.get_all_active_operations();
    for op in &active_ops {
        let thread_id = op.thread_id.as_deref().unwrap_or(&op.event_id);
        if !seen_ids.insert(thread_id.to_string()) {
            continue;
        }
        if store_ref.get_thread_by_id(thread_id).is_none() {
            seen_ids.remove(thread_id); // Don't count unknown threads
            continue;
        }
        let project_a_tag = store_ref.get_project_a_tag_for_thread(thread_id);
        ordered_threads.push((thread_id.to_string(), project_a_tag));
    }

    // 2. Recently active conversations across all projects
    let mut recent_threads: Vec<(&Thread, String)> = Vec::new();
    for project in store_ref.get_projects().iter().filter(|p| !p.is_deleted) {
        let a_tag = project.a_tag();
        for thread in store_ref.get_threads(&a_tag) {
            if !seen_ids.contains(&thread.id) {
                recent_threads.push((thread, a_tag.clone()));
            }
        }
    }
    recent_threads.sort_by(|a, b| b.0.effective_last_activity.cmp(&a.0.effective_last_activity));

    for (thread, a_tag) in recent_threads.into_iter().take(15) {
        if !seen_ids.insert(thread.id.clone()) {
            continue;
        }
        ordered_threads.push((thread.id.clone(), Some(a_tag)));
    }

    let Some((thread_id, project_a_tag)) = ordered_threads.get(idx.saturating_sub(1)) else {
        return CommandResult::Lines(vec![print_error_raw(&format!("No conversation at index {idx}"))]);
    };

    let thread_id = thread_id.clone();
    let Some(thread) = store_ref.get_thread_by_id(&thread_id) else {
        return CommandResult::Lines(vec![print_error_raw("Conversation not found")]);
    };
    let title = thread.title.clone();

    // Determine if we need to switch projects
    let needs_project_switch = project_a_tag
        .as_ref()
        .map(|a| state.current_project.as_ref() != Some(a))
        .unwrap_or(false);
    let switch_a_tag = project_a_tag.clone();

    drop(store_ref);

    // Switch project if needed
    if needs_project_switch {
        if let Some(a_tag) = &switch_a_tag {
            let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMessages {
                project_a_tag: a_tag.clone(),
            });
            let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMetadata {
                project_a_tag: a_tag.clone(),
            });
            state.switch_project(a_tag.clone(), runtime);
            auto_select_agent(state, runtime);
        }
    }

    state.current_conversation = Some(thread_id.clone());

    let mut output = Vec::new();
    output.push(print_system_raw(&format!("Opened: {title}")));

    let store = runtime.data_store();
    let store_ref = store.borrow();
    let messages = store_ref.get_messages(&thread_id);
    let show_count = messages.len().min(10);
    let start = messages.len().saturating_sub(show_count);

    if show_count > 0 {
        if start > 0 {
            output.push(print_system_raw(&format!("  ... {} earlier messages", start)));
        }
        let mut last_pk: Option<String> = None;
        for msg in &messages[start..] {
            if let Some(formatted) = format_message(msg, &store_ref, &state.user_pubkey, &mut last_pk) {
                output.push(formatted);
            }
        }
        state.last_displayed_pubkey = last_pk;
        output.push(print_separator_raw());
    }

    if needs_project_switch {
        CommandResult::ClearScreen(output)
    } else {
        CommandResult::Lines(output)
    }
}

fn handle_new_command(arg: &str, state: &mut ReplState, runtime: &CoreRuntime) -> CommandResult {
    let arg = arg.trim();

    if let Some(at_pos) = arg.find('@') {
        let agent_part = arg[..at_pos].trim();
        let after_at = arg[at_pos + 1..].trim();

        // Handle "@project agent" format (space after project)
        let (project_part, agent_override) = match after_at.find(' ') {
            Some(sp) => (&after_at[..sp], Some(after_at[sp + 1..].trim())),
            None => (after_at, None),
        };
        let agent_name = if !agent_part.is_empty() {
            agent_part
        } else {
            agent_override.unwrap_or("")
        };

        // Find and switch project
        if !project_part.is_empty() {
            let store = runtime.data_store();
            let store_ref = store.borrow();
            let projects: Vec<&Project> = store_ref
                .get_projects()
                .iter()
                .filter(|p| !p.is_deleted)
                .collect();
            let lower = project_part.to_lowercase();
            if let Some(project) = projects.iter().find(|p| p.title.to_lowercase().contains(&lower)) {
                let a_tag = project.a_tag();
                drop(store_ref);

                let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMessages {
                    project_a_tag: a_tag.clone(),
                });
                let _ = runtime.handle().send(NostrCommand::SubscribeToProjectMetadata {
                    project_a_tag: a_tag.clone(),
                });
                state.switch_project(a_tag, runtime);
                auto_select_agent(state, runtime);
            }
        }

        // Find and select agent by name
        if !agent_name.is_empty() {
            if let Some(ref a_tag) = state.current_project {
                let store = runtime.data_store();
                let store_ref = store.borrow();
                if let Some(agents) = store_ref.get_online_agents(a_tag) {
                    let lower = agent_name.to_lowercase();
                    if let Some(agent) = agents.iter().find(|a| a.name.to_lowercase().contains(&lower)) {
                        state.current_agent = Some(agent.pubkey.clone());
                        state.current_agent_name = Some(agent.name.clone());
                    }
                }
            }
        }
    } else if !arg.is_empty() {
        // Just agent name in current project
        if let Some(ref a_tag) = state.current_project {
            let store = runtime.data_store();
            let store_ref = store.borrow();
            if let Some(agents) = store_ref.get_online_agents(a_tag) {
                let lower = arg.to_lowercase();
                if let Some(agent) = agents.iter().find(|a| a.name.to_lowercase().contains(&lower)) {
                    state.current_agent = Some(agent.pubkey.clone());
                    state.current_agent_name = Some(agent.name.clone());
                }
            }
        }
    }

    state.current_conversation = None;
    state.last_displayed_pubkey = None;
    CommandResult::ClearScreen(vec![])
}

fn handle_send_message(content: &str, state: &mut ReplState, runtime: &CoreRuntime) -> Vec<String> {
    let Some(ref a_tag) = state.current_project else {
        return vec![print_error_raw("Select a project first with /project")];
    };

    // Auto-create a new conversation if none is active
    if state.current_conversation.is_none() {
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel(1);

        let _ = runtime.handle().send(NostrCommand::PublishThread {
            project_a_tag: a_tag.clone(),
            title: String::new(),
            content: content.to_string(),
            agent_pubkey: state.current_agent.clone(),
            nudge_ids: vec![],
            skill_ids: vec![],
            reference_conversation_id: None,
            reference_report_a_tag: None,
            fork_message_id: None,
            response_tx: Some(response_tx),
        });

        if let Ok(event_id) = response_rx.recv_timeout(std::time::Duration::from_secs(5)) {
            state.current_conversation = Some(event_id);
        }

        state.last_displayed_pubkey = Some(state.user_pubkey.clone());
        return vec![print_user_message_raw(content)];
    }

    let thread_id = state.current_conversation.as_ref().unwrap();

    let _ = runtime.handle().send(NostrCommand::PublishMessage {
        thread_id: thread_id.clone(),
        project_a_tag: a_tag.clone(),
        content: content.to_string(),
        agent_pubkey: state.current_agent.clone(),
        reply_to: None,
        nudge_ids: vec![],
        skill_ids: vec![],
        ask_author_pubkey: None,
        response_tx: None,
    });

    state.last_displayed_pubkey = Some(state.user_pubkey.clone());
    vec![print_user_message_raw(content)]
}

fn handle_boot_command(arg: Option<&str>, runtime: &CoreRuntime) -> Vec<String> {
    let arg = arg.unwrap_or("").trim();
    if arg.is_empty() {
        return vec![print_error_raw("Usage: /boot <project>")];
    }

    let store = runtime.data_store();
    let store_ref = store.borrow();
    let projects: Vec<&Project> = store_ref
        .get_projects()
        .iter()
        .filter(|p| !p.is_deleted)
        .collect();

    // Try numeric index first, then name match
    let matched = if let Ok(idx) = arg.parse::<usize>() {
        projects.get(idx.saturating_sub(1)).copied()
    } else {
        let lower = arg.to_lowercase();
        projects.iter().find(|p| p.title.to_lowercase().contains(&lower)).copied()
    };

    let Some(project) = matched else {
        return vec![print_error_raw(&format!("No project matching '{arg}'"))];
    };

    if store_ref.is_project_online(&project.a_tag()) {
        return vec![print_system_raw(&format!("{} is already online", project.title))];
    }

    let a_tag = project.a_tag();
    let pubkey = project.pubkey.clone();
    let title = project.title.clone();
    drop(store_ref);

    let _ = runtime.handle().send(NostrCommand::BootProject {
        project_a_tag: a_tag,
        project_pubkey: Some(pubkey),
    });

    vec![print_system_raw(&format!("Booting {}...", title))]
}

fn handle_status_command(state: &ReplState, runtime: &CoreRuntime) -> Vec<String> {
    let mut output = Vec::new();
    let project = state.project_display(runtime);
    let agent = state.agent_display();
    let conv = match &state.current_conversation {
        Some(id) => {
            let store = runtime.data_store();
            let store_ref = store.borrow();
            store_ref
                .get_thread_by_id(id)
                .map(|t| t.title.clone())
                .unwrap_or_else(|| format!("{}...", &id[..id.len().min(12)]))
        }
        None => "none".to_string(),
    };

    output.push(format!("{WHITE_BOLD}Status:{RESET}"));
    output.push(format!("  Project:      {CYAN}{project}{RESET}"));
    output.push(format!("  Agent:        {GREEN}{agent}{RESET}"));
    output.push(format!("  Conversation: {conv}"));

    if let Some(ref a_tag) = state.current_project {
        let store = runtime.data_store();
        let store_ref = store.borrow();
        let online = store_ref.is_project_online(a_tag);
        let status = if online {
            format!("{GREEN}online{RESET}")
        } else {
            format!("{RED}offline{RESET}")
        };
        output.push(format!("  Backend:      {status}"));
    }
    output
}

enum CommandResult {
    Lines(Vec<String>),
    /// Set the buffer to this string and show completion menu
    ShowCompletion(String),
    /// Clear the entire screen, optionally print lines, reposition input at bottom
    ClearScreen(Vec<String>),
}

// ─── Event Handlers ─────────────────────────────────────────────────────────

fn handle_core_event(event: &CoreEvent, state: &mut ReplState, runtime: &CoreRuntime) -> Option<String> {
    match event {
        CoreEvent::Message(msg) => {
            if msg.pubkey == state.user_pubkey {
                if state.current_conversation.is_none() {
                    let store = runtime.data_store();
                    let store_ref = store.borrow();
                    if let Some(thread) = store_ref.get_thread_by_id(&msg.thread_id) {
                        if thread.pubkey == state.user_pubkey {
                            drop(store_ref);
                            state.current_conversation = Some(msg.thread_id.clone());
                        }
                    }
                }
                return None;
            }

            if state.current_conversation.as_deref() != Some(&msg.thread_id) {
                return None;
            }

            if msg.is_reasoning {
                return None;
            }

            if state.streaming_in_progress {
                // Final message event arrives after streaming completes
                state.streaming_in_progress = false;
                state.stream_buffer.clear();
                state.last_displayed_pubkey = Some(msg.pubkey.clone());
                return Some(format!("\n{}", print_separator_raw()));
            }

            let store = runtime.data_store();
            let store_ref = store.borrow();
            let formatted = format_message(msg, &store_ref, &state.user_pubkey, &mut state.last_displayed_pubkey);
            drop(store_ref);

            formatted.map(|f| {
                if is_tool_use(msg) {
                    // Tool use: no separator
                    f
                } else {
                    format!("{f}\n{}", print_separator_raw())
                }
            })
        }
        _ => None,
    }
}

// ─── Main Loop ──────────────────────────────────────────────────────────────

// ─── Image Upload ───────────────────────────────────────────────────────────

enum UploadResult {
    Success(String),
    Error(String),
}

/// Decode percent-encoded file paths (e.g. from file:// URLs)
fn urlencoded_decode(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '%' {
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
                result.push('%');
                result.push_str(&hex);
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// Convert arboard ImageData (RGBA) to PNG bytes
fn image_to_png(image: &arboard::ImageData) -> anyhow::Result<Vec<u8>> {
    use std::io::Cursor;
    let width = image.width as u32;
    let height = image.height as u32;
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

/// Check if text is an image file path and upload it. Returns true if handled.
fn try_upload_image_file(
    text: &str,
    keys: &Keys,
    upload_tx: tokio::sync::mpsc::Sender<UploadResult>,
) -> bool {
    let path = text.trim();
    if path.is_empty() {
        return false;
    }

    let path = if let Some(file_path) = path.strip_prefix("file://") {
        urlencoded_decode(file_path)
    } else {
        path.replace("\\ ", " ")
    };

    let path_obj = std::path::Path::new(&path);
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

    if !path_obj.exists() {
        return false;
    }

    let data = match std::fs::read(&path) {
        Ok(data) => data,
        Err(_) => return false,
    };

    let keys = keys.clone();
    let mime_type = mime_type.to_string();
    tokio::spawn(async move {
        let result = match tenex_core::nostr::upload_image(&data, &keys, &mime_type).await {
            Ok(url) => UploadResult::Success(url),
            Err(e) => UploadResult::Error(format!("Upload failed: {}", e)),
        };
        let _ = upload_tx.send(result).await;
    });

    true
}

/// Handle Ctrl+V clipboard paste — try image first, then text
fn handle_clipboard_paste(
    editor: &mut LineEditor,
    keys: &Keys,
    upload_tx: tokio::sync::mpsc::Sender<UploadResult>,
    stdout: &mut Stdout,
    state: &ReplState,
    runtime: &CoreRuntime,
    completion: &mut CompletionMenu,
) {
    let mut clipboard = match arboard::Clipboard::new() {
        Ok(c) => c,
        Err(_) => return,
    };

    if let Ok(image) = clipboard.get_image() {
        let png_data = match image_to_png(&image) {
            Ok(data) => data,
            Err(e) => {
                let msg = print_error_raw(&format!("Failed to convert image: {}", e));
                print_above_input(stdout, &msg, state, runtime, editor, completion);
                return;
            }
        };

        let msg = print_system_raw("Uploading image...");
        print_above_input(stdout, &msg, state, runtime, editor, completion);

        let keys = keys.clone();
        let tx = upload_tx;
        tokio::spawn(async move {
            let result = match tenex_core::nostr::upload_image(&png_data, &keys, "image/png").await
            {
                Ok(url) => UploadResult::Success(url),
                Err(e) => UploadResult::Error(format!("Upload failed: {}", e)),
            };
            let _ = tx.send(result).await;
        });
    } else if let Ok(text) = clipboard.get_text() {
        if !try_upload_image_file(&text, keys, upload_tx) {
            editor.handle_paste(&text);
            redraw_input(stdout, state, runtime, editor, completion);
        } else {
            let msg = print_system_raw("Uploading image...");
            print_above_input(stdout, &msg, state, runtime, editor, completion);
        }
    }
}

// ─── Main Loop ──────────────────────────────────────────────────────────────

fn resolve_nsec(args: &Args) -> Result<String> {
    if let Some(ref nsec) = args.nsec {
        return Ok(nsec.clone());
    }
    if let Ok(nsec) = std::env::var("TENEX_NSEC") {
        if !nsec.is_empty() {
            return Ok(nsec);
        }
    }
    anyhow::bail!("No nsec provided. Use --nsec or set TENEX_NSEC env var.")
}

async fn run_repl(
    runtime: &mut CoreRuntime,
    data_rx: &Receiver<DataChange>,
    state: &mut ReplState,
    keys: &Keys,
) -> Result<()> {
    let mut stdout = io::stdout();
    let mut editor = LineEditor::new();
    let mut completion = CompletionMenu::new();
    let mut events = EventStream::new();
    let (upload_tx, mut upload_rx) = tokio::sync::mpsc::channel::<UploadResult>(8);

    // Initial sync delay
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Process any initial events
    if let Some(note_keys) = runtime.poll_note_keys() {
        let _ = runtime.process_note_keys(&note_keys);
    }

    // Auto-select the first online project (if any)
    auto_select_project(state, runtime);

    redraw_input(&mut stdout, state, runtime, &editor, &mut completion);

    let mut tick = tokio::time::interval(tokio::time::Duration::from_millis(50));
    tick.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);

    loop {
        tokio::select! {
            maybe_event = events.next() => {
                let Some(Ok(event)) = maybe_event else {
                    break;
                };

                match event {
                    Event::Paste(text) => {
                        // Check if it's an image file path (terminal drag-drop)
                        if try_upload_image_file(&text, keys, upload_tx.clone()) {
                            let msg = print_system_raw("Uploading image...");
                            print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion);
                        } else {
                            editor.handle_paste(&text);
                            if editor.has_attachments() {
                                let msg = format!("{DIM}(pasted as text attachment){RESET}");
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion);
                            } else {
                                redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                            }
                        }
                        continue;
                    }
                    Event::Key(KeyEvent { code, modifiers, kind, .. }) => {

                // Only handle key press events (not release/repeat)
                if kind != crossterm::event::KeyEventKind::Press {
                    continue;
                }

                // ── Backend approval prompt handling ──
                {
                    let store = runtime.data_store();
                    let first_pending = store.borrow().trust.pending_backend_approvals.first()
                        .map(|a| a.backend_pubkey.clone());
                    if let Some(backend_pk) = first_pending {
                        match code {
                            KeyCode::Char('a') | KeyCode::Char('y') | KeyCode::Enter => {
                                let mut store_ref = store.borrow_mut();
                                let name = store_ref.get_profile_name(&backend_pk);
                                store_ref.add_approved_backend(&backend_pk);
                                drop(store_ref);
                                let msg = print_system_raw(&format!("Approved backend: {}", name));
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion);
                                continue;
                            }
                            KeyCode::Char('b') | KeyCode::Char('n') => {
                                let mut store_ref = store.borrow_mut();
                                let name = store_ref.get_profile_name(&backend_pk);
                                store_ref.add_blocked_backend(&backend_pk);
                                drop(store_ref);
                                let msg = print_system_raw(&format!("Blocked backend: {}", name));
                                print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion);
                                continue;
                            }
                            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                                raw_println!();
                                raw_println!("{}", print_system_raw("Goodbye."));
                                return Ok(());
                            }
                            _ => {
                                continue;
                            }
                        }
                    }
                }

                match (code, modifiers) {
                    // Ctrl+C → quit
                    (KeyCode::Char('c'), m) if m.contains(KeyModifiers::CONTROL) => {
                        raw_println!();
                        raw_println!("{}", print_system_raw("Goodbye."));
                        return Ok(());
                    }

                    // Enter
                    (KeyCode::Enter, _) => {
                        if completion.visible {
                            if let Some(action) = completion.accept() {
                                match action {
                                    ItemAction::ReplaceFull(text) => {
                                        editor.set_buffer(&text);
                                        completion.update_from_buffer(&editor.buffer, state, runtime);
                                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                                        continue;
                                    }
                                    ItemAction::Submit(value) => {
                                        if editor.buffer.starts_with('@') {
                                            // @-prefix agent picker → route to /agent
                                            editor.set_buffer(&format!("/agent {value}"));
                                        } else {
                                            let cmd_part = match editor.buffer.find(' ') {
                                                Some(pos) => editor.buffer[..pos].to_string(),
                                                None => editor.buffer.clone(),
                                            };
                                            editor.set_buffer(&format!("{cmd_part} {value}"));
                                        }
                                        // Fall through to submit
                                    }
                                }
                            }
                        }
                        {
                            clear_input_area(&mut stdout, &mut completion);
                            completion.input_area_drawn = false;
                            // Submit line
                            let line = editor.submit();
                            raw_println!();

                            if line.trim().is_empty() {
                                redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                                continue;
                            }

                            let result = if line.starts_with('/') {
                                let (cmd, arg) = match line.find(' ') {
                                    Some(pos) => (&line[..pos], Some(line[pos + 1..].trim())),
                                    None => (line.as_str(), None),
                                };
                                match cmd {
                                    "/project" | "/p" => CommandResult::Lines(handle_project_command(arg, state, runtime)),
                                    "/agent" | "/a" => CommandResult::Lines(handle_agent_command(arg, state, runtime)),
                                    "/new" | "/n" => handle_new_command(arg.unwrap_or(""), state, runtime),
                                    "/conversations" | "/c" | "/open" | "/o" => handle_open_command(arg, state, runtime),
                                    "/active" => handle_active_command(arg, state, runtime),
                                    "/boot" | "/b" => CommandResult::Lines(handle_boot_command(arg, runtime)),
                                    "/status" | "/s" => CommandResult::Lines(handle_status_command(state, runtime)),
                                    "/help" | "/h" => CommandResult::Lines(vec![print_help_raw()]),
                                    "/quit" | "/q" => {
                                        raw_println!("{}", print_system_raw("Goodbye."));
                                        return Ok(());
                                    }
                                    _ => CommandResult::Lines(vec![print_error_raw(&format!("Unknown command: {cmd}. Type /help for commands."))]),
                                }
                            } else {
                                CommandResult::Lines(handle_send_message(&line, state, runtime))
                            };

                            match result {
                                CommandResult::Lines(output_lines) => {
                                    for l in &output_lines {
                                        raw_println!("{}", l);
                                    }
                                    if !state.streaming_in_progress {
                                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                                    }
                                }
                                CommandResult::ShowCompletion(buf) => {
                                    editor.set_buffer(&buf);
                                    completion.update_from_buffer(&editor.buffer, state, runtime);
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                                }
                                CommandResult::ClearScreen(lines) => {
                                    execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0)).ok();
                                    completion.input_area_drawn = false;
                                    let (_, rows) = terminal::size().unwrap_or((80, 24));
                                    // Position so lines + input area fit above the bottom
                                    let content_rows = lines.len() as u16;
                                    let start = rows.saturating_sub(5 + content_rows);
                                    execute!(stdout, cursor::MoveTo(0, start)).ok();
                                    for l in &lines {
                                        raw_println!("{}", l);
                                    }
                                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                                }
                            }
                        }
                    }

                    // Tab → apply selected completion to buffer, or trigger menu
                    (KeyCode::Tab, m) if !m.contains(KeyModifiers::SHIFT) => {
                        if completion.visible && !completion.items.is_empty() {
                            // Apply currently selected item to buffer
                            let item = &completion.items[completion.selected];
                            match &item.action {
                                ItemAction::ReplaceFull(text) => {
                                    editor.set_buffer(text);
                                }
                                ItemAction::Submit(value) => {
                                    let cmd_part = match editor.buffer.find(' ') {
                                        Some(pos) => editor.buffer[..pos].to_string(),
                                        None => editor.buffer.clone(),
                                    };
                                    editor.set_buffer(&format!("{cmd_part} {value}"));
                                }
                            }
                            completion.select_next();
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                        } else if editor.buffer.starts_with('/') || editor.buffer.starts_with('@') {
                            completion.update_from_buffer(&editor.buffer, state, runtime);
                            // Apply first match immediately
                            if !completion.items.is_empty() {
                                let item = &completion.items[completion.selected];
                                match &item.action {
                                    ItemAction::ReplaceFull(text) => {
                                        editor.set_buffer(text);
                                    }
                                    ItemAction::Submit(value) => {
                                        let cmd_part = match editor.buffer.find(' ') {
                                            Some(pos) => editor.buffer[..pos].to_string(),
                                            None => editor.buffer.clone(),
                                        };
                                        editor.set_buffer(&format!("{cmd_part} {value}"));
                                    }
                                }
                                completion.select_next();
                            }
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                        }
                    }

                    // Shift+Tab → prev completion
                    (KeyCode::BackTab, _) => {
                        if completion.visible && !completion.items.is_empty() {
                            completion.select_prev();
                            let item = &completion.items[completion.selected];
                            match &item.action {
                                ItemAction::ReplaceFull(text) => editor.set_buffer(text),
                                ItemAction::Submit(value) => {
                                    let cmd = match editor.buffer.find(' ') {
                                        Some(p) => editor.buffer[..p].to_string(),
                                        None => editor.buffer.clone(),
                                    };
                                    editor.set_buffer(&format!("{cmd} {value}"));
                                }
                            }
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                        }
                    }

                    // Escape → hide menu / deselect attachment
                    (KeyCode::Esc, _) => {
                        if editor.selected_attachment.is_some() {
                            editor.selected_attachment = None;
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                        } else if completion.visible {
                            completion.hide();
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                        }
                    }

                    // Up arrow
                    (KeyCode::Up, _) => {
                        if editor.selected_attachment.is_some() {
                            editor.selected_attachment = None;
                        } else if completion.visible {
                            completion.select_prev();
                        } else {
                            editor.history_up();
                        }
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Down arrow
                    (KeyCode::Down, _) => {
                        if !completion.visible && editor.selected_attachment.is_none()
                            && editor.cursor == editor.buffer.len() && editor.has_attachments()
                        {
                            editor.selected_attachment = Some(0);
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                        } else if completion.visible {
                            completion.select_next();
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                        } else {
                            editor.history_down();
                            redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                        }
                    }

                    // Backspace
                    (KeyCode::Backspace, m) if !m.contains(KeyModifiers::ALT) => {
                        if let Some(idx) = editor.selected_attachment {
                            editor.remove_attachment(idx);
                        } else if let Some(att_idx) = editor.marker_before_cursor() {
                            editor.selected_attachment = Some(att_idx);
                        } else {
                            editor.delete_back();
                        }
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Delete
                    (KeyCode::Delete, _) => {
                        if let Some(idx) = editor.selected_attachment {
                            editor.remove_attachment(idx);
                        } else {
                            editor.delete_forward();
                        }
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Left
                    (KeyCode::Left, m) if !m.contains(KeyModifiers::ALT) => {
                        if let Some(idx) = editor.selected_attachment {
                            editor.selected_attachment = Some(idx.saturating_sub(1));
                        } else {
                            editor.move_left();
                        }
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Right
                    (KeyCode::Right, m) if !m.contains(KeyModifiers::ALT) => {
                        if let Some(idx) = editor.selected_attachment {
                            let max = editor.attachments.len().saturating_sub(1);
                            editor.selected_attachment = Some((idx + 1).min(max));
                        } else {
                            editor.move_right();
                        }
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Home / Ctrl+A
                    (KeyCode::Home, _) => {
                        editor.move_home();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }
                    (KeyCode::Char('a'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_home();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // End / Ctrl+E
                    (KeyCode::End, _) => {
                        editor.move_end();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }
                    (KeyCode::Char('e'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_end();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Ctrl+W → delete word back
                    (KeyCode::Char('w'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.delete_word_back();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Ctrl+U → clear line
                    (KeyCode::Char('u'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.set_buffer("");
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Ctrl+K → kill to end of line
                    (KeyCode::Char('k'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.kill_to_end();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Ctrl+D → delete forward (or quit on empty)
                    (KeyCode::Char('d'), m) if m.contains(KeyModifiers::CONTROL) => {
                        if editor.buffer.is_empty() {
                            raw_println!();
                            raw_println!("{}", print_system_raw("Goodbye."));
                            return Ok(());
                        }
                        editor.delete_forward();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Ctrl+B → move left
                    (KeyCode::Char('b'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_left();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Ctrl+F → move right
                    (KeyCode::Char('f'), m) if m.contains(KeyModifiers::CONTROL) => {
                        editor.move_right();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Ctrl+L → clear screen
                    (KeyCode::Char('l'), m) if m.contains(KeyModifiers::CONTROL) => {
                        execute!(stdout, terminal::Clear(ClearType::All), cursor::MoveTo(0, 0)).ok();
                        completion.input_area_drawn = false;
                        let (_, rows) = terminal::size().unwrap_or((80, 24));
                        execute!(stdout, cursor::MoveTo(0, rows.saturating_sub(5))).ok();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Alt+B / Alt+Left → word left
                    (KeyCode::Char('b'), m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_left();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }
                    (KeyCode::Left, m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_left();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Alt+F / Alt+Right → word right
                    (KeyCode::Char('f'), m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_right();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }
                    (KeyCode::Right, m) if m.contains(KeyModifiers::ALT) => {
                        editor.move_word_right();
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Alt+D → delete word forward
                    (KeyCode::Char('d'), m) if m.contains(KeyModifiers::ALT) => {
                        editor.delete_word_forward();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Alt+Backspace → delete word back
                    (KeyCode::Backspace, m) if m.contains(KeyModifiers::ALT) => {
                        editor.delete_word_back();
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Regular character
                    (KeyCode::Char(c), m) if !m.contains(KeyModifiers::CONTROL) && !m.contains(KeyModifiers::ALT) => {
                        editor.selected_attachment = None;
                        editor.insert_char(c);
                        completion.update_from_buffer(&editor.buffer, state, runtime);
                        redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                    }

                    // Ctrl+V → clipboard paste (image or text)
                    (KeyCode::Char('v'), m) if m.contains(KeyModifiers::CONTROL) => {
                        handle_clipboard_paste(&mut editor, keys, upload_tx.clone(), &mut stdout, state, runtime, &mut completion);
                    }

                    _ => {}
                }
                } // end Event::Key
                    _ => {}
                } // end match event
            }

            // Upload result channel
            Some(result) = upload_rx.recv() => {
                match result {
                    UploadResult::Success(url) => {
                        let id = editor.add_image_attachment(url);
                        let msg = print_system_raw(&format!("Image uploaded → [Image #{}]", id));
                        print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion);
                    }
                    UploadResult::Error(err) => {
                        let msg = print_error_raw(&format!("Image upload failed: {}", err));
                        print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion);
                    }
                }
            }

            note_keys = runtime.next_note_keys() => {
                if let Some(note_keys) = note_keys {
                    match runtime.process_note_keys(&note_keys) {
                        Ok(events) => {
                            for event in &events {
                                if let Some(text) = handle_core_event(event, state, runtime) {
                                    if state.streaming_in_progress {
                                        // During streaming, just print raw
                                        raw_println!("{}", text);
                                    } else {
                                        print_above_input(&mut stdout, &text, state, runtime, &editor, &mut completion);
                                    }
                                }
                            }
                        }
                        Err(e) => {
                            let msg = print_error_raw(&format!("Event processing error: {e}"));
                            print_above_input(&mut stdout, &msg, state, runtime, &editor, &mut completion);
                        }
                    }
                }
            }

            _ = tick.tick() => {
                drain_data_changes(data_rx, state, runtime, &mut stdout, &editor, &mut completion);
                state.wave_frame = state.wave_frame.wrapping_add(1);
                let agents_active = state.has_active_agents(runtime);
                if (state.is_animating() || agents_active) && !state.streaming_in_progress {
                    redraw_input(&mut stdout, state, runtime, &editor, &mut completion);
                }
            }
        }
    }

    Ok(())
}

fn drain_data_changes(
    data_rx: &Receiver<DataChange>,
    state: &mut ReplState,
    runtime: &CoreRuntime,
    stdout: &mut Stdout,
    editor: &LineEditor,
    completion: &mut CompletionMenu,
) {
    loop {
        match data_rx.try_recv() {
            Ok(DataChange::LocalStreamChunk {
                conversation_id,
                text_delta,
                is_finish,
                ..
            }) => {
                if state.current_conversation.as_deref() != Some(&conversation_id) {
                    continue;
                }

                if let Some(delta) = &text_delta {
                    if !state.streaming_in_progress {
                        state.streaming_in_progress = true;
                        state.stream_buffer.clear();

                        // Clear input area
                        clear_input_area(stdout, completion);
                        completion.input_area_drawn = false;

                        // Check if consecutive from same agent
                        let is_consecutive = state.last_displayed_pubkey.as_deref() == state.current_agent.as_deref()
                            && state.current_agent.is_some();

                        if is_consecutive {
                            let agent_name = state.agent_display();
                            let indent = " ".repeat(agent_name.len() + 3);
                            write!(stdout, "{indent}").ok();
                        } else {
                            let agent_name = state.agent_display();
                            write!(stdout, "{BRIGHT_GREEN}{agent_name} ›{RESET} ").ok();
                        }
                    }
                    write!(stdout, "{delta}").ok();
                    stdout.flush().ok();
                    state.stream_buffer.push_str(delta);
                }

                if is_finish && state.streaming_in_progress {
                    // Rewrite streamed content with markdown colors:
                    // Count lines we printed (header line + content lines)
                    let raw_lines = state.stream_buffer.lines().count().max(1);
                    // Move cursor back to start of agent output
                    queue!(stdout, cursor::MoveUp(raw_lines as u16 - 1)).ok();
                    write!(stdout, "\r").ok();
                    for _ in 0..raw_lines {
                        queue!(stdout, terminal::Clear(ClearType::CurrentLine)).ok();
                        write!(stdout, "\r\n").ok();
                    }
                    queue!(stdout, cursor::MoveUp(raw_lines as u16)).ok();
                    write!(stdout, "\r").ok();

                    // Reprint with colors, respecting consecutive dedup
                    let agent_name = state.agent_display();
                    let is_consecutive = state.last_displayed_pubkey.as_deref() == state.current_agent.as_deref()
                        && state.current_agent.is_some();

                    if is_consecutive {
                        let indent = " ".repeat(agent_name.len() + 3);
                        let colored = colorize_markdown(&state.stream_buffer);
                        for line in colored.lines() {
                            write!(stdout, "{indent}{line}\r\n").ok();
                        }
                    } else {
                        let colored = print_agent_message_raw(&agent_name, &state.stream_buffer);
                        for line in colored.lines() {
                            write!(stdout, "{line}\r\n").ok();
                        }
                    }
                    stdout.flush().ok();

                    // Update last_displayed_pubkey for the agent
                    if let Some(ref pk) = state.current_agent {
                        state.last_displayed_pubkey = Some(pk.clone());
                    }

                    raw_println!("{}", print_separator_raw());
                    state.streaming_in_progress = false;
                    state.stream_buffer.clear();

                    redraw_input(stdout, state, runtime, editor, completion);
                }
            }
            Ok(DataChange::ProjectStatus { json }) => {
                // Parse kind to detect 24133 (operations status) for activity notifications
                let kind = serde_json::from_str::<serde_json::Value>(&json)
                    .ok()
                    .and_then(|v| v.get("kind")?.as_u64());

                let store = runtime.data_store();
                let mut store_ref = store.borrow_mut();
                store_ref.handle_status_event_json(&json);

                // Redraw if there are pending backend approvals to show
                let has_pending = store_ref.trust.has_pending_approvals();

                // kind:24133 — agent activity updates the status bar on redraw
                let mut needs_redraw = kind == Some(24133) || has_pending;

                drop(store_ref);

                // Auto-select a project when one comes online and none is selected
                if state.current_project.is_none() {
                    if auto_select_project(state, runtime) {
                        needs_redraw = true;
                    }
                }

                if needs_redraw && !state.streaming_in_progress {
                    redraw_input(stdout, state, runtime, editor, completion);
                }
            }
            Ok(_) => {}
            Err(std::sync::mpsc::TryRecvError::Empty) => break,
            Err(std::sync::mpsc::TryRecvError::Disconnected) => break,
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();

    let nsec = resolve_nsec(&args)?;
    let secret_key = SecretKey::parse(&nsec)?;
    let keys = Keys::new(secret_key);
    let user_pubkey = get_current_pubkey(&keys);

    let config = CoreConfig::default();
    let mut runtime = CoreRuntime::new(config)?;
    let handle = runtime.handle();

    let data_rx = runtime
        .take_data_rx()
        .ok_or_else(|| anyhow::anyhow!("Core runtime already has active data receiver"))?;

    runtime
        .data_store()
        .borrow_mut()
        .apply_authenticated_user(user_pubkey.clone());

    // Clone keys before Connect (which moves them)
    let keys_for_repl = keys.clone();

    // Connect to relays
    println!("{DIM}Connecting...{RESET}");
    let (response_tx, response_rx) = std::sync::mpsc::channel();
    handle.send(NostrCommand::Connect {
        keys,
        user_pubkey: user_pubkey.clone(),
        relay_urls: vec![],
        response_tx: Some(response_tx),
    })?;

    match response_rx.recv_timeout(std::time::Duration::from_secs(15)) {
        Ok(Ok(())) => {
            println!("{GREEN}Connected.{RESET}");
        }
        Ok(Err(e)) => {
            anyhow::bail!("Connection failed: {e}");
        }
        Err(_) => {
            anyhow::bail!("Connection timed out after 15s");
        }
    }

    println!();
    println!("{WHITE_BOLD}tenex-repl{RESET} {DIM}— type /help for commands{RESET}");
    println!();

    let mut state = ReplState::new(user_pubkey);

    // Install panic hook to restore terminal
    let default_panic = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(
            io::stdout(),
            cursor::Show,
            crossterm::event::DisableBracketedPaste
        );
        default_panic(info);
    }));

    // Enable raw mode + bracketed paste
    terminal::enable_raw_mode()?;
    execute!(io::stdout(), crossterm::event::EnableBracketedPaste)?;

    let result = run_repl(&mut runtime, &data_rx, &mut state, &keys_for_repl).await;

    // Restore terminal
    terminal::disable_raw_mode()?;
    execute!(
        io::stdout(),
        cursor::Show,
        crossterm::event::DisableBracketedPaste
    )?;

    runtime.shutdown();
    result
}
