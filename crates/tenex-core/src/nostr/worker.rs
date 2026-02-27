use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use anyhow::Result;
use nostr_ndb::database::{
    Backend, DatabaseError, DatabaseEventStatus, Events, NostrDatabase, RejectedReason,
    SaveEventStatus,
};
use nostr_sdk::prelude::*;
use nostrdb::{Ndb, Transaction};
use tokio::runtime::Runtime;
use tokio::sync::mpsc as tokio_mpsc;
use tokio::sync::watch;
use tokio::sync::RwLock;

use crate::constants::RELAY_URL;
use crate::models::ProjectStatus;
use crate::stats::{
    SharedEventStats, SharedNegentropySyncStats, SharedSubscriptionStats, SubscriptionInfo,
};
use crate::store::ingest_events;
use crate::streaming::{LocalStreamChunk, SocketStreamClient};

// Event kind constants
const KIND_TEXT_NOTE: u16 = 1;
const KIND_REACTION: u16 = 7;
const KIND_LONG_FORM_CONTENT: u16 = 30023;
const KIND_COMMENT: u16 = 1111;
const KIND_PROJECT_METADATA: u16 = 513;
const KIND_AGENT: u16 = 4199;
const KIND_MCP_TOOL: u16 = 4200;
const KIND_NUDGE: u16 = 4201;
const KIND_SKILL: u16 = 4202;
const KIND_BOOKMARK_LIST: u16 = 14202;
const KIND_PROJECT_STATUS: u16 = 24010;
const KIND_PROJECT_DRAFT: u16 = 31933;
const KIND_TEAM_PACK: u16 = 34199;
const KIND_AGENT_STATUS: u16 = 24133;

/// Wrapper around `nostr_ndb::NdbDatabase` that rejects ephemeral events.
///
/// This keeps relay notifications flowing, but avoids persisting ephemeral kinds
/// (20000-29999) into the shared `nostrdb` cache.
#[derive(Debug, Clone)]
struct EphemeralFilteringNdbDatabase {
    inner: nostr_ndb::NdbDatabase,
}

type NostrBoxedFuture<'a, T> = nostr_ndb::database::nostr::util::BoxedFuture<'a, T>;

impl EphemeralFilteringNdbDatabase {
    fn new(inner: nostr_ndb::NdbDatabase) -> Self {
        Self { inner }
    }
}

impl NostrDatabase for EphemeralFilteringNdbDatabase {
    fn backend(&self) -> Backend {
        self.inner.backend()
    }

    fn save_event<'a>(
        &'a self,
        event: &'a Event,
    ) -> NostrBoxedFuture<'a, Result<SaveEventStatus, DatabaseError>> {
        if event.kind.is_ephemeral() {
            return Box::pin(async { Ok(SaveEventStatus::Rejected(RejectedReason::Ephemeral)) });
        }

        self.inner.save_event(event)
    }

    fn check_id<'a>(
        &'a self,
        event_id: &'a EventId,
    ) -> NostrBoxedFuture<'a, Result<DatabaseEventStatus, DatabaseError>> {
        self.inner.check_id(event_id)
    }

    fn event_by_id<'a>(
        &'a self,
        event_id: &'a EventId,
    ) -> NostrBoxedFuture<'a, Result<Option<Event>, DatabaseError>> {
        self.inner.event_by_id(event_id)
    }

    fn count(&self, filter: Filter) -> NostrBoxedFuture<'_, Result<usize, DatabaseError>> {
        self.inner.count(filter)
    }

    fn query(&self, filter: Filter) -> NostrBoxedFuture<'_, Result<Events, DatabaseError>> {
        self.inner.query(filter)
    }

    fn negentropy_items(
        &self,
        filter: Filter,
    ) -> NostrBoxedFuture<'_, Result<Vec<(EventId, Timestamp)>, DatabaseError>> {
        self.inner.negentropy_items(filter)
    }

    fn delete(&self, filter: Filter) -> NostrBoxedFuture<'_, Result<(), DatabaseError>> {
        self.inner.delete(filter)
    }

    fn wipe(&self) -> NostrBoxedFuture<'_, Result<(), DatabaseError>> {
        self.inner.wipe()
    }
}

static START_TIME: OnceLock<Instant> = OnceLock::new();
static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();
static LOG_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

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
    let lock = LOG_LOCK.get_or_init(|| Mutex::new(()));
    if let Ok(_guard) = lock.lock() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(get_log_path())
        {
            let _ = writeln!(file, "[{:>8}ms] [{}] {}", elapsed_ms(), tag, msg);
        }
    }
}

#[macro_export]
macro_rules! tlog {
    ($tag:expr, $($arg:tt)*) => {
        $crate::nostr::worker::log_to_file($tag, &format!($($arg)*))
    };
}

