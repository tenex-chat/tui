use ratatui::{
    style::{Color, Style},
    text::{Line, Span},
};
use serde::{Deserialize, Serialize};

use crate::ui::format::truncate_with_ellipsis;
use crate::ui::theme;
use crate::ui::todo::is_todo_write;

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

            for next_ch in chars.by_ref() {
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

/// Get icon for a tool call based on its name (Unicode symbols for consistent width)
pub fn tool_icon(name: &str) -> &'static str {
    let lower = name.to_lowercase();

    if is_todo_write(&lower) {
        return "☐";
    }

    match lower.as_str() {
        "edit" | "str_replace_editor" | "fs_edit" => "✎",
        "write" | "file_write" | "fs_write" => "✎",
        "read" | "file_read" | "fs_read" => "◉",
        "bash" | "execute_bash" | "shell" => "$",
        "glob" | "find" | "fs_glob" => "◎",
        "grep" | "search" | "fs_grep" => "◎",
        "task" | "agent" => "▶",
        "web_search" | "websearch" => "◎",
        _ => "⚙",
    }
}

/// Get semantic verb for a tool (e.g., "Reading", "Writing")
pub fn tool_verb(name: &str) -> &'static str {
    let lower = name.to_lowercase();

    if is_todo_write(&lower) {
        return "";
    }

    match lower.as_str() {
        "read" | "file_read" | "fs_read" => "Reading",
        "write" | "file_write" | "fs_write" => "Writing",
        "edit" | "str_replace_editor" | "fs_edit" => "Editing",
        "bash" | "execute_bash" | "shell" => "", // uses $ prefix instead
        "glob" | "find" | "fs_glob" => "Searching",
        "grep" | "search" | "fs_grep" => "Searching",
        "task" | "agent" => "",
        "web_search" | "websearch" => "Searching",
        _ => "Executing",
    }
}

/// Extract a meaningful target/file from tool parameters
pub fn extract_target(tool_call: &ToolCall) -> Option<String> {
    let params = &tool_call.parameters;
    let name = tool_call.name.to_lowercase();

    // For file operations (fs_read, fs_write, fs_edit, etc.), prefer description over path
    if matches!(
        name.as_str(),
        "read"
            | "file_read"
            | "fs_read"
            | "write"
            | "file_write"
            | "fs_write"
            | "edit"
            | "str_replace_editor"
            | "fs_edit"
    ) {
        if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
            return Some(truncate_with_ellipsis(desc, 60));
        }
    }

    // For bash, prefer description over command (like Svelte)
    if matches!(name.as_str(), "bash" | "execute_bash" | "shell") {
        if let Some(desc) = params.get("description").and_then(|v| v.as_str()) {
            return Some(desc.to_string());
        }
        if let Some(cmd) = params.get("command").and_then(|v| v.as_str()) {
            return Some(truncate_with_ellipsis(cmd, 50));
        }
    }

    // Try common parameter names for file paths
    for key in ["file_path", "path", "filePath", "file", "target"] {
        if let Some(val) = params.get(key).and_then(|v| v.as_str()) {
            // Shorten long paths - show last 2 components
            let parts: Vec<&str> = val.split('/').collect();
            if parts.len() > 2 {
                return Some(format!(".../{}", parts[parts.len() - 2..].join("/")));
            }
            return Some(val.to_string());
        }
    }

    // For search/grep, show the pattern
    if let Some(pattern) = params.get("pattern").and_then(|v| v.as_str()) {
        let truncated = truncate_with_ellipsis(pattern, 30);
        return Some(format!("\"{}\"", truncated));
    }

    // Query for web search
    if let Some(query) = params.get("query").and_then(|v| v.as_str()) {
        return Some(format!("\"{}\"", truncate_with_ellipsis(query, 30)));
    }

    None
}

