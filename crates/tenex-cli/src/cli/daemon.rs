use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use tokio::net::UnixListener;
use tokio::sync::broadcast;

use anyhow::Result;
use serde::Deserialize;

use crate::cli::http::run_server;
use crate::nostr::{self, DataChange, NostrCommand};
use crate::store::AppDataStore;
use tenex_core::config::CoreConfig;
use tenex_core::models::PreferencesStorage;
use tenex_core::nostr::set_log_path;
use tenex_core::runtime::{CoreHandle, CoreRuntime};
use tenex_core::slug::{validate_slug, SlugValidation};

use super::config::CliConfig;
use super::protocol::{Request, Response};

const SOCKET_NAME: &str = "tenex-cli.sock";
const PID_FILE: &str = "daemon.pid";
const LOG_FILE: &str = "tenex.log";

/// Get socket path from data directory
pub fn socket_path(data_dir: &Path) -> PathBuf {
    data_dir.join(SOCKET_NAME)
}

/// Run the daemon server
#[tokio::main]
pub async fn run_daemon(
    data_dir: PathBuf,
    config: Option<CliConfig>,
    http_enabled: bool,
    http_bind: String,
) -> Result<()> {
    eprintln!("Starting tenex-cli daemon...");

    // Ensure data directory exists
    fs::create_dir_all(&data_dir)?;

    // Set log path before any logging happens
    let log_path = data_dir.join(LOG_FILE);
    set_log_path(log_path.clone());
    eprintln!("Log file: {:?}", log_path);

    // Socket path
    let socket_path = data_dir.join(SOCKET_NAME);

    // Remove stale socket if exists
    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }

    // Write PID file
    let pid_path = data_dir.join(PID_FILE);
    fs::write(&pid_path, std::process::id().to_string())?;

    // Bind socket early so clients can connect while we initialize
    let listener = UnixListener::bind(&socket_path)?;
    eprintln!("Listening on {:?}", socket_path);

    // Initialize core runtime
    let mut core_runtime = CoreRuntime::new(CoreConfig::new(&data_dir))?;
    let core_handle = core_runtime.handle();
    let data_rx = core_runtime
        .take_data_rx()
        .expect("data_rx should be available");

    // Initialize preferences for credential storage and trusted backends
    let prefs = PreferencesStorage::new(data_dir.to_str().unwrap_or("tenex_data"));

    // Create a SINGLE shared data store for both Unix socket and HTTP handlers.
    // This ensures both see the same projects, threads, and messages.
    // Note: Using std::sync::Mutex because daemon has sync code paths (handle_connection)
    let shared_data_store = Arc::new(Mutex::new(AppDataStore::new(core_runtime.ndb())));

    // Create a broadcast channel for DataChange events.
    // This allows multiple consumers (HTTP SSE streams + daemon) to receive all events.
    // FIX: Previously used Arc<Mutex<Receiver>> which caused data loss - first consumer
    // would steal all events from other consumers.
    let (broadcast_tx, _broadcast_rx) = broadcast::channel::<DataChange>(1024);
    let broadcast_tx_for_http = broadcast_tx.clone();
    let broadcast_tx_for_forward = broadcast_tx.clone();

    // Spawn a task to forward from the original mpsc receiver to the broadcast channel
    let data_rx_mutex = Arc::new(Mutex::new(data_rx));
    let data_rx_for_forward = data_rx_mutex.clone();
    tokio::spawn(async move {
        loop {
            // Non-blocking receive from the original channel
            let data_change = {
                let rx = data_rx_for_forward.lock().unwrap();
                rx.try_recv()
            };

            match data_change {
                Ok(change) => {
                    // Forward to broadcast channel (ignore send errors - no subscribers is OK)
                    let _ = broadcast_tx_for_forward.send(change);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {
                    // No data available, sleep briefly
                    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    // Channel closed, exit the forwarding loop
                    break;
                }
            }
        }
    });

    // Set trusted backends from preferences
    {
        let approved = prefs.approved_backend_pubkeys().clone();
        let blocked = prefs.blocked_backend_pubkeys().clone();
        shared_data_store
            .lock()
            .unwrap()
            .trust
            .set_trusted_backends(approved, blocked);
    }

    // Try to auto-login: config credentials take priority over stored credentials
    let keys = try_auto_login_with_config(config.as_ref(), &prefs, &core_handle);
    if keys.is_some() {
        eprintln!("Auto-login successful");
    } else {
        eprintln!("No stored credentials or password required - daemon running without login");
    }

    // Track state
    let start_time = Instant::now();
    let ndb = core_runtime.ndb();

    // Spawn HTTP server if enabled (shares the same data store)
    let http_task = if http_enabled {
        let http_core_handle = core_handle.clone();
        let http_store_clone = shared_data_store.clone();
        Some(tokio::spawn(async move {
            if let Err(e) = run_server(
                http_bind,
                http_core_handle,
                http_store_clone,
                broadcast_tx_for_http,
            )
            .await
            {
                eprintln!("HTTP server error: {}", e);
            }
        }))
    } else {
        None
    };

    // Subscribe to broadcast for daemon's own use (handling ProjectStatus)
    let mut daemon_rx = broadcast_tx.subscribe();

    // Main event loop - unified for both HTTP and socket-only modes
    loop {
        // Drain any pending DataChange events from broadcast (non-blocking)
        // Update the shared data store with status events
        loop {
            match daemon_rx.try_recv() {
                Ok(data_change) => {
                    if let DataChange::ProjectStatus { json } = &data_change {
                        shared_data_store
                            .lock()
                            .unwrap()
                            .handle_status_event_json(json);
                    }
                }
                Err(broadcast::error::TryRecvError::Empty) => break,
                Err(broadcast::error::TryRecvError::Lagged(_)) => continue, // Skip lagged messages
                Err(broadcast::error::TryRecvError::Closed) => break,
            }
        }

        tokio::select! {
            accept_result = listener.accept() => {
                match accept_result {
                    Ok((stream, _)) => {
                        // Convert tokio UnixStream to std UnixStream for blocking I/O
                        let std_stream = stream.into_std()?;
                        std_stream.set_nonblocking(false)?;
                        let should_shutdown = handle_connection(
                            std_stream,
                            &shared_data_store,
                            &core_handle,
                            start_time,
                            keys.is_some(),
                        )?;

                        if should_shutdown {
                            eprintln!("Shutdown requested");
                            break;
                        }
                    }
                    Err(e) => {
                        eprintln!("Connection error: {}", e);
                    }
                }
            }
            Some(note_keys) = core_runtime.next_note_keys() => {
                // Process note keys and update the SHARED data store
                // This ensures both HTTP and Unix socket see the same data
                let mut store = shared_data_store.lock().unwrap();
                if let Err(e) = tenex_core::runtime::process_note_keys(
                    ndb.as_ref(),
                    &mut store,
                    &core_handle,
                    &note_keys,
                ) {
                    eprintln!("Failed to process core events: {}", e);
                }
            }
            // Small timeout to periodically check for DataChange events
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
        }
    }

    // Abort HTTP task on shutdown if it was running
    if let Some(task) = http_task {
        task.abort();
    }

    // Cleanup
    core_runtime.shutdown();
    fs::remove_file(&socket_path).ok();
    fs::remove_file(&pid_path).ok();

    eprintln!("Daemon stopped");
    Ok(())
}

