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
//! ## Event Semantics (kind:24133) - AUTHORITATIVE PER-CONVERSATION CONTRACT:
//!
//! Each 24133 event represents an **authoritative snapshot** of active agents for a
//! specific conversation at a specific point in time. This is a **replacement** semantic,
//! not an additive one:
//!
//! - Each event contains exactly ONE `e` tag (conversation ID via thread_id or event_id)
//! - The `p` tags list ALL agents currently active on that conversation
//! - An event with 0 `p` tags means all agents have stopped working on that conversation
//! - When a new 24133 event arrives for a conversation, it REPLACES the previous agent list
//! - Agents on OTHER conversations are NOT affected by this event
//! - Project is identified via `a` tag for filtering
//!
//! ### Ordering Guarantees:
//! - Events are processed in timestamp order (created_at) per conversation
//! - Same-second events use event_id as a tiebreaker for deterministic ordering
//! - Out-of-order/stale events are rejected to maintain consistency
//!
//! ### Runtime Tracking:
//! - `llm-runtime` tags provide confirmed runtime in seconds
//! - Each event's llm-runtime is only counted ONCE (deduplicated by event_id)
//! - This prevents double-counting on replays or reprocessing

use std::collections::{HashMap, HashSet};
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

/// Composite key for event ordering: (timestamp, event_id) for deterministic same-second handling.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct EventOrderKey {
    created_at: u64,
    event_id: String,
}

impl EventOrderKey {
    fn new(created_at: u64, event_id: impl Into<String>) -> Self {
        Self {
            created_at,
            event_id: event_id.into(),
        }
    }
}

/// Real-time agent tracking state.
/// In-memory only - resets on application restart (session-scoped).
#[derive(Debug, Default)]
pub struct AgentTrackingState {
    /// Active agents mapped to when they started working.
    /// Key: (conversation_id, agent_pubkey), Value: start time (Instant)
    active_agents: HashMap<AgentInstanceKey, Instant>,

    /// Last processed event key per conversation for deterministic ordering.
    /// Key: conversation_id, Value: (created_at, event_id) for same-second tiebreaking
    last_event_key: HashMap<String, EventOrderKey>,

    /// Confirmed runtime in seconds from completed agent work (llm-runtime tags).
    confirmed_runtime_secs: u64,

    /// Set of event IDs that have already contributed llm-runtime.
    /// Prevents double-counting on replays or reprocessing.
    runtime_event_ids: HashSet<String>,
}

