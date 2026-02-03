use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use anyhow::Result;
use nostr_sdk::prelude::*;
use nostrdb::{Ndb, Transaction};
use tokio::runtime::Runtime;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::watch;
use tokio::sync::RwLock;

use crate::constants::RELAY_URL;
use crate::models::ProjectStatus;
use crate::stats::{SharedEventStats, SharedNegentropySyncStats, SharedSubscriptionStats, SubscriptionInfo};
use crate::store::ingest_events;
use crate::streaming::{LocalStreamChunk, SocketStreamClient};

// Event kind constants
const KIND_TEXT_NOTE: u16 = 1;
const KIND_LONG_FORM_CONTENT: u16 = 30023;
const KIND_PROJECT_METADATA: u16 = 513;
const KIND_AGENT: u16 = 4199;
const KIND_MCP_TOOL: u16 = 4200;
const KIND_NUDGE: u16 = 4201;
const KIND_PROJECT_STATUS: u16 = 24010;
const KIND_PROJECT_DRAFT: u16 = 31933;
const KIND_AGENT_STATUS: u16 = 24133;

static START_TIME: OnceLock<Instant> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

/// Set the log file path. Must be called before any logging occurs.
pub fn set_log_path(path: PathBuf) {
    let _ = LOG_PATH.set(path);
}

fn get_log_path() -> PathBuf {
    LOG_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| PathBuf::from("/tmp/tenex.log"))
}

pub fn elapsed_ms() -> u64 {
    START_TIME.get_or_init(Instant::now).elapsed().as_millis() as u64
}

pub fn log_to_file(tag: &str, msg: &str) {
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(get_log_path())
    {
        let _ = writeln!(file, "[{:>8}ms] [{}] {}", elapsed_ms(), tag, msg);
    }
}

#[macro_export]
macro_rules! tlog {
    ($tag:expr, $($arg:tt)*) => {
        $crate::nostr::worker::log_to_file($tag, &format!($($arg)*))
    };
}

fn debug_log(msg: &str) {
    if std::env::var("TENEX_DEBUG").map(|v| v == "1").unwrap_or(false) {
        tlog!("DEBUG", "{}", msg);
    }
}

/// Extract project name from a_tag coordinate (format: kind:pubkey:identifier)
fn extract_project_name(a_tag: &str) -> &str {
    a_tag.split(':').nth(2).unwrap_or("unknown")
}

/// Subscribe to a project if not already subscribed, with automatic rollback on failure
async fn subscribe_project_if_new(
    client: &Client,
    ndb: &Ndb,
    a_tag: &str,
    subscribed_projects: &Arc<RwLock<HashSet<String>>>,
    subscription_stats: &SharedSubscriptionStats,
) -> Result<bool> {
    // Atomic check+insert: if insert returns true, we're the first to subscribe to this project
    let is_new = {
        let mut projects = subscribed_projects.write().await;
        projects.insert(a_tag.to_string())
    };

    if !is_new {
        return Ok(false); // Already subscribed
    }

    // Try to subscribe
    match subscribe_project_filters(client, ndb, subscription_stats, a_tag).await {
        Ok(_) => {
            debug_log(&format!("✅ Subscribed to newly online project: {}", extract_project_name(a_tag)));
            Ok(true)
        }
        Err(e) => {
            // Rollback on failure
            subscribed_projects.write().await.remove(a_tag);
            Err(e)
        }
    }
}

/// Subscribe to all filters for a project (metadata, messages, reports).
/// Returns Ok(()) only if ALL subscriptions succeed - this is all-or-nothing.
/// Also registers subscription stats for each filter.
///
/// This function is used in multiple places:
/// 1. Initial connection (start_subscriptions) for existing projects
/// 2. Notification handler for newly discovered projects
/// 3. Command handlers for explicit project subscription requests
async fn subscribe_project_filters(
    client: &Client,
    ndb: &Ndb,
    subscription_stats: &SharedSubscriptionStats,
    project_a_tag: &str,
) -> Result<()> {
    let project_name = extract_project_name(project_a_tag);

    // Metadata subscription (kind:513)
    let mut metadata_filter = Filter::new()
        .kind(Kind::Custom(KIND_PROJECT_METADATA))
        .custom_tag(SingleLetterTag::lowercase(Alphabet::A), project_a_tag.to_string());
    if let Some(latest) = latest_kind_timestamp_for_project(ndb, KIND_PROJECT_METADATA, project_a_tag) {
        // Subtract 1s to avoid missing same-second events
        metadata_filter = metadata_filter.since(Timestamp::from(latest.saturating_sub(1)));
    }
    let metadata_filter_json = serde_json::to_string(&metadata_filter).ok();
    let metadata_output = client.subscribe(metadata_filter.clone(), None).await
        .map_err(|e| anyhow::anyhow!("Failed to subscribe to metadata for {}: {}", project_name, e))?;
    subscription_stats.register(
        metadata_output.val.to_string(),
        SubscriptionInfo::new(
            format!("{} metadata", project_name),
            vec![KIND_PROJECT_METADATA],
            Some(project_a_tag.to_string()),
        ).with_raw_filter(metadata_filter_json.unwrap_or_default()),
    );

    // Messages subscription (kind:1)
    let mut message_filter = Filter::new()
        .kind(Kind::from(KIND_TEXT_NOTE))
        .custom_tag(SingleLetterTag::lowercase(Alphabet::A), project_a_tag.to_string());
    if let Some(latest) = latest_kind_timestamp_for_project(ndb, KIND_TEXT_NOTE, project_a_tag) {
        // Subtract 1s to avoid missing same-second events
        message_filter = message_filter.since(Timestamp::from(latest.saturating_sub(1)));
    }
    let message_filter_json = serde_json::to_string(&message_filter).ok();
    let message_output = client.subscribe(message_filter.clone(), None).await
        .map_err(|e| anyhow::anyhow!("Failed to subscribe to messages for {}: {}", project_name, e))?;
    subscription_stats.register(
        message_output.val.to_string(),
        SubscriptionInfo::new(
            format!("{} messages", project_name),
            vec![KIND_TEXT_NOTE],
            Some(project_a_tag.to_string()),
        ).with_raw_filter(message_filter_json.unwrap_or_default()),
    );

    // Long-form content subscription (kind:30023)
    let longform_filter = Filter::new()
        .kind(Kind::Custom(KIND_LONG_FORM_CONTENT))
        .custom_tag(SingleLetterTag::lowercase(Alphabet::A), project_a_tag.to_string());
    let longform_filter_json = serde_json::to_string(&longform_filter).ok();
    let longform_output = client.subscribe(longform_filter.clone(), None).await
        .map_err(|e| anyhow::anyhow!("Failed to subscribe to reports for {}: {}", project_name, e))?;
    subscription_stats.register(
        longform_output.val.to_string(),
        SubscriptionInfo::new(
            format!("{} reports", project_name),
            vec![KIND_LONG_FORM_CONTENT],
            Some(project_a_tag.to_string()),
        ).with_raw_filter(longform_filter_json.unwrap_or_default()),
    );

    Ok(())
}

fn latest_kind_timestamp_for_project(ndb: &Ndb, kind: u16, project_a_tag: &str) -> Option<u64> {
    let txn = Transaction::new(ndb).ok()?;
    let filter = nostrdb::Filter::new()
        .kinds([kind as u64])
        .tags([project_a_tag], 'a')
        .build();

    let results = ndb.query(&txn, &[filter], 1_000_000).ok()?;
    let mut max_ts: Option<u64> = None;

    for r in results.iter() {
        if let Ok(note) = ndb.get_note_by_key(&txn, r.note_key) {
            let created_at = note.created_at();
            max_ts = Some(max_ts.map_or(created_at, |current| current.max(created_at)));
        }
    }

    max_ts
}

/// Response channel for commands that need to return data (like event IDs)
pub type EventIdSender = std::sync::mpsc::SyncSender<String>;

