use super::*;

#[uniffi::export]
impl TenexCore {
    // ===== Diagnostics Methods =====

    /// Get a comprehensive diagnostics snapshot for the iOS Diagnostics view.
    /// Returns all diagnostic information in a single batched call for efficiency.
    ///
    /// This function is best-effort: each section is collected independently.
    /// If one section fails (e.g., lock error), other sections can still succeed.
    /// Check `section_errors` for any failures.
    ///
    /// Set `include_database_stats` to false to skip expensive DB scanning when
    /// the Database tab is not active.
    pub fn get_diagnostics_snapshot(&self, include_database_stats: bool) -> DiagnosticsSnapshot {
        let mut section_errors: Vec<String> = Vec::new();
        let data_dir = get_data_dir();

        // ===== 1. System Diagnostics (best-effort) =====
        let system = self
            .collect_system_diagnostics(&data_dir)
            .map_err(|e| section_errors.push(format!("System: {}", e)))
            .ok();

        // ===== 2. Negentropy Sync Diagnostics (best-effort) =====
        let sync = self
            .collect_sync_diagnostics()
            .map_err(|e| section_errors.push(format!("Sync: {}", e)))
            .ok();

        // ===== 3. Subscription Diagnostics (best-effort) =====
        let (subscriptions, total_subscription_events) =
            match self.collect_subscription_diagnostics() {
                Ok((subs, total)) => (Some(subs), total),
                Err(e) => {
                    section_errors.push(format!("Subscriptions: {}", e));
                    (None, 0)
                }
            };

        // ===== 4. Database Diagnostics (best-effort, optionally skipped) =====
        let database = if include_database_stats {
            self.collect_database_diagnostics(&data_dir)
                .map_err(|e| section_errors.push(format!("Database: {}", e)))
                .ok()
        } else {
            None // Intentionally skipped for performance
        };

        DiagnosticsSnapshot {
            system,
            sync,
            subscriptions,
            total_subscription_events,
            database,
            section_errors,
        }
    }
}
