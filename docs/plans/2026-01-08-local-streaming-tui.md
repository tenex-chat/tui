# Local Streaming TUI Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Connect to backend Unix socket to receive real-time LLM chunks, display streaming content in chat view, reconcile with Nostr events.

**Architecture:** Socket client runs in NostrWorker thread alongside relay connection, sends chunks via existing `data_tx` channel. Chat view accumulates chunks until Nostr event arrives.

**Tech Stack:** Rust `tokio::net::UnixStream`, NDJSON parsing with `serde_json`, integrates with existing `DataChange` enum.

---

## Task 1: Add LocalStreamChunk Type

**Files:**
- Create: `src/streaming/mod.rs`
- Create: `src/streaming/types.rs`
- Modify: `src/lib.rs`

**Step 1: Create streaming module directory**

```bash
mkdir -p src/streaming
```

**Step 2: Create types.rs**

Create `src/streaming/types.rs`:

```rust
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
```

**Step 3: Create mod.rs**

Create `src/streaming/mod.rs`:

```rust
mod types;
mod socket_client;

pub use types::LocalStreamChunk;
pub use socket_client::SocketStreamClient;
```

**Step 4: Add to lib.rs**

Add to `src/lib.rs`:

```rust
pub mod streaming;
```

**Step 5: Commit**

```bash
git add src/streaming/ src/lib.rs
git commit -m "feat(streaming): add LocalStreamChunk type"
```

---

## Task 2: Create SocketStreamClient

**Files:**
- Create: `src/streaming/socket_client.rs`

**Step 1: Create socket_client.rs**

Create `src/streaming/socket_client.rs`:

```rust
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use super::LocalStreamChunk;

/// Client for connecting to the local streaming socket
pub struct SocketStreamClient {
    socket_path: PathBuf,
}

impl SocketStreamClient {
    pub fn new() -> Self {
        Self {
            socket_path: Self::default_socket_path(),
        }
    }

    pub fn with_path(path: PathBuf) -> Self {
        Self { socket_path: path }
    }

    fn default_socket_path() -> PathBuf {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            PathBuf::from(runtime_dir).join("tenex-stream.sock")
        } else {
            PathBuf::from("/tmp/tenex-stream.sock")
        }
    }

    /// Try to connect to the socket, returns None if socket doesn't exist
    pub async fn connect(&self) -> Option<UnixStream> {
        if !self.socket_path.exists() {
            debug!("Streaming socket not found at {:?}", self.socket_path);
            return None;
        }

        match UnixStream::connect(&self.socket_path).await {
            Ok(stream) => {
                info!("Connected to streaming socket at {:?}", self.socket_path);
                Some(stream)
            }
            Err(e) => {
                warn!("Failed to connect to streaming socket: {}", e);
                None
            }
        }
    }

    /// Run the socket client, sending chunks through the provided channel
    /// Reconnects automatically if connection is lost
    pub async fn run(self, chunk_tx: mpsc::Sender<LocalStreamChunk>) {
        loop {
            if let Some(stream) = self.connect().await {
                if let Err(e) = self.read_stream(stream, &chunk_tx).await {
                    error!("Stream read error: {}", e);
                }
            }

            // Wait before reconnect attempt
            tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
        }
    }

    async fn read_stream(
        &self,
        stream: UnixStream,
        chunk_tx: &mpsc::Sender<LocalStreamChunk>,
    ) -> Result<(), std::io::Error> {
        let reader = BufReader::new(stream);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            if line.is_empty() {
                continue;
            }

            match serde_json::from_str::<LocalStreamChunk>(&line) {
                Ok(chunk) => {
                    if chunk_tx.send(chunk).await.is_err() {
                        warn!("Chunk receiver dropped");
                        break;
                    }
                }
                Err(e) => {
                    warn!("Failed to parse chunk: {} - line: {}", e, line);
                }
            }
        }

        info!("Streaming socket disconnected");
        Ok(())
    }

    pub fn socket_path(&self) -> &PathBuf {
        &self.socket_path
    }
}

impl Default for SocketStreamClient {
    fn default() -> Self {
        Self::new()
    }
}
```

**Step 2: Commit**

```bash
git add src/streaming/socket_client.rs
git commit -m "feat(streaming): add SocketStreamClient for Unix socket connection"
```

---

## Task 3: Extend DataChange Enum

