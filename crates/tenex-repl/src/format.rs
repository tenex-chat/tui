use chrono::Local;
use crate::{DIM, GREEN, YELLOW, WHITE_BOLD, BRIGHT_GREEN, RED, CYAN, RESET};
use crate::markdown::{colorize_markdown, CODE_BLOCK};
use crate::util::term_width;
use tenex_core::models::Message;
use tenex_core::store::app_data_store::AppDataStore;

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

    let label = header.trim_matches('-').trim();
    let line_info = format!(" ({} lines)", total);
    let header_text = format!(" {} {}", label, line_info);
    let header_pad = box_width.saturating_sub(header_text.len() + 2);
    let mut out = format!(
        "{DIM}╭─{}{}{RESET}",
        header_text,
        "─".repeat(header_pad)
    );

    for line in lines.iter().take(preview_count) {
        let truncated = if line.len() > box_width - 4 {
            format!("{}...", &line[..box_width.saturating_sub(7)])
        } else {
            line.to_string()
        };
        let pad = box_width.saturating_sub(truncated.len() + 4);
        out.push_str(&format!("\n{DIM}│{RESET} {CODE_BLOCK}{truncated}{RESET}{}", " ".repeat(pad)));
    }

    if total > preview_count {
        let more = format!("... ({} more lines)", total - preview_count);
        let pad = box_width.saturating_sub(more.len() + 4);
        out.push_str(&format!("\n{DIM}│ {more}{}{RESET}", " ".repeat(pad)));
    }

    out.push_str(&format!("\n{DIM}╰{}╯{RESET}", "─".repeat(box_width.saturating_sub(2))));

    out
}