pub enum NostrCommand {
    Connect {
        keys: Keys,
        user_pubkey: String,
        response_tx: Option<Sender<Result<(), String>>>,
    },
    PublishThread {
        project_a_tag: String,
        title: String,
        content: String,
        agent_pubkey: Option<String>,
        branch: Option<String>,
        nudge_ids: Vec<String>,
        /// Optional reference to another conversation (adds "context" tag for referencing source conversations)
        reference_conversation_id: Option<String>,
        /// Optional fork message ID (used with reference_conversation_id to create a "fork" tag)
        fork_message_id: Option<String>,
        /// Optional channel to send back the event ID after signing
        response_tx: Option<EventIdSender>,
    },
    PublishMessage {
        thread_id: String,
        project_a_tag: String,
        content: String,
        agent_pubkey: Option<String>,
        reply_to: Option<String>,
        branch: Option<String>,
        nudge_ids: Vec<String>,
        /// Pubkey of the ask event author (for p-tagging when replying to ask events)
        ask_author_pubkey: Option<String>,
        /// Optional channel to send back the event ID after signing
        response_tx: Option<EventIdSender>,
    },
    #[allow(dead_code)]
    BootProject {
        project_a_tag: String,
        project_pubkey: Option<String>,
    },
    UpdateProjectAgents {
        project_a_tag: String,
        agent_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
    },
    /// Create a new project (kind:31933)
    CreateProject {
        name: String,
        description: String,
        agent_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
    },
    /// Create a new agent definition (kind:4199)
    CreateAgentDefinition {
        name: String,
        description: String,
        role: String,
        instructions: String,
        version: String,
        source_id: Option<String>,
        is_fork: bool,
    },
    /// Stop operations on specified events (kind:24134)
    StopOperations {
        project_a_tag: String,
        event_ids: Vec<String>,
        agent_pubkeys: Vec<String>,
    },
    /// Update agent configuration (kind:24020)
    UpdateAgentConfig {
        project_a_tag: String,
        agent_pubkey: String,
        model: Option<String>,
        tools: Vec<String>,
    },
    /// Subscribe to messages for a new project
    SubscribeToProjectMessages {
        project_a_tag: String,
    },
    /// Subscribe to metadata for a new project
    SubscribeToProjectMetadata {
        project_a_tag: String,
    },
    /// Create a new nudge (kind:4201)
    CreateNudge {
        title: String,
        description: String,
        content: String,
        hashtags: Vec<String>,
        /// Tools to allow (allow-tool tags) - Additive mode
        allow_tools: Vec<String>,
        /// Tools to deny (deny-tool tags) - Additive mode
        deny_tools: Vec<String>,
        /// Exclusive tool list (only-tool tags) - Exclusive mode
        /// When present, overrides all other tool permissions
        only_tools: Vec<String>,
    },
    /// Update an existing nudge (republish kind:4201 with same d-tag for replaceable events)
    /// Note: kind:4201 is NOT a replaceable event in NIP-33, so we create a new event
    /// and the old one becomes superseded by the newer timestamp
    UpdateNudge {
        original_id: String,
        title: String,
        description: String,
        content: String,
        hashtags: Vec<String>,
        allow_tools: Vec<String>,
        deny_tools: Vec<String>,
        /// Exclusive tool list (only-tool tags)
        only_tools: Vec<String>,
    },
    /// Delete a nudge (kind:5 deletion event referencing the nudge)
    DeleteNudge {
        nudge_id: String,
    },
    /// Disconnect from relays but keep the worker running
    Disconnect {
        /// Optional response channel to signal when disconnect is complete
        response_tx: Option<Sender<Result<(), String>>>,
    },
    /// Get current relay connection status
    GetRelayStatus {
        response_tx: Sender<usize>,
    },
    Shutdown,
}

/// Data changes that require the worker channel (not handled by SubscriptionStream).
#[derive(Debug, Clone)]
pub enum DataChange {
    /// Chunk from local streaming socket (not from Nostr)
    LocalStreamChunk {
        agent_pubkey: String,
        conversation_id: String,
        text_delta: Option<String>,
        reasoning_delta: Option<String>,
        is_finish: bool,
    },
    /// Ephemeral project status event (kind:24010) - not cached in nostrdb
    ProjectStatus {
        json: String,
    },
    /// MCP tools changed (kind:4200 events)
    MCPToolsChanged,
}

pub struct NostrWorker {
    client: Option<Client>,
    keys: Option<Keys>,
    user_pubkey: Option<String>,
    ndb: Arc<Ndb>,
    data_tx: Sender<DataChange>,
    command_rx: Receiver<NostrCommand>,
    rt_handle: Option<tokio::runtime::Handle>,
    event_stats: SharedEventStats,
    subscription_stats: SharedSubscriptionStats,
    negentropy_stats: SharedNegentropySyncStats,
    /// Pubkeys for which we've already requested kind:0 profiles
    /// Uses tokio::sync::RwLock to avoid blocking the Tokio runtime
    requested_profiles: Arc<RwLock<HashSet<String>>>,
    /// Project a_tags for which we've already subscribed to messages
    /// This prevents duplicate subscriptions when projects are rediscovered
    /// Uses tokio::sync::RwLock to avoid blocking the Tokio runtime
    subscribed_projects: Arc<RwLock<HashSet<String>>>,
    /// Cancellation token sender - signals background tasks to stop on disconnect
    cancel_tx: Option<watch::Sender<bool>>,
}

impl NostrWorker {
    pub fn new(
        ndb: Arc<Ndb>,
        data_tx: Sender<DataChange>,
        command_rx: Receiver<NostrCommand>,
        event_stats: SharedEventStats,
        subscription_stats: SharedSubscriptionStats,
        negentropy_stats: SharedNegentropySyncStats,
    ) -> Self {
        Self {
            client: None,
            keys: None,
            user_pubkey: None,
            ndb,
            data_tx,
            command_rx,
            rt_handle: None,
            event_stats,
            subscription_stats,
            negentropy_stats,
            requested_profiles: Arc::new(RwLock::new(HashSet::new())),
            subscribed_projects: Arc::new(RwLock::new(HashSet::new())),
            cancel_tx: None,
        }
    }