/// Try to login with config credentials first, then env vars, then stored credentials
fn try_auto_login_with_config(
    config: Option<&CliConfig>,
    prefs: &PreferencesStorage,
    core_handle: &CoreHandle,
) -> Option<nostr_sdk::Keys> {
    // Try config credentials first
    if let Some(cfg) = config {
        if let Some(ref creds) = cfg.credentials {
            match try_login_with_credentials(&creds.key, creds.password.as_deref(), core_handle) {
                Ok(keys) => return Some(keys),
                Err(e) => {
                    eprintln!("Failed to login with config credentials: {}", e);
                }
            }
        }
    }

    // Try environment variables (used when daemon is spawned by client, or set manually)
    if let Ok(key) = std::env::var("TENEX_NSEC") {
        let password = std::env::var("TENEX_NSEC_PASSWORD").ok();
        match try_login_with_credentials(&key, password.as_deref(), core_handle) {
            Ok(keys) => return Some(keys),
            Err(e) => {
                eprintln!("Failed to login with TENEX_NSEC: {}", e);
            }
        }
    }

    // Fall back to stored credentials
    try_auto_login(prefs, core_handle)
}

/// Try to parse and login with the provided key (nsec or ncryptsec)
fn try_login_with_credentials(
    key: &str,
    password: Option<&str>,
    core_handle: &CoreHandle,
) -> anyhow::Result<nostr_sdk::Keys> {
    use nostr_sdk::prelude::*;

    let keys = if key.starts_with("ncryptsec") {
        // Encrypted key - needs password
        let password = password.ok_or_else(|| {
            anyhow::anyhow!("Password required for ncryptsec but not provided in config")
        })?;
        let encrypted = EncryptedSecretKey::from_bech32(key)?;
        let secret_key = encrypted.decrypt(password)?;
        Keys::new(secret_key)
    } else if key.starts_with("nsec") {
        // Unencrypted nsec
        let secret_key = SecretKey::from_bech32(key)?;
        Keys::new(secret_key)
    } else {
        return Err(anyhow::anyhow!(
            "Invalid key format: expected nsec or ncryptsec"
        ));
    };

    let pubkey = crate::nostr::get_current_pubkey(&keys);
    core_handle
        .send(NostrCommand::Connect {
            keys: keys.clone(),
            user_pubkey: pubkey,
            response_tx: None,
        })
        .map_err(|_| anyhow::anyhow!("Failed to send Connect command"))?;

    Ok(keys)
}

fn try_auto_login(prefs: &PreferencesStorage, core_handle: &CoreHandle) -> Option<nostr_sdk::Keys> {
    if !nostr::has_stored_credentials(prefs) {
        return None;
    }

    // Check if password required
    if nostr::credentials_need_password(prefs) {
        // Try to get password from environment
        if let Ok(password) = std::env::var("TENEX_PASSWORD") {
            match nostr::load_stored_keys(&password, prefs) {
                Ok(keys) => {
                    let pubkey = nostr::get_current_pubkey(&keys);
                    if core_handle
                        .send(NostrCommand::Connect {
                            keys: keys.clone(),
                            user_pubkey: pubkey,
                            response_tx: None,
                        })
                        .is_ok()
                    {
                        return Some(keys);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to decrypt credentials: {}", e);
                }
            }
        }
        return None;
    }

    // Unencrypted credentials
    match nostr::load_unencrypted_keys(prefs) {
        Ok(keys) => {
            let pubkey = nostr::get_current_pubkey(&keys);
            if core_handle
                .send(NostrCommand::Connect {
                    keys: keys.clone(),
                    user_pubkey: pubkey,
                    response_tx: None,
                })
                .is_ok()
            {
                return Some(keys);
            }
        }
        Err(e) => {
            eprintln!("Failed to load credentials: {}", e);
        }
    }

    None
}

fn handle_connection(
    stream: UnixStream,
    data_store: &Arc<Mutex<AppDataStore>>,
    core_handle: &CoreHandle,
    start_time: Instant,
    logged_in: bool,
) -> Result<bool> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut line = String::new();

    while reader.read_line(&mut line)? > 0 {
        let request: Request = match serde_json::from_str(&line) {
            Ok(r) => r,
            Err(e) => {
                let response = Response::error(0, "PARSE_ERROR", &e.to_string());
                writeln!(writer, "{}", serde_json::to_string(&response)?)?;
                line.clear();
                continue;
            }
        };

        let (response, should_shutdown) =
            handle_request(&request, data_store, core_handle, start_time, logged_in);

        writeln!(writer, "{}", serde_json::to_string(&response)?)?;
        writer.flush()?;

        if should_shutdown {
            return Ok(true);
        }

        line.clear();
    }

    Ok(false)
}