fn debug_log(msg: &str) {
    if std::env::var("TENEX_DEBUG")
        .map(|v| v == "1")
        .unwrap_or(false)
    {
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
            debug_log(&format!(
                "✅ Subscribed to newly online project: {}",
                extract_project_name(a_tag)
            ));
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
        .custom_tag(
            SingleLetterTag::lowercase(Alphabet::A),
            project_a_tag.to_string(),
        );
    if let Some(latest) =
        latest_kind_timestamp_for_project(ndb, KIND_PROJECT_METADATA, project_a_tag)
    {
        // Subtract 1s to avoid missing same-second events
        metadata_filter = metadata_filter.since(Timestamp::from(latest.saturating_sub(1)));
    }
    let metadata_filter_json = serde_json::to_string(&metadata_filter).ok();
    let metadata_output = client
        .subscribe(metadata_filter.clone(), None)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to subscribe to metadata for {}: {}",
                project_name,
                e
            )
        })?;
    subscription_stats.register(
        metadata_output.val.to_string(),
        SubscriptionInfo::new(
            format!("{} metadata", project_name),
            vec![KIND_PROJECT_METADATA],
            Some(project_a_tag.to_string()),
        )
        .with_raw_filter(metadata_filter_json.unwrap_or_default()),
    );

    // Messages subscription (kind:1)
    let mut message_filter = Filter::new().kind(Kind::from(KIND_TEXT_NOTE)).custom_tag(
        SingleLetterTag::lowercase(Alphabet::A),
        project_a_tag.to_string(),
    );
    if let Some(latest) = latest_kind_timestamp_for_project(ndb, KIND_TEXT_NOTE, project_a_tag) {
        // Subtract 1s to avoid missing same-second events
        message_filter = message_filter.since(Timestamp::from(latest.saturating_sub(1)));
    }
    let message_filter_json = serde_json::to_string(&message_filter).ok();
    let message_output = client
        .subscribe(message_filter.clone(), None)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to subscribe to messages for {}: {}",
                project_name,
                e
            )
        })?;
    subscription_stats.register(
        message_output.val.to_string(),
        SubscriptionInfo::new(
            format!("{} messages", project_name),
            vec![KIND_TEXT_NOTE],
            Some(project_a_tag.to_string()),
        )
        .with_raw_filter(message_filter_json.unwrap_or_default()),
    );

    // Long-form content subscription (kind:30023)
    let longform_filter = Filter::new()
        .kind(Kind::Custom(KIND_LONG_FORM_CONTENT))
        .custom_tag(
            SingleLetterTag::lowercase(Alphabet::A),
            project_a_tag.to_string(),
        );
    let longform_filter_json = serde_json::to_string(&longform_filter).ok();
    let longform_output = client
        .subscribe(longform_filter.clone(), None)
        .await
        .map_err(|e| {
            anyhow::anyhow!("Failed to subscribe to reports for {}: {}", project_name, e)
        })?;
    subscription_stats.register(
        longform_output.val.to_string(),
        SubscriptionInfo::new(
            format!("{} reports", project_name),
            vec![KIND_LONG_FORM_CONTENT],
            Some(project_a_tag.to_string()),
        )
        .with_raw_filter(longform_filter_json.unwrap_or_default()),
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
        relay_urls: Vec<String>,
        response_tx: Option<Sender<Result<(), String>>>,
    },
    PublishThread {
        project_a_tag: String,
        title: String,
        content: String,
        agent_pubkey: Option<String>,
        nudge_ids: Vec<String>,
        skill_ids: Vec<String>,
        /// Optional reference to another conversation (adds "context" tag for referencing source conversations)
        reference_conversation_id: Option<String>,
        /// Optional report a-tag reference (adds second "a" tag for report discussions)
        reference_report_a_tag: Option<String>,
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
        nudge_ids: Vec<String>,
        skill_ids: Vec<String>,
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
        agent_definition_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
    },
    /// Save a project - create new or update existing (kind:31933)
    SaveProject {
        /// Optional slug (d-tag). If not provided, generated from name.
        /// Should be pre-normalized by the caller using slug::normalize_slug.
        slug: Option<String>,
        name: String,
        description: String,
        agent_definition_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
        /// Client identifier for the client tag (e.g., "tenex-cli", "tenex-tui")
        #[allow(dead_code)]
        client: Option<String>,
    },
    /// Update an existing project (kind:31933 replaceable)
    UpdateProject {
        project_a_tag: String,
        title: String,
        description: String,
        repo_url: Option<String>,
        picture_url: Option<String>,
        agent_definition_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
        /// Client identifier for the client tag
        client: Option<String>,
    },
    /// Tombstone-delete an existing project by republishing with ["deleted"] tag
    DeleteProject {
        project_a_tag: String,
        /// Client identifier for the client tag
        client: Option<String>,
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
    /// Delete an agent definition (kind:5 deletion event referencing kind:4199)
    DeleteAgentDefinition {
        agent_id: String,
        /// Client identifier for the client tag
        client: Option<String>,
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
        /// Additional marker tags (e.g. ["pm"])
        tags: Vec<String>,
    },
    /// Update global agent configuration (kind:24020) without a project a-tag
    UpdateGlobalAgentConfig {
        agent_pubkey: String,
        model: Option<String>,
        tools: Vec<String>,
        /// Additional marker tags (e.g. ["pm"])
        tags: Vec<String>,
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
    /// Publish or update the user's bookmark list (kind:14202 replaceable event).
    /// The full list is published each time (not incremental) since it is replaceable.
    /// Empty vec clears the bookmark list.
    PublishBookmarkList {
        bookmarked_ids: Vec<String>,
    },
    /// React to a team pack (kind:7 NIP-25).
    ReactToTeam {
        team_coordinate: String,
        team_event_id: String,
        team_pubkey: String,
        is_like: bool,
        /// Optional channel to send back the reaction event ID after signing
        response_tx: Option<EventIdSender>,
    },
    /// Post a team comment (kind:1111 NIP-22).
    PostTeamComment {
        team_coordinate: String,
        team_event_id: String,
        team_pubkey: String,
        content: String,
        parent_comment_id: Option<String>,
        parent_comment_pubkey: Option<String>,
        /// Optional channel to send back the comment event ID after signing
        response_tx: Option<EventIdSender>,
    },
    /// Publish user profile (kind:0 metadata event)
    PublishProfile {
        name: String,
        picture_url: Option<String>,
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
    /// Force reconnection to relays and restart subscriptions
    /// Used by pull-to-refresh to ensure fresh data is fetched
    ForceReconnect {
        /// Optional response channel to signal when reconnection is complete
        response_tx: Option<Sender<Result<(), String>>>,
    },
    /// Start the NIP-46 bunker (remote signer)
    StartBunker {
        response_tx: Sender<Result<String, String>>,
    },
    /// Stop the NIP-46 bunker
    StopBunker {
        response_tx: Sender<Result<(), String>>,
    },
    /// Respond to a pending bunker signing request
    BunkerResponse {
        request_id: String,
        approved: bool,
    },
    /// Get the bunker audit log
    GetBunkerAuditLog {
        response_tx: Sender<Vec<super::bunker::BunkerAuditEntry>>,
    },
    /// Add an auto-approve rule for bunker signing
    AddBunkerAutoApproveRule {
        requester_pubkey: String,
        event_kind: Option<u16>,
    },
    /// Remove an auto-approve rule for bunker signing
    RemoveBunkerAutoApproveRule {
        requester_pubkey: String,
        event_kind: Option<u16>,
    },
    /// Get all bunker auto-approve rules
    GetBunkerAutoApproveRules {
        response_tx: Sender<Vec<super::bunker::BunkerAutoApproveRule>>,
    },
    /// Register APNs device token for push notifications (kind:25000)
    /// Publishes a NIP-44 encrypted event to the backend with device token info.
    RegisterApnsToken {
        device_token: String,
        enable: bool,
        backend_pubkey: String,
        device_id: String,
    },
    /// Delete an agent from a project or globally (kind:24030)
    DeleteAgent {
        /// The hex pubkey of the agent to delete
        agent_pubkey: String,
        /// Project a-tag (`31933:<owner_pubkey>:<d_tag>`).
        /// When `Some`, scope is "project" and the a-tag is included in the event.
        /// When `None`, scope is "global" and no a-tag is included.
        project_a_tag: Option<String>,
        /// Optional reason text (event content)
        reason: Option<String>,
        /// Client identifier for the client tag
        client: Option<String>,
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
    ProjectStatus { json: String },
    /// MCP tools changed (kind:4200 events)
    MCPToolsChanged,
    /// NIP-46 bunker signing request requiring user approval
    BunkerSignRequest {
        request: super::bunker::BunkerSignRequest,
    },
    /// Bookmark list was published (kind:14202) - optimistic update notification
    BookmarkListChanged { bookmarked_ids: Vec<String> },
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
    /// NIP-46 bunker service (remote signer)
    bunker_service: Option<super::bunker::BunkerService>,
    /// Relay URLs to connect to (set via Connect command)
    relay_urls: Vec<String>,
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
            bunker_service: None,
            relay_urls: vec![RELAY_URL.to_string()],
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
                    NostrCommand::Connect {
                        keys,
                        user_pubkey,
                        relay_urls,
                        response_tx,
                    } => {
                        debug_log(&format!(
                            "Worker: Connecting with user {}",
                            &user_pubkey[..8]
                        ));
                        if !relay_urls.is_empty() {
                            self.relay_urls = relay_urls;
                        }
                        let result = rt.block_on(self.handle_connect(keys, user_pubkey));
                        if let Some(tx) = response_tx {
                            let _ = tx.send(result.as_ref().map(|_| ()).map_err(|e| e.to_string()));
                        }
                        if let Err(e) = result {
                            tlog!("ERROR", "Failed to connect: {}", e);
                        }
                    }
                    NostrCommand::PublishThread {
                        project_a_tag,
                        title,
                        content,
                        agent_pubkey,
                        nudge_ids,
                        skill_ids,
                        reference_conversation_id,
                        reference_report_a_tag,
                        fork_message_id,
                        response_tx,
                    } => {
                        debug_log("Worker: Publishing thread");
                        match rt.block_on(self.handle_publish_thread(
                            project_a_tag,
                            title,
                            content,
                            agent_pubkey,
                            nudge_ids,
                            skill_ids,
                            reference_conversation_id,
                            reference_report_a_tag,
                            fork_message_id,
                        )) {
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
                    NostrCommand::PublishMessage {
                        thread_id,
                        project_a_tag,
                        content,
                        agent_pubkey,
                        reply_to,
                        nudge_ids,
                        skill_ids,
                        ask_author_pubkey,
                        response_tx,
                    } => {
                        tlog!("SEND", "Worker received PublishMessage command");
                        match rt.block_on(self.handle_publish_message(
                            thread_id,
                            project_a_tag,
                            content,
                            agent_pubkey,
                            reply_to,
                            nudge_ids,
                            skill_ids,
                            ask_author_pubkey,
                        )) {
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
                    NostrCommand::BootProject {
                        project_a_tag,
                        project_pubkey,
                    } => {
                        debug_log(&format!("Worker: Booting project {}", project_a_tag));
                        if let Err(e) =
                            rt.block_on(self.handle_boot_project(project_a_tag, project_pubkey))
                        {
                            tlog!("ERROR", "Failed to boot project: {}", e);
                        }
                    }
                    NostrCommand::UpdateProjectAgents {
                        project_a_tag,
                        agent_definition_ids,
                        mcp_tool_ids,
                    } => {
                        debug_log(&format!(
                            "Worker: Updating project agents for {}",
                            project_a_tag
                        ));
                        if let Err(e) = rt.block_on(self.handle_update_project_agents(
                            project_a_tag,
                            agent_definition_ids,
                            mcp_tool_ids,
                        )) {
                            tlog!("ERROR", "Failed to update project agents: {}", e);
                        }
                    }
                    NostrCommand::SaveProject {
                        slug,
                        name,
                        description,
                        agent_definition_ids,
                        mcp_tool_ids,
                        client,
                    } => {
                        debug_log(&format!("Worker: Saving project {}", name));
                        if let Err(e) = rt.block_on(self.handle_save_project(
                            slug,
                            name,
                            description,
                            agent_definition_ids,
                            mcp_tool_ids,
                            client,
                        )) {
                            tlog!("ERROR", "Failed to save project: {}", e);
                        }
                    }
                    NostrCommand::UpdateProject {
                        project_a_tag,
                        title,
                        description,
                        repo_url,
                        picture_url,
                        agent_definition_ids,
                        mcp_tool_ids,
                        client,
                    } => {
                        debug_log(&format!("Worker: Updating project {}", project_a_tag));
                        if let Err(e) = rt.block_on(self.handle_update_project(
                            project_a_tag,
                            title,
                            description,
                            repo_url,
                            picture_url,
                            agent_definition_ids,
                            mcp_tool_ids,
                            client,
                        )) {
                            tlog!("ERROR", "Failed to update project: {}", e);
                        }
                    }
                    NostrCommand::DeleteProject {
                        project_a_tag,
                        client,
                    } => {
                        debug_log(&format!(
                            "Worker: Tombstone deleting project {}",
                            project_a_tag
                        ));
                        if let Err(e) =
                            rt.block_on(self.handle_delete_project(project_a_tag, client))
                        {
                            tlog!("ERROR", "Failed to delete project: {}", e);
                        }
                    }
                    NostrCommand::CreateAgentDefinition {
                        name,
                        description,
                        role,
                        instructions,
                        version,
                        source_id,
                        is_fork,
                    } => {
                        debug_log(&format!("Worker: Creating agent definition {}", name));
                        if let Err(e) = rt.block_on(self.handle_create_agent_definition(
                            name,
                            description,
                            role,
                            instructions,
                            version,
                            source_id,
                            is_fork,
                        )) {
                            tlog!("ERROR", "Failed to create agent definition: {}", e);
                        }
                    }
                    NostrCommand::DeleteAgentDefinition { agent_id, client } => {
                        let short_id: String = agent_id.chars().take(8).collect();
                        debug_log(&format!("Worker: Deleting agent definition {}", short_id));
                        if let Err(e) =
                            rt.block_on(self.handle_delete_agent_definition(agent_id, client))
                        {
                            tlog!("ERROR", "Failed to delete agent definition: {}", e);
                        }
                    }
                    NostrCommand::StopOperations {
                        project_a_tag,
                        event_ids,
                        agent_pubkeys,
                    } => {
                        debug_log(&format!(
                            "Worker: Sending stop command for {} events",
                            event_ids.len()
                        ));
                        if let Err(e) = rt.block_on(self.handle_stop_operations(
                            project_a_tag,
                            event_ids,
                            agent_pubkeys,
                        )) {
                            tlog!("ERROR", "Failed to send stop command: {}", e);
                        }
                    }
                    NostrCommand::UpdateAgentConfig {
                        project_a_tag,
                        agent_pubkey,
                        model,
                        tools,
                        tags,
                    } => {
                        debug_log(&format!(
                            "Worker: Updating agent config for {}",
                            &agent_pubkey[..8]
                        ));
                        if let Err(e) = rt.block_on(self.handle_update_agent_config(
                            project_a_tag,
                            agent_pubkey,
                            model,
                            tools,
                            tags,
                        )) {
                            tlog!("ERROR", "Failed to update agent config: {}", e);
                        }
                    }
                    NostrCommand::UpdateGlobalAgentConfig {
                        agent_pubkey,
                        model,
                        tools,
                        tags,
                    } => {
                        debug_log(&format!(
                            "Worker: Updating global agent config for {}",
                            &agent_pubkey[..8]
                        ));
                        if let Err(e) = rt.block_on(self.handle_update_global_agent_config(
                            agent_pubkey,
                            model,
                            tools,
                            tags,
                        )) {
                            tlog!("ERROR", "Failed to update global agent config: {}", e);
                        }
                    }
                    NostrCommand::SubscribeToProjectMessages { project_a_tag } => {
                        debug_log(&format!(
                            "Worker: Subscribing to messages for project {}",
                            &project_a_tag
                        ));
                        if let Err(e) =
                            rt.block_on(self.handle_subscribe_to_project_messages(project_a_tag))
                        {
                            tlog!("ERROR", "Failed to subscribe to project messages: {}", e);
                        }
                    }
                    NostrCommand::SubscribeToProjectMetadata { project_a_tag } => {
                        debug_log(&format!(
                            "Worker: Subscribing to metadata for project {}",
                            &project_a_tag
                        ));
                        if let Err(e) =
                            rt.block_on(self.handle_subscribe_to_project_metadata(project_a_tag))
                        {
                            tlog!("ERROR", "Failed to subscribe to project metadata: {}", e);
                        }
                    }
                    NostrCommand::CreateNudge {
                        title,
                        description,
                        content,
                        hashtags,
                        allow_tools,
                        deny_tools,
                        only_tools,
                    } => {
                        debug_log(&format!("Worker: Creating nudge '{}'", title));
                        if let Err(e) = rt.block_on(self.handle_create_nudge(
                            title,
                            description,
                            content,
                            hashtags,
                            allow_tools,
                            deny_tools,
                            only_tools,
                        )) {
                            tlog!("ERROR", "Failed to create nudge: {}", e);
                        }
                    }
                    NostrCommand::UpdateNudge {
                        original_id,
                        title,
                        description,
                        content,
                        hashtags,
                        allow_tools,
                        deny_tools,
                        only_tools,
                    } => {
                        debug_log(&format!("Worker: Updating nudge '{}'", title));
                        if let Err(e) = rt.block_on(self.handle_update_nudge(
                            original_id,
                            title,
                            description,
                            content,
                            hashtags,
                            allow_tools,
                            deny_tools,
                            only_tools,
                        )) {
                            tlog!("ERROR", "Failed to update nudge: {}", e);
                        }
                    }
                    NostrCommand::DeleteNudge { nudge_id } => {
                        debug_log(&format!("Worker: Deleting nudge {}", &nudge_id[..8]));
                        if let Err(e) = rt.block_on(self.handle_delete_nudge(nudge_id)) {
                            tlog!("ERROR", "Failed to delete nudge: {}", e);
                        }
                    }
                    NostrCommand::PublishBookmarkList { bookmarked_ids } => {
                        debug_log(&format!(
                            "Worker: Publishing bookmark list ({} items)",
                            bookmarked_ids.len()
                        ));
                        if let Err(e) =
                            rt.block_on(self.handle_publish_bookmark_list(bookmarked_ids.clone()))
                        {
                            tlog!("ERROR", "Failed to publish bookmark list: {}", e);
                        }
                        // Notify via DataChange so the FFI callback notifies the UI layer.
                        let _ = self
                            .data_tx
                            .send(DataChange::BookmarkListChanged { bookmarked_ids });
                    }
                    NostrCommand::DeleteAgent {
                        agent_pubkey,
                        project_a_tag,
                        reason,
                        client,
                    } => {
                        let short_pk: String = agent_pubkey.chars().take(8).collect();
                        let scope = if project_a_tag.is_some() {
                            "project"
                        } else {
                            "global"
                        };
                        debug_log(&format!(
                            "Worker: Deleting agent {} (scope: {})",
                            short_pk, scope
                        ));
                        if let Err(e) = rt.block_on(self.handle_delete_agent(
                            agent_pubkey,
                            project_a_tag,
                            reason,
                            client,
                        )) {
                            tlog!("ERROR", "Failed to delete agent: {}", e);
                        }
                    }
                    NostrCommand::ReactToTeam {
                        team_coordinate,
                        team_event_id,
                        team_pubkey,
                        is_like,
                        response_tx,
                    } => {
                        debug_log(&format!(
                            "Worker: Reacting to team {} ({})",
                            &team_event_id[..8.min(team_event_id.len())],
                            if is_like { "like" } else { "unlike" }
                        ));
                        match rt.block_on(self.handle_react_to_team(
                            team_coordinate,
                            team_event_id,
                            team_pubkey,
                            is_like,
                        )) {
                            Ok(event_id) => {
                                if let Some(tx) = response_tx {
                                    let _ = tx.send(event_id);
                                }
                            }
                            Err(e) => tlog!("ERROR", "Failed to react to team: {}", e),
                        }
                    }
                    NostrCommand::PostTeamComment {
                        team_coordinate,
                        team_event_id,
                        team_pubkey,
                        content,
                        parent_comment_id,
                        parent_comment_pubkey,
                        response_tx,
                    } => {
                        debug_log(&format!(
                            "Worker: Posting team comment on {}",
                            &team_event_id[..8.min(team_event_id.len())]
                        ));
                        match rt.block_on(self.handle_post_team_comment(
                            team_coordinate,
                            team_event_id,
                            team_pubkey,
                            content,
                            parent_comment_id,
                            parent_comment_pubkey,
                        )) {
                            Ok(event_id) => {
                                if let Some(tx) = response_tx {
                                    let _ = tx.send(event_id);
                                }
                            }
                            Err(e) => tlog!("ERROR", "Failed to post team comment: {}", e),
                        }
                    }
                    NostrCommand::PublishProfile { name, picture_url } => {
                        debug_log(&format!("Worker: Publishing user profile '{}'", name));
                        if let Err(e) = rt.block_on(self.handle_publish_profile(name, picture_url))
                        {
                            tlog!("ERROR", "Failed to publish profile: {}", e);
                        }
                    }
                    NostrCommand::Disconnect { response_tx } => {
                        debug_log("Worker: Disconnecting");
                        // Stop bunker if running
                        if let Some(mut service) = self.bunker_service.take() {
                            service.stop();
                        }
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
                                relays
                                    .values()
                                    .filter(|r| r.status() == nostr_sdk::RelayStatus::Connected)
                                    .count()
                            } else {
                                0
                            }
                        });
                        let _ = response_tx.send(connected_count);
                    }
                    NostrCommand::ForceReconnect { response_tx } => {
                        debug_log("Worker: Force reconnecting");
                        let result = rt.block_on(self.handle_force_reconnect());
                        if let Err(ref e) = result {
                            tlog!("ERROR", "Failed to force reconnect: {}", e);
                        }
                        if let Some(tx) = response_tx {
                            let _ = tx.send(result.as_ref().map(|_| ()).map_err(|e| e.to_string()));
                        }
                    }
                    NostrCommand::StartBunker { response_tx } => {
                        let result = if let Some(service) = &self.bunker_service {
                            Ok(service.bunker_uri().to_string())
                        } else if let Some(keys) = &self.keys {
                            // Create a channel for bunker signing requests → data_tx
                            let data_tx = self.data_tx.clone();
                            let (req_tx, req_rx) = std::sync::mpsc::channel();

                            // Spawn a forwarding thread: bunker requests → DataChange
                            std::thread::Builder::new()
                                .name("bunker-request-fwd".to_string())
                                .spawn(move || {
                                    while let Ok(request) = req_rx.recv() {
                                        let _ =
                                            data_tx.send(DataChange::BunkerSignRequest { request });
                                    }
                                })
                                .ok();

                            match super::bunker::BunkerService::start(keys.clone(), req_tx) {
                                Ok(service) => {
                                    let uri = service.bunker_uri().to_string();
                                    self.bunker_service = Some(service);
                                    Ok(uri)
                                }
                                Err(e) => Err(e),
                            }
                        } else {
                            Err("Not connected — no keys available".to_string())
                        };
                        let _ = response_tx.send(result);
                    }
                    NostrCommand::StopBunker { response_tx } => {
                        if let Some(mut service) = self.bunker_service.take() {
                            service.stop();
                            let _ = response_tx.send(Ok(()));
                        } else {
                            let _ = response_tx.send(Err("Bunker not running".to_string()));
                        }
                    }
                    NostrCommand::BunkerResponse {
                        request_id,
                        approved,
                    } => {
                        if let Some(ref service) = self.bunker_service {
                            if let Err(e) = service.respond(&request_id, approved) {
                                tlog!("ERROR", "BunkerResponse failed: {}", e);
                            }
                        }
                    }
                    NostrCommand::GetBunkerAuditLog { response_tx } => {
                        let entries = self
                            .bunker_service
                            .as_ref()
                            .map(|s| s.audit_log())
                            .unwrap_or_default();
                        let _ = response_tx.send(entries);
                    }
                    NostrCommand::AddBunkerAutoApproveRule {
                        requester_pubkey,
                        event_kind,
                    } => {
                        if let Some(ref service) = self.bunker_service {
                            service.add_auto_approve_rule(super::bunker::BunkerAutoApproveRule {
                                requester_pubkey,
                                event_kind,
                            });
                        }
                    }
                    NostrCommand::RemoveBunkerAutoApproveRule {
                        requester_pubkey,
                        event_kind,
                    } => {
                        if let Some(ref service) = self.bunker_service {
                            service.remove_auto_approve_rule(&requester_pubkey, event_kind);
                        }
                    }
                    NostrCommand::GetBunkerAutoApproveRules { response_tx } => {
                        let rules = self
                            .bunker_service
                            .as_ref()
                            .map(|s| s.auto_approve_rules())
                            .unwrap_or_default();
                        let _ = response_tx.send(rules);
                    }
                    NostrCommand::RegisterApnsToken {
                        device_token,
                        enable,
                        backend_pubkey,
                        device_id,
                    } => {
                        tlog!("PUSH", "Worker: Registering APNs token (enable={})", enable);
                        if let Err(e) = rt.block_on(self.handle_register_apns_token(
                            device_token,
                            enable,
                            backend_pubkey,
                            device_id,
                        )) {
                            tlog!("ERROR", "Failed to register APNs token: {}", e);
                        }
                    }
                    NostrCommand::Shutdown => {
                        debug_log("Worker: Shutting down");
                        // Stop bunker if running
                        if let Some(mut service) = self.bunker_service.take() {
                            service.stop();
                        }
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
        let ndb_database =
            EphemeralFilteringNdbDatabase::new(nostr_ndb::NdbDatabase::from((*self.ndb).clone()));

        let client = Client::builder()
            .signer(keys.clone())
            .database(ndb_database)
            .build();

        for url in &self.relay_urls {
            client.add_relay(url).await?;
        }

        tlog!("CONN", "Starting relay connect...");
        let connect_start = std::time::Instant::now();
        let connect_result =
            tokio::time::timeout(std::time::Duration::from_secs(10), client.connect()).await;
        let connect_elapsed = connect_start.elapsed();

        match &connect_result {
            Ok(()) => tlog!("CONN", "Connect completed in {:?}", connect_elapsed),
            Err(_) => {
                tlog!("CONN", "Connect TIMED OUT after {:?}", connect_elapsed);
                return Err(anyhow::anyhow!(
                    "Connection timed out after {:?}",
                    connect_elapsed
                ));
            }
        }

        // Verify at least one relay is actually connected using polling loop
        // This handles race conditions where relay status may transition asynchronously
        let verify_start = std::time::Instant::now();
        let verify_timeout = std::time::Duration::from_secs(5);
        let poll_interval = std::time::Duration::from_millis(100);

        loop {
            let relays = client.relays().await;
            let connected_count = relays
                .values()
                .filter(|r| r.status() == nostr_sdk::RelayStatus::Connected)
                .count();

            if connected_count > 0 {
                tlog!(
                    "CONN",
                    "Verified {} relay(s) connected after {:?}",
                    connected_count,
                    verify_start.elapsed()
                );
                break;
            }

            if verify_start.elapsed() >= verify_timeout {
                tlog!(
                    "CONN",
                    "No relays connected after {:?} polling",
                    verify_timeout
                );
                return Err(anyhow::anyhow!(
                    "No relays connected after {:?} verification timeout",
                    verify_timeout
                ));
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
        let client = self
            .client
            .as_ref()
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
        let rt_handle = self
            .rt_handle
            .as_ref()
            .expect("spawn_negentropy_sync called before runtime initialized")
            .clone();
        let negentropy_stats = self.negentropy_stats.clone();
        let cancel_rx = self
            .cancel_tx
            .as_ref()
            .expect("spawn_negentropy_sync called before cancel_tx initialized")
            .subscribe();

        // Mark negentropy sync as enabled
        negentropy_stats.set_enabled(true);

        let subscribed_projects = self.subscribed_projects.clone();
        rt_handle.spawn(async move {
            run_negentropy_sync(
                client,
                ndb,
                pubkey,
                negentropy_stats,
                cancel_rx,
                subscribed_projects,
            )
            .await;
        });
    }

    async fn start_subscriptions(&mut self, user_pubkey: &str) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;

        let pubkey = PublicKey::parse(user_pubkey)?;

        tlog!("CONN", "Starting subscriptions...");
        let sub_start = std::time::Instant::now();

        // 1a. User's projects (kind:31933) - authored by user
        let project_filter_owned = Filter::new()
            .kind(Kind::Custom(KIND_PROJECT_DRAFT))
            .author(pubkey);
        let project_filter_json = serde_json::to_string(&project_filter_owned).ok();
        let output = client.subscribe(project_filter_owned.clone(), None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new(
                "User projects (authored)".to_string(),
                vec![KIND_PROJECT_DRAFT],
                None,
            )
            .with_raw_filter(project_filter_json.unwrap_or_default()),
        );
        tlog!(
            "CONN",
            "Subscribed to projects (kind:{}) - authored by user",
            KIND_PROJECT_DRAFT
        );

        // 1b. Projects where user is a participant (kind:31933) - via p-tag
        let project_filter_participant = Filter::new()
            .kind(Kind::Custom(KIND_PROJECT_DRAFT))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::P), pubkey.to_hex());
        let project_p_filter_json = serde_json::to_string(&project_filter_participant).ok();
        let output = client
            .subscribe(project_filter_participant.clone(), None)
            .await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new(
                "User projects (participant)".to_string(),
                vec![KIND_PROJECT_DRAFT],
                None,
            )
            .with_raw_filter(project_p_filter_json.unwrap_or_default()),
        );
        tlog!(
            "CONN",
            "Subscribed to projects (kind:{}) - p-tagged user",
            KIND_PROJECT_DRAFT
        );

        // 2. Status events (kind:24010, kind:24133) - since 45 seconds ago
        // kind:24010 is the GLOBAL subscription that tells us which projects are online.
        // When we receive these events, we create per-project subscriptions for kind:1, 513, 30023.
        let since_time = Timestamp::now() - 45;
        let project_status_filter = Filter::new()
            .kind(Kind::Custom(KIND_PROJECT_STATUS))
            .custom_tag(
                SingleLetterTag::lowercase(Alphabet::P),
                user_pubkey.to_string(),
            )
            .since(since_time);
        let project_status_json = serde_json::to_string(&project_status_filter).ok();
        let project_output = client
            .subscribe(project_status_filter.clone(), None)
            .await?;
        self.subscription_stats.register(
            project_output.val.to_string(),
            SubscriptionInfo::new(
                "Project status updates".to_string(),
                vec![KIND_PROJECT_STATUS],
                None,
            )
            .with_raw_filter(project_status_json.unwrap_or_default()),
        );

        // Backend uses uppercase P tag for kind:24133
        let agent_status_filter = Filter::new()
            .kind(Kind::Custom(KIND_AGENT_STATUS))
            .custom_tag(
                SingleLetterTag::uppercase(Alphabet::P),
                user_pubkey.to_string(),
            )
            .since(since_time);
        let agent_status_json = serde_json::to_string(&agent_status_filter).ok();
        let agent_output = client.subscribe(agent_status_filter.clone(), None).await?;
        self.subscription_stats.register(
            agent_output.val.to_string(),
            SubscriptionInfo::new(
                "Operations status updates".to_string(),
                vec![KIND_AGENT_STATUS],
                None,
            )
            .with_raw_filter(agent_status_json.unwrap_or_default()),
        );

        tlog!(
            "CONN",
            "Subscribed to status events (kind:{}, kind:{})",
            KIND_PROJECT_STATUS,
            KIND_AGENT_STATUS
        );

        // 2c. User's bookmark list (kind:14202) - authored by current user
        let bookmark_filter = Filter::new()
            .kind(Kind::Custom(KIND_BOOKMARK_LIST))
            .author(pubkey);
        let bookmark_filter_json = serde_json::to_string(&bookmark_filter).ok();
        let output = client.subscribe(bookmark_filter.clone(), None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new(
                "User bookmark list".to_string(),
                vec![KIND_BOOKMARK_LIST],
                None,
            )
            .with_raw_filter(bookmark_filter_json.unwrap_or_default()),
        );
        tlog!(
            "CONN",
            "Subscribed to bookmark list (kind:{}) - authored by user",
            KIND_BOOKMARK_LIST
        );

        // 3. Global definitions/social (kind:34199, 4199, 4200, 4201, 4202, 1111, 7)
        let global_filter = Filter::new().kinds(vec![
            Kind::Custom(KIND_TEAM_PACK),
            Kind::Custom(KIND_AGENT),
            Kind::Custom(KIND_MCP_TOOL),
            Kind::Custom(KIND_NUDGE),
            Kind::Custom(KIND_SKILL),
            Kind::Custom(KIND_COMMENT),
            Kind::Custom(KIND_REACTION),
        ]);
        let global_filter_json = serde_json::to_string(&global_filter).ok();
        let output = client.subscribe(global_filter.clone(), None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new(
                "Global definitions".to_string(),
                vec![
                    KIND_TEAM_PACK,
                    KIND_AGENT,
                    KIND_MCP_TOOL,
                    KIND_NUDGE,
                    KIND_SKILL,
                    KIND_COMMENT,
                    KIND_REACTION,
                ],
                None,
            )
            .with_raw_filter(global_filter_json.unwrap_or_default()),
        );
        tlog!(
            "CONN",
            "Subscribed to global definitions/social (kind:{}, kind:{}, kind:{}, kind:{}, kind:{}, kind:{}, kind:{})",
            KIND_TEAM_PACK,
            KIND_AGENT,
            KIND_MCP_TOOL,
            KIND_NUDGE,
            KIND_SKILL,
            KIND_COMMENT,
            KIND_REACTION
        );

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

        tlog!(
            "CONN",
            "All subscriptions set up in {:?}",
            sub_start.elapsed()
        );

        self.spawn_notification_handler();

        Ok(())
    }

    fn spawn_notification_handler(&self) {
        let client = self
            .client
            .as_ref()
            .expect("spawn_notification_handler called before Connect")
            .clone();
        let ndb = self.ndb.clone();
        let rt_handle = self
            .rt_handle
            .as_ref()
            .expect("spawn_notification_handler called before runtime initialized")
            .clone();
        let event_stats = self.event_stats.clone();
        let subscription_stats = self.subscription_stats.clone();
        let data_tx = self.data_tx.clone();
        let requested_profiles = self.requested_profiles.clone();
        let subscribed_projects = self.subscribed_projects.clone();
        let mut cancel_rx = self
            .cancel_tx
            .as_ref()
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

                                    // Log event reception
                                    let event_id_hex = event.id.to_hex();
                                    tlog!("EVT", "kind:{} id={}", event.kind.as_u16(), &event_id_hex);

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
                                                        let project_name = d_tag.split(':').next_back().unwrap_or(d_tag);
                                                        tlog!("CONN", "New project discovered: {}, subscribed to messages", project_name);
                                                    }
                                                    Ok(false) => {
                                                        // Already subscribed - no action needed
                                                    }
                                                    Err(e) => {
                                                        let project_name = d_tag.split(':').next_back().unwrap_or(d_tag);
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

    fn handle_incoming_event(ndb: &Ndb, event: Event, relay_url: &str) -> Result<()> {
        // Ingest the event into nostrdb with relay metadata
        // UI gets notified via nostrdb SubscriptionStream when events are ready
        ingest_events(ndb, std::slice::from_ref(&event), Some(relay_url))?;

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_thread_event_builder(
        project_a_tag: String,
        title: String,
        content: String,
        agent_pubkey: Option<String>,
        nudge_ids: Vec<String>,
        skill_ids: Vec<String>,
        reference_conversation_id: Option<String>,
        reference_report_a_tag: Option<String>,
        fork_message_id: Option<String>,
    ) -> Result<EventBuilder> {
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

        // Optional report a-tag reference for report discussion threads.
        if let Some(report_a_tag) = reference_report_a_tag {
            let report_coordinate = Self::parse_report_coordinate(&report_a_tag)?;
            event = event.tag(Tag::coordinate(report_coordinate, None));
        }

        // Agent p-tag for routing (required for agent to respond)
        if let Some(agent_pk) = agent_pubkey {
            if let Ok(pk) = PublicKey::parse(&agent_pk) {
                event = event.tag(Tag::public_key(pk));
            }
        }

        // Nudge tags
        for nudge_id in nudge_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("nudge")),
                vec![nudge_id],
            ));
        }

        // Skill tags (custom "skill" tag, same format as nudge tags)
        for skill_id in skill_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("skill")),
                vec![skill_id],
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
            if let Some(conv_id) = reference_conversation_id {
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

        Ok(event)
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_publish_thread(
        &self,
        project_a_tag: String,
        title: String,
        content: String,
        agent_pubkey: Option<String>,
        nudge_ids: Vec<String>,
        skill_ids: Vec<String>,
        reference_conversation_id: Option<String>,
        reference_report_a_tag: Option<String>,
        fork_message_id: Option<String>,
    ) -> Result<String> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;

        let event = Self::build_thread_event_builder(
            project_a_tag,
            title,
            content,
            agent_pubkey,
            nudge_ids,
            skill_ids,
            reference_conversation_id,
            reference_report_a_tag,
            fork_message_id,
        )?;

        // Build and sign the event
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;
        let signed_event = event.sign_with_keys(keys)?;
        let event_id = signed_event.id.to_hex();

        // Ingest locally into nostrdb so it appears immediately
        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;
        // UI gets notified via nostrdb SubscriptionStream when data is ready

        // Send to relay with timeout (don't block forever on degraded connections)
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => debug_log(&format!("Published thread: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send thread to relay: {}", e),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending thread to relay (event was saved locally)"
            ),
        }

        Ok(event_id)
    }

    fn parse_report_coordinate(report_a_tag: &str) -> Result<Coordinate> {
        if !report_a_tag.starts_with("30023:") {
            return Err(anyhow::anyhow!(
                "Invalid report coordinate kind (expected 30023): {}",
                report_a_tag
            ));
        }

        Coordinate::parse(report_a_tag)
            .map_err(|e| anyhow::anyhow!("Invalid report coordinate: {}", e))
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_publish_message(
        &self,
        thread_id: String,
        project_a_tag: String,
        content: String,
        agent_pubkey: Option<String>,
        reply_to: Option<String>,
        nudge_ids: Vec<String>,
        skill_ids: Vec<String>,
        ask_author_pubkey: Option<String>,
    ) -> Result<String> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;

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

        // Nudge tags
        for nudge_id in nudge_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("nudge")),
                vec![nudge_id],
            ));
        }

        // Skill tags (custom "skill" tag, same format as nudge tags)
        for skill_id in skill_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("skill")),
                vec![skill_id],
            ));
        }

        // Build and sign the event
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;
        tlog!("SEND", "Signing message...");
        let sign_start = std::time::Instant::now();
        let signed_event = event.sign_with_keys(keys)?;
        let event_id = signed_event.id.to_hex();
        tlog!("SEND", "Signed in {:?}", sign_start.elapsed());

        // Ingest locally into nostrdb - UI gets notified via SubscriptionStream
        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;
        tlog!("SEND", "Ingested locally, now sending to relay...");

        // Send to relay with timeout (don't block forever on degraded connections)
        let send_start = std::time::Instant::now();
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => tlog!(
                "SEND",
                "Published message in {:?}: {}",
                send_start.elapsed(),
                output.id()
            ),
            Ok(Err(e)) => tlog!("SEND", "Failed after {:?}: {}", send_start.elapsed(), e),
            Err(_) => tlog!(
                "SEND",
                "TIMEOUT after {:?} (event saved locally)",
                send_start.elapsed()
            ),
        }

        Ok(event_id)
    }

    async fn handle_boot_project(
        &self,
        project_a_tag: String,
        project_pubkey: Option<String>,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

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
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => debug_log(&format!("Sent boot request: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send boot request to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending boot request to relay"),
        }

        Ok(())
    }

    /// Publish a kind:25000 push notification registration event.
    ///
    /// The payload is NIP-44 encrypted to the backend's public key so only the
    /// backend can decrypt the device token.  The event carries:
    /// - `p` tag pointing to the backend pubkey (for relay routing)
    /// - Encrypted content containing `{ "notifications": { "enable": <bool>,
    ///   "apn_token": "<hex>", "device_id": "<uuid>" } }`
    async fn handle_register_apns_token(
        &self,
        device_token: String,
        enable: bool,
        backend_pubkey: String,
        device_id: String,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client - not connected"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys - not logged in"))?;

        // Parse the backend public key
        let backend_pk = PublicKey::parse(&backend_pubkey).map_err(|e| {
            anyhow::anyhow!(
                "Invalid backend pubkey '{}': {}",
                &backend_pubkey[..8.min(backend_pubkey.len())],
                e
            )
        })?;

        // Build JSON payload
        let payload = serde_json::json!({
            "notifications": {
                "enable": enable,
                "apn_token": device_token,
                "device_id": device_id
            }
        });
        let payload_str = serde_json::to_string(&payload)?;

        // NIP-44 encrypt to the backend's pubkey
        let encrypted = nip44::encrypt(
            keys.secret_key(),
            &backend_pk,
            &payload_str,
            nip44::Version::default(),
        )
        .map_err(|e| anyhow::anyhow!("NIP-44 encryption failed: {}", e))?;

        // Build kind:25000 event with an explicit lowercase p-tag for backend routing.
        let event = EventBuilder::new(Kind::Custom(25000), &encrypted).tag(Tag::custom(
            TagKind::Custom(std::borrow::Cow::Borrowed("p")),
            vec![backend_pk.to_hex()],
        ));

        let signed_event = event.sign_with_keys(keys)?;

        // Publish to relay (no local ingest - this is a service registration, not UI data)
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => tlog!(
                "PUSH",
                "APNs registration published: {} enable={}",
                output.id(),
                enable
            ),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send APNs registration to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending APNs registration to relay"),
        }

        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn build_project_event_builder(
        d_tag: String,
        title: String,
        description: String,
        repo_url: Option<String>,
        picture_url: Option<String>,
        participants: &[String],
        agent_definition_ids: &[String],
        mcp_tool_ids: &[String],
        client_name: String,
        is_deleted: bool,
    ) -> EventBuilder {
        let mut event = EventBuilder::new(Kind::Custom(31933), &description)
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("d")),
                vec![d_tag],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec![title],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec![client_name],
            ));

        if let Some(repo) = repo_url.filter(|s| !s.trim().is_empty()) {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("repo")),
                vec![repo],
            ));
        }

        if let Some(picture) = picture_url.filter(|s| !s.trim().is_empty()) {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("picture")),
                vec![picture],
            ));
        }

        if is_deleted {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deleted")),
                Vec::<String>::new(),
            ));
        }

        for participant in participants {
            if let Ok(pk) = PublicKey::parse(participant) {
                event = event.tag(Tag::public_key(pk));
            }
        }

        for agent_id in agent_definition_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("agent")),
                vec![agent_id.clone()],
            ));
        }

        for tool_id in mcp_tool_ids {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("mcp")),
                vec![tool_id.clone()],
            ));
        }

        event
    }

    async fn publish_project_event(
        &self,
        client: &Client,
        keys: &Keys,
        event: EventBuilder,
        action_label: &str,
    ) -> Result<()> {
        let signed_event = event.sign_with_keys(keys)?;
        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;

        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => debug_log(&format!("{}: {}", action_label, output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send project event to relay: {}", e),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending project event to relay (saved locally)"
            ),
        }

        Ok(())
    }

    async fn handle_update_project_agents(
        &self,
        project_a_tag: String,
        agent_definition_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Get the existing project from nostrdb
        let projects = crate::store::get_projects(&self.ndb)?;
        let project = projects
            .iter()
            .find(|p| p.a_tag() == project_a_tag)
            .ok_or_else(|| anyhow::anyhow!("Project not found: {}", project_a_tag))?;

        let event = Self::build_project_event_builder(
            project.id.clone(),
            project.title.clone(),
            project.description.clone().unwrap_or_default(),
            project.repo_url.clone(),
            project.picture_url.clone(),
            &project.participants,
            &agent_definition_ids,
            &mcp_tool_ids,
            "tenex-tui".to_string(),
            false,
        );

        self.publish_project_event(client, keys, event, "Updated project agents")
            .await
    }

    async fn handle_save_project(
        &self,
        slug: Option<String>,
        name: String,
        description: String,
        agent_definition_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
        client_tag: Option<String>,
    ) -> Result<()> {
        use crate::slug::slug_from_name;

        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Use provided slug or generate d-tag from name using consistent normalization
        let d_tag = slug.unwrap_or_else(|| slug_from_name(&name));

        // Determine client identifier
        let client_name = client_tag.unwrap_or_else(|| "tenex".to_string());

        let event = Self::build_project_event_builder(
            d_tag,
            name,
            description,
            None,
            None,
            &[],
            &agent_definition_ids,
            &mcp_tool_ids,
            client_name,
            false,
        );

        self.publish_project_event(client, keys, event, "Saved project")
            .await
    }

    #[allow(clippy::too_many_arguments)]
    async fn handle_update_project(
        &self,
        project_a_tag: String,
        title: String,
        description: String,
        repo_url: Option<String>,
        picture_url: Option<String>,
        agent_definition_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
        client_tag: Option<String>,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        let projects = crate::store::get_projects(&self.ndb)?;
        let project = projects
            .iter()
            .find(|p| p.a_tag() == project_a_tag)
            .ok_or_else(|| anyhow::anyhow!("Project not found: {}", project_a_tag))?;

        let client_name = client_tag.unwrap_or_else(|| "tenex-ios".to_string());
        let event = Self::build_project_event_builder(
            project.id.clone(),
            title,
            description,
            repo_url,
            picture_url,
            &project.participants,
            &agent_definition_ids,
            &mcp_tool_ids,
            client_name,
            false,
        );

        self.publish_project_event(client, keys, event, "Updated project")
            .await
    }

    async fn handle_delete_project(
        &self,
        project_a_tag: String,
        client_tag: Option<String>,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        let projects = crate::store::get_projects(&self.ndb)?;
        let project = projects
            .iter()
            .find(|p| p.a_tag() == project_a_tag)
            .ok_or_else(|| anyhow::anyhow!("Project not found: {}", project_a_tag))?;

        let client_name = client_tag.unwrap_or_else(|| "tenex-ios".to_string());
        let event = Self::build_project_event_builder(
            project.id.clone(),
            project.title.clone(),
            project.description.clone().unwrap_or_default(),
            project.repo_url.clone(),
            project.picture_url.clone(),
            &project.participants,
            &project.agent_definition_ids,
            &project.mcp_tool_ids,
            client_name,
            true,
        );

        self.publish_project_event(client, keys, event, "Deleted project")
            .await
    }

    #[allow(clippy::too_many_arguments)]
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
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

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
        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => debug_log(&format!(
                "Created agent definition '{}': {}",
                name,
                output.id()
            )),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send agent definition to relay: {}", e),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending agent definition to relay (saved locally)"
            ),
        }

        Ok(())
    }

    /// Delete an agent definition (kind:5 deletion event)
    async fn handle_delete_agent_definition(
        &self,
        agent_id: String,
        client_tag: Option<String>,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        let event_id =
            EventId::parse(&agent_id).map_err(|e| anyhow::anyhow!("Invalid event ID: {}", e))?;

        let client_name = client_tag.unwrap_or_else(|| "tenex-ios".to_string());

        let deletion_request = EventDeletionRequest::new().id(event_id);
        let event = EventBuilder::delete(deletion_request).tag(Tag::custom(
            TagKind::Custom(std::borrow::Cow::Borrowed("client")),
            vec![client_name],
        ));

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb
        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => {
                let short_id: String = agent_id.chars().take(8).collect();
                debug_log(&format!(
                    "Deleted agent definition {}: deletion event {}",
                    short_id,
                    output.id()
                ))
            }
            Ok(Err(e)) => tlog!("ERROR", "Failed to send deletion event to relay: {}", e),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending deletion event to relay (saved locally)"
            ),
        }

        Ok(())
    }

    /// Publish a kind:24030 agent deletion event.
    ///
    /// Schema:
    /// ```json
    /// {
    ///   "kind": 24030,
    ///   "tags": [
    ///     ["p", "<agent_pubkey_hex>"],
    ///     ["a", "31933:<project_author>:<d_tag>"],  // present iff scope == "project"
    ///     ["r", "project" | "global"]
    ///   ],
    ///   "content": "<optional reason>"
    /// }
    /// ```
    async fn handle_delete_agent(
        &self,
        agent_pubkey: String,
        project_a_tag: Option<String>,
        reason: Option<String>,
        client_tag: Option<String>,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        let pk = PublicKey::parse(&agent_pubkey)
            .map_err(|e| anyhow::anyhow!("Invalid agent pubkey: {}", e))?;

        let client_name = client_tag.unwrap_or_else(|| "tenex-ios".to_string());
        let scope = if project_a_tag.is_some() {
            "project"
        } else {
            "global"
        };

        // Build kind:24030 event
        let mut event = EventBuilder::new(Kind::Custom(24030), reason.as_deref().unwrap_or(""))
            .tag(Tag::public_key(pk))
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::R)),
                vec![scope.to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec![client_name],
            ));

        // Add project a-tag when scope is "project"
        if let Some(ref a_tag) = project_a_tag {
            let coordinate = Coordinate::parse(a_tag)
                .map_err(|e| anyhow::anyhow!("Invalid project coordinate '{}': {}", a_tag, e))?;
            event = event.tag(Tag::coordinate(coordinate, None));
        }

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb
        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => {
                let short_pk: String = agent_pubkey.chars().take(8).collect();
                debug_log(&format!(
                    "Sent kind:24030 delete for agent {} ({}): {}",
                    short_pk,
                    scope,
                    output.id()
                ))
            }
            Ok(Err(e)) => tlog!(
                "ERROR",
                "Failed to send agent deletion event to relay: {}",
                e
            ),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending agent deletion event to relay (saved locally)"
            ),
        }

        Ok(())
    }

    async fn handle_stop_operations(
        &self,
        project_a_tag: String,
        event_ids: Vec<String>,
        agent_pubkeys: Vec<String>,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

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
            client.send_event(&signed_event),
        )
        .await
        {
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
        tags: Vec<String>,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Parse project coordinate for a-tag
        let coordinate = Coordinate::parse(&project_a_tag)
            .map_err(|e| anyhow::anyhow!("Invalid project coordinate: {}", e))?;

        // Build kind:24020 agent config update event with project a-tag
        let base =
            EventBuilder::new(Kind::Custom(24020), "").tag(Tag::coordinate(coordinate, None));
        let event = build_agent_config_event(base, &agent_pubkey, model, &tools, &tags);
        let signed_event = event.sign_with_keys(keys)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => debug_log(&format!("Sent agent config update: {}", output.id())),
            Ok(Err(e)) => tlog!(
                "ERROR",
                "Failed to send agent config update to relay: {}",
                e
            ),
            Err(_) => tlog!("ERROR", "Timeout sending agent config update to relay"),
        }

        Ok(())
    }

    /// Send a global kind:24020 agent config event (no a-tag, agent-scoped only).
    async fn handle_update_global_agent_config(
        &self,
        agent_pubkey: String,
        model: Option<String>,
        tools: Vec<String>,
        tags: Vec<String>,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Build kind:24020 global agent config event (no a-tag)
        let base = EventBuilder::new(Kind::Custom(24020), "");
        let event = build_agent_config_event(base, &agent_pubkey, model, &tools, &tags);
        let signed_event = event.sign_with_keys(keys)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => {
                debug_log(&format!("Sent global agent config update: {}", output.id()))
            }
            Ok(Err(e)) => tlog!(
                "ERROR",
                "Failed to send global agent config update to relay: {}",
                e
            ),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending global agent config update to relay"
            ),
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
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;

        // Use atomic check+insert to prevent duplicate subscriptions
        // This is critical for iOS where refresh() and notification handler can race
        let is_new = self
            .subscribed_projects
            .write()
            .await
            .insert(project_a_tag.clone());
        if !is_new {
            // Already subscribed (likely by notification handler)
            let project_name = project_a_tag.split(':').nth(2).unwrap_or("unknown");
            tlog!(
                "CONN",
                "Skipping duplicate subscription for project: {}",
                project_name
            );
            return Ok(());
        }

        let project_name = project_a_tag.split(':').nth(2).unwrap_or("unknown");
        tlog!("CONN", "Adding subscriptions for project: {}", project_name);

        // Use the shared helper for consistent subscription behavior
        match subscribe_project_filters(client, &self.ndb, &self.subscription_stats, &project_a_tag)
            .await
        {
            Ok(()) => Ok(()),
            Err(e) => {
                // Subscription failed - remove from set so we can retry later
                self.subscribed_projects
                    .write()
                    .await
                    .remove(&project_a_tag);
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

    async fn handle_publish_profile(
        &self,
        name: String,
        picture_url: Option<String>,
    ) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        let mut metadata = Metadata::new().name(name);
        if let Some(url) = picture_url {
            if let Ok(parsed) = url.parse() {
                metadata = metadata.picture(parsed);
            }
        }

        let event = EventBuilder::metadata(&metadata).sign_with_keys(keys)?;

        match tokio::time::timeout(std::time::Duration::from_secs(5), client.send_event(&event))
            .await
        {
            Ok(Ok(output)) => debug_log(&format!("Published profile: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to publish profile to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout publishing profile to relay"),
        }

        Ok(())
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

    /// Force reconnect to relays and restart all subscriptions.
    /// Used by pull-to-refresh to ensure fresh data is fetched from relays.
    async fn handle_force_reconnect(&mut self) -> Result<()> {
        // Save keys and user_pubkey before disconnect clears them
        let keys = match self.keys.clone() {
            Some(k) => k,
            None => return Err(anyhow::anyhow!("No keys - not logged in")),
        };
        let user_pubkey = match self.user_pubkey.clone() {
            Some(p) => p,
            None => return Err(anyhow::anyhow!("No user_pubkey - not logged in")),
        };

        tlog!("CONN", "Force reconnect: disconnecting...");

        // Disconnect (this clears client, keys, user_pubkey, and cancels background tasks)
        self.handle_disconnect().await?;

        tlog!("CONN", "Force reconnect: reconnecting...");

        // Reconnect with the same credentials
        self.handle_connect(keys, user_pubkey).await?;

        tlog!("CONN", "Force reconnect: completed");
        Ok(())
    }

    /// Create a new nudge (kind:4201)
    #[allow(clippy::too_many_arguments)]
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
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

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
        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => debug_log(&format!("Created nudge '{}': {}", title, output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send nudge to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending nudge to relay (saved locally)"),
        }

        Ok(())
    }

    /// Update an existing nudge (create new event with updated content)
    /// Since kind:4201 is not a replaceable event, we create a new event
    /// and add a reference to the original
    #[allow(clippy::too_many_arguments)]
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
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

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
        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => debug_log(&format!("Updated nudge '{}': {}", title, output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send updated nudge to relay: {}", e),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending updated nudge to relay (saved locally)"
            ),
        }

        Ok(())
    }

    /// Publish the user's bookmark list as a kind:14202 replaceable event.
    ///
    /// Each bookmarked nudge/skill ID becomes an `["e", "<id>"]` tag.
    /// Publishing replaces any previous bookmark list from the same author.
    async fn handle_publish_bookmark_list(&self, bookmarked_ids: Vec<String>) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Build kind:14202 event with empty content and ["e", "<id>"] tags
        let mut event = EventBuilder::new(Kind::Custom(14202), "");

        for id in &bookmarked_ids {
            let event_id = EventId::parse(id)
                .map_err(|e| anyhow::anyhow!("Invalid bookmark event ID '{}': {}", id, e))?;
            event = event.tag(Tag::event(event_id));
        }

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally so the nostrdb subscription stream fires and AppDataStore updates
        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;

        // Publish to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => debug_log(&format!(
                "Published bookmark list ({} items): {}",
                bookmarked_ids.len(),
                output.id()
            )),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send bookmark list to relay: {}", e),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending bookmark list to relay (saved locally)"
            ),
        }

        Ok(())
    }

    /// Delete a nudge (kind:5 deletion event)
    async fn handle_delete_nudge(&self, nudge_id: String) -> Result<()> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Parse the nudge event ID
        let event_id =
            EventId::parse(&nudge_id).map_err(|e| anyhow::anyhow!("Invalid event ID: {}", e))?;

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
        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(output)) => debug_log(&format!(
                "Deleted nudge {}: deletion event {}",
                &nudge_id[..8],
                output.id()
            )),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send deletion event to relay: {}", e),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending deletion event to relay (saved locally)"
            ),
        }

        Ok(())
    }

    /// React to a team pack via NIP-25 kind:7 with dual anchors (`a` + `e`).
    async fn handle_react_to_team(
        &self,
        team_coordinate: String,
        team_event_id: String,
        team_pubkey: String,
        is_like: bool,
    ) -> Result<String> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        let reaction_content = if is_like { "+" } else { "-" };
        let mut event = EventBuilder::new(Kind::Custom(KIND_REACTION), reaction_content)
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("k")),
                vec![KIND_TEAM_PACK.to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("a")),
                vec![team_coordinate.clone()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("e")),
                vec![team_event_id.clone()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-ios".to_string()],
            ));

        if let Ok(pk) = PublicKey::parse(&team_pubkey) {
            event = event.tag(Tag::public_key(pk));
        }

        let signed_event = event.sign_with_keys(keys)?;
        let event_id = signed_event.id.to_hex();

        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;

        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tlog!("ERROR", "Failed to send team reaction to relay: {}", e),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending team reaction to relay (saved locally)"
            ),
        }

        Ok(event_id)
    }

    /// Post a team comment via NIP-22 kind:1111 with dual anchors and optional reply.
    async fn handle_post_team_comment(
        &self,
        team_coordinate: String,
        team_event_id: String,
        team_pubkey: String,
        content: String,
        parent_comment_id: Option<String>,
        parent_comment_pubkey: Option<String>,
    ) -> Result<String> {
        let client = self
            .client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self
            .keys
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("No keys"))?;

        let mut event = EventBuilder::new(Kind::Custom(KIND_COMMENT), &content)
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("K")),
                vec![KIND_TEAM_PACK.to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("k")),
                vec![KIND_TEAM_PACK.to_string()],
            ))
            // Dual root context anchors for best interoperability.
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("A")),
                vec![team_coordinate.clone()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("E")),
                vec![team_event_id.clone()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("a")),
                vec![team_coordinate.clone(), "".to_string(), "root".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("e")),
                vec![team_event_id.clone(), "".to_string(), "root".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("client")),
                vec!["tenex-ios".to_string()],
            ));

        if let Ok(pk) = PublicKey::parse(&team_pubkey) {
            event = event.tag(Tag::public_key(pk));
        }

        if let Some(parent_id) = parent_comment_id {
            event = event
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("E")),
                    vec![parent_id.clone()],
                ))
                .tag(Tag::custom(
                    TagKind::Custom(std::borrow::Cow::Borrowed("e")),
                    vec![parent_id, "".to_string(), "reply".to_string()],
                ));
        }

        if let Some(parent_pubkey) = parent_comment_pubkey {
            if let Ok(pk) = PublicKey::parse(&parent_pubkey) {
                event = event.tag(Tag::public_key(pk));
            }
        }

        let signed_event = event.sign_with_keys(keys)?;
        let event_id = signed_event.id.to_hex();

        ingest_events(&self.ndb, std::slice::from_ref(&signed_event), None)?;

        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(&signed_event),
        )
        .await
        {
            Ok(Ok(_)) => {}
            Ok(Err(e)) => tlog!("ERROR", "Failed to send team comment to relay: {}", e),
            Err(_) => tlog!(
                "ERROR",
                "Timeout sending team comment to relay (saved locally)"
            ),
        }

        Ok(event_id)
    }
}

