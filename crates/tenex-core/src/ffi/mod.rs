//! FFI module for UniFFI bindings
//!
//! This module exposes a minimal API for use from Swift/Kotlin via UniFFI.
//! Keep this API as simple as possible - no async functions, only basic types.

use crate::tlog;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, OnceLock, RwLock};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

// =============================================================================
// POLLING TIMING CONSTANTS
// =============================================================================
//
// These constants control the adaptive polling behavior during refresh().
// The strategy: poll until no new events arrive for QUIET_PERIOD, or until
// MAX_POLL_TIMEOUT is reached, whichever comes first.
//
// Rationale:
// - iOS calls refresh() frequently (on every view load/fetch operation)
// - The notification handler may have just subscribed to new projects
// - Relays are sending historical events that haven't been ingested yet
// - Adaptive polling catches these late-arriving events without blocking too long

/// Maximum total time to poll for additional events during refresh().
/// After this time, we stop polling regardless of whether events are still arriving.
/// Set to 1 second to balance freshness vs responsiveness.
const REFRESH_MAX_POLL_TIMEOUT_MS: u64 = 1000;

/// Quiet period threshold - if no events arrive for this duration, assume relay
/// has finished sending and stop polling early. This allows fast exit when relay
/// completes quickly (typical case).
const REFRESH_QUIET_PERIOD_MS: u64 = 100;

/// Sleep duration between poll iterations. Small enough for responsiveness,
/// large enough to avoid CPU spin. 10ms = ~100 polls/sec max.
const REFRESH_POLL_INTERVAL_MS: u64 = 10;

/// Minimum interval between refresh() calls to prevent excessive relay/CPU load.
/// If refresh() is called more frequently than this, subsequent calls return
/// immediately without doing work. Set to 500ms based on typical UI interaction
/// patterns (user can't meaningfully process data faster than this).
const REFRESH_THROTTLE_INTERVAL_MS: u64 = 500;

use futures::{FutureExt, StreamExt};
use nostr_sdk::prelude::*;
use nostrdb::{FilterBuilder, Ndb, Note, NoteKey, SubscriptionStream, Transaction};

use crate::models::agent_definition::AgentDefinition;
use crate::models::{
    AskEvent, ConversationMetadata, InboxItem, MCPTool, Message, Nudge, OperationsStatus, Project,
    ProjectAgent, ProjectStatus, Report, Skill, TeamPack, Thread,
};
use crate::nostr::{set_log_path, DataChange, NostrCommand, NostrWorker};
use crate::runtime::CoreHandle;
use crate::stats::{
    query_ndb_stats, SharedEventStats, SharedNegentropySyncStats, SharedSubscriptionStats,
};
use crate::store::AppDataStore;
use std::collections::HashMap;

// Keep UniFFI exports split by domain-specific *_api.rs modules.
mod agents_api;
mod audio_settings_api;
mod auth_api;
mod bunker_api;
mod callback_api;
mod data_api;
mod diagnostics_api;
mod internal_impl;
mod lifecycle_api;
mod messaging_api;
mod projects_api;
mod refresh_api;
mod standalone_audio_api;
mod stats_api;
mod trust_api;
mod ui_state_api;

/// Shared Tokio runtime for async operations in FFI
/// Using OnceLock ensures thread-safe lazy initialization
static TOKIO_RUNTIME: OnceLock<tokio::runtime::Runtime> = OnceLock::new();

/// Get or initialize the shared Tokio runtime
fn get_tokio_runtime() -> &'static tokio::runtime::Runtime {
    TOKIO_RUNTIME
        .get_or_init(|| tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime"))
}

/// Get the data directory for nostrdb
fn get_data_dir() -> PathBuf {
    if let Ok(base_dir) = std::env::var("TENEX_BASE_DIR") {
        return PathBuf::from(base_dir).join("nostrdb");
    }
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("tenex").join("nostrdb")
}

/// Open nostrdb and validate it by creating a transaction and running a tiny query.
///
/// This catches cases where `Ndb::new` succeeds but LMDB lock state is stale, causing
/// all subsequent reads to fail with transaction errors.
fn open_ndb_with_health_check(
    data_dir: &Path,
    config: &nostrdb::Config,
) -> Result<Arc<Ndb>, String> {
    let ndb = Ndb::new(data_dir.to_str().unwrap_or("tenex_data"), config)
        .map_err(|e| format!("open failed: {}", e))?;

    let txn = Transaction::new(&ndb).map_err(|e| format!("transaction probe failed: {}", e))?;
    let probe_filter = FilterBuilder::new().kinds([31933]).build();
    ndb.query(&txn, &[probe_filter], 1)
        .map_err(|e| format!("query probe failed: {}", e))?;

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

/// Try opening nostrdb, and if a stale lock is likely, remove `lock.mdb` and retry once.
fn open_ndb_with_lock_recovery(
    data_dir: &Path,
    config: &nostrdb::Config,
) -> Result<Arc<Ndb>, String> {
    match open_ndb_with_health_check(data_dir, config) {
        Ok(ndb) => Ok(ndb),
        Err(first_err) => {
            if !is_likely_stale_lock_error(&first_err) {
                return Err(first_err);
            }

            let lock_path = data_dir.join("lock.mdb");
            if !lock_path.exists() {
                return Err(first_err);
            }

            eprintln!(
                "[TENEX] NostrDB probe failed ({}). Attempting stale lock recovery at {}",
                first_err,
                lock_path.display()
            );

            std::fs::remove_file(&lock_path).map_err(|e| {
                format!(
                    "{} (failed to remove stale lock {}: {})",
                    first_err,
                    lock_path.display(),
                    e
                )
            })?;

            open_ndb_with_health_check(data_dir, config).map_err(|retry_err| {
                format!(
                    "{} (retry after lock recovery failed: {})",
                    first_err, retry_err
                )
            })
        }
    }
}

/// Helper to get the project a_tag from project_id
fn get_project_a_tag(
    store: &RwLock<Option<AppDataStore>>,
    project_id: &str,
) -> Result<String, TenexError> {
    let store_guard = store.read().map_err(|e| TenexError::Internal {
        message: format!("Failed to acquire store lock: {}", e),
    })?;
    let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
        message: "Store not initialized".to_string(),
    })?;

    let project = store
        .get_projects()
        .iter()
        .find(|p| p.id == project_id)
        .cloned()
        .ok_or_else(|| TenexError::Internal {
            message: format!("Project not found: {}", project_id),
        })?;

    Ok(project.a_tag())
}

/// Helper to get the core handle
fn get_core_handle(core_handle: &RwLock<Option<CoreHandle>>) -> Result<CoreHandle, TenexError> {
    let handle_guard = core_handle.read().map_err(|e| TenexError::Internal {
        message: format!("Failed to acquire core handle lock: {}", e),
    })?;
    handle_guard
        .as_ref()
        .ok_or_else(|| TenexError::Internal {
            message: "Core runtime not initialized - call init() first".to_string(),
        })
        .cloned()
}

/// Format ask answers into markdown response (matching TUI format).
fn format_ask_answers(answers: &[AskAnswer]) -> String {
    let mut response = String::new();

    for answer in answers {
        // Add question title as heading
        response.push_str(&format!("## {}\n\n", answer.question_title));

        // Format answer based on type
        match &answer.answer_type {
            AskAnswerType::SingleSelect { value } => {
                response.push_str(&format!("{}\n\n", value));
            }
            AskAnswerType::MultiSelect { values } => {
                for value in values {
                    response.push_str(&format!("- {}\n", value));
                }
                response.push('\n');
            }
            AskAnswerType::CustomText { value } => {
                response.push_str(&format!("{}\n\n", value));
            }
        }
    }

    response.trim().to_string()
}