impl AgentTrackingState {
    /// Create a new empty tracking state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Clear all tracking state (used on logout/disconnect).
    pub fn clear(&mut self) {
        self.active_agents.clear();
        self.last_event_key.clear();
        self.confirmed_runtime_secs = 0;
        self.runtime_event_ids.clear();
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
    /// This implements the **authoritative per-conversation contract**: each event
    /// represents a complete snapshot of active agents for that conversation, replacing
    /// any previous state.
    ///
    /// ## Parameters:
    /// - `conversation_id`: The conversation (thread_id or event_id) being updated
    /// - `event_id`: The unique event ID (for same-second ordering and deduplication)
    /// - `agent_pubkeys`: Current active agents (p-tags); empty = all agents stopped
    /// - `created_at`: Event timestamp (Unix seconds)
    /// - `project_coordinate`: The project this conversation belongs to (a-tag)
    /// - `current_project`: The currently selected project in the UI (for filtering)
    ///
    /// ## Returns:
    /// - `true` if the event was processed (affected state)
    /// - `false` if the event was rejected (stale/out-of-order or wrong project)
    ///
    /// ## Ordering:
    /// Events are ordered by (created_at, event_id) to handle same-second events
    /// deterministically. This ensures consistent state even when multiple events
    /// arrive within the same second.
    pub fn process_24133_event(
        &mut self,
        conversation_id: &str,
        event_id: &str,
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

        // Create composite key for deterministic ordering
        let new_key = EventOrderKey::new(created_at, event_id);

        // Reject stale/out-of-order events using composite key comparison
        // This handles same-second events by using event_id as tiebreaker
        if let Some(last_key) = self.last_event_key.get(conversation_id) {
            if new_key <= *last_key {
                return false; // Stale or already-processed event
            }
        }

        // Update last event key for this conversation
        self.last_event_key.insert(conversation_id.to_string(), new_key);

        // Build set of current agents for efficient lookup
        let current_agents: HashSet<&str> =
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
    ///
    /// ## Deduplication:
    /// Each event's runtime is only counted ONCE. If an event_id has already
    /// contributed runtime (e.g., on replay or reprocessing), the runtime
    /// is silently ignored to prevent double-counting.
    ///
    /// ## Returns:
    /// - `true` if the runtime was added (first time seeing this event)
    /// - `false` if the runtime was already counted (duplicate event_id)
    pub fn add_confirmed_runtime(&mut self, event_id: &str, runtime_secs: u64) -> bool {
        // Check if we've already counted runtime from this event
        if !self.runtime_event_ids.insert(event_id.to_string()) {
            return false; // Already counted, skip to prevent double-counting
        }

        self.confirmed_runtime_secs = self.confirmed_runtime_secs.saturating_add(runtime_secs);
        true
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
            "event1",
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
            "event1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        assert_eq!(state.active_agent_count(), 1);

        // Remove all agents with empty p-tags
        state.process_24133_event(
            "conv1",
            "event2",
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
            "event2",
            &["agent1".to_string()],
            1001,
            "31933:user:project",
            None,
        );

        // Stale event should be rejected
        let processed = state.process_24133_event(
            "conv1",
            "event1",
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
            "event1",
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

        // Add same agent with first event
        state.process_24133_event(
            "conv1",
            "event1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        // Add same agent with second event (newer timestamp)
        state.process_24133_event(
            "conv1",
            "event2",
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
            "event1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        state.process_24133_event(
            "conv2",
            "event2",
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
            "event1",
            &["agent1".to_string(), "agent2".to_string()],
            1000,
            "31933:user:project",
            None,
        );

        // Wait a bit (1+ second to get non-zero runtime)
        thread::sleep(Duration::from_millis(1100));

        // Both agents contribute to unconfirmed runtime
        // With 2 agents running for ~1.1s each, total should be ~2s
        let runtime = state.unconfirmed_runtime_secs();
        // Each agent contributes ~1 second, so total should be at least 2
        assert!(runtime >= 2, "Expected runtime >= 2, got {}", runtime);
    }

    #[test]
    fn test_confirmed_runtime() {
        let mut state = AgentTrackingState::new();

        // First event adds runtime
        let added1 = state.add_confirmed_runtime("event1", 100);
        assert!(added1);

        // Second event adds runtime
        let added2 = state.add_confirmed_runtime("event2", 50);
        assert!(added2);

        assert_eq!(state.confirmed_runtime_secs(), 150);
    }

    #[test]
    fn test_confirmed_runtime_deduplication() {
        let mut state = AgentTrackingState::new();

        // First add from event1
        let added1 = state.add_confirmed_runtime("event1", 100);
        assert!(added1);
        assert_eq!(state.confirmed_runtime_secs(), 100);

        // Duplicate add from same event1 should be rejected
        let added2 = state.add_confirmed_runtime("event1", 100);
        assert!(!added2);
        assert_eq!(state.confirmed_runtime_secs(), 100); // Still 100, not 200

        // Different event should still work
        let added3 = state.add_confirmed_runtime("event2", 50);
        assert!(added3);
        assert_eq!(state.confirmed_runtime_secs(), 150);
    }

    #[test]
    fn test_total_runtime() {
        let mut state = AgentTrackingState::new();

        state.add_confirmed_runtime("event1", 100);

        // Total should include confirmed
        assert!(state.total_runtime_secs() >= 100);
    }

    #[test]
    fn test_clear() {
        let mut state = AgentTrackingState::new();

        state.process_24133_event(
            "conv1",
            "event1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        state.add_confirmed_runtime("event1", 100);

        state.clear();

        assert_eq!(state.active_agent_count(), 0);
        assert_eq!(state.confirmed_runtime_secs(), 0);
        // After clear, same event_id can contribute again
        let added = state.add_confirmed_runtime("event1", 100);
        assert!(added);
    }

    #[test]
    fn test_same_timestamp_with_different_event_id_accepted() {
        let mut state = AgentTrackingState::new();

        // First event at t=1000
        let processed1 = state.process_24133_event(
            "conv1",
            "event_aaa",  // Lexicographically smaller
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        assert!(processed1);
        assert_eq!(state.active_agent_count(), 1);

        // Same timestamp but different (larger) event_id should be accepted
        let processed2 = state.process_24133_event(
            "conv1",
            "event_bbb",  // Lexicographically larger, so newer
            &["agent2".to_string()],
            1000, // Same timestamp
            "31933:user:project",
            None,
        );

        assert!(processed2);
        // Should now have agent2, not agent1 (authoritative replacement)
        let agents = state.get_active_agents_for_conversation("conv1");
        assert_eq!(agents.len(), 1);
        assert!(agents.contains(&"agent2"));
    }

    #[test]
    fn test_same_timestamp_same_event_id_rejected() {
        let mut state = AgentTrackingState::new();

        // First event
        let processed1 = state.process_24133_event(
            "conv1",
            "event1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        assert!(processed1);

        // Same timestamp AND same event_id should be rejected (duplicate)
        let processed2 = state.process_24133_event(
            "conv1",
            "event1",  // Same event_id
            &["agent2".to_string()],
            1000, // Same timestamp
            "31933:user:project",
            None,
        );

        assert!(!processed2);
        // Should still have agent1
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
            "event1",
            &["agent1".to_string(), "agent2".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        state.process_24133_event(
            "conv2",
            "event2",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        assert_eq!(state.active_agent_count(), 3);

        // Remove agent2 from conv1 only
        state.process_24133_event(
            "conv1",
            "event3",
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
            "event1",
            &["agent1".to_string()],
            1000,
            "31933:user:project",
            None,
        );

        // Event on conv2 at t=500 should succeed (different conversation)
        let processed = state.process_24133_event(
            "conv2",
            "event2",
            &["agent2".to_string()],
            500, // Earlier timestamp, but different conversation
            "31933:user:project",
            None,
        );

        assert!(processed);
        assert_eq!(state.active_agent_count(), 2);
    }

    #[test]
    fn test_authoritative_replacement_semantics() {
        let mut state = AgentTrackingState::new();

        // Initial state: agent1, agent2, agent3 on conv1
        state.process_24133_event(
            "conv1",
            "event1",
            &["agent1".to_string(), "agent2".to_string(), "agent3".to_string()],
            1000,
            "31933:user:project",
            None,
        );
        assert_eq!(state.active_agent_count(), 3);

        // New event: only agent1 is active (authoritative replacement)
        state.process_24133_event(
            "conv1",
            "event2",
            &["agent1".to_string()],
            1001,
            "31933:user:project",
            None,
        );

        // Should now only have agent1
        assert_eq!(state.active_agent_count(), 1);
        let agents = state.get_active_agents_for_conversation("conv1");
        assert_eq!(agents, vec!["agent1"]);
    }
}
