use std::collections::HashSet;
use std::io::Write;
use std::path::PathBuf;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, OnceLock, RwLock};
use std::time::Instant;

use anyhow::Result;
use nostr_sdk::prelude::*;
use nostrdb::Ndb;
use tokio::runtime::Runtime;
use tokio::sync::mpsc as tokio_mpsc;

use crate::constants::RELAY_URL;
use crate::stats::{SharedEventStats, SharedSubscriptionStats, SubscriptionInfo};
use crate::store::{get_projects, ingest_events};
use crate::streaming::{LocalStreamChunk, SocketStreamClient};

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

/// Response channel for commands that need to return data (like event IDs)
pub type EventIdSender = std::sync::mpsc::SyncSender<String>;

pub enum NostrCommand {
    Connect { keys: Keys, user_pubkey: String },
    PublishThread {
        project_a_tag: String,
        title: String,
        content: String,
        agent_pubkey: Option<String>,
        branch: Option<String>,
        nudge_ids: Vec<String>,
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
    },
    /// Create a new project (kind:31933)
    CreateProject {
        name: String,
        description: String,
        agent_ids: Vec<String>,
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
    /// Pubkeys for which we've already requested kind:0 profiles
    requested_profiles: Arc<RwLock<HashSet<String>>>,
}

impl NostrWorker {
    pub fn new(
        ndb: Arc<Ndb>,
        data_tx: Sender<DataChange>,
        command_rx: Receiver<NostrCommand>,
        event_stats: SharedEventStats,
        subscription_stats: SharedSubscriptionStats,
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
            requested_profiles: Arc::new(RwLock::new(HashSet::new())),
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
                    NostrCommand::Connect { keys, user_pubkey } => {
                        debug_log(&format!("Worker: Connecting with user {}", &user_pubkey[..8]));
                        if let Err(e) = rt.block_on(self.handle_connect(keys, user_pubkey)) {
                            tlog!("ERROR", "Failed to connect: {}", e);
                        }
                    }
                    NostrCommand::PublishThread { project_a_tag, title, content, agent_pubkey, branch, nudge_ids, response_tx } => {
                        debug_log("Worker: Publishing thread");
                        match rt.block_on(self.handle_publish_thread(project_a_tag, title, content, agent_pubkey, branch, nudge_ids)) {
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
                    NostrCommand::UpdateProjectAgents { project_a_tag, agent_ids } => {
                        debug_log(&format!("Worker: Updating project agents for {}", project_a_tag));
                        if let Err(e) = rt.block_on(self.handle_update_project_agents(project_a_tag, agent_ids)) {
                            tlog!("ERROR", "Failed to update project agents: {}", e);
                        }
                    }
                    NostrCommand::CreateProject { name, description, agent_ids } => {
                        debug_log(&format!("Worker: Creating project {}", name));
                        if let Err(e) = rt.block_on(self.handle_create_project(name, description, agent_ids)) {
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
                    NostrCommand::Shutdown => {
                        debug_log("Worker: Shutting down");
                        let _ = rt.block_on(self.handle_disconnect());
                        break;
                    }
                }
            }
        }

        debug_log("Nostr worker thread stopped");
    }

    async fn handle_connect(&mut self, keys: Keys, user_pubkey: String) -> Result<()> {
        let client = Client::new(keys.clone());

        client.add_relay(RELAY_URL).await?;

        tlog!("CONN", "Starting relay connect...");
        let connect_start = std::time::Instant::now();
        let connect_result = tokio::time::timeout(std::time::Duration::from_secs(10), client.connect()).await;
        let connect_elapsed = connect_start.elapsed();

        match &connect_result {
            Ok(()) => tlog!("CONN", "Connect completed in {:?}", connect_elapsed),
            Err(_) => tlog!("CONN", "Connect TIMED OUT after {:?}", connect_elapsed),
        }

        self.client = Some(client);
        self.keys = Some(keys);
        self.user_pubkey = Some(user_pubkey.clone());

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

        rt_handle.spawn(async move {
            run_negentropy_sync(client, ndb, pubkey).await;
        });
    }

    async fn start_subscriptions(&mut self, user_pubkey: &str) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        let pubkey = PublicKey::parse(user_pubkey)?;

        tlog!("CONN", "Starting subscriptions...");
        let sub_start = std::time::Instant::now();

        // 1. User's projects (kind:31933)
        let project_filter = Filter::new().kind(Kind::Custom(31933)).author(pubkey);
        let output = client.subscribe(vec![project_filter], None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new("User projects".to_string(), vec![31933], None),
        );
        tlog!("CONN", "Subscribed to projects (kind:31933)");

        // 2. Status events (kind:24010, kind:24133) - since 45 seconds ago
        let since_time = Timestamp::now() - 45;
        let status_filter = Filter::new()
            .kind(Kind::Custom(24010))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::P), vec![user_pubkey.to_string()])
            .since(since_time);
        let operations_status_filter = Filter::new()
            .kind(Kind::Custom(24133))
            .custom_tag(SingleLetterTag::uppercase(Alphabet::P), vec![user_pubkey.to_string()])
            .since(since_time);
        let output = client.subscribe(vec![status_filter, operations_status_filter], None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new("Status updates".to_string(), vec![24010, 24133], None),
        );
        tlog!("CONN", "Subscribed to status events (kind:24010, kind:24133)");

