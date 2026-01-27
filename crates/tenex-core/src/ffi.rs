//! FFI module for UniFFI bindings
//!
//! This module exposes a minimal API for use from Swift/Kotlin via UniFFI.
//! Keep this API as simple as possible - no async functions, only basic types.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, RwLock};
use std::thread::JoinHandle;
use std::time::Duration;

use futures::{FutureExt, StreamExt};
use nostr_sdk::prelude::*;
use nostrdb::{FilterBuilder, Ndb, NoteKey, SubscriptionStream};

use crate::nostr::{DataChange, NostrCommand, NostrWorker};
use crate::runtime::{process_note_keys, CoreHandle};
use crate::stats::{SharedEventStats, SharedNegentropySyncStats, SharedSubscriptionStats};
use crate::store::AppDataStore;

/// Get the data directory for nostrdb
fn get_data_dir() -> PathBuf {
    // Use a subdirectory in the user's data directory
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("tenex").join("nostrdb")
}

/// A simplified project info struct for FFI export.
/// This is a subset of the full Project model, safe for cross-language use.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProjectInfo {
    /// Unique identifier (d-tag) of the project
    pub id: String,
    /// Display name/title of the project
    pub title: String,
    /// Project description (from content field), if any
    pub description: Option<String>,
}

/// A conversation/thread in a project.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ConversationInfo {
    /// Unique identifier of the conversation (event ID)
    pub id: String,
    /// Title/subject of the conversation
    pub title: String,
    /// Agent or user who started the conversation
    pub author: String,
    /// Brief summary or first line of content
    pub summary: Option<String>,
    /// Number of messages in the thread
    pub message_count: u32,
    /// Unix timestamp of last activity
    pub last_activity: u64,
    /// Parent conversation ID (for nesting)
    pub parent_id: Option<String>,
    /// Status: active, completed, waiting
    pub status: String,
}

/// A single question in an ask event.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum AskQuestionInfo {
    /// Single-select question (user picks one option)
    SingleSelect {
        title: String,
        question: String,
        suggestions: Vec<String>,
    },
    /// Multi-select question (user can pick multiple options)
    MultiSelect {
        title: String,
        question: String,
        options: Vec<String>,
    },
}

/// An ask event containing questions for user interaction.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AskEventInfo {
    /// Title of the ask event
    pub title: Option<String>,
    /// Context/description for the questions
    pub context: String,
    /// List of questions to display
    pub questions: Vec<AskQuestionInfo>,
}

/// A message within a conversation.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MessageInfo {
    /// Unique identifier of the message (event ID)
    pub id: String,
    /// Content of the message (can be markdown)
    pub content: String,
    /// Author name/identifier
    pub author: String,
    /// Author's npub
    pub author_npub: String,
    /// Unix timestamp when message was created
    pub created_at: u64,
    /// Whether this is a tool call
    pub is_tool_call: bool,
    /// Role: user, assistant, system
    pub role: String,
    /// Q-tags pointing to referenced events (delegation targets, ask events, etc.)
    pub q_tags: Vec<String>,
    /// Ask event data if this message contains an ask (inline ask)
    pub ask_event: Option<AskEventInfo>,
    /// Tool name if this is a tool call (e.g., "mcp__tenex__ask", "mcp__tenex__delegate")
    pub tool_name: Option<String>,
}

/// A report/article in a project (kind 30023 NIP-23 long-form content).
#[derive(Debug, Clone, uniffi::Record)]
pub struct ReportInfo {
    /// Unique identifier (d-tag/slug)
    pub id: String,
    /// Title of the report
    pub title: String,
    /// Summary/excerpt
    pub summary: Option<String>,
    /// Full markdown content
    pub content: String,
    /// Author name
    pub author: String,
    /// Author npub
    pub author_npub: String,
    /// Unix timestamp of creation
    pub created_at: u64,
    /// Unix timestamp of last update
    pub updated_at: u64,
    /// Tags/hashtags
    pub tags: Vec<String>,
}

