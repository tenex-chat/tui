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
            // AI SDK v6 uses "text", older versions used "textDelta"
            self.data.get("text")?.as_str()
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

    /// Extract reasoning delta if this is a reasoning-delta chunk
    pub fn reasoning_delta(&self) -> Option<&str> {
        if self.data.get("type")?.as_str()? == "reasoning-delta" {
            // AI SDK uses "delta" or "text" for reasoning chunks
            self.data
                .get("delta")
                .or_else(|| self.data.get("text"))
                .and_then(|v| v.as_str())
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_text_delta_extraction() {
        let chunk = LocalStreamChunk {
            agent_pubkey: "abc".to_string(),
            conversation_id: "def".to_string(),
            data: json!({
                "type": "text-delta",
                "text": "Hello"
            }),
        };
        assert_eq!(chunk.text_delta(), Some("Hello"));
    }

    #[test]
    fn test_finish_detection() {
        let chunk = LocalStreamChunk {
            agent_pubkey: "abc".to_string(),
            conversation_id: "def".to_string(),
            data: json!({
                "type": "finish",
                "finishReason": "stop"
            }),
        };
        assert!(chunk.is_finish());
    }

    #[test]
    fn test_reasoning_delta_extraction() {
        let chunk = LocalStreamChunk {
            agent_pubkey: "abc".to_string(),
            conversation_id: "def".to_string(),
            data: json!({
                "type": "reasoning-delta",
                "delta": "Let me think..."
            }),
        };
        assert_eq!(chunk.reasoning_delta(), Some("Let me think..."));
    }

    #[test]
    fn test_non_text_returns_none() {
        let chunk = LocalStreamChunk {
            agent_pubkey: "abc".to_string(),
            conversation_id: "def".to_string(),
            data: json!({
                "type": "tool-call",
                "toolName": "search"
            }),
        };
        assert_eq!(chunk.text_delta(), None);
        assert!(!chunk.is_finish());
    }
}