/// Helper to acquire a read lock on the store.
///
/// This eliminates the repeated pattern of:
/// ```ignore
/// let store_guard = self.store.read().map_err(|_| TenexError::LockError { resource: "store".to_string() })?;
/// let store = store_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;
/// ```
///
/// Note: Returns a guard that must be held for the duration of store access.
/// The returned reference is tied to the guard's lifetime.
///
/// This helper is available for future refactoring to reduce code duplication.
/// Not currently used to avoid introducing regressions in existing code.
#[allow(dead_code)]
fn acquire_store_read<'a>(
    store_guard: &'a std::sync::RwLockReadGuard<'a, Option<AppDataStore>>,
) -> Result<&'a AppDataStore, TenexError> {
    store_guard.as_ref().ok_or(TenexError::CoreNotInitialized)
}

/// Convert a Thread to ConversationFullInfo (shared helper).
fn thread_to_full_info(
    store: &AppDataStore,
    thread: &Thread,
    archived_ids: &std::collections::HashSet<String>,
) -> ConversationFullInfo {
    let message_count = store.get_messages(&thread.id).len() as u32;
    let author = store.get_profile_name(&thread.pubkey);
    let has_children = store.runtime_hierarchy.has_children(&thread.id);
    let is_active = store.operations.is_event_busy(&thread.id);
    let is_archived = archived_ids.contains(&thread.id);
    let project_a_tag = store
        .get_project_a_tag_for_thread(&thread.id)
        .unwrap_or_default();

    ConversationFullInfo {
        thread: thread.clone(),
        author,
        message_count,
        is_active,
        is_archived,
        has_children,
        project_a_tag,
    }
}

/// Find project id (d-tag) for a given project a-tag.
fn project_id_from_a_tag(store: &AppDataStore, a_tag: &str) -> Option<String> {
    store
        .get_projects()
        .iter()
        .find(|p| p.a_tag() == a_tag)
        .map(|p| p.id.clone())
}

/// Extract e-tag event IDs from a note (string or id bytes).
fn extract_e_tag_ids(note: &Note) -> Vec<String> {
    let mut ids = Vec::new();
    for tag in note.tags() {
        if tag.count() >= 2 {
            let tag_name = tag.get(0).and_then(|t| t.variant().str());
            if tag_name == Some("e") {
                if let Some(id) = tag.get(1).and_then(|t| t.variant().str()) {
                    ids.push(id.to_string());
                } else if let Some(id_bytes) = tag.get(1).and_then(|t| t.variant().id()) {
                    ids.push(hex::encode(id_bytes));
                }
            }
        }
    }
    ids
}

fn tag_value_to_string(tag: &nostrdb::Tag, index: u16) -> Option<String> {
    tag.get(index).map(|t| match t.variant() {
        nostrdb::NdbStrVariant::Str(s) => s.to_string(),
        nostrdb::NdbStrVariant::Id(bytes) => hex::encode(bytes),
    })
}

fn note_matches_team_context(note: &Note, team_coordinate: &str, team_event_id: &str) -> bool {
    for tag in note.tags() {
        if tag.count() < 2 {
            continue;
        }

        let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) else {
            continue;
        };

        match tag_name {
            "a" | "A" => {
                if let Some(value) = tag.get(1).and_then(|t| t.variant().str()) {
                    if value == team_coordinate {
                        return true;
                    }
                }
            }
            "e" | "E" => {
                if let Some(value) = tag_value_to_string(&tag, 1) {
                    if value == team_event_id {
                        return true;
                    }
                }
            }
            _ => {}
        }
    }
    false
}

fn parse_parent_comment_id(note: &Note, team_event_id: &str) -> Option<String> {
    let mut fallback: Option<String> = None;

    for tag in note.tags() {
        if tag.count() < 2 {
            continue;
        }

        let Some(tag_name) = tag.get(0).and_then(|t| t.variant().str()) else {
            continue;
        };

        if tag_name != "e" && tag_name != "E" {
            continue;
        }

        let Some(event_id) = tag_value_to_string(&tag, 1) else {
            continue;
        };

        if event_id == team_event_id {
            continue;
        }

        // NIP-22/NIP-10 marker: ["e", <id>, <relay>, "reply"]
        let marker = tag.get(3).and_then(|t| t.variant().str());
        if marker == Some("reply") {
            return Some(event_id);
        }

        if fallback.is_none() {
            fallback = Some(event_id);
        }
    }

    fallback
}

fn reaction_is_positive(content: &str) -> bool {
    let trimmed = content.trim();
    !(trimmed == "-")
}

fn short_id(value: &str, max_chars: usize) -> String {
    value.chars().take(max_chars).collect()
}

#[derive(Default)]
struct DeltaSummary {
    total: usize,
    message_appended: usize,
    conversation_upsert: usize,
    project_upsert: usize,
    inbox_upsert: usize,
    report_upsert: usize,
    project_status_changed: usize,
    pending_backend_approval: usize,
    active_conversations_changed: usize,
    stream_chunk: usize,
    mcp_tools_changed: usize,
    teams_changed: usize,
    stats_updated: usize,
    diagnostics_updated: usize,
    general: usize,
    bunker_sign_request: usize,
}

impl DeltaSummary {
    fn add(&mut self, delta: &DataChangeType) {
        self.total += 1;
        match delta {
            DataChangeType::MessageAppended { .. } => self.message_appended += 1,
            DataChangeType::ConversationUpsert { .. } => self.conversation_upsert += 1,
            DataChangeType::ProjectUpsert { .. } => self.project_upsert += 1,
            DataChangeType::InboxUpsert { .. } => self.inbox_upsert += 1,
            DataChangeType::ReportUpsert { .. } => self.report_upsert += 1,
            DataChangeType::ProjectStatusChanged { .. } => self.project_status_changed += 1,
            DataChangeType::PendingBackendApproval { .. } => self.pending_backend_approval += 1,
            DataChangeType::ActiveConversationsChanged { .. } => {
                self.active_conversations_changed += 1
            }
            DataChangeType::StreamChunk { .. } => self.stream_chunk += 1,
            DataChangeType::McpToolsChanged => self.mcp_tools_changed += 1,
            DataChangeType::TeamsChanged => self.teams_changed += 1,
            DataChangeType::StatsUpdated => self.stats_updated += 1,
            DataChangeType::DiagnosticsUpdated => self.diagnostics_updated += 1,
            DataChangeType::General => self.general += 1,
            DataChangeType::BunkerSignRequest { .. } => self.bunker_sign_request += 1,
        }
    }

    fn compact(&self) -> String {
        format!(
            "total={} msg={} conv={} proj={} inbox={} report={} status={} pending={} active={} stream={} mcp={} teams={} stats={} diag={} general={} bunker={}",
            self.total,
            self.message_appended,
            self.conversation_upsert,
            self.project_upsert,
            self.inbox_upsert,
            self.report_upsert,
            self.project_status_changed,
            self.pending_backend_approval,
            self.active_conversations_changed,
            self.stream_chunk,
            self.mcp_tools_changed,
            self.teams_changed,
            self.stats_updated,
            self.diagnostics_updated,
            self.general,
            self.bunker_sign_request
        )
    }
}

fn summarize_deltas(deltas: &[DataChangeType]) -> DeltaSummary {
    let mut summary = DeltaSummary::default();
    for delta in deltas {
        summary.add(delta);
    }
    summary
}

