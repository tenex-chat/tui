use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use serde::{Deserialize, Serialize};

use crate::ui::theme;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
    #[serde(default)]
    pub result: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallGroup {
    #[serde(default)]
    pub tool_calls: Vec<ToolCall>,
}

pub enum MessageContent {
    PlainText(String),
    Mixed {
        text_parts: Vec<String>,
        tool_calls: Vec<ToolCall>,
    },
}

pub fn parse_message_content(content: &str) -> MessageContent {
    let mut tool_calls = Vec::new();
    let mut text_parts = Vec::new();
    let mut current_text = String::new();

    let mut chars = content.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch == '{' {
            let mut json_str = String::from("{");
            let mut brace_count = 1;
            let mut in_string = false;
            let mut escape_next = false;

            while let Some(next_ch) = chars.next() {
                json_str.push(next_ch);

                if escape_next {
                    escape_next = false;
                    continue;
                }

                match next_ch {
                    '\\' => escape_next = true,
                    '"' => in_string = !in_string,
                    '{' if !in_string => brace_count += 1,
                    '}' if !in_string => {
                        brace_count -= 1;
                        if brace_count == 0 {
                            break;
                        }
                    }
                    _ => {}
                }
            }

            if let Ok(tool_call) = serde_json::from_str::<ToolCall>(&json_str) {
                if !tool_call.name.is_empty() {
                    if !current_text.trim().is_empty() {
                        text_parts.push(current_text.clone());
                        current_text.clear();
                    }
                    tool_calls.push(tool_call);
                    continue;
                }
            }

            if let Ok(group) = serde_json::from_str::<ToolCallGroup>(&json_str) {
                if !group.tool_calls.is_empty() {
                    if !current_text.trim().is_empty() {
                        text_parts.push(current_text.clone());
                        current_text.clear();
                    }
                    tool_calls.extend(group.tool_calls);
                    continue;
                }
            }

            current_text.push_str(&json_str);
        } else {
            current_text.push(ch);
        }
    }

    if !current_text.trim().is_empty() {
        text_parts.push(current_text);
    }

    if tool_calls.is_empty() {
        MessageContent::PlainText(content.to_string())
    } else {
        MessageContent::Mixed {
            text_parts,
            tool_calls,
        }
    }
}

/// Get icon for a tool call based on its name
pub fn tool_icon(name: &str) -> &'static str {
    match name.to_lowercase().as_str() {
        "edit" | "str_replace_editor" => "✏️",
        "write" | "file_write" => "📝",
        "read" | "file_read" => "📖",
        "bash" | "execute_bash" | "shell" => "⚡",
        "glob" | "find" => "🔍",
        "grep" | "search" => "🔎",
        "task" | "agent" => "🤖",
        "web_search" | "websearch" => "🌐",
        "todowrite" | "todo" => "📋",
        _ => "⚙️",
    }
}

/// Extract a meaningful target/file from tool parameters
pub fn extract_target(tool_call: &ToolCall) -> Option<String> {
    let params = &tool_call.parameters;

    // Try common parameter names for file paths
    for key in ["file_path", "path", "filePath", "file", "target"] {
        if let Some(val) = params.get(key).and_then(|v| v.as_str()) {
            // Shorten long paths - show last 2 components
            let parts: Vec<&str> = val.split('/').collect();
            if parts.len() > 2 {
                return Some(format!(".../{}", parts[parts.len()-2..].join("/")));
            }
            return Some(val.to_string());
        }
    }

    // For bash commands, show the command
    if let Some(cmd) = params.get("command").and_then(|v| v.as_str()) {
        let truncated: String = cmd.chars().take(40).collect();
        if cmd.len() > 40 {
            return Some(format!("{}...", truncated));
        }
        return Some(truncated);
    }

    // For search/grep, show the pattern
    if let Some(pattern) = params.get("pattern").and_then(|v| v.as_str()) {
        let truncated: String = pattern.chars().take(30).collect();
        if pattern.len() > 30 {
            return Some(format!("\"{}...\"", truncated));
        }
        return Some(format!("\"{}\"", truncated));
    }

    None
}

/// Render a single tool call as a compact single line
#[allow(dead_code)]
pub fn render_tool_call_compact(tool_call: &ToolCall) -> Line<'static> {
    let icon = tool_icon(&tool_call.name);
    let target = extract_target(tool_call);

    let mut spans = vec![
        Span::styled("  ", Style::default()), // indent
        Span::styled(format!("{} ", icon), Style::default()),
        Span::styled(
            tool_call.name.clone(),
            theme::tool_name().add_modifier(Modifier::BOLD),
        ),
    ];

    if let Some(t) = target {
        spans.push(Span::styled(" ", Style::default()));
        spans.push(Span::styled(t, theme::tool_target()));
    }

    Line::from(spans)
}

/// Render a single tool call - returns multiple lines for detailed view
#[allow(dead_code)]
fn render_tool_call_detailed(tool_call: &ToolCall) -> Vec<Line<'static>> {
    // Detailed rendering with box drawing (unused for now, keeping for future)
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        format!("┌─ {} ─────────────────────────────────────", tool_call.name),
        theme::tool_name(),
    )));

    if let Some(target) = extract_target(tool_call) {
        lines.push(Line::from(vec![
            Span::styled("│ ", theme::tool_name()),
            Span::styled(target, theme::tool_target()),
        ]));
    }

    lines.push(Line::from(Span::styled(
        "└─────────────────────────────────────────────────",
        theme::tool_name(),
    )));

    lines
}

#[allow(dead_code)]
pub fn render_tool_calls_group(tool_calls: &[ToolCall]) -> Vec<Line<'static>> {
    // Render each tool call as a compact line
    tool_calls.iter().map(render_tool_call_compact).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_plain_text() {
        let content = "Hello, world!";
        match parse_message_content(content) {
            MessageContent::PlainText(text) => assert_eq!(text, "Hello, world!"),
            _ => panic!("Expected PlainText"),
        }
    }

    #[test]
    fn test_parse_tool_call() {
        let content = r#"Here is a tool call: {"name": "search", "parameters": {"query": "test"}}"#;
        match parse_message_content(content) {
            MessageContent::Mixed { text_parts: _, tool_calls } => {
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].name, "search");
            }
            _ => panic!("Expected Mixed content"),
        }
    }

    #[test]
    fn test_render_tool_call() {
        let tool_call = ToolCall {
            id: "123".to_string(),
            name: "test_tool".to_string(),
            parameters: serde_json::json!({"key": "value"}),
            result: None,
        };
        let line = render_tool_call_compact(&tool_call);
        assert!(!line.spans.is_empty());
    }
}