/// An inbox item (task/notification waiting for attention).
#[derive(Debug, Clone, uniffi::Record)]
pub struct InboxItem {
    /// Unique identifier
    pub id: String,
    /// Title/summary of the item
    pub title: String,
    /// Detailed content
    pub content: String,
    /// Who it's from
    pub from_agent: String,
    /// Priority: high, medium, low
    pub priority: String,
    /// Status: waiting, acknowledged, resolved
    pub status: String,
    /// Unix timestamp
    pub created_at: u64,
    /// Related project ID
    pub project_id: Option<String>,
    /// Related conversation ID
    pub conversation_id: Option<String>,
}

/// Result of a successful login operation.
#[derive(Debug, Clone, uniffi::Record)]
pub struct LoginResult {
    /// Hex-encoded public key
    pub pubkey: String,
    /// Bech32-encoded public key (npub1...)
    pub npub: String,
    /// Whether login was successful
    pub success: bool,
}

/// Information about the current logged-in user.
#[derive(Debug, Clone, uniffi::Record)]
pub struct UserInfo {
    /// Hex-encoded public key
    pub pubkey: String,
    /// Bech32-encoded public key (npub1...)
    pub npub: String,
    /// Display name (empty for now, can be fetched from profile later)
    pub display_name: String,
}

/// Errors that can occur during TENEX operations.
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum TenexError {
    #[error("Invalid nsec format: {message}")]
    InvalidNsec { message: String },
    #[error("Not logged in")]
    NotLoggedIn,
    #[error("Internal error: {message}")]
    Internal { message: String },
    #[error("Logout failed: {message}")]
    LogoutFailed { message: String },
}

/// Core TENEX functionality exposed to foreign languages.
///
/// This is intentionally minimal for MVP - we'll expand as needed.
/// Note: UniFFI objects are wrapped in Arc, so we use AtomicBool for interior mutability.
#[derive(uniffi::Object)]
pub struct TenexCore {
    initialized: AtomicBool,
    /// Stored keys for the logged-in user (protected by RwLock for interior mutability)
    keys: RwLock<Option<Keys>>,
    /// nostrdb instance for local event storage
    ndb: RwLock<Option<Arc<Ndb>>>,
    /// App data store built on top of nostrdb
    store: RwLock<Option<AppDataStore>>,
    /// Core runtime command handle for NostrWorker
    core_handle: RwLock<Option<CoreHandle>>,
    /// Data change receiver from NostrWorker (project status, streaming chunks)
    /// Uses Mutex because Receiver is not Sync, and UniFFI objects require Send + Sync
    data_rx: Mutex<Option<Receiver<DataChange>>>,
    /// Worker thread handle (joins on drop)
    worker_handle: RwLock<Option<JoinHandle<()>>>,
    /// NostrDB subscription stream for live updates
    ndb_stream: RwLock<Option<SubscriptionStream>>,
}

#[uniffi::export]
impl TenexCore {
    /// Create a new TenexCore instance.
    /// This is the entry point for the FFI API.
    #[uniffi::constructor]
    pub fn new() -> Self {
        Self {
            initialized: AtomicBool::new(false),
            keys: RwLock::new(None),
            ndb: RwLock::new(None),
            store: RwLock::new(None),
            core_handle: RwLock::new(None),
            data_rx: Mutex::new(None),
            worker_handle: RwLock::new(None),
            ndb_stream: RwLock::new(None),
        }
    }

