use super::*;

// Private implementation methods for TenexCore (not exposed via UniFFI)
impl TenexCore {
    pub(super) fn sync_trusted_backends_from_preferences(&self) -> Result<(), TenexError> {
        let (approved, blocked) = {
            let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;
            let prefs = prefs_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;
            prefs.trusted_backends()
        };

        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;
        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        store.trust.set_trusted_backends(approved, blocked);
        Ok(())
    }

    pub(super) fn persist_trusted_backends_to_preferences(
        &self,
        approved: std::collections::HashSet<String>,
        blocked: std::collections::HashSet<String>,
    ) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;
        let prefs = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs
            .set_trusted_backends(approved, blocked)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    pub(super) fn persist_current_trusted_backends(&self) -> Result<(), TenexError> {
        let (approved, blocked) = {
            let store_guard = self.store.read().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire store lock: {}", e),
            })?;
            let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
                message: "Store not initialized - call init() first".to_string(),
            })?;
            (
                store.trust.approved_backends.clone(),
                store.trust.blocked_backends.clone(),
            )
        };

        self.persist_trusted_backends_to_preferences(approved, blocked)
    }

    /// Collect system diagnostics (version, status)
    pub(super) fn collect_system_diagnostics(
        &self,
        data_dir: &std::path::Path,
    ) -> Result<SystemDiagnostics, String> {
        let is_initialized = self.initialized.load(Ordering::SeqCst);
        let is_logged_in = self.is_logged_in();
        let log_path = data_dir.join("tenex.log").to_string_lossy().to_string();
        let (relay_connected, connected_relays) = self.get_relay_status();

        Ok(SystemDiagnostics {
            log_path,
            version: env!("CARGO_PKG_VERSION").to_string(),
            is_initialized,
            is_logged_in,
            relay_connected,
            connected_relays,
        })
    }

    pub(super) fn get_relay_status(&self) -> (bool, u32) {
        use std::time::Duration;

        let handle = match get_core_handle(&self.core_handle) {
            Ok(handle) => handle,
            Err(_) => return (false, 0),
        };

        let (tx, rx) = std::sync::mpsc::channel();
        if handle
            .send(NostrCommand::GetRelayStatus { response_tx: tx })
            .is_err()
        {
            return (false, 0);
        }

        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(count) => (count > 0, count.min(u32::MAX as usize) as u32),
            Err(_) => (false, 0),
        }
    }

    /// Collect negentropy sync diagnostics
    pub(super) fn collect_sync_diagnostics(&self) -> Result<NegentropySyncDiagnostics, String> {
        use crate::stats::NegentropySyncStatus;

        let stats_guard = self
            .negentropy_stats
            .read()
            .map_err(|_| "Failed to acquire negentropy_stats lock".to_string())?;

        Ok(if let Some(stats) = stats_guard.as_ref() {
            let snapshot = stats.snapshot();
            let seconds_since_last_cycle =
                snapshot.last_cycle_time().map(|t| t.elapsed().as_secs());

            let recent_results: Vec<SyncResultDiagnostic> = snapshot
                .recent_results
                .iter()
                .map(|r| SyncResultDiagnostic {
                    kind_label: r.kind_label.clone(),
                    events_received: r.events_received,
                    status: match r.status {
                        NegentropySyncStatus::Ok => "ok".to_string(),
                        NegentropySyncStatus::Unsupported => "unsupported".to_string(),
                        NegentropySyncStatus::Failed => "failed".to_string(),
                    },
                    error: r.error.clone(),
                    seconds_ago: r.completed_at.elapsed().as_secs(),
                })
                .collect();

            NegentropySyncDiagnostics {
                enabled: snapshot.enabled,
                current_interval_secs: snapshot.current_interval_secs,
                seconds_since_last_cycle,
                sync_in_progress: snapshot.sync_in_progress,
                successful_syncs: snapshot.successful_syncs,
                failed_syncs: snapshot.failed_syncs,
                unsupported_syncs: snapshot.unsupported_syncs,
                total_events_reconciled: snapshot.total_events_reconciled,
                recent_results,
            }
        } else {
            // No stats available yet - return default
            NegentropySyncDiagnostics {
                enabled: false,
                current_interval_secs: 0,
                seconds_since_last_cycle: None,
                sync_in_progress: false,
                successful_syncs: 0,
                failed_syncs: 0,
                unsupported_syncs: 0,
                total_events_reconciled: 0,
                recent_results: Vec::new(),
            }
        })
    }

    /// Collect subscription diagnostics
    pub(super) fn collect_subscription_diagnostics(
        &self,
    ) -> Result<(Vec<SubscriptionDiagnostics>, u64), String> {
        let stats_guard = self
            .subscription_stats
            .read()
            .map_err(|_| "Failed to acquire subscription_stats lock".to_string())?;

        Ok(if let Some(stats) = stats_guard.as_ref() {
            let snapshot = stats.snapshot();
            let subs: Vec<SubscriptionDiagnostics> = snapshot
                .subscriptions
                .iter()
                .map(|(sub_id, info)| SubscriptionDiagnostics {
                    sub_id: sub_id.clone(),
                    description: info.description.clone(),
                    kinds: info.kinds.clone(),
                    raw_filter: info.raw_filter.clone(),
                    events_received: info.events_received,
                    age_secs: info.created_at.elapsed().as_secs(),
                })
                .collect();
            let total = snapshot.total_events();
            (subs, total)
        } else {
            (Vec::new(), 0)
        })
    }

    /// Collect database diagnostics (potentially expensive - scans event kinds)
    pub(super) fn collect_database_diagnostics(
        &self,
        data_dir: &std::path::Path,
    ) -> Result<DatabaseStats, String> {
        // CRITICAL: Acquire transaction lock before creating any nostrdb Transactions.
        // query_ndb_stats() creates a Transaction, so we must hold this lock to prevent
        // concurrent access with refresh() which also creates Transactions.
        let _tx_guard = self
            .ndb_transaction_lock
            .lock()
            .map_err(|_| "Failed to acquire transaction lock".to_string())?;

        let ndb_guard = self
            .ndb
            .read()
            .map_err(|_| "Failed to acquire ndb lock".to_string())?;

        Ok(if let Some(ndb) = ndb_guard.as_ref() {
            // Get event counts by kind using the existing query_ndb_stats function
            let kind_counts = query_ndb_stats(ndb);

            // Convert to Vec<KindEventCount> and sort by count descending
            let mut event_counts: Vec<KindEventCount> = kind_counts
                .into_iter()
                .map(|(kind, count)| KindEventCount {
                    kind,
                    count,
                    name: get_kind_name(kind),
                })
                .collect();
            event_counts.sort_by(|a, b| b.count.cmp(&a.count));

            let total_events: u64 = event_counts.iter().map(|k| k.count).sum();
            let db_size_bytes = get_db_file_size(data_dir);

            DatabaseStats {
                db_size_bytes,
                event_counts_by_kind: event_counts,
                total_events,
            }
        } else {
            DatabaseStats {
                db_size_bytes: 0,
                event_counts_by_kind: Vec::new(),
                total_events: 0,
            }
        })
    }
}