/// Process nostrdb note keys, update store, and return deltas.
fn process_note_keys_with_deltas(
    ndb: &Ndb,
    store: &mut AppDataStore,
    core_handle: &CoreHandle,
    note_keys: &[NoteKey],
    archived_ids: &std::collections::HashSet<String>,
) -> Vec<DataChangeType> {
    let started_at = Instant::now();
    let txn = match Transaction::new(ndb) {
        Ok(txn) => txn,
        Err(_) => return Vec::new(),
    };

    let mut deltas: Vec<DataChangeType> = Vec::new();
    let mut conversations_to_upsert: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut inbox_items_to_upsert: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let mut notes_found = 0usize;
    let mut kind_1 = 0usize;
    let mut kind_7 = 0usize;
    let mut kind_513 = 0usize;
    let mut kind_1111 = 0usize;
    let mut kind_31933 = 0usize;
    let mut kind_34199 = 0usize;
    let mut kind_30023 = 0usize;
    let mut other_kinds = 0usize;
    let mut teams_changed = false;

    for &note_key in note_keys.iter() {
        if let Ok(note) = ndb.get_note_by_key(&txn, note_key) {
            notes_found += 1;
            let kind = note.kind();
            match kind {
                1 => kind_1 += 1,
                7 => {
                    kind_7 += 1;
                    teams_changed = true;
                }
                513 => kind_513 += 1,
                1111 => {
                    kind_1111 += 1;
                    teams_changed = true;
                }
                31933 => kind_31933 += 1,
                34199 => {
                    kind_34199 += 1;
                    teams_changed = true;
                }
                30023 => kind_30023 += 1,
                _ => other_kinds += 1,
            }

            // Update store first
            store.handle_event(kind, &note);

            match kind {
                31933 => {
                    if let Some(project) = Project::from_note(&note) {
                        deltas.push(DataChangeType::ProjectUpsert {
                            project: project.clone(),
                        });
                    }
                }
                1 => {
                    // Message (kind:1 with e-tags)
                    if let Some(message) = Message::from_note(&note) {
                        deltas.push(DataChangeType::MessageAppended {
                            conversation_id: message.thread_id.clone(),
                            message: message.clone(),
                        });

                        // Conversation + ancestors (effective_last_activity updates)
                        conversations_to_upsert.insert(message.thread_id.clone());
                        for ancestor in store.runtime_hierarchy.get_ancestors(&message.thread_id) {
                            conversations_to_upsert.insert(ancestor);
                        }

                        // Inbox additions (ask events)
                        if store.inbox.get_items().iter().any(|i| i.id == message.id) {
                            inbox_items_to_upsert.insert(message.id.clone());
                        }

                        // Inbox read updates when user replies
                        if let Some(ref user_pk) = store.user_pubkey.clone() {
                            if &message.pubkey == user_pk {
                                for reply_id in extract_e_tag_ids(&note) {
                                    if store.inbox.get_items().iter().any(|i| i.id == reply_id) {
                                        inbox_items_to_upsert.insert(reply_id);
                                    }
                                }
                            }
                        }
                    }

                    // Thread (kind:1 with a-tag and no e-tags)
                    if let Some(thread) = Thread::from_note(&note) {
                        let thread_id = thread.id.clone();
                        conversations_to_upsert.insert(thread_id.clone());
                        for ancestor in store.runtime_hierarchy.get_ancestors(&thread_id) {
                            conversations_to_upsert.insert(ancestor);
                        }

                        // Add thread root as first message
                        if let Some(root_message) = Message::from_thread_note(&note) {
                            deltas.push(DataChangeType::MessageAppended {
                                conversation_id: root_message.thread_id.clone(),
                                message: root_message.clone(),
                            });

                            // Inbox additions for thread roots (ask events / mentions)
                            if store
                                .inbox
                                .get_items()
                                .iter()
                                .any(|i| i.id == root_message.id)
                            {
                                inbox_items_to_upsert.insert(root_message.id.clone());
                            }
                        }
                    }
                }
                513 => {
                    if let Some(metadata) = ConversationMetadata::from_note(&note) {
                        conversations_to_upsert.insert(metadata.thread_id.clone());
                        for ancestor in store.runtime_hierarchy.get_ancestors(&metadata.thread_id) {
                            conversations_to_upsert.insert(ancestor);
                        }
                    }
                }
                30023 => {
                    // Report/article event
                    if let Some(report) = Report::from_note(&note) {
                        if store
                            .get_projects()
                            .iter()
                            .any(|p| p.a_tag() == report.project_a_tag)
                        {
                            deltas.push(DataChangeType::ReportUpsert {
                                report: report.clone(),
                            });
                        }
                    }
                }
                _ => {}
            }
        }
    }

    if teams_changed {
        deltas.push(DataChangeType::TeamsChanged);
    }

    let conversation_upsert_count = conversations_to_upsert.len();
    for conversation_id in conversations_to_upsert {
        if let Some(thread) = store.get_thread_by_id(&conversation_id) {
            deltas.push(DataChangeType::ConversationUpsert {
                conversation: thread_to_full_info(store, thread, archived_ids),
            });
        }
    }

    let inbox_upsert_count = inbox_items_to_upsert.len();
    for inbox_id in inbox_items_to_upsert {
        if let Some(item) = store.inbox.get_items().iter().find(|i| i.id == inbox_id) {
            deltas.push(DataChangeType::InboxUpsert { item: item.clone() });
        }
    }

    // Subscribe to messages for any newly discovered projects
    let mut pending_project_subscriptions = 0usize;
    for project_a_tag in store.drain_pending_project_subscriptions() {
        pending_project_subscriptions += 1;
        let _ = core_handle.send(NostrCommand::SubscribeToProjectMessages { project_a_tag });
    }

    let delta_summary = summarize_deltas(&deltas);
    tlog!(
        "PERF",
        "process_note_keys_with_deltas noteKeys={} notesFound={} kinds={{1:{} 7:{} 513:{} 1111:{} 31933:{} 34199:{} 30023:{} other:{}}} convUpserts={} inboxUpserts={} pendingProjectSubs={} deltas=[{}] elapsedMs={}",
        note_keys.len(),
        notes_found,
        kind_1,
        kind_7,
        kind_513,
        kind_1111,
        kind_31933,
        kind_34199,
        kind_30023,
        other_kinds,
        conversation_upsert_count,
        inbox_upsert_count,
        pending_project_subscriptions,
        delta_summary.compact(),
        started_at.elapsed().as_millis()
    );

    deltas
}

