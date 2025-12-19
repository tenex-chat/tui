use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::collections::HashSet;
use anyhow::Result;
use nostr_sdk::prelude::*;
use rusqlite::Connection;
use tokio::runtime::Runtime;
use crate::store::{events::StoredEvent, insert_events};
use tracing::{info, error, debug};

pub enum NostrCommand {
    Connect { keys: Keys, user_pubkey: String },
    Sync,
    PublishThread { project_a_tag: String, title: String, content: String },
    PublishMessage { thread_id: String, content: String },
    Disconnect,
    Shutdown,
}

#[derive(Debug, Clone)]
pub enum DataChange {
    ProjectsUpdated,
    ThreadsUpdated(String),
    MessagesUpdated(String),
    ProfilesUpdated,
}

pub struct NostrWorker {
    client: Option<Client>,
    keys: Option<Keys>,
    user_pubkey: Option<String>,
    db_conn: Arc<Mutex<Connection>>,
    data_tx: Sender<DataChange>,
    command_rx: Receiver<NostrCommand>,
    subscribed_projects: HashSet<String>,
    needed_profiles: HashSet<String>,
}

impl NostrWorker {
    pub fn new(
        db_conn: Arc<Mutex<Connection>>,
        data_tx: Sender<DataChange>,
        command_rx: Receiver<NostrCommand>,
    ) -> Self {
        Self {
            client: None,
            keys: None,
            user_pubkey: None,
            db_conn,
            data_tx,
            command_rx,
            subscribed_projects: HashSet::new(),
            needed_profiles: HashSet::new(),
        }
    }

    pub fn run(mut self) {
        let rt = Runtime::new().expect("Failed to create runtime");

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
                    NostrCommand::PublishThread { project_a_tag, title, content } => {
                        info!("Worker: Publishing thread");
                        if let Err(e) = rt.block_on(self.handle_publish_thread(project_a_tag, title, content)) {
                            error!("Failed to publish thread: {}", e);
                        }
                    }
                    NostrCommand::PublishMessage { thread_id, content } => {
                        info!("Worker: Publishing message");
                        if let Err(e) = rt.block_on(self.handle_publish_message(thread_id, content)) {
                            error!("Failed to publish message: {}", e);
                        }
                    }
                    NostrCommand::Disconnect => {
                        info!("Worker: Disconnecting");
                        if let Err(e) = rt.block_on(self.handle_disconnect()) {
                            error!("Failed to disconnect: {}", e);
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

        client.add_relay("wss://tenex.chat").await?;

        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            client.connect()
        )
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

        let project_filter = Filter::new()
            .kind(Kind::Custom(31933))
            .author(pubkey);

        let mention_filter = Filter::new()
            .pubkey(pubkey);

        info!("Starting persistent subscriptions");

        let subscription_id = client.subscribe(vec![project_filter, mention_filter], None).await?;

        debug!("Subscription started: {:?}", subscription_id);

        self.spawn_notification_handler();

        Ok(())
    }

    fn spawn_notification_handler(&self) {
        let client = self.client.as_ref().unwrap().clone();
        let db_conn = self.db_conn.clone();
        let data_tx = self.data_tx.clone();

        tokio::spawn(async move {
            let mut notifications = client.notifications();

            loop {
                if let Ok(notification) = notifications.recv().await {
                    if let RelayPoolNotification::Event { event, .. } = notification {
                        debug!("Received event: kind={} id={}", event.kind, event.id);

                        if let Err(e) = Self::handle_incoming_event(&db_conn, &data_tx, *event) {
                            error!("Failed to handle event: {}", e);
                        }
                    }
                }
            }
        });
    }

    fn handle_incoming_event(
        db_conn: &Arc<Mutex<Connection>>,
        data_tx: &Sender<DataChange>,
        event: Event,
    ) -> Result<()> {
        let stored = StoredEvent {
            id: event.id.to_hex(),
            pubkey: event.pubkey.to_hex(),
            kind: event.kind.as_u16() as u32,
            created_at: event.created_at.as_u64(),
            content: event.content.clone(),
            tags: event.tags.iter().map(|t| {
                t.as_slice().iter().map(|s| s.to_string()).collect()
            }).collect(),
            sig: event.sig.to_string(),
        };

        insert_events(db_conn, &[stored])?;

        match event.kind.as_u16() {
            31933 => {
                info!("Project event received, notifying UI");
                data_tx.send(DataChange::ProjectsUpdated)?;
            }
            11 => {
                if let Some(a_tag) = Self::get_a_tag(&event) {
                    info!("Thread event received for project {}", &a_tag[..20]);
                    data_tx.send(DataChange::ThreadsUpdated(a_tag))?;
                }
            }
            1111 => {
                if let Some(thread_id) = Self::get_thread_id(&event) {
                    info!("Message event received for thread {}", &thread_id[..8]);
                    data_tx.send(DataChange::MessagesUpdated(thread_id))?;
                }
            }
            0 => {
                info!("Profile event received");
                data_tx.send(DataChange::ProfilesUpdated)?;
            }
            _ => {}
        }

        Ok(())
    }

    fn get_a_tag(event: &Event) -> Option<String> {
        event.tags.iter()
            .find(|t| {
                t.as_slice().first().map(|s| s == "a").unwrap_or(false)
            })
            .and_then(|t| t.as_slice().get(1))
            .map(|s| s.to_string())
    }

    fn get_thread_id(event: &Event) -> Option<String> {
        event.tags.iter()
            .find(|t| {
                t.as_slice().first().map(|s| s == "e").unwrap_or(false)
            })
            .and_then(|t| t.as_slice().get(1))
            .map(|s| s.to_string())
    }

    async fn handle_sync(&mut self) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let user_pubkey = self.user_pubkey.as_ref().ok_or_else(|| anyhow::anyhow!("No user pubkey"))?;

        info!("Starting sync for user {}", &user_pubkey[..8]);

        let pubkey = PublicKey::parse(user_pubkey)?;

        let project_filter = Filter::new()
            .kind(Kind::Custom(31933))
            .author(pubkey);

        let events = client.fetch_events(vec![project_filter], std::time::Duration::from_secs(10)).await?;

        let stored: Vec<StoredEvent> = events
            .into_iter()
            .map(|e| StoredEvent {
                id: e.id.to_hex(),
                pubkey: e.pubkey.to_hex(),
                kind: e.kind.as_u16() as u32,
                created_at: e.created_at.as_u64(),
                content: e.content.clone(),
                tags: e.tags.iter().map(|t| {
                    t.as_slice().iter().map(|s| s.to_string()).collect()
                }).collect(),
                sig: e.sig.to_string(),
            })
            .collect();

        insert_events(&self.db_conn, &stored)?;
        self.data_tx.send(DataChange::ProjectsUpdated)?;

        let projects = crate::store::get_projects(&self.db_conn)?;

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

        let thread_events = client.fetch_events(vec![thread_filter.clone()], std::time::Duration::from_secs(10)).await?;

        let stored_threads: Vec<StoredEvent> = thread_events
            .iter()
            .map(|e| StoredEvent {
                id: e.id.to_hex(),
                pubkey: e.pubkey.to_hex(),
                kind: e.kind.as_u16() as u32,
                created_at: e.created_at.as_u64(),
                content: e.content.clone(),
                tags: e.tags.iter().map(|t| {
                    t.as_slice().iter().map(|s| s.to_string()).collect()
                }).collect(),
                sig: e.sig.to_string(),
            })
            .collect();

        insert_events(&self.db_conn, &stored_threads)?;

        self.data_tx.send(DataChange::ThreadsUpdated(project_a_tag.to_string()))?;

        let thread_ids: Vec<EventId> = thread_events.iter().map(|e| e.id).collect();

        if !thread_ids.is_empty() {
            let message_filter = Filter::new()
                .kind(Kind::Custom(1111))
                .events(thread_ids);

            let message_events = client.fetch_events(vec![message_filter.clone()], std::time::Duration::from_secs(10)).await?;

            let stored_messages: Vec<StoredEvent> = message_events
                .into_iter()
                .map(|e| StoredEvent {
                    id: e.id.to_hex(),
                    pubkey: e.pubkey.to_hex(),
                    kind: e.kind.as_u16() as u32,
                    created_at: e.created_at.as_u64(),
                    content: e.content.clone(),
                    tags: e.tags.iter().map(|t| {
                        t.as_slice().iter().map(|s| s.to_string()).collect()
                    }).collect(),
                    sig: e.sig.to_string(),
                })
                .collect();

            insert_events(&self.db_conn, &stored_messages)?;

            for thread in thread_events.iter() {
                if let Some(thread_id) = Self::get_thread_id(&thread) {
                    self.data_tx.send(DataChange::MessagesUpdated(thread_id))?;
                }
            }
        }

        client.subscribe(vec![thread_filter], None).await?;

        Ok(())
    }

