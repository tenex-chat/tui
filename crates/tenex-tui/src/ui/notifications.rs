// Centralized notification/toast system for TUI status feedback
// Replaces ad-hoc status_message with a proper queue supporting priorities and auto-dismiss

use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// Notification priority levels (higher = more important)
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NotificationLevel {
    Info,
    Success,
    Warning,
    Error,
}

impl NotificationLevel {
    /// Get the icon for this notification level
    pub fn icon(&self) -> &'static str {
        match self {
            NotificationLevel::Info => "ℹ",
            NotificationLevel::Success => "✓",
            NotificationLevel::Warning => "⚠",
            NotificationLevel::Error => "✗",
        }
    }
}

/// A single notification
#[derive(Debug, Clone)]
pub struct Notification {
    pub message: String,
    pub level: NotificationLevel,
    pub duration: Duration,
    pub shown_at: Option<Instant>,
    /// Optional thread ID for "message for you" notifications (enables jump-to-thread)
    pub thread_id: Option<String>,
}

impl Notification {
    /// Create an info notification (default 3 second duration)
    pub fn info(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: NotificationLevel::Info,
            duration: Duration::from_secs(3),
            shown_at: None,
            thread_id: None,
        }
    }

    /// Create a success notification (default 3 second duration)
    pub fn success(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: NotificationLevel::Success,
            duration: Duration::from_secs(3),
            shown_at: None,
            thread_id: None,
        }
    }

    /// Create a warning notification (default 4 second duration)
    pub fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: NotificationLevel::Warning,
            duration: Duration::from_secs(4),
            shown_at: None,
            thread_id: None,
        }
    }

    /// Create an error notification (default 5 second duration)
    pub fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            level: NotificationLevel::Error,
            duration: Duration::from_secs(5),
            shown_at: None,
            thread_id: None,
        }
    }

    /// Create a message notification for a specific thread (30 second duration)
    /// This is for "message for you" notifications that can be jumped to
    pub fn message_for_user(message: impl Into<String>, thread_id: String) -> Self {
        Self {
            message: message.into(),
            level: NotificationLevel::Warning,
            duration: Duration::from_secs(30),
            shown_at: None,
            thread_id: Some(thread_id),
        }
    }

    /// Set a custom duration for this notification
    pub fn duration(mut self, duration: Duration) -> Self {
        self.duration = duration;
        self
    }

    /// Check if this notification has expired
    pub fn is_expired(&self) -> bool {
        self.shown_at
            .map(|shown| shown.elapsed() >= self.duration)
            .unwrap_or(false)
    }

    /// Mark this notification as being shown now
    pub fn mark_shown(&mut self) {
        if self.shown_at.is_none() {
            self.shown_at = Some(Instant::now());
        }
    }
}

/// Queue of notifications with priority handling
#[derive(Debug, Default)]
pub struct NotificationQueue {
    /// Queue of pending notifications (front = next to show)
    queue: VecDeque<Notification>,
    /// Currently displayed notification
    current: Option<Notification>,
    /// Track recent messages for deduplication (message hash -> expiry)
    recent_messages: Vec<(u64, Instant)>,
}

impl NotificationQueue {
    pub fn new() -> Self {
        Self::default()
    }

    /// Add a notification to the queue
    /// Higher priority notifications replace lower priority ones (the old notification is dropped)
    pub fn push(&mut self, notification: Notification) {
        // Check for duplicate (same message shown recently)
        let hash = Self::hash_message(&notification.message);
        let now = Instant::now();

        // Clean up old entries
        self.recent_messages.retain(|(_, expiry)| *expiry > now);

        // Check if this message was recently shown (within last 2 seconds)
        if self.recent_messages.iter().any(|(h, _)| *h == hash) {
            return; // Skip duplicate
        }

        // Track this message to prevent duplicates
        self.recent_messages.push((hash, now + Duration::from_secs(2)));

        // If there's a current notification, check priority
        if let Some(ref current) = self.current {
            if notification.level > current.level {
                // Higher priority - replace current (old notification is dropped)
                // Don't re-queue the old notification - it was already shown,
                // showing it again would create duplicate toasts
                self.current = Some(notification);
                if let Some(ref mut n) = self.current {
                    n.mark_shown();
                }
                return;
            }
        }

        // No current notification - show immediately
        if self.current.is_none() {
            let mut n = notification;
            n.mark_shown();
            self.current = Some(n);
        } else {
            // Add to queue based on priority (higher priority at front)
            let pos = self.queue.iter()
                .position(|n| n.level < notification.level)
                .unwrap_or(self.queue.len());
            self.queue.insert(pos, notification);
        }
    }

    /// Get the current notification being displayed
    pub fn current(&self) -> Option<&Notification> {
        self.current.as_ref()
    }

    /// Dismiss the current notification
    pub fn dismiss(&mut self) {
        self.current = None;
        self.advance();
    }

    /// Update the queue - advance to next notification if current expired
    pub fn tick(&mut self) {
        if let Some(ref current) = self.current {
            if current.is_expired() {
                self.current = None;
                self.advance();
            }
        }
    }

    /// Advance to the next notification in queue
    fn advance(&mut self) {
        if self.current.is_none() {
            if let Some(mut next) = self.queue.pop_front() {
                next.mark_shown();
                self.current = Some(next);
            }
        }
    }

    /// Check if there are any notifications (current or pending)
    pub fn is_empty(&self) -> bool {
        self.current.is_none() && self.queue.is_empty()
    }

    /// Clear all notifications
    pub fn clear(&mut self) {
        self.current = None;
        self.queue.clear();
    }

    /// Simple hash for deduplication
    fn hash_message(message: &str) -> u64 {
        use std::hash::{Hash, Hasher};
        use std::collections::hash_map::DefaultHasher;
        let mut hasher = DefaultHasher::new();
        message.hash(&mut hasher);
        hasher.finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_creation() {
        let n = Notification::info("test");
        assert_eq!(n.level, NotificationLevel::Info);
        assert_eq!(n.duration, Duration::from_secs(3));

        let n = Notification::error("error").duration(Duration::from_secs(10));
        assert_eq!(n.level, NotificationLevel::Error);
        assert_eq!(n.duration, Duration::from_secs(10));
    }

    #[test]
    fn test_queue_basic() {
        let mut q = NotificationQueue::new();
        assert!(q.is_empty());

        q.push(Notification::info("first"));
        assert!(!q.is_empty());
        assert_eq!(q.current().unwrap().message, "first");

        q.dismiss();
        assert!(q.is_empty());
    }

    #[test]
    fn test_priority_replaces_current() {
        let mut q = NotificationQueue::new();

        q.push(Notification::info("low priority"));
        assert_eq!(q.current().unwrap().message, "low priority");

        // Error replaces info (info is dropped, not re-queued)
        q.push(Notification::error("high priority"));
        assert_eq!(q.current().unwrap().message, "high priority");

        // After dismissing high priority, queue is empty because the low priority
        // notification was dropped when replaced (prevents duplicate toast bug)
        q.dismiss();
        assert!(q.current().is_none());
    }

    #[test]
    fn test_level_ordering() {
        assert!(NotificationLevel::Error > NotificationLevel::Warning);
        assert!(NotificationLevel::Warning > NotificationLevel::Success);
        assert!(NotificationLevel::Success > NotificationLevel::Info);
    }
}
