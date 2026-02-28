use super::*;

#[uniffi::export]
impl TenexCore {
    /// Login with an nsec (Nostr secret key in bech32 format).
    ///
    /// The nsec should be in the format `nsec1...`.
    /// On success, stores the keys and triggers async relay connection.
    /// Login succeeds immediately even if relays are unreachable.
    pub fn login(&self, nsec: String) -> Result<LoginResult, TenexError> {
        let login_started_at = Instant::now();
        tlog!("PERF", "ffi.login start");
        // Parse the nsec into a SecretKey
        let parse_started_at = Instant::now();
        let secret_key = SecretKey::parse(&nsec).map_err(|e| {
            tlog!("ERROR", "ffi.login invalid nsec: {}", e);
            TenexError::InvalidNsec {
                message: e.to_string(),
            }
        })?;
        tlog!(
            "PERF",
            "ffi.login parsed secret key elapsedMs={}",
            parse_started_at.elapsed().as_millis()
        );

        // Create Keys from the secret key
        let keys = Keys::new(secret_key);

        // Get the public key in both formats
        let pubkey = keys.public_key().to_hex();
        let npub = keys
            .public_key()
            .to_bech32()
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to encode npub: {}", e),
            })?;

        // Store the keys immediately (authentication is local)
        let store_keys_started_at = Instant::now();
        {
            let mut keys_guard = self.keys.write().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire write lock: {}", e),
            })?;
            *keys_guard = Some(keys.clone());
        }
        tlog!(
            "PERF",
            "ffi.login stored keys elapsedMs={}",
            store_keys_started_at.elapsed().as_millis()
        );

        // Apply authenticated user context in one shared store path.
        let apply_user_started_at = Instant::now();
        {
            let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire store lock: {}", e),
            })?;
            if let Some(store) = store_guard.as_mut() {
                store.apply_authenticated_user(pubkey.clone());
            }
        }
        tlog!(
            "PERF",
            "ffi.login apply_authenticated_user elapsedMs={}",
            apply_user_started_at.elapsed().as_millis()
        );

        // Re-apply persisted backend trust after store rebuild/logout cycles.
        let trust_sync_started_at = Instant::now();
        self.sync_trusted_backends_from_preferences()?;
        tlog!(
            "PERF",
            "ffi.login sync_trusted_backends_from_preferences elapsedMs={}",
            trust_sync_started_at.elapsed().as_millis()
        );

        // Trigger async relay connection (non-blocking, fire-and-forget)
        let core_handle_started_at = Instant::now();
        let core_handle = {
            let handle_guard = self.core_handle.read().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire core handle lock: {}", e),
            })?;
            handle_guard
                .as_ref()
                .ok_or_else(|| TenexError::Internal {
                    message: "Core runtime not initialized - call init() first".to_string(),
                })?
                .clone()
        };
        tlog!(
            "PERF",
            "ffi.login resolved core handle elapsedMs={}",
            core_handle_started_at.elapsed().as_millis()
        );

        let send_connect_started_at = Instant::now();
        let relay_urls = self.get_configured_relays();
        let _ = core_handle.send(NostrCommand::Connect {
            keys,
            user_pubkey: pubkey.clone(),
            relay_urls,
            response_tx: None, // Don't wait for response
        });
        tlog!(
            "PERF",
            "ffi.login queued connect elapsedMs={}",
            send_connect_started_at.elapsed().as_millis()
        );

        tlog!(
            "PERF",
            "ffi.login complete totalMs={}",
            login_started_at.elapsed().as_millis()
        );
        Ok(LoginResult {
            pubkey,
            npub,
            success: true,
        })
    }

    /// Generate a fresh Nostr keypair.
    ///
    /// Pure function — no state changes, no login side effects.
    /// Returns nsec, npub, and hex pubkey for the caller to store as needed.
    pub fn generate_keypair(&self) -> Result<GeneratedKeypair, TenexError> {
        let keys = Keys::generate();

        let nsec = keys
            .secret_key()
            .to_bech32()
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to encode nsec: {}", e),
            })?;
        let npub = keys
            .public_key()
            .to_bech32()
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to encode npub: {}", e),
            })?;
        let pubkey_hex = keys.public_key().to_hex();

        Ok(GeneratedKeypair {
            nsec,
            npub,
            pubkey_hex,
        })
    }

    /// Publish a kind:0 profile metadata event for the logged-in user.
    ///
    /// Sets the user's display name and optionally a profile picture URL.
    /// Fire-and-forget — does not wait for relay confirmation.
    pub fn publish_profile(
        &self,
        name: String,
        picture_url: Option<String>,
    ) -> Result<(), TenexError> {
        let keys_guard = self.keys.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire keys lock: {}", e),
        })?;
        if keys_guard.is_none() {
            return Err(TenexError::NotLoggedIn);
        }
        drop(keys_guard);

        let core_handle = get_core_handle(&self.core_handle)?;
        core_handle
            .send(NostrCommand::PublishProfile { name, picture_url })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send publish profile command: {}", e),
            })?;

        Ok(())
    }

    /// Get information about the currently logged-in user.
    ///
    /// Returns None if not logged in.
    pub fn get_current_user(&self) -> Option<UserInfo> {
        let keys_guard = self.keys.read().ok()?;
        let keys = keys_guard.as_ref()?;

        let pubkey = keys.public_key().to_hex();
        let npub = keys.public_key().to_bech32().ok()?;

        Some(UserInfo {
            pubkey,
            npub,
            display_name: String::new(), // Empty for now, can be fetched from profile later
        })
    }

    /// Get profile picture URL for a pubkey from kind:0 metadata.
    ///
    /// Returns the picture URL if the profile exists and has a picture set.
    /// This fetches from cached kind:0 events, not the network.
    ///
    /// Returns Result to allow Swift to properly handle errors (lock failures, etc.)
    /// instead of silently returning nil.
    pub fn get_profile_picture(&self, pubkey: String) -> Result<Option<String>, TenexError> {
        let store_guard = self.store.read().map_err(|_| TenexError::LockError {
            resource: "store".to_string(),
        })?;

        let store = store_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;

        Ok(store.get_profile_picture(&pubkey))
    }

    /// Convert an npub (bech32) string to a hex pubkey string.
    /// Returns None if the input is not a valid npub.
    /// This is useful for converting authorNpub (which is bech32 format) to hex
    /// format needed by functions like get_profile_name.
    pub fn npub_to_hex(&self, npub: String) -> Option<String> {
        // Use nostr_sdk's PublicKey to parse the bech32 npub
        match PublicKey::from_bech32(&npub) {
            Ok(pk) => Some(pk.to_hex()),
            Err(_) => None,
        }
    }

    /// Get the display name for a pubkey.
    /// Returns the profile name if available, otherwise formats the pubkey as npub.
    pub fn get_profile_name(&self, pubkey: String) -> String {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return pubkey,
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return pubkey,
        };

        store.get_profile_name(&pubkey)
    }

    /// Check if a user is currently logged in.
    /// Returns true only if we have stored keys.
    pub fn is_logged_in(&self) -> bool {
        self.keys
            .read()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Logout the current user.
    /// Disconnects from relays, wipes local Nostr cache files, and resets session state.
    /// This prevents stale data from previous accounts from leaking to new logins.
    ///
    /// This method is deterministic - it waits for the disconnect to complete before
    /// clearing state, preventing race conditions with subsequent login attempts.
    ///
    /// Returns an error if the disconnect fails or times out. In that case, the
    /// session state is NOT cleared to avoid leaving a zombie relay session.
    pub fn logout(&self) -> Result<(), TenexError> {
        // Disconnect from relays first and WAIT for it to complete
        // This prevents race conditions if login is called immediately after
        let disconnect_result = if let Ok(handle_guard) = self.core_handle.read() {
            if let Some(handle) = handle_guard.as_ref() {
                let (response_tx, response_rx) = mpsc::channel::<Result<(), String>>();
                if handle
                    .send(NostrCommand::Disconnect {
                        response_tx: Some(response_tx),
                    })
                    .is_err()
                {
                    // Channel closed, worker already stopped - treat as success
                    eprintln!(
                        "[TENEX] logout: Worker channel closed, treating as already disconnected"
                    );
                    Ok(())
                } else {
                    // Wait for disconnect to complete (with timeout to avoid hanging forever)
                    match response_rx.recv_timeout(Duration::from_secs(5)) {
                        Ok(Ok(())) => {
                            eprintln!("[TENEX] logout: Disconnect confirmed");
                            Ok(())
                        }
                        Ok(Err(e)) => {
                            eprintln!("[TENEX] logout: Disconnect failed: {}", e);
                            Err(TenexError::LogoutFailed {
                                message: format!("Disconnect error: {}", e),
                            })
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            eprintln!("[TENEX] logout: Disconnect timed out after 5 seconds, forcing shutdown");
                            // On timeout, send Shutdown command and wait for worker thread to stop
                            let _ = handle.send(NostrCommand::Shutdown);
                            // Wait for worker thread to actually stop
                            let shutdown_success = if let Ok(mut worker_guard) =
                                self.worker_handle.write()
                            {
                                if let Some(worker_handle) = worker_guard.take() {
                                    let join_result = worker_handle.join();
                                    if join_result.is_ok() {
                                        eprintln!("[TENEX] logout: Worker thread joined after forced shutdown");
                                        true
                                    } else {
                                        eprintln!("[TENEX] logout: Worker thread join failed after shutdown");
                                        false
                                    }
                                } else {
                                    // No worker handle, assume it's already stopped
                                    true
                                }
                            } else {
                                eprintln!("[TENEX] logout: Could not acquire worker_handle lock during shutdown");
                                false
                            };

                            if shutdown_success {
                                // Worker confirmed stopped, safe to clear state
                                Ok(())
                            } else {
                                // Shutdown failed, return error and don't clear state
                                Err(TenexError::LogoutFailed {
                                    message: "Disconnect timed out and forced shutdown failed"
                                        .to_string(),
                                })
                            }
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => {
                            eprintln!("[TENEX] logout: Response channel disconnected, worker likely stopped");
                            Ok(()) // Worker stopped, treat as success
                        }
                    }
                }
            } else {
                // No handle means not logged in
                Ok(())
            }
        } else {
            // Lock error - cannot confirm disconnect, return error and don't clear state
            eprintln!(
                "[TENEX] logout: Could not acquire core_handle lock - cannot confirm disconnect"
            );
            Err(TenexError::LogoutFailed {
                message: "Could not acquire core_handle lock".to_string(),
            })
        };

        // If disconnect failed, bail out without clearing state.
        disconnect_result?;

        // Clear keys immediately.
        if let Ok(mut keys_guard) = self.keys.write() {
            *keys_guard = None;
        }

        // Rebuild runtime with a fresh on-disk cache so the next login starts clean.
        self.reset_runtime_after_logout()?;

        eprintln!("[TENEX] logout: Session state cleared and local cache wiped");
        Ok(())
    }
}

impl TenexCore {
    fn remove_file_if_exists(path: &std::path::Path) -> Result<(), TenexError> {
        match std::fs::remove_file(path) {
            Ok(()) => Ok(()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(e) => Err(TenexError::LogoutFailed {
                message: format!("Failed to remove {}: {}", path.display(), e),
            }),
        }
    }

    fn wipe_local_cache_files(&self) -> Result<(), TenexError> {
        let data_dir = get_data_dir();
        let state_cache_path = crate::store::state_cache::cache_path(&data_dir);
        let state_cache_tmp_path = state_cache_path.with_extension("bin.tmp");

        Self::remove_file_if_exists(&data_dir.join("data.mdb"))?;
        Self::remove_file_if_exists(&data_dir.join("lock.mdb"))?;
        Self::remove_file_if_exists(&state_cache_path)?;
        Self::remove_file_if_exists(&state_cache_tmp_path)?;
        Ok(())
    }

    fn reset_runtime_after_logout(&self) -> Result<(), TenexError> {
        let _txn_guard =
            self.ndb_transaction_lock
                .lock()
                .map_err(|_| TenexError::LogoutFailed {
                    message: "Failed to acquire transaction lock during logout".to_string(),
                })?;

        // Ensure worker drops all Ndb handles before wiping files.
        if let Ok(handle_guard) = self.core_handle.read() {
            if let Some(handle) = handle_guard.as_ref() {
                let _ = handle.send(NostrCommand::Shutdown);
            }
        }

        {
            let mut worker_guard =
                self.worker_handle
                    .write()
                    .map_err(|_| TenexError::LogoutFailed {
                        message: "Failed to acquire worker handle lock during logout".to_string(),
                    })?;
            if let Some(worker_handle) = worker_guard.take() {
                worker_handle.join().map_err(|_| TenexError::LogoutFailed {
                    message: "Failed to join worker thread during logout".to_string(),
                })?;
            }
        }

        {
            let mut handle_guard =
                self.core_handle
                    .write()
                    .map_err(|_| TenexError::LogoutFailed {
                        message: "Failed to clear core handle during logout".to_string(),
                    })?;
            *handle_guard = None;
        }
        {
            let mut data_rx_guard = self.data_rx.lock().map_err(|_| TenexError::LogoutFailed {
                message: "Failed to clear data receiver during logout".to_string(),
            })?;
            *data_rx_guard = None;
        }
        {
            let mut stream_guard =
                self.ndb_stream
                    .write()
                    .map_err(|_| TenexError::LogoutFailed {
                        message: "Failed to clear NostrDB stream during logout".to_string(),
                    })?;
            *stream_guard = None;
        }
        {
            let mut store_guard = self.store.write().map_err(|_| TenexError::LogoutFailed {
                message: "Failed to clear app store during logout".to_string(),
            })?;
            *store_guard = None;
        }
        {
            let mut ndb_guard = self.ndb.write().map_err(|_| TenexError::LogoutFailed {
                message: "Failed to clear NostrDB handle during logout".to_string(),
            })?;
            *ndb_guard = None;
        }
        {
            let mut stats_guard =
                self.subscription_stats
                    .write()
                    .map_err(|_| TenexError::LogoutFailed {
                        message: "Failed to clear subscription stats during logout".to_string(),
                    })?;
            *stats_guard = None;
        }
        {
            let mut stats_guard =
                self.negentropy_stats
                    .write()
                    .map_err(|_| TenexError::LogoutFailed {
                        message: "Failed to clear negentropy stats during logout".to_string(),
                    })?;
            *stats_guard = None;
        }

        self.cached_today_runtime_ms.store(0, Ordering::Release);
        self.last_refresh_ms.store(0, Ordering::Relaxed);

        self.wipe_local_cache_files()?;

        self.initialized.store(false, Ordering::SeqCst);
        if !self.init() {
            return Err(TenexError::LogoutFailed {
                message: "Failed to reinitialize core after logout".to_string(),
            });
        }

        Ok(())
    }
}