fn handle_request(
    request: &Request,
    data_store: &Arc<Mutex<AppDataStore>>,
    core_handle: &CoreHandle,
    _start_time: Instant,
    logged_in: bool,
) -> (Response, bool) {
    let id = request.id;

    match request.method.as_str() {
        "list_projects" => {
            let store = data_store.lock().unwrap();
            let projects: Vec<_> = store
                .get_projects()
                .iter()
                .map(|p| {
                    let a_tag = p.a_tag();
                    let online_agents = store.get_online_agents(&a_tag);
                    let mut obj = serde_json::json!({
                        "slug": p.id,
                        "name": p.name,
                        "booted": online_agents.is_some(),
                    });
                    if let Some(agents) = online_agents {
                        obj["participants"] =
                            serde_json::json!(agents.iter().map(agent_to_json).collect::<Vec<_>>());
                    }
                    obj
                })
                .collect();
            (Response::success(id, serde_json::json!(projects)), false)
        }

        "list_threads" => {
            let project_slug = request.params["project_slug"].as_str().unwrap_or("");
            let wait_for_project = request.params["wait_for_project"]
                .as_bool()
                .unwrap_or(false);

            // If wait_for_project is true, wait for the 24010 event first
            if wait_for_project {
                if let Err(err_response) = wait_for_project_status(data_store, project_slug, id) {
                    return err_response;
                }
            }

            let store = data_store.lock().unwrap();

            let project_a_tag = match find_project_a_tag_by_slug(&store, project_slug) {
                Some(a_tag) => a_tag,
                None => {
                    return (
                        Response::error(
                            id,
                            "PROJECT_NOT_FOUND",
                            &format!("Project '{}' not found", project_slug),
                        ),
                        false,
                    );
                }
            };

            let threads: Vec<_> = store
                .get_threads(&project_a_tag)
                .iter()
                .map(|t| {
                    serde_json::json!({
                        "id": t.id,
                        "title": t.title,
                        "last_activity": t.last_activity,
                        "pubkey": t.pubkey,
                    })
                })
                .collect();
            (Response::success(id, serde_json::json!(threads)), false)
        }

        "list_agents" => {
            let project_slug = request.params["project_slug"].as_str().unwrap_or("");
            let wait_for_project = request.params["wait_for_project"]
                .as_bool()
                .unwrap_or(false);

            // If wait_for_project is true, wait for the 24010 event first
            if wait_for_project {
                if let Err(err_response) = wait_for_project_status(data_store, project_slug, id) {
                    return err_response;
                }
            }

            let store = data_store.lock().unwrap();

            let project_a_tag = match find_project_a_tag_by_slug(&store, project_slug) {
                Some(a_tag) => a_tag,
                None => {
                    return (
                        Response::error(
                            id,
                            "PROJECT_NOT_FOUND",
                            &format!("Project '{}' not found", project_slug),
                        ),
                        false,
                    );
                }
            };

            let agents: Vec<_> = store
                .get_online_agents(&project_a_tag)
                .map(|agents| agents.iter().map(agent_to_json).collect())
                .unwrap_or_default();
            (Response::success(id, serde_json::json!(agents)), false)
        }

        "list_messages" => {
            let thread_id = request.params["thread_id"].as_str().unwrap_or("");
            let store = data_store.lock().unwrap();
            let messages: Vec<_> = store
                .get_messages(thread_id)
                .iter()
                .map(|m| {
                    let mut obj = serde_json::json!({
                        "id": m.id,
                        "content": m.content,
                        "created_at": m.created_at,
                        "pubkey": m.pubkey,
                    });
                    if let Some(name) = resolve_author_name(&store, &m.pubkey) {
                        obj["author_name"] = serde_json::json!(name);
                    }
                    obj
                })
                .collect();
            (Response::success(id, serde_json::json!(messages)), false)
        }

        "get_state" => {
            let store = data_store.lock().unwrap();
            let projects = store.get_projects();
            let mut thread_count = 0;
            let mut message_count = 0;

            for project in projects {
                let threads = store.get_threads(&project.a_tag());
                thread_count += threads.len();
                for thread in threads {
                    message_count += store.get_messages(&thread.id).len();
                }
            }

            (
                Response::success(
                    id,
                    serde_json::json!({
                        "projects": projects.len(),
                        "threads": thread_count,
                        "messages": message_count,
                        "logged_in": logged_in,
                    }),
                ),
                false,
            )
        }

        "send_message" => {
            let project_slug = request.params["project_slug"]
                .as_str()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let thread_id = request.params["thread_id"]
                .as_str()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let recipient_slug = request.params["recipient_slug"]
                .as_str()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let content = request.params["content"]
                .as_str()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let wait_for_project = request.params["wait_for_project"]
                .as_bool()
                .unwrap_or(false);

            // Validate skill_ids parameter with comprehensive checks
            let skill_ids = match validate_skill_ids_param(&request.params["skill_ids"]) {
                Ok(ids) => ids,
                Err(msg) => {
                    return (Response::error(id, "INVALID_PARAMS", &msg), false);
                }
            };

            // Validate nudge_ids parameter with comprehensive checks
            let nudge_ids = match validate_nudge_ids_param(&request.params["nudge_ids"]) {
                Ok(ids) => ids,
                Err(msg) => {
                    return (Response::error(id, "INVALID_PARAMS", &msg), false);
                }
            };

            let (project_slug, thread_id, recipient_slug, content) =
                match (project_slug, thread_id, recipient_slug, content) {
                    (Some(p), Some(t), Some(r), Some(c)) => (p, t, r, c),
                    _ => {
                        return (
                            Response::error(
                                id,
                                "INVALID_PARAMS",
                                "project_slug, thread_id, recipient_slug, and content are required",
                            ),
                            false,
                        );
                    }
                };

            // If wait_for_project is true, wait for the 24010 event first
            if wait_for_project {
                if let Err(err_response) = wait_for_project_status(data_store, project_slug, id) {
                    return err_response;
                }
            }

            let store = data_store.lock().unwrap();
            let lookup = find_agent_in_project(&store, project_slug, recipient_slug);
            drop(store);

            match lookup {
                Ok(result) => {
                    // Create response channel to get the event ID back
                    let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

                    match core_handle.send(NostrCommand::PublishMessage {
                        thread_id: thread_id.to_string(),
                        project_a_tag: result.project_a_tag,
                        content: content.to_string(),
                        agent_pubkey: Some(result.agent_pubkey),
                        reply_to: Some(thread_id.to_string()),
                        nudge_ids,
                        skill_ids,
                        ask_author_pubkey: None,
                        response_tx: Some(response_tx),
                    }) {
                        Ok(_) => {
                            // Wait for the event ID (with timeout)
                            match response_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                                Ok(message_id) => (
                                    Response::success(
                                        id,
                                        serde_json::json!({
                                            "status": "sent",
                                            "message_id": message_id
                                        }),
                                    ),
                                    false,
                                ),
                                Err(_) => (
                                    Response::success(
                                        id,
                                        serde_json::json!({
                                            "status": "sent",
                                            "message_id": null
                                        }),
                                    ),
                                    false,
                                ),
                            }
                        }
                        Err(e) => (
                            Response::error(
                                id,
                                "SEND_FAILED",
                                &format!("Failed to send message: {}", e),
                            ),
                            false,
                        ),
                    }
                }
                Err(AgentLookupError::ProjectNotFound) => (
                    Response::error(
                        id,
                        "PROJECT_NOT_FOUND",
                        &format!("Project '{}' not found", project_slug),
                    ),
                    false,
                ),
                Err(AgentLookupError::AgentNotFound) => (
                    Response::error(
                        id,
                        "AGENT_NOT_FOUND",
                        &format!(
                            "Agent with slug '{}' not found in project '{}'",
                            recipient_slug, project_slug
                        ),
                    ),
                    false,
                ),
            }
        }

        "create_thread" => {
            let project_slug = request.params["project_slug"]
                .as_str()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let recipient_slug = request.params["recipient_slug"]
                .as_str()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let content = request.params["content"]
                .as_str()
                .map(|s| s.trim())
                .filter(|s| !s.is_empty());
            let wait_for_project = request.params["wait_for_project"]
                .as_bool()
                .unwrap_or(false);

            // Validate skill_ids parameter with comprehensive checks
            let skill_ids = match validate_skill_ids_param(&request.params["skill_ids"]) {
                Ok(ids) => ids,
                Err(msg) => {
                    return (Response::error(id, "INVALID_PARAMS", &msg), false);
                }
            };

            // Validate nudge_ids parameter with comprehensive checks
            let nudge_ids = match validate_nudge_ids_param(&request.params["nudge_ids"]) {
                Ok(ids) => ids,
                Err(msg) => {
                    return (Response::error(id, "INVALID_PARAMS", &msg), false);
                }
            };

            let (project_slug, recipient_slug, content) =
                match (project_slug, recipient_slug, content) {
                    (Some(p), Some(r), Some(c)) => (p, r, c),
                    _ => {
                        return (
                            Response::error(
                                id,
                                "INVALID_PARAMS",
                                "project_slug, recipient_slug, and content are required",
                            ),
                            false,
                        );
                    }
                };

            // If wait_for_project is true, wait for the 24010 event first
            if wait_for_project {
                if let Err(err_response) = wait_for_project_status(data_store, project_slug, id) {
                    return err_response;
                }
            }

            let store = data_store.lock().unwrap();
            let lookup = find_agent_in_project(&store, project_slug, recipient_slug);
            drop(store);

            match lookup {
                Ok(result) => {
                    // Use a truncated version of content for the title (first 50 chars)
                    // Use chars() to safely handle multi-byte UTF-8 characters
                    let title: String = if content.chars().count() > 50 {
                        format!("{}...", content.chars().take(50).collect::<String>())
                    } else {
                        content.to_string()
                    };

                    // Create response channel to get the event ID back
                    let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

                    match core_handle.send(NostrCommand::PublishThread {
                        project_a_tag: result.project_a_tag,
                        title,
                        content: content.to_string(),
                        agent_pubkey: Some(result.agent_pubkey),
                        nudge_ids,
                        skill_ids,
                        reference_conversation_id: None,
                        fork_message_id: None,
                        response_tx: Some(response_tx),
                    }) {
                        Ok(_) => {
                            // Wait for the event ID (with timeout)
                            match response_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                                Ok(thread_id) => (
                                    Response::success(
                                        id,
                                        serde_json::json!({
                                            "status": "created",
                                            "thread_id": thread_id
                                        }),
                                    ),
                                    false,
                                ),
                                Err(_) => (
                                    Response::success(
                                        id,
                                        serde_json::json!({
                                            "status": "created",
                                            "thread_id": null
                                        }),
                                    ),
                                    false,
                                ),
                            }
                        }
                        Err(e) => (
                            Response::error(
                                id,
                                "CREATE_FAILED",
                                &format!("Failed to create thread: {}", e),
                            ),
                            false,
                        ),
                    }
                }
                Err(AgentLookupError::ProjectNotFound) => (
                    Response::error(
                        id,
                        "PROJECT_NOT_FOUND",
                        &format!("Project '{}' not found", project_slug),
                    ),
                    false,
                ),
                Err(AgentLookupError::AgentNotFound) => (
                    Response::error(
                        id,
                        "AGENT_NOT_FOUND",
                        &format!(
                            "Agent with slug '{}' not found in project '{}'",
                            recipient_slug, project_slug
                        ),
                    ),
                    false,
                ),
            }
        }

        "boot_project" => {
            let project_slug = request.params["project_slug"].as_str().unwrap_or("");

            if project_slug.is_empty() {
                return (
                    Response::error(id, "INVALID_PARAMS", "project_slug is required"),
                    false,
                );
            }

            // Find the project by slug to get its a_tag and pubkey
            let store = data_store.lock().unwrap();
            let project = store
                .get_projects()
                .iter()
                .find(|p| p.id == project_slug)
                .map(|p| (p.a_tag(), p.pubkey.clone()));
            drop(store);

            let (project_a_tag, project_pubkey) = match project {
                Some((a_tag, pubkey)) => (a_tag, Some(pubkey)),
                None => {
                    return (
                        Response::error(
                            id,
                            "PROJECT_NOT_FOUND",
                            &format!("Project '{}' not found", project_slug),
                        ),
                        false,
                    );
                }
            };

            if core_handle
                .send(NostrCommand::BootProject {
                    project_a_tag,
                    project_pubkey,
                })
                .is_ok()
            {
                (
                    Response::success(id, serde_json::json!({"status": "boot_sent"})),
                    false,
                )
            } else {
                (
                    Response::error(id, "BOOT_FAILED", "Failed to send boot request"),
                    false,
                )
            }
        }

        "status" => (
            Response::success(
                id,
                serde_json::json!({
                    "status": "running",
                    "logged_in": logged_in,
                }),
            ),
            false,
        ),

        "shutdown" => (
            Response::success(id, serde_json::json!({"status": "shutting_down"})),
            true,
        ),

        "list_agent_definitions" => {
            let store = data_store.lock().unwrap();
            let agent_defs: Vec<_> = store
                .content
                .get_agent_definitions()
                .iter()
                .map(|ad| {
                    serde_json::json!({
                        "id": ad.id,
                        "pubkey": ad.pubkey,
                        "d_tag": ad.d_tag,
                        "name": ad.name,
                        "description": ad.description,
                        "role": ad.role,
                        "instructions": ad.instructions,
                        "picture": ad.picture,
                        "version": ad.version,
                        "model": ad.model,
                        "tools": ad.tools,
                        "mcp_servers": ad.mcp_servers,
                        "use_criteria": ad.use_criteria,
                        "created_at": ad.created_at,
                    })
                })
                .collect();
            (Response::success(id, serde_json::json!(agent_defs)), false)
        }

        "list_mcp_tools" => {
            let store = data_store.lock().unwrap();
            let mcp_tools: Vec<_> = store
                .content
                .get_mcp_tools()
                .iter()
                .map(|tool| {
                    serde_json::json!({
                        "id": tool.id,
                        "pubkey": tool.pubkey,
                        "d_tag": tool.d_tag,
                        "name": tool.name,
                        "description": tool.description,
                        "command": tool.command,
                        "parameters": tool.parameters,
                        "capabilities": tool.capabilities,
                        "server_url": tool.server_url,
                        "version": tool.version,
                        "created_at": tool.created_at,
                    })
                })
                .collect();
            (Response::success(id, serde_json::json!(mcp_tools)), false)
        }

        "list_skills" => {
            let store = data_store.lock().unwrap();
            let skills: Vec<_> = store
                .content
                .get_skills()
                .iter()
                .map(|skill| {
                    serde_json::json!({
                        "id": skill.id,
                        "pubkey": skill.pubkey,
                        "title": skill.title,
                        "description": skill.description,
                        "content": skill.content,
                        "hashtags": skill.hashtags,
                        "file_ids": skill.file_ids,
                        "created_at": skill.created_at,
                    })
                })
                .collect();
            (Response::success(id, serde_json::json!(skills)), false)
        }

        "list_nudges" => {
            let store = data_store.lock().unwrap();
            let nudges: Vec<_> = store
                .content
                .get_nudges()
                .iter()
                .map(|nudge| {
                    serde_json::json!({
                        "id": nudge.id,
                        "pubkey": nudge.pubkey,
                        "title": nudge.title,
                        "description": nudge.description,
                        "content": nudge.content,
                        "hashtags": nudge.hashtags,
                        "allowed_tools": nudge.allowed_tools,
                        "denied_tools": nudge.denied_tools,
                        "only_tools": nudge.only_tools,
                        "supersedes": nudge.supersedes,
                        "created_at": nudge.created_at,
                    })
                })
                .collect();
            (Response::success(id, serde_json::json!(nudges)), false)
        }

        "show_project" => {
            let project_slug = request.params["project_slug"].as_str().unwrap_or("");
            let wait_for_project = request.params["wait_for_project"]
                .as_bool()
                .unwrap_or(false);

            // If wait_for_project is true, wait for the 24010 event first
            if wait_for_project {
                if let Err(err_response) = wait_for_project_status(data_store, project_slug, id) {
                    return err_response;
                }
            }

            let store = data_store.lock().unwrap();

            // Find the project by slug
            let project = store
                .get_projects()
                .iter()
                .find(|p| p.id == project_slug)
                .cloned();

            let project = match project {
                Some(p) => p,
                None => {
                    return (
                        Response::error(
                            id,
                            "PROJECT_NOT_FOUND",
                            &format!("Project '{}' not found", project_slug),
                        ),
                        false,
                    );
                }
            };

            let a_tag = project.a_tag();

            // Get full project status (kind:24010)
            let status = store.get_project_status(&a_tag);

            let response = match status {
                Some(status) => {
                    // Build detailed agents array with models and tools
                    let agents: Vec<_> = status
                        .agents
                        .iter()
                        .map(|a| {
                            serde_json::json!({
                                "name": a.name,
                                "pubkey": a.pubkey,
                                "is_pm": a.is_pm,
                                "model": a.model,
                                "tools": a.tools,
                            })
                        })
                        .collect();

                    serde_json::json!({
                        "slug": project.id,
                        "name": project.name,
                        "pubkey": project.pubkey,
                        "booted": status.is_online(),
                        "agents": agents,
                        "branches": status.branches,
                        "all_models": status.all_models,
                        "all_tools": status.all_tools(),
                        "backend_pubkey": status.backend_pubkey,
                        "created_at": status.created_at,
                    })
                }
                None => {
                    // Project exists but no status event (not booted)
                    serde_json::json!({
                        "slug": project.id,
                        "name": project.name,
                        "pubkey": project.pubkey,
                        "booted": false,
                        "agents": [],
                        "branches": [],
                        "all_models": [],
                        "all_tools": [],
                        "backend_pubkey": null,
                        "created_at": null,
                    })
                }
            };

            (Response::success(id, response), false)
        }

        "save_project" => {
            #[derive(Deserialize)]
            struct SaveProjectParams {
                slug: Option<String>,
                name: String,
                #[serde(default)]
                description: String,
                #[serde(default)]
                agent_ids: Vec<String>,
                #[serde(default)]
                mcp_tool_ids: Vec<String>,
                /// Client identifier for the client tag (e.g., "tenex-cli", "tenex-tui")
                #[serde(default)]
                client: Option<String>,
            }

            let params: SaveProjectParams = match serde_json::from_value(request.params.clone()) {
                Ok(p) => p,
                Err(_) => {
                    return (
                        Response::error(id, "INVALID_PARAMS", "Invalid save_project params"),
                        false,
                    );
                }
            };

            let name = params.name.trim();
            if name.is_empty() {
                return (
                    Response::error(
                        id,
                        "INVALID_PARAMS",
                        "name is required and cannot be empty or whitespace-only",
                    ),
                    false,
                );
            }

            // Validate and normalize slug (or generate from name)
            let (final_slug, slug_was_generated) = if let Some(ref user_slug) = params.slug {
                // User provided a slug - validate and normalize it
                match validate_slug(user_slug) {
                    SlugValidation::Valid(normalized) => (normalized, false),
                    SlugValidation::Empty => {
                        return (
                            Response::error(
                                id,
                                "INVALID_SLUG",
                                "Slug cannot be empty or whitespace-only",
                            ),
                            false,
                        );
                    }
                    SlugValidation::OnlyDashes => {
                        return (
                            Response::error(
                                id,
                                "INVALID_SLUG",
                                "Slug must contain at least one alphanumeric character",
                            ),
                            false,
                        );
                    }
                }
            } else {
                // No slug provided - generate from name
                match validate_slug(name) {
                    SlugValidation::Valid(normalized) => (normalized, true),
                    SlugValidation::Empty | SlugValidation::OnlyDashes => {
                        return (
                            Response::error(
                                id,
                                "INVALID_NAME",
                                "Cannot generate slug from name - name must contain at least one alphanumeric character",
                            ),
                            false,
                        );
                    }
                }
            };

            // Use provided client identifier or default based on context
            let client_tag = params.client.unwrap_or_else(|| "tenex-cli".to_string());

            if core_handle
                .send(NostrCommand::SaveProject {
                    slug: Some(final_slug.clone()),
                    name: name.to_string(),
                    description: params.description,
                    agent_ids: params.agent_ids,
                    mcp_tool_ids: params.mcp_tool_ids,
                    client: Some(client_tag),
                })
                .is_ok()
            {
                let mut response = serde_json::json!({"status": "saved", "slug": final_slug});
                if slug_was_generated {
                    response["slug_generated"] = serde_json::json!(true);
                }
                (Response::success(id, response), false)
            } else {
                (
                    Response::error(id, "SAVE_FAILED", "Failed to save project"),
                    false,
                )
            }
        }

        "set_agent_settings" => {
            #[derive(Deserialize)]
            struct SetAgentSettingsParams {
                project_slug: String,
                agent_slug: String,
                model: String,
                #[serde(default)]
                tools: Vec<String>,
                #[serde(default)]
                wait_for_project: bool,
                #[serde(default)]
                wait: bool,
            }

            let params: SetAgentSettingsParams =
                match serde_json::from_value(request.params.clone()) {
                    Ok(p) => p,
                    Err(_) => {
                        return (
                            Response::error(
                                id,
                                "INVALID_PARAMS",
                                "Invalid set_agent_settings params",
                            ),
                            false,
                        );
                    }
                };

            // If wait_for_project is true, wait for the 24010 event first
            if params.wait_for_project {
                if let Err(err_response) =
                    wait_for_project_status(data_store, &params.project_slug, id)
                {
                    return err_response;
                }
            }

            let store = data_store.lock().unwrap();
            let lookup = find_agent_in_project(&store, &params.project_slug, &params.agent_slug);

            // Get the current status timestamp for wait comparison
            let current_timestamp = store
                .get_project_status(
                    &find_project_a_tag_by_slug(&store, &params.project_slug).unwrap_or_default(),
                )
                .map(|s| s.created_at)
                .unwrap_or(0);
            drop(store);

            match lookup {
                Ok(result) => {
                    match core_handle.send(NostrCommand::UpdateAgentConfig {
                        project_a_tag: result.project_a_tag.clone(),
                        agent_pubkey: result.agent_pubkey.clone(),
                        model: Some(params.model.clone()),
                        tools: params.tools.clone(),
                        tags: Vec::new(),
                    }) {
                        Ok(_) => {
                            if params.wait {
                                // Wait for a new 24010 event with updated timestamp
                                let start = std::time::Instant::now();
                                let timeout = std::time::Duration::from_secs(30);

                                loop {
                                    if start.elapsed() > timeout {
                                        return (
                                            Response::success(
                                                id,
                                                serde_json::json!({
                                                    "status": "sent",
                                                    "warning": "Timeout waiting for confirmation - settings may still be applied"
                                                }),
                                            ),
                                            false,
                                        );
                                    }

                                    std::thread::sleep(std::time::Duration::from_millis(500));

                                    let store = data_store.lock().unwrap();
                                    if let Some(status) =
                                        store.get_project_status(&result.project_a_tag)
                                    {
                                        if status.created_at > current_timestamp {
                                            // New status received - check if settings were applied
                                            let agent_updated = status.agents.iter().any(|a| {
                                                a.pubkey == result.agent_pubkey
                                                    && a.model.as_deref() == Some(&params.model)
                                            });

                                            return (
                                                Response::success(
                                                    id,
                                                    serde_json::json!({
                                                        "status": "confirmed",
                                                        "agent_updated": agent_updated,
                                                        "new_timestamp": status.created_at
                                                    }),
                                                ),
                                                false,
                                            );
                                        }
                                    }
                                }
                            } else {
                                (
                                    Response::success(
                                        id,
                                        serde_json::json!({
                                            "status": "sent",
                                            "message": "Agent settings update sent. Use --wait to confirm application."
                                        }),
                                    ),
                                    false,
                                )
                            }
                        }
                        Err(e) => (
                            Response::error(
                                id,
                                "SEND_FAILED",
                                &format!("Failed to send agent settings update: {}", e),
                            ),
                            false,
                        ),
                    }
                }
                Err(AgentLookupError::ProjectNotFound) => (
                    Response::error(
                        id,
                        "PROJECT_NOT_FOUND",
                        &format!("Project '{}' not found", params.project_slug),
                    ),
                    false,
                ),
                Err(AgentLookupError::AgentNotFound) => (
                    Response::error(
                        id,
                        "AGENT_NOT_FOUND",
                        &format!(
                            "Agent with slug '{}' not found in project '{}'",
                            params.agent_slug, params.project_slug
                        ),
                    ),
                    false,
                ),
            }
        }

        _ => (
            Response::error(
                id,
                "UNKNOWN_METHOD",
                &format!("Unknown method: {}", request.method),
            ),
            false,
        ),
    }
}

