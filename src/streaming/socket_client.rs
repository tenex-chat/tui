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
