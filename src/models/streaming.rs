use std::collections::HashMap;

/// Accumulates deltas and reconstructs content from out-of-order chunks
#[derive(Debug, Clone, Default)]
pub struct DeltaContentAccumulator {
    /// Map of sequence -> content
    deltas: HashMap<u32, String>,
    /// Cached reconstructed content
    cached_content: String,
    /// Highest contiguous sequence number (for fast-path append)
    highest_contiguous: i32,
}

impl DeltaContentAccumulator {
    pub fn new() -> Self {
        Self {
            deltas: HashMap::new(),
            cached_content: String::new(),
            highest_contiguous: -1,
        }
    }

    /// Add a delta and return the reconstructed content
    pub fn add_delta(&mut self, sequence: Option<u32>, content: &str) -> &str {
        let seq = sequence.unwrap_or_else(|| self.deltas.len() as u32);

        self.deltas.insert(seq, content.to_string());

        // Fast path: if this is the next expected sequence, just append
        if seq as i32 == self.highest_contiguous + 1 {
            self.cached_content.push_str(content);
            self.highest_contiguous = seq as i32;

            // Check if we can now append more sequences that were waiting
            while self.deltas.contains_key(&((self.highest_contiguous + 1) as u32)) {
                self.highest_contiguous += 1;
            }
        } else {
            // Slow path: out-of-order, reconstruct from all deltas
            self.cached_content = self.reconstruct();
            self.update_highest_contiguous();
        }

        &self.cached_content
    }

    fn reconstruct(&self) -> String {
        let mut sorted: Vec<_> = self.deltas.iter().collect();
        sorted.sort_by_key(|(seq, _)| *seq);
        sorted.iter().map(|(_, content)| content.as_str()).collect()
    }

    fn update_highest_contiguous(&mut self) {
        self.highest_contiguous = -1;
        while self.deltas.contains_key(&((self.highest_contiguous + 1) as u32)) {
            self.highest_contiguous += 1;
        }
    }

    pub fn content(&self) -> &str {
        &self.cached_content
    }
}

/// Represents an active streaming session (one per agent pubkey)
#[derive(Debug, Clone)]
pub struct StreamingSession {
    /// Agent pubkey (the session key)
    pub pubkey: String,
    /// Message being replied to (from 'e' tag with "reply" marker, NIP-10)
    pub message_id: String,
    /// Thread root (from 'e' tag with "root" marker, NIP-10) - used for filtering by thread
    pub thread_id: String,
    /// Delta accumulator
    pub accumulator: DeltaContentAccumulator,
    /// Most recent created_at from any delta
    pub latest_created_at: u64,
}

impl StreamingSession {
    pub fn new(pubkey: String, message_id: String, thread_id: String, created_at: u64) -> Self {
        Self {
            pubkey,
            message_id,
            thread_id,
            accumulator: DeltaContentAccumulator::new(),
            latest_created_at: created_at,
        }
    }

    /// Add a delta to this session
    pub fn add_delta(&mut self, sequence: Option<u32>, content: &str, created_at: u64) {
        self.accumulator.add_delta(sequence, content);
        if created_at > self.latest_created_at {
            self.latest_created_at = created_at;
        }
    }

    /// Get the reconstructed content
    pub fn content(&self) -> &str {
        self.accumulator.content()
    }

    /// Check if this session has any content (vs just a typing indicator)
    pub fn has_content(&self) -> bool {
        !self.accumulator.content().trim().is_empty()
    }
}
