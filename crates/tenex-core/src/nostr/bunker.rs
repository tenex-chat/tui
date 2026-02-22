use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, SyncSender};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use nostr_connect::signer::{NostrConnectKeys, NostrConnectRemoteSigner, NostrConnectSignerActions};
use nostr_sdk::prelude::*;

use crate::constants::RELAY_URL;
use crate::tlog;

/// Information about a signing request, sent to the UI for approval.
#[derive(Debug, Clone)]
pub struct BunkerSignRequest {
    pub request_id: String,
    pub requester_pubkey: String,
    pub event_kind: Option<u16>,
    pub event_content: Option<String>,
    pub event_tags_json: Option<String>,
}

/// Bridge between the synchronous `NostrConnectSignerActions::approve` callback
/// and the async UI approval flow.
///
/// When `approve()` is called by the signer:
/// 1. For non-signing requests (Ping, GetPublicKey, Connect), auto-approve
/// 2. For SignEvent, create a sync_channel, store the sender, send the request
///    info to the UI via `request_tx`, then block on the receiver with a 60s timeout
struct BunkerApprovalBridge {
    /// Pending approval responses keyed by request_id
    pending: Arc<Mutex<HashMap<String, SyncSender<bool>>>>,
    /// Channel to send signing requests to the callback listener
    request_tx: std::sync::mpsc::Sender<BunkerSignRequest>,
    /// Flag to signal shutdown — approve() returns false immediately when set
    shutdown: Arc<AtomicBool>,
}

impl NostrConnectSignerActions for BunkerApprovalBridge {
    fn approve(&self, public_key: &PublicKey, req: &NostrConnectRequest) -> bool {
        if self.shutdown.load(Ordering::Relaxed) {
            return false;
        }

        match req {
            NostrConnectRequest::Ping
            | NostrConnectRequest::GetPublicKey
            | NostrConnectRequest::Connect { .. } => {
                tlog!("BUNKER", "auto-approve {:?} from {}", req_name(req), public_key);
                true
            }
            NostrConnectRequest::SignEvent(unsigned) => {
                let request_id = uuid::Uuid::new_v4().to_string();
                let (resp_tx, resp_rx) = mpsc::sync_channel::<bool>(1);

                // Store the response sender
                if let Ok(mut pending) = self.pending.lock() {
                    pending.insert(request_id.clone(), resp_tx);
                }

                let tags_json = serde_json::to_string(&unsigned.tags).unwrap_or_default();

                let sign_request = BunkerSignRequest {
                    request_id: request_id.clone(),
                    requester_pubkey: public_key.to_hex(),
                    event_kind: Some(unsigned.kind.as_u16()),
                    event_content: Some(unsigned.content.clone()),
                    event_tags_json: Some(tags_json),
                };

                tlog!(
                    "BUNKER",
                    "sign_event request id={} from={} kind={}",
                    request_id,
                    &public_key.to_hex()[..8],
                    unsigned.kind.as_u16()
                );

                // Send to UI
                if self.request_tx.send(sign_request).is_err() {
                    tlog!("BUNKER", "request_tx send failed, rejecting");
                    if let Ok(mut pending) = self.pending.lock() {
                        pending.remove(&request_id);
                    }
                    return false;
                }

                // Block waiting for user response (60s timeout)
                match resp_rx.recv_timeout(Duration::from_secs(60)) {
                    Ok(approved) => {
                        tlog!("BUNKER", "sign_event id={} approved={}", request_id, approved);
                        if let Ok(mut pending) = self.pending.lock() {
                            pending.remove(&request_id);
                        }
                        approved
                    }
                    Err(_) => {
                        tlog!("BUNKER", "sign_event id={} timed out, rejecting", request_id);
                        if let Ok(mut pending) = self.pending.lock() {
                            pending.remove(&request_id);
                        }
                        false
                    }
                }
            }
            // Encrypt/decrypt requests — auto-approve for now
            _ => {
                tlog!("BUNKER", "auto-approve {:?} from {}", req_name(req), public_key);
                true
            }
        }
    }
}

fn req_name(req: &NostrConnectRequest) -> &'static str {
    match req {
        NostrConnectRequest::Ping => "Ping",
        NostrConnectRequest::GetPublicKey => "GetPublicKey",
        NostrConnectRequest::Connect { .. } => "Connect",
        NostrConnectRequest::SignEvent(_) => "SignEvent",
        NostrConnectRequest::Nip04Encrypt { .. } => "Nip04Encrypt",
        NostrConnectRequest::Nip04Decrypt { .. } => "Nip04Decrypt",
        NostrConnectRequest::Nip44Encrypt { .. } => "Nip44Encrypt",
        NostrConnectRequest::Nip44Decrypt { .. } => "Nip44Decrypt",
    }
}