/// Serialize a ProjectAgent to JSON for CLI output
fn agent_to_json(a: &tenex_core::models::ProjectAgent) -> serde_json::Value {
    serde_json::json!({
        "name": a.name,
        "pubkey": a.pubkey,
        "is_pm": a.is_pm,
        "model": a.model,
    })
}

/// Resolve an author name from pubkey:
/// 1. Check if pubkey belongs to an online agent -> return agent name
/// 2. Otherwise check profile name from kind:0
/// 3. Return None if no real name found (don't return truncated pubkey)
fn resolve_author_name(store: &AppDataStore, pubkey: &str) -> Option<String> {
    // Check all online agents across all projects
    for project in store.get_projects() {
        if let Some(agents) = store.get_online_agents(&project.a_tag()) {
            for agent in agents {
                if agent.pubkey == pubkey {
                    return Some(agent.name.clone());
                }
            }
        }
    }

    // Fall back to profile name - but only if it's a real name (not truncated pubkey)
    let profile_name = store.get_profile_name(pubkey);
    if profile_name.len() > 16
        || !profile_name
            .chars()
            .all(|c| c.is_ascii_hexdigit() || c == '.')
    {
        Some(profile_name)
    } else {
        None
    }
}

/// Default timeout for waiting for project status (30 seconds)
const WAIT_FOR_PROJECT_TIMEOUT_SECS: u64 = 30;

