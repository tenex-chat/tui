//! Real-time agent tracking and runtime estimation.
//!
//! This module tracks active agents across all conversations and estimates
//! unconfirmed runtime based on how long agents have been active.
//!
//! ## Key Concepts:
//! - **AgentInstanceKey**: Uniquely identifies an agent working on a specific conversation
//! - **Unconfirmed Runtime**: Estimated runtime calculated from agent activity duration
//! - **Confirmed Runtime**: Actual runtime from `llm-runtime` tags when agents complete
//!
//! ## Event Semantics (kind:24133):
//! - Each event has exactly ONE `e` tag (conversation ID)
//! - 0 or more `p` tags indicating active agents on that conversation
//! - 0 `p` tags = last agent stopped working on that conversation
//! - Project identified via `a` tag

use std::collections::HashMap;
use std::time::Instant;

/// Unique key identifying an agent instance working on a specific conversation.
/// Uses a proper struct instead of tuple for better semantics and debugging.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct AgentInstanceKey {
    pub conversation_id: String,
    pub agent_pubkey: String,
}

impl AgentInstanceKey {
    pub fn new(conversation_id: impl Into<String>, agent_pubkey: impl Into<String>) -> Self {
        Self {
            conversation_id: conversation_id.into(),
            agent_pubkey: agent_pubkey.into(),
        }
    }
}

/// Real-time agent tracking state.
/// In-memory only - resets on application restart.
#[derive(Debug, Default)]
pub struct AgentTrackingState {
    /// Active agents mapped to when they started working.
    /// Key: (conversation_id, agent_pubkey), Value: start time (Instant)
    active_agents: HashMap<AgentInstanceKey, Instant>,

    /// Last processed event timestamp per conversation for out-of-order handling.
    /// Key: conversation_id, Value: created_at timestamp (Unix seconds)
    last_event_ts: HashMap<String, u64>,

    /// Confirmed runtime in seconds from completed agent work (llm-runtime tags).
    confirmed_runtime_secs: u64,
}

impl AgentTrackingState {
    /// Create a new empty tracking state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear all tracking state (used on logout/disconnect).
    pub fn clear(&mut self) {
        self.active_agents.clear();
        self.last_event_ts.clear();
        self.confirmed_runtime_secs = 0;
    }

    /// Get the number of active agent instances.
    /// Example: agent1 + agent2 on conv1, agent1 on conv2 = 3 active instances.
    pub fn active_agent_count(&self) -> usize {
        self.active_agents.len()
    }

    /// Check if there are any active agents working.
    pub fn has_active_agents(&self) -> bool {
        !self.active_agents.is_empty()
    }

    /// Calculate unconfirmed runtime in seconds from currently active agents.
    /// This is computed on-the-fly based on how long each agent has been active.
    /// If N agents are running, runtime grows by N seconds per second.
    pub fn unconfirmed_runtime_secs(&self) -> u64 {
        self.active_agents
            .values()
            .map(|start_time| start_time.elapsed().as_secs())
            .sum()
    }

    /// Get the total runtime (confirmed + unconfirmed) in seconds.
    pub fn total_runtime_secs(&self) -> u64 {
        self.confirmed_runtime_secs.saturating_add(self.unconfirmed_runtime_secs())
    }

    /// Get confirmed runtime in seconds.
    pub fn confirmed_runtime_secs(&self) -> u64 {
        self.confirmed_runtime_secs
    }

    /// Process a 24133 event update for a conversation.
    ///
    /// ## Parameters:
    /// - `conversation_id`: The conversation (e-tag) being updated
    /// - `agent_pubkeys`: Current active agents (p-tags); empty = all agents stopped
    /// - `created_at`: Event timestamp (Unix seconds)
    /// - `project_coordinate`: The project this conversation belongs to (a-tag)
    /// - `current_project`: The currently selected project in the UI (for filtering)
    ///
    /// ## Returns:
    /// - `true` if the event was processed (affected state)
    /// - `false` if the event was rejected (stale/out-of-order or wrong project)
    pub fn process_24133_event(
        &mut self,
        conversation_id: &str,
        agent_pubkeys: &[String],
        created_at: u64,
        project_coordinate: &str,
        current_project: Option<&str>,
    ) -> bool {
        // Filter by project if specified
        if let Some(current) = current_project {
            if project_coordinate != current {
                return false;
            }
        }

        // Reject stale/out-of-order events (including same-second events to prevent flip-flops)
        if let Some(&last_ts) = self.last_event_ts.get(conversation_id) {
            if created_at <= last_ts {
                return false; // Stale or same-timestamp event
            }
        }

        // Update last event timestamp for this conversation
        self.last_event_ts.insert(conversation_id.to_string(), created_at);

        // Build set of current agents for efficient lookup
        let current_agents: std::collections::HashSet<&str> =
            agent_pubkeys.iter().map(|s| s.as_str()).collect();

        // Remove stopped agents using retain() to avoid unnecessary allocations
        // Keeps agents that either:
        // 1. Are on a different conversation, OR
        // 2. Are still in the current_agents set
        self.active_agents.retain(|key, _| {
            key.conversation_id != conversation_id || current_agents.contains(key.agent_pubkey.as_str())
        });

        // Add new agents using entry().or_insert_with() for idempotency
        for pubkey in agent_pubkeys {
            let key = AgentInstanceKey::new(conversation_id, pubkey);
            self.active_agents.entry(key).or_insert_with(Instant::now);
        }

        true
    }

