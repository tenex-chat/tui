use pulldown_cmark::{Event, Parser, Tag, TagEnd};
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};

use crate::ui::theme;

#[derive(Debug, Clone)]
struct StyleStack {
    styles: Vec<Style>,
}

impl StyleStack {
    fn new() -> Self {
        Self {
            styles: vec![Style::default().fg(theme::TEXT_PRIMARY)],
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
    let mut in_image = false;
    let mut image_alt = String::new();
    let mut image_url = String::new();

    for event in parser {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {}
                Tag::Heading { .. } => {
                    style_stack.push(|s| s.fg(theme::ACCENT_WARNING).add_modifier(Modifier::BOLD));
                }
                Tag::BlockQuote(_) => {
                    style_stack.push(|s| s.fg(theme::TEXT_MUTED).add_modifier(Modifier::ITALIC));
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
                        Style::default().fg(theme::TEXT_MUTED),
                    ));
                }
                Tag::Emphasis => {
                    style_stack.push(|s| s.add_modifier(Modifier::ITALIC));
                }
                Tag::Strong => {
                    style_stack.push(|s| s.add_modifier(Modifier::BOLD));
                }
                Tag::Link { .. } => {
                    style_stack.push(|s| s.fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::UNDERLINED));
                }
                Tag::Image { dest_url, .. } => {
                    in_image = true;
                    image_alt.clear();
                    image_url = dest_url.to_string();
                    style_stack.push(|s| s.fg(theme::ACCENT_SPECIAL));
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
                            Style::default().fg(theme::ACCENT_SUCCESS),
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
                TagEnd::Image => {
                    in_image = false;

                    // Flush current line if any
                    if !current_line.is_empty() {
                        lines.push(Line::from(current_line.clone()));
                        current_line.clear();
                    }

                    // Render image block - clone strings to ensure 'static lifetime
                    let alt_text = if image_alt.is_empty() {
                        "Image".to_string()
                    } else {
                        image_alt.clone()
                    };

                    lines.push(Line::from(""));
                    lines.push(Line::from(vec![
                        Span::styled("   🖼  ".to_string(), Style::default().fg(theme::ACCENT_SPECIAL)),
                        Span::styled(
                            alt_text,
                            Style::default().fg(theme::ACCENT_SPECIAL).add_modifier(Modifier::BOLD)
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("       ".to_string(), Style::default()),
                        Span::styled(
                            image_url.clone(),
                            Style::default().fg(theme::ACCENT_PRIMARY).add_modifier(Modifier::UNDERLINED)
                        ),
                    ]));
                    lines.push(Line::from(vec![
                        Span::styled("       [Press 'o' to open in viewer]".to_string(), Style::default().fg(theme::TEXT_DIM)),
                    ]));
                    lines.push(Line::from(""));

                    image_alt.clear();
                    image_url.clear();
                    style_stack.pop();
                }
                TagEnd::Emphasis | TagEnd::Strong | TagEnd::Link => {
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
                } else if in_image {
                    // Collect alt text for image
                    image_alt.push_str(&text);
                } else {
                    current_line.push(Span::styled(text.to_string(), style_stack.current()));
                }
            }
            Event::Code(code) => {
                current_line.push(Span::styled(
                    code.to_string(),
                    Style::default().fg(theme::ACCENT_SUCCESS),
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
                    Style::default().fg(theme::TEXT_DIM),
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
            Style::default().fg(theme::TEXT_PRIMARY),
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

    #[test]
    fn test_render_image() {
        let text = "![diagram](https://example.com/image.png)";
        let lines = render_markdown(text);

        // Should contain image block with icon, alt text, URL, and hint
        assert!(lines.len() >= 4);

        // Find the line with the icon
        let icon_line = lines.iter().find(|line| {
            line.spans.iter().any(|span| span.content.contains("🖼"))
        });
        assert!(icon_line.is_some(), "Should have icon line");

        // Find the line with the URL
        let url_line = lines.iter().find(|line| {
            line.spans.iter().any(|span| span.content.contains("https://example.com/image.png"))
        });
        assert!(url_line.is_some(), "Should have URL line");
    }

    #[test]
    fn test_render_image_with_text() {
        let text = "Here's an image: ![diagram](https://example.com/image.png) and some more text";
        let lines = render_markdown(text);

        // Should contain both text and image block
        assert!(lines.len() > 1);

        // Should have image icon
        let has_icon = lines.iter().any(|line| {
            line.spans.iter().any(|span| span.content.contains("🖼"))
        });
        assert!(has_icon, "Should have image icon");
    }

    #[test]
    fn test_render_multiple_images() {
        let text = "![first](https://example.com/1.png) ![second](https://example.com/2.png)";
        let lines = render_markdown(text);

        // Should have blocks for both images
        let icon_count = lines.iter().filter(|line| {
            line.spans.iter().any(|span| span.content.contains("🖼"))
        }).count();
        assert_eq!(icon_count, 2, "Should have two image blocks");
    }
}
