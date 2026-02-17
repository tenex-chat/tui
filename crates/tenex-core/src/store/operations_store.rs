use crate::models::OperationsStatus;
use crate::store::AgentTrackingState;
use nostrdb::Note;
use std::collections::{HashMap, HashSet};

/// Sub-store for operations status (kind:24133) and real-time agent tracking.
pub struct OperationsStore {
    /// Maps event_id -> OperationsStatus (which agents are working on which events)
    pub(crate) operations_by_event: HashMap<String, OperationsStatus>,

    /// Real-time agent tracking - tracks active agents and estimates unconfirmed runtime.
    /// In-memory only, resets on app restart.
    pub agent_tracking: AgentTrackingState,
}

impl OperationsStore {
    pub fn new() -> Self {
        Self {
            operations_by_event: HashMap::new(),
            agent_tracking: AgentTrackingState::new(),
        }
    }

    pub fn clear(&mut self) {
        self.operations_by_event.clear();
        self.agent_tracking.clear();
    }

    // ===== Event Handlers =====

    pub fn handle_operations_status_event(&mut self, note: &Note) {
        if let Some(status) = OperationsStatus::from_note(note) {
            self.upsert_operations_status(status);
        }
    }

    pub fn handle_operations_status_event_value(&mut self, event: &serde_json::Value) {
        if let Some(status) = OperationsStatus::from_value(event) {
            self.upsert_operations_status(status);
        }
    }

    /// Shared helper to upsert an OperationsStatus into the store.
    /// Handles both JSON and Note-based event paths.
    /// Also updates agent_tracking state for real-time active agent counts.
    pub fn upsert_operations_status(&mut self, status: OperationsStatus) {
        let event_id = status.event_id.clone();
        let nostr_event_id = status.nostr_event_id.clone();

        // Use thread_id (conversation root) for tracking, falling back to event_id
        let conversation_id = status.thread_id.as_deref().unwrap_or(&status.event_id);

        // Update agent tracking state for real-time monitoring
        let processed = self.agent_tracking.process_24133_event(
            conversation_id,
            &nostr_event_id,
            &status.agent_pubkeys,
            status.created_at,
            &status.project_coordinate,
            None, // Track all projects, not filtered
        );

        // Skip processing if event was rejected (stale/out-of-order)
        if !processed {
            return;
        }

        // If llm-runtime tag is present, add confirmed runtime (with deduplication)
        if let Some(runtime_secs) = status.llm_runtime_secs {
            self.agent_tracking
                .add_confirmed_runtime(&nostr_event_id, runtime_secs);
        }

        // If no agents are working, remove the entry
        if status.agent_pubkeys.is_empty() {
            self.operations_by_event.remove(&event_id);
        } else {
            // Only update if this event is newer than what we have
            if let Some(existing) = self.operations_by_event.get(&event_id) {
                if existing.created_at > status.created_at {
                    return;
                }
            }
            self.operations_by_event.insert(event_id, status);
        }
    }

    // ===== Query Methods =====

    pub fn get_working_agents(&self, event_id: &str) -> Vec<String> {
        self.operations_by_event
            .get(event_id)
            .map(|s| s.agent_pubkeys.clone())
            .unwrap_or_default()
    }

    pub fn is_event_busy(&self, event_id: &str) -> bool {
        self.operations_by_event
            .get(event_id)
            .map(|s| !s.agent_pubkeys.is_empty())
            .unwrap_or(false)
    }

    pub fn get_active_operations_count(&self, project_a_tag: &str) -> usize {
        self.operations_by_event
            .values()
            .filter(|s| s.project_coordinate == project_a_tag && !s.agent_pubkeys.is_empty())
            .count()
    }

    pub fn get_active_event_ids(&self, project_a_tag: &str) -> Vec<String> {
        self.operations_by_event
            .iter()
            .filter(|(_, s)| s.project_coordinate == project_a_tag && !s.agent_pubkeys.is_empty())
            .map(|(id, _)| id.clone())
            .collect()
    }

    pub fn get_project_working_agents(&self, project_a_tag: &str) -> Vec<String> {
        let mut agents: HashSet<String> = HashSet::new();
        for status in self.operations_by_event.values() {
            if status.project_coordinate == project_a_tag && !status.agent_pubkeys.is_empty() {
                agents.extend(status.agent_pubkeys.iter().cloned());
            }
        }
        agents.into_iter().collect()
    }

    pub fn is_project_busy(&self, project_a_tag: &str) -> bool {
        self.operations_by_event
            .values()
            .any(|s| s.project_coordinate == project_a_tag && !s.agent_pubkeys.is_empty())
    }

    pub fn get_all_active_operations(&self) -> Vec<&OperationsStatus> {
        let mut operations: Vec<&OperationsStatus> = self
            .operations_by_event
            .values()
            .filter(|s| !s.agent_pubkeys.is_empty())
            .collect();
        operations.sort_by_key(|s| s.created_at);
        operations
    }

    pub fn active_operations_count(&self) -> usize {
        self.operations_by_event
            .values()
            .filter(|s| !s.agent_pubkeys.is_empty())
            .count()
    }

