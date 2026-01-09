use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use anyhow::Result;
use nostr_sdk::prelude::*;
use nostrdb::Ndb;
use tokio::runtime::Runtime;
use tokio::sync::mpsc as tokio_mpsc;
use tracing::{debug, error, info};

use crate::store::ingest_events;
use crate::streaming::{LocalStreamChunk, SocketStreamClient};

const RELAY_URL: &str = "wss://tenex.chat";

fn debug_log(msg: &str) {
    if std::env::var("TENEX_DEBUG").map(|v| v == "1").unwrap_or(false) {
        eprintln!("[WORKER] {}", msg);
    }
}

pub enum NostrCommand {
    Connect { keys: Keys, user_pubkey: String },
    Sync,
    PublishThread {
        project_a_tag: String,
        title: String,
        content: String,
        agent_pubkey: Option<String>,
        branch: Option<String>,
    },
    PublishMessage {
        thread_id: String,
        project_a_tag: String,
        content: String,
        agent_pubkey: Option<String>,
        reply_to: Option<String>,
        branch: Option<String>,
    },
    #[allow(dead_code)]
    BootProject {
        project_a_tag: String,
        project_pubkey: Option<String>,
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
}

pub struct NostrWorker {
    client: Option<Client>,
    keys: Option<Keys>,
    user_pubkey: Option<String>,
    ndb: Arc<Ndb>,
    data_tx: Sender<DataChange>,
    command_rx: Receiver<NostrCommand>,
    subscribed_projects: HashSet<String>,
    needed_profiles: HashSet<String>,
    rt_handle: Option<tokio::runtime::Handle>,
}

impl NostrWorker {
    pub fn new(ndb: Arc<Ndb>, data_tx: Sender<DataChange>, command_rx: Receiver<NostrCommand>) -> Self {
        Self {
            client: None,
            keys: None,
            user_pubkey: None,
            ndb,
            data_tx,
            command_rx,
            subscribed_projects: HashSet::new(),
            needed_profiles: HashSet::new(),
            rt_handle: None,
        }
    }