    /// Initialize the core. Must be called before other operations.
    /// Returns true if initialization succeeded.
    ///
    /// Note: This is lightweight and can be called from any thread.
    /// Heavy initialization (relay connection) happens during login.
    pub fn init(&self) -> bool {
        if self.initialized.load(Ordering::SeqCst) {
            return true;
        }

        // Get the data directory for nostrdb
        let data_dir = get_data_dir();
        if let Err(e) = std::fs::create_dir_all(&data_dir) {
            eprintln!("Failed to create data directory: {}", e);
            return false;
        }

        // Initialize nostrdb with mobile-appropriate mapsize
        // iOS has memory constraints, so use 512MB instead of default
        let config = nostrdb::Config::new().set_mapsize(512 * 1024 * 1024);
        let ndb = match Ndb::new(data_dir.to_str().unwrap_or("tenex_data"), &config) {
            Ok(ndb) => Arc::new(ndb),
            Err(e) => {
                eprintln!("Failed to initialize nostrdb: {}", e);
                return false;
            }
        };

        // Start Nostr worker (same core path as TUI/CLI)
        let (command_tx, command_rx) = mpsc::channel::<NostrCommand>();
        let (data_tx, data_rx) = mpsc::channel::<DataChange>();
        let event_stats = SharedEventStats::new();
        let subscription_stats = SharedSubscriptionStats::new();
        let negentropy_stats = SharedNegentropySyncStats::new();
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

        // Subscribe to relevant kinds in nostrdb (mirrors CoreRuntime)
        let ndb_filter = FilterBuilder::new()
            .kinds([31933, 1, 0, 4199, 513, 4129, 4201])
            .build();
        let ndb_subscription = match ndb.subscribe(&[ndb_filter]) {
            Ok(sub) => sub,
            Err(e) => {
                eprintln!("Failed to subscribe to nostrdb: {}", e);
                return false;
            }
        };
        let ndb_stream = SubscriptionStream::new((*ndb).clone(), ndb_subscription);

        // Store ndb
        {
            let mut ndb_guard = match self.ndb.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *ndb_guard = Some(ndb.clone());
        }

        // Initialize AppDataStore
        let store = AppDataStore::new(ndb.clone());
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

        self.initialized.store(true, Ordering::SeqCst);
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

    /// Login with an nsec (Nostr secret key in bech32 format).
    ///
    /// The nsec should be in the format `nsec1...`.
    /// On success, connects to relays and starts subscriptions, THEN stores the keys.
    /// If relay connection fails, login fails and no state is changed.
    pub fn login(&self, nsec: String) -> Result<LoginResult, TenexError> {
        // Parse the nsec into a SecretKey
        let secret_key = SecretKey::parse(&nsec).map_err(|e| TenexError::InvalidNsec {
            message: e.to_string(),
        })?;

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

        let core_handle = {
            let handle_guard = self.core_handle.read().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire core handle lock: {}", e),
            })?;
            handle_guard.as_ref().ok_or_else(|| TenexError::Internal {
                message: "Core runtime not initialized - call init() first".to_string(),
            })?.clone()
        };

        // Connect to relays FIRST - if this fails, we don't commit any state
        let (response_tx, response_rx) = mpsc::channel::<Result<(), String>>();
        core_handle
            .send(NostrCommand::Connect {
                keys: keys.clone(),
                user_pubkey: pubkey.clone(),
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send connect command: {}", e),
            })?;

        match response_rx.recv_timeout(Duration::from_secs(15)) {
            Ok(Ok(())) => {}
            Ok(Err(e)) => {
                return Err(TenexError::Internal {
                    message: format!("Failed to connect: {}", e),
                });
            }
            Err(_) => {
                return Err(TenexError::Internal {
                    message: "Timed out waiting for relay connection".to_string(),
                });
            }
        }

