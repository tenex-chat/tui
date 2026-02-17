//! Slug utilities for project identifiers.
//!
//! This module provides consistent slug normalization and validation
//! across all entry points (CLI, TUI, daemon).

/// Normalize a slug to a consistent format.
///
/// This applies the same normalization rules regardless of whether the slug
/// is user-provided or auto-generated from a name:
/// - Trim leading/trailing whitespace
/// - Convert to lowercase
/// - Replace any non-alphanumeric character with a dash
/// - Collapse consecutive dashes into a single dash
/// - Remove leading/trailing dashes
///
/// # Examples
/// ```
/// use tenex_core::slug::normalize_slug;
///
/// assert_eq!(normalize_slug("My Project"), "my-project");
/// assert_eq!(normalize_slug("  test  "), "test");
/// assert_eq!(normalize_slug("foo--bar"), "foo-bar");
/// assert_eq!(normalize_slug("-test-"), "test");
/// assert_eq!(normalize_slug("Hello World!"), "hello-world");
/// ```
pub fn normalize_slug(input: &str) -> String {
    let normalized: String = input
        .trim()
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect();

    // Collapse consecutive dashes and trim dashes from ends
    collapse_dashes(&normalized)
}

/// Generate a slug from a project name.
///
/// This is an alias for `normalize_slug` to make intent clear when
/// auto-generating a slug from a project name.
pub fn slug_from_name(name: &str) -> String {
    normalize_slug(name)
}

/// Collapse consecutive dashes and remove leading/trailing dashes.
fn collapse_dashes(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut prev_was_dash = true; // Start true to skip leading dashes

    for c in s.chars() {
        if c == '-' {
            if !prev_was_dash {
                result.push(c);
                prev_was_dash = true;
            }
        } else {
            result.push(c);
            prev_was_dash = false;
        }
    }

    // Remove trailing dash if present
    if result.ends_with('-') {
        result.pop();
    }

    result
}

/// Result of validating a slug.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SlugValidation {
    /// Slug is valid
    Valid(String),
    /// Slug is empty after normalization
    Empty,
    /// Slug contains only dashes (invalid)
    OnlyDashes,
}

/// Validate and normalize a slug.
///
/// Returns `SlugValidation::Valid` with the normalized slug if valid,
/// or an error variant describing why validation failed.
///
/// # Examples
/// ```
/// use tenex_core::slug::{validate_slug, SlugValidation};
///
/// assert_eq!(validate_slug("my-project"), SlugValidation::Valid("my-project".to_string()));
/// assert_eq!(validate_slug("  test  "), SlugValidation::Valid("test".to_string()));
/// assert_eq!(validate_slug(""), SlugValidation::Empty);
/// assert_eq!(validate_slug("   "), SlugValidation::Empty);
/// assert_eq!(validate_slug("---"), SlugValidation::OnlyDashes);
/// ```
pub fn validate_slug(input: &str) -> SlugValidation {
    let normalized = normalize_slug(input);

    if normalized.is_empty() {
        // Check if original had content but it was all invalid chars
        let trimmed = input.trim();
        if !trimmed.is_empty() && trimmed.chars().all(|c| !c.is_alphanumeric()) {
            SlugValidation::OnlyDashes
        } else {
            SlugValidation::Empty
        }
    } else {
        SlugValidation::Valid(normalized)
    }
}

/// Check if a string would produce a valid slug.
pub fn is_valid_slug(input: &str) -> bool {
    matches!(validate_slug(input), SlugValidation::Valid(_))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_slug() {
        assert_eq!(normalize_slug("My Project"), "my-project");
        assert_eq!(normalize_slug("  test  "), "test");
        assert_eq!(normalize_slug("foo--bar"), "foo-bar");
        assert_eq!(normalize_slug("-test-"), "test");
        assert_eq!(normalize_slug("Hello World!"), "hello-world");
        assert_eq!(
            normalize_slug("Test  Multiple   Spaces"),
            "test-multiple-spaces"
        );
        assert_eq!(normalize_slug("CamelCase"), "camelcase");
        assert_eq!(normalize_slug("with_underscores"), "with-underscores");
        assert_eq!(normalize_slug("123-numeric"), "123-numeric");
    }

    #[test]
    fn test_slug_from_name() {
        assert_eq!(slug_from_name("My Cool Project"), "my-cool-project");
    }

    #[test]
    fn test_validate_slug() {
        assert_eq!(
            validate_slug("valid-slug"),
            SlugValidation::Valid("valid-slug".to_string())
        );
        assert_eq!(
            validate_slug("  needs-trim  "),
            SlugValidation::Valid("needs-trim".to_string())
        );
        assert_eq!(validate_slug(""), SlugValidation::Empty);
        assert_eq!(validate_slug("   "), SlugValidation::Empty);
        // "---" normalizes to empty, but has original content that's all non-alphanumeric
        assert_eq!(validate_slug("---"), SlugValidation::OnlyDashes);
        // "!@#$%" also has original content but all non-alphanumeric
        assert_eq!(validate_slug("!@#$%"), SlugValidation::OnlyDashes);
    }

    #[test]
    fn test_is_valid_slug() {
        assert!(is_valid_slug("valid"));
        assert!(is_valid_slug("also-valid"));
        assert!(!is_valid_slug(""));
        assert!(!is_valid_slug("   "));
    }

    #[test]
    fn test_collapse_dashes() {
        assert_eq!(collapse_dashes("a--b"), "a-b");
        assert_eq!(collapse_dashes("a---b"), "a-b");
        assert_eq!(collapse_dashes("-start"), "start");
        assert_eq!(collapse_dashes("end-"), "end");
        assert_eq!(collapse_dashes("-both-"), "both");
    }
}