        // 3. Agent definitions (kind:4199)
        let agent_filter = Filter::new().kind(Kind::Custom(4199));
        let output = client.subscribe(vec![agent_filter], None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new("Agent definitions".to_string(), vec![4199], None),
        );
        tlog!("CONN", "Subscribed to agent definitions (kind:4199)");

        // 4. Nudges (kind:4201)
        let nudge_filter = Filter::new().kind(Kind::Custom(4201));
        let output = client.subscribe(vec![nudge_filter], None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new("Nudges".to_string(), vec![4201], None),
        );
        tlog!("CONN", "Subscribed to nudges (kind:4201)");

        // 5. Agent lessons (kind:4129)
        let lesson_filter = Filter::new().kind(Kind::Custom(4129));
        let output = client.subscribe(vec![lesson_filter], None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new("Agent lessons".to_string(), vec![4129], None),
        );
        tlog!("CONN", "Subscribed to agent lessons (kind:4129)");

        // 6. Per-project subscriptions (kind:513 metadata, kind:1 messages)
        let project_atags: Vec<String> = get_projects(&self.ndb)
            .unwrap_or_default()
            .iter()
            .map(|p| p.a_tag())
            .collect();

        if !project_atags.is_empty() {
            tlog!("CONN", "Setting up subscriptions for {} existing projects", project_atags.len());

            for project_a_tag in &project_atags {
                // Extract project name for description
                let project_name = project_a_tag.split(':').nth(2).unwrap_or("unknown");

                // Metadata subscription (kind:513)
                let metadata_filter = Filter::new()
                    .kind(Kind::Custom(513))
                    .custom_tag(SingleLetterTag::lowercase(Alphabet::A), vec![project_a_tag.clone()]);
                let output = client.subscribe(vec![metadata_filter], None).await?;
                self.subscription_stats.register(
                    output.val.to_string(),
                    SubscriptionInfo::new(
                        format!("{} metadata", project_name),
                        vec![513],
                        Some(project_a_tag.clone()),
                    ),
                );

                // Messages subscription (kind:1)
                let message_filter = Filter::new()
                    .kind(Kind::from(1))
                    .custom_tag(SingleLetterTag::lowercase(Alphabet::A), vec![project_a_tag.clone()]);
                let output = client.subscribe(vec![message_filter], None).await?;
                self.subscription_stats.register(
                    output.val.to_string(),
                    SubscriptionInfo::new(
                        format!("{} messages", project_name),
                        vec![1],
                        Some(project_a_tag.clone()),
                    ),
                );

                // Long-form content subscription (kind:30023)
                let longform_filter = Filter::new()
                    .kind(Kind::Custom(30023))
                    .custom_tag(SingleLetterTag::lowercase(Alphabet::A), vec![project_a_tag.clone()]);
                let output = client.subscribe(vec![longform_filter], None).await?;
                self.subscription_stats.register(
                    output.val.to_string(),
                    SubscriptionInfo::new(
                        format!("{} reports", project_name),
                        vec![30023],
                        Some(project_a_tag.clone()),
                    ),
                );
            }
            tlog!("CONN", "Subscribed to kind:513, kind:1, and kind:30023 for {} projects", project_atags.len());
        } else {
            tlog!("CONN", "No projects found, skipping kind:513 and kind:1 subscriptions");
        }

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