    // ===== Agent Tracking Delegation =====

    pub fn has_active_agents(&self) -> bool {
        self.agent_tracking.has_active_agents()
    }

    pub fn active_agent_count(&self) -> usize {
        self.agent_tracking.active_agent_count()
    }

    #[cfg(test)]
    pub fn confirmed_runtime_secs(&self) -> u64 {
        self.agent_tracking.confirmed_runtime_secs()
    }

    pub fn unconfirmed_runtime_secs(&self) -> u64 {
        self.agent_tracking.unconfirmed_runtime_secs()
    }

    #[cfg(test)]
    pub fn get_active_agents_for_conversation(&self, conversation_id: &str) -> Vec<String> {
        self.agent_tracking
            .get_active_agents_for_conversation(conversation_id)
            .into_iter()
            .map(|s| s.to_string())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::OperationsStatus;

    fn make_test_operations_status(
        event_id: &str,
        project: &str,
        agents: Vec<&str>,
        created_at: u64,
    ) -> OperationsStatus {
        OperationsStatus {
            nostr_event_id: format!("nostr-{}", event_id),
            event_id: event_id.to_string(),
            agent_pubkeys: agents.into_iter().map(|s| s.to_string()).collect(),
            project_coordinate: project.to_string(),
            created_at,
            thread_id: None,
            llm_runtime_secs: None,
        }
    }

    #[test]
    fn test_empty_store() {
        let store = OperationsStore::new();
        assert!(store.get_working_agents("ev1").is_empty());
        assert!(!store.is_event_busy("ev1"));
        assert_eq!(store.get_active_operations_count("proj1"), 0);
        assert!(store.get_active_event_ids("proj1").is_empty());
        assert!(!store.is_project_busy("proj1"));
        assert!(store.get_all_active_operations().is_empty());
        assert_eq!(store.active_operations_count(), 0);
    }

    #[test]
    fn test_working_agents() {
        let mut store = OperationsStore::new();
        store.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["agent1", "agent2"], 100),
        );

        let agents = store.get_working_agents("ev1");
        assert_eq!(agents.len(), 2);
        assert!(store.is_event_busy("ev1"));
        assert!(!store.is_event_busy("ev2"));
    }

    #[test]
    fn test_per_project_counts() {
        let mut store = OperationsStore::new();
        store.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["a1"], 100),
        );
        store.operations_by_event.insert(
            "ev2".to_string(),
            make_test_operations_status("ev2", "proj1", vec!["a2"], 200),
        );
        store.operations_by_event.insert(
            "ev3".to_string(),
            make_test_operations_status("ev3", "proj2", vec!["a3"], 300),
        );

        assert_eq!(store.get_active_operations_count("proj1"), 2);
        assert_eq!(store.get_active_operations_count("proj2"), 1);
        assert!(store.is_project_busy("proj1"));
        assert!(store.is_project_busy("proj2"));
        assert!(!store.is_project_busy("proj3"));

        let proj1_events = store.get_active_event_ids("proj1");
        assert_eq!(proj1_events.len(), 2);
    }

    #[test]
    fn test_empty_agents_not_counted() {
        let mut store = OperationsStore::new();
        store.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec![], 100),
        );
        store.operations_by_event.insert(
            "ev2".to_string(),
            make_test_operations_status("ev2", "proj1", vec!["a1"], 200),
        );

        assert_eq!(store.get_active_operations_count("proj1"), 1);
        assert!(!store.is_event_busy("ev1"));
        assert!(store.is_event_busy("ev2"));
        assert_eq!(store.active_operations_count(), 1);
    }

    #[test]
    fn test_all_active_sorted_by_created_at() {
        let mut store = OperationsStore::new();
        store.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["a1"], 300),
        );
        store.operations_by_event.insert(
            "ev2".to_string(),
            make_test_operations_status("ev2", "proj1", vec!["a2"], 100),
        );
        store.operations_by_event.insert(
            "ev3".to_string(),
            make_test_operations_status("ev3", "proj1", vec!["a3"], 200),
        );

        let all = store.get_all_active_operations();
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].event_id, "ev2");
        assert_eq!(all[1].event_id, "ev3");
        assert_eq!(all[2].event_id, "ev1");
    }

    #[test]
    fn test_project_working_agents_deduped() {
        let mut store = OperationsStore::new();
        store.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["agent1", "agent2"], 100),
        );
        store.operations_by_event.insert(
            "ev2".to_string(),
            make_test_operations_status("ev2", "proj1", vec!["agent1"], 200),
        );

        let agents = store.get_project_working_agents("proj1");
        assert_eq!(agents.len(), 2);
    }

    #[test]
    fn test_cleared_on_clear() {
        let mut store = OperationsStore::new();
        store.operations_by_event.insert(
            "ev1".to_string(),
            make_test_operations_status("ev1", "proj1", vec!["a1"], 100),
        );

        store.clear();

        assert!(store.get_all_active_operations().is_empty());
        assert_eq!(store.active_operations_count(), 0);
    }
}
