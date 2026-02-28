use crate::RESET;

const BOLD_COLOR: &str = "\x1b[1;38;5;222m";
const CODE_INLINE: &str = "\x1b[36;48;5;237m";
pub(crate) const CODE_BLOCK: &str = "\x1b[38;5;248m";
const CODE_BLOCK_BAR: &str = "\x1b[38;5;240m";
const HEADING_COLOR: &str = "\x1b[1;38;5;222m";
const LIST_MARKER: &str = "\x1b[36m";

pub(crate) fn colorize_markdown(content: &str) -> String {
    let mut out = String::new();
    let mut in_code_block = false;

    for line in content.lines() {
        if !out.is_empty() {
            out.push('\n');
        }

        if line.trim_start().starts_with("```") {
            in_code_block = !in_code_block;
            out.push_str(&format!("{CODE_BLOCK_BAR}{}{RESET}", line));
            continue;
        }

        if in_code_block {
            out.push_str(&format!("{CODE_BLOCK_BAR}│{CODE_BLOCK} {}{RESET}", line));
            continue;
        }

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

fn colorize_inline(text: &str) -> String {
    let mut out = String::new();
    let chars: Vec<char> = text.chars().collect();
    let len = chars.len();
    let mut i = 0;

    while i < len {
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

fn find_closing(chars: &[char], start: usize, marker: &[char]) -> Option<usize> {
    let mlen = marker.len();
    if start + mlen > chars.len() {
        return None;
    }
    (start..chars.len() - mlen + 1).find(|&i| chars[i..i + mlen] == *marker)
}