        rt_handle.spawn(async move {
            let mut notifications = client.notifications();
            let mut first_event = true;
            let handler_start = std::time::Instant::now();
            tlog!("CONN", "Notification handler started, waiting for events...");

            loop {
                if let Ok(notification) = notifications.recv().await {
                    if let RelayPoolNotification::Event { relay_url, subscription_id, event } = notification {
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
                        if kind == 24010 || kind == 24133 {
                            if let Ok(json) = serde_json::to_string(&*event) {
                                let _ = data_tx.send(DataChange::ProjectStatus { json });
                            }
                        } else {
                            // All other events go through nostrdb
                            if let Err(e) = Self::handle_incoming_event(&ndb, *event.clone(), relay_url.as_str()) {
                                tlog!("ERROR", "Failed to handle event: {}", e);
                            }

                            // For kind:1 messages, request author's profile if we haven't already
                            if kind == 1 {
                                let author_hex = event.pubkey.to_hex();
                                let should_request = {
                                    let profiles = requested_profiles.read().unwrap();
                                    !profiles.contains(&author_hex)
                                };
                                if should_request {
                                    {
                                        let mut profiles = requested_profiles.write().unwrap();
                                        profiles.insert(author_hex.clone());
                                    }
                                    // Subscribe to kind:0 for this author
                                    let profile_filter = Filter::new()
                                        .kind(Kind::Metadata)
                                        .author(event.pubkey);
                                    if let Err(e) = client.subscribe(vec![profile_filter], None).await {
                                        tlog!("ERROR", "Failed to subscribe to profile for {}: {}", &author_hex[..8], e);
                                    } else {
                                        debug_log(&format!("Subscribed to profile for author {}", &author_hex[..8]));
                                    }
                                }
                            }
                        }
                    }
                }
            }
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
    ) -> Result<String> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        // Parse project coordinate for proper a-tag
        let coordinate = Coordinate::parse(&project_a_tag)
            .map_err(|e| anyhow::anyhow!("Invalid project coordinate: {}", e))?;

        let mut event = EventBuilder::new(Kind::from(1), &content)
            // Project reference (a tag) - required
            .tag(Tag::coordinate(coordinate))
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
            client.send_event(signed_event)
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
            .tag(Tag::coordinate(coordinate))
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
            client.send_event(signed_event)
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
            .tag(Tag::coordinate(coordinate))
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
            client.send_event(signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Sent boot request: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send boot request to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending boot request to relay"),
        }

        Ok(())
    }

    async fn handle_update_project_agents(&self, project_a_tag: String, agent_ids: Vec<String>) -> Result<()> {
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

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(signed_event)
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

        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;

        // Send to relay with timeout
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(signed_event)
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
            client.send_event(signed_event)
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
            .tag(Tag::coordinate(coordinate))
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
            client.send_event(signed_event)
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
            .tag(Tag::coordinate(coordinate))
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
            client.send_event(signed_event)
        ).await {
            Ok(Ok(output)) => debug_log(&format!("Sent agent config update: {}", output.id())),
            Ok(Err(e)) => tlog!("ERROR", "Failed to send agent config update to relay: {}", e),
            Err(_) => tlog!("ERROR", "Timeout sending agent config update to relay"),
        }

        Ok(())
    }

    async fn handle_subscribe_to_project_messages(&self, project_a_tag: String) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        tlog!("CONN", "Adding subscription for kind:1 and kind:30023 with a-tag: {}", project_a_tag);

        // Extract project name for description
        let project_name = project_a_tag.split(':').nth(2).unwrap_or("unknown");

        // Messages (kind:1)
        let message_filter = Filter::new()
            .kind(Kind::from(1))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::A), vec![project_a_tag.clone()]);

        // Long-form content (kind:30023)
        let longform_filter = Filter::new()
            .kind(Kind::Custom(30023))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::A), vec![project_a_tag.clone()]);

        let output = client.subscribe(vec![message_filter, longform_filter], None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new(
                format!("{} msgs+reports", project_name),
                vec![1, 30023],
                Some(project_a_tag),
            ),
        );

        Ok(())
    }

    async fn handle_subscribe_to_project_metadata(&self, project_a_tag: String) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        tlog!("CONN", "Adding subscription for kind:513 metadata with a-tag: {}", project_a_tag);

        // Extract project name for description
        let project_name = project_a_tag.split(':').nth(2).unwrap_or("unknown");

        let filter = Filter::new()
            .kind(Kind::Custom(513))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::A), vec![project_a_tag.clone()]);

        let output = client.subscribe(vec![filter], None).await?;
        self.subscription_stats.register(
            output.val.to_string(),
            SubscriptionInfo::new(
                format!("{} metadata", project_name),
                vec![513],
                Some(project_a_tag),
            ),
        );

        Ok(())
    }

    async fn handle_disconnect(&mut self) -> Result<()> {
        if let Some(client) = &self.client {
            client.disconnect().await?;
        }
        self.client = None;
        self.keys = None;
        self.user_pubkey = None;
        Ok(())
    }
}