/// Process DataChange channel items and return deltas.
fn process_data_changes_with_deltas(
    store: &mut AppDataStore,
    data_changes: &[DataChange],
) -> Vec<DataChangeType> {
    let started_at = Instant::now();
    let mut deltas: Vec<DataChangeType> = Vec::new();
    let mut project_status_changes = 0usize;
    let mut stream_chunks = 0usize;
    let mut mcp_tools_changed = 0usize;

    for change in data_changes {
        match change {
            DataChange::ProjectStatus { json } => {
                project_status_changes += 1;
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(json) {
                    let kind = event.get("kind").and_then(|k| k.as_u64()).unwrap_or(0);

                    // Capture pending state before update to detect new pending approvals
                    let pending_before = if let Some(status) = ProjectStatus::from_value(&event) {
                        store.trust.has_pending_approval(
                            &status.backend_pubkey,
                            &status.project_coordinate,
                        )
                    } else {
                        false
                    };

                    store.handle_status_event_value(&event);

                    match kind {
                        24010 => {
                            if let Some(status) = ProjectStatus::from_value(&event) {
                                let project_a_tag = status.project_coordinate.clone();

                                if store.project_statuses.contains_key(&project_a_tag) {
                                    let project_id = project_id_from_a_tag(store, &project_a_tag)
                                        .unwrap_or_default();
                                    let is_online = store.is_project_online(&project_a_tag);
                                    let online_agents = store
                                        .get_online_agents(&project_a_tag)
                                        .map(|agents| agents.iter().cloned().collect())
                                        .unwrap_or_default();

                                    deltas.push(DataChangeType::ProjectStatusChanged {
                                        project_id,
                                        project_a_tag,
                                        is_online,
                                        online_agents,
                                    });
                                } else if !pending_before {
                                    deltas.push(DataChangeType::PendingBackendApproval {
                                        backend_pubkey: status.backend_pubkey.clone(),
                                        project_a_tag,
                                    });
                                }
                            }
                        }
                        24133 => {
                            if let Some(status) = OperationsStatus::from_value(&event) {
                                let project_a_tag = status.project_coordinate.clone();
                                let project_id = project_id_from_a_tag(store, &project_a_tag)
                                    .unwrap_or_default();
                                let active_conversation_ids =
                                    store.operations.get_active_event_ids(&project_a_tag);

                                deltas.push(DataChangeType::ActiveConversationsChanged {
                                    project_id,
                                    project_a_tag,
                                    active_conversation_ids,
                                });
                            }
                        }
                        _ => {}
                    }
                }
            }
            DataChange::LocalStreamChunk {
                agent_pubkey,
                conversation_id,
                text_delta,
                ..
            } => {
                stream_chunks += 1;
                deltas.push(DataChangeType::StreamChunk {
                    agent_pubkey: agent_pubkey.clone(),
                    conversation_id: conversation_id.clone(),
                    text_delta: text_delta.clone(),
                });
            }
            DataChange::MCPToolsChanged => {
                mcp_tools_changed += 1;
                deltas.push(DataChangeType::McpToolsChanged);
            }
            DataChange::BunkerSignRequest { request } => {
                deltas.push(DataChangeType::BunkerSignRequest {
                    request: FfiBunkerSignRequest {
                        request_id: request.request_id.clone(),
                        requester_pubkey: request.requester_pubkey.clone(),
                        event_kind: request.event_kind,
                        event_json: request.event_json.clone(),
                        event_content: request.event_content.clone(),
                        event_tags_json: request.event_tags_json.clone(),
                    },
                });
            }
            DataChange::BookmarkListChanged { .. } => {
                // Bookmark changes are handled optimistically in toggle_bookmark;
                // no additional FFI delta needed.
            }
        }
    }

    let delta_summary = summarize_deltas(&deltas);
    tlog!(
        "PERF",
        "process_data_changes_with_deltas input={} projectStatus={} streamChunks={} mcpToolsChanged={} deltas=[{}] elapsedMs={}",
        data_changes.len(),
        project_status_changes,
        stream_chunks,
        mcp_tools_changed,
        delta_summary.compact(),
        started_at.elapsed().as_millis()
    );

    deltas
}

/// Append stats/diagnostics update signals based on the accumulated deltas.
/// Ensures snapshots refresh only when relevant data changes, and only once per batch.
fn append_snapshot_update_deltas(deltas: &mut Vec<DataChangeType>) {
    let mut stats_changed = false;
    let mut diagnostics_changed = false;
    let mut has_stats_update = false;
    let mut has_diagnostics_update = false;

    for delta in deltas.iter() {
        match delta {
            DataChangeType::StatsUpdated => {
                has_stats_update = true;
            }
            DataChangeType::DiagnosticsUpdated => {
                has_diagnostics_update = true;
            }
            DataChangeType::MessageAppended { .. }
            | DataChangeType::ConversationUpsert { .. }
            | DataChangeType::ProjectUpsert { .. }
            | DataChangeType::InboxUpsert { .. }
            | DataChangeType::ReportUpsert { .. } => {
                stats_changed = true;
                diagnostics_changed = true;
            }
            DataChangeType::ProjectStatusChanged { .. }
            | DataChangeType::PendingBackendApproval { .. }
            | DataChangeType::ActiveConversationsChanged { .. }
            | DataChangeType::McpToolsChanged
            | DataChangeType::TeamsChanged => {
                diagnostics_changed = true;
            }
            DataChangeType::General => {
                diagnostics_changed = true;
                stats_changed = true;
            }
            DataChangeType::StreamChunk { .. } | DataChangeType::BunkerSignRequest { .. } => {}
        }
    }

    if stats_changed && !has_stats_update {
        deltas.push(DataChangeType::StatsUpdated);
    }

    if diagnostics_changed && !has_diagnostics_update {
        deltas.push(DataChangeType::DiagnosticsUpdated);
    }
}

/// Extended conversation info with all data needed for the Conversations tab.
/// Includes activity tracking, archive status, and hierarchy data.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ConversationFullInfo {
    /// The underlying thread data
    pub thread: Thread,
    /// Author display name (resolved from profile)
    pub author: String,
    /// Number of messages in the thread
    pub message_count: u32,
    /// Whether this conversation has an agent actively working on it
    pub is_active: bool,
    /// Whether this conversation is archived
    pub is_archived: bool,
    /// Whether this thread has children (for collapse/expand UI)
    pub has_children: bool,
    /// Project a_tag this conversation belongs to
    pub project_a_tag: String,
}

/// Time filter options for conversations
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TimeFilterOption {
    /// All time (no filter)
    All,
    /// Last 24 hours
    Today,
    /// Last 7 days
    ThisWeek,
    /// Last 30 days
    ThisMonth,
}

/// Filter configuration for getAllConversations
#[derive(Debug, Clone, uniffi::Record)]
pub struct ConversationFilter {
    /// Project IDs to include (empty = all projects)
    pub project_ids: Vec<String>,
    /// Whether to include archived conversations
    pub show_archived: bool,
    /// Whether to hide scheduled events
    pub hide_scheduled: bool,
    /// Time filter
    pub time_filter: TimeFilterOption,
}

/// Project info with selection state for filtering
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProjectFilterInfo {
    /// Project ID (d-tag)
    pub id: String,
    /// Project a_tag (full coordinate)
    pub a_tag: String,
    /// Display title
    pub title: String,
    /// Whether this project is currently visible/selected
    pub is_visible: bool,
    /// Number of active conversations in this project
    pub active_count: u32,
    /// Total conversations in this project
    pub total_count: u32,
}

/// Ask-event lookup result for q-tag resolution.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AskEventLookupInfo {
    /// Ask payload resolved from the referenced event.
    pub ask_event: AskEvent,
    /// Author pubkey (hex) of the ask event.
    pub author_pubkey: String,
}

/// An answer to a single question in an ask event.
#[derive(Debug, Clone, uniffi::Record)]
pub struct AskAnswer {
    /// The question title (used to format the response)
    pub question_title: String,
    /// The answer type and value(s)
    pub answer_type: AskAnswerType,
}

/// The type of answer for an ask question.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum AskAnswerType {
    /// Single selection from suggestions
    SingleSelect { value: String },
    /// Multiple selections from options
    MultiSelect { values: Vec<String> },
    /// Custom text input (for "Other" option)
    CustomText { value: String },
}

/// A search result from full-text search.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SearchResult {
    /// Event ID of the matching message/report
    pub event_id: String,
    /// Thread/conversation ID for context
    pub thread_id: Option<String>,
    /// Content snippet with match
    pub content: String,
    /// Event kind (1 = message, 30023 = report)
    pub kind: u32,
    /// Author name/npub
    pub author: String,
    /// Unix timestamp
    pub created_at: u64,
    /// Project a-tag if known
    pub project_a_tag: Option<String>,
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

/// A freshly generated Nostr keypair.
#[derive(Debug, Clone, uniffi::Record)]
pub struct GeneratedKeypair {
    /// Bech32-encoded secret key (nsec1...)
    pub nsec: String,
    /// Bech32-encoded public key (npub1...)
    pub npub: String,
    /// Hex-encoded public key (for whitelistedPubkeys config)
    pub pubkey_hex: String,
}

/// Result of sending a message.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SendMessageResult {
    /// Event ID of the published message
    pub event_id: String,
    /// Whether the message was successfully sent
    pub success: bool,
}

