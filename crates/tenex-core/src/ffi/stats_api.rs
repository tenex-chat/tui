use super::*;

#[uniffi::export]
impl TenexCore {
    /// Get comprehensive stats snapshot with all data needed for iOS Stats tab.
    /// This is a single batched FFI call that returns all stats data pre-computed.
    ///
    /// Returns Result to distinguish "no data" from "core error".
    pub fn get_stats_snapshot(&self) -> Result<StatsSnapshot, TenexError> {
        let store_guard = self.store.read().map_err(|_| TenexError::LockError {
            resource: "store".to_string(),
        })?;

        let store = store_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;

        // ===== 1. Metric Cards Data =====
        // Total cost for the past COST_WINDOW_DAYS (shared constant with TUI stats page)
        use crate::constants::{CHART_WINDOW_DAYS, COST_WINDOW_DAYS};
        const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Use saturating_sub to safely handle clock skew or pre-epoch edge cases
        let cost_window_start = now.saturating_sub(COST_WINDOW_DAYS * SECONDS_PER_DAY);
        let total_cost = store.get_total_cost_since(cost_window_start);

        // ===== 2. Rankings Data =====
        let cost_by_project_raw = store.get_cost_by_project();
        let cost_by_project: Vec<ProjectCost> = cost_by_project_raw
            .into_iter()
            .map(|(a_tag, name, cost)| ProjectCost { a_tag, name, cost })
            .collect();

        // ===== 3. Runtime Chart Data (CHART_WINDOW_DAYS) =====
        let runtime_by_day_raw = store.statistics.get_runtime_by_day(CHART_WINDOW_DAYS);
        let mut runtime_by_day: Vec<DayRuntime> = runtime_by_day_raw
            .into_iter()
            .map(|(day_start, runtime_ms)| DayRuntime {
                day_start,
                runtime_ms,
            })
            .collect();

        // Sort by day_start descending (newest first)
        runtime_by_day.sort_by(|a, b| b.day_start.cmp(&a.day_start));

        // ===== 4. Messages Chart Data (CHART_WINDOW_DAYS) =====
        let (user_messages_raw, all_messages_raw) = store.get_messages_by_day(CHART_WINDOW_DAYS);

        // Combine into single vector with day_start as key
        let mut messages_map: std::collections::HashMap<u64, (u64, u64)> =
            std::collections::HashMap::new();
        for (day_start, user_count) in user_messages_raw {
            messages_map.entry(day_start).or_insert((0, 0)).0 = user_count;
        }
        for (day_start, all_count) in all_messages_raw {
            messages_map.entry(day_start).or_insert((0, 0)).1 = all_count;
        }

        let mut messages_by_day: Vec<DayMessages> = messages_map
            .into_iter()
            .map(|(day_start, (user_count, all_count))| DayMessages {
                day_start,
                user_count,
                all_count,
            })
            .collect();

        // Sort by day_start descending (newest first)
        messages_by_day.sort_by(|a, b| b.day_start.cmp(&a.day_start));

        // ===== 5. Activity Grid Data (30 days × 24 hours = 720 hours) =====
        const ACTIVITY_HOURS: usize = 30 * 24;
        let tokens_by_hour_raw = store.statistics.get_tokens_by_hour(ACTIVITY_HOURS);
        let messages_by_hour_raw = store.statistics.get_message_count_by_hour(ACTIVITY_HOURS);

        // Find max values for normalization (both tokens and messages)
        let max_tokens = tokens_by_hour_raw
            .values()
            .max()
            .copied()
            .unwrap_or(1)
            .max(1);
        let max_messages = messages_by_hour_raw
            .values()
            .max()
            .copied()
            .unwrap_or(1)
            .max(1);

        // Combine and pre-normalize intensity values (0-255) for BOTH tokens and messages
        let mut activity_map: std::collections::HashMap<u64, (u64, u64)> =
            std::collections::HashMap::new();
        for (hour_start, tokens) in tokens_by_hour_raw {
            activity_map.entry(hour_start).or_insert((0, 0)).0 = tokens;
        }
        for (hour_start, messages) in messages_by_hour_raw {
            activity_map.entry(hour_start).or_insert((0, 0)).1 = messages;
        }

        let mut activity_by_hour: Vec<HourActivity> = activity_map
            .into_iter()
            .map(|(hour_start, (tokens, messages))| {
                // Normalize tokens to 0-255 intensity scale
                let token_intensity = if max_tokens == 0 {
                    0
                } else {
                    ((tokens as f64 / max_tokens as f64) * 255.0).round() as u8
                };

                // Normalize messages to 0-255 intensity scale
                let message_intensity = if max_messages == 0 {
                    0
                } else {
                    ((messages as f64 / max_messages as f64) * 255.0).round() as u8
                };

                HourActivity {
                    hour_start,
                    tokens,
                    messages,
                    token_intensity,
                    message_intensity,
                }
            })
            .collect();

        // Sort by hour_start ascending (oldest first, as grid is rendered with newest at bottom)
        activity_by_hour.sort_by(|a, b| a.hour_start.cmp(&b.hour_start));

        // ===== Return Complete Snapshot =====
        Ok(StatsSnapshot {
            total_cost_14_days: total_cost,
            cost_by_project,
            messages_by_day,
            runtime_by_day,
            activity_by_hour,
            max_tokens,
            max_messages,
        })
    }
}
