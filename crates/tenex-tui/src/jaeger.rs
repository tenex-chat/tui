//! Jaeger trace viewer utilities - URL construction and browser opening.

/// Validates and normalizes a Jaeger endpoint URL.
///
/// Rules:
/// - Must not be empty
/// - Must have http:// or https:// scheme
/// - Trailing slashes are removed
///
/// Returns the normalized endpoint or an error message.
pub fn validate_and_normalize_endpoint(endpoint: &str) -> Result<String, String> {
    let trimmed = endpoint.trim();

    if trimmed.is_empty() {
        return Err("Jaeger endpoint cannot be empty".to_string());
    }

    // Check for valid scheme
    if !trimmed.starts_with("http://") && !trimmed.starts_with("https://") {
        return Err("Jaeger endpoint must start with http:// or https://".to_string());
    }

    // Remove trailing slashes
    let normalized = trimmed.trim_end_matches('/').to_string();

    Ok(normalized)
}

/// Opens a Jaeger trace URL in the system's default browser.
///
/// Returns Ok(()) on success, or an error message on failure.
pub fn open_trace_url(url: &str) -> Result<(), String> {
    #[cfg(target_os = "macos")]
    let result = std::process::Command::new("open").arg(url).spawn();

    #[cfg(target_os = "linux")]
    let result = std::process::Command::new("xdg-open").arg(url).spawn();

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    let result: Result<std::process::Child, std::io::Error> =
        Err(std::io::Error::new(std::io::ErrorKind::Unsupported, "Unsupported platform"));

    result.map(|_| ()).map_err(|e| format!("Failed to open browser: {}", e))
}

/// Builds a Jaeger trace URL for a specific trace and optional span.
///
/// Returns the formatted URL or an error if the endpoint is invalid.
pub fn build_trace_url(endpoint: &str, trace_id: &str, span_id: Option<&str>) -> Result<String, String> {
    let normalized_endpoint = validate_and_normalize_endpoint(endpoint)?;

    let url = if let Some(span) = span_id {
        format!("{}/trace/{}?uiFind={}", normalized_endpoint, trace_id, span)
    } else {
        format!("{}/trace/{}", normalized_endpoint, trace_id)
    };

    Ok(url)
}

/// Opens a Jaeger trace in the browser for the given trace ID and optional span ID.
///
/// This is a convenience function that combines URL building and browser opening.
/// Returns Ok(()) on success, or an error message describing what went wrong.
pub fn open_trace(endpoint: &str, trace_id: &str, span_id: Option<&str>) -> Result<(), String> {
    let url = build_trace_url(endpoint, trace_id, span_id)?;
    open_trace_url(&url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_empty_endpoint() {
        assert!(validate_and_normalize_endpoint("").is_err());
        assert!(validate_and_normalize_endpoint("   ").is_err());
    }

    #[test]
    fn test_validate_missing_scheme() {
        assert!(validate_and_normalize_endpoint("localhost:16686").is_err());
        assert!(validate_and_normalize_endpoint("jaeger.example.com").is_err());
    }

    #[test]
    fn test_validate_valid_http() {
        let result = validate_and_normalize_endpoint("http://localhost:16686");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://localhost:16686");
    }

    #[test]
    fn test_validate_valid_https() {
        let result = validate_and_normalize_endpoint("https://jaeger.example.com");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://jaeger.example.com");
    }

    #[test]
    fn test_normalize_trailing_slashes() {
        let result = validate_and_normalize_endpoint("http://localhost:16686/");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://localhost:16686");

        let result = validate_and_normalize_endpoint("https://jaeger.example.com///");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "https://jaeger.example.com");
    }

    #[test]
    fn test_normalize_whitespace() {
        let result = validate_and_normalize_endpoint("  http://localhost:16686  ");
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://localhost:16686");
    }

    #[test]
    fn test_build_trace_url_without_span() {
        let result = build_trace_url("http://localhost:16686", "abc123", None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://localhost:16686/trace/abc123");
    }

    #[test]
    fn test_build_trace_url_with_span() {
        let result = build_trace_url("http://localhost:16686", "abc123", Some("def456"));
        assert!(result.is_ok());
        assert_eq!(
            result.unwrap(),
            "http://localhost:16686/trace/abc123?uiFind=def456"
        );
    }

    #[test]
    fn test_build_trace_url_normalizes_endpoint() {
        let result = build_trace_url("http://localhost:16686///", "abc123", None);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), "http://localhost:16686/trace/abc123");
    }

    #[test]
    fn test_build_trace_url_invalid_endpoint() {
        let result = build_trace_url("", "abc123", None);
        assert!(result.is_err());

        let result = build_trace_url("localhost:16686", "abc123", None);
        assert!(result.is_err());
    }
}
