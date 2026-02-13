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
            self.agent_tracking.add_confirmed_runtime(&nostr_event_id, runtime_secs);
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
        let mut operations: Vec<&OperationsStatus> = self.operations_by_event
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
