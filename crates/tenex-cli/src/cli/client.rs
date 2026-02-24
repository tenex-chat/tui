use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::Duration;
use std::{collections::HashSet, io};

use anyhow::{Context, Result};

use super::config::CliConfig;
use super::daemon::socket_path as get_socket_path;
use super::protocol::{CliCommand, Response};

const MAX_WAIT_SECONDS: u64 = 10;
const POLL_INTERVAL_MS: u64 = 100;
const BOOT_WAIT_TIMEOUT_SECS: u64 = 60;
const BOOT_POLL_INTERVAL_MS: u64 = 500;
const REPLY_POLL_INTERVAL_MS: u64 = 500;
const BUNKER_WATCH_POLL_INTERVAL_MS: u64 = 500;

#[derive(Debug, Clone, serde::Deserialize)]
struct PendingBunkerRequest {
    request_id: String,
    requester_pubkey: String,
    event_kind: Option<u16>,
    #[serde(default)]
    event_content: Option<String>,
    #[serde(default)]
    received_at_ms: Option<u64>,
}

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
            cmd.env("TENEX_NSEC", &creds.key);
            if let Some(ref password) = creds.password {
                cmd.env("TENEX_NSEC_PASSWORD", password);
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

/// Send a command to the daemon and return the response
fn send_command_raw(
    command: &CliCommand,
    data_dir: &Path,
    config: Option<&CliConfig>,
) -> Result<Response> {
    let request = match command.to_request(1) {
        Some(r) => r,
        None => anyhow::bail!("Command cannot be sent to daemon"),
    };

    let stream = connect_to_daemon(data_dir, config)?;
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;

    // Send request
    let request_json = serde_json::to_string(&request)?;
    writeln!(writer, "{}", request_json)?;
    writer.flush()?;

    // Read response
    let mut response_line = String::new();
    reader.read_line(&mut response_line)?;

    Ok(serde_json::from_str(&response_line)?)
}

/// Get project info if online, None otherwise
fn get_booted_project(
    project_slug: &str,
    data_dir: &Path,
    config: Option<&CliConfig>,
) -> Result<Option<serde_json::Value>> {
    let response = send_command_raw(&CliCommand::ListProjects, data_dir, config)?;

    if let Some(result) = response.result {
        if let Some(projects) = result.as_array() {
            for project in projects {
                if project.get("slug").and_then(|s| s.as_str()) == Some(project_slug) {
                    if project
                        .get("booted")
                        .and_then(|b| b.as_bool())
                        .unwrap_or(false)
                    {
                        return Ok(Some(project.clone()));
                    }
                    return Ok(None);
                }
            }
        }
    }
    Ok(None)
}

/// Get messages from a thread, returns Vec of message objects
fn get_thread_messages(
    thread_id: &str,
    data_dir: &Path,
    config: Option<&CliConfig>,
) -> Result<Vec<serde_json::Value>> {
    let response = send_command_raw(
        &CliCommand::ListMessages {
            thread_id: thread_id.to_string(),
        },
        data_dir,
        config,
    )?;

    if let Some(result) = response.result {
        if let Some(messages) = result.as_array() {
            return Ok(messages.clone());
        }
    }
    Ok(vec![])
}

/// Wait for a reply message in the thread that wasn't sent by us (different from our_message_id)
fn wait_for_reply(
    thread_id: &str,
    our_message_id: &str,
    wait_secs: u64,
    data_dir: &Path,
    config: Option<&CliConfig>,
    pretty: bool,
) -> Result<bool> {
    let start = std::time::Instant::now();
    let timeout = Duration::from_secs(wait_secs);

    eprintln!("Waiting up to {} seconds for agent reply...", wait_secs);

    // Get current message IDs to know what's new
    let initial_messages = get_thread_messages(thread_id, data_dir, config)?;
    let initial_ids: std::collections::HashSet<String> = initial_messages
        .iter()
        .filter_map(|m| m.get("id").and_then(|id| id.as_str()).map(String::from))
        .collect();

    while start.elapsed() < timeout {
        thread::sleep(Duration::from_millis(REPLY_POLL_INTERVAL_MS));

        let messages = get_thread_messages(thread_id, data_dir, config)?;

        // Find new messages that weren't in the initial set and aren't our message
        for msg in &messages {
            let msg_id = msg.get("id").and_then(|id| id.as_str()).unwrap_or("");
            if !initial_ids.contains(msg_id) && msg_id != our_message_id {
                // Found a reply!
                eprintln!("Received reply:");
                if pretty {
                    println!("{}", serde_json::to_string_pretty(msg)?);
                } else {
                    println!("{}", serde_json::to_string(msg)?);
                }
                return Ok(true);
            }
        }
    }

    eprintln!("Timeout waiting for reply");
    Ok(false)
}

/// Send a command to the daemon and print the response
pub fn send_command(
    command: CliCommand,
    pretty: bool,
    data_dir: &Path,
    config: Option<CliConfig>,
) -> Result<()> {
    if let CliCommand::BunkerWatch = command {
        return watch_bunker_requests(data_dir, config.as_ref());
    }

    // Handle boot-project --wait specially
    if let CliCommand::BootProject {
        ref project_slug,
        wait: true,
    } = command
    {
        let response = send_command_raw(&command, data_dir, config.as_ref())?;

        if let Some(error) = response.error {
            eprintln!("Error [{}]: {}", error.code, error.message);
            std::process::exit(1);
        }

        // Poll until project is online
        eprintln!("Waiting for project '{}' to come online...", project_slug);
        let start = std::time::Instant::now();
        while start.elapsed().as_secs() < BOOT_WAIT_TIMEOUT_SECS {
            if let Some(project) = get_booted_project(project_slug, data_dir, config.as_ref())? {
                if pretty {
                    println!("{}", serde_json::to_string_pretty(&project)?);
                } else {
                    println!("{}", serde_json::to_string(&project)?);
                }
                return Ok(());
            }
            thread::sleep(Duration::from_millis(BOOT_POLL_INTERVAL_MS));
        }

        eprintln!("Timeout waiting for project to come online");
        std::process::exit(1);
    }

    // Handle create-thread with --wait
    if let CliCommand::CreateThread {
        wait_secs: Some(wait_secs),
        ..
    } = command
    {
        let response = send_command_raw(&command, data_dir, config.as_ref())?;

        if let Some(error) = response.error {
            eprintln!("Error [{}]: {}", error.code, error.message);
            std::process::exit(1);
        }

        if let Some(result) = response.result {
            // Always print the create result first (contains thread_id)
            if pretty {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", serde_json::to_string(&result)?);
            }

            // Extract thread_id for waiting
            if let Some(thread_id) = result.get("thread_id").and_then(|t| t.as_str()) {
                wait_for_reply(
                    thread_id,
                    thread_id,
                    wait_secs,
                    data_dir,
                    config.as_ref(),
                    pretty,
                )?;
            } else {
                eprintln!("Warning: Could not get thread_id, cannot wait for reply");
            }
        }

        return Ok(());
    }

    // Handle send-message with --wait
    if let CliCommand::SendMessage {
        ref thread_id,
        wait_secs: Some(wait_secs),
        ..
    } = command
    {
        let thread_id_for_wait = thread_id.clone();
        let response = send_command_raw(&command, data_dir, config.as_ref())?;

        if let Some(error) = response.error {
            eprintln!("Error [{}]: {}", error.code, error.message);
            std::process::exit(1);
        }

        if let Some(result) = response.result {
            // Always print the send result first (contains message_id)
            if pretty {
                println!("{}", serde_json::to_string_pretty(&result)?);
            } else {
                println!("{}", serde_json::to_string(&result)?);
            }

            // Extract message_id for waiting
            let our_message_id = result
                .get("message_id")
                .and_then(|m| m.as_str())
                .unwrap_or("");
            wait_for_reply(
                &thread_id_for_wait,
                our_message_id,
                wait_secs,
                data_dir,
                config.as_ref(),
                pretty,
            )?;
        }

        return Ok(());
    }

    let response = send_command_raw(&command, data_dir, config.as_ref())?;

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

fn watch_bunker_requests(data_dir: &Path, config: Option<&CliConfig>) -> Result<()> {
    let status_response = send_command_raw(&CliCommand::BunkerStatus, data_dir, config)?;
    if let Some(error) = status_response.error {
        anyhow::bail!("Error [{}]: {}", error.code, error.message);
    }
    let is_running = status_response
        .result
        .as_ref()
        .and_then(|r| r.get("running"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);
    if !is_running {
        anyhow::bail!("Bunker is not running. Start it with `tenex-cli bunker start`.");
    }

    eprintln!("Watching bunker requests. Actions: a=approve, A=approve+rule, r=reject, s=skip, q=quit");

    let mut seen_request_ids = HashSet::new();
    loop {
        let response = send_command_raw(&CliCommand::BunkerListPending, data_dir, config)?;
        if let Some(error) = response.error {
            anyhow::bail!("Error [{}]: {}", error.code, error.message);
        }
        let pending = parse_pending_requests(response.result);

        for request in pending {
            if seen_request_ids.contains(&request.request_id) {
                continue;
            }
            seen_request_ids.insert(request.request_id.clone());

            println!();
            println!("Request ID: {}", request.request_id);
            println!("Requester: {}", request.requester_pubkey);
            println!(
                "Event kind: {}",
                request
                    .event_kind
                    .map(|k| k.to_string())
                    .unwrap_or_else(|| "unknown".to_string())
            );
            if let Some(received_at_ms) = request.received_at_ms {
                println!("Received at (ms): {}", received_at_ms);
            }
            if let Some(content) = request.event_content.as_deref() {
                let preview: String = content.chars().take(160).collect();
                if content.chars().count() > 160 {
                    println!("Content: {}...", preview);
                } else {
                    println!("Content: {}", preview);
                }
            }

            loop {
                print!("[a/A/r/s/q] > ");
                io::stdout().flush()?;

                let mut input = String::new();
                io::stdin().read_line(&mut input)?;
                let action = input.trim().chars().next();

                match action {
                    Some('a') => {
                        send_bunker_response(data_dir, config, &request.request_id, true)?;
                        println!("Approved");
                        break;
                    }
                    Some('A') => {
                        send_bunker_response(data_dir, config, &request.request_id, true)?;
                        send_bunker_rule_add(
                            data_dir,
                            config,
                            &request.requester_pubkey,
                            request.event_kind,
                        )?;
                        println!("Approved and persisted auto-approve rule");
                        break;
                    }
                    Some('r') => {
                        send_bunker_response(data_dir, config, &request.request_id, false)?;
                        println!("Rejected");
                        break;
                    }
                    Some('s') => {
                        println!("Skipped");
                        break;
                    }
                    Some('q') => return Ok(()),
                    _ => {
                        println!("Invalid action. Use a, A, r, s, or q.");
                    }
                }
            }
        }

        thread::sleep(Duration::from_millis(BUNKER_WATCH_POLL_INTERVAL_MS));
    }
}

fn parse_pending_requests(result: Option<serde_json::Value>) -> Vec<PendingBunkerRequest> {
    let Some(result) = result else {
        return Vec::new();
    };

    let pending_value = result.get("pending").cloned().unwrap_or(result);
    serde_json::from_value::<Vec<PendingBunkerRequest>>(pending_value).unwrap_or_default()
}

fn send_bunker_response(
    data_dir: &Path,
    config: Option<&CliConfig>,
    request_id: &str,
    approved: bool,
) -> Result<()> {
    let response = send_command_raw(
        &CliCommand::BunkerRespond {
            request_id: request_id.to_string(),
            approved,
        },
        data_dir,
        config,
    )?;

    if let Some(error) = response.error {
        anyhow::bail!("Error [{}]: {}", error.code, error.message);
    }
    Ok(())
}

fn send_bunker_rule_add(
    data_dir: &Path,
    config: Option<&CliConfig>,
    requester_pubkey: &str,
    event_kind: Option<u16>,
) -> Result<()> {
    let response = send_command_raw(
        &CliCommand::BunkerRulesAdd {
            requester_pubkey: requester_pubkey.to_string(),
            event_kind,
        },
        data_dir,
        config,
    )?;

    if let Some(error) = response.error {
        anyhow::bail!("Error [{}]: {}", error.code, error.message);
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
