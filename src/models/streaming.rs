use std::collections::HashMap;

/// Represents a streaming delta chunk (Nostr kind:21111)
/// These events accumulate to form the final message content
#[derive(Debug, Clone)]
pub struct StreamingDelta {
    pub message_id: String,
    pub delta: String,
    pub sequence: Option<u32>,
    pub created_at: u64,
}


/// Accumulates streaming deltas for messages
#[derive(Debug, Default)]
pub struct StreamingAccumulator {
    /// Map of message_id -> accumulated deltas
    deltas: HashMap<String, Vec<StreamingDelta>>,
}

impl StreamingAccumulator {
    pub fn new() -> Self {
        Self {
            deltas: HashMap::new(),
        }
    }

    /// Add a delta to the accumulator
    pub fn add_delta(&mut self, delta: StreamingDelta) {
        self.deltas
            .entry(delta.message_id.clone())
            .or_default()
            .push(delta);
    }

    /// Get accumulated content for a message
    pub fn get_content(&self, message_id: &str) -> Option<String> {
        self.deltas.get(message_id).map(|deltas| {
            let mut sorted = deltas.clone();
            // Sort by sequence if available, otherwise by created_at
            sorted.sort_by(|a, b| {
                match (a.sequence, b.sequence) {
                    (Some(seq_a), Some(seq_b)) => seq_a.cmp(&seq_b),
                    _ => a.created_at.cmp(&b.created_at),
                }
            });
            sorted.iter().map(|d| d.delta.as_str()).collect()
        })
    }

    /// Clear deltas for a message (after final message received)
    pub fn clear_message(&mut self, message_id: &str) {
        self.deltas.remove(message_id);
    }

    /// Get all message IDs with pending deltas
    pub fn pending_messages(&self) -> Vec<&str> {
        self.deltas.keys().map(|s| s.as_str()).collect()
    }
}