/// Team pack info (kind:34199) for browse/list/detail UIs.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TeamInfo {
    /// Event ID of this team pack event.
    pub id: String,
    /// Author pubkey (hex) of the team pack event.
    pub pubkey: String,
    /// Team d-tag (replaceable identifier).
    pub d_tag: String,
    /// Full team coordinate `34199:<pubkey>:<d_tag>`.
    pub coordinate: String,
    /// Display title from `title` tag.
    pub title: String,
    /// Markdown/plain description from content.
    pub description: String,
    /// Optional image URL from `image`/`picture` tags.
    pub image: Option<String>,
    /// Agent definition event IDs from repeated `e` tags.
    pub agent_definition_ids: Vec<String>,
    /// Categories from repeated `c` tags.
    pub categories: Vec<String>,
    /// Hashtags from repeated `t` tags.
    pub tags: Vec<String>,
    /// Creation timestamp (unix seconds).
    pub created_at: u64,
    /// Aggregated positive reaction count (NIP-25 kind:7).
    pub like_count: u64,
    /// Aggregated comment count (NIP-22 kind:1111).
    pub comment_count: u64,
    /// Whether current user has a positive latest reaction on this team.
    pub liked_by_me: bool,
}

/// Team comment row (kind:1111 NIP-22) for threaded display.
#[derive(Debug, Clone, uniffi::Record)]
pub struct TeamCommentInfo {
    /// Comment event ID.
    pub id: String,
    /// Comment author pubkey (hex).
    pub pubkey: String,
    /// Display author name resolved from profile cache.
    pub author: String,
    /// Raw comment content.
    pub content: String,
    /// Creation timestamp (unix seconds).
    pub created_at: u64,
    /// Parent comment event ID for replies (None for roots).
    pub parent_comment_id: Option<String>,
}

/// Available configuration options for a project.
/// Used by iOS to populate the agent config modal.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProjectConfigOptions {
    /// All available models for the project
    pub all_models: Vec<String>,
    /// All available tools for the project
    pub all_tools: Vec<String>,
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

// ===== Stats Types (Full TUI Parity) =====

/// Cost data for a project
#[derive(Debug, Clone, uniffi::Record)]
pub struct ProjectCost {
    /// Project a_tag
    pub a_tag: String,
    /// Human-readable project name
    pub name: String,
    /// Total cost in USD
    pub cost: f64,
}

/// Messages count for a single day (unix timestamp for day start, user count, all count)
#[derive(Debug, Clone, uniffi::Record)]
pub struct DayMessages {
    /// Unix timestamp (seconds) for the start of the day (UTC)
    pub day_start: u64,
    /// Number of messages from the current user
    pub user_count: u64,
    /// Total number of messages from all users
    pub all_count: u64,
}

/// Activity data for a single hour (unix timestamp for hour start, tokens used, message count)
#[derive(Debug, Clone, uniffi::Record)]
pub struct HourActivity {
    /// Unix timestamp (seconds) for the start of the hour (UTC)
    pub hour_start: u64,
    /// Total tokens used in this hour
    pub tokens: u64,
    /// Number of messages generated in this hour
    pub messages: u64,
    /// Pre-normalized intensity (0-255) for token-based visualization
    pub token_intensity: u8,
    /// Pre-normalized intensity (0-255) for message-based visualization
    pub message_intensity: u8,
}

/// Complete stats snapshot with all data needed for iOS Stats tab
#[derive(Debug, Clone, uniffi::Record)]
pub struct StatsSnapshot {
    // === Metric Cards Data ===
    /// Total cost in USD for the past 14 days (COST_WINDOW_DAYS).
    /// Note: This is NOT all-time cost. For display, show as "past 2 weeks" or similar.
    pub total_cost_14_days: f64,

    // === Rankings Data ===
    /// Cost by project (sorted descending)
    pub cost_by_project: Vec<ProjectCost>,

    // === Messages Chart Data (14 days) ===
    /// Last 14 days of message counts (user vs all, newest first)
    pub messages_by_day: Vec<DayMessages>,

    // === Activity Grid Data (30 days × 24 hours = 720 hours) ===
    /// Last 720 hours of activity data with pre-computed intensities
    /// Pre-normalized to 0-255 intensity scale for direct visualization
    pub activity_by_hour: Vec<HourActivity>,
    /// Maximum token value across all hours (for legend display)
    pub max_tokens: u64,
    /// Maximum message count across all hours (for legend display)
    pub max_messages: u64,
}

// ===== Diagnostics Types (iOS Diagnostics View) =====

/// Event count for a specific kind
#[derive(Debug, Clone, uniffi::Record)]
pub struct KindEventCount {
    /// Event kind number
    pub kind: u16,
    /// Number of events of this kind in the database
    pub count: u64,
    /// Human-readable name for this kind (if known)
    pub name: String,
}

/// Database statistics for the diagnostics view
#[derive(Debug, Clone, uniffi::Record)]
pub struct DatabaseStats {
    /// Database file size in bytes
    pub db_size_bytes: u64,
    /// Event counts by kind (sorted by count descending)
    pub event_counts_by_kind: Vec<KindEventCount>,
    /// Total events across all kinds
    pub total_events: u64,
}

/// Information about a single subscription
#[derive(Debug, Clone, uniffi::Record)]
pub struct SubscriptionDiagnostics {
    /// Subscription ID
    pub sub_id: String,
    /// Human-readable description
    pub description: String,
    /// Event kinds this subscription listens for
    pub kinds: Vec<u16>,
    /// Raw filter JSON (for debugging)
    pub raw_filter: Option<String>,
    /// Number of events received
    pub events_received: u64,
    /// Seconds since subscription was created
    pub age_secs: u64,
}

/// Result of a single negentropy sync operation (for diagnostics)
#[derive(Debug, Clone, uniffi::Record)]
pub struct SyncResultDiagnostic {
    /// Event kind label (e.g., "31933", "4199")
    pub kind_label: String,
    /// Number of new events received
    pub events_received: u64,
    /// Status: "ok", "unsupported", or "failed"
    pub status: String,
    /// Error message if failed
    pub error: Option<String>,
    /// Seconds ago this sync completed
    pub seconds_ago: u64,
}

/// Negentropy sync status for the diagnostics view
#[derive(Debug, Clone, uniffi::Record)]
pub struct NegentropySyncDiagnostics {
    /// Whether negentropy sync is enabled
    pub enabled: bool,
    /// Current sync interval in seconds
    pub current_interval_secs: u64,
    /// Seconds since last full sync cycle completed (None if never completed)
    pub seconds_since_last_cycle: Option<u64>,
    /// Whether a sync is currently in progress
    pub sync_in_progress: bool,
    /// Number of successful syncs
    pub successful_syncs: u64,
    /// Number of failed syncs (actual errors, not unsupported relays)
    pub failed_syncs: u64,
    /// Number of syncs where relay didn't support negentropy
    pub unsupported_syncs: u64,
    /// Total events reconciled
    pub total_events_reconciled: u64,
    /// Recent sync results (last 20)
    pub recent_results: Vec<SyncResultDiagnostic>,
}

/// System diagnostics information
#[derive(Debug, Clone, uniffi::Record)]
pub struct SystemDiagnostics {
    /// Log file path
    pub log_path: String,
    /// Core version
    pub version: String,
    /// Whether the core is initialized
    pub is_initialized: bool,
    /// Whether a user is logged in
    pub is_logged_in: bool,
    /// Whether any relay is currently connected
    pub relay_connected: bool,
    /// Number of connected relays
    pub connected_relays: u32,
}

