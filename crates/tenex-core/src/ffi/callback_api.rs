use super::*;

#[uniffi::export]
impl TenexCore {
    // =========================================================================
    // EVENT CALLBACK API
    // =========================================================================

    /// Register a callback to receive event notifications.
    /// Call this after login to enable push-based updates.
    ///
    /// The callback will be invoked from a background thread when:
    /// - New messages arrive for a conversation
    /// - Project status changes (kind:24010)
    /// - Streaming text chunks arrive
    /// - Any other data changes
    ///
    /// Note: Only one callback can be registered at a time.
    /// Calling this again will replace the previous callback.
    pub fn set_event_callback(&self, callback: Box<dyn EventCallback>) {
        let callback: Arc<dyn EventCallback> = Arc::from(callback);
        tlog!("PERF", "ffi.set_event_callback called");

        // Store callback
        if let Ok(mut guard) = self.event_callback.write() {
            *guard = Some(callback.clone());
        }

        // Start listener thread if not already running
        if !self.callback_listener_running.swap(true, Ordering::SeqCst) {
            self.start_callback_listener();
            tlog!(
                "PERF",
                "ffi.set_event_callback started callback listener thread"
            );
        }
    }

    /// Clear the event callback and stop the listener thread.
    /// Call this on logout to clean up resources.
    pub fn clear_event_callback(&self) {
        let started_at = Instant::now();
        // Clear callback first to prevent new notifications
        if let Ok(mut guard) = self.event_callback.write() {
            *guard = None;
        }
        // Signal listener thread to stop
        self.callback_listener_running
            .store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.callback_listener_handle.write() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        tlog!(
            "PERF",
            "ffi.clear_event_callback complete elapsedMs={}",
            started_at.elapsed().as_millis()
        );
    }
}