    pub fn run(mut self) {
        let rt = Runtime::new().expect("Failed to create runtime");
        self.rt_handle = Some(rt.handle().clone());
        debug_log("Nostr worker thread started");

        // Setup local streaming socket client
        let (local_chunk_tx, mut local_chunk_rx) = tokio_mpsc::channel::<LocalStreamChunk>(256);
        let socket_client = SocketStreamClient::new();
        let data_tx_for_socket = self.data_tx.clone();

        // Spawn socket client task
        rt.spawn(async move {
            socket_client.run(local_chunk_tx).await;
        });

        // Spawn task to forward local chunks to data_tx
        let rt_handle = rt.handle().clone();
        rt_handle.spawn(async move {
            while let Some(chunk) = local_chunk_rx.recv().await {
                // Extract borrowed values before moving owned fields
                let text_delta = chunk.text_delta().map(String::from);
                let reasoning_delta = chunk.reasoning_delta().map(String::from);
                let is_finish = chunk.is_finish();
                let data_change = DataChange::LocalStreamChunk {
                    agent_pubkey: chunk.agent_pubkey,
                    conversation_id: chunk.conversation_id,
                    text_delta,
                    reasoning_delta,
                    is_finish,
                };
                let _ = data_tx_for_socket.send(data_change);
            }
        });

        loop {
            if let Ok(cmd) = self.command_rx.recv() {
                match cmd {
                    NostrCommand::Connect { keys, user_pubkey, response_tx } => {
                        debug_log(&format!("Worker: Connecting with user {}", &user_pubkey[..8]));
                        let result = rt.block_on(self.handle_connect(keys, user_pubkey));
                        if let Some(tx) = response_tx {
                            let _ = tx.send(result.as_ref().map(|_| ()).map_err(|e| e.to_string()));
                        }
                        if let Err(e) = result {
                            tlog!("ERROR", "Failed to connect: {}", e);
                        }
                    }
                    NostrCommand::PublishThread { project_a_tag, title, content, agent_pubkey, branch, nudge_ids, reference_conversation_id, fork_message_id, response_tx } => {
                        debug_log("Worker: Publishing thread");
                        match rt.block_on(self.handle_publish_thread(project_a_tag, title, content, agent_pubkey, branch, nudge_ids, reference_conversation_id, fork_message_id)) {
                            Ok(event_id) => {
                                if let Some(tx) = response_tx {
                                    let _ = tx.send(event_id);
                                }
                            }
                            Err(e) => {
                                tlog!("ERROR", "Failed to publish thread: {}", e);
                            }
                        }
                    }
                    NostrCommand::PublishMessage { thread_id, project_a_tag, content, agent_pubkey, reply_to, branch, nudge_ids, ask_author_pubkey, response_tx } => {
                        tlog!("SEND", "Worker received PublishMessage command");
                        match rt.block_on(self.handle_publish_message(thread_id, project_a_tag, content, agent_pubkey, reply_to, branch, nudge_ids, ask_author_pubkey)) {
                            Ok(event_id) => {
                                if let Some(tx) = response_tx {
                                    let _ = tx.send(event_id);
                                }
                            }
                            Err(e) => {
                                tlog!("ERROR", "Failed to publish message: {}", e);
                            }
                        }
                    }
                    NostrCommand::BootProject { project_a_tag, project_pubkey } => {
                        debug_log(&format!("Worker: Booting project {}", project_a_tag));
                        if let Err(e) = rt.block_on(self.handle_boot_project(project_a_tag, project_pubkey)) {
                            tlog!("ERROR", "Failed to boot project: {}", e);
                        }
                    }
                    NostrCommand::UpdateProjectAgents { project_a_tag, agent_ids, mcp_tool_ids } => {
                        debug_log(&format!("Worker: Updating project agents for {}", project_a_tag));
                        if let Err(e) = rt.block_on(self.handle_update_project_agents(project_a_tag, agent_ids, mcp_tool_ids)) {
                            tlog!("ERROR", "Failed to update project agents: {}", e);
                        }
                    }
                    NostrCommand::CreateProject { name, description, agent_ids, mcp_tool_ids } => {
                        debug_log(&format!("Worker: Creating project {}", name));
                        if let Err(e) = rt.block_on(self.handle_create_project(name, description, agent_ids, mcp_tool_ids)) {
                            tlog!("ERROR", "Failed to create project: {}", e);
                        }
                    }
                    NostrCommand::CreateAgentDefinition { name, description, role, instructions, version, source_id, is_fork } => {
                        debug_log(&format!("Worker: Creating agent definition {}", name));
                        if let Err(e) = rt.block_on(self.handle_create_agent_definition(name, description, role, instructions, version, source_id, is_fork)) {
                            tlog!("ERROR", "Failed to create agent definition: {}", e);
                        }
                    }
                    NostrCommand::StopOperations { project_a_tag, event_ids, agent_pubkeys } => {
                        debug_log(&format!("Worker: Sending stop command for {} events", event_ids.len()));
                        if let Err(e) = rt.block_on(self.handle_stop_operations(project_a_tag, event_ids, agent_pubkeys)) {
                            tlog!("ERROR", "Failed to send stop command: {}", e);
                        }
                    }
                    NostrCommand::UpdateAgentConfig { project_a_tag, agent_pubkey, model, tools } => {
                        debug_log(&format!("Worker: Updating agent config for {}", &agent_pubkey[..8]));
                        if let Err(e) = rt.block_on(self.handle_update_agent_config(project_a_tag, agent_pubkey, model, tools)) {
                            tlog!("ERROR", "Failed to update agent config: {}", e);
                        }
                    }
                    NostrCommand::SubscribeToProjectMessages { project_a_tag } => {
                        debug_log(&format!("Worker: Subscribing to messages for project {}", &project_a_tag));
                        if let Err(e) = rt.block_on(self.handle_subscribe_to_project_messages(project_a_tag)) {
                            tlog!("ERROR", "Failed to subscribe to project messages: {}", e);
                        }
                    }
                    NostrCommand::SubscribeToProjectMetadata { project_a_tag } => {
                        debug_log(&format!("Worker: Subscribing to metadata for project {}", &project_a_tag));
                        if let Err(e) = rt.block_on(self.handle_subscribe_to_project_metadata(project_a_tag)) {
                            tlog!("ERROR", "Failed to subscribe to project metadata: {}", e);
                        }
                    }
                    NostrCommand::CreateNudge { title, description, content, hashtags, allow_tools, deny_tools, only_tools } => {
                        debug_log(&format!("Worker: Creating nudge '{}'", title));
                        if let Err(e) = rt.block_on(self.handle_create_nudge(title, description, content, hashtags, allow_tools, deny_tools, only_tools)) {
                            tlog!("ERROR", "Failed to create nudge: {}", e);
                        }
                    }
                    NostrCommand::UpdateNudge { original_id, title, description, content, hashtags, allow_tools, deny_tools, only_tools } => {
                        debug_log(&format!("Worker: Updating nudge '{}'", title));
                        if let Err(e) = rt.block_on(self.handle_update_nudge(original_id, title, description, content, hashtags, allow_tools, deny_tools, only_tools)) {
                            tlog!("ERROR", "Failed to update nudge: {}", e);
                        }
                    }
                    NostrCommand::DeleteNudge { nudge_id } => {
                        debug_log(&format!("Worker: Deleting nudge {}", &nudge_id[..8]));
                        if let Err(e) = rt.block_on(self.handle_delete_nudge(nudge_id)) {
                            tlog!("ERROR", "Failed to delete nudge: {}", e);
                        }
                    }
                    NostrCommand::Disconnect { response_tx } => {
                        debug_log("Worker: Disconnecting");
                        let result = rt.block_on(self.handle_disconnect());
                        if let Err(ref e) = result {
                            tlog!("ERROR", "Failed to disconnect: {}", e);
                        }
                        if let Some(tx) = response_tx {
                            let _ = tx.send(result.as_ref().map(|_| ()).map_err(|e| e.to_string()));
                        }
                    }
                    NostrCommand::GetRelayStatus { response_tx } => {
                        let connected_count = rt.block_on(async {
                            if let Some(client) = self.client.as_ref() {
                                let relays = client.relays().await;
                                relays.values()
                                    .filter(|r| r.status() == nostr_sdk::RelayStatus::Connected)
                                    .count()
                            } else {
                                0
                            }
                        });
                        let _ = response_tx.send(connected_count);
                    }
                    NostrCommand::Shutdown => {
                        debug_log("Worker: Shutting down");
                        if let Err(e) = rt.block_on(self.handle_disconnect()) {
                            eprintln!("[TENEX] Shutdown: disconnect failed: {}", e);
                            tlog!("ERROR", "Shutdown disconnect failed: {}", e);
                        } else {
                            eprintln!("[TENEX] Shutdown: disconnect completed");
                        }
                        break;
                    }
                }
            }
        }

        debug_log("Nostr worker thread stopped");
    }

    async fn handle_connect(&mut self, keys: Keys, user_pubkey: String) -> Result<()> {
        // Clone the existing Ndb and wrap it in NdbDatabase for the Client
        // This avoids opening a second database handle (which would cause LMDB concurrency crashes)
        // The clone is safe - Ndb internally uses Arc for the LMDB environment
        let ndb_database = nostr_ndb::NdbDatabase::from((*self.ndb).clone());

        let client = Client::builder()
            .signer(keys.clone())
            .database(ndb_database)
            .build();

        client.add_relay(RELAY_URL).await?;

        tlog!("CONN", "Starting relay connect...");
        let connect_start = std::time::Instant::now();
        let connect_result = tokio::time::timeout(std::time::Duration::from_secs(10), client.connect()).await;
        let connect_elapsed = connect_start.elapsed();

        match &connect_result {
            Ok(()) => tlog!("CONN", "Connect completed in {:?}", connect_elapsed),
            Err(_) => {
                tlog!("CONN", "Connect TIMED OUT after {:?}", connect_elapsed);
                return Err(anyhow::anyhow!("Connection timed out after {:?}", connect_elapsed));
            }
        }

        // Verify at least one relay is actually connected using polling loop
        // This handles race conditions where relay status may transition asynchronously
        let verify_start = std::time::Instant::now();
        let verify_timeout = std::time::Duration::from_secs(5);
        let poll_interval = std::time::Duration::from_millis(100);

        loop {
            let relays = client.relays().await;
            let connected_count = relays.values().filter(|r| r.status() == nostr_sdk::RelayStatus::Connected).count();

            if connected_count > 0 {
                tlog!("CONN", "Verified {} relay(s) connected after {:?}", connected_count, verify_start.elapsed());
                break;
            }

            if verify_start.elapsed() >= verify_timeout {
                tlog!("CONN", "No relays connected after {:?} polling", verify_timeout);
                return Err(anyhow::anyhow!("No relays connected after {:?} verification timeout", verify_timeout));
            }

            tokio::time::sleep(poll_interval).await;
        }

        self.client = Some(client);
        self.keys = Some(keys);
        self.user_pubkey = Some(user_pubkey.clone());

        // Create cancellation token for background tasks
        let (cancel_tx, _cancel_rx) = watch::channel(false);
        self.cancel_tx = Some(cancel_tx);

        self.start_subscriptions(&user_pubkey).await?;

        // Spawn negentropy sync for efficient reconciliation with relays that support it
        self.spawn_negentropy_sync(&user_pubkey);

        Ok(())
    }

    fn spawn_negentropy_sync(&self, user_pubkey: &str) {
        let client = self.client.as_ref()
            .expect("spawn_negentropy_sync called before Connect")
            .clone();
        let ndb = self.ndb.clone();
        let pubkey = match PublicKey::parse(user_pubkey) {
            Ok(p) => p,
            Err(e) => {
                tlog!("SYNC", "Failed to parse pubkey: {}", e);
                return;
            }
        };
        let rt_handle = self.rt_handle.as_ref()
            .expect("spawn_negentropy_sync called before runtime initialized")
            .clone();
        let negentropy_stats = self.negentropy_stats.clone();
        let cancel_rx = self.cancel_tx.as_ref()
            .expect("spawn_negentropy_sync called before cancel_tx initialized")
            .subscribe();

        // Mark negentropy sync as enabled
        negentropy_stats.set_enabled(true);

        let subscribed_projects = self.subscribed_projects.clone();
        rt_handle.spawn(async move {
            run_negentropy_sync(client, ndb, pubkey, negentropy_stats, cancel_rx, subscribed_projects).await;
        });
    }