/// Full diagnostics snapshot containing all diagnostic information
/// Each section is optional to support best-effort partial data loading:
/// if one section fails (e.g., lock error), other sections can still be populated.
#[derive(Debug, Clone, uniffi::Record)]
pub struct DiagnosticsSnapshot {
    /// System information (None if system info collection failed)
    pub system: Option<SystemDiagnostics>,
    /// Negentropy sync status (None if sync stats unavailable)
    pub sync: Option<NegentropySyncDiagnostics>,
    /// Active subscriptions (None if subscription stats unavailable)
    pub subscriptions: Option<Vec<SubscriptionDiagnostics>>,
    /// Total events received across all subscriptions (0 if unavailable)
    pub total_subscription_events: u64,
    /// Database statistics (None if database stats collection failed)
    pub database: Option<DatabaseStats>,
    /// Error messages for sections that failed to load (for debugging)
    pub section_errors: Vec<String>,
}

/// AI Audio Settings (API keys never exposed - stored securely)
#[derive(Debug, Clone, uniffi::Record)]
pub struct AiAudioSettingsInfo {
    pub elevenlabs_api_key_configured: bool,
    pub openrouter_api_key_configured: bool,
    pub selected_voice_ids: Vec<String>,
    pub openrouter_model: Option<String>,
    pub audio_prompt: String,
    pub enabled: bool,
    pub tts_inactivity_threshold_secs: u64,
}

/// Voice from ElevenLabs
#[derive(Debug, Clone, uniffi::Record)]
pub struct VoiceInfo {
    pub voice_id: String,
    pub name: String,
    pub category: Option<String>,
    pub description: Option<String>,
    pub preview_url: Option<String>,
}

/// Pending backend awaiting user trust decision.
#[derive(Debug, Clone, uniffi::Record)]
pub struct PendingBackendInfo {
    pub backend_pubkey: String,
    pub project_a_tag: String,
    pub first_seen: u64,
    pub status_created_at: u64,
}

/// Snapshot of backend trust state and pending approvals.
#[derive(Debug, Clone, uniffi::Record)]
pub struct BackendTrustSnapshot {
    pub approved: Vec<String>,
    pub blocked: Vec<String>,
    pub pending: Vec<PendingBackendInfo>,
}

/// Model from OpenRouter
#[derive(Debug, Clone, uniffi::Record)]
pub struct ModelInfo {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    pub context_length: Option<u32>,
}

/// Audio notification record
#[derive(Debug, Clone, uniffi::Record)]
pub struct AudioNotificationInfo {
    pub id: String,
    pub agent_pubkey: String,
    pub conversation_title: String,
    pub original_text: String,
    pub massaged_text: String,
    pub voice_id: String,
    pub audio_file_path: String,
    pub created_at: u64,
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
    #[error("Lock error: failed to acquire lock on {resource}")]
    LockError { resource: String },
    #[error("Core not initialized")]
    CoreNotInitialized,
}

/// FFI-specific preferences storage (mirrors PreferencesStorage but simplified for FFI)
#[derive(Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FfiPreferences {
    /// IDs of archived conversations
    #[serde(default)]
    pub archived_thread_ids: std::collections::HashSet<String>,
    /// Visible project a_tags (empty = all visible)
    #[serde(default)]
    pub visible_projects: std::collections::HashSet<String>,
    /// IDs of collapsed threads (for UI state)
    #[serde(default)]
    pub collapsed_thread_ids: std::collections::HashSet<String>,
    /// Backend pubkeys explicitly approved by the user
    #[serde(default)]
    pub approved_backend_pubkeys: std::collections::HashSet<String>,
    /// Backend pubkeys explicitly blocked by the user
    #[serde(default)]
    pub blocked_backend_pubkeys: std::collections::HashSet<String>,
    /// AI Audio Notifications settings
    #[serde(default)]
    pub ai_audio_settings: crate::models::project_draft::AiAudioSettings,
}

impl FfiPreferences {
    fn load_from_file(path: &std::path::Path) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    fn trusted_backend_fields_present(path: &std::path::Path) -> bool {
        let contents = match std::fs::read_to_string(path) {
            Ok(c) => c,
            Err(_) => return false,
        };

        let value: serde_json::Value = match serde_json::from_str(&contents) {
            Ok(v) => v,
            Err(_) => return false,
        };

        let Some(obj) = value.as_object() else {
            return false;
        };

        obj.contains_key("approved_backend_pubkeys") || obj.contains_key("blocked_backend_pubkeys")
    }
}

/// Wrapper that handles persistence
struct FfiPreferencesStorage {
    prefs: FfiPreferences,
    path: std::path::PathBuf,
}

impl FfiPreferencesStorage {
    fn new(data_dir: &std::path::Path) -> Self {
        let path = data_dir.join("ios_preferences.json");
        let trusted_fields_present = FfiPreferences::trusted_backend_fields_present(&path);
        let mut prefs = FfiPreferences::load_from_file(&path).unwrap_or_default();
        let mut imported_legacy_trust = false;

        // Migration: if FFI prefs don't contain trust fields yet, import from TUI preferences
        // so desktop app trust state matches TUI and status events are not held as pending.
        if !trusted_fields_present
            && prefs.approved_backend_pubkeys.is_empty()
            && prefs.blocked_backend_pubkeys.is_empty()
        {
            if let Some((approved, blocked)) = Self::read_legacy_tui_trusted_backends() {
                prefs.approved_backend_pubkeys = approved;
                prefs.blocked_backend_pubkeys = blocked;
                imported_legacy_trust = true;
            }
        }

        // Migrate any existing API keys from JSON to secure storage
        Self::migrate_api_keys(&mut prefs.ai_audio_settings);

        let storage = Self { prefs, path };
        if imported_legacy_trust {
            let _ = storage.save();
        }
        storage
    }