**Files:**
- Modify: `src/nostr/worker.rs`

**Step 1: Add LocalChunk variant to DataChange**

In `src/nostr/worker.rs`, find the `DataChange` enum and add a new variant:

```rust
pub enum DataChange {
    StreamingDelta {
        pubkey: String,
        message_id: String,
        thread_id: String,
        sequence: Option<u64>,
        created_at: u64,
        delta: String,
    },
    /// Chunk from local streaming socket (not from Nostr)
    LocalStreamChunk {
        agent_pubkey: String,
        conversation_id: String,
        text_delta: Option<String>,
        reasoning_delta: Option<String>,
        is_finish: bool,
    },
}
```

**Step 2: Commit**

```bash
git add src/nostr/worker.rs
git commit -m "feat(nostr): add LocalStreamChunk variant to DataChange"
```

---

## Task 4: Integrate Socket Client into NostrWorker

**Files:**
- Modify: `src/nostr/worker.rs`

**Step 1: Add import**

Add at top of `src/nostr/worker.rs`:

```rust
use crate::streaming::{LocalStreamChunk, SocketStreamClient};
```

**Step 2: Add socket client channel**

In the `NostrWorker::run()` method, add socket client setup before the main loop:

```rust
// Setup local streaming socket client
let (local_chunk_tx, mut local_chunk_rx) = tokio::sync::mpsc::channel::<LocalStreamChunk>(256);
let socket_client = SocketStreamClient::new();

// Spawn socket client task
tokio::spawn(async move {
    socket_client.run(local_chunk_tx).await;
});
```

**Step 3: Handle local chunks in the event loop**

Add a branch to handle local chunks in the tokio::select! or main loop:

```rust
// In the main processing loop, add:
tokio::select! {
    // ... existing branches ...

    Some(chunk) = local_chunk_rx.recv() => {
        let data_change = DataChange::LocalStreamChunk {
            agent_pubkey: chunk.agent_pubkey,
            conversation_id: chunk.conversation_id,
            text_delta: chunk.text_delta().map(String::from),
            reasoning_delta: chunk.reasoning_delta().map(String::from),
            is_finish: chunk.is_finish(),
        };
        let _ = data_tx.send(data_change);
    }
}
```

**Step 4: Commit**

```bash
git add src/nostr/worker.rs
git commit -m "feat(nostr): integrate socket client into worker"
```

---

## Task 5: Add Local Streaming State to App

**Files:**
- Modify: `src/ui/app.rs`

**Step 1: Add streaming buffer struct**

Add near other struct definitions in `src/ui/app.rs`:

```rust
/// Buffer for local streaming content (per conversation)
#[derive(Default, Clone)]
pub struct LocalStreamBuffer {
    pub agent_pubkey: String,
    pub text_content: String,
    pub reasoning_content: String,
    pub is_complete: bool,
}
```

**Step 2: Add to App struct**

Add field to `App` struct:

```rust
/// Local streaming buffers by conversation_id
pub local_stream_buffers: std::collections::HashMap<String, LocalStreamBuffer>,
```

**Step 3: Initialize in App::new()**

Add to initialization:

```rust
local_stream_buffers: std::collections::HashMap::new(),
```

**Step 4: Add helper methods**

Add to `impl App`:

```rust
/// Get streaming content for current conversation
pub fn local_streaming_content(&self) -> Option<&LocalStreamBuffer> {
    let conv_id = self.current_conversation_id()?;
    self.local_stream_buffers.get(&conv_id)
}

/// Update streaming buffer from local chunk
pub fn handle_local_stream_chunk(
    &mut self,
    agent_pubkey: String,
    conversation_id: String,
    text_delta: Option<String>,
    reasoning_delta: Option<String>,
    is_finish: bool,
) {
    let buffer = self.local_stream_buffers
        .entry(conversation_id)
        .or_insert_with(|| LocalStreamBuffer {
            agent_pubkey: agent_pubkey.clone(),
            ..Default::default()
        });

    if let Some(delta) = text_delta {
        buffer.text_content.push_str(&delta);
    }
    if let Some(delta) = reasoning_delta {
        buffer.reasoning_content.push_str(&delta);
    }
    if is_finish {
        buffer.is_complete = true;
    }
}

/// Clear streaming buffer when Nostr event arrives
pub fn clear_local_stream_buffer(&mut self, conversation_id: &str) {
    self.local_stream_buffers.remove(conversation_id);
}

fn current_conversation_id(&self) -> Option<String> {
    // Return thread_id or conversation root - adjust based on existing logic
    self.selected_thread.clone()
}
```

