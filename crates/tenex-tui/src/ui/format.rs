/// Truncate string to a max length without adding an ellipsis.
pub fn truncate_plain(s: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }

    if s.chars().count() <= max_len {
        s.to_string()
    } else {
        s.chars().take(max_len).collect()
    }
}

/// Truncate string to a max length, adding an ellipsis when truncated.
pub fn truncate_with_ellipsis(s: &str, max_len: usize) -> String {
    if max_len == 0 {
        return String::new();
    }

    if s.chars().count() <= max_len {
        return s.to_string();
    }

    if max_len <= 3 {
        return ".".repeat(max_len);
    }

    let take = max_len - 3;
    let mut truncated: String = s.chars().take(take).collect();
    truncated.push_str("...");
    truncated
}

/// Format a timestamp as relative time (e.g., "2m ago", "1h ago").
pub fn format_relative_time(timestamp: u64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);

    let diff = now.saturating_sub(timestamp);

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else {
        format!("{}w ago", diff / 604800)
    }
}

/// Map status label to a Unicode symbol.
pub fn status_label_to_symbol(label: &str) -> &'static str {
    match label.to_lowercase().as_str() {
        "in progress" | "in-progress" | "working" | "active" => "🔧",
        "blocked" | "waiting" | "paused" => "🚧",
        "done" | "complete" | "completed" | "finished" => "✅",
        "reviewing" | "review" | "in review" => "👀",
        "testing" | "in testing" => "🧪",
        "planning" | "draft" | "design" => "📝",
        "urgent" | "critical" | "high priority" => "🔥",
        "bug" | "issue" | "error" => "🐛",
        "enhancement" | "feature" | "new" => "✨",
        "question" | "help needed" => "❓",
        _ => "📌",
    }
}
