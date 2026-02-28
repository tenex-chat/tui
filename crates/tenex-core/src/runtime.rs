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
use crate::models::Message;
use crate::nostr::{DataChange, NostrCommand, NostrWorker};
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

    pub fn send(&self, command: NostrCommand) -> Result<(), Box<mpsc::SendError<NostrCommand>>> {
        self.command_tx.send(command).map_err(Box::new)
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

fn open_ndb_with_health_check(
    data_dir: &std::path::Path,
    config: &nostrdb::Config,
) -> Result<Arc<Ndb>> {
    let ndb = Ndb::new(data_dir.to_str().unwrap_or("tenex_data"), config)?;
    let txn = Transaction::new(&ndb)?;
    let probe_filter = FilterBuilder::new().kinds([31933]).build();
    let _ = ndb.query(&txn, &[probe_filter], 1)?;
    Ok(Arc::new(ndb))
}

fn is_likely_stale_lock_error(message: &str) -> bool {
    let lower = message.to_ascii_lowercase();
    lower.contains("transaction failed")
        || lower.contains("lock")
        || lower.contains("resource temporarily unavailable")
        || lower.contains("busy")
        || lower.contains("mdb_bad_rslot")
        || lower.contains("mdb_readers_full")
}

fn open_ndb_with_lock_recovery(
    data_dir: &std::path::Path,
    config: &nostrdb::Config,
) -> Result<Arc<Ndb>> {
    match open_ndb_with_health_check(data_dir, config) {
        Ok(ndb) => Ok(ndb),
        Err(first_err) => {
            let first = first_err.to_string();
            if !is_likely_stale_lock_error(&first) {
                return Err(first_err);
            }

            let lock_path = data_dir.join("lock.mdb");
            if lock_path.exists() {
                let _ = std::fs::remove_file(&lock_path);
            }

            open_ndb_with_health_check(data_dir, config).map_err(|retry_err| {
                anyhow::anyhow!(
                    "{} (retry after lock recovery failed: {})",
                    first,
                    retry_err
                )
            })
        }
    }
}

/// Process note keys and update the given data store.
/// This standalone function allows external callers (like the daemon) to process
/// note keys with a custom data store instead of the CoreRuntime's internal one.
pub fn process_note_keys(
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
                    } else if let Some(message) = Message::from_thread_note(&note) {
                        // Thread root: emit as CoreEvent::Message if it p-tags the user
                        // so TUI can show status bar notifications for ask events
                        if let Some(ref user_pk) = data_store.user_pubkey {
                            if message.p_tags.iter().any(|p| p == user_pk) {
                                events.push(CoreEvent::Message(message));
                            }
                        }
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
                30023 => {
                    // Report upsert - use the event returned by handle_event
                    if let Some(event) = status_event {
                        events.push(event);
                    }
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
        let ndb_config = nostrdb::Config::new().set_mapsize(8 * 1024 * 1024 * 1024);
        let ndb = open_ndb_with_lock_recovery(&config.data_dir, &ndb_config)?;

        let data_store = Rc::new(RefCell::new(AppDataStore::new_with_cache(
            ndb.clone(),
            config.data_dir.clone(),
        )));
        let db = Arc::new(Database::with_ndb(ndb.clone(), &config.data_dir)?);

        let (command_tx, command_rx) = mpsc::channel::<NostrCommand>();
        let (data_tx, data_rx) = mpsc::channel::<DataChange>();

        let event_stats = SharedEventStats::new();
        let subscription_stats = SharedSubscriptionStats::new();
        let negentropy_stats = SharedNegentropySyncStats::new();
        let worker = NostrWorker::new(
            ndb.clone(),
            data_tx,
            command_rx,
            event_stats.clone(),
            subscription_stats.clone(),
            negentropy_stats.clone(),
        );
        let worker_handle = std::thread::spawn(move || {
            worker.run();
        });

        // NOTE: Ephemeral kinds (24010, 24133) are intentionally excluded.
        // They are processed directly via DataChange channel, not through nostrdb.
        // Include content-definition kinds so UI tabs can react live to newly
        // published agent definitions, nudges, skills, team packs, and MCP tools.
        let ndb_filter = FilterBuilder::new()
            .kinds([
                31933, 1, 0, 513, 4129, 30023, 14202, 34199, 4199, 4200, 4201, 4202,
            ])
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
        // Persist the in-memory state before stopping so the next startup can skip
        // the expensive rebuild_from_ndb().  Any error is logged; never panics.
        self.data_store.borrow().save_cache();

        let _ = self.handle.send(NostrCommand::Shutdown);
        if let Some(worker_handle) = self.worker_handle.take() {
            let _ = worker_handle.join();
        }
    }
}