/// Manages the lifecycle of the NIP-46 bunker signer.
pub struct BunkerService {
    /// Pending approval responses — shared with the bridge
    pending: Arc<Mutex<HashMap<String, SyncSender<bool>>>>,
    /// Shutdown flag — shared with the bridge and serve thread
    shutdown: Arc<AtomicBool>,
    /// Handle to the dedicated OS thread running the signer
    thread_handle: Option<std::thread::JoinHandle<()>>,
    /// The bunker URI for clients to connect
    bunker_uri: String,
}

impl BunkerService {
    /// Start the bunker service.
    ///
    /// Spawns a dedicated OS thread with its own Tokio runtime that runs
    /// `NostrConnectRemoteSigner::serve()`. This avoids blocking the main
    /// worker's Tokio executor when `approve()` blocks waiting for user input.
    ///
    /// The user's keys are used as both the signer and user keypair, so the
    /// bunker URI will be `bunker://<user-pubkey>?relay=...&secret=...`.
    pub fn start(
        user_keys: Keys,
        request_tx: std::sync::mpsc::Sender<BunkerSignRequest>,
    ) -> Result<Self, String> {
        let connect_keys = NostrConnectKeys {
            signer: user_keys.clone(),
            user: user_keys,
        };

        let secret = uuid::Uuid::new_v4().to_string();

        let remote_signer = NostrConnectRemoteSigner::new(
            connect_keys,
            vec![RELAY_URL],
            Some(secret),
            None, // default relay options
        )
        .map_err(|e| format!("Failed to create remote signer: {}", e))?;

        let bunker_uri = remote_signer.bunker_uri().to_string();
        tlog!("BUNKER", "bunker URI: {}", bunker_uri);

        let pending: Arc<Mutex<HashMap<String, SyncSender<bool>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let shutdown = Arc::new(AtomicBool::new(false));

        let bridge = BunkerApprovalBridge {
            pending: pending.clone(),
            request_tx,
            shutdown: shutdown.clone(),
        };

        let shutdown_clone = shutdown.clone();
        let thread_handle = std::thread::Builder::new()
            .name("bunker-signer".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create bunker Tokio runtime");

                rt.block_on(async {
                    tlog!("BUNKER", "serve() started");
                    if let Err(e) = remote_signer.serve(bridge).await {
                        if !shutdown_clone.load(Ordering::Relaxed) {
                            tlog!("BUNKER", "serve() error: {}", e);
                        }
                    }
                    tlog!("BUNKER", "serve() ended");
                });
            })
            .map_err(|e| format!("Failed to spawn bunker thread: {}", e))?;

        Ok(Self {
            pending,
            shutdown,
            thread_handle: Some(thread_handle),
            bunker_uri,
        })
    }

    /// Get the bunker URI for clients to connect.
    pub fn bunker_uri(&self) -> &str {
        &self.bunker_uri
    }

    /// Respond to a pending signing request.
    pub fn respond(&self, request_id: &str, approved: bool) -> Result<(), String> {
        let sender = {
            let mut pending = self
                .pending
                .lock()
                .map_err(|e| format!("Lock poisoned: {}", e))?;
            pending
                .remove(request_id)
                .ok_or_else(|| format!("No pending request with id {}", request_id))?
        };

        sender
            .send(approved)
            .map_err(|_| "Receiver dropped (request may have timed out)".to_string())
    }

    /// Stop the bunker service.
    pub fn stop(&mut self) {
        tlog!("BUNKER", "stopping");
        self.shutdown.store(true, Ordering::SeqCst);

        // Reject all pending requests
        if let Ok(mut pending) = self.pending.lock() {
            for (id, sender) in pending.drain() {
                tlog!("BUNKER", "rejecting pending request {} on shutdown", id);
                let _ = sender.send(false);
            }
        }

        // The serve() loop will exit when the client is dropped or shutdown is detected.
        // We don't join the thread here to avoid blocking — it will clean up on its own.
        if let Some(handle) = self.thread_handle.take() {
            // Give it a brief moment to notice the shutdown
            std::thread::spawn(move || {
                let _ = handle.join();
            });
        }

        tlog!("BUNKER", "stopped");
    }
}

impl Drop for BunkerService {
    fn drop(&mut self) {
        if !self.shutdown.load(Ordering::Relaxed) {
            self.stop();
        }
    }
}
