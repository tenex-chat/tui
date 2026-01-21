use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;

use anyhow::{Context, Result};

use super::config::CliConfig;
use super::daemon::socket_path as get_socket_path;
use super::protocol::{CliCommand, Response};

const MAX_WAIT_SECONDS: u64 = 10;
const POLL_INTERVAL_MS: u64 = 100;

/// Connect to the daemon, auto-spawning if needed
fn connect_to_daemon(data_dir: &Path, config: Option<&CliConfig>) -> Result<UnixStream> {
    let socket_path = get_socket_path(data_dir);

    // Try to connect first
    if let Ok(stream) = UnixStream::connect(&socket_path) {
        return Ok(stream);
    }

    // Socket doesn't exist or daemon not running - spawn it
    eprintln!("Daemon not running, starting...");
    spawn_daemon(data_dir, config)?;

    // Wait for socket to become available
    let start = std::time::Instant::now();
    while start.elapsed().as_secs() < MAX_WAIT_SECONDS {
        if socket_path.exists() {
            if let Ok(stream) = UnixStream::connect(&socket_path) {
                eprintln!("Connected to daemon");
                return Ok(stream);
            }
        }
        thread::sleep(Duration::from_millis(POLL_INTERVAL_MS));
    }

    anyhow::bail!("Timed out waiting for daemon to start")
}

/// Spawn the daemon as a background process
fn spawn_daemon(data_dir: &Path, config: Option<&CliConfig>) -> Result<()> {
    // Get the path to our own executable
    let exe_path = std::env::current_exe().context("Failed to get executable path")?;

    let mut cmd = Command::new(&exe_path);
    cmd.arg("--daemon");
    cmd.arg("--data-dir").arg(data_dir);

    // Pass credentials via environment to avoid them showing in process list
    if let Some(cfg) = config {
        if let Some(ref creds) = cfg.credentials {
            cmd.env("TENEX_KEY", &creds.key);
            if let Some(ref password) = creds.password {
                cmd.env("TENEX_KEY_PASSWORD", password);
            }
        }
    }

    // Spawn as detached process
    cmd.stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::inherit()) // Keep stderr for debugging
        .spawn()
        .context("Failed to spawn daemon")?;

    Ok(())
}

/// Send a command to the daemon and get the response
pub fn send_command(command: CliCommand, pretty: bool, data_dir: &Path, config: Option<CliConfig>) -> Result<()> {
    let request = match command.to_request(1) {
        Some(r) => r,
        None => anyhow::bail!("Command cannot be sent to daemon"),
    };

    let stream = connect_to_daemon(data_dir, config.as_ref())?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    // Send request
    let request_json = serde_json::to_string(&request)?;
    writeln!(writer, "{}", request_json)?;
    writer.flush()?;

    // Read response
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;

    let response: Response = serde_json::from_str(&response_line)?;

    // Print result
    if let Some(error) = response.error {
        eprintln!("Error [{}]: {}", error.code, error.message);
        std::process::exit(1);
    }

    if let Some(result) = response.result {
        if pretty {
            println!("{}", serde_json::to_string_pretty(&result)?);
        } else {
            println!("{}", serde_json::to_string(&result)?);
        }
    }

    Ok(())
}

/// Check if the daemon is running
pub fn is_daemon_running(data_dir: &Path) -> bool {
    let socket_path = get_socket_path(data_dir);
    if !socket_path.exists() {
        return false;
    }

    // Try to connect
    UnixStream::connect(&socket_path).is_ok()
}

/// Get the socket path for external use
pub fn socket_path(data_dir: &Path) -> PathBuf {
    get_socket_path(data_dir)
}
