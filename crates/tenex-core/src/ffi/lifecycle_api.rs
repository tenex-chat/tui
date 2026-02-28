use super::*;

#[uniffi::export]
impl TenexCore {
    /// Create a new TenexCore instance.
    /// This is the entry point for the FFI API.
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            keys: Arc::new(RwLock::new(None)),
            ndb: Arc::new(RwLock::new(None)),
            store: Arc::new(RwLock::new(None)),
            core_handle: Arc::new(RwLock::new(None)),
            data_rx: Arc::new(Mutex::new(None)),
            worker_handle: Arc::new(RwLock::new(None)),
            last_refresh_ms: AtomicU64::new(0),
            ndb_stream: Arc::new(RwLock::new(None)),
            preferences: Arc::new(RwLock::new(None)),
            subscription_stats: Arc::new(RwLock::new(None)),
            negentropy_stats: Arc::new(RwLock::new(None)),
            event_callback: Arc::new(RwLock::new(None)),
            callback_listener_running: Arc::new(AtomicBool::new(false)),
            callback_listener_handle: Arc::new(RwLock::new(None)),
            ndb_transaction_lock: Arc::new(Mutex::new(())),
            cached_today_runtime_ms: Arc::new(AtomicU64::new(0)),
            relay_urls: Arc::new(RwLock::new(vec![crate::constants::RELAY_URL.to_string()])),
        }
    }

    /// Initialize the core. Must be called before other operations.
    /// Returns true if initialization succeeded.
    ///
    /// Note: This is lightweight and can be called from any thread.
    /// Heavy initialization (relay connection) happens during login.
    pub fn init(&self) -> bool {
        let init_started_at = Instant::now();
        if self.initialized.load(Ordering::SeqCst) {
            tlog!("PERF", "ffi.init already initialized");
            return true;
        }

        // Get the data directory for nostrdb
        let data_dir = get_data_dir();
        let log_path = data_dir.join("tenex.log");
        set_log_path(log_path.clone());
        tlog!(
            "PERF",
            "ffi.init start dataDir={} logPath={}",
            data_dir.display(),
            log_path.display()
        );
        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            eprintln!("Failed to create data directory: {}", e);
            tlog!("ERROR", "ffi.init failed creating data dir: {}", e);
            return false;
        }

        // Initialize nostrdb with an expanded map size.
        // 2GB has proven too small for active accounts and leads to persistent
        // TransactionFailed / map pressure behavior.
        let config = nostrdb::Config::new().set_mapsize(8 * 1024 * 1024 * 1024);
        let ndb_open_started_at = Instant::now();
        let ndb = match open_ndb_with_lock_recovery(&data_dir, &config) {
            Ok(ndb) => ndb,
            Err(e) => {
                eprintln!("Failed to initialize nostrdb: {}", e);
                tlog!("ERROR", "ffi.init failed opening ndb: {}", e);
                return false;
            }
        };
        tlog!(
            "PERF",
            "ffi.init ndb opened elapsedMs={}",
            ndb_open_started_at.elapsed().as_millis()
        );

        // Start Nostr worker (same core path as TUI/CLI)
        let worker_started_at = Instant::now();
        let (command_tx, command_rx) = mpsc::channel::<NostrCommand>();
        let (data_tx, data_rx) = mpsc::channel::<DataChange>();
        let event_stats = SharedEventStats::new();
        let subscription_stats = SharedSubscriptionStats::new();
        let negentropy_stats = SharedNegentropySyncStats::new();

        // Clone stats before passing to worker so we can expose them via FFI
        let subscription_stats_clone = subscription_stats.clone();
        let negentropy_stats_clone = negentropy_stats.clone();

        let worker = NostrWorker::new(
            ndb.clone(),
            data_tx,
            command_rx,
            event_stats,
            subscription_stats,
            negentropy_stats,
        );
        let worker_handle = std::thread::spawn(move || {
            worker.run();
        });
        tlog!(
            "PERF",
            "ffi.init worker started elapsedMs={}",
            worker_started_at.elapsed().as_millis()
        );

        // Subscribe to relevant kinds in nostrdb (mirrors CoreRuntime)
        let subscribe_started_at = Instant::now();
        let ndb_filter = FilterBuilder::new()
            .kinds([
                31933, 1, 0, 513, 4129, 30023, 34199, 4199, 4200, 4201, 4202, 1111, 7,
            ])
            .build();
        let ndb_subscription = match ndb.subscribe(&[ndb_filter]) {
            Ok(sub) => sub,
            Err(e) => {
                eprintln!("Failed to subscribe to nostrdb: {}", e);
                tlog!("ERROR", "ffi.init failed creating ndb subscription: {}", e);
                return false;
            }
        };
        let ndb_stream = SubscriptionStream::new((*ndb).clone(), ndb_subscription);
        tlog!(
            "PERF",
            "ffi.init ndb stream ready elapsedMs={}",
            subscribe_started_at.elapsed().as_millis()
        );

        // Store ndb
        {
            let mut ndb_guard = match self.ndb.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *ndb_guard = Some(ndb.clone());
        }

        // Initialize AppDataStore
        let store_init_started_at = Instant::now();
        let store = AppDataStore::new(ndb.clone());
        tlog!(
            "PERF",
            "ffi.init AppDataStore::new elapsedMs={}",
            store_init_started_at.elapsed().as_millis()
        );
        {
            let mut store_guard = match self.store.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *store_guard = Some(store);
        }

        // Store worker handle + channels
        {
            let mut handle_guard = match self.core_handle.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *handle_guard = Some(CoreHandle::new(command_tx));
        }
        {
            let mut data_rx_guard = match self.data_rx.lock() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *data_rx_guard = Some(data_rx);
        }
        {
            let mut worker_guard = match self.worker_handle.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *worker_guard = Some(worker_handle);
        }
        {
            let mut stream_guard = match self.ndb_stream.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *stream_guard = Some(ndb_stream);
        }

        // Initialize preferences storage
        {
            let prefs = FfiPreferencesStorage::new(&data_dir);
            let mut prefs_guard = match self.preferences.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *prefs_guard = Some(prefs);
        }

        // Apply persisted backend trust state to the runtime store.
        if self.sync_trusted_backends_from_preferences().is_err() {
            tlog!("ERROR", "ffi.init failed syncing trusted backends");
            return false;
        }

        // Store stats references for diagnostics
        {
            let mut stats_guard = match self.subscription_stats.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *stats_guard = Some(subscription_stats_clone);
        }
        {
            let mut stats_guard = match self.negentropy_stats.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *stats_guard = Some(negentropy_stats_clone);
        }
        self.initialized.store(true, Ordering::SeqCst);
        tlog!(
            "PERF",
            "ffi.init complete totalMs={}",
            init_started_at.elapsed().as_millis()
        );
        true
    }

    /// Check if the core is initialized.
    pub fn is_initialized(&self) -> bool {
        self.initialized.load(Ordering::SeqCst)
    }

    /// Get the version of tenex-core.
    pub fn version(&self) -> String {
        env!("CARGO_PKG_VERSION").to_string()
    }
}
