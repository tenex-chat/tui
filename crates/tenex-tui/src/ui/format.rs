/// Get current Unix timestamp in seconds.
fn now_seconds() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

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
    let diff = now_seconds().saturating_sub(timestamp);

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

/// Format duration since a timestamp started (e.g., "2m", "1h 30m", "2d 5h").
pub fn format_duration_since(started_at: u64) -> String {
    let diff = now_seconds().saturating_sub(started_at);

    if diff < 60 {
        format!("{}s", diff)
    } else if diff < 3600 {
        format!("{}m", diff / 60)
    } else if diff < 86400 {
        let hours = diff / 3600;
        let mins = (diff % 3600) / 60;
        if mins > 0 {
            format!("{}h {}m", hours, mins)
        } else {
            format!("{}h", hours)
        }
    } else {
        let days = diff / 86400;
        let hours = (diff % 86400) / 3600;
        if hours > 0 {
            format!("{}d {}h", days, hours)
        } else {
            format!("{}d", days)
        }
    }
}

/// Format a timestamp as compact relative time or absolute date.
/// - For messages < 7 days old: "5m", "2h", "3d"
/// - For messages >= 7 days old: "2025-12-20"
pub fn format_message_time(timestamp: u64) -> String {
    let diff = now_seconds().saturating_sub(timestamp);
    const SEVEN_DAYS: u64 = 7 * 24 * 60 * 60;

    if diff < SEVEN_DAYS {
        // Relative time (compact format)
        if diff < 60 {
            "now".to_string()
        } else if diff < 3600 {
            format!("{}m", diff / 60)
        } else if diff < 86400 {
            format!("{}h", diff / 3600)
        } else {
            format!("{}d", diff / 86400)
        }
    } else {
        // Absolute date for older messages
        use chrono::{TimeZone, Utc};
        Utc.timestamp_opt(timestamp as i64, 0)
            .single()
            .map(|dt| dt.format("%Y-%m-%d").to_string())
            .unwrap_or_else(|| "unknown".to_string())
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
