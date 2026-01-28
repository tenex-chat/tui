//! Search utilities shared across crates.
//!
//! Provides consistent search semantics for text matching, including:
//! - Multi-term AND queries with '+' operator
//! - ASCII case-insensitive matching

/// Parse a search query into individual search terms.
///
/// The '+' operator splits the query into multiple terms that must ALL match
/// (AND semantics at the conversation level). Each term is trimmed and lowercased.
///
/// # Examples
/// - "error" -> ["error"]
/// - "error+timeout" -> ["error", "timeout"]
/// - "  error + timeout  " -> ["error", "timeout"]
/// - "error++timeout" -> ["error", "timeout"] (empty terms ignored)
/// - "" -> []
pub fn parse_search_terms(query: &str) -> Vec<String> {
    query
        .split('+')
        .map(|s| s.trim().to_lowercase())
        .filter(|s| !s.is_empty())
        .collect()
}

/// Check if text contains a search term (ASCII case-insensitive)
/// Uses ASCII-only case folding for consistency with highlight_text_spans
pub fn text_contains_term(text: &str, term: &str) -> bool {
    let text_chars: Vec<char> = text.chars().collect();
    let term_chars: Vec<char> = term.chars().collect();

    if term_chars.is_empty() {
        return true;
    }

    if text_chars.len() < term_chars.len() {
        return false;
    }

    // Use ASCII case-insensitive matching (consistent with highlighting)
    for start_idx in 0..=(text_chars.len() - term_chars.len()) {
        let matches = term_chars.iter().enumerate().all(|(i, tc)| {
            text_chars
                .get(start_idx + i)
                .map_or(false, |c| c.eq_ignore_ascii_case(tc))
        });
        if matches {
            return true;
        }
    }
    false
}

/// Check if text contains ALL search terms (ASCII case-insensitive)
/// Returns true only if every term in the slice is found in the text.
pub fn text_contains_all_terms(text: &str, terms: &[String]) -> bool {
    if terms.is_empty() {
        return true;
    }
    terms.iter().all(|term| text_contains_term(text, term))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_search_terms() {
        assert_eq!(parse_search_terms("error"), vec!["error"]);
        assert_eq!(
            parse_search_terms("error+timeout"),
            vec!["error", "timeout"]
        );
        assert_eq!(
            parse_search_terms("  error + timeout  "),
            vec!["error", "timeout"]
        );
        assert_eq!(
            parse_search_terms("error++timeout"),
            vec!["error", "timeout"]
        );
        assert!(parse_search_terms("").is_empty());
        assert_eq!(parse_search_terms("ERROR"), vec!["error"]);
    }

    #[test]
    fn test_text_contains_term() {
        assert!(text_contains_term("Hello World", "hello"));
        assert!(text_contains_term("Hello World", "WORLD"));
        assert!(text_contains_term("Hello World", "lo Wo"));
        assert!(!text_contains_term("Hello World", "xyz"));
        assert!(text_contains_term("Hello World", "")); // Empty term matches all
        assert!(!text_contains_term("Hi", "Hello")); // Term longer than text
    }

    #[test]
    fn test_text_contains_all_terms() {
        let terms = vec!["error".to_string(), "timeout".to_string()];
        assert!(text_contains_all_terms(
            "An error occurred with timeout",
            &terms
        ));
        assert!(!text_contains_all_terms("An error occurred", &terms));
        assert!(text_contains_all_terms("Any text", &[])); // Empty terms match all
    }
}
