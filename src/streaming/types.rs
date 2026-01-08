use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Chunk received from local streaming socket
/// Matches backend's LocalStreamChunk format
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LocalStreamChunk {
    /// Hex pubkey of the agent generating this response
    pub agent_pubkey: String,
    /// Root event ID of the conversation (hex)
    pub conversation_id: String,
    /// Raw AI SDK chunk - passthrough without transformation
    pub data: Value,
}

impl LocalStreamChunk {
    /// Extract text delta if this is a text-delta chunk
    pub fn text_delta(&self) -> Option<&str> {
        if self.data.get("type")?.as_str()? == "text-delta" {
            self.data.get("textDelta")?.as_str()
        } else {
            None
        }
    }

    /// Check if this is a finish chunk
    pub fn is_finish(&self) -> bool {
        self.data
            .get("type")
            .and_then(|t| t.as_str())
            .map(|t| t == "finish")
            .unwrap_or(false)
    }

    /// Extract reasoning delta if this is a reasoning chunk
    pub fn reasoning_delta(&self) -> Option<&str> {
        if self.data.get("type")?.as_str()? == "reasoning" {
            self.data.get("textDelta")?.as_str()
        } else {
            None
        }
    }
}
