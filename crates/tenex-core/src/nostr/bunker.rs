use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Instant;

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

/// Audit log entry for a NIP-46 interaction.
#[derive(Debug, Clone)]
pub struct BunkerAuditEntry {
    pub timestamp_ms: u64,
    pub completed_at_ms: u64,
    pub request_id: String,
    pub source_event_id: String,
    pub requester_pubkey: String,
    pub request_type: String,
    pub event_kind: Option<u16>,
    pub event_content_preview: Option<String>,
    pub event_content_full: Option<String>,
    pub event_tags_json: Option<String>,
    pub request_payload_json: Option<String>,
    pub response_payload_json: Option<String>,
    pub decision: String,
    pub response_time_ms: u64,
}

/// A rule that auto-approves signing requests without prompting the UI.
#[derive(Debug, Clone)]
pub struct BunkerAutoApproveRule {
    pub requester_pubkey: String,
    pub event_kind: Option<u16>, // None = any kind from this pubkey
}

/// Manages the lifecycle of the NIP-46 bunker signer.
///
/// Replaces the `nostr-connect` library's `serve()` with our own loop
/// using `RelayPool` directly. This gives us full control over shutdown:
/// the serve loop uses `tokio::select!` between relay notifications and
/// a `watch` channel, so `stop()` actually terminates the thread.
pub struct BunkerService {
    shutdown_tx: tokio::sync::watch::Sender<bool>,
    pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
    thread_handle: Option<std::thread::JoinHandle<()>>,
    bunker_uri: String,
    audit_log: Arc<Mutex<Vec<BunkerAuditEntry>>>,
    auto_approve_rules: Arc<Mutex<Vec<BunkerAutoApproveRule>>>,
}

impl BunkerService {
    /// Start the bunker service.
    ///
    /// Spawns a dedicated OS thread with its own Tokio runtime that runs
    /// our NIP-46 serve loop. The loop handles incoming `Kind::NostrConnect`
    /// events, decrypts them, processes NIP-46 requests, and sends responses.
    ///
    /// SignEvent requests are forwarded to the UI for approval via `request_tx`.
    /// All other request types (Connect, Ping, GetPublicKey, encrypt/decrypt)
    /// are auto-approved.
    pub fn start(
        user_keys: Keys,
        request_tx: std::sync::mpsc::Sender<BunkerSignRequest>,
    ) -> Result<Self, String> {
        let secret = uuid::Uuid::new_v4().to_string();

        let relay_url =
            RelayUrl::parse(RELAY_URL).map_err(|e| format!("Failed to parse relay URL: {}", e))?;

        let bunker_uri = NostrConnectURI::Bunker {
            remote_signer_public_key: user_keys.public_key(),
            relays: vec![relay_url.clone()],
            secret: Some(secret.clone()),
        }
        .to_string();

        tlog!("BUNKER", "bunker URI: {}", bunker_uri);

        let pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>> =
            Arc::new(Mutex::new(HashMap::new()));
        let audit_log: Arc<Mutex<Vec<BunkerAuditEntry>>> = Arc::new(Mutex::new(Vec::new()));
        let auto_approve_rules: Arc<Mutex<Vec<BunkerAutoApproveRule>>> =
            Arc::new(Mutex::new(Vec::new()));

        let (shutdown_tx, shutdown_rx) = tokio::sync::watch::channel(false);

        let pending_clone = pending.clone();
        let audit_log_clone = audit_log.clone();
        let auto_approve_rules_clone = auto_approve_rules.clone();

        let thread_handle = std::thread::Builder::new()
            .name("bunker-signer".to_string())
            .spawn(move || {
                let rt = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .expect("Failed to create bunker Tokio runtime");

                rt.block_on(async {
                    if let Err(e) = serve_loop(
                        user_keys,
                        relay_url,
                        secret,
                        shutdown_rx,
                        pending_clone,
                        audit_log_clone,
                        auto_approve_rules_clone,
                        request_tx,
                    )
                    .await
                    {
                        tlog!("BUNKER", "serve loop error: {}", e);
                    }
                    tlog!("BUNKER", "serve() ended");
                });
            })
            .map_err(|e| format!("Failed to spawn bunker thread: {}", e))?;

        Ok(Self {
            shutdown_tx,
            pending,
            thread_handle: Some(thread_handle),
            bunker_uri,
            audit_log,
            auto_approve_rules,
        })
    }