/// Wait for a project's 24010 status event to appear.
/// Returns Ok(a_tag) if found within timeout, or an error response if not found.
fn wait_for_project_status(
    data_store: &Arc<Mutex<AppDataStore>>,
    project_slug: &str,
    id: u64,
) -> Result<String, (Response, bool)> {
    let start = std::time::Instant::now();
    let timeout = std::time::Duration::from_secs(WAIT_FOR_PROJECT_TIMEOUT_SECS);

    loop {
        if start.elapsed() > timeout {
            return Err((
                Response::error(
                    id,
                    "TIMEOUT",
                    &format!(
                        "Timeout waiting for project status (24010 event) for '{}'. Project may not be booted.",
                        project_slug
                    ),
                ),
                false,
            ));
        }

        let store = data_store.lock().unwrap();
        if let Some(a_tag) = find_project_a_tag_by_slug(&store, project_slug) {
            if store.get_project_status(&a_tag).is_some() {
                return Ok(a_tag);
            }
        }
        drop(store);

        std::thread::sleep(std::time::Duration::from_millis(500));
    }
}

/// Error type for agent lookup failures
enum AgentLookupError {
    ProjectNotFound,
    AgentNotFound,
}

/// Find a project's a_tag by its slug (d-tag).
/// Returns None if no project with that slug is found.
fn find_project_a_tag_by_slug(store: &AppDataStore, slug: &str) -> Option<String> {
    store
        .get_projects()
        .iter()
        .find(|p| p.id == slug)
        .map(|p| p.a_tag())
}