    async fn fetch_profiles(&mut self) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;
        let pubkeys: Vec<String> = self.needed_profiles.drain().collect();

        let pks: Vec<PublicKey> = pubkeys
            .iter()
            .filter_map(|p| PublicKey::parse(p).ok())
            .collect();

        if pks.is_empty() {
            return Ok(());
        }

        let filter = Filter::new()
            .kind(Kind::Metadata)
            .authors(pks);

        let events = client.fetch_events(vec![filter], std::time::Duration::from_secs(10)).await?;

        let stored: Vec<StoredEvent> = events
            .into_iter()
            .map(|e| StoredEvent {
                id: e.id.to_hex(),
                pubkey: e.pubkey.to_hex(),
                kind: e.kind.as_u16() as u32,
                created_at: e.created_at.as_u64(),
                content: e.content.clone(),
                tags: e.tags.iter().map(|t| {
                    t.as_slice().iter().map(|s| s.to_string()).collect()
                }).collect(),
                sig: e.sig.to_string(),
            })
            .collect();

        insert_events(&self.db_conn, &stored)?;
        self.data_tx.send(DataChange::ProfilesUpdated)?;

        Ok(())
    }

    async fn handle_publish_thread(&self, project_a_tag: String, title: String, content: String) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        let event = EventBuilder::new(
            Kind::Custom(11),
            content,
        )
        .tag(Tag::custom(
            TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
            vec![project_a_tag.clone()],
        ))
        .tag(Tag::custom(
            TagKind::Custom(std::borrow::Cow::Borrowed("title")),
            vec![title],
        ));

        let event_id = client.send_event_builder(event).await?;
        info!("Published thread: {}", event_id.id());

        Ok(())
    }

    async fn handle_publish_message(&self, thread_id: String, content: String) -> Result<()> {
        let client = self.client.as_ref().ok_or_else(|| anyhow::anyhow!("No client"))?;

        let event = EventBuilder::new(
            Kind::Custom(1111),
            content,
        )
        .tag(Tag::event(EventId::parse(&thread_id)?));

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
