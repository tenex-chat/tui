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

/// Inspect the LMDB free-list to detect write-time corruption before nostrdb opens.
///
/// Returns `Some(reason)` when corruption is detected, `None` when the database
/// looks healthy (or when the file is absent / cannot be read).
///
/// The free-list leaf page stores entries whose data is an `MDB_IDL`: a
/// length-prefixed array of page-numbers (each a `u64`).  After a crash with
/// two concurrent LMDB writers the count field ends up as a multi-billion
/// garbage value.  Any count > `last_pg` (the highest allocated page) is
/// impossible, so we use that as the corruption signal.
fn check_lmdb_free_list_corruption(data_dir: &std::path::Path) -> Option<String> {
    use std::io::{Read, Seek, SeekFrom};

    let data_mdb = data_dir.join("data.mdb");
    let mut f = std::fs::File::open(&data_mdb).ok()?;

    // ── meta page 0 (always at file offset 0) ──────────────────────────────
    let mut meta_page = [0u8; 256];
    f.read_exact(&mut meta_page).ok()?;

    let magic = u32::from_le_bytes(meta_page[16..20].try_into().ok()?);
    if magic != 0xBEEFC0DE {
        return Some(format!("bad LMDB magic 0x{magic:08x}"));
    }

    // LMDB stores the page size in free_db.md_pad (first field of mm_dbs[0]).
    // mm_dbs[0] starts at meta+24 (page offset 40).
    let page_size = u32::from_le_bytes(meta_page[40..44].try_into().ok()?) as u64;
    if page_size < 512 || page_size > 65536 || (page_size & (page_size - 1)) != 0 {
        return Some(format!("implausible page_size={page_size}"));
    }

    // free_db.md_root: offset 40 within MDB_db, which starts at page offset 40.
    // → page offset 80.
    let free_root = u64::from_le_bytes(meta_page[80..88].try_into().ok()?);
    // mm_last_pg at page offset 136.
    let last_pg = u64::from_le_bytes(meta_page[136..144].try_into().ok()?);

    if free_root < 2 || free_root > last_pg {
        return None; // no free list or obviously bad root — let LMDB handle it
    }

    // ── free-list root page ─────────────────────────────────────────────────
    f.seek(SeekFrom::Start(free_root * page_size)).ok()?;
    let mut fp = vec![0u8; page_size as usize];
    f.read_exact(&mut fp).ok()?;

    // Page pgno must match what we seeked to.
    let stored_pgno = u64::from_le_bytes(fp[0..8].try_into().ok()?);
    if stored_pgno != free_root {
        return Some(format!(
            "free-list root page {free_root} has pgno={stored_pgno}"
        ));
    }

    // Page flags must include LEAF (0x02) or BRANCH (0x01).
    let flags = u16::from_le_bytes(fp[10..12].try_into().ok()?);
    if flags & 0x03 == 0 {
        return Some(format!(
            "free-list root page {free_root} has invalid flags=0x{flags:04x}"
        ));
    }

    // ── scan node entries (inline data only; LEAF page) ─────────────────────
    let lower = u16::from_le_bytes(fp[12..14].try_into().ok()?) as usize;
    let num_keys = lower.saturating_sub(16) / 2;

    for i in 0..num_keys {
        let node_off = u16::from_le_bytes(fp[16 + i * 2..18 + i * 2].try_into().ok()?) as usize;
        if node_off + 8 > fp.len() {
            break;
        }
        // MDB_node header: mn_lo(u16), mn_hi(u16), mn_flags(u16), mn_ksize(u16)
        let mn_lo = u16::from_le_bytes(fp[node_off..node_off + 2].try_into().ok()?) as u64;
        let mn_hi = u16::from_le_bytes(fp[node_off + 2..node_off + 4].try_into().ok()?) as u64;
        let mn_flags = u16::from_le_bytes(fp[node_off + 4..node_off + 6].try_into().ok()?);
        let mn_ksize = u16::from_le_bytes(fp[node_off + 6..node_off + 8].try_into().ok()?) as usize;
        let data_size = mn_lo | (mn_hi << 16);

        // Skip overflow nodes (F_BIGDATA = 0x01)
        if mn_flags & 0x01 != 0 {
            continue;
        }

        let data_off = node_off + 8 + mn_ksize;
        if data_off + 8 > fp.len() || data_size < 8 {
            continue;
        }

        // First u64 in the value is the MDB_IDL count.
        let count = u64::from_le_bytes(fp[data_off..data_off + 8].try_into().ok()?);
        if count > last_pg {
            return Some(format!(
                "free-list entry {i} has page-count={count} > last_pg={last_pg}: database is corrupted"
            ));
        }
    }

    None
}

/// Wipe LMDB data files and return true if corruption was detected.
fn wipe_ndb_if_corrupted(data_dir: &std::path::Path) -> bool {
    if let Some(reason) = check_lmdb_free_list_corruption(data_dir) {
        eprintln!(
            "WARNING: nostrdb database is corrupted ({}). \
             Wiping database — data will be re-synced from relays.",
            reason
        );
        let _ = std::fs::remove_file(data_dir.join("data.mdb"));
        let _ = std::fs::remove_file(data_dir.join("lock.mdb"));
        true
    } else {
        false
    }
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
    // Detect and wipe corrupted LMDB free-list before handing off to nostrdb.
    // A concurrent-writer crash can leave garbage page-counts in the free-list
    // that cause mdb_midl_xmerge to SIGSEGV on the first write transaction.
    wipe_ndb_if_corrupted(data_dir);

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
                    let note_id = hex::encode(note.id());
                    crate::tlog!(
                        "NDB-SUB",
                        "kind:1 note_key={} id={}",
                        note_key.as_u64(),
                        &note_id
                    );
                    if let Some(message) = Message::from_note(&note) {
                        crate::tlog!(
                            "NDB-SUB",
                            "  → message thread={} id={}",
                            message.thread_id,
                            message.id
                        );
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

/// Convenience wrapper for callers that hold note key IDs as raw u64 values.
pub fn process_note_key_ids(
    ndb: &Ndb,
    data_store: &mut AppDataStore,
    handle: &CoreHandle,
    key_ids: &[u64],
) -> Result<Vec<CoreEvent>> {
    let note_keys: Vec<NoteKey> = key_ids.iter().map(|&k| NoteKey::new(k)).collect();
    process_note_keys(ndb, data_store, handle, &note_keys)
}

impl CoreRuntime {
    pub fn new(config: CoreConfig) -> Result<Self> {
        std::fs::create_dir_all(&config.data_dir)?;
        let ndb_config = nostrdb::Config::new().set_mapsize(16 * 1024 * 1024 * 1024);
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
        // kind:0 (NIP-01 metadata + per-agent capability announcement) is
        // replaceable, so it lives in nostrdb and is observed via this filter.
        let ndb_filter = FilterBuilder::new()
            .kinds([
                31933, 1, 0, 513, 4129, 30023, 34199, 4199, 4200, 4201, 4202,
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