/// Attach the common tags to a kind:24020 agent config `EventBuilder`.
///
/// Adds (in order):
/// - NIP-89 `client` tag
/// - `p` tag for `agent_pubkey` (skipped if the pubkey cannot be parsed)
/// - `model` tag when `model` is `Some`
/// - one `tool` tag per entry in `tools`
/// - one bare marker tag per entry in `tags` (e.g. `"pm"`)
fn build_agent_config_event(
    mut event: EventBuilder,
    agent_pubkey: &str,
    model: Option<String>,
    tools: &[String],
    tags: &[String],
) -> EventBuilder {
    // NIP-89 client tag
    event = event.tag(Tag::custom(
        TagKind::Custom(std::borrow::Cow::Borrowed("client")),
        vec!["tenex-tui".to_string()],
    ));

    // p-tag for the agent being configured
    if let Ok(pk) = PublicKey::parse(agent_pubkey) {
        event = event.tag(Tag::public_key(pk));
    }

    // Model tag (optional)
    if let Some(m) = model {
        event = event.tag(Tag::custom(
            TagKind::Custom(std::borrow::Cow::Borrowed("model")),
            vec![m],
        ));
    }

    // Tool tags (exhaustive list — empty means no tools)
    for tool in tools {
        event = event.tag(Tag::custom(
            TagKind::Custom(std::borrow::Cow::Borrowed("tool")),
            vec![tool.clone()],
        ));
    }

    // Marker tags (e.g. ["pm"])
    for tag in tags {
        event = event.tag(Tag::custom(
            TagKind::Custom(std::borrow::Cow::Owned(tag.clone())),
            Vec::<String>::new(),
        ));
    }

    event
}