// Private implementation methods for TenexCore (event callback listener)
impl TenexCore {
    /// Start the background listener thread that monitors data channels
    /// and fires callbacks when events arrive.
    pub(super) fn start_callback_listener(&self) {
        let running = self.callback_listener_running.clone();
        let data_rx = self.data_rx.clone();
        let ndb_stream = self.ndb_stream.clone();
        let store = self.store.clone();
        let prefs = self.preferences.clone();
        let ndb = self.ndb.clone();
        let core_handle = self.core_handle.clone();
        let txn_lock = self.ndb_transaction_lock.clone();
        let callback_ref = self.event_callback.clone();
        let cached_runtime = self.cached_today_runtime_ms.clone();

        let handle = std::thread::spawn(move || {
            tlog!("PERF", "callback_listener thread started");
            while running.load(Ordering::Relaxed) {
                let cycle_started_at = Instant::now();
                let mut data_changes: Vec<DataChange> = Vec::new();
                if let Ok(rx_guard) = data_rx.lock() {
                    if let Some(rx) = rx_guard.as_ref() {
                        while let Ok(change) = rx.try_recv() {
                            data_changes.push(change);
                        }
                    }
                }

                let mut note_batches: Vec<Vec<NoteKey>> = Vec::new();
                if let Ok(mut stream_guard) = ndb_stream.write() {
                    if let Some(stream) = stream_guard.as_mut() {
                        while let Some(note_keys) = stream.next().now_or_never().flatten() {
                            note_batches.push(note_keys);
                        }
                    }
                }

                if data_changes.is_empty() && note_batches.is_empty() {
                    std::thread::sleep(Duration::from_millis(50));
                    continue;
                }
                let data_change_count = data_changes.len();
                let note_batch_count = note_batches.len();
                let note_key_count: usize = note_batches.iter().map(|batch| batch.len()).sum();

                let _tx_guard = match txn_lock.lock() {
                    Ok(guard) => guard,
                    Err(_) => {
                        std::thread::sleep(Duration::from_millis(50));
                        continue;
                    }
                };

                let ndb = match ndb.read().ok().and_then(|g| g.as_ref().cloned()) {
                    Some(db) => db,
                    None => continue,
                };
                let core_handle = match core_handle.read().ok().and_then(|g| g.as_ref().cloned()) {
                    Some(handle) => handle,
                    None => continue,
                };

                let mut store_guard = match store.write() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                let store_ref = match store_guard.as_mut() {
                    Some(s) => s,
                    None => continue,
                };

                let prefs_guard = match prefs.read() {
                    Ok(g) => g,
                    Err(_) => continue,
                };
                let archived_ids = prefs_guard
                    .as_ref()
                    .map(|p| p.prefs.archived_thread_ids.clone())
                    .unwrap_or_default();

                let process_started_at = Instant::now();
                let mut deltas: Vec<DataChangeType> = Vec::new();

                if !data_changes.is_empty() {
                    deltas.extend(process_data_changes_with_deltas(store_ref, &data_changes));
                }

                for note_keys in note_batches {
                    if !note_keys.is_empty() {
                        deltas.extend(process_note_keys_with_deltas(
                            ndb.as_ref(),
                            store_ref,
                            &core_handle,
                            &note_keys,
                            &archived_ids,
                        ));
                    }
                }

                // Update lock-free runtime cache while we still hold the store write lock
                let (runtime_ms, _, _) = store_ref.get_statusbar_runtime_ms();
                cached_runtime.store(runtime_ms, Ordering::Release);

                drop(store_guard);

                append_snapshot_update_deltas(&mut deltas);
                let delta_summary = summarize_deltas(&deltas);
                let process_elapsed_ms = process_started_at.elapsed().as_millis();
                let mut callback_count = 0usize;
                let callback_started_at = Instant::now();

                if let Ok(cb_guard) = callback_ref.read() {
                    if let Some(cb) = cb_guard.as_ref() {
                        callback_count = deltas.len();
                        for delta in deltas {
                            cb.on_data_changed(delta);
                        }
                    }
                }
                let callback_elapsed_ms = callback_started_at.elapsed().as_millis();
                tlog!(
                    "PERF",
                    "callback_listener cycle dataChanges={} noteBatches={} noteKeys={} processMs={} callbackCount={} callbackMs={} deltas=[{}] totalMs={}",
                    data_change_count,
                    note_batch_count,
                    note_key_count,
                    process_elapsed_ms,
                    callback_count,
                    callback_elapsed_ms,
                    delta_summary.compact(),
                    cycle_started_at.elapsed().as_millis()
                );
            }
            tlog!("PERF", "callback_listener thread stopped");
        });

        if let Ok(mut guard) = self.callback_listener_handle.write() {
            *guard = Some(handle);
        }
    }
}

/// Get human-readable name for a Nostr event kind
fn get_kind_name(kind: u16) -> String {
    match kind {
        0 => "Metadata".to_string(),
        1 => "Text Notes".to_string(),
        3 => "Contact List".to_string(),
        4 => "DMs".to_string(),
        7 => "Reactions".to_string(),
        513 => "Conversations".to_string(),
        1111 => "Comments".to_string(),
        4129 => "Lessons".to_string(),
        4199 => "Agent Definitions".to_string(),
        4200 => "MCP Tools".to_string(),
        4201 => "Nudges".to_string(),
        4202 => "Skills".to_string(),
        24010 => "Project Status".to_string(),
        24133 => "Operations Status".to_string(),
        30023 => "Articles".to_string(),
        31933 => "Projects".to_string(),
        34199 => "Teams".to_string(),
        _ => format!("Kind {}", kind),
    }
}

/// Get the LMDB database file size in bytes
fn get_db_file_size(data_dir: &std::path::Path) -> u64 {
    // LMDB stores data in a file named "data.mdb"
    let db_file = data_dir.join("data.mdb");
    std::fs::metadata(&db_file).map(|m| m.len()).unwrap_or(0)
}
