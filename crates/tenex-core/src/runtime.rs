use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::Arc;
use std::thread::JoinHandle;

use anyhow::Result;
use futures::{FutureExt, StreamExt};
use nostrdb::{FilterBuilder, Ndb, NoteKey, SubscriptionStream, Transaction};

use crate::config::CoreConfig;
use crate::events::CoreEvent;
use crate::nostr::{DataChange, NostrCommand, NostrWorker};
use crate::models::Message;
use crate::stats::{SharedEventStats, SharedNegentropySyncStats, SharedSubscriptionStats};
use crate::store::{AppDataStore, Database};

#[derive(Clone)]
pub struct CoreHandle {
    command_tx: Sender<NostrCommand>,
}

impl CoreHandle {
    pub(crate) fn new(command_tx: Sender<NostrCommand>) -> Self {
        Self { command_tx }
    }

    pub fn send(&self, command: NostrCommand) -> Result<(), mpsc::SendError<NostrCommand>> {
        self.command_tx.send(command)
    }
}

pub struct CoreRuntime {
    ndb: Arc<Ndb>,
    data_store: Rc<RefCell<AppDataStore>>,
    db: Arc<Database>,
    data_rx: Option<Receiver<DataChange>>,
    handle: CoreHandle,
    worker_handle: Option<JoinHandle<()>>,
    ndb_stream: SubscriptionStream,
    event_stats: SharedEventStats,
    subscription_stats: SharedSubscriptionStats,
    negentropy_stats: SharedNegentropySyncStats,
}

pub(crate) fn process_note_keys(
    ndb: &Ndb,
    data_store: &mut AppDataStore,
    handle: &CoreHandle,
    note_keys: &[NoteKey],
) -> Result<Vec<CoreEvent>> {
    let txn = Transaction::new(ndb)?;
    let mut events = Vec::with_capacity(note_keys.len());

    for &note_key in note_keys.iter() {
        if let Ok(note) = ndb.get_note_by_key(&txn, note_key) {
            let kind = note.kind();

            // handle_event returns CoreEvent for kinds that need special handling (24010)
            let status_event = data_store.handle_event(kind, &note);

            match kind {
                1 => {
                    if let Some(message) = Message::from_note(&note) {
                        events.push(CoreEvent::Message(message));
                    }
                }
                24010 => {
                    // Use the event returned by handle_event (trust-validated)
                    if let Some(event) = status_event {
                        events.push(event);
                    }
                }
                24133 => {
                    // Operations status - already handled by data_store.handle_event
                    // No CoreEvent needed as UI will query data_store directly
                }
                _ => {}
            }
        }
    }

    // Subscribe to messages for any newly discovered projects
    for project_a_tag in data_store.drain_pending_project_subscriptions() {
        let _ = handle.send(NostrCommand::SubscribeToProjectMessages { project_a_tag });
    }

    Ok(events)
}

impl CoreRuntime {
    pub fn new(config: CoreConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.data_dir)?;
        let ndb = Arc::new(Ndb::new(
            config.data_dir.to_str().unwrap_or("tenex_data"),
            &nostrdb::Config::new(),
        )?);

        let data_store = Rc::new(RefCell::new(AppDataStore::new(ndb.clone())));
        let db = Arc::new(Database::with_ndb(ndb.clone(), &config.data_dir)?);

        let (command_tx, command_rx) = mpsc::channel::<NostrCommand>();
        let (data_tx, data_rx) = mpsc::channel::<DataChange>();

        let event_stats = SharedEventStats::new();
        let subscription_stats = SharedSubscriptionStats::new();
        let negentropy_stats = SharedNegentropySyncStats::new();
        let worker = NostrWorker::new(
            ndb.clone(),
            config.data_dir.clone(),
            data_tx,
            command_rx,
            event_stats.clone(),
            subscription_stats.clone(),
            negentropy_stats.clone(),
        );
        let worker_handle = std::thread::spawn(move || {
            worker.run();
        });

        // NOTE: Ephemeral kinds (24010, 24133) are intentionally excluded
        // They are processed directly via DataChange channel, not through nostrdb
        let ndb_filter = FilterBuilder::new()
            .kinds([31933, 1, 0, 4199, 513, 4129, 4201])
            .build();
        let ndb_subscription = ndb.subscribe(&[ndb_filter])?;
        let ndb_stream = SubscriptionStream::new((*ndb).clone(), ndb_subscription);

        Ok(Self {
            ndb,
            data_store,
            db,
            data_rx: Some(data_rx),
            handle: CoreHandle { command_tx },
            worker_handle: Some(worker_handle),
            ndb_stream,
            event_stats,
            subscription_stats,
            negentropy_stats,
        })
    }

    pub fn handle(&self) -> CoreHandle {
        self.handle.clone()
    }

    pub fn data_store(&self) -> Rc<RefCell<AppDataStore>> {
        self.data_store.clone()
    }

    pub fn database(&self) -> Arc<Database> {
        self.db.clone()
    }

    pub fn ndb(&self) -> Arc<Ndb> {
        self.ndb.clone()
    }

    pub fn event_stats(&self) -> SharedEventStats {
        self.event_stats.clone()
    }

    pub fn subscription_stats(&self) -> SharedSubscriptionStats {
        self.subscription_stats.clone()
    }

    pub fn negentropy_stats(&self) -> SharedNegentropySyncStats {
        self.negentropy_stats.clone()
    }

    pub fn take_data_rx(&mut self) -> Option<Receiver<DataChange>> {
        self.data_rx.take()
    }

    pub async fn next_note_keys(&mut self) -> Option<Vec<NoteKey>> {
        self.ndb_stream.next().await
    }

    pub fn poll_note_keys(&mut self) -> Option<Vec<NoteKey>> {
        self.ndb_stream.next().now_or_never().flatten()
    }

    pub fn process_note_keys(&self, note_keys: &[NoteKey]) -> Result<Vec<CoreEvent>> {
        let mut store = self.data_store.borrow_mut();
        process_note_keys(self.ndb.as_ref(), &mut store, &self.handle, note_keys)
    }

    pub fn shutdown(&mut self) {
        let _ = self.handle.send(NostrCommand::Shutdown);
        if let Some(worker_handle) = self.worker_handle.take() {
            let _ = worker_handle.join();
        }
    }
}
