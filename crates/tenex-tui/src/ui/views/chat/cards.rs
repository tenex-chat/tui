use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

pub(crate) fn pad_line(spans: &mut Vec<Span>, current_len: usize, width: usize, bg: Color) {
    let pad = width.saturating_sub(current_len);
    if pad > 0 {
        spans.push(Span::styled(" ".repeat(pad), Style::default().bg(bg)));
    }
}

pub(crate) fn author_line(
    author: &str,
    indicator_color: Color,
    bg: Color,
    width: usize,
) -> Line<'static> {
    let mut spans = vec![
        Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
        Span::styled(" ", Style::default().bg(bg)),
        Span::styled(
            author.to_string(),
            Style::default()
                .fg(indicator_color)
                .add_modifier(Modifier::BOLD)
                .bg(bg),
        ),
    ];
    let current_len = 2 + author.len(); // "│ " + author
    pad_line(&mut spans, current_len, width, bg);
    Line::from(spans)
}

pub(crate) fn dot_line(indicator_color: Color, bg: Color, width: usize) -> Line<'static> {
    let mut spans = vec![
        Span::styled("·", Style::default().fg(indicator_color).bg(bg)),
        Span::styled(" ", Style::default().bg(bg)),
    ];
    pad_line(&mut spans, 2, width, bg);
    Line::from(spans)
}

pub(crate) fn markdown_lines(
    markdown_lines: &[Line],
    indicator_color: Color,
    bg: Color,
    width: usize,
) -> Vec<Line<'static>> {
    let mut out = Vec::with_capacity(markdown_lines.len());
    for md_line in markdown_lines {
        let mut spans = vec![
            Span::styled("│", Style::default().fg(indicator_color).bg(bg)),
            Span::styled(" ", Style::default().bg(bg)),
        ];
        let mut line_len = 2; // "│ "
        for span in &md_line.spans {
            line_len += span.content.len();
            let mut new_style = span.style;
            new_style = new_style.bg(bg);
            spans.push(Span::styled(span.content.to_string(), new_style));
        }
        pad_line(&mut spans, line_len, width, bg);
        out.push(Line::from(spans));
    }
    out
}
