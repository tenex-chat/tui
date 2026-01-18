use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use super::LocalStreamChunk;

fn debug_log(msg: &str) {
    if std::env::var("TENEX_DEBUG").map(|v| v == "1").unwrap_or(false) {
        eprintln!("[SOCKET] {}", msg);
    }
}

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
            debug_log(&format!("Streaming socket not found at {:?}", self.socket_path));
            return None;
        }

        match UnixStream::connect(&self.socket_path).await {
            Ok(stream) => {
                debug_log(&format!("Connected to streaming socket at {:?}", self.socket_path));
                Some(stream)
            }
            Err(e) => {
                debug_log(&format!("Failed to connect to streaming socket: {}", e));
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
                    eprintln!("[SOCKET ERROR] Stream read error: {}", e);
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
                        debug_log("Chunk receiver dropped");
                        break;
                    }
                }
                Err(e) => {
                    debug_log(&format!("Failed to parse chunk: {} - line: {}", e, line));
                }
            }
        }

        debug_log("Streaming socket disconnected");
        Ok(())
    }
}

impl Default for SocketStreamClient {
    fn default() -> Self {
        Self::new()
    }
}