/// Run negentropy sync loop with adaptive timing
/// Syncs project-scoped kinds: 31933 (projects), 513 (conversation metadata),
/// 1 (messages), and 30023 (long-form content).
/// Also syncs global definitions/social kinds to back cold-start queries.
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
            tlog!(
                "SYNC",
                "Negentropy sync received cancellation signal, exiting"
            );
            break;
        }

        stats.set_in_progress(true);
        let total_new =
            sync_all_filters(&client, &ndb, &user_pubkey, &stats, &subscribed_projects).await;
        stats.record_cycle_complete();
        stats.set_in_progress(false);

        if total_new == 0 {
            interval_secs = (interval_secs * 2).min(MAX_INTERVAL);
            tlog!("SYNC", "No gaps found. Next sync in {}s", interval_secs);
        } else {
            interval_secs = 60;
            tlog!(
                "SYNC",
                "Found {} new events. Next sync in {}s",
                total_new,
                interval_secs
            );
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
    let project_filter = Filter::new().kind(Kind::Custom(31933)).author(*user_pubkey);
    total_new += sync_filter(client, project_filter, "31933-authored", stats).await;

    // Projects where user is a participant (kind 31933) - via p-tag
    let project_p_filter = Filter::new().kind(Kind::Custom(31933)).custom_tag(
        SingleLetterTag::lowercase(Alphabet::P),
        user_pubkey_hex.clone(),
    );
    total_new += sync_filter(client, project_p_filter, "31933-p-tagged", stats).await;

    // Team packs (kind 34199)
    let team_filter = Filter::new().kind(Kind::Custom(KIND_TEAM_PACK));
    total_new += sync_filter(client, team_filter, "34199", stats).await;

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

    // Skills (kind 4202) - global, like nudges and agent definitions
    let skill_filter = Filter::new().kind(Kind::Custom(4202));
    total_new += sync_filter(client, skill_filter, "4202", stats).await;

    // Comments (kind 1111)
    let comment_filter = Filter::new().kind(Kind::Custom(KIND_COMMENT));
    total_new += sync_filter(client, comment_filter, "1111", stats).await;

    // Reactions (kind 7)
    let reaction_filter = Filter::new().kind(Kind::Custom(KIND_REACTION));
    total_new += sync_filter(client, reaction_filter, "7", stats).await;

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

/// Perform negentropy sync for a single filter with automatic pagination
/// Returns the number of new events received across all pages
async fn sync_filter(
    client: &Client,
    mut filter: Filter,
    label: &str,
    stats: &SharedNegentropySyncStats,
) -> u64 {
    const LIMIT: usize = 10_000; // Match NdbDatabase's MAX_RESULTS
    const MAX_PAGES: usize = 50; // Safety limit to prevent infinite loops

    let opts = SyncOptions::default();
    let mut total_count = 0u64;
    let mut page = 0;

    // Add limit to filter
    filter = filter.limit(LIMIT);

    loop {
        page += 1;

        if page > MAX_PAGES {
            tlog!(
                "SYNC",
                "kind:{} reached max pages ({}), stopping",
                label,
                MAX_PAGES
            );
            break;
        }

        match client.sync(filter.clone(), &opts).await {
            Ok(output) => {
                let event_ids = output.val.received;
                let count = event_ids.len();

                if count > 0 {
                    tlog!(
                        "SYNC",
                        "kind:{} page {} -> {} new events",
                        label,
                        page,
                        count
                    );
                    total_count += count as u64;

                    // If we got a full page, there might be more
                    if count >= LIMIT {
                        // Query database to find the oldest event from this batch
                        // to set .until() for next page
                        if let Ok(events) = client.database().query(filter.clone()).await {
                            if let Some(oldest) = events.iter().map(|e| e.created_at).min() {
                                // Next page: get events older than this
                                filter = filter.until(oldest - 1);
                                tlog!(
                                    "SYNC_DEBUG",
                                    "kind:{} -> fetching next page (events before {})",
                                    label,
                                    oldest
                                );
                                continue; // Fetch next page
                            }
                        }
                    }
                }

                // No more events or couldn't paginate - we're done
                if page == 1 && count == 0 {
                    tlog!(
                        "SYNC_DEBUG",
                        "kind:{} -> 0 new (DB already had them)",
                        label
                    );
                } else if page > 1 {
                    tlog!(
                        "SYNC",
                        "kind:{} COMPLETE -> {} total events across {} pages",
                        label,
                        total_count,
                        page
                    );
                }

                break; // Done paginating
            }
            Err(e) => {
                let err_str = format!("{}", e);
                let is_unsupported =
                    err_str.contains("not supported") || err_str.contains("NEG-ERR");

                if !is_unsupported {
                    tlog!("SYNC", "kind:{} page {} failed: {}", label, page, e);
                }

                stats.record_failure(label, &err_str, is_unsupported);
                break;
            }
        }
    }

    // Record total success
    if total_count > 0 {
        stats.record_success(label, total_count);
    }

    total_count
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_ephemeral_filtering_database_rejects_ephemeral_events() {
        let dir = tempdir().unwrap();
        let db = crate::store::Database::new(dir.path()).unwrap();
        let ndb_database =
            EphemeralFilteringNdbDatabase::new(nostr_ndb::NdbDatabase::from((*db.ndb).clone()));

        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(KIND_PROJECT_STATUS), "{}")
            .sign_with_keys(&keys)
            .unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let status = rt.block_on(ndb_database.save_event(&event)).unwrap();

        assert_eq!(status, SaveEventStatus::Rejected(RejectedReason::Ephemeral));

        let txn = Transaction::new(&db.ndb).unwrap();
        let filter = nostrdb::Filter::new()
            .kinds([KIND_PROJECT_STATUS as u64])
            .build();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert!(
            results.is_empty(),
            "ephemeral events must not be cached in nostrdb"
        );
    }

    #[test]
    fn test_ephemeral_filtering_database_still_saves_non_ephemeral_events() {
        let dir = tempdir().unwrap();
        let db = crate::store::Database::new(dir.path()).unwrap();
        let ndb_database =
            EphemeralFilteringNdbDatabase::new(nostr_ndb::NdbDatabase::from((*db.ndb).clone()));

        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::TextNote, "hello world")
            .sign_with_keys(&keys)
            .unwrap();

        let rt = tokio::runtime::Runtime::new().unwrap();
        let status = rt.block_on(ndb_database.save_event(&event)).unwrap();
        assert_eq!(status, SaveEventStatus::Success);

        let filter = nostrdb::Filter::new()
            .kinds([KIND_TEXT_NOTE as u64])
            .build();
        let found = crate::store::events::wait_for_event_processing(&db.ndb, filter.clone(), 5000);
        assert!(
            found,
            "non-ephemeral event was not processed within timeout"
        );

        let txn = Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 10).unwrap();
        assert_eq!(results.len(), 1, "non-ephemeral events should still cache");
    }

    #[test]
    fn test_coordinate_parse() {
        let a_tag =
            "31933:09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7:DDD-83ayt6";
        let result = Coordinate::parse(a_tag);
        println!("Parse result: {:?}", result);
        assert!(result.is_ok(), "Failed to parse: {:?}", result.err());
    }

    #[test]
    fn test_parse_report_coordinate_accepts_valid_report_coordinate() {
        let report_a_tag =
            "30023:09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7:weekly-report";
        let result = NostrWorker::parse_report_coordinate(report_a_tag);
        assert!(result.is_ok(), "expected valid report coordinate");
    }

    #[test]
    fn test_parse_report_coordinate_rejects_non_report_coordinate() {
        let project_a_tag =
            "31933:09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7:project";
        let result = NostrWorker::parse_report_coordinate(project_a_tag);
        assert!(result.is_err(), "expected non-report coordinate to be rejected");
    }

    #[test]
    fn test_build_thread_event_includes_project_and_report_a_tags() {
        let project_a_tag =
            "31933:09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7:project";
        let report_a_tag =
            "30023:09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7:weekly-report";

        let keys = Keys::generate();
        let event = NostrWorker::build_thread_event_builder(
            project_a_tag.to_string(),
            "Report discussion".to_string(),
            "Let's discuss the report".to_string(),
            None,
            vec![],
            vec![],
            None,
            Some(report_a_tag.to_string()),
            None,
        )
        .unwrap()
        .sign_with_keys(&keys)
        .unwrap();

        let event_json = serde_json::to_value(event).unwrap();
        let tags = event_json
            .get("tags")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let a_tags: Vec<String> = tags
            .iter()
            .filter_map(|tag| {
                let arr = tag.as_array()?;
                if arr.first().and_then(|v| v.as_str()) == Some("a") {
                    arr.get(1).and_then(|v| v.as_str()).map(str::to_string)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(a_tags.len(), 2, "expected exactly two a-tags");
        assert!(a_tags.iter().any(|tag| tag == project_a_tag));
        assert!(a_tags.iter().any(|tag| tag == report_a_tag));
    }

    #[test]
    fn test_build_thread_event_rejects_invalid_report_coordinate() {
        let project_a_tag =
            "31933:09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7:project";
        let invalid_report_a_tag =
            "30023:09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7";

        let result = NostrWorker::build_thread_event_builder(
            project_a_tag.to_string(),
            "Report discussion".to_string(),
            "Let's discuss the report".to_string(),
            None,
            vec![],
            vec![],
            None,
            Some(invalid_report_a_tag.to_string()),
            None,
        );

        assert!(result.is_err(), "expected invalid report a-tag to fail");
    }

    #[test]
    fn test_delete_project_publishes_kind_31933_with_d_and_deleted_tag() {
        let keys = Keys::generate();
        let d_tag = "project-delete-check".to_string();
        let event = NostrWorker::build_project_event_builder(
            d_tag.clone(),
            "Project Delete Check".to_string(),
            "description".to_string(),
            Some("https://example.com/repo".to_string()),
            Some("https://example.com/pic.png".to_string()),
            &[],
            &[],
            &[],
            "tenex-ios".to_string(),
            true,
        )
        .sign_with_keys(&keys)
        .unwrap();

        let event_json = serde_json::to_value(event).unwrap();
        assert_eq!(
            event_json.get("kind").and_then(|v| v.as_u64()),
            Some(31933),
            "Delete must publish kind:31933"
        );

        let tags = event_json
            .get("tags")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let has_d_tag = tags.iter().any(|tag| {
            let Some(arr) = tag.as_array() else {
                return false;
            };
            arr.first().and_then(|v| v.as_str()) == Some("d")
                && arr.get(1).and_then(|v| v.as_str()) == Some(d_tag.as_str())
        });
        assert!(has_d_tag, "Delete event must preserve original d-tag");

        let has_deleted_tag = tags.iter().any(|tag| {
            let Some(arr) = tag.as_array() else {
                return false;
            };
            arr.first().and_then(|v| v.as_str()) == Some("deleted")
        });
        assert!(has_deleted_tag, "Delete event must include deleted tag");
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
        assert!(
            has_tag_type(&tags, "only-tool"),
            "Should have only-tool tags"
        );

        // Exclusive mode: should NOT emit allow-tool or deny-tool tags
        assert!(
            !has_tag_type(&tags, "allow-tool"),
            "Should NOT have allow-tool tags in exclusive mode"
        );
        assert!(
            !has_tag_type(&tags, "deny-tool"),
            "Should NOT have deny-tool tags in exclusive mode"
        );

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
        assert!(
            has_tag_type(&tags, "allow-tool"),
            "Should have allow-tool tags"
        );
        assert!(
            has_tag_type(&tags, "deny-tool"),
            "Should have deny-tool tags"
        );

        // Additive mode: should NOT emit only-tool tags
        assert!(
            !has_tag_type(&tags, "only-tool"),
            "Should NOT have only-tool tags in additive mode"
        );

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
        assert!(
            !has_tag_type(&tags, "allow-tool"),
            "Exclusive mode must never emit allow-tool"
        );
        assert!(
            !has_tag_type(&tags, "deny-tool"),
            "Exclusive mode must never emit deny-tool"
        );

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
        assert!(
            !has_tag_type(&tags, "only-tool"),
            "Additive mode must never emit only-tool"
        );
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