**Step 5: Commit**

```bash
git add src/ui/app.rs
git commit -m "feat(ui): add local streaming buffer to App state"
```

---

## Task 6: Handle LocalStreamChunk in Main Event Loop

**Files:**
- Modify: `src/main.rs`

**Step 1: Update check_for_data_updates function**

Find `check_for_data_updates()` and add handling for LocalStreamChunk:

```rust
fn check_for_data_updates(app: &mut App) {
    while let Ok(change) = app.data_rx.try_recv() {
        match change {
            DataChange::StreamingDelta { /* existing */ } => {
                // existing handling
            }
            DataChange::LocalStreamChunk {
                agent_pubkey,
                conversation_id,
                text_delta,
                reasoning_delta,
                is_finish,
            } => {
                app.handle_local_stream_chunk(
                    agent_pubkey,
                    conversation_id,
                    text_delta,
                    reasoning_delta,
                    is_finish,
                );
            }
        }
    }
}
```

**Step 2: Commit**

```bash
git add src/main.rs
git commit -m "feat: handle LocalStreamChunk in main event loop"
```

---

## Task 7: Render Streaming Content in Chat View

**Files:**
- Modify: `src/ui/views/chat.rs`

**Step 1: Add streaming content to chat rendering**

Find where messages are rendered (likely in `render_chat()` function) and add streaming content display after the last message:

```rust
// After rendering existing messages, check for streaming content
if let Some(buffer) = app.local_streaming_content() {
    if !buffer.text_content.is_empty() || !buffer.reasoning_content.is_empty() {
        // Render streaming indicator
        let streaming_style = Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::ITALIC);

        // Show agent name with streaming indicator
        let agent_name = app.get_agent_name(&buffer.agent_pubkey)
            .unwrap_or_else(|| "Agent".to_string());
        let header = format!("{} ▌", agent_name); // Cursor indicator

        let header_span = Span::styled(header, Style::default().fg(Color::Green));

        // Render streaming text
        let mut content = buffer.text_content.clone();
        if !buffer.is_complete {
            content.push('▌'); // Blinking cursor effect
        }

        let text_paragraph = Paragraph::new(content)
            .style(streaming_style)
            .wrap(Wrap { trim: false });

        // Render in appropriate area - adjust based on existing chat layout
        // This is a simplified example - integrate with existing message rendering
    }
}
```

**Step 2: Clear buffer when message event arrives**

In the Nostr event handling code, when a new message arrives for a conversation, clear the streaming buffer:

```rust
// When processing new message event
app.clear_local_stream_buffer(&conversation_id);
```

**Step 3: Commit**

```bash
git add src/ui/views/chat.rs
git commit -m "feat(ui): render local streaming content in chat view"
```

---

## Task 8: Add Cargo Dependencies

**Files:**
- Modify: `Cargo.toml`

**Step 1: Verify dependencies**

Check that these are already present (they likely are):
- `serde` with derive feature
- `serde_json`
- `tokio` with net feature

If `tokio` doesn't have `net` feature, add it:

```toml
tokio = { version = "1", features = ["full", "net"] }
```

**Step 2: Commit (if changes needed)**

```bash
git add Cargo.toml
git commit -m "chore: ensure tokio net feature enabled"
```

---

## Task 9: Add Tests

**Files:**
- Create: `src/streaming/types.rs` (add tests module)

**Step 1: Add tests to types.rs**

Add at the bottom of `src/streaming/types.rs`:

```rust
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
                "textDelta": "Hello"
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
                "type": "reasoning",
                "textDelta": "Let me think..."
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
```

**Step 2: Run tests**

```bash
cargo test streaming
```

**Step 3: Commit**

```bash
git add src/streaming/types.rs
git commit -m "test(streaming): add LocalStreamChunk tests"
```

---

## Verification

After completing all tasks:

1. Build: `cargo build`
2. Run tests: `cargo test`
3. Start backend daemon (should create socket)
4. Start TUI: `cargo run`
5. Verify connection log: "Connected to streaming socket"
6. Send a message to an agent
7. Verify streaming text appears with cursor indicator before final Nostr event
8. Verify final message replaces streaming content when Nostr event arrives