/// Result of looking up an agent by slug within a project
struct AgentLookupResult {
    project_a_tag: String,
    agent_pubkey: String,
}

/// Find an agent's pubkey by their name within a specific project (identified by slug).
/// Uses the online agents from ProjectStatus (kind:24010).
/// Also returns the project's a_tag to avoid a second lookup.
fn find_agent_in_project(
    store: &AppDataStore,
    project_slug: &str,
    agent_name: &str,
) -> Result<AgentLookupResult, AgentLookupError> {
    // Find the project by slug to get its a_tag
    let project_a_tag =
        find_project_a_tag_by_slug(store, project_slug).ok_or(AgentLookupError::ProjectNotFound)?;

    // Look through the online agents from ProjectStatus
    let agents = store
        .get_online_agents(&project_a_tag)
        .ok_or(AgentLookupError::AgentNotFound)?;

    for agent in agents {
        if agent.name == agent_name {
            return Ok(AgentLookupResult {
                project_a_tag,
                agent_pubkey: agent.pubkey.clone(),
            });
        }
    }

    Err(AgentLookupError::AgentNotFound)
}

/// Validates skill IDs from JSON-RPC parameters.
///
/// Performs comprehensive validation:
/// 1. Checks that `skill_ids` is null (returns empty vec) or an array
/// 2. Ensures all elements are strings (returns error if any non-string)
/// 3. Trims whitespace, filters empty strings, and deduplicates
/// 4. Validates 64-character hex format
///
/// Returns Ok(Vec<String>) with validated IDs, or Err(String) with error message.
fn validate_skill_ids_param(skill_ids_param: &serde_json::Value) -> Result<Vec<String>, String> {
    use std::collections::HashSet;

    if skill_ids_param.is_null() {
        return Ok(Vec::new());
    }

    let arr = skill_ids_param
        .as_array()
        .ok_or("skill_ids must be an array of strings")?;

    // First pass: ensure all elements are strings
    for (i, v) in arr.iter().enumerate() {
        if !v.is_string() {
            return Err(format!(
                "skill_ids[{}] must be a string, got {}",
                i,
                match v {
                    serde_json::Value::Null => "null",
                    serde_json::Value::Bool(_) => "boolean",
                    serde_json::Value::Number(_) => "number",
                    serde_json::Value::Array(_) => "array",
                    serde_json::Value::Object(_) => "object",
                    serde_json::Value::String(_) => unreachable!(),
                }
            ));
        }
    }

    // Second pass: validate and collect
    let mut seen = HashSet::new();
    let mut validated = Vec::new();

    for v in arr {
        let s = v.as_str().unwrap(); // Safe: verified above
        let trimmed = s.trim();

        // Skip empty/whitespace-only IDs
        if trimmed.is_empty() {
            continue;
        }

        // Skip duplicates
        if seen.contains(trimmed) {
            continue;
        }

        // Validate 64-character hex format
        if trimmed.len() != 64 {
            return Err(format!(
                "skill_ids must be 64-character hex strings (got {} characters): {}",
                trimmed.len(),
                trimmed
            ));
        }
        if !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "skill_ids must contain only hex characters: {}",
                trimmed
            ));
        }

        seen.insert(trimmed.to_string());
        validated.push(trimmed.to_string());
    }

    Ok(validated)
}

