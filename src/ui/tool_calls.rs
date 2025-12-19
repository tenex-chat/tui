use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCall {
    #[serde(default)]
    pub id: String,
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub parameters: serde_json::Value,
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

pub fn render_tool_call(tool_call: &ToolCall) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    lines.push(Line::from(Span::styled(
        "┌─ Tool Call ─────────────────────────────────────",
        Style::default().fg(Color::Cyan),
    )));

    lines.push(Line::from(vec![
        Span::styled("│ ", Style::default().fg(Color::Cyan)),
        Span::styled("Name: ", Style::default().fg(Color::Yellow)),
        Span::styled(
            tool_call.name.clone(),
            Style::default()
                .fg(Color::Green)
                .add_modifier(Modifier::BOLD),
        ),
    ]));

    if !tool_call.id.is_empty() {
        lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(Color::Cyan)),
            Span::styled("ID: ", Style::default().fg(Color::Yellow)),
            Span::styled(
                tool_call.id.clone(),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
    }

    if !tool_call.parameters.is_null() && tool_call.parameters != serde_json::json!({}) {
        lines.push(Line::from(vec![
            Span::styled("│ ", Style::default().fg(Color::Cyan)),
            Span::styled("Parameters:", Style::default().fg(Color::Yellow)),
        ]));

        let params_str = serde_json::to_string_pretty(&tool_call.parameters).unwrap_or_default();
        for param_line in params_str.lines() {
            lines.push(Line::from(vec![
                Span::styled("│   ", Style::default().fg(Color::Cyan)),
                Span::styled(
                    param_line.to_string(),
                    Style::default().fg(Color::White),
                ),
            ]));
        }
    }

    lines.push(Line::from(Span::styled(
        "└─────────────────────────────────────────────────",
        Style::default().fg(Color::Cyan),
    )));

    lines
}

pub fn render_tool_calls_group(tool_calls: &[ToolCall]) -> Vec<Line<'static>> {
    let mut lines = Vec::new();

    if tool_calls.len() > 1 {
        lines.push(Line::from(Span::styled(
            format!("┌─ Tool Calls ({}) ─────────────────────────────────", tool_calls.len()),
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        )));
        lines.push(Line::from(""));
    }

    for (i, tool_call) in tool_calls.iter().enumerate() {
        if tool_calls.len() > 1 {
            lines.push(Line::from(Span::styled(
                format!("│ [{}/{}]", i + 1, tool_calls.len()),
                Style::default().fg(Color::Magenta),
            )));
        }

        let mut tool_lines = render_tool_call(tool_call);
        if tool_calls.len() > 1 {
            for line in &mut tool_lines {
                let spans = line.spans.clone();
                line.spans.clear();
                line.spans.push(Span::styled("│ ", Style::default().fg(Color::Magenta)));
                line.spans.extend(spans);
            }
        }
        lines.extend(tool_lines);

        if i < tool_calls.len() - 1 {
            lines.push(Line::from(""));
        }
    }

    if tool_calls.len() > 1 {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "└─────────────────────────────────────────────────",
            Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD),
        )));
    }

    lines
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
        };
        let lines = render_tool_call(&tool_call);
        assert!(!lines.is_empty());
    }
}