    /// Get the bunker URI for clients to connect.
    pub fn bunker_uri(&self) -> &str {
        &self.bunker_uri
    }

    /// Get the audit log entries.
    pub fn audit_log(&self) -> Vec<BunkerAuditEntry> {
        self.audit_log
            .lock()
            .map(|log| log.clone())
            .unwrap_or_default()
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

    /// Add an auto-approve rule.
    pub fn add_auto_approve_rule(&self, rule: BunkerAutoApproveRule) {
        if let Ok(mut rules) = self.auto_approve_rules.lock() {
            // Don't add duplicates
            let exists = rules.iter().any(|r| {
                r.requester_pubkey == rule.requester_pubkey && r.event_kind == rule.event_kind
            });
            if !exists {
                tlog!(
                    "BUNKER",
                    "added auto-approve rule: pubkey={} kind={:?}",
                    &rule.requester_pubkey[..8.min(rule.requester_pubkey.len())],
                    rule.event_kind
                );
                rules.push(rule);
            }
        }
    }

    /// Remove an auto-approve rule.
    pub fn remove_auto_approve_rule(&self, requester_pubkey: &str, event_kind: Option<u16>) {
        if let Ok(mut rules) = self.auto_approve_rules.lock() {
            rules.retain(|r| {
                !(r.requester_pubkey == requester_pubkey && r.event_kind == event_kind)
            });
        }
    }

    /// Get all auto-approve rules.
    pub fn auto_approve_rules(&self) -> Vec<BunkerAutoApproveRule> {
        self.auto_approve_rules
            .lock()
            .map(|rules| rules.clone())
            .unwrap_or_default()
    }

    /// Stop the bunker service.
    pub fn stop(&mut self) {
        tlog!("BUNKER", "stopping");

        // Signal the serve loop to exit
        let _ = self.shutdown_tx.send(true);

        // Reject all pending requests
        if let Ok(mut pending) = self.pending.lock() {
            for (id, sender) in pending.drain() {
                tlog!("BUNKER", "rejecting pending request {} on shutdown", id);
                let _ = sender.send(false);
            }
        }

        // Wait for the thread to exit (it will because shutdown_rx fires in select!)
        if let Some(handle) = self.thread_handle.take() {
            let _ = handle.join();
        }

        tlog!("BUNKER", "stopped");
    }
}

impl Drop for BunkerService {
    fn drop(&mut self) {
        if !*self.shutdown_tx.borrow() {
            self.stop();
        }
    }
}

/// The NIP-46 serve loop. Replaces `NostrConnectRemoteSigner::serve()`.
///
/// 1. Creates a `RelayPool`, adds the relay, connects, subscribes to NostrConnect events
/// 2. Runs a `tokio::select!` loop: recv relay notifications OR shutdown signal
/// 3. On event: decrypt → parse NIP-46 message → handle request → send response
/// 4. On shutdown: break and clean up the pool
async fn serve_loop(
    keys: Keys,
    relay_url: RelayUrl,
    secret: String,
    mut shutdown_rx: tokio::sync::watch::Receiver<bool>,
    pending: Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
    audit_log: Arc<Mutex<Vec<BunkerAuditEntry>>>,
    auto_approve_rules: Arc<Mutex<Vec<BunkerAutoApproveRule>>>,
    request_tx: std::sync::mpsc::Sender<BunkerSignRequest>,
) -> Result<(), String> {
    let pool = RelayPool::default();

    pool.add_relay(&relay_url, RelayOptions::default())
        .await
        .map_err(|e| format!("Failed to add relay: {}", e))?;

    pool.connect().await;

    let filter = Filter::new()
        .pubkey(keys.public_key())
        .kind(Kind::NostrConnect)
        .since(Timestamp::now());

    pool.subscribe(filter, SubscribeOptions::default())
        .await
        .map_err(|e| format!("Failed to subscribe: {}", e))?;

    tlog!("BUNKER", "serve() started");

    let mut notifications = pool.notifications();

    loop {
        tokio::select! {
            result = notifications.recv() => {
                match result {
                    Ok(RelayPoolNotification::Event { event, .. }) => {
                        if event.kind == Kind::NostrConnect {
                            handle_nostr_connect_event(
                                &keys,
                                &secret,
                                &event,
                                &pool,
                                &pending,
                                &audit_log,
                                &auto_approve_rules,
                                &request_tx,
                            ).await;
                        }
                    }
                    Ok(_) => {} // Ignore other notification types
                    Err(e) => {
                        tlog!("BUNKER", "notification recv error: {}, resubscribing", e);
                        // Lagged behind - resubscribe
                        notifications = pool.notifications();
                    }
                }
            }
            _ = shutdown_rx.changed() => {
                tlog!("BUNKER", "shutdown signal received");
                break;
            }
        }
    }

    pool.shutdown().await;

    Ok(())
}

/// Check if a NIP-46 message JSON is a connect request (used for fallback handling).
fn is_connect_method(msg_json: &str) -> bool {
    serde_json::from_str::<serde_json::Value>(msg_json)
        .ok()
        .and_then(|v| v.get("method")?.as_str().map(|s| s == "connect"))
        .unwrap_or(false)
}

/// Extract secret param from connect message JSON (params[1]).
fn extract_connect_secret(msg_json: &str) -> Option<String> {
    serde_json::from_str::<serde_json::Value>(msg_json)
        .ok()
        .and_then(|v| {
            v.get("params")?
                .as_array()?
                .get(1)?
                .as_str()
                .map(|s| s.to_string())
        })
}

/// Handle a single NIP-46 NostrConnect event.
async fn handle_nostr_connect_event(
    keys: &Keys,
    secret: &str,
    event: &Event,
    pool: &RelayPool,
    pending: &Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
    audit_log: &Arc<Mutex<Vec<BunkerAuditEntry>>>,
    auto_approve_rules: &Arc<Mutex<Vec<BunkerAutoApproveRule>>>,
    request_tx: &std::sync::mpsc::Sender<BunkerSignRequest>,
) {
    let start = Instant::now();
    let received_at_ms = Timestamp::now().as_secs() * 1000;
    let source_event_id = event.id.to_hex();

    // Decrypt the message
    let msg_json = match nip44::decrypt(keys.secret_key(), &event.pubkey, &event.content) {
        Ok(json) => json,
        Err(e) => {
            tlog!(
                "BUNKER",
                "decrypt failed from {}: {}",
                &event.pubkey.to_hex()[..8],
                e
            );
            return;
        }
    };

    // Parse NIP-46 message
    let msg = match NostrConnectMessage::from_json(&msg_json) {
        Ok(m) => m,
        Err(e) => {
            tlog!("BUNKER", "parse message failed: {}", e);
            let elapsed_ms = start.elapsed().as_millis() as u64;
            if let Ok(mut log) = audit_log.lock() {
                log.push(BunkerAuditEntry {
                    timestamp_ms: received_at_ms,
                    completed_at_ms: received_at_ms.saturating_add(elapsed_ms),
                    request_id: "<unparsed>".to_string(),
                    source_event_id: source_event_id.clone(),
                    requester_pubkey: event.pubkey.to_hex(),
                    request_type: "ParseError".to_string(),
                    event_kind: None,
                    event_content_preview: None,
                    event_content_full: None,
                    event_tags_json: None,
                    request_payload_json: Some(msg_json.clone()),
                    response_payload_json: None,
                    decision: "error".to_string(),
                    response_time_ms: elapsed_ms,
                });
            }
            return;
        }
    };

    let id = msg.id().to_string();

    // Try to parse as a request. If that fails, handle known methods with
    // malformed params (e.g. connect with empty pubkey from some backends).
    let parsed = msg.to_request();

    let requester_pubkey = event.pubkey.to_hex();
    let mut request_type = "Unknown".to_string();
    let mut event_kind: Option<u16> = None;
    let mut event_content_preview: Option<String> = None;
    let mut event_content_full: Option<String> = None;
    let mut event_tags_json: Option<String> = None;
    let request_payload_json = Some(msg_json.clone());
    let response: NostrConnectResponse;
    let decision: String;

    match parsed {
        Ok(req) => {
            request_type = req_name(&req).to_string();

            // Extract event details for audit log
            let details = match &req {
                NostrConnectRequest::SignEvent(unsigned) => {
                    let preview = if unsigned.content.len() > 200 {
                        Some(format!("{}...", &unsigned.content[..200]))
                    } else {
                        Some(unsigned.content.clone())
                    };
                    let tags = serde_json::to_string(&unsigned.tags).ok();
                    (
                        Some(unsigned.kind.as_u16()),
                        preview,
                        Some(unsigned.content.clone()),
                        tags,
                    )
                }
                _ => (None, None, None, None),
            };
            event_kind = details.0;
            event_content_preview = details.1;
            event_content_full = details.2;
            event_tags_json = details.3;

            let result = process_request(
                keys,
                secret,
                &event.pubkey,
                &req,
                pending,
                auto_approve_rules,
                request_tx,
            )
            .await;
            response = result.0;
            decision = result.1;
        }
        Err(e) => {
            // Fallback: handle connect with empty/invalid pubkey param
            if is_connect_method(&msg_json) {
                tlog!("BUNKER", "connect fallback id={}", id);
                request_type = "Connect".to_string();
                event_kind = None;
                event_content_preview = None;
                event_tags_json = None;

                let client_secret = extract_connect_secret(&msg_json);
                // Accept connect if: secret matches, or no secret was sent
                // (the NIP-44 encryption already authenticates the sender)
                if client_secret.is_none() || match_secret(secret, client_secret.as_deref()) {
                    response = NostrConnectResponse::with_result(ResponseResult::Ack);
                    decision = "auto-approved".to_string();
                } else {
                    response = NostrConnectResponse::with_error("Secret not match");
                    decision = "error".to_string();
                }
            } else {
                tlog!("BUNKER", "unsupported request id={}: {}", id, e);
                request_type = "Unsupported".to_string();
                response = NostrConnectResponse::with_error(format!("Unsupported request: {}", e));
                decision = "error".to_string();
            }
        }
    };

    let elapsed_ms = start.elapsed().as_millis() as u64;
    let completed_at_ms = received_at_ms.saturating_add(elapsed_ms);

    // Build response payload JSON for audit/export.
    let response_msg = NostrConnectMessage::response(id.clone(), response);
    let response_payload_json = Some(response_msg.as_json());

    // Log to audit trail
    if let Ok(mut log) = audit_log.lock() {
        log.push(BunkerAuditEntry {
            timestamp_ms: received_at_ms,
            completed_at_ms,
            request_id: id.clone(),
            source_event_id: source_event_id.clone(),
            requester_pubkey: requester_pubkey.clone(),
            request_type: request_type.clone(),
            event_kind,
            event_content_preview,
            event_content_full,
            event_tags_json,
            request_payload_json,
            response_payload_json,
            decision: decision.clone(),
            response_time_ms: elapsed_ms,
        });
    }

    // Build and send response event
    match EventBuilder::nostr_connect(keys, event.pubkey, response_msg) {
        Ok(builder) => match builder.sign_with_keys(keys) {
            Ok(response_event) => {
                if let Err(e) = pool.send_event(&response_event).await {
                    tlog!("BUNKER", "send response failed: {}", e);
                }
            }
            Err(e) => tlog!("BUNKER", "sign response failed: {}", e),
        },
        Err(e) => tlog!("BUNKER", "build response failed: {}", e),
    }

    tlog!(
        "BUNKER",
        "{} from {} -> {} ({}ms)",
        request_type,
        &requester_pubkey[..8],
        decision,
        elapsed_ms
    );
}

/// Process a NIP-46 request and return the response + decision string.
async fn process_request(
    keys: &Keys,
    secret: &str,
    requester_pubkey: &PublicKey,
    req: &NostrConnectRequest,
    pending: &Arc<Mutex<HashMap<String, tokio::sync::oneshot::Sender<bool>>>>,
    auto_approve_rules: &Arc<Mutex<Vec<BunkerAutoApproveRule>>>,
    request_tx: &std::sync::mpsc::Sender<BunkerSignRequest>,
) -> (NostrConnectResponse, String) {
    match req {
        NostrConnectRequest::Connect {
            remote_signer_public_key,
            secret: client_secret,
        } => {
            if *remote_signer_public_key != keys.public_key() {
                return (
                    NostrConnectResponse::with_error("Remote signer public key not match"),
                    "error".to_string(),
                );
            }
            // Accept if: secret matches, or no secret was sent by client
            // (NIP-44 encryption already authenticates the sender)
            if client_secret.is_some() && !match_secret(secret, client_secret.as_deref()) {
                return (
                    NostrConnectResponse::with_error("Secret not match"),
                    "error".to_string(),
                );
            }
            (
                NostrConnectResponse::with_result(ResponseResult::Ack),
                "auto-approved".to_string(),
            )
        }
        NostrConnectRequest::Ping => (
            NostrConnectResponse::with_result(ResponseResult::Pong),
            "auto-approved".to_string(),
        ),
        NostrConnectRequest::GetPublicKey => (
            NostrConnectResponse::with_result(ResponseResult::GetPublicKey(keys.public_key())),
            "auto-approved".to_string(),
        ),
        NostrConnectRequest::Nip04Encrypt { public_key, text } => {
            match nip04::encrypt(keys.secret_key(), public_key, text) {
                Ok(ciphertext) => (
                    NostrConnectResponse::with_result(ResponseResult::Nip04Encrypt { ciphertext }),
                    "auto-approved".to_string(),
                ),
                Err(e) => (
                    NostrConnectResponse::with_error(e.to_string()),
                    "error".to_string(),
                ),
            }
        }
        NostrConnectRequest::Nip04Decrypt {
            public_key,
            ciphertext,
        } => match nip04::decrypt(keys.secret_key(), public_key, ciphertext) {
            Ok(plaintext) => (
                NostrConnectResponse::with_result(ResponseResult::Nip04Decrypt { plaintext }),
                "auto-approved".to_string(),
            ),
            Err(e) => (
                NostrConnectResponse::with_error(e.to_string()),
                "error".to_string(),
            ),
        },
        NostrConnectRequest::Nip44Encrypt { public_key, text } => {
            match nip44::encrypt(
                keys.secret_key(),
                public_key,
                text,
                nip44::Version::default(),
            ) {
                Ok(ciphertext) => (
                    NostrConnectResponse::with_result(ResponseResult::Nip44Encrypt { ciphertext }),
                    "auto-approved".to_string(),
                ),
                Err(e) => (
                    NostrConnectResponse::with_error(e.to_string()),
                    "error".to_string(),
                ),
            }
        }
        NostrConnectRequest::Nip44Decrypt {
            public_key,
            ciphertext,
        } => match nip44::decrypt(keys.secret_key(), public_key, ciphertext) {
            Ok(plaintext) => (
                NostrConnectResponse::with_result(ResponseResult::Nip44Decrypt { plaintext }),
                "auto-approved".to_string(),
            ),
            Err(e) => (
                NostrConnectResponse::with_error(e.to_string()),
                "error".to_string(),
            ),
        },
        NostrConnectRequest::SignEvent(unsigned) => {
            // Check auto-approve rules before prompting UI
            let pubkey_hex = requester_pubkey.to_hex();
            let kind_u16 = unsigned.kind.as_u16();
            let auto_approved = auto_approve_rules
                .lock()
                .map(|rules| {
                    rules.iter().any(|r| {
                        r.requester_pubkey == pubkey_hex
                            && (r.event_kind.is_none() || r.event_kind == Some(kind_u16))
                    })
                })
                .unwrap_or(false);

            if auto_approved {
                tlog!(
                    "BUNKER",
                    "auto-approved SignEvent kind={} from {}",
                    kind_u16,
                    &pubkey_hex[..8]
                );
                return match unsigned.clone().sign_with_keys(keys) {
                    Ok(signed) => (
                        NostrConnectResponse::with_result(ResponseResult::SignEvent(Box::new(
                            signed,
                        ))),
                        "auto-approved".to_string(),
                    ),
                    Err(e) => (
                        NostrConnectResponse::with_error(e.to_string()),
                        "error".to_string(),
                    ),
                };
            }

            let request_id = uuid::Uuid::new_v4().to_string();
            let (resp_tx, resp_rx) = tokio::sync::oneshot::channel::<bool>();

            // Store the response sender
            if let Ok(mut map) = pending.lock() {
                map.insert(request_id.clone(), resp_tx);
            }

            let tags_json = serde_json::to_string(&unsigned.tags).unwrap_or_default();

            let sign_request = BunkerSignRequest {
                request_id: request_id.clone(),
                requester_pubkey: requester_pubkey.to_hex(),
                event_kind: Some(unsigned.kind.as_u16()),
                event_content: Some(unsigned.content.clone()),
                event_tags_json: Some(tags_json),
            };

            // Send to UI
            if request_tx.send(sign_request).is_err() {
                if let Ok(mut map) = pending.lock() {
                    map.remove(&request_id);
                }
                return (
                    NostrConnectResponse::with_error("Rejected"),
                    "rejected".to_string(),
                );
            }

            // Wait for user response with 60s timeout
            match tokio::time::timeout(std::time::Duration::from_secs(60), resp_rx).await {
                Ok(Ok(true)) => {
                    // Approved - sign the event
                    match unsigned.clone().sign_with_keys(keys) {
                        Ok(signed) => (
                            NostrConnectResponse::with_result(ResponseResult::SignEvent(Box::new(
                                signed,
                            ))),
                            "approved".to_string(),
                        ),
                        Err(e) => (
                            NostrConnectResponse::with_error(e.to_string()),
                            "error".to_string(),
                        ),
                    }
                }
                Ok(Ok(false)) => {
                    // Rejected by user
                    (
                        NostrConnectResponse::with_error("Rejected"),
                        "rejected".to_string(),
                    )
                }
                Ok(Err(_)) => {
                    // Channel closed (shutdown)
                    (
                        NostrConnectResponse::with_error("Rejected"),
                        "rejected".to_string(),
                    )
                }
                Err(_) => {
                    // Timeout
                    if let Ok(mut map) = pending.lock() {
                        map.remove(&request_id);
                    }
                    (
                        NostrConnectResponse::with_error("Timeout"),
                        "timed-out".to_string(),
                    )
                }
            }
        }
    }
}

fn match_secret(our_secret: &str, their_secret: Option<&str>) -> bool {
    match their_secret {
        Some(s) => s == our_secret,
        None => false,
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