    async fn start_subscriptions(&mut self, user_pubkey: &str) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        let pubkey = PublicKey::parse(user_pubkey)?;

        tlog!("CONN", "Starting subscriptions...");
        let sub_start = std::time::Instant::now();

        // 1. User's projects (kind:31933) - only owned projects
        let project_filter_owned = Filter::new().kind(Kind::Custom(KIND_PROJECT_DRAFT)).author(pubkey);
        let project_filter_json = serde_json::to_string(&project_filter_owned).ok();
        let output = client.subscribe(project_filter_owned.clone(), None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new("User projects".to_string(), vec![KIND_PROJECT_DRAFT], None)
                .with_raw_filter(project_filter_json.unwrap_or_default()),
        );
        tlog!("CONN", "Subscribed to projects (kind:{}) - owned by user", KIND_PROJECT_DRAFT);

        // 2. Status events (kind:24010, kind:24133) - since 45 seconds ago
        // kind:24010 is the GLOBAL subscription that tells us which projects are online.
        // When we receive these events, we create per-project subscriptions for kind:1, 513, 30023.
        let since_time = Timestamp::now() - 45;
        let project_status_filter = Filter::new()
            .kind(Kind::Custom(KIND_PROJECT_STATUS))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::P), user_pubkey.to_string())
            .since(since_time);
        let project_status_json = serde_json::to_string(&project_status_filter).ok();
        let project_output = client.subscribe(project_status_filter.clone(), None).await?;
        self.subscription_stats.register(
            project_output.val.to_string(),
            SubscriptionInfo::new("Project status updates".to_string(), vec![KIND_PROJECT_STATUS], None)
                .with_raw_filter(project_status_json.unwrap_or_default()),
        );

        // Backend uses uppercase P tag for kind:24133
        let agent_status_filter = Filter::new()
            .kind(Kind::Custom(KIND_AGENT_STATUS))
            .custom_tag(SingleLetterTag::uppercase(Alphabet::P), user_pubkey.to_string())
            .since(since_time);
        let agent_status_json = serde_json::to_string(&agent_status_filter).ok();
        let agent_output = client.subscribe(agent_status_filter.clone(), None).await?;
        self.subscription_stats.register(
            agent_output.val.to_string(),
            SubscriptionInfo::new("Operations status updates".to_string(), vec![KIND_AGENT_STATUS], None)
                .with_raw_filter(agent_status_json.unwrap_or_default()),
        );

        tlog!("CONN", "Subscribed to status events (kind:{}, kind:{})", KIND_PROJECT_STATUS, KIND_AGENT_STATUS);

        // 3. Global event definitions (kind:4199, 4200, 4201)
        let global_filter = Filter::new()
            .kinds(vec![Kind::Custom(KIND_AGENT), Kind::Custom(KIND_MCP_TOOL), Kind::Custom(KIND_NUDGE)]);
        let global_filter_json = serde_json::to_string(&global_filter).ok();
        let output = client.subscribe(global_filter.clone(), None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new("Global definitions".to_string(), vec![KIND_AGENT, KIND_MCP_TOOL, KIND_NUDGE], None)
                .with_raw_filter(global_filter_json.unwrap_or_default()),
        );
        tlog!("CONN", "Subscribed to global definitions (kind:{}, kind:{}, kind:{})", KIND_AGENT, KIND_MCP_TOOL, KIND_NUDGE);

        // 4. Per-project subscriptions (kind:513 metadata, kind:1 messages, kind:30023 reports)
        // OPTIMIZATION: We no longer subscribe to ALL projects at startup.
        // Instead, we subscribe only to projects that are:
        // a) Online (discovered via kind:24010 status events)
        // b) Explicitly requested by user (via SubscribeToProjectMessages command)
        //
        // This dramatically reduces subscription count from 3*N (where N is total projects)
        // to 3*M (where M is online/active projects).
        //
        // The notification handler will create subscriptions when:
        // - kind:24010 status events arrive for new online projects
        // - kind:31933 project events arrive (user discovers/adds project)
        tlog!("CONN", "Skipping bulk project subscriptions - will subscribe to projects on-demand when they come online or are explicitly requested");

        tlog!("CONN", "All subscriptions set up in {:?}", sub_start.elapsed());

        self.spawn_notification_handler();

        Ok(())
    }

    fn spawn_notification_handler(&self) {
        let client = self.client.as_ref()
            .expect("spawn_notification_handler called before Connect")
            .clone();
        let ndb = self.ndb.clone();
        let rt_handle = self.rt_handle.as_ref()
            .expect("spawn_notification_handler called before runtime initialized")
            .clone();
        let event_stats = self.event_stats.clone();
        let subscription_stats = self.subscription_stats.clone();
        let data_tx = self.data_tx.clone();
        let requested_profiles = self.requested_profiles.clone();
        let subscribed_projects = self.subscribed_projects.clone();
        let mut cancel_rx = self.cancel_tx.as_ref()
            .expect("spawn_notification_handler called before cancel_tx initialized")
            .subscribe();

        rt_handle.spawn(async move {
            let mut notifications = client.notifications();
            let mut first_event = true;
            let handler_start = std::time::Instant::now();
            tlog!("CONN", "Notification handler started, waiting for events...");

            loop {
                tokio::select! {
                    // Check for cancellation signal
                    _ = cancel_rx.changed() => {
                        if *cancel_rx.borrow() {
                            tlog!("CONN", "Notification handler received cancellation signal, exiting");
                            break;
                        }
                    }
                    // Process notifications
                    result = notifications.recv() => {
                        match result {
                            Ok(notification) => {
                                match notification {
                                    RelayPoolNotification::Event { relay_url, subscription_id, event } => {
                                    if first_event {
                                        tlog!("CONN", "First event received after {:?}", handler_start.elapsed());
                                        first_event = false;
                                    }
                                    debug_log(&format!("Received event: kind={} id={} from {}", event.kind, event.id, relay_url));

                                    // Track stats - extract project a-tag from event tags
                                    let project_a_tag = event.tags.iter()
                                        .find(|t| t.kind() == TagKind::SingleLetter(SingleLetterTag::lowercase(nostr_sdk::Alphabet::A)))
                                        .and_then(|t| t.content())
                                        .map(|s| s.to_string());
                                    event_stats.record(event.kind.as_u16(), project_a_tag.as_deref());

                                    // Track per-subscription event count
                                    subscription_stats.record_event(&subscription_id.to_string());

                                    let kind = event.kind.as_u16();

                                    // Ephemeral events (24010, 24133) go through DataChange channel
                                    if kind == KIND_PROJECT_STATUS || kind == KIND_AGENT_STATUS {
                                        if let Ok(json) = serde_json::to_string(&*event) {
                                            if let Err(e) = data_tx.send(DataChange::ProjectStatus { json: json.clone() }) {
                                                debug_log(&format!("Failed to send ProjectStatus data change: {}", e));
                                            }

                                            // For kind:24010 (project status), subscribe to project if it's newly online
                                            if kind == KIND_PROJECT_STATUS {
                                                if let Some(status) = ProjectStatus::from_json(&json) {
                                                    let a_tag = status.project_coordinate.clone();

                                                    match subscribe_project_if_new(&client, &ndb, &a_tag, &subscribed_projects, &subscription_stats).await {
                                                        Ok(true) => {
                                                            tlog!("CONN", "Project came online: {}, subscribed to messages", extract_project_name(&a_tag));
                                                        }
                                                        Ok(false) => {
                                                            // Already subscribed - no action needed
                                                        }
                                                        Err(e) => {
                                                            tlog!("ERROR", "Failed to subscribe to online project {}: {}", extract_project_name(&a_tag), e);
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    } else {
                                        if kind == KIND_TEXT_NOTE {
                                            let project_a_tag = event.tags.iter()
                                                .find(|t| t.kind() == TagKind::SingleLetter(SingleLetterTag::lowercase(nostr_sdk::Alphabet::A)))
                                                .and_then(|t| t.content())
                                                .unwrap_or("unknown");
                                            tlog!(
                                                "EVT",
                                                "kind:1 received id={} author={} a-tag={}",
                                                event.id.to_hex(),
                                                event.pubkey.to_hex(),
                                                project_a_tag
                                            );
                                        }
                                        // All other events go through nostrdb
                                        if let Err(e) = Self::handle_incoming_event(&ndb, *event.clone(), relay_url.as_str()) {
                                            tlog!("ERROR", "Failed to handle event: {}", e);
                                        }

                                        // For kind:1 messages, request author's profile if we haven't already
                                        // Use atomic check+insert pattern: insert returns true if value was newly inserted
                                        if kind == 1 {
                                            let author_hex = event.pubkey.to_hex();
                                            // Atomic check+insert: if insert returns true, we're the first to claim this profile
                                            let is_new = requested_profiles.write().await.insert(author_hex.clone());
                                            if is_new {
                                                // Subscribe to kind:0 for this author
                                                let profile_filter = Filter::new()
                                                    .kind(Kind::Metadata)
                                                    .author(event.pubkey);
                                                if let Err(e) = client.subscribe(profile_filter, None).await {
                                                    // Subscription failed - remove from set so we can retry later
                                                    requested_profiles.write().await.remove(&author_hex);
                                                    tlog!("ERROR", "Failed to subscribe to profile for {}: {}", &author_hex[..8], e);
                                                } else {
                                                    debug_log(&format!("Subscribed to profile for author {}", &author_hex[..8]));
                                                }
                                            }
                                        }

                                        // For kind:31933 (project), immediately subscribe to its messages
                                        // This fixes iOS connectivity where projects are discovered after initial subscriptions
                                        if kind == KIND_PROJECT_DRAFT {
                                            // Extract d-tag to build the a_tag (kind:pubkey:d_tag format)
                                            if let Some(d_tag) = event.tags.iter()
                                                .find(|t| t.kind() == TagKind::SingleLetter(SingleLetterTag::lowercase(nostr_sdk::Alphabet::D)))
                                                .and_then(|t| t.content())
                                            {
                                                let a_tag = format!("31933:{}:{}", event.pubkey.to_hex(), d_tag);

                                                match subscribe_project_if_new(&client, &ndb, &a_tag, &subscribed_projects, &subscription_stats).await {
                                                    Ok(true) => {
                                                        let project_name = d_tag.split(':').last().unwrap_or(d_tag);
                                                        tlog!("CONN", "New project discovered: {}, subscribed to messages", project_name);
                                                    }
                                                    Ok(false) => {
                                                        // Already subscribed - no action needed
                                                    }
                                                    Err(e) => {
                                                        let project_name = d_tag.split(':').last().unwrap_or(d_tag);
                                                        tlog!("ERROR", "Failed to subscribe to project {}: {}", project_name, e);
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    }
                                    RelayPoolNotification::Message { relay_url, message } => {
                                        if let RelayMessage::Ok { event_id, status, message } = &message {
                                            tlog!(
                                                "OK",
                                                "Relay OK from {}: id={} status={} msg={}",
                                                relay_url,
                                                event_id.to_hex(),
                                                status,
                                                message
                                            );
                                        }
                                    }
                                    RelayPoolNotification::Shutdown => {
                                        tlog!("CONN", "Notification handler received relay pool shutdown, exiting");
                                        break;
                                    }
                                }
                            }
                            Err(_) => {
                                // Channel closed, exit gracefully
                                tlog!("CONN", "Notification channel closed, handler exiting");
                                break;
                            }
                        }
                    }
                }
            }
            tlog!("CONN", "Notification handler stopped");
        });
    }

    fn handle_incoming_event(
        ndb: &Ndb,
        event: Event,
        relay_url: &str,
    ) -> Result<()> {
        // Ingest the event into nostrdb with relay metadata
        // UI gets notified via nostrdb SubscriptionStream when events are ready
        ingest_events(ndb, &[event.clone()], Some(relay_url))?;

        Ok(())
    }

    async fn handle_publish_thread(
        &self,
        project_a_tag: String,
        title: String,
        content: String,
        agent_pubkey: Option<String>,
        branch: Option<String>,
        nudge_ids: Vec<String>,
        reference_conversation_id: Option<String>,
        fork_message_id: Option<String>,
    ) -> Result<String> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        // Parse project coordinate for proper a-tag
        let coordinate = Coordinate::parse(&project_a_tag)
            .map_err(|e| anyhow::anyhow!("Invalid project coordinate: {}", e))?;

        let mut event = EventBuilder::new(Kind::from(1), &content)
            // Project reference (a tag) - required
            .tag(Tag::coordinate(coordinate, None))
            // Title tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec![title],
            ))
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        // Agent p-tag for routing (required for agent to respond)
        if let Some(agent_pk) = agent_pubkey {
            if let Ok(pk) = PublicKey::parse(&agent_pk) {
                event = event.tag(Tag::public_key(pk));
            }
        }

        // Branch tag
        if let Some(br) = branch {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("branch")),
                vec![br],
            ));
        }

        // Nudge tags
        for nudge_id in nudge_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("nudge")),
                vec![nudge_id],
            ));
        }

        // Reference conversation tag ("context" tag) for linking to source conversation
        // NOTE: Using "context" instead of "q" because "q" is reserved for delegation/child links
        if let Some(ref_id) = reference_conversation_id.clone() {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("context")),
                vec![ref_id],
            ));
        }

        // Fork tag: includes both conversation ID and message ID for forking from a specific point
        // Format: ["fork", "<conversation-id>", "<message-id>"]
        // INVARIANT: fork_message_id requires reference_conversation_id - enforce this to prevent silent data loss
        if let Some(msg_id) = fork_message_id {
            if let Some(conv_id) = reference_conversation_id.clone() {
                event = event.tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("fork")),
                    vec![conv_id, msg_id],
                ));
            } else {
                // CRITICAL: fork_message_id without reference_conversation_id is invalid state
                tlog!("ERROR", "Fork tag dropped: fork_message_id set without reference_conversation_id. This is a bug.");
                // Log the invalid state for debugging but continue execution
            }
        }

        // Build and sign the event
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;
        let signed_event = event.sign_with_keys(keys)?;
        let event_id = signed_event.id.to_hex();

        // Ingest locally into nostrdb so it appears immediately
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;
        // UI gets notified via nostrdb SubscriptionStream when data is ready

        // Send to relay with timeout (don't block forever on degraded connections)
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Published thread: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send thread to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending thread to relay (event was saved locally)"),
        }

        Ok(event_id)
    }

    async fn handle_publish_message(
        &self,
        thread_id: String,
        project_a_tag: String,
        content: String,
        agent_pubkey: Option<String>,
        reply_to: Option<String>,
        branch: Option<String>,
        nudge_ids: Vec<String>,
        ask_author_pubkey: Option<String>,
    ) -> Result<String> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        // Parse project coordinate for proper a-tag
        let coordinate = Coordinate::parse(&project_a_tag)
            .map_err(|e| anyhow::anyhow!("Invalid project coordinate: {}", e))?;

        let mut event = EventBuilder::new(Kind::from(1), &content)
            // NIP-10: e-tag with "root" marker (required)
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.clone(), "".to_string(), "root".to_string()],
            ))
            // Project reference (a tag)
            .tag(Tag::coordinate(coordinate, None))
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        // NIP-10: e-tag with "reply" marker (optional, for threaded replies)
        if let Some(reply_id) = reply_to {
            event = event.tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![reply_id, "".to_string(), "reply".to_string()],
            ));
        }

        // Agent p-tag for routing
        if let Some(agent_pk) = agent_pubkey {
            if let Ok(pk) = PublicKey::parse(&agent_pk) {
                event = event.tag(Tag::public_key(pk));
            }
        }

        // Ask author p-tag (when replying to ask events)
        if let Some(ask_pk) = ask_author_pubkey {
            if let Ok(pk) = PublicKey::parse(&ask_pk) {
                event = event.tag(Tag::public_key(pk));
            }
        }

        // Branch tag
        if let Some(br) = branch {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("branch")),
                vec![br],
            ));
        }

        // Nudge tags
        for nudge_id in nudge_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("nudge")),
                vec![nudge_id],
            ));
        }

        // Build and sign the event
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;
        tlog!("SEND", "Signing message...");
        let sign_start = std::time::Instant::now();
        let signed_event = event.sign_with_keys(keys)?;
        let event_id = signed_event.id.to_hex();
        tlog!("SEND", "Signed in {:?}", sign_start.elapsed());

        // Ingest locally into nostrdb - UI gets notified via SubscriptionStream
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;
        tlog!("SEND", "Ingested locally, now sending to relay...");

        // Send to relay with timeout (don't block forever on degraded connections)
        let send_start = std::time::Instant::now();
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => tlog!("SEND", "Published message in {:?}: {}", send_start.elapsed(), output.id()),
            Ok(Err(e)) => tlog!("SEND", "Failed after {:?}: {}", send_start.elapsed(), e),
            Err(_) => tlog!("SEND", "TIMEOUT after {:?} (event saved locally)", send_start.elapsed()),
        }

        Ok(event_id)
    }

    async fn handle_boot_project(&self, project_a_tag: String, project_pubkey: Option<String>) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Parse project coordinate for proper a-tag
        let coordinate = Coordinate::parse(&project_a_tag)
            .map_err(|e| anyhow::anyhow!("Invalid project coordinate: {}", e))?;

        // Kind 24000 boot request with a-tag pointing to project
        let mut event = EventBuilder::new(Kind::Custom(24000), "")
            .tag(Tag::coordinate(coordinate, None))
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        // Add p-tag for project owner (required by backend)
        if let Some(pubkey) = project_pubkey {
            if let Ok(pk) = PublicKey::parse(&pubkey) {
                event = event.tag(Tag::public_key(pk));
            }
        }

        let signed_event = event.sign_with_keys(keys)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Sent boot request: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send boot request to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending boot request to relay"),
        }

        Ok(())
    }

    async fn handle_update_project_agents(&self, project_a_tag: String, agent_ids: Vec<String>, mcp_tool_ids: Vec<String>) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Get the existing project from nostrdb
        let projects = crate::store::get_projects(&self.ndb)?;
        let project = projects
            .iter()
            .find(|p| p.a_tag() == project_a_tag)
            .ok_or_else(|| anyhow::anyhow!("Project not found: {}", project_a_tag))?;

        // Build the updated project event (kind 31933, NIP-33 replaceable)
        let mut event = EventBuilder::new(Kind::Custom(31933), "")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("d")),
                vec![project.id.clone()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec![project.name.clone()],
            ))
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        // Add participant p-tags
        for participant in &project.participants {
            if let Ok(pk) = PublicKey::parse(participant) {
                event = event.tag(Tag::public_key(pk));
            }
        }

        // Add agent tags (first agent is PM)
        for agent_id in &agent_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("agent")),
                vec![agent_id.clone()],
            ));
        }

        // Add MCP tool tags
        for tool_id in &mcp_tool_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("mcp")),
                vec![tool_id.clone()],
            ));
        }

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Updated project agents: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send project update to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending project update to relay (saved locally)"),
        }

        Ok(())
    }

    async fn handle_create_project(
        &self,
        name: String,
        description: String,
        agent_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
    ) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Generate d-tag from name (lowercase, replace spaces with dashes)
        let d_tag = name
            .to_lowercase()
            .chars()
            .map(|c| if c.is_alphanumeric() { c } else { '-' })
            .collect::<String>();

        // Build the project event (kind 31933, NIP-33 replaceable)
        let mut event = EventBuilder::new(Kind::Custom(31933), &description)
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("d")),
                vec![d_tag],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec![name],
            ))
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        // Add agent tags
        for agent_id in &agent_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("agent")),
                vec![agent_id.clone()],
            ));
        }

        // Add MCP tool tags
        for tool_id in &mcp_tool_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("mcp")),
                vec![tool_id.clone()],
            ));
        }

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Created project: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send project to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending project to relay (saved locally)"),
        }

        Ok(())
    }

    async fn handle_create_agent_definition(
        &self,
        name: String,
        description: String,
        role: String,
        instructions: String,
        version: String,
        source_id: Option<String>,
        is_fork: bool,
    ) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Build the agent definition event (kind 4199)
        let mut event = EventBuilder::new(Kind::Custom(4199), &instructions)
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec![name.clone()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("description")),
                vec![description],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("role")),
                vec![role],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("version")),
                vec![version],
            ))
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        // Add source reference for fork/clone
        if let Some(ref source) = source_id {
            if is_fork {
                // Fork: use 'e' tag to reference parent
                event = event.tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("e")),
                    vec![source.clone()],
                ));
            } else {
                // Clone: use 'cloned-from' tag
                event = event.tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("cloned-from")),
                    vec![source.clone()],
                ));
            }
        }

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Created agent definition '{}': {}", name, output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send agent definition to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending agent definition to relay (saved locally)"),
        }

        Ok(())
    }

    async fn handle_stop_operations(
        &self,
        project_a_tag: String,
        event_ids: Vec<String>,
        agent_pubkeys: Vec<String>,
    ) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Parse project coordinate for a-tag
        let coordinate = Coordinate::parse(&project_a_tag)
            .map_err(|e| anyhow::anyhow!("Invalid project coordinate: {}", e))?;

        // Build kind:24134 stop command event
        let mut event = EventBuilder::new(Kind::Custom(24134), "")
            .tag(Tag::coordinate(coordinate, None))
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        // Add e-tags for events to stop
        for event_id in &event_ids {
            event = event.tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![event_id.clone()],
            ));
        }

        // Add p-tags for agents to stop (optional - if empty, stops all agents on the events)
        for agent_pk in &agent_pubkeys {
            if let Ok(pk) = PublicKey::parse(agent_pk) {
                event = event.tag(Tag::public_key(pk));
            }
        }

        let signed_event = event.sign_with_keys(keys)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Sent stop command: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send stop command to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending stop command to relay"),
        }

        Ok(())
    }

    async fn handle_update_agent_config(
        &self,
        project_a_tag: String,
        agent_pubkey: String,
        model: Option<String>,
        tools: Vec<String>,
    ) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Parse project coordinate for a-tag
        let coordinate = Coordinate::parse(&project_a_tag)
            .map_err(|e| anyhow::anyhow!("Invalid project coordinate: {}", e))?;

        // Build kind:24020 agent config update event
        let mut event = EventBuilder::new(Kind::Custom(24020), "")
            .tag(Tag::coordinate(coordinate, None))
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        // Add p-tag for the agent being configured
        if let Ok(pk) = PublicKey::parse(&agent_pubkey) {
            event = event.tag(Tag::public_key(pk));
        }

        // Add model tag if specified
        if let Some(m) = model {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("model")),
                vec![m],
            ));
        }

        // Add tool tags (exhaustive list - empty means no tools)
        for tool in &tools {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("tool")),
                vec![tool.clone()],
            ));
        }

        let signed_event = event.sign_with_keys(keys)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Sent agent config update: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send agent config update to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending agent config update to relay"),
        }

        Ok(())
    }

    /// Shared helper for subscribing to project filters with deduplication.
    ///
    /// This method handles the common pattern of:
    /// 1. Atomic check+insert to prevent duplicate subscriptions (critical for iOS races)
    /// 2. Logging the subscription attempt
    /// 3. Calling subscribe_project_filters
    /// 4. Removing from set on failure to allow retry
    ///
    /// Used by both handle_subscribe_to_project_messages and handle_subscribe_to_project_metadata
    /// since they had identical logic.
    async fn subscribe_to_project_with_dedup(&self, project_a_tag: String) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        // Use atomic check+insert to prevent duplicate subscriptions
        // This is critical for iOS where refresh() and notification handler can race
        let is_new = self.subscribed_projects.write().await.insert(project_a_tag.clone());
        if !is_new {
            // Already subscribed (likely by notification handler)
            let project_name = project_a_tag.split(':').nth(2).unwrap_or("unknown");
            tlog!("CONN", "Skipping duplicate subscription for project: {}", project_name);
            return Ok(());
        }

        let project_name = project_a_tag.split(':').nth(2).unwrap_or("unknown");
        tlog!("CONN", "Adding subscriptions for project: {}", project_name);

        // Use the shared helper for consistent subscription behavior
        match subscribe_project_filters(client, &self.ndb, &self.subscription_stats, &project_a_tag).await {
            Ok(()) => Ok(()),
            Err(e) => {
                // Subscription failed - remove from set so we can retry later
                self.subscribed_projects.write().await.remove(&project_a_tag);
                Err(e)
            }
        }
    }

    async fn handle_subscribe_to_project_messages(&self, project_a_tag: String) -> Result<()> {
        self.subscribe_to_project_with_dedup(project_a_tag).await
    }

    async fn handle_subscribe_to_project_metadata(&self, project_a_tag: String) -> Result<()> {
        self.subscribe_to_project_with_dedup(project_a_tag).await
    }

    async fn handle_disconnect(&mut self) -> Result<()> {
        // Signal cancellation to background tasks FIRST, before disconnecting
        if let Some(cancel_tx) = &self.cancel_tx {
            let _ = cancel_tx.send(true);
            tlog!("CONN", "Sent cancellation signal to background tasks");
        }

        if let Some(client) = &self.client {
            client.disconnect().await;
        }

        // Clear requested_profiles so new sessions start fresh
        // This prevents stale/missing profiles when reconnecting with same or different user
        self.requested_profiles.write().await.clear();
        tlog!("CONN", "Cleared requested_profiles");

        // Clear subscribed_projects so new sessions start fresh
        // This ensures projects will be re-subscribed on reconnect
        self.subscribed_projects.write().await.clear();
        tlog!("CONN", "Cleared subscribed_projects");

        self.client = None;
        self.keys = None;
        self.user_pubkey = None;
        self.cancel_tx = None;
        Ok(())
    }

    /// Create a new nudge (kind:4201)
    async fn handle_create_nudge(
        &self,
        title: String,
        description: String,
        content: String,
        hashtags: Vec<String>,
        allow_tools: Vec<String>,
        deny_tools: Vec<String>,
        only_tools: Vec<String>,
    ) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Build the nudge event (kind:4201)
        let mut event = EventBuilder::new(Kind::Custom(4201), &content)
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec![title.clone()],
            ))
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        // Add description tag if non-empty
        if !description.is_empty() {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("description")),
                vec![description],
            ));
        }

        // Add hashtag tags
        for tag in &hashtags {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                vec![tag.clone()],
            ));
        }

        // Tool permissions: only-tool takes priority (XOR with allow/deny)
        if !only_tools.is_empty() {
            // Exclusive mode: only-tool tags
            for tool in &only_tools {
                event = event.tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("only-tool")),
                    vec![tool.clone()],
                ));
            }
        } else {
            // Additive mode: allow-tool and deny-tool tags
            for tool in &allow_tools {
                event = event.tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("allow-tool")),
                    vec![tool.clone()],
                ));
            }

            for tool in &deny_tools {
                event = event.tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("deny-tool")),
                    vec![tool.clone()],
                ));
            }
        }

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Created nudge '{}': {}", title, output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send nudge to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending nudge to relay (saved locally)"),
        }

        Ok(())
    }

    /// Update an existing nudge (create new event with updated content)
    /// Since kind:4201 is not a replaceable event, we create a new event
    /// and add a reference to the original
    async fn handle_update_nudge(
        &self,
        original_id: String,
        title: String,
        description: String,
        content: String,
        hashtags: Vec<String>,
        allow_tools: Vec<String>,
        deny_tools: Vec<String>,
        only_tools: Vec<String>,
    ) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Build the updated nudge event (kind:4201)
        let mut event = EventBuilder::new(Kind::Custom(4201), &content)
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec![title.clone()],
            ))
            // Reference to original nudge (supersedes relationship)
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("supersedes")),
                vec![original_id],
            ))
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        // Add description tag if non-empty
        if !description.is_empty() {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("description")),
                vec![description],
            ));
        }

        // Add hashtag tags
        for tag in &hashtags {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("t")),
                vec![tag.clone()],
            ));
        }

        // Tool permissions: only-tool takes priority (XOR with allow/deny)
        if !only_tools.is_empty() {
            // Exclusive mode: only-tool tags
            for tool in &only_tools {
                event = event.tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("only-tool")),
                    vec![tool.clone()],
                ));
            }
        } else {
            // Additive mode: allow-tool and deny-tool tags
            for tool in &allow_tools {
                event = event.tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("allow-tool")),
                    vec![tool.clone()],
                ));
            }

            for tool in &deny_tools {
                event = event.tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("deny-tool")),
                    vec![tool.clone()],
                ));
            }
        }

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Updated nudge '{}': {}", title, output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send updated nudge to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending updated nudge to relay (saved locally)"),
        }

        Ok(())
    }

    /// Delete a nudge (kind:5 deletion event)
    async fn handle_delete_nudge(&self, nudge_id: String) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Parse the nudge event ID
        let event_id = EventId::parse(&nudge_id)
            .map_err(|e| anyhow::anyhow!("Invalid event ID: {}", e))?;

        // Build the deletion event (kind:5 per NIP-09)
        let deletion_request = EventDeletionRequest::new().id(event_id);
        let event = EventBuilder::delete(deletion_request)
            // NIP-89 client tag
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-tui".to_string()],
            ));

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Deleted nudge {}: deletion event {}", &nudge_id[..8], output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send deletion event to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending deletion event to relay (saved locally)"),
        }

        Ok(())
    }
}

