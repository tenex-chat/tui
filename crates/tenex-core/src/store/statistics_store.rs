use crate::models::Message;
use crate::store::RUNTIME_CUTOFF_TIMESTAMP;
use nostrdb::{Ndb, Transaction};
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tracing::{trace, warn};

/// Default batch size for pagination queries.
const DEFAULT_PAGINATION_BATCH_SIZE: i32 = 10_000;

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

    pub fn get_messages_by_day(&self, num_days: usize) -> (Vec<(u64, u64)>, Vec<(u64, u64)>) {
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

    pub fn get_tokens_by_hour_from(&self, current_hour_start: u64, num_hours: usize) -> HashMap<u64, u64> {
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

    pub fn get_message_count_by_hour_from(&self, current_hour_start: u64, num_hours: usize) -> HashMap<u64, u64> {
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

            if let Some((_, message_count)) = self.llm_activity_by_hour.get(&(day_start, hour_of_day)) {
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
        self.runtime_by_day_counts.get(&today_start).copied().unwrap_or(0)
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
        let earliest_day = today_start.saturating_sub((num_days as u64).saturating_sub(1) * seconds_per_day);

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

    pub fn increment_message_day_count(&mut self, created_at: u64, pubkey: &str, user_pubkey: Option<&str>) {
        let seconds_per_day: u64 = 86400;
        let day_start = (created_at / seconds_per_day) * seconds_per_day;

        let entry = self.messages_by_day_counts.entry(day_start).or_insert((0, 0));

        // Increment all count
        entry.1 += 1;

        // Increment user count if matches current user
        if user_pubkey == Some(pubkey) {
            entry.0 += 1;
        }
    }

    pub fn increment_llm_activity_hour(&mut self, created_at: u64, llm_metadata: &[(String, String)]) {
        if llm_metadata.is_empty() {
            return;
        }

        let seconds_per_day: u64 = 86400;
        let seconds_per_hour: u64 = 3600;

        let day_start = (created_at / seconds_per_day) * seconds_per_day;
        let seconds_since_day_start = created_at - day_start;
        let hour_of_day = (seconds_since_day_start / seconds_per_hour) as u8;

        let tokens = llm_metadata
            .iter()
            .find(|(key, _)| key == "total-tokens")
            .and_then(|(_, value)| value.parse::<u64>().ok())
            .unwrap_or(0);

        let entry = self.llm_activity_by_hour.entry((day_start, hour_of_day)).or_insert((0, 0));
        entry.0 += tokens;
        entry.1 += 1;
    }

    pub fn increment_runtime_day_count(&mut self, created_at: u64, llm_metadata: &[(String, String)]) {
        if created_at < RUNTIME_CUTOFF_TIMESTAMP {
            return;
        }

        let runtime_ms: u64 = llm_metadata
            .iter()
            .filter(|(key, _)| key == "runtime")
            .filter_map(|(_, value)| value.parse::<u64>().ok())
            .sum();

        if runtime_ms == 0 {
            return;
        }

        let seconds_per_day: u64 = 86400;
        let day_start = (created_at / seconds_per_day) * seconds_per_day;
        let entry = self.runtime_by_day_counts.entry(day_start).or_insert(0);
        *entry = entry.saturating_add(runtime_ms);
    }

    // ===== Rebuild Methods =====

    pub fn rebuild_messages_by_day_counts(
        &mut self,
        ndb: &Arc<Ndb>,
        user_pubkey: &Option<String>,
        project_a_tags: &[String],
    ) {
        self._rebuild_messages_by_day_counts_impl(ndb, user_pubkey, project_a_tags, DEFAULT_PAGINATION_BATCH_SIZE);
    }

    #[cfg(test)]
    pub fn rebuild_messages_by_day_counts_with_batch_size(
        &mut self,
        ndb: &Arc<Ndb>,
        user_pubkey: &Option<String>,
        project_a_tags: &[String],
        batch_size: i32,
    ) {
        self._rebuild_messages_by_day_counts_impl(ndb, user_pubkey, project_a_tags, batch_size);
    }

    fn _rebuild_messages_by_day_counts_impl(
        &mut self,
        ndb: &Arc<Ndb>,
        user_pubkey: &Option<String>,
        project_a_tags: &[String],
        batch_size: i32,
    ) {
        self.messages_by_day_counts.clear();

        let seconds_per_day: u64 = 86400;

        // Query user messages directly from nostrdb using .authors() filter
        if let Some(ref user_pk) = user_pubkey {
            if let Ok(pubkey_bytes) = hex::decode(user_pk) {
                if pubkey_bytes.len() == 32 {
                    let pubkey_array: [u8; 32] = pubkey_bytes.try_into().unwrap();

                    match Transaction::new(ndb) {
                        Ok(txn) => {
                            let mut until_timestamp: Option<u64> = None;
                            let mut seen_event_ids: HashSet<[u8; 32]> = HashSet::new();
                            let mut total_user_messages: u64 = 0;

                            loop {
                                let mut filter_builder = nostrdb::Filter::new()
                                    .kinds([1])
                                    .authors([&pubkey_array]);

                                if let Some(until) = until_timestamp {
                                    filter_builder = filter_builder.until(until);
                                }

                                let filter = filter_builder.build();

                                match ndb.query(&txn, &[filter], batch_size) {
                                    Ok(results) => {
                                        if results.is_empty() {
                                            break;
                                        }

                                        let page_size = results.len();
                                        let mut page_oldest_timestamp: Option<u64> = None;
                                        let mut page_newest_timestamp: Option<u64> = None;
                                        let mut new_events_in_page = 0;

                                        for result in results.iter() {
                                            if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
                                                let event_id = *note.id();
                                                let created_at = note.created_at();

                                                match page_oldest_timestamp {
                                                    None => page_oldest_timestamp = Some(created_at),
                                                    Some(t) if created_at < t => page_oldest_timestamp = Some(created_at),
                                                    _ => {}
                                                }
                                                match page_newest_timestamp {
                                                    None => page_newest_timestamp = Some(created_at),
                                                    Some(t) if created_at > t => page_newest_timestamp = Some(created_at),
                                                    _ => {}
                                                }

                                                if seen_event_ids.contains(&event_id) {
                                                    continue;
                                                }
                                                seen_event_ids.insert(event_id);

                                                let day_start = (created_at / seconds_per_day) * seconds_per_day;
                                                let entry = self.messages_by_day_counts.entry(day_start).or_insert((0, 0));
                                                entry.0 += 1;
                                                total_user_messages += 1;
                                                new_events_in_page += 1;
                                            }
                                        }

                                        if page_size >= (batch_size as usize) {
                                            if let (Some(oldest), Some(newest)) = (page_oldest_timestamp, page_newest_timestamp) {
                                                if oldest == newest {
                                                    warn!(
                                                        "Potential same-second overflow detected in user messages query: \
                                                        {} events at timestamp {}. If more than {} events share this timestamp, \
                                                        some may be missed due to nostrdb pagination limitations.",
                                                        page_size, oldest, batch_size
                                                    );
                                                }
                                            }
                                        }

                                        if page_size < (batch_size as usize) {
                                            break;
                                        }

                                        if new_events_in_page == 0 {
                                            match page_oldest_timestamp {
                                                Some(t) if t > 0 => until_timestamp = Some(t - 1),
                                                _ => break,
                                            }
                                        } else {
                                            match page_oldest_timestamp {
                                                Some(t) => until_timestamp = Some(t),
                                                None => break,
                                            }
                                        }
                                    }
                                    Err(e) => {
                                        warn!("Failed to query user messages from nostrdb: {:?}", e);
                                        break;
                                    }
                                }
                            }

                            trace!("Counted {} total user messages", total_user_messages);
                        }
                        Err(e) => {
                            warn!("Failed to create transaction for user messages query: {:?}", e);
                        }
                    }
                } else {
                    warn!("Invalid user pubkey length: {} (expected 32 bytes)", pubkey_bytes.len());
                }
            } else {
                warn!("Failed to decode user pubkey from hex: {}", user_pk);
            }
        }

        // Query all project messages
        if !project_a_tags.is_empty() {
            match Transaction::new(ndb) {
                Ok(txn) => {
                    let mut seen_event_ids: HashSet<[u8; 32]> = HashSet::new();
                    let mut total_project_messages: u64 = 0;

                    for a_tag in project_a_tags {
                        let mut until_timestamp: Option<u64> = None;

                        loop {
                            let mut filter_builder = nostrdb::Filter::new()
                                .kinds([1])
                                .tags([a_tag.as_str()], 'a');

                            if let Some(until) = until_timestamp {
                                filter_builder = filter_builder.until(until);
                            }

                            let filter = filter_builder.build();

                            match ndb.query(&txn, &[filter], batch_size) {
                                Ok(results) => {
                                    if results.is_empty() {
                                        break;
                                    }

                                    let page_size = results.len();
                                    let mut page_oldest_timestamp: Option<u64> = None;
                                    let mut page_newest_timestamp: Option<u64> = None;
                                    let mut new_events_in_page = 0;

                                    for result in results.iter() {
                                        if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
                                            let event_id = *note.id();
                                            let created_at = note.created_at();

                                            match page_oldest_timestamp {
                                                None => page_oldest_timestamp = Some(created_at),
                                                Some(t) if created_at < t => page_oldest_timestamp = Some(created_at),
                                                _ => {}
                                            }
                                            match page_newest_timestamp {
                                                None => page_newest_timestamp = Some(created_at),
                                                Some(t) if created_at > t => page_newest_timestamp = Some(created_at),
                                                _ => {}
                                            }

                                            if seen_event_ids.contains(&event_id) {
                                                continue;
                                            }
                                            seen_event_ids.insert(event_id);

                                            let day_start = (created_at / seconds_per_day) * seconds_per_day;
                                            let entry = self.messages_by_day_counts.entry(day_start).or_insert((0, 0));
                                            entry.1 += 1;
                                            total_project_messages += 1;
                                            new_events_in_page += 1;
                                        }
                                    }

                                    if page_size >= (batch_size as usize) {
                                        if let (Some(oldest), Some(newest)) = (page_oldest_timestamp, page_newest_timestamp) {
                                            if oldest == newest {
                                                warn!(
                                                    "Potential same-second overflow detected in project '{}' messages query: \
                                                    {} events at timestamp {}. If more than {} events share this timestamp, \
                                                    some may be missed due to nostrdb pagination limitations.",
                                                    a_tag, page_size, oldest, batch_size
                                                );
                                            }
                                        }
                                    }

                                    if page_size < (batch_size as usize) {
                                        break;
                                    }

                                    if new_events_in_page == 0 {
                                        match page_oldest_timestamp {
                                            Some(t) if t > 0 => until_timestamp = Some(t - 1),
                                            _ => break,
                                        }
                                    } else {
                                        match page_oldest_timestamp {
                                            Some(t) => until_timestamp = Some(t),
                                            None => break,
                                        }
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to query project messages for a-tag '{}': {:?}", a_tag, e);
                                    break;
                                }
                            }
                        }
                    }

                    trace!("Counted {} total project messages (deduplicated)", total_project_messages);
                }
                Err(e) => {
                    warn!("Failed to create transaction for project messages query: {:?}", e);
                }
            }
        }
    }

    pub fn rebuild_llm_activity_by_hour(&mut self, messages_by_thread: &HashMap<String, Vec<Message>>) {
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

                    let tokens = message.llm_metadata
                        .iter()
                        .find(|(key, _)| key == "total-tokens")
                        .and_then(|(_, value)| value.parse::<u64>().ok())
                        .unwrap_or(0);

                    let entry = self.llm_activity_by_hour.entry((day_start, hour_of_day)).or_insert((0, 0));
                    entry.0 += tokens;
                    entry.1 += 1;
                }
            }
        }
    }

    pub fn rebuild_runtime_by_day_counts(&mut self, messages_by_thread: &HashMap<String, Vec<Message>>) {
        let mut counts: HashMap<u64, u64> = HashMap::new();

        for messages in messages_by_thread.values() {
            for message in messages {
                let created_at = message.created_at;
                if created_at < RUNTIME_CUTOFF_TIMESTAMP {
                    continue;
                }

                let runtime_ms: u64 = message.llm_metadata
                    .iter()
                    .filter(|(key, _)| key == "runtime")
                    .filter_map(|(_, value)| value.parse::<u64>().ok())
                    .sum();

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
