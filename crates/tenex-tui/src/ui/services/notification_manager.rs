use crate::ui::notifications::{Notification, NotificationQueue};

/// Manages toast notifications and status messages.
/// Extracted from App to follow Single Responsibility Principle.
pub struct NotificationManager {
    /// Queue of active notifications (private - use accessor methods)
    notifications: NotificationQueue,
}

impl NotificationManager {
    pub fn new() -> Self {
        Self {
            notifications: NotificationQueue::new(),
        }
    }

    /// Add a notification to the queue
    pub fn notify(&mut self, notification: Notification) {
        self.notifications.push(notification);
    }

    /// Set a warning status message (legacy compatibility)
    /// Prefer using notify() with specific notification types for new code
    pub fn set_warning_status(&mut self, message: &str) {
        self.notifications.push(Notification::warning(message));
    }

    /// Poll and update notification timers (call each tick)
    pub fn tick(&mut self) {
        self.notifications.tick();
    }

    /// Dismiss the current notification
    pub fn dismiss(&mut self) {
        self.notifications.dismiss();
    }

    /// Get the current notification being displayed
    pub fn current(&self) -> Option<&Notification> {
        self.notifications.current()
    }

    /// Check if there are any active notifications
    pub fn has_notifications(&self) -> bool {
        !self.notifications.is_empty()
    }

    /// Check if the notification queue is empty
    pub fn is_empty(&self) -> bool {
        self.notifications.is_empty()
    }
}

impl Default for NotificationManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_notification_manager_new() {
        let manager = NotificationManager::new();
        assert!(manager.is_empty());
        assert!(!manager.has_notifications());
    }

    #[test]
    fn test_notify() {
        let mut manager = NotificationManager::new();
        assert!(manager.is_empty());
        manager.notify(Notification::info("test"));
        assert!(!manager.is_empty());
        assert!(manager.has_notifications());
        assert_eq!(manager.current().unwrap().message, "test");
    }

    #[test]
    fn test_set_warning_status() {
        let mut manager = NotificationManager::new();
        manager.set_warning_status("warning message");
        assert!(manager.has_notifications());
        let notification = manager.current().unwrap();
        assert_eq!(notification.message, "warning message");
    }

    #[test]
    fn test_dismiss() {
        let mut manager = NotificationManager::new();
        manager.notify(Notification::info("test"));
        assert!(manager.current().is_some());
        manager.dismiss();
        assert!(manager.current().is_none());
    }

    #[test]
    fn test_tick_does_not_panic() {
        let mut manager = NotificationManager::new();
        manager.notify(Notification::info("test"));
        // Just verify tick doesn't panic
        manager.tick();
        manager.tick();
    }
}