/// Run negentropy sync loop with adaptive timing
/// Syncs project-scoped kinds: 31933 (projects), 513 (conversation metadata),
/// 1 (messages), and 30023 (long-form content).
/// Global event types (4199, 4200, 4201) are handled via real-time subscriptions only.
async fn run_negentropy_sync(
    client: Client,
    ndb: Arc<Ndb>,
    user_pubkey: PublicKey,
    stats: SharedNegentropySyncStats,
    mut cancel_rx: watch::Receiver<bool>,
    subscribed_projects: Arc<RwLock<HashSet<String>>>,
) {
    use std::time::Duration;

    let mut interval_secs: u64 = 60;
    const MAX_INTERVAL: u64 = 900; // 15 minutes cap

    tlog!("SYNC", "Starting initial negentropy sync...");
    stats.set_interval(interval_secs);

    loop {
        // Check for cancellation before each cycle
        if *cancel_rx.borrow() {
            tlog!("SYNC", "Negentropy sync received cancellation signal, exiting");
            break;
        }

        stats.set_in_progress(true);
        let total_new = sync_all_filters(&client, &ndb, &user_pubkey, &stats, &subscribed_projects).await;
        stats.record_cycle_complete();
        stats.set_in_progress(false);

        if total_new == 0 {
            interval_secs = (interval_secs * 2).min(MAX_INTERVAL);
            tlog!("SYNC", "No gaps found. Next sync in {}s", interval_secs);
        } else {
            interval_secs = 60;
            tlog!("SYNC", "Found {} new events. Next sync in {}s", total_new, interval_secs);
        }

        stats.set_interval(interval_secs);

        // Use select! to wait for either the sleep to complete or cancellation
        tokio::select! {
            _ = cancel_rx.changed() => {
                if *cancel_rx.borrow() {
                    tlog!("SYNC", "Negentropy sync received cancellation signal during sleep, exiting");
                    break;
                }
            }
            _ = tokio::time::sleep(Duration::from_secs(interval_secs)) => {
                // Continue to next iteration
            }
        }
    }
    tlog!("SYNC", "Negentropy sync loop stopped");
}

