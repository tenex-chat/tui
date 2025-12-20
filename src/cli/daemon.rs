use std::cell::RefCell;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::Instant;

use anyhow::Result;
use nostrdb::Ndb;

use crate::nostr::{self, DataChange, NostrCommand, NostrWorker};
use crate::store::{AppDataStore, Database};

use super::protocol::{Request, Response};

const SOCKET_NAME: &str = "tenex-cli.sock";
const PID_FILE: &str = "daemon.pid";

pub fn get_socket_path() -> PathBuf {
    let base_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tenex-cli");
    base_dir.join(SOCKET_NAME)
}

fn get_pid_path() -> PathBuf {
    let base_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".tenex-cli");
    base_dir.join(PID_FILE)
}

fn get_data_dir() -> PathBuf {
    PathBuf::from("tenex_data")
}

/// Run the daemon server
pub fn run_daemon() -> Result<()> {
    eprintln!("Starting tenex-cli daemon...");

    // Ensure base directory exists
    let socket_path = get_socket_path();
    if let Some(parent) = socket_path.parent() {
        fs::create_dir_all(parent)?;
    }

    // Remove stale socket if exists
    if socket_path.exists() {
        fs::remove_file(&socket_path)?;
    }

    // Write PID file
    let pid_path = get_pid_path();
    fs::write(&pid_path, std::process::id().to_string())?;

    // Bind socket early so clients can connect while we initialize
    let listener = UnixListener::bind(&socket_path)?;
    eprintln!("Listening on {:?}", socket_path);

    // Initialize nostrdb
    let data_dir = get_data_dir();
    fs::create_dir_all(&data_dir)?;
    let ndb = Arc::new(Ndb::new(
        data_dir.to_str().unwrap_or("tenex_data"),
        &nostrdb::Config::new(),
    )?);

    // Create app data store
    let data_store = Rc::new(RefCell::new(AppDataStore::new(ndb.clone())));

    // Create database for credentials
    let db = Database::with_ndb(ndb.clone(), &data_dir)?;

    // Set up channels for NostrWorker
    let (command_tx, command_rx) = mpsc::channel::<NostrCommand>();
    let (data_tx, data_rx) = mpsc::channel::<DataChange>();

    // Start NostrWorker thread
    let worker = NostrWorker::new(ndb.clone(), data_tx, command_rx);
    let worker_handle = std::thread::spawn(move || {
        worker.run();
    });

    // Try to auto-login if credentials are available
    let keys = try_auto_login(&db, &command_tx);
    if keys.is_some() {
        eprintln!("Auto-login successful");
    } else {
        eprintln!("No stored credentials or password required - daemon running without login");
    }

    // Track state
    let start_time = Instant::now();

    // Handle connections
    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                let should_shutdown = handle_connection(
                    stream,
                    &data_store,
                    &command_tx,
                    &data_rx,
                    &db,
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

    // Cleanup
    command_tx.send(NostrCommand::Shutdown).ok();
    worker_handle.join().ok();
    fs::remove_file(&socket_path).ok();
    fs::remove_file(&pid_path).ok();

    eprintln!("Daemon stopped");
    Ok(())
}

fn try_auto_login(
    db: &Database,
    command_tx: &mpsc::Sender<NostrCommand>,
) -> Option<nostr_sdk::Keys> {
    let conn = db.credentials_conn();

    if !nostr::has_stored_credentials(&conn) {
        return None;
    }

    // Check if password required
    if nostr::credentials_need_password(&conn) {
        // Try to get password from environment
        if let Ok(password) = std::env::var("TENEX_PASSWORD") {
            match nostr::load_stored_keys(&password, &conn) {
                Ok(keys) => {
                    let pubkey = nostr::get_current_pubkey(&keys);
                    if command_tx
                        .send(NostrCommand::Connect {
                            keys: keys.clone(),
                            user_pubkey: pubkey,
                        })
                        .is_ok()
                    {
                        command_tx.send(NostrCommand::Sync).ok();
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
    match nostr::load_unencrypted_keys(&conn) {
        Ok(keys) => {
            let pubkey = nostr::get_current_pubkey(&keys);
            if command_tx
                .send(NostrCommand::Connect {
                    keys: keys.clone(),
                    user_pubkey: pubkey,
                })
                .is_ok()
            {
                command_tx.send(NostrCommand::Sync).ok();
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
    command_tx: &mpsc::Sender<NostrCommand>,
    data_rx: &mpsc::Receiver<DataChange>,
    db: &Database,
    start_time: Instant,
    logged_in: bool,
) -> Result<bool> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut writer = stream;
    let mut line = String::new();

    // Process any pending data updates
    while let Ok(DataChange::StreamingDelta {
        pubkey,
        message_id,
        thread_id,
        sequence,
        created_at,
        delta,
    }) = data_rx.try_recv()
    {
        data_store.borrow_mut().handle_streaming_delta(
            pubkey, message_id, thread_id, sequence, created_at, delta,
        );
    }

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
            handle_request(&request, data_store, command_tx, db, start_time, logged_in);

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
    command_tx: &mpsc::Sender<NostrCommand>,
    _db: &Database,
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
                    if command_tx
                        .send(NostrCommand::PublishMessage {
                            thread_id: thread_id.to_string(),
                            project_a_tag,
                            content: content.to_string(),
                            agent_pubkey: None,
                            reply_to: Some(thread_id.to_string()),
                            branch: None,
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

            if command_tx
                .send(NostrCommand::PublishThread {
                    project_a_tag: project_id.to_string(),
                    title: title.to_string(),
                    content: title.to_string(),
                    agent_pubkey: None,
                    branch: None,
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

        "sync" => {
            if command_tx.send(NostrCommand::Sync).is_ok() {
                (
                    Response::success(id, serde_json::json!({"status": "syncing"})),
                    false,
                )
            } else {
                (Response::error(id, "SYNC_FAILED", "Failed to sync"), false)
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