    pub fn run(mut self) {
        let rt = Runtime::new().expect("Failed to create runtime");
        self.rt_handle = Some(rt.handle().clone());
        info!("Nostr worker thread started");

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
                        info!("Worker: Connecting with user {}", &user_pubkey[..8]);
                        if let Err(e) = rt.block_on(self.handle_connect(keys, user_pubkey)) {
                            error!("Failed to connect: {}", e);
                        }
                    }
                    NostrCommand::Sync => {
                        info!("Worker: Syncing data");
                        if let Err(e) = rt.block_on(self.handle_sync()) {
                            error!("Failed to sync: {}", e);
                        }
                    }
                    NostrCommand::PublishThread { project_a_tag, title, content, agent_pubkey, branch } => {
                        info!("Worker: Publishing thread");
                        if let Err(e) = rt.block_on(self.handle_publish_thread(project_a_tag, title, content, agent_pubkey, branch)) {
                            error!("Failed to publish thread: {}", e);
                        }
                    }
                    NostrCommand::PublishMessage { thread_id, project_a_tag, content, agent_pubkey, reply_to, branch } => {
                        info!("Worker: Publishing message");
                        if let Err(e) = rt.block_on(self.handle_publish_message(thread_id, project_a_tag, content, agent_pubkey, reply_to, branch)) {
                            error!("Failed to publish message: {}", e);
                        }
                    }
                    NostrCommand::BootProject { project_a_tag, project_pubkey } => {
                        info!("Worker: Booting project {}", project_a_tag);
                        if let Err(e) = rt.block_on(self.handle_boot_project(project_a_tag, project_pubkey)) {
                            error!("Failed to boot project: {}", e);
                        }
                    }
                    NostrCommand::Shutdown => {
                        info!("Worker: Shutting down");
                        let _ = rt.block_on(self.handle_disconnect());
                        break;
                    }
                }
            }
        }

        info!("Nostr worker thread stopped");
    }

    async fn handle_connect(&mut self, keys: Keys, user_pubkey: String) -> Result<()> {
        let client = Client::new(keys.clone());

        client.add_relay(RELAY_URL).await?;

        tokio::time::timeout(std::time::Duration::from_secs(10), client.connect())
            .await
            .ok();

        self.client = Some(client);
        self.keys = Some(keys);
        self.user_pubkey = Some(user_pubkey.clone());

        self.start_subscriptions(&user_pubkey).await?;

        Ok(())
    }

    async fn start_subscriptions(&mut self, user_pubkey: &str) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        let pubkey = PublicKey::parse(user_pubkey)?;

        // User's projects
        let project_filter = Filter::new().kind(Kind::Custom(31933)).author(pubkey);

        // Events mentioning the user
        let mention_filter = Filter::new().pubkey(pubkey);

        // Agent definitions (kind 4199)
        let agent_filter = Filter::new().kind(Kind::Custom(4199));

        // Project status (kind 24010) - subscribe to both p-tagged events AND all 24010 events
        // We filter client-side by project coordinate since 24010 events may not have p-tags
        let status_filter = Filter::new()
            .kind(Kind::Custom(24010))
            .pubkey(pubkey);

        // Also subscribe to ALL 24010 events (we'll filter by project coord client-side)
        let all_status_filter = Filter::new()
            .kind(Kind::Custom(24010));

        // Conversation metadata (kind 513) - provides titles and summaries for threads
        let metadata_filter = Filter::new().kind(Kind::Custom(513));

        // Agent lessons (kind 4129) - learning insights from agents
        let lesson_filter = Filter::new().kind(Kind::Custom(4129));

        info!("Starting persistent subscriptions");

        let subscription_id = client
            .subscribe(
                vec![
                    project_filter,
                    mention_filter,
                    agent_filter,
                    status_filter,
                    all_status_filter,
                    metadata_filter,
                    lesson_filter,
                ],
                None,
            )
            .await?;

        debug!("Subscription started: {:?}", subscription_id);

        self.spawn_notification_handler();

        Ok(())
    }

    fn spawn_notification_handler(&self) {
        let client = self.client.as_ref().unwrap().clone();
        let ndb = self.ndb.clone();
        let rt_handle = self.rt_handle.as_ref().unwrap().clone();

        rt_handle.spawn(async move {
            let mut notifications = client.notifications();

            loop {
                if let Ok(notification) = notifications.recv().await {
                    if let RelayPoolNotification::Event { relay_url, event, .. } = notification {
                        debug!("Received event: kind={} id={} from {}", event.kind, event.id, relay_url);

                        if let Err(e) = Self::handle_incoming_event(&ndb, *event, relay_url.as_str()) {
                            error!("Failed to handle event: {}", e);
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

    async fn handle_sync(&mut self) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let user_pubkey = self.user_pubkey.as_ref().ok_or_else(|| anyhow::anyhow!("No user pubkey"))?;

        info!("Starting sync for user {}", &user_pubkey[..8]);
        debug_log(&format!("Starting sync for user {}", user_pubkey));

        let pubkey = PublicKey::parse(user_pubkey)?;

        // Fetch projects
        let project_filter = Filter::new().kind(Kind::Custom(31933)).author(pubkey);
        debug_log(&format!("Fetching projects (kind 31933) for author {}", user_pubkey));

        let events = client
            .fetch_events(vec![project_filter], std::time::Duration::from_secs(10))
            .await?;

        let events_vec: Vec<Event> = events.into_iter().collect();
        debug_log(&format!("Fetched {} project events", events_vec.len()));
        for event in &events_vec {
            debug_log(&format!("  Project event: id={}, created_at={}", &event.id.to_hex()[..16], event.created_at.as_u64()));
        }
        ingest_events(&self.ndb, &events_vec, Some(RELAY_URL))?;
        debug_log("Ingested project events into nostrdb");
        // UI gets notified via nostrdb SubscriptionStream when data is ready

        // Fetch project status events (kind 24010) for user's projects
        // First try with p-tag filter, then also fetch by project coordinates
        let status_filter = Filter::new()
            .kind(Kind::Custom(24010))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::P), [user_pubkey]);

        info!("Fetching 24010 events with p-tag filter for user {}", &user_pubkey[..16]);
        let status_events = client
            .fetch_events(vec![status_filter], std::time::Duration::from_secs(10))
            .await?;

        let mut status_events_vec: Vec<Event> = status_events.into_iter().collect();
        info!("Fetched {} 24010 events with p-tag filter", status_events_vec.len());

        // Also fetch 24010 events by project a-tags (in case they don't have p-tags)
        let projects = crate::store::get_projects(&self.ndb)?;
        if !projects.is_empty() {
            let project_coords: Vec<String> = projects.iter().map(|p| p.a_tag()).collect();
            info!("Also fetching 24010 events for {} project coordinates", project_coords.len());

            let coord_filter = Filter::new()
                .kind(Kind::Custom(24010))
                .custom_tag(SingleLetterTag::lowercase(Alphabet::A), project_coords);

            let coord_events = client
                .fetch_events(vec![coord_filter], std::time::Duration::from_secs(10))
                .await?;

            let coord_events_vec: Vec<Event> = coord_events.into_iter().collect();
            info!("Fetched {} 24010 events by a-tag filter", coord_events_vec.len());

            // Deduplicate by event ID
            let existing_ids: std::collections::HashSet<_> = status_events_vec.iter().map(|e| e.id).collect();
            for event in coord_events_vec {
                if !existing_ids.contains(&event.id) {
                    status_events_vec.push(event);
                }
            }
        }

        if !status_events_vec.is_empty() {
            info!("Processing {} total 24010 events", status_events_vec.len());
            ingest_events(&self.ndb, &status_events_vec, Some(RELAY_URL))?;

            // UI gets notified via SubscriptionStream when events are ready
        } else {
            info!("No 24010 events found during sync");
        }

        // Get projects from nostrdb to subscribe to their content
        let projects = crate::store::get_projects(&self.ndb)?;

        for project in &projects {
            let project_a_tag = project.a_tag();

            if !self.subscribed_projects.contains(&project_a_tag) {
                self.subscribe_to_project_content(client, &project_a_tag).await?;
                self.subscribed_projects.insert(project_a_tag.clone());
            }

            self.needed_profiles.insert(project.pubkey.clone());
            for p in &project.participants {
                self.needed_profiles.insert(p.clone());
            }
        }

        if !self.needed_profiles.is_empty() {
            self.fetch_profiles().await?;
        }

        info!("Sync complete");

        Ok(())
    }

    async fn subscribe_to_project_content(&self, client: &Client, project_a_tag: &str) -> Result<()> {
        // Fetch all kind:1 events for this project (both threads and messages)
        let kind1_filter = Filter::new()
            .kind(Kind::from(1))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::A), [project_a_tag]);

        let kind1_events = client
            .fetch_events(vec![kind1_filter.clone()], std::time::Duration::from_secs(10))
            .await?;

        let kind1_events_vec: Vec<Event> = kind1_events.iter().cloned().collect();
        ingest_events(&self.ndb, &kind1_events_vec, Some(RELAY_URL))?;
        // UI gets notified via nostrdb SubscriptionStream when data is ready

        // Extract thread IDs (kind:1 events without e-tags)
        let thread_ids: Vec<EventId> = kind1_events
            .iter()
            .filter(|e| !e.tags.iter().any(|t| t.as_slice().first().map(|s| s == "e").unwrap_or(false)))
            .map(|e| e.id)
            .collect();

        if !thread_ids.is_empty() {
            // Convert thread IDs to hex strings for tag filtering
            let thread_id_hexes: Vec<String> = thread_ids.iter().map(|id| id.to_hex()).collect();

            // Fetch conversation metadata (kind 513) for these threads
            // Kind 513 uses lowercase "e" tag to reference threads
            let metadata_filter = Filter::new()
                .kind(Kind::Custom(513))
                .custom_tag(SingleLetterTag::lowercase(Alphabet::E), thread_id_hexes.clone());

            let metadata_events = client
                .fetch_events(vec![metadata_filter], std::time::Duration::from_secs(10))
                .await?;

            let metadata_events_vec: Vec<Event> = metadata_events.into_iter().collect();
            ingest_events(&self.ndb, &metadata_events_vec, Some(RELAY_URL))?;
            // UI gets notified via nostrdb SubscriptionStream when data is ready
        }

        // Subscribe for real-time updates on kind:1 events for this project
        client.subscribe(vec![kind1_filter], None).await?;

        Ok(())
    }

    async fn fetch_profiles(&mut self) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let pubkeys: Vec<String> = self.needed_profiles.drain().collect();

        let pks: Vec<PublicKey> = pubkeys.iter().filter_map(|p| PublicKey::parse(p).ok()).collect();

        if pks.is_empty() {
            return Ok(());
        }

        let filter = Filter::new().kind(Kind::Metadata).authors(pks);

        let events = client.fetch_events(vec![filter], std::time::Duration::from_secs(10)).await?;

        let events_vec: Vec<Event> = events.into_iter().collect();
        ingest_events(&self.ndb, &events_vec, Some(RELAY_URL))?;
        // UI gets notified via SubscriptionStream when events are ready

        Ok(())
    }

    async fn handle_publish_thread(
        &self,
        project_a_tag: String,
        title: String,
        content: String,
        agent_pubkey: Option<String>,
        branch: Option<String>,
    ) -> Result<()> {
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

        // Build and sign the event
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;
        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb so it appears immediately
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;
        // UI gets notified via nostrdb SubscriptionStream when data is ready

        // Send to relay with timeout (don't block forever on degraded connections)
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(signed_event)
        ).await {
            Ok(Ok(output)) => info!("Published thread: {}", output.id()),
            Ok(Err(e)) => error!("Failed to send thread to relay: {}", e),
            Err(_) => error!("Timeout sending thread to relay (event was saved locally)"),
        }

        Ok(())
    }

    async fn handle_publish_message(
        &self,
        thread_id: String,
        project_a_tag: String,
        content: String,
        agent_pubkey: Option<String>,
        reply_to: Option<String>,
        branch: Option<String>,
    ) -> Result<()> {
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
            .tag(Tag::coordinate(coordinate));

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

        // Branch tag
        if let Some(br) = branch {
            event = event.tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("branch")),
                vec![br],
            ));
        }

        // Build and sign the event
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;
        let signed_event = event.sign_with_keys(keys)?;

        // Ingest locally into nostrdb - UI gets notified via SubscriptionStream
        ingest_events(&self.ndb, &[signed_event.clone()], None)?;

        // Send to relay with timeout (don't block forever on degraded connections)
        match tokio::time::timeout(
            std::time::Duration::from_secs(5),
            client.send_event(signed_event)
        ).await {
            Ok(Ok(output)) => info!("Published message: {}", output.id()),
            Ok(Err(e)) => error!("Failed to send message to relay: {}", e),
            Err(_) => error!("Timeout sending message to relay (event was saved locally)"),
        }

        Ok(())
    }

    async fn handle_boot_project(&self, project_a_tag: String, project_pubkey: Option<String>) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let keys = self.keys.as_ref().ok_or_else(|| anyhow::anyhow!("No keys"))?;

        // Parse project coordinate for proper a-tag
        let coordinate = Coordinate::parse(&project_a_tag)
            .map_err(|e| anyhow::anyhow!("Invalid project coordinate: {}", e))?;

        // Kind 24000 boot request with a-tag pointing to project
        let mut event = EventBuilder::new(Kind::Custom(24000), "")
            .tag(Tag::coordinate(coordinate));

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
            Ok(Ok(output)) => info!("Sent boot request: {}", output.id()),
            Ok(Err(e)) => error!("Failed to send boot request to relay: {}", e),
            Err(_) => error!("Timeout sending boot request to relay"),
        }

        Ok(())
    }

    async fn handle_disconnect(&mut self) -> Result<()> {
        if let Some(client) = &self.client {
            client.disconnect().await?;
        }
        self.client = None;
        self.keys = None;
        self.user_pubkey = None;
        self.subscribed_projects.clear();
        Ok(())
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