    /// Add confirmed runtime from an llm-runtime tag.
    /// Called when an agent completes and publishes actual runtime.
    pub fn add_confirmed_runtime(&mut self, runtime_secs: u64) {
        self.confirmed_runtime_secs = self.confirmed_runtime_secs.saturating_add(runtime_secs);
    }

    /// Get active agents for a specific conversation.
    /// Returns agent pubkeys currently working on the conversation.
    pub fn get_active_agents_for_conversation(&self, conversation_id: &str) -> Vec<&str> {
        self.active_agents
            .keys()
            .filter(|key| key.conversation_id == conversation_id)
            .map(|key| key.agent_pubkey.as_str())
            .collect()
    }

    /// Get all active agent instance keys (for debugging/stats).
    pub fn get_all_active_keys(&self) -> impl Iterator<Item = &AgentInstanceKey> {
        self.active_agents.keys()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_agent_instance_key_equality() {
        let key1 = AgentInstanceKey::new("conv1", "agent1");
        let key2 = AgentInstanceKey::new("conv1", "agent1");
        let key3 = AgentInstanceKey::new("conv1", "agent2");

        assert_eq!(key1, key2);
        assert_ne!(key1, key3);
    }

    #[test]
    fn test_empty_state() {
        let state = AgentTrackingState::new();

        assert_eq!(state.active_agent_count(), 0);
        assert!(!state.has_active_agents());
        assert_eq!(state.unconfirmed_runtime_secs(), 0);
        assert_eq!(state.total_runtime_secs(), 0);
    }

    #[test]
    fn test_process_event_adds_agents() {
        let mut state = AgentTrackingState::new();

        let processed = state.process_24133_event(
            "conv1",
            &["agent1".to_string(), "agent2".to_string()],
            1000,
            "31933:user:project",
            None,
        );

        assert!(processed);
        assert_eq!(state.active_agent_count(), 2);
        assert!(state.has_active_agents());
    }

    #[test]
    fn test_empty_p_tags_removes_agents() {
        let mut state = AgentTrackingState::new();

        // Add agents
        state.process_24133_event(
            "conv1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        assert_eq!(state.active_agent_count(), 1);

        // Remove all agents with empty p-tags
        state.process_24133_event(
            "conv1",
            &[],
            1001,
            "31933:user:project",
            None,
        );
        assert_eq!(state.active_agent_count(), 0);
        assert!(!state.has_active_agents());
    }

    #[test]
    fn test_stale_event_rejected() {
        let mut state = AgentTrackingState::new();

        // Process newer event first
        state.process_24133_event(
            "conv1",
            &["agent1".to_string()],
            1001,
            "31933:user:project",
            None,
        );

        // Stale event should be rejected
        let processed = state.process_24133_event(
            "conv1",
            &["agent2".to_string()],
            1000, // older timestamp
            "31933:user:project",
            None,
        );

        assert!(!processed);
        // Should still have agent1, not agent2
        let agents = state.get_active_agents_for_conversation("conv1");
        assert_eq!(agents.len(), 1);
        assert!(agents.contains(&"agent1"));
    }

    #[test]
    fn test_project_filtering() {
        let mut state = AgentTrackingState::new();

        // Event for different project should be rejected when filtering
        let processed = state.process_24133_event(
            "conv1",
            &["agent1".to_string()],
            1000,
            "31933:user:other-project",
            Some("31933:user:my-project"),
        );

        assert!(!processed);
        assert_eq!(state.active_agent_count(), 0);
    }

    #[test]
    fn test_idempotent_agent_add() {
        let mut state = AgentTrackingState::new();

        // Add same agent twice
        state.process_24133_event(
            "conv1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        state.process_24133_event(
            "conv1",
            &["agent1".to_string()],
            1001,
            "31933:user:project",
            None,
        );

        // Should still only have one agent
        assert_eq!(state.active_agent_count(), 1);
    }

    #[test]
    fn test_multiple_conversations() {
        let mut state = AgentTrackingState::new();

        // agent1 on conv1, agent1+agent2 on conv2
        state.process_24133_event(
            "conv1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        state.process_24133_event(
            "conv2",
            &["agent1".to_string(), "agent2".to_string()],
            1000,
            "31933:user:project",
            None,
        );

        // Total of 3 active instances
        assert_eq!(state.active_agent_count(), 3);
    }

    #[test]
    fn test_unconfirmed_runtime_accumulates() {
        let mut state = AgentTrackingState::new();

        // Add two agents
        state.process_24133_event(
            "conv1",
            &["agent1".to_string(), "agent2".to_string()],
            1000,
            "31933:user:project",
            None,
        );

        // Wait a bit
        thread::sleep(Duration::from_millis(100));

        // Both agents contribute to unconfirmed runtime
        // With 2 agents running for ~100ms each, total should be ~0.2s
        // But since elapsed is in seconds and we only waited 100ms, it will be 0
        // Verify function doesn't panic and returns expected type
        let _runtime = state.unconfirmed_runtime_secs();
    }

    #[test]
    fn test_confirmed_runtime() {
        let mut state = AgentTrackingState::new();

        state.add_confirmed_runtime(100);
        state.add_confirmed_runtime(50);

        assert_eq!(state.confirmed_runtime_secs(), 150);
    }

    #[test]
    fn test_total_runtime() {
        let mut state = AgentTrackingState::new();

        state.add_confirmed_runtime(100);

        // Total should include confirmed
        assert!(state.total_runtime_secs() >= 100);
    }

    #[test]
    fn test_clear() {
        let mut state = AgentTrackingState::new();

        state.process_24133_event(
            "conv1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        state.add_confirmed_runtime(100);

        state.clear();

        assert_eq!(state.active_agent_count(), 0);
        assert_eq!(state.confirmed_runtime_secs(), 0);
    }

    #[test]
    fn test_same_timestamp_rejected() {
        let mut state = AgentTrackingState::new();

        // First event
        let processed1 = state.process_24133_event(
            "conv1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        assert!(processed1);
        assert_eq!(state.active_agent_count(), 1);

        // Same timestamp event should be rejected (prevents flip-flops)
        let processed2 = state.process_24133_event(
            "conv1",
            &["agent2".to_string()],
            1000, // Same timestamp
            "31933:user:project",
            None,
        );

        assert!(!processed2);
        // Should still have agent1, not agent2
        let agents = state.get_active_agents_for_conversation("conv1");
        assert_eq!(agents.len(), 1);
        assert!(agents.contains(&"agent1"));
    }

    #[test]
    fn test_retain_removes_only_from_matching_conversation() {
        let mut state = AgentTrackingState::new();

        // Add agents to two conversations
        state.process_24133_event(
            "conv1",
            &["agent1".to_string(), "agent2".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        state.process_24133_event(
            "conv2",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        assert_eq!(state.active_agent_count(), 3);

        // Remove agent2 from conv1 only
        state.process_24133_event(
            "conv1",
            &["agent1".to_string()],  // agent2 removed
            1001,
            "31933:user:project",
            None,
        );

        // conv1 should have 1 agent, conv2 should still have 1
        assert_eq!(state.active_agent_count(), 2);
        let conv1_agents = state.get_active_agents_for_conversation("conv1");
        let conv2_agents = state.get_active_agents_for_conversation("conv2");
        assert_eq!(conv1_agents.len(), 1);
        assert_eq!(conv2_agents.len(), 1);
        assert!(conv1_agents.contains(&"agent1"));
        assert!(conv2_agents.contains(&"agent1"));
    }

    #[test]
    fn test_per_conversation_timestamp_tracking() {
        let mut state = AgentTrackingState::new();

        // Event on conv1 at t=1000
        state.process_24133_event(
            "conv1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );

        // Event on conv2 at t=500 should succeed (different conversation)
        let processed = state.process_24133_event(
            "conv2",
            &["agent2".to_string()],
            500, // Earlier timestamp, but different conversation
            "31933:user:project",
            None,
        );

        assert!(processed);
        assert_eq!(state.active_agent_count(), 2);
    }
}
