use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

#[derive(Debug, Clone)]
struct StyleStack {
    styles: Vec<Style>,
}

impl StyleStack {
    fn new() -> Self {
        Self {
            styles: vec![Style::default().fg(Color::White)],
        }
    }

    fn current(&self) -> Style {
        *self.styles.last().unwrap()
    }

    fn push(&mut self, modifier: impl Fn(Style) -> Style) {
        let new_style = modifier(self.current());
        self.styles.push(new_style);
    }

    fn pop(&mut self) {
        if self.styles.len() > 1 {
            self.styles.pop();
        }
    }
}

pub fn render_markdown(text: &str) -> Vec<Line<'static>> {
    let parser = Parser::new(text);
    let mut lines: Vec<Line<'static>> = Vec::new();
    let mut current_line: Vec<Span<'static>> = Vec::new();
    let mut style_stack = StyleStack::new();
    let mut in_code_block = false;
    let mut code_block_lines: Vec<String> = Vec::new();
    let mut list_level: usize = 0;

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { .. } => {
                    style_stack.push(|s| s.fg(Color::Yellow).add_modifier(Modifier::BOLD));
                }
                Tag::BlockQuote(_) => {
                    style_stack.push(|s| s.fg(Color::Gray).add_modifier(Modifier::ITALIC));
                }
                Tag::CodeBlock(_) => {
                    in_code_block = true;
                    code_block_lines.clear();
                }
                Tag::List(_) => {
                    list_level += 1;
                }
                Tag::Item => {
                    let indent = "  ".repeat(list_level.saturating_sub(1));
                    current_line.push(Span::styled(
                        format!("{}• ", indent).to_string(),
                        Style::default().fg(Color::Cyan),
                    ));
                }
                Tag::Emphasis => {
                    style_stack.push(|s| s.add_modifier(Modifier::ITALIC));
                }
                Tag::Strong => {
                    style_stack.push(|s| s.add_modifier(Modifier::BOLD));
                }
                Tag::Link { .. } => {
                    style_stack.push(|s| s.fg(Color::Blue).add_modifier(Modifier::UNDERLINED));
                }
                Tag::Image { .. } => {
                    style_stack.push(|s| s.fg(Color::Magenta));
                }
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Paragraph => {
                    if !current_line.is_empty() {
                        lines.push(Line::from(current_line.clone()));
                        current_line.clear();
                    }
                    lines.push(Line::from(""));
                }
                TagEnd::Heading(_) => {
                    if !current_line.is_empty() {
                        lines.push(Line::from(current_line.clone()));
                        current_line.clear();
                    }
                    lines.push(Line::from(""));
                    style_stack.pop();
                }
                TagEnd::BlockQuote(_) => {
                    if !current_line.is_empty() {
                        lines.push(Line::from(current_line.clone()));
                        current_line.clear();
                    }
                    lines.push(Line::from(""));
                    style_stack.pop();
                }
                TagEnd::CodeBlock => {
                    in_code_block = false;
                    for code_line in &code_block_lines {
                        lines.push(Line::from(Span::styled(
                            code_line.clone(),
                            Style::default().fg(Color::Green),
                        )));
                    }
                    lines.push(Line::from(""));
                }
                TagEnd::List(_) => {
                    list_level = list_level.saturating_sub(1);
                    if !current_line.is_empty() {
                        lines.push(Line::from(current_line.clone()));
                        current_line.clear();
                    }
                }
                TagEnd::Item => {
                    if !current_line.is_empty() {
                        lines.push(Line::from(current_line.clone()));
                        current_line.clear();
                    }
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link | TagEnd::Image => {
                    style_stack.pop();
                }
                _ => {}
            },
            Event::Text(text) => {
                if in_code_block {
                    // Split by newlines - pulldown_cmark sends entire code block as one text event
                    for line in text.lines() {
                        code_block_lines.push(line.to_string());
                    }
                } else {
                    current_line.push(Span::styled(text.to_string(), style_stack.current()));
                }
            }
            Event::Code(code) => {
                current_line.push(Span::styled(
                    code.to_string(),
                    Style::default().fg(Color::Green),
                ));
            }
            Event::SoftBreak | Event::HardBreak => {
                if !current_line.is_empty() {
                    lines.push(Line::from(current_line.clone()));
                    current_line.clear();
                }
            }
            Event::Rule => {
                lines.push(Line::from(Span::styled(
                    "─".repeat(80),
                    Style::default().fg(Color::DarkGray),
                )));
                lines.push(Line::from(""));
            }
            _ => {}
        }
    }

    if !current_line.is_empty() {
        lines.push(Line::from(current_line));
    }

    if lines.is_empty() {
        lines.push(Line::from(Span::styled(
            text.to_string(),
            Style::default().fg(Color::White),
        )));
    }

    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_render_plain_text() {
        let text = "Hello, world!";
        let lines = render_markdown(text);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_render_bold() {
        let text = "**bold text**";
        let lines = render_markdown(text);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_render_code() {
        let text = "`inline code`";
        let lines = render_markdown(text);
        assert!(!lines.is_empty());
    }

    #[test]
    fn test_render_code_block() {
        let text = "```\ncode block\n```";
        let lines = render_markdown(text);
        assert!(lines.len() > 1);
    }
}
