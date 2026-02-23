use crate::models::Message;
use crate::store::RUNTIME_CUTOFF_TIMESTAMP;
use std::collections::HashMap;

/// (user_messages_by_day, all_messages_by_day)
pub type MessagesByDayCounts = (Vec<(u64, u64)>, Vec<(u64, u64)>);

/// Sub-store for pre-aggregated statistics data.
/// Supports O(1) lookups for Stats view and Activity grid.
pub struct StatisticsStore {
    /// Pre-aggregated message counts by day: day_start_timestamp -> (user_count, all_count)
    pub(crate) messages_by_day_counts: HashMap<u64, (u64, u64)>,

    /// Pre-aggregated hourly LLM activity: (day_start, hour_of_day) -> (token_count, message_count)
    pub(crate) llm_activity_by_hour: HashMap<(u64, u8), (u64, u64)>,

    /// Pre-aggregated runtime totals by day: day_start_timestamp -> total runtime_ms
    pub(crate) runtime_by_day_counts: HashMap<u64, u64>,
}

impl Default for StatisticsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl StatisticsStore {
    pub fn new() -> Self {
        Self {
            messages_by_day_counts: HashMap::new(),
            llm_activity_by_hour: HashMap::new(),
            runtime_by_day_counts: HashMap::new(),
        }
    }

    pub fn clear(&mut self) {
        self.messages_by_day_counts.clear();
        self.llm_activity_by_hour.clear();
        self.runtime_by_day_counts.clear();
    }

    // ===== Getters =====

    pub fn get_messages_by_day(&self, num_days: usize) -> MessagesByDayCounts {
        if num_days == 0 {
            return (Vec::new(), Vec::new());
        }

        let seconds_per_day: u64 = 86400;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let today_start = (now / seconds_per_day) * seconds_per_day;
        let earliest_day = today_start - ((num_days - 1) as u64 * seconds_per_day);

        let mut user_result: Vec<(u64, u64)> = Vec::new();
        let mut all_result: Vec<(u64, u64)> = Vec::new();

        for (&day_start, &(user_count, all_count)) in &self.messages_by_day_counts {
            if day_start >= earliest_day {
                if user_count > 0 {
                    user_result.push((day_start, user_count));
                }
                if all_count > 0 {
                    all_result.push((day_start, all_count));
                }
            }
        }

        user_result.sort_by_key(|(day, _)| *day);
        all_result.sort_by_key(|(day, _)| *day);

        (user_result, all_result)
    }

    pub fn get_tokens_by_hour(&self, num_hours: usize) -> HashMap<u64, u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let seconds_per_hour: u64 = 3600;
        let current_hour_start = (now / seconds_per_hour) * seconds_per_hour;