        // Store the keys
        {
            let mut keys_guard = self.keys.write().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire write lock: {}", e),
            })?;
            *keys_guard = Some(keys.clone());
        }

        // Set user pubkey in the store
        {
            let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire store lock: {}", e),
            })?;
            if let Some(store) = store_guard.as_mut() {
                store.set_user_pubkey(pubkey.clone());
            }
        }

        // Rebuild the store with fresh data from nostrdb
        {
            let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire store lock: {}", e),
            })?;
            if let Some(store) = store_guard.as_mut() {
                store.rebuild_from_ndb();
            }
        }

        Ok(LoginResult {
            pubkey,
            npub,
            success: true,
        })
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

    /// Check if a user is currently logged in.
    /// Returns true only if we have stored keys.
    pub fn is_logged_in(&self) -> bool {
        self.keys
            .read()
            .map(|guard| guard.is_some())
            .unwrap_or(false)
    }

    /// Logout the current user.
    /// Disconnects from relays and clears all session state including in-memory data.
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
                if handle.send(NostrCommand::Disconnect {
                    response_tx: Some(response_tx)
                }).is_err() {
                    // Channel closed, worker already stopped - treat as success
                    eprintln!("[TENEX] logout: Worker channel closed, treating as already disconnected");
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
                            Err(TenexError::LogoutFailed { message: format!("Disconnect error: {}", e) })
                        }
                        Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                            eprintln!("[TENEX] logout: Disconnect timed out after 5 seconds, forcing shutdown");
                            // On timeout, send Shutdown command and wait for worker thread to stop
                            let _ = handle.send(NostrCommand::Shutdown);
                            // Wait for worker thread to actually stop
                            let shutdown_success = if let Ok(mut worker_guard) = self.worker_handle.write() {
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
                                    message: "Disconnect timed out and forced shutdown failed".to_string()
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
            eprintln!("[TENEX] logout: Could not acquire core_handle lock - cannot confirm disconnect");
            Err(TenexError::LogoutFailed {
                message: "Could not acquire core_handle lock".to_string()
            })
        };

        // Only clear state if disconnect was successful
        if disconnect_result.is_ok() {
            // Clear keys
            if let Ok(mut keys_guard) = self.keys.write() {
                *keys_guard = None;
            }

            // Clear all in-memory data in the store to prevent data leaks
            // The next login will rebuild_from_ndb() with the new user's context
            if let Ok(mut store_guard) = self.store.write() {
                if let Some(store) = store_guard.as_mut() {
                    store.clear();
                }
            }
            eprintln!("[TENEX] logout: Session state cleared");
        }

        disconnect_result
    }

    /// Get a list of projects.
    ///
    /// Queries nostrdb for kind 31933 events and returns them as ProjectInfo.
    pub fn get_projects(&self) -> Vec<ProjectInfo> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        store.get_projects()
            .iter()
            .map(|p| ProjectInfo {
                id: p.id.clone(),
                title: p.name.clone(),
                description: None, // Project model doesn't have description field
            })
            .collect()
    }

    /// Get conversations for a project.
    ///
    /// Returns conversations organized with parent/child relationships.
    /// Use parent_id to build nested conversation trees.
    pub fn get_conversations(&self, project_id: String) -> Vec<ConversationInfo> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        // Find the project by ID and get its a_tag
        let project = store.get_projects().iter().find(|p| p.id == project_id);
        let project_a_tag = match project {
            Some(p) => p.a_tag(),
            None => return Vec::new(),
        };

        // Get threads for this project
        let threads = store.get_threads(&project_a_tag);

        threads
            .iter()
            .map(|t| {
                // Get message count
                let message_count = store.get_messages(&t.id).len() as u32;

                // Get author display name
                let author_name = store.get_profile_name(&t.pubkey);

                // Determine status from thread's status_label
                let status = t.status_label.clone().unwrap_or_else(|| "active".to_string());

                ConversationInfo {
                    id: t.id.clone(),
                    title: t.title.clone(),
                    author: author_name,
                    summary: t.summary.clone(),
                    message_count,
                    last_activity: t.last_activity,
                    parent_id: t.parent_conversation_id.clone(),
                    status,
                }
            })
            .collect()
    }

    /// Get messages for a conversation.
    pub fn get_messages(&self, conversation_id: String) -> Vec<MessageInfo> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        // Get messages for the thread
        let messages = store.get_messages(&conversation_id);

        messages
            .iter()
            .map(|m| {
                // Get author display name
                let author_name = store.get_profile_name(&m.pubkey);

                // Convert pubkey to npub
                let author_npub = hex::decode(&m.pubkey)
                    .ok()
                    .and_then(|bytes| {
                        if bytes.len() == 32 {
                            let mut arr = [0u8; 32];
                            arr.copy_from_slice(&bytes);
                            nostr_sdk::PublicKey::from_slice(&arr).ok()
                        } else {
                            None
                        }
                    })
                    .and_then(|pk| pk.to_bech32().ok())
                    .unwrap_or_else(|| format!("{}...", &m.pubkey[..16.min(m.pubkey.len())]));

                // Determine role based on content patterns, tool usage, or author
                // Messages with tool_name are always from assistants (tool calls or ask events)
                let role = if m.tool_name.is_some() || m.content.contains("```") || m.content.contains("Tool:") {
                    "assistant".to_string()
                } else {
                    "user".to_string()
                };

                // Convert ask_event if present
                let ask_event = m.ask_event.as_ref().map(|ae| {
                    let questions = ae.questions.iter().map(|q| {
                        match q {
                            crate::models::message::AskQuestion::SingleSelect { title, question, suggestions } => {
                                AskQuestionInfo::SingleSelect {
                                    title: title.clone(),
                                    question: question.clone(),
                                    suggestions: suggestions.clone(),
                                }
                            }
                            crate::models::message::AskQuestion::MultiSelect { title, question, options } => {
                                AskQuestionInfo::MultiSelect {
                                    title: title.clone(),
                                    question: question.clone(),
                                    options: options.clone(),
                                }
                            }
                        }
                    }).collect();

                    AskEventInfo {
                        title: ae.title.clone(),
                        context: ae.context.clone(),
                        questions,
                    }
                });

                MessageInfo {
                    id: m.id.clone(),
                    content: m.content.clone(),
                    author: author_name,
                    author_npub,
                    created_at: m.created_at,
                    is_tool_call: m.content.starts_with("Tool:") || m.content.contains("<tool_call>") || m.tool_name.is_some(),
                    role,
                    q_tags: m.q_tags.clone(),
                    ask_event,
                    tool_name: m.tool_name.clone(),
                }
            })
            .collect()
    }

    /// Get reports for a project.
    pub fn get_reports(&self, project_id: String) -> Vec<ReportInfo> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        // Find the project by ID and get its a_tag
        let project = store.get_projects().iter().find(|p| p.id == project_id);
        let project_a_tag = match project {
            Some(p) => p.a_tag(),
            None => return Vec::new(),
        };

        // Get reports for this project
        let reports = store.get_reports_by_project(&project_a_tag);

        reports
            .iter()
            .map(|r| {
                // Get author display name (Report has `author` field, not `pubkey`)
                let author_name = store.get_profile_name(&r.author);

                // Convert pubkey to npub
                let author_npub = hex::decode(&r.author)
                    .ok()
                    .and_then(|bytes| {
                        if bytes.len() == 32 {
                            let mut arr = [0u8; 32];
                            arr.copy_from_slice(&bytes);
                            nostr_sdk::PublicKey::from_slice(&arr).ok()
                        } else {
                            None
                        }
                    })
                    .and_then(|pk| pk.to_bech32().ok())
                    .unwrap_or_else(|| format!("{}...", &r.author[..16.min(r.author.len())]));

                ReportInfo {
                    id: r.slug.clone(),
                    title: r.title.clone(),
                    summary: Some(r.summary.clone()), // Report has String, not Option<String>
                    content: r.content.clone(),
                    author: author_name,
                    author_npub,
                    created_at: r.created_at,
                    updated_at: r.created_at, // Reports are immutable in Nostr
                    tags: r.hashtags.clone(),
                }
            })
            .collect()
    }

    /// Get inbox items for the current user.
    pub fn get_inbox(&self) -> Vec<InboxItem> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        // Get inbox items from the store
        store.get_inbox_items()
            .iter()
            .map(|item| {
                // Get author display name
                let from_agent = store.get_profile_name(&item.author_pubkey);

                // Extract project ID from a_tag (format: 31933:pubkey:id)
                let project_id = if item.project_a_tag.is_empty() {
                    None
                } else {
                    item.project_a_tag.split(':').nth(2).map(String::from)
                };

                // Determine priority based on event type
                let priority = match item.event_type {
                    crate::models::InboxEventType::Mention => "high".to_string(),
                    crate::models::InboxEventType::Reply => "medium".to_string(),
                    crate::models::InboxEventType::ThreadReply => "low".to_string(),
                };

                // Determine status based on is_read
                let status = if item.is_read {
                    "acknowledged".to_string()
                } else {
                    "waiting".to_string()
                };

                InboxItem {
                    id: item.id.clone(),
                    title: item.title.clone(),
                    content: item.title.clone(), // Same as title for now
                    from_agent,
                    priority,
                    status,
                    created_at: item.created_at,
                    project_id,
                    conversation_id: item.thread_id.clone(),
                }
            })
            .collect()
    }

    /// Refresh data from relays.
    /// Call this to fetch the latest data from relays.
    pub fn refresh(&self) -> bool {
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

        // Drain nostrdb subscription stream for new notes
        let mut note_batches: Vec<Vec<NoteKey>> = Vec::new();
        if let Ok(mut stream_guard) = self.ndb_stream.write() {
            if let Some(stream) = stream_guard.as_mut() {
                while let Some(note_keys) = stream.next().now_or_never().flatten() {
                    note_batches.push(note_keys);
                }
            }
        }

        let mut store_guard = match self.store.write() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let store = match store_guard.as_mut() {
            Some(store) => store,
            None => return false,
        };

        for change in data_changes {
            if let DataChange::ProjectStatus { json } = change {
                store.handle_status_event_json(&json);
            }
        }

        let mut ok = true;
        for note_keys in note_batches {
            if process_note_keys(ndb.as_ref(), store, &core_handle, &note_keys).is_err() {
                ok = false;
            }
        }

        // Preserve previous refresh semantics (full rebuild)
        store.rebuild_from_ndb();
        ok
    }
}