/// Sync all non-ephemeral kinds using negentropy reconciliation
async fn sync_all_filters(
    client: &Client,
    _ndb: &Ndb,
    user_pubkey: &PublicKey,
    stats: &SharedNegentropySyncStats,
    subscribed_projects: &Arc<RwLock<HashSet<String>>>,
) -> u64 {
    let mut total_new: u64 = 0;
    let user_pubkey_hex = user_pubkey.to_hex();

    // User's projects (kind 31933) - authored by user
    let project_filter = Filter::new()
        .kind(Kind::Custom(31933))
        .author(*user_pubkey);
    total_new += sync_filter(client, project_filter, "31933-authored", stats).await;

    // Projects where user is a participant (kind 31933) - via p-tag
    let project_p_filter = Filter::new()
        .kind(Kind::Custom(31933))
        .custom_tag(SingleLetterTag::lowercase(Alphabet::P), user_pubkey_hex.clone());
    total_new += sync_filter(client, project_p_filter, "31933-p-tagged", stats).await;

    // Agent definitions (kind 4199)
    let agent_filter = Filter::new().kind(Kind::Custom(4199));
    total_new += sync_filter(client, agent_filter, "4199", stats).await;

    // Conversation metadata (kind 513) - only for subscribed projects
    if !subscribed_projects.read().await.is_empty() {
        let subscribed = subscribed_projects.read().await;
        let atags: Vec<String> = subscribed.iter().cloned().collect();
        let metadata_filter = Filter::new()
            .kind(Kind::Custom(513))
            .custom_tags(SingleLetterTag::lowercase(Alphabet::A), atags);
        total_new += sync_filter(client, metadata_filter, "513", stats).await;
    }

    // MCP tools (kind 4200)
    let mcp_tool_filter = Filter::new().kind(Kind::Custom(4200));
    total_new += sync_filter(client, mcp_tool_filter, "4200", stats).await;

    // Nudges (kind 4201) - global, like agent definitions
    let nudge_filter = Filter::new().kind(Kind::Custom(4201));
    total_new += sync_filter(client, nudge_filter, "4201", stats).await;

    // Messages (kind 1) and long-form content (kind 30023) with project a-tags
    // OPTIMIZATION: Only sync for projects we're actually subscribed to (online/active projects)
    let subscribed = subscribed_projects.read().await;
    if !subscribed.is_empty() {
        let atags: Vec<String> = subscribed.iter().cloned().collect();

        let msg_filter = Filter::new()
            .kind(Kind::from(1))
            .custom_tags(SingleLetterTag::lowercase(Alphabet::A), atags.clone());
        total_new += sync_filter(client, msg_filter, "1", stats).await;

        let longform_filter = Filter::new()
            .kind(Kind::Custom(30023))
            .custom_tags(SingleLetterTag::lowercase(Alphabet::A), atags);
        total_new += sync_filter(client, longform_filter, "30023", stats).await;
    }

    total_new
}

