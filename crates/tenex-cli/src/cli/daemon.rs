use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Instant;

use tokio::net::UnixListener;

use anyhow::Result;
use serde::Deserialize;

use crate::nostr::{self, DataChange, NostrCommand};
use crate::store::AppDataStore;
use tenex_core::config::CoreConfig;
use tenex_core::models::PreferencesStorage;
use tenex_core::nostr::set_log_path;
use tenex_core::runtime::{CoreHandle, CoreRuntime};

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
pub async fn run_daemon(data_dir: PathBuf, config: Option<CliConfig>) -> Result<()> {
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
    let data_store = core_runtime.data_store();
    let core_handle = core_runtime.handle();
    let data_rx = core_runtime.take_data_rx().expect("data_rx should be available");

    // Initialize preferences for credential storage and trusted backends
    let prefs = PreferencesStorage::new(data_dir.to_str().unwrap_or("tenex_data"));

    // Set trusted backends from preferences
    {
        let approved = prefs.approved_backend_pubkeys().clone();
        let blocked = prefs.blocked_backend_pubkeys().clone();
        data_store.borrow_mut().set_trusted_backends(approved, blocked);
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

    // Handle connections - use async accept to allow batch exporter to run
    loop {
        // Drain any pending DataChange events (non-blocking)
        while let Ok(data_change) = data_rx.try_recv() {
            match data_change {
                DataChange::ProjectStatus { json } => {
                    data_store.borrow_mut().handle_status_event_json(&json);
                }
                _ => {}
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
                            &data_store,
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
                if let Err(e) = core_runtime.process_note_keys(&note_keys) {
                    eprintln!("Failed to process core events: {}", e);
                }
            }
            // Small timeout to periodically check for DataChange events
            _ = tokio::time::sleep(std::time::Duration::from_millis(100)) => {}
        }
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

    // Try environment variables (used when daemon is spawned by client)
    if let Ok(key) = std::env::var("TENEX_KEY") {
        let password = std::env::var("TENEX_KEY_PASSWORD").ok();
        match try_login_with_credentials(&key, password.as_deref(), core_handle) {
            Ok(keys) => return Some(keys),
            Err(e) => {
                eprintln!("Failed to login with TENEX_KEY: {}", e);
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
        let secret_key = encrypted.to_secret_key(password)?;
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
    data_store: &Rc<RefCell<AppDataStore>>,
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
    data_store: &Rc<RefCell<AppDataStore>>,
    core_handle: &CoreHandle,
    start_time: Instant,
    logged_in: bool,
) -> (Response, bool) {
    let id = request.id;

    match request.method.as_str() {
        "list_projects" => {
            let store = data_store.borrow();
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
                        obj["participants"] = serde_json::json!(
                            agents.iter().map(agent_to_json).collect::<Vec<_>>()
                        );
                    }
                    obj
                })
                .collect();
            (Response::success(id, serde_json::json!(projects)), false)
        }

        "list_threads" => {
            let project_slug = request.params["project_slug"].as_str().unwrap_or("");
            let wait_for_project = request.params["wait_for_project"].as_bool().unwrap_or(false);

            // If wait_for_project is true, wait for the 24010 event first
            if wait_for_project {
                if let Err(err_response) = wait_for_project_status(data_store, project_slug, id) {
                    return err_response;
                }
            }

            let store = data_store.borrow();

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
            let wait_for_project = request.params["wait_for_project"].as_bool().unwrap_or(false);

            // If wait_for_project is true, wait for the 24010 event first
            if wait_for_project {
                if let Err(err_response) = wait_for_project_status(data_store, project_slug, id) {
                    return err_response;
                }
            }

            let store = data_store.borrow();

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
            let store = data_store.borrow();
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
            let store = data_store.borrow();
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
                        "uptime_seconds": start_time.elapsed().as_secs(),
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
            let wait_for_project = request.params["wait_for_project"].as_bool().unwrap_or(false);

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

            let store = data_store.borrow();
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
                        branch: None,
                        nudge_ids: vec![],
                        ask_author_pubkey: None,
                        response_tx: Some(response_tx),
                    }) {
                        Ok(_) => {
                            // Wait for the event ID (with timeout)
                            match response_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                                Ok(message_id) => (
                                    Response::success(id, serde_json::json!({
                                        "status": "sent",
                                        "message_id": message_id
                                    })),
                                    false,
                                ),
                                Err(_) => (
                                    Response::success(id, serde_json::json!({
                                        "status": "sent",
                                        "message_id": null
                                    })),
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
            let wait_for_project = request.params["wait_for_project"].as_bool().unwrap_or(false);

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

            let store = data_store.borrow();
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
                        branch: None,
                        nudge_ids: vec![],
                        reference_conversation_id: None,
                        fork_message_id: None,
                        response_tx: Some(response_tx),
                    }) {
                        Ok(_) => {
                            // Wait for the event ID (with timeout)
                            match response_rx.recv_timeout(std::time::Duration::from_secs(5)) {
                                Ok(thread_id) => (
                                    Response::success(id, serde_json::json!({
                                        "status": "created",
                                        "thread_id": thread_id
                                    })),
                                    false,
                                ),
                                Err(_) => (
                                    Response::success(id, serde_json::json!({
                                        "status": "created",
                                        "thread_id": null
                                    })),
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
            let store = data_store.borrow();
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

        "status" => {
            (
                Response::success(
                    id,
                    serde_json::json!({
                        "status": "running",
                        "logged_in": logged_in,
                        "uptime_seconds": start_time.elapsed().as_secs(),
                    }),
                ),
                false,
            )
        }

        "shutdown" => (
            Response::success(id, serde_json::json!({"status": "shutting_down"})),
            true,
        ),

        "list_agent_definitions" => {
            let store = data_store.borrow();
            let agent_defs: Vec<_> = store
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

        "show_project" => {
            let project_slug = request.params["project_slug"].as_str().unwrap_or("");
            let wait_for_project = request.params["wait_for_project"].as_bool().unwrap_or(false);

            // If wait_for_project is true, wait for the 24010 event first
            if wait_for_project {
                if let Err(err_response) = wait_for_project_status(data_store, project_slug, id) {
                    return err_response;
                }
            }

            let store = data_store.borrow();

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

        "create_project" => {
            #[derive(Deserialize)]
            struct CreateProjectParams {
                name: String,
                #[serde(default)]
                description: String,
                #[serde(default)]
                agent_ids: Vec<String>,
                #[serde(default)]
                mcp_tool_ids: Vec<String>,
            }

            let params: CreateProjectParams = match serde_json::from_value(request.params.clone()) {
                Ok(p) => p,
                Err(_) => {
                    return (
                        Response::error(id, "INVALID_PARAMS", "Invalid create_project params"),
                        false,
                    );
                }
            };

            let name = params.name.trim();
            if name.is_empty() {
                return (
                    Response::error(id, "INVALID_PARAMS", "name is required and cannot be empty or whitespace-only"),
                    false,
                );
            }

            if core_handle
                .send(NostrCommand::CreateProject {
                    name: name.to_string(),
                    description: params.description,
                    agent_ids: params.agent_ids,
                    mcp_tool_ids: params.mcp_tool_ids,
                })
                .is_ok()
            {
                (
                    Response::success(id, serde_json::json!({"status": "created"})),
                    false,
                )
            } else {
                (
                    Response::error(id, "CREATE_FAILED", "Failed to create project"),
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

            let params: SetAgentSettingsParams = match serde_json::from_value(request.params.clone()) {
                Ok(p) => p,
                Err(_) => {
                    return (
                        Response::error(id, "INVALID_PARAMS", "Invalid set_agent_settings params"),
                        false,
                    );
                }
            };

            // If wait_for_project is true, wait for the 24010 event first
            if params.wait_for_project {
                if let Err(err_response) = wait_for_project_status(data_store, &params.project_slug, id) {
                    return err_response;
                }
            }

            let store = data_store.borrow();
            let lookup = find_agent_in_project(&store, &params.project_slug, &params.agent_slug);

            // Get the current status timestamp for wait comparison
            let current_timestamp = store
                .get_project_status(&find_project_a_tag_by_slug(&store, &params.project_slug).unwrap_or_default())
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
                    }) {
                        Ok(_) => {
                            if params.wait {
                                // Wait for a new 24010 event with updated timestamp
                                let start = std::time::Instant::now();
                                let timeout = std::time::Duration::from_secs(30);

                                loop {
                                    if start.elapsed() > timeout {
                                        return (
                                            Response::success(id, serde_json::json!({
                                                "status": "sent",
                                                "warning": "Timeout waiting for confirmation - settings may still be applied"
                                            })),
                                            false,
                                        );
                                    }

                                    std::thread::sleep(std::time::Duration::from_millis(500));

                                    let store = data_store.borrow();
                                    if let Some(status) = store.get_project_status(&result.project_a_tag) {
                                        if status.created_at > current_timestamp {
                                            // New status received - check if settings were applied
                                            let agent_updated = status.agents.iter().any(|a| {
                                                a.pubkey == result.agent_pubkey &&
                                                a.model.as_deref() == Some(&params.model)
                                            });

                                            return (
                                                Response::success(id, serde_json::json!({
                                                    "status": "confirmed",
                                                    "agent_updated": agent_updated,
                                                    "new_timestamp": status.created_at
                                                })),
                                                false,
                                            );
                                        }
                                    }
                                }
                            } else {
                                (
                                    Response::success(id, serde_json::json!({
                                        "status": "sent",
                                        "message": "Agent settings update sent. Use --wait to confirm application."
                                    })),
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
    if profile_name.len() > 16 || !profile_name.chars().all(|c| c.is_ascii_hexdigit() || c == '.') {
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
    data_store: &Rc<RefCell<AppDataStore>>,
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

        let store = data_store.borrow();
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
    let project_a_tag = find_project_a_tag_by_slug(store, project_slug)
        .ok_or(AgentLookupError::ProjectNotFound)?;

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