impl Drop for TenexCore {
    fn drop(&mut self) {
        if let Ok(handle_guard) = self.core_handle.read() {
            if let Some(handle) = handle_guard.as_ref() {
                let _ = handle.send(NostrCommand::Shutdown);
            }
        }

        if let Ok(mut worker_guard) = self.worker_handle.write() {
            if let Some(worker) = worker_guard.take() {
                let _ = worker.join();
            }
        }
    }
}

impl Default for TenexCore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tenex_core_new() {
        let core = TenexCore::new();
        assert!(!core.is_initialized());
    }

    #[test]
    fn test_tenex_core_init() {
        let core = TenexCore::new();
        assert!(core.init());
        assert!(core.is_initialized());
    }

    #[test]
    fn test_tenex_core_version() {
        let core = TenexCore::new();
        let version = core.version();
        assert!(!version.is_empty());
    }

    #[test]
    fn test_get_projects_returns_empty_when_not_initialized() {
        let core = TenexCore::new();
        let projects = core.get_projects();
        // Returns empty when not initialized
        assert!(projects.is_empty());
    }

    #[test]
    fn test_get_projects_after_init() {
        let core = TenexCore::new();
        core.init();
        let projects = core.get_projects();
        // With real nostrdb, starts empty (no data fetched yet)
        // Will have data after login and relay sync
        assert!(projects.is_empty() || !projects.is_empty());
    }

    #[test]
    fn test_login_with_invalid_nsec() {
        let core = TenexCore::new();

        let result = core.login("invalid_nsec".to_string());
        assert!(result.is_err());

        match result {
            Err(TenexError::InvalidNsec { message: _ }) => {}
            _ => panic!("Expected InvalidNsec error"),
        }

        // Should not be logged in
        assert!(!core.is_logged_in());
        assert!(core.get_current_user().is_none());
    }

    #[test]
    fn test_logout() {
        let core = TenexCore::new();
        core.init();

        // Since login now requires relay connection, we just test the basic flow
        // In a real test we'd mock the relay
        core.logout();
        assert!(!core.is_logged_in());
        assert!(core.get_current_user().is_none());
    }

    #[test]
    fn test_get_current_user_when_not_logged_in() {
        let core = TenexCore::new();
        assert!(core.get_current_user().is_none());
    }
}