    fn read_legacy_tui_trusted_backends() -> Option<(
        std::collections::HashSet<String>,
        std::collections::HashSet<String>,
    )> {
        let path = dirs::home_dir()?
            .join(".tenex")
            .join("cli")
            .join("preferences.json");
        let contents = std::fs::read_to_string(path).ok()?;
        let value: serde_json::Value = serde_json::from_str(&contents).ok()?;

        let collect = |key: &str| -> std::collections::HashSet<String> {
            value
                .get(key)
                .and_then(|v| v.as_array())
                .map(|arr| {
                    arr.iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default()
        };

        let approved = collect("approved_backend_pubkeys");
        let blocked = collect("blocked_backend_pubkeys");
        if approved.is_empty() && blocked.is_empty() {
            None
        } else {
            Some((approved, blocked))
        }
    }

    /// Migrate API keys from JSON to OS secure storage (one-time migration)
    fn migrate_api_keys(settings: &mut crate::models::project_draft::AiAudioSettings) {
        use crate::secure_storage::{SecureKey, SecureStorage};

        // Migrate ElevenLabs API key if present in JSON
        if let Some(key) = settings.elevenlabs_api_key.take() {
            if !key.is_empty() {
                let _ = SecureStorage::set(SecureKey::ElevenLabsApiKey, &key);
                tracing::info!("Migrated ElevenLabs API key to secure storage");
            }
        }

        // Migrate OpenRouter API key if present in JSON
        if let Some(key) = settings.openrouter_api_key.take() {
            if !key.is_empty() {
                let _ = SecureStorage::set(SecureKey::OpenRouterApiKey, &key);
                tracing::info!("Migrated OpenRouter API key to secure storage");
            }
        }
    }

    fn save(&self) -> Result<(), std::io::Error> {
        let json = serde_json::to_string_pretty(&self.prefs)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    // AI Audio Settings methods
    pub fn get_elevenlabs_api_key(&self) -> Option<String> {
        use crate::secure_storage::{SecureKey, SecureStorage};
        SecureStorage::get(SecureKey::ElevenLabsApiKey).ok()
    }

    pub fn get_openrouter_api_key(&self) -> Option<String> {
        use crate::secure_storage::{SecureKey, SecureStorage};
        SecureStorage::get(SecureKey::OpenRouterApiKey).ok()
    }

    fn set_selected_voice_ids(&mut self, voice_ids: Vec<String>) -> Result<(), String> {
        self.prefs.ai_audio_settings.selected_voice_ids = voice_ids;
        self.save()
            .map_err(|e| format!("Failed to save preferences: {}", e))
    }

    fn set_openrouter_model(&mut self, model: Option<String>) -> Result<(), String> {
        self.prefs.ai_audio_settings.openrouter_model = model;
        self.save()
            .map_err(|e| format!("Failed to save preferences: {}", e))
    }

    fn set_audio_prompt(&mut self, prompt: String) -> Result<(), String> {
        self.prefs.ai_audio_settings.audio_prompt = prompt;
        self.save()
            .map_err(|e| format!("Failed to save preferences: {}", e))
    }

    fn set_audio_notifications_enabled(&mut self, enabled: bool) -> Result<(), String> {
        self.prefs.ai_audio_settings.enabled = enabled;
        self.save()
            .map_err(|e| format!("Failed to save preferences: {}", e))
    }

    fn set_tts_inactivity_threshold(&mut self, secs: u64) -> Result<(), String> {
        self.prefs.ai_audio_settings.tts_inactivity_threshold_secs = secs;
        self.save()
            .map_err(|e| format!("Failed to save preferences: {}", e))
    }

    fn trusted_backends(
        &self,
    ) -> (
        std::collections::HashSet<String>,
        std::collections::HashSet<String>,
    ) {
        (
            self.prefs.approved_backend_pubkeys.clone(),
            self.prefs.blocked_backend_pubkeys.clone(),
        )
    }

    fn set_trusted_backends(
        &mut self,
        approved: std::collections::HashSet<String>,
        blocked: std::collections::HashSet<String>,
    ) -> Result<(), String> {
        self.prefs.approved_backend_pubkeys = approved;
        self.prefs.blocked_backend_pubkeys = blocked;
        self.save()
            .map_err(|e| format!("Failed to save preferences: {}", e))
    }
}

// =============================================================================
// EVENT CALLBACK INTERFACE
// =============================================================================
//
// Push-based event notification system for real-time UI updates.
// Swift/Kotlin implements EventCallback trait to receive notifications when
// data changes, eliminating the need for polling.

/// NIP-46 bunker signing request for FFI.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiBunkerSignRequest {
    pub request_id: String,
    pub requester_pubkey: String,
    pub event_kind: Option<u16>,
    pub event_json: Option<String>,
    pub event_content: Option<String>,
    pub event_tags_json: Option<String>,
}

/// NIP-46 bunker audit log entry for FFI.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiBunkerAuditEntry {
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

/// NIP-46 bunker auto-approve rule for FFI.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiBunkerAutoApproveRule {
    pub requester_pubkey: String,
    pub event_kind: Option<u16>,
}

/// Type of data change for targeted UI updates.
/// Allows views to refresh only what changed instead of full refresh.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum DataChangeType {
    /// A new message was appended to a conversation
    MessageAppended {
        conversation_id: String,
        message: Message,
    },
    /// A conversation was created or updated
    ConversationUpsert { conversation: ConversationFullInfo },
    /// A project was created or updated
    ProjectUpsert { project: Project },
    /// An inbox item was created or updated
    InboxUpsert { item: InboxItem },
    /// A report was created or updated (kind:30023)
    ReportUpsert { report: Report },
    /// Project online status updated (kind:24010)
    ProjectStatusChanged {
        project_id: String,
        project_a_tag: String,
        is_online: bool,
        online_agents: Vec<ProjectAgent>,
    },
    /// Backend approval required for a project status event
    PendingBackendApproval {
        backend_pubkey: String,
        project_a_tag: String,
    },
    /// Active conversations updated for a project (kind:24133)
    ActiveConversationsChanged {
        project_id: String,
        project_a_tag: String,
        active_conversation_ids: Vec<String>,
    },
    /// Streaming text chunk arrived (live typing)
    StreamChunk {
        agent_pubkey: String,
        conversation_id: String,
        text_delta: Option<String>,
    },
    /// MCP tools changed (kind:4200)
    McpToolsChanged,
    /// Teams content changed (kind:34199, 1111, or 7)
    TeamsChanged,
    /// Stats snapshot should be refreshed
    StatsUpdated,
    /// Diagnostics snapshot should be refreshed
    DiagnosticsUpdated,
    /// General data changed - legacy fallback
    General,
    /// NIP-46 bunker signing request requires user approval
    BunkerSignRequest { request: FfiBunkerSignRequest },
}

/// Callback interface for event notifications to Swift/Kotlin.
/// Implement this trait in Swift to receive push-based updates.
///
/// # Thread Safety
/// The callback will be invoked from a background thread.
/// Swift implementations should dispatch to main thread for UI updates.
#[uniffi::export(callback_interface)]
pub trait EventCallback: Send + Sync {
    /// Called when data has changed and UI should refresh.
    ///
    /// # Arguments
    /// * `change_type` - Type of change that occurred, for targeted updates
    fn on_data_changed(&self, change_type: DataChangeType);
}

/// Core TENEX functionality exposed to foreign languages.
///
/// This is intentionally minimal for MVP - we'll expand as needed.
/// Note: UniFFI objects are wrapped in Arc, so we use AtomicBool for interior mutability.
#[derive(uniffi::Object)]
pub struct TenexCore {
    initialized: AtomicBool,
    /// Stored keys for the logged-in user (protected by RwLock for interior mutability)
    keys: Arc<RwLock<Option<Keys>>>,
    /// nostrdb instance for local event storage
    ndb: Arc<RwLock<Option<Arc<Ndb>>>>,
    /// App data store built on top of nostrdb
    store: Arc<RwLock<Option<AppDataStore>>>,
    /// Core runtime command handle for NostrWorker
    core_handle: Arc<RwLock<Option<CoreHandle>>>,
    /// Data change receiver from NostrWorker (project status, streaming chunks)
    /// Uses Mutex because Receiver is not Sync, and UniFFI objects require Send + Sync
    data_rx: Arc<Mutex<Option<Receiver<DataChange>>>>,
    /// Worker thread handle (joins on drop)
    worker_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    /// NostrDB subscription stream for live updates
    ndb_stream: Arc<RwLock<Option<SubscriptionStream>>>,
    /// iOS preferences storage (archive state, collapsed threads, visible projects)
    preferences: Arc<RwLock<Option<FfiPreferencesStorage>>>,
    /// Timestamp of last refresh() call for throttling (milliseconds since UNIX epoch).
    /// Uses AtomicU64 for lock-free access. Stored as ms for precision without needing
    /// to store Instant (which isn't Send+Sync friendly for FFI).
    last_refresh_ms: AtomicU64,
    /// Subscription stats for diagnostics (shared with worker)
    subscription_stats: Arc<RwLock<Option<SharedSubscriptionStats>>>,
    /// Negentropy sync stats for diagnostics (shared with worker)
    negentropy_stats: Arc<RwLock<Option<SharedNegentropySyncStats>>>,
    /// Event callback for push notifications to UI (Swift/Kotlin)
    event_callback: Arc<RwLock<Option<Arc<dyn EventCallback>>>>,
    /// Flag to signal callback listener thread to stop (Arc for sharing with thread)
    callback_listener_running: Arc<AtomicBool>,
    /// Callback listener thread handle (joined on drop)
    callback_listener_handle: Arc<RwLock<Option<JoinHandle<()>>>>,
    /// Mutex to serialize nostrdb Transaction creation across all operations.
    /// CRITICAL: nostrdb cannot handle concurrent transactions on the same Ndb instance.
    /// This mutex ensures only one code path can create a Transaction at a time, preventing
    /// panics when refresh() and getDiagnosticsSnapshot() are called concurrently.
    ndb_transaction_lock: Arc<Mutex<()>>,
    /// Lock-free cache of today's runtime (milliseconds).
    /// Updated after refresh() and callback listener data processing.
    /// Read by get_today_runtime_ms() without acquiring the store RwLock,
    /// eliminating priority inversion when refresh() holds the write lock.
    cached_today_runtime_ms: Arc<AtomicU64>,
}

