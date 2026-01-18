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
use crate::models::{Message, ProjectStatus};
use crate::store::{AppDataStore, Database};

#[derive(Clone)]
pub struct CoreHandle {
    command_tx: Sender<NostrCommand>,
}

impl CoreHandle {
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

        let worker = NostrWorker::new(ndb.clone(), data_tx, command_rx);
        let worker_handle = std::thread::spawn(move || {
            worker.run();
        });

        let ndb_filter = FilterBuilder::new()
            .kinds([31933, 1, 0, 4199, 24010, 513, 24133])
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
        let txn = Transaction::new(&self.ndb)?;
        let mut events = Vec::with_capacity(note_keys.len());

        for &note_key in note_keys.iter() {
            if let Ok(note) = self.ndb.get_note_by_key(&txn, note_key) {
                let kind = note.kind();

                self.data_store.borrow_mut().handle_event(kind, &note);

                match kind {
                    1 => {
                        if let Some(message) = Message::from_note(&note) {
                            events.push(CoreEvent::Message(message));
                        }
                    }
                    24010 => {
                        if let Some(status) = ProjectStatus::from_note(&note) {
                            events.push(CoreEvent::ProjectStatus(status));
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

        Ok(events)
    }

    pub fn shutdown(&mut self) {
        let _ = self.handle.send(NostrCommand::Shutdown);
        if let Some(worker_handle) = self.worker_handle.take() {
            let _ = worker_handle.join();
        }
    }
}