/// Run negentropy sync loop with adaptive timing
/// Syncs non-ephemeral kinds: 31933, 4199, 513, 4129, 4201, and kind:1 messages
async fn run_negentropy_sync(client: Client, ndb: Arc<Ndb>, user_pubkey: PublicKey) {
    use std::time::Duration;

    let mut interval_secs: u64 = 60;
    const MAX_INTERVAL: u64 = 900; // 15 minutes cap

    tlog!("SYNC", "Starting initial negentropy sync...");

    loop {
        let total_new = sync_all_filters(&client, &ndb, &user_pubkey).await;

        if total_new == 0 {
            interval_secs = (interval_secs * 2).min(MAX_INTERVAL);
            tlog!("SYNC", "No gaps found. Next sync in {}s", interval_secs);
        } else {
            interval_secs = 60;
            tlog!("SYNC", "Found {} new events. Next sync in {}s", total_new, interval_secs);
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}

/// Sync all non-ephemeral kinds using negentropy reconciliation
async fn sync_all_filters(client: &Client, ndb: &Ndb, user_pubkey: &PublicKey) -> usize {
    let mut total_new = 0;

    // User's projects (kind 31933)
    let project_filter = Filter::new()
        .kind(Kind::Custom(31933))
        .author(*user_pubkey);
    total_new += sync_filter(client, project_filter, "31933").await;

    // Agent definitions (kind 4199)
    let agent_filter = Filter::new().kind(Kind::Custom(4199));
    total_new += sync_filter(client, agent_filter, "4199").await;

    // Conversation metadata (kind 513)
    let metadata_filter = Filter::new().kind(Kind::Custom(513));
    total_new += sync_filter(client, metadata_filter, "513").await;

    // Agent lessons (kind 4129)
    let lesson_filter = Filter::new().kind(Kind::Custom(4129));
    total_new += sync_filter(client, lesson_filter, "4129").await;

    // Nudges (kind 4201)
    let nudge_filter = Filter::new().kind(Kind::Custom(4201));
    total_new += sync_filter(client, nudge_filter, "4201").await;

    // Messages (kind 1) and long-form content (kind 30023) with project a-tags - batched in groups of 4
    if let Ok(projects) = get_projects(ndb) {
        let atags: Vec<String> = projects.iter().map(|p| p.a_tag()).collect();
        if !atags.is_empty() {
            // Batch in groups of 4 (same as subscriptions)
            for chunk in atags.chunks(4) {
                let msg_filter = Filter::new()
                    .kind(Kind::from(1))
                    .custom_tag(SingleLetterTag::lowercase(Alphabet::A), chunk.to_vec());
                total_new += sync_filter(client, msg_filter, "1").await;

                let longform_filter = Filter::new()
                    .kind(Kind::Custom(30023))
                    .custom_tag(SingleLetterTag::lowercase(Alphabet::A), chunk.to_vec());
                total_new += sync_filter(client, longform_filter, "30023").await;
            }
        }
    }

    total_new
}

/// Perform negentropy sync for a single filter
/// Returns the number of new events received
async fn sync_filter(client: &Client, filter: Filter, label: &str) -> usize {
    let opts = SyncOptions::default();

    match client.sync(filter, &opts).await {
        Ok(output) => {
            // output.val is Reconciliation, output.success is HashSet<RelayUrl>
            let count = output.val.received.len();

            if count > 0 {
                tlog!("SYNC", "kind:{} -> {} new events", label, count);
            }

            count
        }
        Err(e) => {
            // Only log if it's not a "not supported" error (common for relays without negentropy)
            let err_str = format!("{}", e);
            if !err_str.contains("not supported") && !err_str.contains("NEG-ERR") {
                tlog!("SYNC", "kind:{} failed: {}", label, e);
            }
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

    // Streaming delta parsing tests removed.
}