/// Validates nudge IDs from JSON-RPC parameters.
///
/// Performs comprehensive validation:
/// 1. Checks that `nudge_ids` is null (returns empty vec) or an array
/// 2. Ensures all elements are strings (returns error if any non-string)
/// 3. Trims whitespace, filters empty strings, and deduplicates
/// 4. Validates 64-character hex format
///
/// Returns Ok(Vec<String>) with validated IDs, or Err(String) with error message.
fn validate_nudge_ids_param(nudge_ids_param: &serde_json::Value) -> Result<Vec<String>, String> {
    use std::collections::HashSet;

    if nudge_ids_param.is_null() {
        return Ok(Vec::new());
    }

    let arr = nudge_ids_param
        .as_array()
        .ok_or("nudge_ids must be an array of strings")?;

    // First pass: ensure all elements are strings
    for (i, v) in arr.iter().enumerate() {
        if !v.is_string() {
            return Err(format!(
                "nudge_ids[{}] must be a string, got {}",
                i,
                match v {
                    serde_json::Value::Null => "null",
                    serde_json::Value::Bool(_) => "boolean",
                    serde_json::Value::Number(_) => "number",
                    serde_json::Value::Array(_) => "array",
                    serde_json::Value::Object(_) => "object",
                    serde_json::Value::String(_) => unreachable!(),
                }
            ));
        }
    }

    // Second pass: validate and collect
    let mut seen = HashSet::new();
    let mut validated = Vec::new();

    for v in arr {
        let s = v.as_str().unwrap(); // Safe: verified above
        let trimmed = s.trim();

        // Skip empty/whitespace-only IDs
        if trimmed.is_empty() {
            continue;
        }

        // Skip duplicates
        if seen.contains(trimmed) {
            continue;
        }

        // Validate 64-character hex format
        if trimmed.len() != 64 {
            return Err(format!(
                "nudge_ids must be 64-character hex strings (got {} characters): {}",
                trimmed.len(),
                trimmed
            ));
        }
        if !trimmed.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!(
                "nudge_ids must contain only hex characters: {}",
                trimmed
            ));
        }

        seen.insert(trimmed.to_string());
        validated.push(trimmed.to_string());
    }

    Ok(validated)
}
