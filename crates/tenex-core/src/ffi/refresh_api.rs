use super::*;

#[uniffi::export]
impl TenexCore {
    /// Refresh data from relays.
    /// Call this to fetch the latest data from relays.
    ///
    /// Includes throttling: if called within REFRESH_THROTTLE_INTERVAL_MS of the last
    /// refresh, returns immediately without doing work. This prevents excessive CPU/relay
    /// load from rapid successive calls (e.g., multiple views loading simultaneously).
    pub fn refresh(&self) -> bool {
        let refresh_started_at = Instant::now();
        // Throttle check: skip if we refreshed too recently
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let last_refresh = self.last_refresh_ms.load(Ordering::Relaxed);

        if last_refresh > 0 && now_ms.saturating_sub(last_refresh) < REFRESH_THROTTLE_INTERVAL_MS {
            // Throttled: skip this refresh call
            tlog!(
                "PERF",
                "ffi.refresh throttled deltaMs={} thresholdMs={}",
                now_ms.saturating_sub(last_refresh),
                REFRESH_THROTTLE_INTERVAL_MS
            );
            return true;
        }

        // Update last refresh timestamp (atomic swap for thread safety)
        self.last_refresh_ms.store(now_ms, Ordering::Relaxed);

        // CRITICAL: Acquire transaction lock to prevent concurrent nostrdb Transactions.
        // This lock must be held for the entire duration of note processing to ensure
        // getDiagnosticsSnapshot() cannot create a conflicting Transaction.
        let _tx_guard = match self.ndb_transaction_lock.lock() {
            Ok(guard) => guard,
            Err(_) => return false, // Lock poisoned, fail safely
        };

        let ndb = {
            let ndb_guard = match self.ndb.read() {
                Ok(g) => g,
                Err(_) => return false,
            };
            match ndb_guard.as_ref() {
                Some(ndb) => ndb.clone(),
                None => return false,
            }
        };

        let core_handle = {
            let handle_guard = match self.core_handle.read() {
                Ok(g) => g,
                Err(_) => return false,
            };
            match handle_guard.as_ref() {
                Some(handle) => handle.clone(),
                None => return false,
            }
        };

        // Drain data changes first (ephemeral status updates)
        let mut data_changes = Vec::new();
        if let Ok(rx_guard) = self.data_rx.lock() {
            if let Some(rx) = rx_guard.as_ref() {
                while let Ok(change) = rx.try_recv() {
                    data_changes.push(change);
                }
            }
        }
        let initial_data_change_count = data_changes.len();

        // Drain nostrdb subscription stream for new notes
        let mut note_batches: Vec<Vec<NoteKey>> = Vec::new();
        if let Ok(mut stream_guard) = self.ndb_stream.write() {
            if let Some(stream) = stream_guard.as_mut() {
                while let Some(note_keys) = stream.next().now_or_never().flatten() {
                    note_batches.push(note_keys);
                }
            }
        }
        let initial_note_batch_count = note_batches.len();
        let initial_note_key_count: usize = note_batches.iter().map(|batch| batch.len()).sum();

        let mut store_guard = match self.store.write() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let store = match store_guard.as_mut() {
            Some(store) => store,
            None => return false,
        };

        // Get callback reference before processing changes
        let callback = self.event_callback.read().ok().and_then(|g| g.clone());

        let prefs_guard = match self.preferences.read() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let archived_ids = prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.clone())
            .unwrap_or_default();

        let initial_process_started_at = Instant::now();
        let mut deltas: Vec<DataChangeType> = Vec::new();

        if !data_changes.is_empty() {
            deltas.extend(process_data_changes_with_deltas(store, &data_changes));
        }

        for note_keys in note_batches {
            if !note_keys.is_empty() {
                deltas.extend(process_note_keys_with_deltas(
                    ndb.as_ref(),
                    store,
                    &core_handle,
                    &note_keys,
                    &archived_ids,
                ));
            }
        }
        let initial_process_elapsed_ms = initial_process_started_at.elapsed().as_millis();

        append_snapshot_update_deltas(&mut deltas);
        let initial_delta_summary = summarize_deltas(&deltas);

        let initial_callback_started_at = Instant::now();
        let initial_callback_count = if callback.is_some() { deltas.len() } else { 0 };
        if let Some(ref cb) = callback {
            for delta in deltas {
                cb.on_data_changed(delta);
            }
        }
        let initial_callback_elapsed_ms = initial_callback_started_at.elapsed().as_millis();
        tlog!(
            "PERF",
            "ffi.refresh pass=initial dataChanges={} noteBatches={} noteKeys={} processMs={} callbackCount={} callbackMs={} deltas=[{}]",
            initial_data_change_count,
            initial_note_batch_count,
            initial_note_key_count,
            initial_process_elapsed_ms,
            initial_callback_count,
            initial_callback_elapsed_ms,
            initial_delta_summary.compact()
        );

        let ok = true;

        // Release store lock before polling for more events
        drop(store_guard);

        // Poll for additional events to catch messages arriving from newly subscribed projects.
        //
        // Context: When iOS calls refresh(), the notification handler may have just subscribed
        // to messages for newly discovered projects (kind:31933). The relay is sending historical
        // messages, but they haven't been ingested into nostrdb yet. This polling loop gives
        // time for those events to arrive.
        //
        // Strategy: Poll until no new events arrive for REFRESH_QUIET_PERIOD_MS, or until
        // REFRESH_MAX_POLL_TIMEOUT_MS is reached. This is adaptive - if events keep arriving,
        // we keep polling. If nothing arrives, we exit quickly.
        let poll_started_at = Instant::now();
        let max_deadline = Instant::now() + Duration::from_millis(REFRESH_MAX_POLL_TIMEOUT_MS);
        let mut additional_batches: Vec<Vec<NoteKey>> = Vec::new();
        let mut quiet_since = Instant::now();
        let mut poll_iterations = 0u64;