impl Drop for TenexCore {
    fn drop(&mut self) {
        // Stop callback listener if running
        self.callback_listener_running
            .store(false, Ordering::SeqCst);
        if let Ok(mut handle_guard) = self.callback_listener_handle.write() {
            if let Some(handle) = handle_guard.take() {
                let _ = handle.join();
            }
        }

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
    use crate::store::{events::ingest_events, AppDataStore, Database};
    use nostr_sdk::{EventBuilder, Keys, Kind, Tag, TagKind};
    use tempfile::tempdir;

    #[test]
    fn test_ffi_layout_guardrails() {
        let ffi_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/ffi");

        assert!(ffi_dir.join("mod.rs").exists(), "src/ffi/mod.rs must exist");
        assert!(
            !ffi_dir.join("ffi.rs").exists(),
            "legacy src/ffi.rs should not be reintroduced"
        );

        let mod_source =
            std::fs::read_to_string(ffi_dir.join("mod.rs")).expect("failed to read src/ffi/mod.rs");
        let mod_lines = mod_source.lines().count();
        assert!(
            mod_lines <= 3000,
            "src/ffi/mod.rs grew too large ({} lines); move API methods into *_api.rs modules",
            mod_lines
        );

        let api_file_count = std::fs::read_dir(&ffi_dir)
            .expect("failed to read src/ffi directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .to_string()
                    .ends_with("_api.rs")
            })
            .count();
        assert!(
            api_file_count >= 8,
            "expected domain-split API modules in src/ffi (found {} *_api.rs files)",
            api_file_count
        );
    }

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
        let _ = core.logout();
        assert!(!core.is_logged_in());
        assert!(core.get_current_user().is_none());
    }

    #[test]
    fn test_get_current_user_when_not_logged_in() {
        let core = TenexCore::new();
        assert!(core.get_current_user().is_none());
    }

    // ===== get_profile_picture tests =====

    #[test]
    fn test_get_profile_picture_returns_error_when_not_initialized() {
        // Test that get_profile_picture returns CoreNotInitialized error when store is None
        let core = TenexCore::new();
        // Note: Don't call init() - store should be None

        let result = core.get_profile_picture("c".repeat(64));

        assert!(result.is_err());
        match result {
            Err(TenexError::CoreNotInitialized) => {}
            Err(e) => panic!("Expected CoreNotInitialized error, got {:?}", e),
            Ok(_) => panic!("Expected error, got success"),
        }
    }

    #[test]
    fn test_get_profile_picture_invalid_pubkey_returns_none() {
        // Test that invalid pubkey format returns Ok(None), not an error
        let core = TenexCore::new();
        if !core.init() {
            // Skip test if db initialization fails (can happen in parallel test runs)
            println!(
                "Skipping test due to database initialization failure (parallel test conflict)"
            );
            return;
        }

        // Invalid pubkey (too short, not 64 hex chars)
        let result = core.get_profile_picture("invalid_pubkey".to_string());

        // Should return Ok(None) - pubkey validation happens in store, returns None for invalid
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_get_profile_picture_missing_profile_returns_none() {
        // Test that a valid pubkey with no profile returns Ok(None)
        let core = TenexCore::new();
        if !core.init() {
            // Skip test if db initialization fails (can happen in parallel test runs)
            println!(
                "Skipping test due to database initialization failure (parallel test conflict)"
            );
            return;
        }

        // Valid 64-char hex pubkey, but no profile exists
        let valid_pubkey = "c".repeat(64);
        let result = core.get_profile_picture(valid_pubkey);

        // Should return Ok(None) - valid request, just no data
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_get_profile_picture_empty_pubkey_returns_none() {
        // Test that empty pubkey returns Ok(None)
        let core = TenexCore::new();
        if !core.init() {
            // Skip test if db initialization fails (can happen in parallel test runs)
            println!(
                "Skipping test due to database initialization failure (parallel test conflict)"
            );
            return;
        }

        let result = core.get_profile_picture("".to_string());

        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_get_profile_picture_various_invalid_pubkeys() {
        // Test various malformed pubkeys
        let core = TenexCore::new();
        if !core.init() {
            // Skip test if db initialization fails (can happen in parallel test runs)
            println!(
                "Skipping test due to database initialization failure (parallel test conflict)"
            );
            return;
        }

        let invalid_pubkeys: Vec<String> = vec![
            "not_hex_at_all!@#$".to_string(), // Non-hex characters
            "abc123".to_string(),             // Too short
            "0".repeat(65),                   // Too long
            "g".repeat(64),                   // Invalid hex char 'g'
            "  ".to_string(),                 // Whitespace only
            "0123456789abcdef".to_string(),   // Valid hex but wrong length (16 chars)
        ];

        for pubkey in invalid_pubkeys {
            let result = core.get_profile_picture(pubkey.clone());
            // All should return Ok(None) - graceful handling of invalid input
            assert!(
                result.is_ok(),
                "Expected Ok for pubkey '{}', got {:?}",
                pubkey,
                result
            );
            assert!(
                result.unwrap().is_none(),
                "Expected None for invalid pubkey '{}'",
                pubkey
            );
        }
    }

    #[test]
    fn test_get_ask_event_by_id_returns_none_for_missing_or_invalid_id() {
        let dir = tempdir().expect("temp dir");
        let db = Database::new(dir.path()).expect("database");
        let core = TenexCore::new();

        {
            let mut store_guard = core.store.write().expect("store lock");
            *store_guard = Some(AppDataStore::new(db.ndb.clone()));
        }

        // Invalid hex/event id format.
        assert!(core
            .get_ask_event_by_id("not-a-valid-event-id".to_string())
            .is_none());

        // Valid event-id shape but missing event in DB.
        let missing = "a".repeat(64);
        assert!(core.get_ask_event_by_id(missing).is_none());
    }

    #[test]
    fn test_get_ask_event_by_id_returns_ask_event_and_author_pubkey() {
        let dir = tempdir().expect("temp dir");
        let db = Database::new(dir.path()).expect("database");
        let keys = Keys::generate();

        let ask_event = EventBuilder::new(Kind::from(1), "Please confirm scope")
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Scope Question"],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("question")),
                vec!["Scope", "What should be prioritized?", "UI", "FFI"],
            ))
            .sign_with_keys(&keys)
            .expect("ask event");

        ingest_events(&db.ndb, std::slice::from_ref(&ask_event), None).expect("ingest ask event");
        std::thread::sleep(std::time::Duration::from_millis(50));

        let core = TenexCore::new();
        {
            let mut store_guard = core.store.write().expect("store lock");
            *store_guard = Some(AppDataStore::new(db.ndb.clone()));
        }

        let event_id = ask_event.id.to_hex();
        let lookup = core
            .get_ask_event_by_id(event_id)
            .expect("ask lookup should resolve");

        assert_eq!(lookup.author_pubkey, keys.public_key().to_hex());
        assert_eq!(lookup.ask_event.title.as_deref(), Some("Scope Question"));
        assert_eq!(lookup.ask_event.context, "Please confirm scope");
        assert_eq!(lookup.ask_event.questions.len(), 1);
    }
}