        self.get_tokens_by_hour_from(current_hour_start, num_hours)
    }

    pub fn get_tokens_by_hour_from(
        &self,
        current_hour_start: u64,
        num_hours: usize,
    ) -> HashMap<u64, u64> {
        let mut result: HashMap<u64, u64> = HashMap::new();

        if num_hours == 0 {
            return result;
        }

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;

        for i in 0..num_hours {
            let hour_offset = i as u64 * seconds_per_hour;
            let hour_start = current_hour_start.saturating_sub(hour_offset);

            let day_start = (hour_start / seconds_per_day) * seconds_per_day;
            let seconds_since_day_start = hour_start - day_start;
            let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;

            if let Some((tokens, _)) = self.llm_activity_by_hour.get(&(day_start, hour_of_day)) {
                result.insert(hour_start, *tokens);
            }
        }

        result
    }

    pub fn get_message_count_by_hour(&self, num_hours: usize) -> HashMap<u64, u64> {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let seconds_per_hour: u64 = 3600;
        let current_hour_start = (now / seconds_per_hour) * seconds_per_hour;

        self.get_message_count_by_hour_from(current_hour_start, num_hours)
    }

    pub fn get_message_count_by_hour_from(
        &self,
        current_hour_start: u64,
        num_hours: usize,
    ) -> HashMap<u64, u64> {
        let mut result: HashMap<u64, u64> = HashMap::new();

        if num_hours == 0 {
            return result;
        }

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;

        for i in 0..num_hours {
            let hour_offset = i as u64 * seconds_per_hour;
            let hour_start = current_hour_start.saturating_sub(hour_offset);

            let day_start = (hour_start / seconds_per_day) * seconds_per_day;
            let seconds_since_day_start = hour_start - day_start;
            let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;

            if let Some((_, message_count)) =
                self.llm_activity_by_hour.get(&(day_start, hour_of_day))
            {
                result.insert(hour_start, *message_count);
            }
        }

        result
    }

    pub fn get_today_unique_runtime(&self) -> u64 {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let seconds_per_day: u64 = 86400;
        let today_start = (now / seconds_per_day) * seconds_per_day;
        self.runtime_by_day_counts
            .get(&today_start)
            .copied()
            .unwrap_or(0)
    }

    pub fn get_runtime_by_day(&self, num_days: usize) -> Vec<(u64, u64)> {
        if num_days == 0 {
            return Vec::new();
        }

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let seconds_per_day: u64 = 86400;
        let today_start = (now / seconds_per_day) * seconds_per_day;
        let earliest_day =
            today_start.saturating_sub((num_days as u64).saturating_sub(1) * seconds_per_day);

        let mut result: Vec<(u64, u64)> = self
            .runtime_by_day_counts
            .iter()
            .filter(|(day_start, runtime_ms)| **day_start >= earliest_day && **runtime_ms > 0)
            .map(|(day_start, runtime_ms)| (*day_start, *runtime_ms))
            .collect();
        result.sort_by_key(|(day, _)| *day);
        result
    }

    // ===== Incremental Updates =====

    pub fn increment_message_day_count(
        &mut self,
        created_at: u64,
        pubkey: &str,
        user_pubkey: Option<&str>,
    ) {
        let seconds_per_day: u64 = 86400;
        let day_start = (created_at / seconds_per_day) * seconds_per_day;

        let entry = self
            .messages_by_day_counts
            .entry(day_start)
            .or_insert((0, 0));

        // Increment all count
        entry.1 += 1;

        // Increment user count if matches current user
        if user_pubkey == Some(pubkey) {
            entry.0 += 1;
        }
    }

    pub fn increment_llm_activity_hour(
        &mut self,
        created_at: u64,
        llm_metadata: &HashMap<String, String>,
    ) {
        if llm_metadata.is_empty() {
            return;
        }

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;

        let day_start = (created_at / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = created_at - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;

        let tokens = llm_metadata
            .get("total-tokens")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);

        let entry = self
            .llm_activity_by_hour
            .entry((day_start, hour_of_day))
            .or_insert((0, 0));
        entry.0 += tokens;
        entry.1 += 1;
    }

    pub fn increment_runtime_day_count(
        &mut self,
        created_at: u64,
        llm_metadata: &HashMap<String, String>,
    ) {
        if created_at < RUNTIME_CUTOFF_TIMESTAMP {
            return;
        }

        let runtime_ms: u64 = llm_metadata
            .get("runtime")
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(0);

        if runtime_ms == 0 {
            return;
        }

        let seconds_per_day: u64 = 86400;
        let day_start = (created_at / seconds_per_day) * seconds_per_day;
        let entry = self.runtime_by_day_counts.entry(day_start).or_insert(0);
        *entry = entry.saturating_add(runtime_ms);
    }

    // ===== Rebuild Methods =====

    pub fn rebuild_messages_by_day_counts_from_loaded(
        &mut self,
        user_pubkey: &Option<String>,
        messages_by_thread: &HashMap<String, Vec<Message>>,
    ) {
        self.messages_by_day_counts.clear();
        let seconds_per_day = 86_400u64;

        for messages in messages_by_thread.values() {
            for msg in messages {
                let day_start = (msg.created_at / seconds_per_day) * seconds_per_day;
                let entry = self
                    .messages_by_day_counts
                    .entry(day_start)
                    .or_insert((0, 0));
                entry.1 += 1;
                if user_pubkey.as_deref() == Some(msg.pubkey.as_str()) {
                    entry.0 += 1;
                }
            }
        }
    }

    pub fn rebuild_llm_activity_by_hour(
        &mut self,
        messages_by_thread: &HashMap<String, Vec<Message>>,
    ) {
        self.llm_activity_by_hour.clear();

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;

        for messages in messages_by_thread.values() {
            for message in messages {
                if !message.llm_metadata.is_empty() {
                    let created_at = message.created_at;

                    let day_start = (created_at / seconds_per_day) * seconds_per_day;
                    let seconds_since_day_start = created_at - day_start;
                    let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;

                    let tokens = message
                        .llm_metadata
                        .get("total-tokens")
                        .and_then(|value| value.parse::<u64>().ok())
                        .unwrap_or(0);

                    let entry = self
                        .llm_activity_by_hour
                        .entry((day_start, hour_of_day))
                        .or_insert((0, 0));
                    entry.0 += tokens;
                    entry.1 += 1;
                }
            }
        }
    }

    pub fn rebuild_runtime_by_day_counts(
        &mut self,
        messages_by_thread: &HashMap<String, Vec<Message>>,
    ) {
        let mut counts: HashMap<u64, u64> = HashMap::new();

        for messages in messages_by_thread.values() {
            for message in messages {
                let created_at = message.created_at;
                if created_at < RUNTIME_CUTOFF_TIMESTAMP {
                    continue;
                }

                let runtime_ms: u64 = message
                    .llm_metadata
                    .get("runtime")
                    .and_then(|value| value.parse::<u64>().ok())
                    .unwrap_or(0);

                if runtime_ms == 0 {
                    continue;
                }

                let seconds_per_day: u64 = 86400;
                let day_start = (created_at / seconds_per_day) * seconds_per_day;
                let entry = counts.entry(day_start).or_insert(0);
                *entry = entry.saturating_add(runtime_ms);
            }
        }

        self.runtime_by_day_counts = counts;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_message(
        id: &str,
        pubkey: &str,
        thread_id: &str,
        content: &str,
        created_at: u64,
    ) -> Message {
        Message {
            id: id.to_string(),
            content: content.to_string(),
            pubkey: pubkey.to_string(),
            thread_id: thread_id.to_string(),
            created_at,
            reply_to: None,
            is_reasoning: false,
            ask_event: None,
            q_tags: vec![],
            a_tags: vec![],
            p_tags: vec![],
            tool_name: None,
            tool_args: None,
            llm_metadata: HashMap::new(),
            delegation_tag: None,
            branch: None,
        }
    }

    // ===== Basic Getter Tests =====

    #[test]
    fn test_messages_by_day_empty() {
        let store = StatisticsStore::new();
        let (user, all) = store.get_messages_by_day(7);
        assert!(user.is_empty());
        assert!(all.is_empty());
    }

    #[test]
    fn test_messages_by_day_zero_days() {
        let store = StatisticsStore::new();
        let (user, all) = store.get_messages_by_day(0);
        assert!(user.is_empty());
        assert!(all.is_empty());
    }

    #[test]
    fn test_messages_by_day_window() {
        let mut store = StatisticsStore::new();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();
        let today_start = (now / 86400) * 86400;

        store.messages_by_day_counts.insert(today_start, (5, 10));
        store
            .messages_by_day_counts
            .insert(today_start - 86400, (3, 7));
        store
            .messages_by_day_counts
            .insert(today_start - 86400 * 10, (1, 2));

        let (user, all) = store.get_messages_by_day(3);
        assert_eq!(user.len(), 2);
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_tokens_by_hour_window() {
        let mut store = StatisticsStore::new();

        let reference_hour_start: u64 = 86400 * 100;
        let day_start = (reference_hour_start / 86400) * 86400;

        store.llm_activity_by_hour.insert((day_start, 0), (500, 10));

        let tokens = store.get_tokens_by_hour_from(reference_hour_start, 24);
        assert_eq!(tokens.get(&reference_hour_start), Some(&500));
    }

    #[test]
    fn test_message_count_by_hour() {
        let mut store = StatisticsStore::new();

        let reference_hour_start: u64 = 86400 * 100;
        let day_start = (reference_hour_start / 86400) * 86400;

        store.llm_activity_by_hour.insert((day_start, 0), (500, 10));

        let counts = store.get_message_count_by_hour_from(reference_hour_start, 24);
        assert_eq!(counts.get(&reference_hour_start), Some(&10));
    }

    #[test]
    fn test_zero_hours_returns_empty() {
        let store = StatisticsStore::new();

        let tokens = store.get_tokens_by_hour_from(86400, 0);
        assert!(tokens.is_empty());

        let counts = store.get_message_count_by_hour_from(86400, 0);
        assert!(counts.is_empty());
    }

    // ===== LLM Activity Tests =====

    #[test]
    fn test_llm_activity_utc_day_hour_bucketing() {
        let mut store = StatisticsStore::new();

        let day1_timestamp = 1705361400_u64;
        let day2_timestamp = 1705368600_u64;

        let seconds_per_day: u64 = 86400;
        let day1_start = (day1_timestamp / seconds_per_day) * seconds_per_day;
        let day2_start = (day2_timestamp / seconds_per_day) * seconds_per_day;

        assert_ne!(day1_start, day2_start);

        let mut msg1 = make_test_message("msg1", "pubkey1", "thread1", "test", day1_timestamp);
        msg1.llm_metadata = HashMap::from([("total-tokens".to_string(), "100".to_string())]);

        let mut msg2 = make_test_message("msg2", "pubkey1", "thread1", "test", day2_timestamp);
        msg2.llm_metadata = HashMap::from([("total-tokens".to_string(), "200".to_string())]);

        let mut messages_by_thread = HashMap::new();
        messages_by_thread.insert("thread1".to_string(), vec![msg1, msg2]);
        store.rebuild_llm_activity_by_hour(&messages_by_thread);

        assert_eq!(store.llm_activity_by_hour.len(), 2);

        let key1 = (day1_start, 23_u8);
        assert_eq!(store.llm_activity_by_hour.get(&key1), Some(&(100, 1)));

        let key2 = (day2_start, 1_u8);
        assert_eq!(store.llm_activity_by_hour.get(&key2), Some(&(200, 1)));
    }

    #[test]
    fn test_llm_activity_only_counts_llm_messages() {
        let mut store = StatisticsStore::new();

        let timestamp = 1705363800_u64;

        let user_msg = make_test_message("msg1", "pubkey1", "thread1", "user message", timestamp);

        let mut llm_msg =
            make_test_message("msg2", "pubkey1", "thread1", "LLM response", timestamp);
        llm_msg.llm_metadata = HashMap::from([("total-tokens".to_string(), "150".to_string())]);

        let mut empty_metadata_msg =
            make_test_message("msg3", "pubkey1", "thread1", "empty metadata", timestamp);
        empty_metadata_msg.llm_metadata = HashMap::new();

        let mut messages_by_thread = HashMap::new();
        messages_by_thread.insert(
            "thread1".to_string(),
            vec![user_msg, llm_msg, empty_metadata_msg],
        );
        store.rebuild_llm_activity_by_hour(&messages_by_thread);

        assert_eq!(store.llm_activity_by_hour.len(), 1);

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;
        let day_start = (timestamp / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = timestamp - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;
        let key = (day_start, hour_of_day);

        assert_eq!(store.llm_activity_by_hour.get(&key), Some(&(150, 1)));
    }

    #[test]
    fn test_llm_activity_token_parsing() {
        let mut store = StatisticsStore::new();

        let timestamp = 1705363800_u64;

        let mut msg1 = make_test_message("msg1", "pubkey1", "thread1", "test", timestamp);
        msg1.llm_metadata = HashMap::from([("total-tokens".to_string(), "500".to_string())]);

        let mut msg2 = make_test_message("msg2", "pubkey1", "thread1", "test", timestamp);
        msg2.llm_metadata = HashMap::from([("other-key".to_string(), "value".to_string())]);

        let mut msg3 = make_test_message("msg3", "pubkey1", "thread1", "test", timestamp);
        msg3.llm_metadata = HashMap::from([("total-tokens".to_string(), "invalid".to_string())]);

        let mut messages_by_thread = HashMap::new();
        messages_by_thread.insert("thread1".to_string(), vec![msg1, msg2, msg3]);
        store.rebuild_llm_activity_by_hour(&messages_by_thread);

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;
        let day_start = (timestamp / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = timestamp - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;
        let key = (day_start, hour_of_day);

        assert_eq!(store.llm_activity_by_hour.get(&key), Some(&(500, 3)));
    }

    #[test]
    fn test_llm_activity_window_slicing() {
        let mut store = StatisticsStore::new();

        let seconds_per_hour: u64 = 3600;
        let base_timestamp = 1705316400_u64;

        let mut messages_by_thread: HashMap<String, Vec<Message>> = HashMap::new();
        for i in 0..5u64 {
            let timestamp = base_timestamp + (i * seconds_per_hour);
            let mut msg = make_test_message(
                &format!("msg{}", i),
                "pubkey1",
                "thread1",
                "test",
                timestamp,
            );
            msg.llm_metadata =
                HashMap::from([("total-tokens".to_string(), format!("{}", (i + 1) * 100))]);
            messages_by_thread
                .entry("thread1".to_string())
                .or_default()
                .push(msg);
        }

        store.rebuild_llm_activity_by_hour(&messages_by_thread);

        assert_eq!(store.llm_activity_by_hour.len(), 5);

        let current_hour_start = base_timestamp + (4 * seconds_per_hour);

        let result = store.get_tokens_by_hour_from(current_hour_start, 3);
        assert_eq!(result.len(), 3);
        assert_eq!(
            result.get(&(base_timestamp + 4 * seconds_per_hour)),
            Some(&500_u64)
        );
        assert_eq!(
            result.get(&(base_timestamp + 3 * seconds_per_hour)),
            Some(&400_u64)
        );
        assert_eq!(
            result.get(&(base_timestamp + 2 * seconds_per_hour)),
            Some(&300_u64)
        );

        let result = store.get_tokens_by_hour_from(current_hour_start, 5);
        assert_eq!(result.len(), 5);

        let result = store.get_tokens_by_hour_from(current_hour_start, 10);
        assert_eq!(result.len(), 5);
    }

    #[test]
    fn test_llm_activity_same_hour_aggregation() {
        let mut store = StatisticsStore::new();

        let timestamp = 1705363800_u64;

        let mut msg1 = make_test_message("msg1", "pubkey1", "thread1", "test1", timestamp);
        msg1.llm_metadata = HashMap::from([("total-tokens".to_string(), "100".to_string())]);

        let mut msg2 = make_test_message("msg2", "pubkey1", "thread1", "test2", timestamp + 60);
        msg2.llm_metadata = HashMap::from([("total-tokens".to_string(), "200".to_string())]);

        let mut msg3 = make_test_message("msg3", "pubkey1", "thread1", "test3", timestamp + 120);
        msg3.llm_metadata = HashMap::from([("total-tokens".to_string(), "300".to_string())]);

        let mut messages_by_thread = HashMap::new();
        messages_by_thread.insert("thread1".to_string(), vec![msg1, msg2, msg3]);
        store.rebuild_llm_activity_by_hour(&messages_by_thread);

        assert_eq!(store.llm_activity_by_hour.len(), 1);

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;
        let day_start = (timestamp / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = timestamp - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;
        let key = (day_start, hour_of_day);

        assert_eq!(store.llm_activity_by_hour.get(&key), Some(&(600, 3)));
    }

    #[test]
    fn test_llm_activity_message_count_window() {
        let mut store = StatisticsStore::new();

        let seconds_per_hour: u64 = 3600;
        let base_timestamp = 1705316400_u64;

        let mut messages_by_thread: HashMap<String, Vec<Message>> = HashMap::new();

        // Hour 0: 1 message with 1000 tokens
        let mut msg = make_test_message("msg0", "pubkey1", "thread1", "test", base_timestamp);
        msg.llm_metadata = HashMap::from([("total-tokens".to_string(), "1000".to_string())]);
        messages_by_thread
            .entry("thread1".to_string())
            .or_default()
            .push(msg);

        // Hour 1: 2 messages with 500 tokens each
        for i in 0..2 {
            let mut msg = make_test_message(
                &format!("msg1_{}", i),
                "pubkey1",
                "thread1",
                "test",
                base_timestamp + seconds_per_hour,
            );
            msg.llm_metadata = HashMap::from([("total-tokens".to_string(), "500".to_string())]);
            messages_by_thread
                .entry("thread1".to_string())
                .or_default()
                .push(msg);
        }

        // Hour 2: 3 messages with 333 tokens each
        for i in 0..3 {
            let mut msg = make_test_message(
                &format!("msg2_{}", i),
                "pubkey1",
                "thread1",
                "test",
                base_timestamp + 2 * seconds_per_hour,
            );
            msg.llm_metadata = HashMap::from([("total-tokens".to_string(), "333".to_string())]);
            messages_by_thread
                .entry("thread1".to_string())
                .or_default()
                .push(msg);
        }

        store.rebuild_llm_activity_by_hour(&messages_by_thread);

        let current_hour_start = base_timestamp + 2 * seconds_per_hour;

        let message_result = store.get_message_count_by_hour_from(current_hour_start, 3);
        assert_eq!(message_result.len(), 3);
        assert_eq!(
            message_result.get(&(base_timestamp + 2 * seconds_per_hour)),
            Some(&3_u64)
        );
        assert_eq!(
            message_result.get(&(base_timestamp + seconds_per_hour)),
            Some(&2_u64)
        );
        assert_eq!(message_result.get(&base_timestamp), Some(&1_u64));

        let token_result = store.get_tokens_by_hour_from(current_hour_start, 3);
        assert_eq!(
            token_result.get(&(base_timestamp + 2 * seconds_per_hour)),
            Some(&999_u64)
        );
        assert_eq!(
            token_result.get(&(base_timestamp + seconds_per_hour)),
            Some(&1000_u64)
        );
        assert_eq!(token_result.get(&base_timestamp), Some(&1000_u64));
    }

    #[test]
    fn test_runtime_by_day_counts_use_message_timestamps() {
        let mut store = StatisticsStore::new();

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);
        let seconds_per_day: u64 = 86400;
        let today_start = (now / seconds_per_day) * seconds_per_day;
        let yesterday_start = today_start.saturating_sub(seconds_per_day);

        fn make_message_with_runtime(
            id: &str,
            pubkey: &str,
            thread_id: &str,
            created_at: u64,
            runtime_ms: u64,
        ) -> Message {
            Message {
                id: id.to_string(),
                content: "test".to_string(),
                pubkey: pubkey.to_string(),
                thread_id: thread_id.to_string(),
                created_at,
                reply_to: None,
                is_reasoning: false,
                ask_event: None,
                q_tags: vec![],
                a_tags: vec![],
                p_tags: vec![],
                tool_name: None,
                tool_args: None,
                llm_metadata: HashMap::from([("runtime".to_string(), runtime_ms.to_string())]),
                delegation_tag: None,
                branch: None,
            }
        }

        let messages = vec![
            make_message_with_runtime("msg1", "pubkey1", "thread1", yesterday_start + 60, 1000),
            make_message_with_runtime("msg2", "pubkey1", "thread1", today_start + 120, 2000),
        ];

        let mut messages_by_thread = HashMap::new();
        messages_by_thread.insert("thread1".to_string(), messages);
        store.rebuild_runtime_by_day_counts(&messages_by_thread);

        assert_eq!(store.get_today_unique_runtime(), 2000);

        let by_day = store.get_runtime_by_day(2);
        assert!(by_day.contains(&(yesterday_start, 1000)));
        assert!(by_day.contains(&(today_start, 2000)));
    }
}
