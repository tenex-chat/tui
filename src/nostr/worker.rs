use std::collections::HashSet;
use std::sync::mpsc::{Receiver, Sender};
use std::sync::Arc;

use anyhow::Result;
use nostr_sdk::prelude::*;
use nostrdb::Ndb;
use tokio::runtime::Runtime;
use tracing::{debug, error, info};

use crate::store::ingest_events;

const RELAY_URL: &str = "wss://tenex.chat";

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
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum DataChange {
    ProjectsUpdated,
    ThreadsUpdated(String),
    MessagesUpdated(String),
    ProfilesUpdated,
    AgentsUpdated,
    ProjectStatusUpdated(String),
    StreamingDelta { message_id: String, delta: String },
    ConversationMetadataUpdated,
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

        // Project status (kind 24010) - p-tagged to user
        let status_filter = Filter::new()
            .kind(Kind::Custom(24010))
            .pubkey(pubkey);

        // Streaming deltas (kind 21111) - p-tagged to user
        let streaming_filter = Filter::new()
            .kind(Kind::Custom(21111))
            .pubkey(pubkey);

        // Conversation metadata (kind 513) - provides titles and summaries for threads
        let metadata_filter = Filter::new().kind(Kind::Custom(513));

        info!("Starting persistent subscriptions");

        let subscription_id = client
            .subscribe(
                vec![
                    project_filter,
                    mention_filter,
                    agent_filter,
                    status_filter,
                    streaming_filter,
                    metadata_filter,
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
        let data_tx = self.data_tx.clone();
        let rt_handle = self.rt_handle.as_ref().unwrap().clone();

        rt_handle.spawn(async move {
            let mut notifications = client.notifications();

            loop {
                if let Ok(notification) = notifications.recv().await {
                    if let RelayPoolNotification::Event { relay_url, event, .. } = notification {
                        debug!("Received event: kind={} id={} from {}", event.kind, event.id, relay_url);

                        if let Err(e) =
                            Self::handle_incoming_event(&ndb, &data_tx, *event, relay_url.as_str())
                        {
                            error!("Failed to handle event: {}", e);
                        }
                    }
                }
            }
        });
    }

    fn handle_incoming_event(
        ndb: &Ndb,
        data_tx: &Sender<DataChange>,
        event: Event,
        relay_url: &str,
    ) -> Result<()> {
        // Ingest the event into nostrdb with relay metadata
        ingest_events(ndb, &[event.clone()], Some(relay_url))?;

        // Notify UI about the change
        match event.kind.as_u16() {
            31933 => {
                info!("Project event received, notifying UI");
                data_tx.send(DataChange::ProjectsUpdated)?;
            }
            11 => {
                if let Some(a_tag) = Self::get_a_tag(&event) {
                    info!("Thread event received for project {}", &a_tag[..20.min(a_tag.len())]);
                    data_tx.send(DataChange::ThreadsUpdated(a_tag))?;
                }
            }
            1111 => {
                if let Some(thread_id) = Self::get_thread_id(&event) {
                    info!("Message event received for thread {}", &thread_id[..8.min(thread_id.len())]);
                    data_tx.send(DataChange::MessagesUpdated(thread_id))?;
                }
            }
            0 => {
                info!("Profile event received");
                data_tx.send(DataChange::ProfilesUpdated)?;
            }
            4199 => {
                info!("Agent definition event received");
                data_tx.send(DataChange::AgentsUpdated)?;
            }
            24010 => {
                if let Some(a_tag) = Self::get_a_tag(&event) {
                    info!("Project status event received for {}", &a_tag[..20.min(a_tag.len())]);
                    data_tx.send(DataChange::ProjectStatusUpdated(a_tag))?;
                }
            }
            21111 => {
                // Streaming delta - extract message_id from e tag
                if let Some(message_id) = Self::get_e_tag(&event) {
                    let delta = event.content.to_string();
                    info!("Streaming delta received for message {}", &message_id[..8.min(message_id.len())]);
                    data_tx.send(DataChange::StreamingDelta { message_id, delta })?;
                }
            }
            513 => {
                info!("Conversation metadata event received");
                data_tx.send(DataChange::ConversationMetadataUpdated)?;
            }
            _ => {}
        }

        Ok(())
    }

    fn get_a_tag(event: &Event) -> Option<String> {
        event
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(|s| s == "a").unwrap_or(false))
            .and_then(|t| t.as_slice().get(1))
            .map(|s| s.to_string())
    }

    /// Get thread_id from uppercase "E" tag (NIP-22 root reference)
    fn get_thread_id(event: &Event) -> Option<String> {
        event
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(|s| s == "E").unwrap_or(false))
            .and_then(|t| t.as_slice().get(1))
            .map(|s| s.to_string())
    }

    /// Get message_id from lowercase "e" tag (for streaming deltas)
    fn get_e_tag(event: &Event) -> Option<String> {
        event
            .tags
            .iter()
            .find(|t| t.as_slice().first().map(|s| s == "e").unwrap_or(false))
            .and_then(|t| t.as_slice().get(1))
            .map(|s| s.to_string())
    }

    async fn handle_sync(&mut self) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let user_pubkey = self.user_pubkey.as_ref().ok_or_else(|| anyhow::anyhow!("No user pubkey"))?;

        info!("Starting sync for user {}", &user_pubkey[..8]);

        let pubkey = PublicKey::parse(user_pubkey)?;

        // Fetch projects
        let project_filter = Filter::new().kind(Kind::Custom(31933)).author(pubkey);

        let events = client
            .fetch_events(vec![project_filter], std::time::Duration::from_secs(10))
            .await?;

        let events_vec: Vec<Event> = events.into_iter().collect();
        ingest_events(&self.ndb, &events_vec, Some(RELAY_URL))?;
        self.data_tx.send(DataChange::ProjectsUpdated)?;

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
        let thread_filter = Filter::new()
            .kind(Kind::Custom(11))
            .custom_tag(SingleLetterTag::lowercase(Alphabet::A), [project_a_tag]);

        let thread_events = client
            .fetch_events(vec![thread_filter.clone()], std::time::Duration::from_secs(10))
            .await?;

        let thread_events_vec: Vec<Event> = thread_events.iter().cloned().collect();
        ingest_events(&self.ndb, &thread_events_vec, Some(RELAY_URL))?;

        self.data_tx.send(DataChange::ThreadsUpdated(project_a_tag.to_string()))?;

        // Fetch messages for each thread
        let thread_ids: Vec<EventId> = thread_events.iter().map(|e| e.id).collect();

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

            if !metadata_events_vec.is_empty() {
                self.data_tx.send(DataChange::ConversationMetadataUpdated)?;
            }

            // Fetch messages (kind 1111)
            // Kind 1111 uses uppercase "E" tag (NIP-22 root reference) to reference threads
            let message_filter = Filter::new()
                .kind(Kind::Custom(1111))
                .custom_tag(SingleLetterTag::uppercase(Alphabet::E), thread_id_hexes);

            let message_events = client
                .fetch_events(vec![message_filter.clone()], std::time::Duration::from_secs(10))
                .await?;

            let message_events_vec: Vec<Event> = message_events.into_iter().collect();
            ingest_events(&self.ndb, &message_events_vec, Some(RELAY_URL))?;

            for thread_id in thread_ids {
                self.data_tx.send(DataChange::MessagesUpdated(thread_id.to_hex()))?;
            }
        }

        // Subscribe for real-time updates
        client.subscribe(vec![thread_filter], None).await?;

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
        self.data_tx.send(DataChange::ProfilesUpdated)?;

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

        let mut event = EventBuilder::new(Kind::Custom(11), &content)
            // Project reference (a tag) - required
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec![project_a_tag.clone()],
            ))
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

        let event_id = client.send_event_builder(event).await?;
        info!("Published thread: {}", event_id.id());

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

        let mut event = EventBuilder::new(Kind::Custom(1111), &content)
            // NIP-22: Uppercase "E" tag = root thread reference (required)
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("E")),
                vec![thread_id.clone()],
            ))
            // Kind of root event
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("K")),
                vec!["11".to_string()],
            ))
            // Project reference (a tag)
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec![project_a_tag],
            ));

        // NIP-22: Lowercase "e" tag = reply-to reference (optional, for threaded replies)
        if let Some(reply_id) = reply_to {
            event = event.tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![reply_id],
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

        let event_id = client.send_event_builder(event).await?;
        info!("Published message: {}", event_id.id());

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
