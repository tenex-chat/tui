use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use tokio::net::UnixListener;

use anyhow::Result;
use serde::Deserialize;

use crate::nostr::{self, NostrCommand};
use crate::store::AppDataStore;
use tenex_core::config::CoreConfig;
use tenex_core::models::PreferencesStorage;
use tenex_core::runtime::{CoreHandle, CoreRuntime};

use super::config::CliConfig;
use super::protocol::{Request, Response};

const SOCKET_NAME: &str = "tenex-cli.sock";
const PID_FILE: &str = "daemon.pid";

/// Get socket path, using config override if provided
pub fn get_socket_path(config: Option<&CliConfig>) -> PathBuf {
    if let Some(cfg) = config {
        if let Some(ref path) = cfg.socket_path {
            return path.clone();
        }
    }
    default_socket_path()
}

/// Default socket path when no config is provided
pub fn default_socket_path() -> PathBuf {
    let base_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tenex-cli");
    base_dir.join(SOCKET_NAME)
}

fn get_pid_path(config: Option<&CliConfig>) -> PathBuf {
    // Put PID file next to socket
    let socket_path = get_socket_path(config);
    if let Some(parent) = socket_path.parent() {
        parent.join(PID_FILE)
    } else {
        PathBuf::from(PID_FILE)
    }
}

fn get_data_dir() -> PathBuf {
    CoreConfig::default_data_dir()
}

/// Run the daemon server
#[tokio::main]
pub async fn run_daemon(config: Option<CliConfig>) -> Result<()> {
    eprintln!("Starting tenex-cli daemon...");

    // Ensure base directory exists
    let socket_path = get_socket_path(config.as_ref());
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Remove stale socket if exists
    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }

    // Write PID file
    let pid_path = get_pid_path(config.as_ref());
    fs::write(&pid_path, std::process::id().to_string())?;

    // Bind socket early so clients can connect while we initialize
    let listener = UnixListener::bind(&socket_path)?;
    eprintln!("Listening on {:?}", socket_path);

    // Initialize core runtime
    let data_dir = get_data_dir();
    let mut core_runtime = CoreRuntime::new(CoreConfig::new(&data_dir))?;
    let data_store = core_runtime.data_store();
    let core_handle = core_runtime.handle();

    // Initialize preferences for credential storage
    let prefs = PreferencesStorage::new(data_dir.to_str().unwrap_or("tenex_data"));

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
        }
    }

    // Cleanup
    core_runtime.shutdown();
    fs::remove_file(&socket_path).ok();
    fs::remove_file(&pid_path).ok();

    eprintln!("Daemon stopped");
    Ok(())
}

/// Try to login with config credentials first, then fall back to stored credentials
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
                    serde_json::json!({
                        "id": p.a_tag(),
                        "name": p.name,
                        "pubkey": p.pubkey,
                        "participants": p.participants,
                    })
                })
                .collect();
            (Response::success(id, serde_json::json!(projects)), false)
        }

        "list_threads" => {
            let project_id = request.params["project_id"].as_str().unwrap_or("");
            let store = data_store.borrow();
            let threads: Vec<_> = store
                .get_threads(project_id)
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

        "list_messages" => {
            let thread_id = request.params["thread_id"].as_str().unwrap_or("");
            let store = data_store.borrow();
            let messages: Vec<_> = store
                .get_messages(thread_id)
                .iter()
                .map(|m| {
                    serde_json::json!({
                        "id": m.id,
                        "content": m.content,
                        "created_at": m.created_at,
                        "pubkey": m.pubkey,
                        "author_name": store.get_profile_name(&m.pubkey),
                    })
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
            let thread_id = request.params["thread_id"].as_str().unwrap_or("");
            let content = request.params["content"].as_str().unwrap_or("");

            if thread_id.is_empty() || content.is_empty() {
                return (
                    Response::error(id, "INVALID_PARAMS", "thread_id and content are required"),
                    false,
                );
            }

            // Find the project for this thread
            let store = data_store.borrow();
            let mut project_a_tag = None;
            for project in store.get_projects() {
                for thread in store.get_threads(&project.a_tag()) {
                    if thread.id == thread_id {
                        project_a_tag = Some(project.a_tag());
                        break;
                    }
                }
                if project_a_tag.is_some() {
                    break;
                }
            }
            drop(store);

            match project_a_tag {
                Some(project_a_tag) => {
                    if core_handle
                        .send(NostrCommand::PublishMessage {
                            thread_id: thread_id.to_string(),
                            project_a_tag,
                            content: content.to_string(),
                            agent_pubkey: None,
                            reply_to: Some(thread_id.to_string()),
                            branch: None,
                            nudge_ids: vec![],
                            ask_author_pubkey: None,
                        })
                        .is_ok()
                    {
                        (
                            Response::success(id, serde_json::json!({"status": "sent"})),
                            false,
                        )
                    } else {
                        (
                            Response::error(id, "SEND_FAILED", "Failed to send message"),
                            false,
                        )
                    }
                }
                None => (
                    Response::error(id, "NOT_FOUND", "Thread not found"),
                    false,
                ),
            }
        }

        "create_thread" => {
            let project_id = request.params["project_id"].as_str().unwrap_or("");
            let title = request.params["title"].as_str().unwrap_or("");

            if project_id.is_empty() || title.is_empty() {
                return (
                    Response::error(id, "INVALID_PARAMS", "project_id and title are required"),
                    false,
                );
            }

            if core_handle
                .send(NostrCommand::PublishThread {
                    project_a_tag: project_id.to_string(),
                    title: title.to_string(),
                    content: title.to_string(),
                    agent_pubkey: None,
                    branch: None,
                    nudge_ids: vec![],
                })
                .is_ok()
            {
                (
                    Response::success(id, serde_json::json!({"status": "created"})),
                    false,
                )
            } else {
                (
                    Response::error(id, "CREATE_FAILED", "Failed to create thread"),
                    false,
                )
            }
        }

        "boot_project" => {
            let project_id = request.params["project_id"].as_str().unwrap_or("");

            if project_id.is_empty() {
                return (
                    Response::error(id, "INVALID_PARAMS", "project_id is required"),
                    false,
                );
            }

            // Find the project to get its pubkey
            let store = data_store.borrow();
            let project_pubkey = store
                .get_projects()
                .iter()
                .find(|p| p.a_tag() == project_id)
                .map(|p| p.pubkey.clone());
            drop(store);

            if core_handle
                .send(NostrCommand::BootProject {
                    project_a_tag: project_id.to_string(),
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

        "create_project" => {
            #[derive(Deserialize)]
            struct CreateProjectParams {
                name: String,
                #[serde(default)]
                description: String,
                #[serde(default)]
                agent_ids: Vec<String>,
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