/// Replace [Text Attachment N] markers in body with styled references.
fn style_attachment_markers(body: &str) -> String {
    let mut result = body.to_string();
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

pub(crate) fn print_separator_raw() -> String {
    let time = Local::now().format("%H:%M").to_string();
    let line = "─".repeat(40);
    format!("{DIM}{line} {time}{RESET}")
}

pub(crate) fn print_user_message_raw(content: &str) -> String {
    let rendered = render_content_with_attachments(content);
    format!("{WHITE_BOLD}you ›{RESET} {rendered}")
}

pub(crate) fn print_agent_message_raw(agent_name: &str, content: &str) -> String {
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

pub(crate) fn print_error_raw(msg: &str) -> String {
    format!("{RED}error:{RESET} {msg}")
}

pub(crate) fn print_system_raw(msg: &str) -> String {
    format!("{YELLOW}{msg}{RESET}")
}

pub(crate) fn print_help_raw() -> String {
    format!(
        "{WHITE_BOLD}Commands:{RESET}\n\
         \x20 /project [name]       List projects or switch to one\n\
         \x20 /agent [@project] [n] List/switch agents (@ for other project)\n\
         \x20 /new [agent@project]  Clear screen, new context\n\
         \x20 /conversations [@proj] Browse and open conversations\n\
         \x20 /config [--model|--make-pm|--global] [agent]\n\
         \x20                       Configure agent tools or model\n\
         \x20 /model [agent]        Change agent model (shortcut)\n\
         \x20 /boot [name]          Boot an offline project\n\
         \x20 /active               Active work across all projects\n\
         \x20 /status               Show current context\n\
         \x20 /help                 Show this help\n\
         \x20 /quit                 Exit"
    )
}

// ─── Tool Summary ───────────────────────────────────────────────────────────

const TOOL_DIM: &str = "\x1b[38;5;243m";

pub(crate) fn is_tool_use(msg: &Message) -> bool {
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

/// Render a todo_write call inline with smart diffing.
/// Returns (formatted_output, new_todo_state).
fn format_todo_inline(
    args: &serde_json::Value,
    prev_items: &[(String, String)],
) -> (String, Vec<(String, String)>) {
    let items = args
        .get("todos")
        .or_else(|| args.get("items"))
        .and_then(|v| v.as_array());

    let items = match items {
        Some(arr) => arr,
        None => return (format!("{TOOL_DIM}  ☑ 0 tasks{RESET}"), Vec::new()),
    };

    let current: Vec<(String, String)> = items
        .iter()
        .map(|item| {
            let title = item
                .get("content")
                .or_else(|| item.get("title"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let status = item
                .get("status")
                .and_then(|v| v.as_str())
                .unwrap_or("pending")
                .to_string();
            (title, status)
        })
        .collect();

    let total = current.len();
    let done = current.iter().filter(|(_, s)| s == "completed").count();

    let truncate_title = |s: &str| -> String {
        if s.len() <= 50 {
            s.to_string()
        } else {
            format!("{}...", &s[..47])
        }
    };

    let status_icon = |s: &str| -> (&str, &str) {
        match s {
            "completed" => ("✓", GREEN),
            "in_progress" => ("◒", YELLOW),
            "cancelled" | "skipped" => ("✗", DIM),
            _ => ("◯", DIM),
        }
    };

    let mut lines = Vec::new();

    let header = if done > 0 {
        format!("{TOOL_DIM}  ☑ {done}/{total} tasks{RESET}")
    } else {
        format!("{TOOL_DIM}  ☑ {total} tasks{RESET}")
    };
    lines.push(header);

    let prev_map: std::collections::HashMap<&str, &str> = prev_items
        .iter()
        .map(|(t, s)| (t.as_str(), s.as_str()))
        .collect();

    let is_first = prev_items.is_empty();

    for (title, status) in &current {
        let (icon, color) = status_icon(status);
        let display_title = truncate_title(title);

        if is_first {
            lines.push(format!("{color}    {icon} {display_title}{RESET}"));
        } else {
            let prev_status = prev_map.get(title.as_str()).copied();
            let changed = prev_status != Some(status.as_str());
            let is_active = status == "in_progress";

            if changed || is_active {
                lines.push(format!("{color}    {icon} {display_title}{RESET}"));
            }
        }
    }

    (lines.join("\n"), current)
}

/// Format a message with all rendering rules:
/// - Tool use: muted single-line summary
/// - Consecutive dedup: skip header for same-author non-tool, non-ptag messages
/// - P-tags: show [from -> to] header
pub(crate) fn format_message(
    msg: &Message,
    store: &AppDataStore,
    user_pubkey: &str,
    last_pubkey: &mut Option<String>,
    todo_items: &mut Vec<(String, String)>,
) -> Option<String> {
    if msg.is_reasoning {
        return None;
    }

    let is_user = msg.pubkey == user_pubkey;
    let has_p_tags = !msg.p_tags.is_empty();
    let is_tool = is_tool_use(msg);

    if is_tool {
        *last_pubkey = Some(msg.pubkey.clone());

        let tool_name = msg.tool_name.as_deref().unwrap_or("").to_lowercase();
        if matches!(tool_name.as_str(), "todo_write" | "todowrite" | "mcp__tenex__todo_write") {
            if let Some(args_str) = &msg.tool_args {
                if let Ok(args) = serde_json::from_str::<serde_json::Value>(args_str) {
                    let (formatted, new_items) = format_todo_inline(&args, todo_items);
                    *todo_items = new_items;
                    return Some(formatted);
                }
            }
        }

        return Some(print_tool_use_raw(msg));
    }

    let is_consecutive = !has_p_tags
        && last_pubkey.as_deref() == Some(&msg.pubkey);

    let gap = if !is_consecutive && last_pubkey.is_some() { "\n" } else { "" };

    *last_pubkey = Some(msg.pubkey.clone());

    if is_user {
        if is_consecutive {
            let rendered = render_content_with_attachments(&msg.content);
            return Some(format!("      {}", rendered));
        }
        return Some(format!("{gap}{}", print_user_message_raw(&msg.content)));
    }

    let name = store.get_profile_name(&msg.pubkey);

    let mut out = String::from(gap);

    if has_p_tags {
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
        if out.ends_with('\n') {
            out.pop();
        }
    } else if is_consecutive {
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