/// Render a tool call line with tool-specific formatting (like Svelte renderers)
/// Tool calls render without background, in muted text color
/// Optional content_fallback is used when we don't have special handling for the tool
pub fn render_tool_line(
    tool_call: &ToolCall,
    indicator_color: Color,
    content_fallback: Option<&str>,
) -> Line<'static> {
    let name = tool_call.name.to_lowercase();
    let target = extract_target(tool_call).unwrap_or_default();

    let display_text = if is_todo_write(&name) {
        // todo_write: collapsed count (supports both "todos" and "items")
        let count = tool_call
            .parameters
            .get("todos")
            .or_else(|| tool_call.parameters.get("items"))
            .and_then(|v| v.as_array())
            .map(|a| a.len())
            .unwrap_or(0);
        format!("▸ {} tasks", count)
    } else {
        match name.as_str() {
            // Bash: "$ command" or "$ description"
            "bash" | "execute_bash" | "shell" => format!("$ {}", target),

            // Ask: "Asking: "title" [Question1, Question2, ...]"
            "ask" | "askuserquestion" => {
                let title = tool_call
                    .parameters
                    .get("title")
                    .and_then(|v| v.as_str())
                    .unwrap_or("Question");

                // Extract question headers from questions array
                let question_headers: Vec<String> = tool_call
                    .parameters
                    .get("questions")
                    .and_then(|v| v.as_array())
                    .map(|arr| {
                        arr.iter()
                            .filter_map(|q| {
                                q.get("header")
                                    .and_then(|h| h.as_str())
                                    .map(|s| s.to_string())
                            })
                            .collect()
                    })
                    .unwrap_or_default();

                if question_headers.is_empty() {
                    format!("Asking: \"{}\"", title)
                } else {
                    format!("Asking: \"{}\" [{}]", title, question_headers.join(", "))
                }
            }

            // File operations: emoji + target/description
            "read" | "file_read" | "fs_read" => format!("📖 {}", target),
            "write" | "file_write" | "fs_write" => format!("✏️ {}", target),
            "edit" | "str_replace_editor" | "fs_edit" => format!("✏️ {}", target),

            // Search: emoji + pattern
            "glob" | "find" | "grep" | "search" | "web_search" | "websearch" | "fs_glob"
            | "fs_grep" => {
                format!("🔍 {}", target)
            }

            // Task/Agent: show description
            "task" | "agent" => {
                let desc = tool_call
                    .parameters
                    .get("description")
                    .and_then(|v| v.as_str())
                    .unwrap_or("agent");
                format!("▶ {}", truncate_with_ellipsis(desc, 40))
            }

            // Model change: show variant being switched to
            "change_model" => {
                let variant = tool_call
                    .parameters
                    .get("variant")
                    .and_then(|v| v.as_str())
                    .unwrap_or("default");
                format!("🧠 → {}", variant)
            }

            // Conversation get: show conversation ID and prompt if present
            "conversation_get" | "mcp__tenex__conversation_get" => {
                let conv_id = tool_call
                    .parameters
                    .get("conversationId")
                    .and_then(|v| v.as_str())
                    .unwrap_or("unknown");

                // Show prompt if present and non-empty, otherwise just show conversation ID
                // Treat empty or whitespace-only prompts as absent
                if let Some(prompt) = tool_call
                    .parameters
                    .get("prompt")
                    .and_then(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                {
                    format!(
                        "📜 {} → \"{}\"",
                        truncate_with_ellipsis(conv_id, 12),
                        truncate_with_ellipsis(prompt, 50)
                    )
                } else {
                    format!("📜 {}", truncate_with_ellipsis(conv_id, 12))
                }
            }

            // Default: use content fallback if available, otherwise verb + target
            _ => {
                // If we have a meaningful content fallback, use it
                if let Some(content) = content_fallback {
                    let trimmed = content.trim();
                    if !trimmed.is_empty() {
                        return Line::from(vec![
                            Span::styled("│", Style::default().fg(indicator_color)),
                            Span::raw("  "),
                            Span::styled(
                                truncate_with_ellipsis(trimmed, 80),
                                Style::default().fg(theme::TEXT_MUTED),
                            ),
                        ]);
                    }
                }

                // Fall back to verb + target or just tool name
                let verb = tool_verb(&tool_call.name);
                if verb.is_empty() {
                    if target.is_empty() {
                        tool_call.name.clone()
                    } else {
                        format!("{} {}", tool_call.name, target)
                    }
                } else if target.is_empty() {
                    verb.to_string()
                } else {
                    format!("{} {}", verb, target)
                }
            }
        }
    };

    // No background, muted text color
    Line::from(vec![
        Span::styled("│", Style::default().fg(indicator_color)),
        Span::raw("  "),
        Span::styled(display_text, Style::default().fg(theme::TEXT_MUTED)),
    ])
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
            MessageContent::Mixed {
                text_parts: _,
                tool_calls,
            } => {
                assert_eq!(tool_calls.len(), 1);
                assert_eq!(tool_calls[0].name, "search");
            }
            _ => panic!("Expected Mixed content"),
        }
    }

    #[test]
    fn test_render_tool_line() {
        let tool_call = ToolCall {
            id: "123".to_string(),
            name: "test_tool".to_string(),
            parameters: serde_json::json!({"key": "value"}),
            result: None,
        };
        let line = render_tool_line(&tool_call, Color::Gray, None);
        assert!(!line.spans.is_empty());
    }

    #[test]
    fn test_render_conversation_get_with_prompt() {
        let tool_call = ToolCall {
            id: "123".to_string(),
            name: "mcp__tenex__conversation_get".to_string(),
            parameters: serde_json::json!({
                "conversationId": "aae52c600997",
                "prompt": "Extract the final summary of all changes made"
            }),
            result: None,
        };
        let line = render_tool_line(&tool_call, Color::Gray, None);

        // Check that the line contains the expected content
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("📜"), "Should contain conversation icon");
        assert!(
            text.contains("aae52c600997"),
            "Should contain conversation ID"
        );
        assert!(
            text.contains("Extract the final summary"),
            "Should contain the prompt"
        );
    }

    #[test]
    fn test_render_conversation_get_without_prompt() {
        let tool_call = ToolCall {
            id: "123".to_string(),
            name: "conversation_get".to_string(),
            parameters: serde_json::json!({
                "conversationId": "abc123def456"
            }),
            result: None,
        };
        let line = render_tool_line(&tool_call, Color::Gray, None);

        // Check that the line contains the expected content
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("📜"), "Should contain conversation icon");
        assert!(
            text.contains("abc123def456"),
            "Should contain conversation ID"
        );
        // Should NOT contain the arrow since there's no prompt
        assert!(
            !text.contains("→"),
            "Should not contain arrow without prompt"
        );
    }

    #[test]
    fn test_render_conversation_get_long_prompt_truncated() {
        let long_prompt = "This is a very long prompt that should be truncated because it exceeds the maximum length allowed for display in the tool line";
        let tool_call = ToolCall {
            id: "123".to_string(),
            name: "mcp__tenex__conversation_get".to_string(),
            parameters: serde_json::json!({
                "conversationId": "conv123",
                "prompt": long_prompt
            }),
            result: None,
        };
        let line = render_tool_line(&tool_call, Color::Gray, None);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();

        // The prompt must be truncated and contain an ellipsis ("..." is used by truncate_with_ellipsis)
        assert!(
            text.contains("..."),
            "Long prompt should contain ellipsis when truncated, got: '{}'",
            text
        );

        // Extract the prompt portion (inside the quotes after the arrow)
        // Format is: "│  📜 conv123 → \"truncated_prompt...\""
        // The format method produces: 📜 {} → \"{}\" so we look for content between quotes
        assert!(
            text.contains("→"),
            "Output should contain arrow (→) for prompt display, got: '{}'",
            text
        );
        assert!(
            text.contains('"'),
            "Output should contain quotes (\") around prompt, got: '{}'",
            text
        );

        let arrow_pos = text
            .find("→ \"")
            .expect("Arrow and opening quote should be present in output");
        let after_arrow = &text[arrow_pos + "→ \"".len()..];
        let closing_quote = after_arrow
            .rfind('"')
            .expect("Closing quote should be present in output");
        let displayed_prompt = &after_arrow[..closing_quote];
        // The truncated prompt should be at most 50 characters (including "...")
        assert!(
            displayed_prompt.chars().count() <= 50,
            "Truncated prompt should be at most 50 characters, got {} chars: '{}'",
            displayed_prompt.chars().count(),
            displayed_prompt
        );
    }

    #[test]
    fn test_render_conversation_get_long_id_truncated() {
        let long_id = "abcdef123456789ghijklmnop"; // 25 characters, longer than 12
        let tool_call = ToolCall {
            id: "123".to_string(),
            name: "conversation_get".to_string(),
            parameters: serde_json::json!({
                "conversationId": long_id
            }),
            result: None,
        };
        let line = render_tool_line(&tool_call, Color::Gray, None);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();

        // The ID should be truncated and contain ellipsis ("..." is used by truncate_with_ellipsis)
        assert!(
            text.contains("..."),
            "Long conversation ID should contain ellipsis when truncated, got: '{}'",
            text
        );

        // The full long ID should NOT appear in the output
        assert!(
            !text.contains(long_id),
            "Full long ID should not appear in output, got: '{}'",
            text
        );

        // Extract the ID portion (between 📜 and end of string, since no prompt)
        // Format is: "│  📜 truncated_id..."
        assert!(
            text.contains("📜"),
            "Output should contain conversation icon (📜), got: '{}'",
            text
        );

        let icon_pos = text
            .find("📜 ")
            .expect("Conversation icon (📜) should be present in output");
        let after_icon = &text[icon_pos + "📜 ".len()..];
        let displayed_id = after_icon.trim();
        // The truncated ID should be at most 12 characters (including "...")
        assert!(
            displayed_id.chars().count() <= 12,
            "Truncated ID should be at most 12 characters, got {} chars: '{}'",
            displayed_id.chars().count(),
            displayed_id
        );
    }

    #[test]
    fn test_render_conversation_get_empty_prompt() {
        let tool_call = ToolCall {
            id: "123".to_string(),
            name: "mcp__tenex__conversation_get".to_string(),
            parameters: serde_json::json!({
                "conversationId": "abc123def456",
                "prompt": ""
            }),
            result: None,
        };
        let line = render_tool_line(&tool_call, Color::Gray, None);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("📜"), "Should contain conversation icon");
        assert!(
            text.contains("abc123def456"),
            "Should contain conversation ID"
        );
        // Empty prompt should be treated as absent - no arrow or quotes
        assert!(
            !text.contains("→"),
            "Should not contain arrow with empty prompt"
        );
        assert!(!text.contains("\"\""), "Should not contain empty quotes");
    }

    #[test]
    fn test_render_conversation_get_whitespace_only_prompt() {
        let tool_call = ToolCall {
            id: "123".to_string(),
            name: "conversation_get".to_string(),
            parameters: serde_json::json!({
                "conversationId": "conv987",
                "prompt": "   \t\n   "
            }),
            result: None,
        };
        let line = render_tool_line(&tool_call, Color::Gray, None);

        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("📜"), "Should contain conversation icon");
        assert!(text.contains("conv987"), "Should contain conversation ID");
        // Whitespace-only prompt should be treated as absent - no arrow or quotes
        assert!(
            !text.contains("→"),
            "Should not contain arrow with whitespace-only prompt"
        );
    }
}