        while Instant::now() < max_deadline {
            poll_iterations += 1;
            let mut got_events = false;

            if let Ok(mut stream_guard) = self.ndb_stream.write() {
                if let Some(stream) = stream_guard.as_mut() {
                    // Drain all immediately available events
                    while let Some(note_keys) = stream.next().now_or_never().flatten() {
                        additional_batches.push(note_keys);
                        got_events = true;
                    }
                }
            }

            if got_events {
                // Reset quiet timer - events are still arriving
                quiet_since = Instant::now();
            } else {
                // No events this iteration
                let quiet_duration = Instant::now().duration_since(quiet_since);
                if quiet_duration >= Duration::from_millis(REFRESH_QUIET_PERIOD_MS) {
                    // Been quiet for REFRESH_QUIET_PERIOD_MS, assume relay has finished sending
                    break;
                }
                // Sleep briefly before polling again
                std::thread::sleep(Duration::from_millis(REFRESH_POLL_INTERVAL_MS));
            }
        }
        let poll_elapsed_ms = poll_started_at.elapsed().as_millis();
        let additional_batch_count = additional_batches.len();
        let additional_note_key_count: usize =
            additional_batches.iter().map(|batch| batch.len()).sum();

        // Re-acquire store lock and process additional batches
        let mut store_guard = match self.store.write() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let store = match store_guard.as_mut() {
            Some(store) => store,
            None => return false,
        };

        let prefs_guard = match self.preferences.read() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let archived_ids = prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.clone())
            .unwrap_or_default();

        let callback = self.event_callback.read().ok().and_then(|g| g.clone());

        let additional_process_started_at = Instant::now();
        let mut deltas: Vec<DataChangeType> = Vec::new();
        for note_keys in additional_batches {
            if !note_keys.is_empty() {
                deltas.extend(process_note_keys_with_deltas(
                    ndb.as_ref(),
                    store,
                    &core_handle,
                    &note_keys,
                    &archived_ids,
                ));
            }
        }
        let additional_process_elapsed_ms = additional_process_started_at.elapsed().as_millis();

        append_snapshot_update_deltas(&mut deltas);
        let additional_delta_summary = summarize_deltas(&deltas);

        let additional_callback_started_at = Instant::now();
        let additional_callback_count = if callback.is_some() { deltas.len() } else { 0 };
        if let Some(ref cb) = callback {
            for delta in deltas {
                cb.on_data_changed(delta);
            }
        }
        let additional_callback_elapsed_ms = additional_callback_started_at.elapsed().as_millis();
        tlog!(
            "PERF",
            "ffi.refresh pass=additional pollIterations={} pollMs={} noteBatches={} noteKeys={} processMs={} callbackCount={} callbackMs={} deltas=[{}]",
            poll_iterations,
            poll_elapsed_ms,
            additional_batch_count,
            additional_note_key_count,
            additional_process_elapsed_ms,
            additional_callback_count,
            additional_callback_elapsed_ms,
            additional_delta_summary.compact()
        );

        // Preserve previous refresh semantics (full rebuild)
        let rebuild_started_at = Instant::now();
        store.rebuild_from_ndb();
        let rebuild_elapsed_ms = rebuild_started_at.elapsed().as_millis();

        // Update lock-free runtime cache while we still hold the store write lock
        let (runtime_ms, _, _) = store.get_statusbar_runtime_ms();
        self.cached_today_runtime_ms
            .store(runtime_ms, Ordering::Release);

        tlog!(
            "PERF",
            "ffi.refresh complete rebuildMs={} totalMs={}",
            rebuild_elapsed_ms,
            refresh_started_at.elapsed().as_millis()
        );
        ok
    }

    /// Force reconnection to relays and restart all subscriptions.
    ///
    /// This is used by pull-to-refresh to ensure fresh data is fetched from relays.
    /// Unlike `refresh()` which only drains pending events from the subscription stream,
    /// this method:
    /// 1. Disconnects from all relays
    /// 2. Reconnects with the same credentials
    /// 3. Restarts all subscriptions
    /// 4. Triggers a new negentropy sync
    ///
    /// This is useful when the app has been backgrounded and may have missed events,
    /// or when the user explicitly wants to ensure they have the latest data.
    ///
    /// Returns an error if not logged in or if reconnection fails.
    pub fn force_reconnect(&self) -> Result<(), TenexError> {
        use std::sync::mpsc::channel;

        // Check login state early to avoid unnecessary work
        if !self.is_logged_in() {
            return Err(TenexError::NotLoggedIn);
        }

        let core_handle = get_core_handle(&self.core_handle)?;

        // Create a channel to wait for the reconnect to complete
        let (response_tx, response_rx) = channel();

        core_handle
            .send(NostrCommand::ForceReconnect {
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send force reconnect command: {}", e),
            })?;

        // Wait for the reconnect to complete (with timeout)
        match response_rx.recv_timeout(std::time::Duration::from_secs(30)) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(e)) => Err(TenexError::Internal {
                message: format!("Force reconnect failed: {}", e),
            }),
            Err(std::sync::mpsc::RecvTimeoutError::Timeout) => Err(TenexError::Internal {
                message: "Force reconnect timed out after 30 seconds".to_string(),
            }),
            Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => Err(TenexError::Internal {
                message: "Force reconnect channel disconnected".to_string(),
            }),
        }
    }
}
