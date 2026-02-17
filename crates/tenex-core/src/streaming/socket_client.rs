use std::fs;
use std::path::PathBuf;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::net::UnixStream;
use tokio::sync::mpsc;

use super::LocalStreamChunk;

fn debug_log(msg: &str) {
    if std::env::var("TENEX_DEBUG")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
        eprintln!("[SOCKET] {}", msg);
    }
}

/// Lock info stored in the lockfile
#[derive(serde::Serialize, serde::Deserialize)]
struct LockInfo {
    pid: u32,
    started_at: u64,
}

/// Client for connecting to the local streaming socket
pub struct SocketStreamClient {
    socket_path: PathBuf,
    lock_path: PathBuf,
}

impl SocketStreamClient {
    pub fn new() -> Self {
        let socket_path = Self::default_socket_path();
        let lock_path = socket_path.with_extension("sock.lock");
        Self {
            socket_path,
            lock_path,
        }
    }

    fn default_socket_path() -> PathBuf {
        if let Ok(runtime_dir) = std::env::var("XDG_RUNTIME_DIR") {
            PathBuf::from(runtime_dir).join("tenex-stream.sock")
        } else {
            PathBuf::from("/tmp/tenex-stream.sock")
        }
    }

    /// Check if another process holds the lock and is still running
    fn is_locked_by_another(&self) -> bool {
        if !self.lock_path.exists() {
            return false;
        }

        match fs::read_to_string(&self.lock_path) {
            Ok(content) => {
                if let Ok(info) = serde_json::from_str::<LockInfo>(&content) {
                    // Check if the process is still running (signal 0 = just check existence)
                    let is_running = unsafe { libc::kill(info.pid as i32, 0) } == 0;
                    if is_running && info.pid != std::process::id() {
                        debug_log(&format!("Socket locked by another TUI (PID {})", info.pid));
                        return true;
                    }
                }
                false
            }
            Err(_) => false,
        }
    }

    /// Acquire the lock for this process
    fn acquire_lock(&self) -> bool {
        let info = LockInfo {
            pid: std::process::id(),
            started_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
        };

        if let Ok(content) = serde_json::to_string(&info) {
            if fs::write(&self.lock_path, content).is_ok() {
                debug_log("Acquired streaming socket lock");
                return true;
            }
        }
        false
    }

    /// Try to connect to the socket, returns None if socket doesn't exist
    pub async fn connect(&self) -> Option<UnixStream> {
        if !self.socket_path.exists() {
            debug_log(&format!(
                "Streaming socket not found at {:?}",
                self.socket_path
            ));
            return None;
        }

        match UnixStream::connect(&self.socket_path).await {
            Ok(stream) => {
                debug_log(&format!(
                    "Connected to streaming socket at {:?}",
                    self.socket_path
                ));
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
            // Check if another TUI already owns the socket
            if self.is_locked_by_another() {
                // Another TUI is using the socket - wait longer before checking again
                tokio::time::sleep(tokio::time::Duration::from_secs(10)).await;
                continue;
            }

            // Try to acquire the lock
            if !self.acquire_lock() {
                tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;
                continue;
            }

            if let Some(stream) = self.connect().await {
                if let Err(e) = self.read_stream(stream, &chunk_tx).await {
                    eprintln!("[SOCKET ERROR] Stream read error: {}", e);
                }
            }

            // Wait before reconnect attempt (keep the lock - we're retrying)
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