/// Perform negentropy sync for a single filter
/// Returns the number of new events received
async fn sync_filter(
    client: &Client,
    filter: Filter,
    label: &str,
    stats: &SharedNegentropySyncStats,
) -> u64 {
    let opts = SyncOptions::default();

    match client.sync(filter, &opts).await {
        Ok(output) => {
            // output.val is Reconciliation, output.success is HashSet<RelayUrl>
            let count = output.val.received.len() as u64;

            if count > 0 {
                tlog!("SYNC", "kind:{} -> {} new events", label, count);
            }

            // Record success
            stats.record_success(label, count);

            count
        }
        Err(e) => {
            let err_str = format!("{}", e);
            let is_unsupported = err_str.contains("not supported") || err_str.contains("NEG-ERR");

            // Only log if it's not a "not supported" error (common for relays without negentropy)
            if !is_unsupported {
                tlog!("SYNC", "kind:{} failed: {}", label, e);
            }

            // Record failure
            stats.record_failure(label, &err_str, is_unsupported);

            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_coordinate_parse() {
        let a_tag = "31933:09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7:DDD-83ayt6";
        let result = Coordinate::parse(a_tag);
        println!("Parse result: {:?}", result);
        assert!(result.is_ok(), "Failed to parse: {:?}", result.err());
    }

    /// Helper to build nudge event tags based on tool permission mode
    /// Returns the tags that would be added to a nudge event
    fn build_nudge_tool_tags(
        allow_tools: &[String],
        deny_tools: &[String],
        only_tools: &[String],
    ) -> Vec<Tag> {
        let mut tags = Vec::new();

        // XOR logic: only_tools takes priority (exclusive mode)
        if !only_tools.is_empty() {
            // Exclusive mode: only-tool tags
            for tool in only_tools {
                tags.push(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("only-tool")),
                    vec![tool.clone()],
                ));
            }
        } else {
            // Additive mode: allow-tool and deny-tool tags
            for tool in allow_tools {
                tags.push(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("allow-tool")),
                    vec![tool.clone()],
                ));
            }

            for tool in deny_tools {
                tags.push(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("deny-tool")),
                    vec![tool.clone()],
                ));
            }
        }

        tags
    }

    /// Helper to check if a tag list contains a specific tag type
    fn has_tag_type(tags: &[Tag], tag_name: &str) -> bool {
        tags.iter().any(|t| {
            if let TagKind::Custom(name) = t.kind() {
                name.as_ref() == tag_name
            } else {
                false
            }
        })
    }

    /// Helper to extract all values for a specific tag type
    fn get_tag_values(tags: &[Tag], tag_name: &str) -> Vec<String> {
        tags.iter()
            .filter_map(|t| {
                if let TagKind::Custom(name) = t.kind() {
                    if name.as_ref() == tag_name {
                        t.content().map(|s| s.to_string())
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect()
    }

    #[test]
    fn test_create_nudge_emits_only_tool_tags_in_exclusive_mode() {
        let allow_tools = vec!["Bash".to_string(), "Read".to_string()];
        let deny_tools = vec!["Write".to_string()];
        let only_tools = vec!["Grep".to_string(), "Glob".to_string()];

        let tags = build_nudge_tool_tags(&allow_tools, &deny_tools, &only_tools);

        // Exclusive mode: should emit only-tool tags
        assert!(has_tag_type(&tags, "only-tool"), "Should have only-tool tags");

        // Exclusive mode: should NOT emit allow-tool or deny-tool tags
        assert!(!has_tag_type(&tags, "allow-tool"), "Should NOT have allow-tool tags in exclusive mode");
        assert!(!has_tag_type(&tags, "deny-tool"), "Should NOT have deny-tool tags in exclusive mode");

        // Verify the only-tool values
        let only_values = get_tag_values(&tags, "only-tool");
        assert_eq!(only_values.len(), 2);
        assert!(only_values.contains(&"Grep".to_string()));
        assert!(only_values.contains(&"Glob".to_string()));
    }

    #[test]
    fn test_create_nudge_emits_allow_deny_tags_in_additive_mode() {
        let allow_tools = vec!["Bash".to_string(), "Read".to_string()];
        let deny_tools = vec!["Write".to_string()];
        let only_tools: Vec<String> = vec![]; // Empty = additive mode

        let tags = build_nudge_tool_tags(&allow_tools, &deny_tools, &only_tools);

        // Additive mode: should emit allow-tool and deny-tool tags
        assert!(has_tag_type(&tags, "allow-tool"), "Should have allow-tool tags");
        assert!(has_tag_type(&tags, "deny-tool"), "Should have deny-tool tags");

        // Additive mode: should NOT emit only-tool tags
        assert!(!has_tag_type(&tags, "only-tool"), "Should NOT have only-tool tags in additive mode");

        // Verify the allow-tool values
        let allow_values = get_tag_values(&tags, "allow-tool");
        assert_eq!(allow_values.len(), 2);
        assert!(allow_values.contains(&"Bash".to_string()));
        assert!(allow_values.contains(&"Read".to_string()));

        // Verify the deny-tool values
        let deny_values = get_tag_values(&tags, "deny-tool");
        assert_eq!(deny_values.len(), 1);
        assert!(deny_values.contains(&"Write".to_string()));
    }

    #[test]
    fn test_exclusive_mode_never_emits_allow_deny_tags() {
        // Even when allow/deny lists are non-empty, exclusive mode ignores them
        let allow_tools = vec!["Tool1".to_string(), "Tool2".to_string()];
        let deny_tools = vec!["Tool3".to_string(), "Tool4".to_string()];
        let only_tools = vec!["ExclusiveTool".to_string()]; // Non-empty = exclusive mode

        let tags = build_nudge_tool_tags(&allow_tools, &deny_tools, &only_tools);

        // Exclusive mode takes precedence
        assert!(has_tag_type(&tags, "only-tool"));
        assert!(!has_tag_type(&tags, "allow-tool"), "Exclusive mode must never emit allow-tool");
        assert!(!has_tag_type(&tags, "deny-tool"), "Exclusive mode must never emit deny-tool");

        // Only the only-tool should be present
        let only_values = get_tag_values(&tags, "only-tool");
        assert_eq!(only_values, vec!["ExclusiveTool"]);
    }

    #[test]
    fn test_additive_mode_never_emits_only_tool_tags() {
        let allow_tools = vec!["AllowTool".to_string()];
        let deny_tools = vec!["DenyTool".to_string()];
        let only_tools: Vec<String> = vec![]; // Empty = additive mode

        let tags = build_nudge_tool_tags(&allow_tools, &deny_tools, &only_tools);

        // Additive mode should never emit only-tool
        assert!(!has_tag_type(&tags, "only-tool"), "Additive mode must never emit only-tool");
        assert!(has_tag_type(&tags, "allow-tool"));
        assert!(has_tag_type(&tags, "deny-tool"));
    }

    #[test]
    fn test_update_nudge_xor_logic_exclusive_mode() {
        // This test verifies the same XOR logic applies to UpdateNudge
        let allow_tools = vec!["OldAllow".to_string()];
        let deny_tools = vec!["OldDeny".to_string()];
        let only_tools = vec!["NewExclusive1".to_string(), "NewExclusive2".to_string()];

        let tags = build_nudge_tool_tags(&allow_tools, &deny_tools, &only_tools);

        // XOR: only_tools present means exclusive mode
        assert!(has_tag_type(&tags, "only-tool"));
        assert!(!has_tag_type(&tags, "allow-tool"));
        assert!(!has_tag_type(&tags, "deny-tool"));

        let only_values = get_tag_values(&tags, "only-tool");
        assert_eq!(only_values.len(), 2);
    }

    #[test]
    fn test_update_nudge_xor_logic_additive_mode() {
        // UpdateNudge with empty only_tools falls back to additive mode
        let allow_tools = vec!["AllowA".to_string(), "AllowB".to_string()];
        let deny_tools = vec!["DenyX".to_string()];
        let only_tools: Vec<String> = vec![];

        let tags = build_nudge_tool_tags(&allow_tools, &deny_tools, &only_tools);

        // XOR: empty only_tools means additive mode
        assert!(!has_tag_type(&tags, "only-tool"));
        assert!(has_tag_type(&tags, "allow-tool"));
        assert!(has_tag_type(&tags, "deny-tool"));

        let allow_values = get_tag_values(&tags, "allow-tool");
        assert_eq!(allow_values.len(), 2);

        let deny_values = get_tag_values(&tags, "deny-tool");
        assert_eq!(deny_values.len(), 1);
    }

    #[test]
    fn test_empty_tools_produces_no_tool_tags() {
        let allow_tools: Vec<String> = vec![];
        let deny_tools: Vec<String> = vec![];
        let only_tools: Vec<String> = vec![];

        let tags = build_nudge_tool_tags(&allow_tools, &deny_tools, &only_tools);

        assert!(!has_tag_type(&tags, "only-tool"));
        assert!(!has_tag_type(&tags, "allow-tool"));
        assert!(!has_tag_type(&tags, "deny-tool"));
        assert!(tags.is_empty());
    }
}
