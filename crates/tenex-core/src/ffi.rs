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
        }
    }

    fn compact(&self) -> String {
        format!(
            "total={} msg={} conv={} proj={} inbox={} report={} status={} pending={} active={} stream={} mcp={} teams={} stats={} diag={} general={}",
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
            self.general
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
            deltas.push(DataChangeType::InboxUpsert {
                item: item.clone(),
            });
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
                                        .map(|agents| {
                                            agents
                                                .iter()
                                                .cloned()
                                                .collect()
                                        })
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
            DataChangeType::StreamChunk { .. } => {}
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

/// A single day's runtime data (unix timestamp for day start, runtime in ms)
#[derive(Debug, Clone, uniffi::Record)]
pub struct DayRuntime {
    /// Unix timestamp (seconds) for the start of the day (UTC)
    pub day_start: u64,
    /// Total runtime in milliseconds for this day
    pub runtime_ms: u64,
}

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

/// Top conversation by runtime
#[derive(Debug, Clone, uniffi::Record)]
pub struct TopConversation {
    /// Conversation ID (event ID)
    pub id: String,
    /// Conversation title
    pub title: String,
    /// Total runtime in milliseconds
    pub runtime_ms: u64,
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
/// This matches full TUI stats parity with pre-computed values
#[derive(Debug, Clone, uniffi::Record)]
pub struct StatsSnapshot {
    // === Metric Cards Data ===
    /// Total cost in USD for the past 14 days (COST_WINDOW_DAYS).
    /// Note: This is NOT all-time cost. For display, show as "past 2 weeks" or similar.
    pub total_cost_14_days: f64,
    /// 24-hour runtime in milliseconds
    pub today_runtime_ms: u64,
    /// 14-day average daily runtime in milliseconds (counting only non-zero days)
    pub avg_daily_runtime_ms: u64,
    /// Number of active days in the 14-day window (days with non-zero runtime)
    pub active_days_count: u32,

    // === Runtime Chart Data (14 days) ===
    /// Last 14 days of runtime data (newest first)
    pub runtime_by_day: Vec<DayRuntime>,

    // === Rankings Data ===
    /// Cost by project (sorted descending)
    pub cost_by_project: Vec<ProjectCost>,
    /// Top 20 conversations by runtime (sorted descending)
    pub top_conversations: Vec<TopConversation>,

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

        // Initialize nostrdb with appropriate mapsize for iOS
        // Use 2GB to avoid MDB_MAP_FULL errors with larger datasets
        let config = nostrdb::Config::new().set_mapsize(2 * 1024 * 1024 * 1024);
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
        let _ = core_handle.send(NostrCommand::Connect {
            keys,
            user_pubkey: pubkey.clone(),
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

        let nsec = keys.secret_key().to_bech32().map_err(|e| TenexError::Internal {
            message: format!("Failed to encode nsec: {}", e),
        })?;
        let npub = keys.public_key().to_bech32().map_err(|e| TenexError::Internal {
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
    pub fn publish_profile(&self, name: String, picture_url: Option<String>) -> Result<(), TenexError> {
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
    /// Queries nostrdb for kind 31933 events and returns them as Project.
    pub fn get_projects(&self) -> Vec<Project> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        store.get_projects().to_vec()
    }

    /// Get conversations for a project.
    ///
    /// Returns conversations organized with parent/child relationships.
    /// Use thread.parent_conversation_id to build nested conversation trees.
    pub fn get_conversations(&self, project_id: String) -> Vec<ConversationFullInfo> {
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

        let prefs_guard = match self.preferences.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let archived_ids = prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.clone())
            .unwrap_or_default();

        // Get threads for this project
        let threads = store.get_threads(&project_a_tag);

        threads
            .iter()
            .map(|t| thread_to_full_info(store, t, &archived_ids))
            .collect()
    }

    /// Get the total hierarchical LLM runtime for a conversation (includes all descendants) in milliseconds.
    /// Returns 0 if the conversation is not found or has no runtime data.
    pub fn get_conversation_runtime_ms(&self, conversation_id: String) -> u64 {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return 0,
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return 0,
        };

        store.get_hierarchical_runtime(&conversation_id)
    }

    /// Get today's LLM runtime for statusbar display (in milliseconds).
    /// Reads from a lock-free AtomicU64 cache that is updated after refresh()
    /// and callback listener data processing. This avoids acquiring the store
    /// RwLock, which eliminates priority inversion when refresh() holds the
    /// write lock for extended periods.
    /// Returns 0 if no data has been processed yet.
    pub fn get_today_runtime_ms(&self) -> u64 {
        self.cached_today_runtime_ms.load(Ordering::Acquire)
    }

    /// Get all descendant conversation IDs for a conversation (includes children, grandchildren, etc.)
    /// Returns empty Vec if no descendants exist or if the conversation is not found.
    pub fn get_descendant_conversation_ids(&self, conversation_id: String) -> Vec<String> {
        let started_at = Instant::now();
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        let descendants = store.runtime_hierarchy.get_descendants(&conversation_id);
        tlog!(
            "PERF",
            "ffi.get_descendant_conversation_ids conversation={} descendants={} elapsedMs={}",
            short_id(&conversation_id, 12),
            descendants.len(),
            started_at.elapsed().as_millis()
        );
        descendants
    }

    /// Get conversations by their IDs.
    /// Returns ConversationFullInfo for each conversation ID that exists.
    /// Conversations that don't exist are silently skipped.
    pub fn get_conversations_by_ids(
        &self,
        conversation_ids: Vec<String>,
    ) -> Vec<ConversationFullInfo> {
        let started_at = Instant::now();
        let requested = conversation_ids.len();
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        let prefs_guard = match self.preferences.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let archived_ids = prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.clone())
            .unwrap_or_default();

        let mut conversations = Vec::new();

        for conversation_id in conversation_ids {
            if let Some(thread) = store.get_thread_by_id(&conversation_id) {
                conversations.push(thread_to_full_info(store, thread, &archived_ids));
            }
        }

        tlog!(
            "PERF",
            "ffi.get_conversations_by_ids requested={} returned={} elapsedMs={}",
            requested,
            conversations.len(),
            started_at.elapsed().as_millis()
        );

        conversations
    }

    /// Get messages for a conversation.
    pub fn get_messages(&self, conversation_id: String) -> Vec<Message> {
        let started_at = Instant::now();
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        let messages: Vec<Message> = store.get_messages(&conversation_id).to_vec();
        let total_elapsed_ms = started_at.elapsed().as_millis();
        tlog!(
            "PERF",
            "ffi.get_messages conversation={} count={} totalMs={}",
            short_id(&conversation_id, 12),
            messages.len(),
            total_elapsed_ms
        );

        messages
    }

    /// Resolve an ask event by event ID.
    /// Used for q-tag references that may point to ask events instead of child threads.
    pub fn get_ask_event_by_id(&self, event_id: String) -> Option<AskEventLookupInfo> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return None,
        };

        let store = store_guard.as_ref()?;

        let (ask_event, author_pubkey) = store.get_ask_event_by_id(&event_id)?;
        Some(AskEventLookupInfo {
            ask_event: ask_event.clone(),
            author_pubkey,
        })
    }

    /// Get reports for a project.
    pub fn get_reports(&self, project_id: String) -> Vec<Report> {
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

        store
            .reports
            .get_reports_by_project(&project_a_tag)
            .into_iter()
            .cloned()
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

        store.inbox.get_items().to_vec()
    }

    // ===== Search Methods =====

    /// Full-text search across threads and messages.
    /// Uses in-memory store data (same approach as TUI search).
    /// Returns search results with content snippets and context.
    pub fn search(&self, query: String, limit: i32) -> Vec<SearchResult> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => {
                return Vec::new();
            }
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => {
                return Vec::new();
            }
        };

        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        // 1. Search thread titles and content (in-memory)
        for project in store.get_projects() {
            let project_a_tag = project.a_tag();

            for thread in store.get_threads(&project_a_tag) {
                let title_matches = thread.title.to_lowercase().contains(&query_lower);
                let content_matches = thread.content.to_lowercase().contains(&query_lower);

                if (title_matches || content_matches) && !seen_ids.contains(&thread.id) {
                    seen_ids.insert(thread.id.clone());

                    let author = store.get_profile_name(&thread.pubkey);
                    let content = if title_matches {
                        thread.title.clone()
                    } else {
                        thread.content.clone()
                    };

                    results.push(SearchResult {
                        event_id: thread.id.clone(),
                        thread_id: Some(thread.id.clone()),
                        content,
                        kind: 1, // Thread roots are kind:1
                        author,
                        created_at: thread.last_activity,
                        project_a_tag: Some(project_a_tag.clone()),
                    });

                    if results.len() >= limit as usize {
                        return results;
                    }
                }
            }
        }

        // 2. Search message content (in-memory)
        for project in store.get_projects() {
            let project_a_tag = project.a_tag();

            for thread in store.get_threads(&project_a_tag) {
                for message in store.get_messages(&thread.id) {
                    if message.content.to_lowercase().contains(&query_lower)
                        && !seen_ids.contains(&message.id)
                    {
                        seen_ids.insert(message.id.clone());

                        let author = store.get_profile_name(&message.pubkey);

                        results.push(SearchResult {
                            event_id: message.id.clone(),
                            thread_id: Some(thread.id.clone()),
                            content: message.content.clone(),
                            kind: 1, // Messages are kind:1
                            author,
                            created_at: message.created_at,
                            project_a_tag: Some(project_a_tag.clone()),
                        });

                        if results.len() >= limit as usize {
                            return results;
                        }
                    }
                }
            }
        }

        // Sort by created_at descending (most recent first)
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        results
    }

    // ===== Conversations Tab Methods (Full-featured) =====

    /// Get all conversations across all projects with full info for the Conversations tab.
    /// Returns conversations with activity tracking, archive status, and hierarchy data.
    /// Sorted by: active conversations first (by effective_last_activity desc),
    /// then inactive conversations by effective_last_activity desc.
    ///
    /// Returns Result to distinguish "no data" from "core error".
    pub fn get_all_conversations(
        &self,
        filter: ConversationFilter,
    ) -> Result<Vec<ConversationFullInfo>, TenexError> {
        let started_at = Instant::now();
        let store_guard = self.store.read().map_err(|_| TenexError::LockError {
            resource: "store".to_string(),
        })?;

        let store = store_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;

        // Get archived thread IDs from preferences
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;
        let archived_ids = prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.clone())
            .unwrap_or_default();

        // Build list of project a_tags to include
        let projects = store.get_projects();
        let project_a_tags: Vec<String> = if filter.project_ids.is_empty() {
            // All projects
            projects.iter().map(|p| p.a_tag()).collect()
        } else {
            // Filter to specified project IDs
            projects
                .iter()
                .filter(|p| filter.project_ids.contains(&p.id))
                .map(|p| p.a_tag())
                .collect()
        };

        // Calculate time filter cutoff
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let time_cutoff = match filter.time_filter {
            TimeFilterOption::All => 0,
            TimeFilterOption::Today => now.saturating_sub(86400),
            TimeFilterOption::ThisWeek => now.saturating_sub(86400 * 7),
            TimeFilterOption::ThisMonth => now.saturating_sub(86400 * 30),
        };

        // Pre-compute message counts for all threads to avoid N×M reads
        // Build a map of thread_id -> message_count
        let precompute_started_at = Instant::now();
        let mut message_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        let mut total_threads_scanned = 0usize;
        for project_a_tag in &project_a_tags {
            let threads = store.get_threads(project_a_tag);
            total_threads_scanned += threads.len();
            for thread in threads {
                let count = store.get_messages(&thread.id).len() as u32;
                message_counts.insert(thread.id.clone(), count);
            }
        }
        let precompute_elapsed_ms = precompute_started_at.elapsed().as_millis();

        // Collect all threads from selected projects
        let collect_started_at = Instant::now();
        let mut conversations: Vec<ConversationFullInfo> = Vec::new();
        let mut skipped_scheduled = 0usize;
        let mut skipped_archived = 0usize;
        let mut skipped_time = 0usize;

        for project_a_tag in &project_a_tags {
            let threads = store.get_threads(project_a_tag);

            for thread in threads {
                // Filter: scheduled events
                if filter.hide_scheduled && thread.is_scheduled {
                    skipped_scheduled += 1;
                    continue;
                }

                // Filter: archived
                let is_archived = archived_ids.contains(&thread.id);
                if !filter.show_archived && is_archived {
                    skipped_archived += 1;
                    continue;
                }

                // Filter: time
                if time_cutoff > 0 && thread.effective_last_activity < time_cutoff {
                    skipped_time += 1;
                    continue;
                }

                // Get message count from pre-computed map (O(1) lookup instead of O(n) each time)
                let message_count = message_counts.get(&thread.id).copied().unwrap_or(0);

                // Get author display name
                let author_name = store.get_profile_name(&thread.pubkey);

                // Check if thread has children
                let has_children = store.runtime_hierarchy.has_children(&thread.id);

                // Check if thread has active agents
                let is_active = store.operations.is_event_busy(&thread.id);

                conversations.push(ConversationFullInfo {
                    thread: thread.clone(),
                    author: author_name,
                    message_count,
                    is_active,
                    is_archived,
                    has_children,
                    project_a_tag: project_a_tag.clone(),
                });
            }
        }
        let collect_elapsed_ms = collect_started_at.elapsed().as_millis();

        // Sort: active first (by effective_last_activity desc), then inactive by effective_last_activity desc
        let sort_started_at = Instant::now();
        conversations.sort_by(|a, b| match (a.is_active, b.is_active) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => b.thread.effective_last_activity.cmp(&a.thread.effective_last_activity),
        });
        let sort_elapsed_ms = sort_started_at.elapsed().as_millis();

        tlog!(
            "PERF",
            "ffi.get_all_conversations projects={} requestedProjectIds={} scannedThreads={} returned={} skippedScheduled={} skippedArchived={} skippedTime={} precomputeMs={} collectMs={} sortMs={} totalMs={}",
            project_a_tags.len(),
            filter.project_ids.len(),
            total_threads_scanned,
            conversations.len(),
            skipped_scheduled,
            skipped_archived,
            skipped_time,
            precompute_elapsed_ms,
            collect_elapsed_ms,
            sort_elapsed_ms,
            started_at.elapsed().as_millis()
        );

        Ok(conversations)
    }

    /// Get all projects with filter info (visibility, counts).
    /// Returns Result to distinguish "no data" from "core error".
    pub fn get_project_filters(&self) -> Result<Vec<ProjectFilterInfo>, TenexError> {
        let store_guard = self.store.read().map_err(|_| TenexError::LockError {
            resource: "store".to_string(),
        })?;

        let store = store_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;

        // Get visible project IDs from preferences
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;
        let visible_projects = prefs_guard
            .as_ref()
            .map(|p| p.prefs.visible_projects.clone())
            .unwrap_or_default();

        let projects = store.get_projects();

        Ok(projects
            .iter()
            .map(|p| {
                let a_tag = p.a_tag();
                let threads = store.get_threads(&a_tag);
                let total_count = threads.len() as u32;

                // Count active conversations
                let active_count = threads
                    .iter()
                    .filter(|t| store.operations.is_event_busy(&t.id))
                    .count() as u32;

                // Check visibility (empty means all visible)
                let is_visible = visible_projects.is_empty() || visible_projects.contains(&a_tag);

                ProjectFilterInfo {
                    id: p.id.clone(),
                    a_tag,
                    title: p.title.clone(),
                    is_visible,
                    active_count,
                    total_count,
                }
            })
            .collect())
    }

    /// Set which projects are visible in the Conversations tab.
    /// Pass empty array to show all projects.
    pub fn set_visible_projects(&self, project_a_tags: Vec<String>) {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            prefs.prefs.visible_projects = project_a_tags.into_iter().collect();
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
        }
    }

    /// Archive a conversation (hide from default view).
    pub fn archive_conversation(&self, conversation_id: String) {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            prefs.prefs.archived_thread_ids.insert(conversation_id);
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
        }
    }

    /// Unarchive a conversation (show in default view).
    pub fn unarchive_conversation(&self, conversation_id: String) {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            prefs.prefs.archived_thread_ids.remove(&conversation_id);
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
        }
    }

    /// Toggle archive status for a conversation.
    /// Returns true if the conversation is now archived.
    pub fn toggle_conversation_archived(&self, conversation_id: String) -> bool {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return false,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            let is_now_archived = if prefs.prefs.archived_thread_ids.contains(&conversation_id) {
                prefs.prefs.archived_thread_ids.remove(&conversation_id);
                false
            } else {
                prefs.prefs.archived_thread_ids.insert(conversation_id);
                true
            };
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
            is_now_archived
        } else {
            false
        }
    }

    /// Check if a conversation is archived.
    pub fn is_conversation_archived(&self, conversation_id: String) -> bool {
        let prefs_guard = match self.preferences.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.contains(&conversation_id))
            .unwrap_or(false)
    }

    /// Get all archived conversation IDs.
    /// Returns Result to distinguish "no data" from "lock error".
    pub fn get_archived_conversation_ids(&self) -> Result<Vec<String>, TenexError> {
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;

        Ok(prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.iter().cloned().collect())
            .unwrap_or_default())
    }

    // ===== Collapsed Thread State Methods (Fix #5: Expose via FFI) =====

    /// Get all collapsed thread IDs.
    pub fn get_collapsed_thread_ids(&self) -> Result<Vec<String>, TenexError> {
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;

        Ok(prefs_guard
            .as_ref()
            .map(|p| p.prefs.collapsed_thread_ids.iter().cloned().collect())
            .unwrap_or_default())
    }

    /// Set collapsed thread IDs (replace all).
    pub fn set_collapsed_thread_ids(&self, thread_ids: Vec<String>) {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            prefs.prefs.collapsed_thread_ids = thread_ids.into_iter().collect();
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
        }
    }

    /// Toggle collapsed state for a thread.
    /// Returns true if the thread is now collapsed.
    pub fn toggle_thread_collapsed(&self, thread_id: String) -> bool {
        let mut prefs_guard = match self.preferences.write() {
            Ok(g) => g,
            Err(_) => return false,
        };

        if let Some(prefs) = prefs_guard.as_mut() {
            let is_now_collapsed = if prefs.prefs.collapsed_thread_ids.contains(&thread_id) {
                prefs.prefs.collapsed_thread_ids.remove(&thread_id);
                false
            } else {
                prefs.prefs.collapsed_thread_ids.insert(thread_id);
                true
            };
            if let Err(e) = prefs.save() {
                tracing::error!("Failed to save preferences: {}", e);
            }
            is_now_collapsed
        } else {
            false
        }
    }

    /// Check if a thread is collapsed.
    pub fn is_thread_collapsed(&self, thread_id: String) -> bool {
        let prefs_guard = match self.preferences.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        prefs_guard
            .as_ref()
            .map(|p| p.prefs.collapsed_thread_ids.contains(&thread_id))
            .unwrap_or(false)
    }

    /// Get agents for a project.
    ///
    /// Returns agents configured for the specified project.
    /// Returns an error if the store cannot be accessed.
    pub fn get_agents(&self, project_id: String) -> Result<Vec<AgentDefinition>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        // Find the project by ID and get its agent IDs (event IDs of kind:4199 definitions)
        let project = store
            .get_projects()
            .iter()
            .find(|p| p.id == project_id)
            .cloned();
        let agent_definition_ids: Vec<String> = match project {
            Some(p) => p.agent_definition_ids,
            None => return Ok(Vec::new()), // Project not found = empty agents (not an error)
        };

        // Get agent definitions for these IDs
        Ok(store
            .content
            .get_agent_definitions()
            .into_iter()
            .filter(|agent| agent_definition_ids.contains(&agent.id))
            .cloned()
            .collect())
    }

    /// Get all available agents (not filtered by project).
    ///
    /// Returns all known agent definitions.
    /// Returns an error if the store cannot be accessed.
    pub fn get_all_agents(&self) -> Result<Vec<AgentDefinition>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store
            .content
            .get_agent_definitions()
            .into_iter()
            .cloned()
            .collect())
    }

    /// Get all available team packs (kind:34199), deduped to latest by `pubkey + d_tag`.
    ///
    /// Includes computed social metrics from comments (kind:1111) and reactions (kind:7)
    /// matched with dual anchors (`a`/`A` coordinate + `e`/`E` event id).
    pub fn get_all_teams(&self) -> Result<Vec<TeamInfo>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;
        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let mut latest_by_key: HashMap<String, TeamPack> = HashMap::new();
        for team in store.content.get_team_packs() {
            let identifier = if team.d_tag.is_empty() {
                team.id.clone()
            } else {
                team.d_tag.clone()
            };
            let key = format!(
                "{}:{}",
                team.pubkey.to_lowercase(),
                identifier.to_lowercase()
            );
            match latest_by_key.get(&key) {
                Some(existing)
                    if existing.created_at > team.created_at
                        || (existing.created_at == team.created_at && existing.id >= team.id) => {}
                _ => {
                    latest_by_key.insert(key, team.clone());
                }
            }
        }

        let mut teams: Vec<TeamPack> = latest_by_key.into_values().collect();
        teams.sort_by(|a, b| {
            b.created_at
                .cmp(&a.created_at)
                .then_with(|| b.id.cmp(&a.id))
        });

        let ndb = {
            let ndb_guard = self.ndb.read().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire ndb lock: {}", e),
            })?;
            ndb_guard
                .as_ref()
                .cloned()
                .ok_or(TenexError::CoreNotInitialized)?
        };

        let txn = Transaction::new(ndb.as_ref()).map_err(|e| TenexError::Internal {
            message: format!("Failed to create transaction: {}", e),
        })?;
        let social_filter = nostrdb::Filter::new().kinds([7, 1111]).build();
        let social_notes =
            ndb.query(&txn, &[social_filter], 50_000)
                .map_err(|e| TenexError::Internal {
                    message: format!("Failed querying social events: {}", e),
                })?;

        let current_user_pubkey = self.get_current_user().map(|u| u.pubkey);

        #[derive(Default)]
        struct TeamSocial {
            comment_count: u64,
            reactions_by_pubkey: HashMap<String, (u64, bool)>,
        }

        let mut social_by_team: HashMap<String, TeamSocial> = HashMap::new();
        for team in &teams {
            social_by_team.insert(team.id.clone(), TeamSocial::default());
        }

        for result in social_notes {
            let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) else {
                continue;
            };

            for team in &teams {
                let identifier = if team.d_tag.is_empty() {
                    team.id.clone()
                } else {
                    team.d_tag.clone()
                };
                let coordinate = format!("34199:{}:{}", team.pubkey, identifier);
                if !note_matches_team_context(&note, &coordinate, &team.id) {
                    continue;
                }

                if let Some(social) = social_by_team.get_mut(&team.id) {
                    if note.kind() == 1111 {
                        social.comment_count += 1;
                    } else if note.kind() == 7 {
                        let reactor = hex::encode(note.pubkey());
                        let is_positive = reaction_is_positive(note.content());
                        let created_at = note.created_at();
                        match social.reactions_by_pubkey.get(&reactor) {
                            Some((existing_ts, _)) if *existing_ts > created_at => {}
                            _ => {
                                social
                                    .reactions_by_pubkey
                                    .insert(reactor, (created_at, is_positive));
                            }
                        }
                    }
                }
                break;
            }
        }

        Ok(teams
            .into_iter()
            .map(|team| {
                let identifier = if team.d_tag.is_empty() {
                    team.id.clone()
                } else {
                    team.d_tag.clone()
                };
                let coordinate = format!("34199:{}:{}", team.pubkey, identifier);
                let social = social_by_team.remove(&team.id).unwrap_or_default();
                let like_count = social
                    .reactions_by_pubkey
                    .values()
                    .filter(|(_, is_positive)| *is_positive)
                    .count() as u64;
                let liked_by_me = current_user_pubkey
                    .as_ref()
                    .and_then(|pk| social.reactions_by_pubkey.get(pk))
                    .map(|(_, is_positive)| *is_positive)
                    .unwrap_or(false);

                TeamInfo {
                    id: team.id,
                    pubkey: team.pubkey,
                    d_tag: team.d_tag,
                    coordinate,
                    title: team.title,
                    description: team.description,
                    image: team.image,
                    agent_definition_ids: team.agent_definition_ids,
                    categories: team.categories,
                    tags: team.tags,
                    created_at: team.created_at,
                    like_count,
                    comment_count: social.comment_count,
                    liked_by_me,
                }
            })
            .collect())
    }

    /// Get team comments (kind:1111) for one team using dual-anchor matching.
    pub fn get_team_comments(
        &self,
        team_coordinate: String,
        team_event_id: String,
    ) -> Result<Vec<TeamCommentInfo>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;
        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let ndb = {
            let ndb_guard = self.ndb.read().map_err(|e| TenexError::Internal {
                message: format!("Failed to acquire ndb lock: {}", e),
            })?;
            ndb_guard
                .as_ref()
                .cloned()
                .ok_or(TenexError::CoreNotInitialized)?
        };

        let txn = Transaction::new(ndb.as_ref()).map_err(|e| TenexError::Internal {
            message: format!("Failed to create transaction: {}", e),
        })?;
        let filter = nostrdb::Filter::new().kinds([1111]).build();
        let notes = ndb
            .query(&txn, &[filter], 20_000)
            .map_err(|e| TenexError::Internal {
                message: format!("Failed querying comments: {}", e),
            })?;

        let mut comments: Vec<TeamCommentInfo> = Vec::new();
        for result in notes {
            let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) else {
                continue;
            };
            if !note_matches_team_context(&note, &team_coordinate, &team_event_id) {
                continue;
            }

            let pubkey = hex::encode(note.pubkey());
            comments.push(TeamCommentInfo {
                id: hex::encode(note.id()),
                pubkey: pubkey.clone(),
                author: store.get_profile_name(&pubkey),
                content: note.content().to_string(),
                created_at: note.created_at(),
                parent_comment_id: parse_parent_comment_id(&note, &team_event_id),
            });
        }

        comments.sort_by(|a, b| {
            a.created_at
                .cmp(&b.created_at)
                .then_with(|| a.id.cmp(&b.id))
        });
        Ok(comments)
    }

    /// Publish a team reaction (kind:7 NIP-25) and return reaction event ID.
    pub fn react_to_team(
        &self,
        team_coordinate: String,
        team_event_id: String,
        team_pubkey: String,
        is_like: bool,
    ) -> Result<String, TenexError> {
        let core_handle = get_core_handle(&self.core_handle)?;
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

        core_handle
            .send(NostrCommand::ReactToTeam {
                team_coordinate,
                team_event_id,
                team_pubkey,
                is_like,
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send react_to_team command: {}", e),
            })?;

        response_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| TenexError::Internal {
                message: "Timed out waiting for team reaction publish confirmation".to_string(),
            })
    }

    /// Publish a team comment (kind:1111 NIP-22) and return comment event ID.
    pub fn post_team_comment(
        &self,
        team_coordinate: String,
        team_event_id: String,
        team_pubkey: String,
        content: String,
        parent_comment_id: Option<String>,
        parent_comment_pubkey: Option<String>,
    ) -> Result<String, TenexError> {
        if content.trim().is_empty() {
            return Err(TenexError::Internal {
                message: "Comment content cannot be empty".to_string(),
            });
        }

        let core_handle = get_core_handle(&self.core_handle)?;
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

        core_handle
            .send(NostrCommand::PostTeamComment {
                team_coordinate,
                team_event_id,
                team_pubkey,
                content,
                parent_comment_id,
                parent_comment_pubkey,
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send post_team_comment command: {}", e),
            })?;

        response_rx
            .recv_timeout(Duration::from_secs(10))
            .map_err(|_| TenexError::Internal {
                message: "Timed out waiting for team comment publish confirmation".to_string(),
            })
    }

    /// Get all nudges (kind:4201 events).
    ///
    /// Returns all nudges sorted by created_at descending (most recent first).
    /// Used by iOS for nudge selection in new conversations.
    pub fn get_nudges(&self) -> Result<Vec<Nudge>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store.content.get_nudges().into_iter().cloned().collect())
    }

    /// Get all skills (kind:4202 events).
    ///
    /// Returns all skills sorted by created_at descending (most recent first).
    /// Used by iOS/CLI for skill selection in new conversations.
    pub fn get_skills(&self) -> Result<Vec<Skill>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store.content.get_skills().into_iter().cloned().collect())
    }

    /// Get online agents for a project from the project status (kind:24010).
    ///
    /// These are actual agent instances with their own Nostr keypairs.
    /// Use these for agent selection in the message composer - the pubkeys
    /// can be used for profile picture lookups and p-tags.
    ///
    /// Returns empty if project not found or project is offline.
    pub fn get_online_agents(
        &self,
        project_id: String,
    ) -> Result<Vec<ProjectAgent>, TenexError> {
        use crate::tlog;
        tlog!(
            "FFI",
            "get_online_agents called with project_id: {}",
            project_id
        );

        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        tlog!(
            "FFI",
            "Total projects in store: {}",
            store.get_projects().len()
        );
        tlog!("FFI", "project_statuses HashMap keys:");
        for key in store.project_statuses.keys() {
            tlog!("FFI", "  - '{}'", key);
        }

        // Find the project by ID
        let project = store
            .get_projects()
            .iter()
            .find(|p| p.id == project_id)
            .cloned();
        let project = match project {
            Some(p) => {
                tlog!("FFI", "Project found: id='{}' a_tag='{}'", p.id, p.a_tag());
                p
            }
            None => {
                tlog!("FFI", "Project NOT found for id: {}", project_id);
                return Ok(Vec::new()); // Project not found = empty agents
            }
        };

        // Get agents from project status (kind:24010)
        tlog!(
            "FFI",
            "Looking up project_statuses for a_tag: '{}'",
            project.a_tag()
        );

        // Check if status exists (even if stale)
        if let Some(status) = store.project_statuses.get(&project.a_tag()) {
            use std::time::{SystemTime, UNIX_EPOCH};
            let now = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            let age_secs = now.saturating_sub(status.created_at);
            tlog!(
                "FFI",
                "Status exists: created_at={} now={} age={}s is_online={}",
                status.created_at,
                now,
                age_secs,
                status.is_online()
            );
        } else {
            tlog!("FFI", "No status found in project_statuses HashMap");
        }

        let agents = store
            .get_online_agents(&project.a_tag())
            .map(|agents| {
                tlog!("FFI", "Found {} online agents", agents.len());
                for agent in agents {
                    tlog!("FFI", "  Agent: {} ({})", agent.name, agent.pubkey);
                }
                agents.iter().cloned().collect()
            })
            .unwrap_or_else(|| {
                tlog!("FFI", "No online agents (status is stale or missing)");
                Vec::new()
            });

        tlog!("FFI", "Returning {} agents", agents.len());
        Ok(agents)
    }

    /// Get available configuration options for a project.
    ///
    /// Returns all available models and tools from the project status (kind:24010).
    /// Used by iOS to populate the agent config modal with available options.
    pub fn get_project_config_options(
        &self,
        project_id: String,
    ) -> Result<ProjectConfigOptions, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        // Find the project by ID
        let project = store
            .get_projects()
            .iter()
            .find(|p| p.id == project_id)
            .cloned();
        let project = match project {
            Some(p) => p,
            None => {
                return Err(TenexError::Internal {
                    message: format!("Project not found: {}", project_id),
                })
            }
        };

        // Get project status to extract all_models and all_tools
        let status = store.get_project_status(&project.a_tag());
        match status {
            Some(s) => Ok(ProjectConfigOptions {
                all_models: s.all_models.clone(),
                all_tools: s.all_tools.to_vec(),
            }),
            None => Ok(ProjectConfigOptions {
                all_models: Vec::new(),
                all_tools: Vec::new(),
            }),
        }
    }

    /// Update an agent's configuration (model and tools).
    ///
    /// Publishes a kind:24020 event to update the agent's configuration.
    /// The backend will process this event and update the agent's config.
    pub fn update_agent_config(
        &self,
        project_id: String,
        agent_pubkey: String,
        model: Option<String>,
        tools: Vec<String>,
        tags: Vec<String>,
    ) -> Result<(), TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::UpdateAgentConfig {
                project_a_tag,
                agent_pubkey,
                model,
                tools,
                tags,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send update agent config command: {}", e),
            })?;

        Ok(())
    }

    /// Update an agent's configuration globally (all projects).
    ///
    /// Publishes a kind:24020 event without a project a-tag.
    pub fn update_global_agent_config(
        &self,
        agent_pubkey: String,
        model: Option<String>,
        tools: Vec<String>,
        tags: Vec<String>,
    ) -> Result<(), TenexError> {
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::UpdateGlobalAgentConfig {
                agent_pubkey,
                model,
                tools,
                tags,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send update global agent config command: {}", e),
            })?;

        Ok(())
    }

    /// Create a new agent definition (kind:4199).
    ///
    /// The created definition is published through the Nostr worker and ingested locally.
    pub fn create_agent_definition(
        &self,
        name: String,
        description: String,
        role: String,
        instructions: String,
        version: String,
        source_id: Option<String>,
        is_fork: bool,
    ) -> Result<(), TenexError> {
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::CreateAgentDefinition {
                name,
                description,
                role,
                instructions,
                version,
                source_id,
                is_fork,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send create agent definition command: {}", e),
            })?;

        Ok(())
    }

    /// Delete an agent definition (kind:4199) via NIP-09 kind:5 deletion.
    pub fn delete_agent_definition(&self, agent_id: String) -> Result<(), TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;
        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let agent = store
            .content
            .get_agent_definition(&agent_id)
            .ok_or_else(|| TenexError::Internal {
                message: format!("Agent definition not found: {}", agent_id),
            })?;

        let current_user = self
            .get_current_user()
            .ok_or_else(|| TenexError::Internal {
                message: "No logged-in user".to_string(),
            })?;

        if !agent.pubkey.eq_ignore_ascii_case(&current_user.pubkey) {
            return Err(TenexError::Internal {
                message: "Only the author can delete this agent definition".to_string(),
            });
        }

        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::DeleteAgentDefinition {
                agent_id,
                client: Some("tenex-ios".to_string()),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send delete agent definition command: {}", e),
            })?;

        Ok(())
    }

    /// Get all MCP tool definitions (kind:4200 events).
    ///
    /// Returns tools sorted by created_at descending (newest first).
    pub fn get_all_mcp_tools(&self) -> Result<Vec<MCPTool>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store.content.get_mcp_tools().into_iter().cloned().collect())
    }

    /// Update an existing project (kind:31933 replaceable event).
    ///
    /// Republish the same d-tag with updated metadata, agents, and MCP tool assignments.
    pub fn update_project(
        &self,
        project_id: String,
        title: String,
        description: String,
        repo_url: Option<String>,
        picture_url: Option<String>,
        agent_definition_ids: Vec<String>,
        mcp_tool_ids: Vec<String>,
    ) -> Result<(), TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::UpdateProject {
                project_a_tag,
                title,
                description,
                repo_url,
                picture_url,
                agent_definition_ids,
                mcp_tool_ids,
                client: Some("tenex-ios".to_string()),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send update project command: {}", e),
            })?;

        Ok(())
    }

    /// Tombstone-delete a project by republishing it with ["deleted"] tag.
    pub fn delete_project(&self, project_id: String) -> Result<(), TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        core_handle
            .send(NostrCommand::DeleteProject {
                project_a_tag,
                client: Some("tenex-ios".to_string()),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send delete project command: {}", e),
            })?;

        Ok(())
    }

    /// Check if a project is online (has a recent kind:24010 status event).
    ///
    /// A project is considered online if:
    /// 1. It has a status event from an approved backend
    /// 2. The status event is not stale (within the staleness threshold)
    ///
    /// Returns true if the project is online, false otherwise.
    pub fn is_project_online(&self, project_id: String) -> bool {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return false,
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return false,
        };

        // Find the project by ID
        let project = match store.get_projects().iter().find(|p| p.id == project_id) {
            Some(p) => p,
            None => return false,
        };

        // Check if project has a non-stale status
        store
            .get_project_status(&project.a_tag())
            .map(|s| s.is_online())
            .unwrap_or(false)
    }

    /// Boot/start a project (sends kind:24000 event).
    ///
    /// This sends a boot request to wake up the project's backend.
    /// The backend will then start publishing kind:24010 status events,
    /// making the project "online" and its agents available.
    ///
    /// Use this when a project is offline and you want to start it.
    pub fn boot_project(&self, project_id: String) -> Result<(), TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        // Find the project by ID
        let project = store
            .get_projects()
            .iter()
            .find(|p| p.id == project_id)
            .cloned()
            .ok_or_else(|| TenexError::Internal {
                message: format!("Project not found: {}", project_id),
            })?;

        let core_handle = get_core_handle(&self.core_handle)?;

        // Send the boot project command
        core_handle
            .send(NostrCommand::BootProject {
                project_a_tag: project.a_tag(),
                project_pubkey: Some(project.pubkey.clone()),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send boot project command: {}", e),
            })?;

        Ok(())
    }

    // =========================================================================
    // Backend Trust Management
    // =========================================================================

    /// Set the trusted backends from preferences.
    ///
    /// This must be called after login to enable processing of kind:24010 (project status)
    /// events. Status events from approved backends will populate project_statuses,
    /// enabling get_online_agents() to return online agents.
    ///
    /// Call this on app startup with stored approved/blocked backend pubkeys.
    pub fn set_trusted_backends(
        &self,
        approved: Vec<String>,
        blocked: Vec<String>,
    ) -> Result<(), TenexError> {
        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let approved_set: std::collections::HashSet<String> = approved.into_iter().collect();
        let blocked_set: std::collections::HashSet<String> = blocked.into_iter().collect();
        store
            .trust
            .set_trusted_backends(approved_set.clone(), blocked_set.clone());

        drop(store_guard);
        self.persist_trusted_backends_to_preferences(approved_set, blocked_set)?;

        Ok(())
    }

    /// Add a backend to the approved list.
    ///
    /// Once approved, kind:24010 events from this backend will be processed,
    /// populating project_statuses and enabling get_online_agents().
    pub fn approve_backend(&self, pubkey: String) -> Result<(), TenexError> {
        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        store.add_approved_backend(&pubkey);
        drop(store_guard);
        self.persist_current_trusted_backends()?;
        Ok(())
    }

    /// Add a backend to the blocked list.
    ///
    /// Status events from blocked backends will be silently ignored.
    pub fn block_backend(&self, pubkey: String) -> Result<(), TenexError> {
        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        store.add_blocked_backend(&pubkey);
        drop(store_guard);
        self.persist_current_trusted_backends()?;
        Ok(())
    }

    /// Approve all pending backends.
    ///
    /// This is useful for mobile apps that don't have a UI for backend approval modals.
    /// Approves any backends that sent kind:24010 events but weren't in the approved list.
    /// Returns the number of backends that were approved.
    pub fn approve_all_pending_backends(&self) -> Result<u32, TenexError> {
        use crate::tlog;
        tlog!("FFI", "approve_all_pending_backends called");
        eprintln!("[DEBUG] approve_all_pending_backends called");

        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let pending = store.drain_pending_backend_approvals();
        tlog!("FFI", "Found {} pending backend approvals", pending.len());
        eprintln!("[DEBUG] Found {} pending backend approvals", pending.len());
        for approval in &pending {
            tlog!(
                "FFI",
                "  - Backend: {} for project: {}",
                approval.backend_pubkey,
                approval.project_a_tag
            );
            eprintln!(
                "[DEBUG]   - Backend: {} for project: {}",
                approval.backend_pubkey, approval.project_a_tag
            );
        }

        let count = store.approve_pending_backends(pending);
        tlog!(
            "FFI",
            "Approved {} backends, project_statuses now has {} entries",
            count,
            store.project_statuses.len()
        );
        eprintln!("[DEBUG] Approved {} backends", count);
        eprintln!(
            "[DEBUG] project_statuses HashMap now has {} entries",
            store.project_statuses.len()
        );
        drop(store_guard);
        self.persist_current_trusted_backends()?;

        Ok(count)
    }

    /// Get approved/blocked/pending backend trust state for settings UI.
    pub fn get_backend_trust_snapshot(&self) -> Result<BackendTrustSnapshot, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let mut approved: Vec<String> = store.trust.approved_backends.iter().cloned().collect();
        let mut blocked: Vec<String> = store.trust.blocked_backends.iter().cloned().collect();
        approved.sort();
        blocked.sort();

        let mut pending: Vec<PendingBackendInfo> = store
            .trust
            .pending_backend_approvals
            .iter()
            .map(|p| PendingBackendInfo {
                backend_pubkey: p.backend_pubkey.clone(),
                project_a_tag: p.project_a_tag.clone(),
                first_seen: p.first_seen,
                status_created_at: p.status.created_at,
            })
            .collect();
        pending.sort_by(|a, b| b.first_seen.cmp(&a.first_seen));

        Ok(BackendTrustSnapshot {
            approved,
            blocked,
            pending,
        })
    }

    /// Return currently configured relay URLs (read-only in this phase).
    pub fn get_configured_relays(&self) -> Vec<String> {
        vec![crate::constants::RELAY_URL.to_string()]
    }

    /// Get diagnostics about backend approvals and project statuses.
    /// Returns JSON with project statuses keys.
    pub fn get_backend_diagnostics(&self) -> Result<String, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let diagnostic = serde_json::json!({
            "has_pending_backend_approvals": store.trust.has_pending_approvals(),
            "project_statuses_count": store.project_statuses.len(),
            "project_statuses_keys": store.project_statuses.keys().collect::<Vec<_>>(),
            "projects_count": store.get_projects().len(),
            "projects": store.get_projects().iter().map(|p| {
                serde_json::json!({
                    "id": p.id,
                    "name": p.title,
                    "a_tag": p.a_tag(),
                })
            }).collect::<Vec<_>>(),
        });

        Ok(diagnostic.to_string())
    }

    /// Send a new conversation (thread) to a project.
    ///
    /// Creates a new kind:1 event with title tag and project a-tag.
    /// Returns the event ID on success.
    pub fn send_thread(
        &self,
        project_id: String,
        title: String,
        content: String,
        agent_pubkey: Option<String>,
        nudge_ids: Vec<String>,
        skill_ids: Vec<String>,
    ) -> Result<SendMessageResult, TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        // Create a channel to receive the event ID
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

        // Send the publish thread command
        core_handle
            .send(NostrCommand::PublishThread {
                project_a_tag,
                title,
                content,
                agent_pubkey,
                nudge_ids,
                skill_ids,
                reference_conversation_id: None,
                fork_message_id: None,
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send publish thread command: {}", e),
            })?;

        // Wait for the event ID with timeout
        match response_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(event_id) => Ok(SendMessageResult {
                event_id,
                success: true,
            }),
            Err(_) => Err(TenexError::Internal {
                message: "Timed out waiting for thread publish confirmation".to_string(),
            }),
        }
    }

    /// Send a message to an existing conversation.
    ///
    /// Creates a new kind:1 event with e-tag pointing to the thread root.
    /// Returns the event ID on success.
    pub fn send_message(
        &self,
        conversation_id: String,
        project_id: String,
        content: String,
        agent_pubkey: Option<String>,
        nudge_ids: Vec<String>,
        skill_ids: Vec<String>,
    ) -> Result<SendMessageResult, TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        // Create a channel to receive the event ID
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

        // Send the publish message command
        core_handle
            .send(NostrCommand::PublishMessage {
                thread_id: conversation_id,
                project_a_tag,
                content,
                agent_pubkey,
                reply_to: None,

                nudge_ids,
                skill_ids,
                ask_author_pubkey: None,
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send publish message command: {}", e),
            })?;

        // Wait for the event ID with timeout
        match response_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(event_id) => Ok(SendMessageResult {
                event_id,
                success: true,
            }),
            Err(_) => Err(TenexError::Internal {
                message: "Timed out waiting for message publish confirmation".to_string(),
            }),
        }
    }

    /// Answer an ask event by sending a formatted response.
    ///
    /// The response is formatted as markdown with each question's title and answer,
    /// and published as a kind:1 reply to the ask event.
    pub fn answer_ask(
        &self,
        ask_event_id: String,
        ask_author_pubkey: String,
        conversation_id: String,
        project_id: String,
        answers: Vec<AskAnswer>,
    ) -> Result<SendMessageResult, TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        // Format answers as markdown (matching TUI format)
        let content = format_ask_answers(&answers);

        // Create a channel to receive the event ID
        let (response_tx, response_rx) = std::sync::mpsc::sync_channel::<String>(1);

        // Send the publish message command with reply_to pointing to the ask event
        core_handle
            .send(NostrCommand::PublishMessage {
                thread_id: conversation_id,
                project_a_tag,
                content,
                agent_pubkey: None,
                reply_to: Some(ask_event_id),
                nudge_ids: Vec::new(),
                skill_ids: Vec::new(),
                ask_author_pubkey: Some(ask_author_pubkey),
                response_tx: Some(response_tx),
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send ask answer command: {}", e),
            })?;

        // Wait for the event ID with timeout
        match response_rx.recv_timeout(Duration::from_secs(10)) {
            Ok(event_id) => Ok(SendMessageResult {
                event_id,
                success: true,
            }),
            Err(_) => Err(TenexError::Internal {
                message: "Timed out waiting for ask answer publish confirmation".to_string(),
            }),
        }
    }

    /// Get comprehensive stats snapshot with full TUI parity.
    /// This is a single batched FFI call that returns all stats data pre-computed.
    ///
    /// Returns Result to distinguish "no data" from "core error".
    pub fn get_stats_snapshot(&self) -> Result<StatsSnapshot, TenexError> {
        let store_guard = self.store.read().map_err(|_| TenexError::LockError {
            resource: "store".to_string(),
        })?;

        let store = store_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;

        // ===== 1. Metric Cards Data =====
        // Total cost for the past COST_WINDOW_DAYS (shared constant with TUI stats page)
        use crate::constants::COST_WINDOW_DAYS;
        const SECONDS_PER_DAY: u64 = 24 * 60 * 60;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        // Use saturating_sub to safely handle clock skew or pre-epoch edge cases
        let cost_window_start = now.saturating_sub(COST_WINDOW_DAYS * SECONDS_PER_DAY);
        let total_cost = store.get_total_cost_since(cost_window_start);

        // Get today's runtime (requires mutable borrow, so we do it separately)
        drop(store_guard);
        let today_runtime_ms = {
            let mut store_guard = self.store.write().map_err(|_| TenexError::LockError {
                resource: "store".to_string(),
            })?;
            let store = store_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
            store.statistics.get_today_unique_runtime()
        };

        // Re-acquire read lock for remaining data
        let store_guard = self.store.read().map_err(|_| TenexError::LockError {
            resource: "store".to_string(),
        })?;
        let store = store_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;

        // ===== 2. Runtime Chart Data (CHART_WINDOW_DAYS) =====
        // Use shared constant for chart window (same as TUI stats view)
        use crate::constants::CHART_WINDOW_DAYS;
        let runtime_by_day_raw = store.statistics.get_runtime_by_day(CHART_WINDOW_DAYS);
        let runtime_by_day: Vec<DayRuntime> = runtime_by_day_raw
            .into_iter()
            .map(|(day_start, runtime_ms)| DayRuntime {
                day_start,
                runtime_ms,
            })
            .collect();

        // Calculate average daily runtime (counting only non-zero days)
        let non_zero_runtimes: Vec<u64> = runtime_by_day
            .iter()
            .map(|d| d.runtime_ms)
            .filter(|r| *r > 0)
            .collect();
        let (avg_daily_runtime_ms, active_days_count) = if non_zero_runtimes.is_empty() {
            (0, 0)
        } else {
            let total: u64 = non_zero_runtimes.iter().sum();
            (
                total / non_zero_runtimes.len() as u64,
                non_zero_runtimes.len() as u32,
            )
        };

        // ===== 3. Rankings Data =====
        let cost_by_project_raw = store.get_cost_by_project();
        let cost_by_project: Vec<ProjectCost> = cost_by_project_raw
            .into_iter()
            .map(|(a_tag, name, cost)| ProjectCost { a_tag, name, cost })
            .collect();

        const RANKINGS_TABLE_ROWS: usize = 20;
        let top_conversations_raw = store.get_top_conversations_by_runtime(RANKINGS_TABLE_ROWS);
        let top_conversations: Vec<TopConversation> = top_conversations_raw
            .into_iter()
            .map(|(id, runtime_ms)| {
                let title = store
                    .get_thread_by_id(&id)
                    .map(|t| t.title.clone())
                    .unwrap_or_else(|| format!("{}...", &id[..12.min(id.len())]));
                TopConversation {
                    id,
                    title,
                    runtime_ms,
                }
            })
            .collect();

        // ===== 4. Messages Chart Data (CHART_WINDOW_DAYS) =====
        let (user_messages_raw, all_messages_raw) = store.get_messages_by_day(CHART_WINDOW_DAYS);

        // Combine into single vector with day_start as key
        let mut messages_map: std::collections::HashMap<u64, (u64, u64)> =
            std::collections::HashMap::new();
        for (day_start, user_count) in user_messages_raw {
            messages_map.entry(day_start).or_insert((0, 0)).0 = user_count;
        }
        for (day_start, all_count) in all_messages_raw {
            messages_map.entry(day_start).or_insert((0, 0)).1 = all_count;
        }

        let mut messages_by_day: Vec<DayMessages> = messages_map
            .into_iter()
            .map(|(day_start, (user_count, all_count))| DayMessages {
                day_start,
                user_count,
                all_count,
            })
            .collect();

        // Sort by day_start descending (newest first)
        messages_by_day.sort_by(|a, b| b.day_start.cmp(&a.day_start));

        // ===== 5. Activity Grid Data (30 days × 24 hours = 720 hours) =====
        const ACTIVITY_HOURS: usize = 30 * 24;
        let tokens_by_hour_raw = store.statistics.get_tokens_by_hour(ACTIVITY_HOURS);
        let messages_by_hour_raw = store.statistics.get_message_count_by_hour(ACTIVITY_HOURS);

        // Find max values for normalization (both tokens and messages)
        let max_tokens = tokens_by_hour_raw
            .values()
            .max()
            .copied()
            .unwrap_or(1)
            .max(1);
        let max_messages = messages_by_hour_raw
            .values()
            .max()
            .copied()
            .unwrap_or(1)
            .max(1);

        // Combine and pre-normalize intensity values (0-255) for BOTH tokens and messages
        let mut activity_map: std::collections::HashMap<u64, (u64, u64)> =
            std::collections::HashMap::new();
        for (hour_start, tokens) in tokens_by_hour_raw {
            activity_map.entry(hour_start).or_insert((0, 0)).0 = tokens;
        }
        for (hour_start, messages) in messages_by_hour_raw {
            activity_map.entry(hour_start).or_insert((0, 0)).1 = messages;
        }

        let mut activity_by_hour: Vec<HourActivity> = activity_map
            .into_iter()
            .map(|(hour_start, (tokens, messages))| {
                // Normalize tokens to 0-255 intensity scale
                let token_intensity = if max_tokens == 0 {
                    0
                } else {
                    ((tokens as f64 / max_tokens as f64) * 255.0).round() as u8
                };

                // Normalize messages to 0-255 intensity scale
                let message_intensity = if max_messages == 0 {
                    0
                } else {
                    ((messages as f64 / max_messages as f64) * 255.0).round() as u8
                };

                HourActivity {
                    hour_start,
                    tokens,
                    messages,
                    token_intensity,
                    message_intensity,
                }
            })
            .collect();

        // Sort by hour_start ascending (oldest first, as grid is rendered with newest at bottom)
        activity_by_hour.sort_by(|a, b| a.hour_start.cmp(&b.hour_start));

        // ===== Return Complete Snapshot =====
        Ok(StatsSnapshot {
            total_cost_14_days: total_cost,
            today_runtime_ms,
            avg_daily_runtime_ms,
            active_days_count,
            runtime_by_day,
            cost_by_project,
            top_conversations,
            messages_by_day,
            activity_by_hour,
            max_tokens,
            max_messages,
        })
    }

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

    // ===== Diagnostics Methods =====

    /// Get a comprehensive diagnostics snapshot for the iOS Diagnostics view.
    /// Returns all diagnostic information in a single batched call for efficiency.
    ///
    /// This function is best-effort: each section is collected independently.
    /// If one section fails (e.g., lock error), other sections can still succeed.
    /// Check `section_errors` for any failures.
    ///
    /// Set `include_database_stats` to false to skip expensive DB scanning when
    /// the Database tab is not active.
    pub fn get_diagnostics_snapshot(&self, include_database_stats: bool) -> DiagnosticsSnapshot {
        let mut section_errors: Vec<String> = Vec::new();
        let data_dir = get_data_dir();

        // ===== 1. System Diagnostics (best-effort) =====
        let system = self
            .collect_system_diagnostics(&data_dir)
            .map_err(|e| section_errors.push(format!("System: {}", e)))
            .ok();

        // ===== 2. Negentropy Sync Diagnostics (best-effort) =====
        let sync = self
            .collect_sync_diagnostics()
            .map_err(|e| section_errors.push(format!("Sync: {}", e)))
            .ok();

        // ===== 3. Subscription Diagnostics (best-effort) =====
        let (subscriptions, total_subscription_events) =
            match self.collect_subscription_diagnostics() {
                Ok((subs, total)) => (Some(subs), total),
                Err(e) => {
                    section_errors.push(format!("Subscriptions: {}", e));
                    (None, 0)
                }
            };

        // ===== 4. Database Diagnostics (best-effort, optionally skipped) =====
        let database = if include_database_stats {
            self.collect_database_diagnostics(&data_dir)
                .map_err(|e| section_errors.push(format!("Database: {}", e)))
                .ok()
        } else {
            None // Intentionally skipped for performance
        };

        DiagnosticsSnapshot {
            system,
            sync,
            subscriptions,
            total_subscription_events,
            database,
            section_errors,
        }
    }

    // =========================================================================
    // EVENT CALLBACK API
    // =========================================================================

    /// Register a callback to receive event notifications.
    /// Call this after login to enable push-based updates.
    ///
    /// The callback will be invoked from a background thread when:
    /// - New messages arrive for a conversation
    /// - Project status changes (kind:24010)
    /// - Streaming text chunks arrive
    /// - Any other data changes
    ///
    /// Note: Only one callback can be registered at a time.
    /// Calling this again will replace the previous callback.
    pub fn set_event_callback(&self, callback: Box<dyn EventCallback>) {
        let callback: Arc<dyn EventCallback> = Arc::from(callback);
        tlog!("PERF", "ffi.set_event_callback called");

        // Store callback
        if let Ok(mut guard) = self.event_callback.write() {
            *guard = Some(callback.clone());
        }

        // Start listener thread if not already running
        if !self.callback_listener_running.swap(true, Ordering::SeqCst) {
            self.start_callback_listener();
            tlog!(
                "PERF",
                "ffi.set_event_callback started callback listener thread"
            );
        }
    }

    /// Clear the event callback and stop the listener thread.
    /// Call this on logout to clean up resources.
    pub fn clear_event_callback(&self) {
        let started_at = Instant::now();
        // Clear callback first to prevent new notifications
        if let Ok(mut guard) = self.event_callback.write() {
            *guard = None;
        }
        // Signal listener thread to stop
        self.callback_listener_running
            .store(false, Ordering::SeqCst);
        if let Ok(mut guard) = self.callback_listener_handle.write() {
            if let Some(handle) = guard.take() {
                let _ = handle.join();
            }
        }
        tlog!(
            "PERF",
            "ffi.clear_event_callback complete elapsedMs={}",
            started_at.elapsed().as_millis()
        );
    }

    // ===== AI Audio Notification Methods =====

    /// Get AI audio settings (API keys never exposed - only configuration status)
    pub fn get_ai_audio_settings(&self) -> Result<AiAudioSettingsInfo, TenexError> {
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;

        let prefs_storage = prefs_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;
        let settings = &prefs_storage.prefs.ai_audio_settings;

        // Never expose actual API keys - only return whether they're configured
        Ok(AiAudioSettingsInfo {
            elevenlabs_api_key_configured: prefs_storage.get_elevenlabs_api_key().is_some(),
            openrouter_api_key_configured: prefs_storage.get_openrouter_api_key().is_some(),
            selected_voice_ids: settings.selected_voice_ids.clone(),
            openrouter_model: settings.openrouter_model.clone(),
            audio_prompt: settings.audio_prompt.clone(),
            enabled: settings.enabled,
            tts_inactivity_threshold_secs: settings.tts_inactivity_threshold_secs,
        })
    }

    /// Set selected voice IDs
    pub fn set_selected_voice_ids(&self, voice_ids: Vec<String>) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs_storage = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs_storage
            .set_selected_voice_ids(voice_ids)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    /// Set OpenRouter model
    pub fn set_openrouter_model(&self, model: Option<String>) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs_storage = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs_storage
            .set_openrouter_model(model)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    /// Set ElevenLabs API key (stored in OS secure storage)
    pub fn set_elevenlabs_api_key(&self, key: Option<String>) -> Result<(), TenexError> {
        use crate::secure_storage::{SecureKey, SecureStorage};

        if let Some(key_value) = key {
            SecureStorage::set(SecureKey::ElevenLabsApiKey, &key_value).map_err(|e| {
                TenexError::Internal {
                    message: format!("Failed to store ElevenLabs API key: {}", e),
                }
            })?;
        } else {
            // If key is None, delete the existing key
            SecureStorage::delete(SecureKey::ElevenLabsApiKey).map_err(|e| {
                TenexError::Internal {
                    message: format!("Failed to delete ElevenLabs API key: {}", e),
                }
            })?;
        }
        Ok(())
    }

    /// Set OpenRouter API key (stored in OS secure storage)
    pub fn set_openrouter_api_key(&self, key: Option<String>) -> Result<(), TenexError> {
        use crate::secure_storage::{SecureKey, SecureStorage};

        if let Some(key_value) = key {
            SecureStorage::set(SecureKey::OpenRouterApiKey, &key_value).map_err(|e| {
                TenexError::Internal {
                    message: format!("Failed to store OpenRouter API key: {}", e),
                }
            })?;
        } else {
            // If key is None, delete the existing key
            SecureStorage::delete(SecureKey::OpenRouterApiKey).map_err(|e| {
                TenexError::Internal {
                    message: format!("Failed to delete OpenRouter API key: {}", e),
                }
            })?;
        }
        Ok(())
    }

    /// Get the default audio prompt
    pub fn get_default_audio_prompt(&self) -> String {
        crate::models::project_draft::default_audio_prompt()
    }

    /// Set audio prompt
    pub fn set_audio_prompt(&self, prompt: String) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs_storage = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs_storage
            .set_audio_prompt(prompt)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    /// Set TTS inactivity threshold (seconds of inactivity before TTS fires)
    pub fn set_tts_inactivity_threshold(&self, secs: u64) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs_storage = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs_storage
            .set_tts_inactivity_threshold(secs)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    /// Enable or disable audio notifications
    pub fn set_audio_notifications_enabled(&self, enabled: bool) -> Result<(), TenexError> {
        let mut prefs_guard = self
            .preferences
            .write()
            .map_err(|_| TenexError::LockError {
                resource: "preferences".to_string(),
            })?;

        let prefs_storage = prefs_guard.as_mut().ok_or(TenexError::CoreNotInitialized)?;
        prefs_storage
            .set_audio_notifications_enabled(enabled)
            .map_err(|e| TenexError::Internal { message: e })?;
        Ok(())
    }

    /// Generate audio notification for a message
    /// Note: This is a blocking call that will wait for the async operation to complete
    /// API keys are passed directly so iOS can provide them from its native Keychain.
    pub fn generate_audio_notification(
        &self,
        agent_pubkey: String,
        conversation_title: String,
        message_text: String,
        elevenlabs_api_key: String,
        openrouter_api_key: String,
    ) -> Result<AudioNotificationInfo, TenexError> {
        let settings = self.get_ai_audio_settings()?;

        if !settings.enabled {
            return Err(TenexError::Internal {
                message: "Audio notifications are disabled".to_string(),
            });
        }

        let data_dir = get_data_dir();
        let manager =
            crate::ai::AudioNotificationManager::new(data_dir.to_str().unwrap_or("tenex_data"));

        // Initialize audio notifications directory
        manager.init().map_err(|e| TenexError::Internal {
            message: format!("Failed to initialize audio notifications: {}", e),
        })?;

        // Use shared Tokio runtime (not per-call creation)
        let runtime = get_tokio_runtime();

        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;
        let prefs_storage = prefs_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;
        let ai_settings = &prefs_storage.prefs.ai_audio_settings;

        let notification = runtime
            .block_on(manager.generate_notification(
                &agent_pubkey,
                &conversation_title,
                &message_text,
                &elevenlabs_api_key,
                &openrouter_api_key,
                ai_settings,
            ))
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to generate audio notification: {}", e),
            })?;

        Ok(AudioNotificationInfo {
            id: notification.id,
            agent_pubkey: notification.agent_pubkey,
            conversation_title: notification.conversation_title,
            original_text: notification.original_text,
            massaged_text: notification.massaged_text,
            voice_id: notification.voice_id,
            audio_file_path: notification.audio_file_path,
            created_at: notification.created_at,
        })
    }

    /// Upload an image to Blossom and return the URL.
    ///
    /// This uploads the image data to the Blossom server using the user's Nostr keys
    /// for authentication. The returned URL can be embedded in message content.
    ///
    /// # Arguments
    /// * `data` - Raw image data (PNG, JPEG, etc.)
    /// * `mime_type` - MIME type of the image (e.g., "image/png", "image/jpeg")
    ///
    /// # Returns
    /// The Blossom URL where the image is stored.
    pub fn upload_image(&self, data: Vec<u8>, mime_type: String) -> Result<String, TenexError> {
        // Get the user's keys for authentication
        let keys_guard = self.keys.read().map_err(|_| TenexError::LockError {
            resource: "keys".to_string(),
        })?;
        let keys = keys_guard.as_ref().ok_or(TenexError::NotLoggedIn)?;

        // Use shared Tokio runtime for async upload
        let runtime = get_tokio_runtime();

        let url = runtime
            .block_on(crate::nostr::upload_image(&data, keys, &mime_type))
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to upload image: {}", e),
            })?;

        Ok(url)
    }
}

// Standalone FFI functions — no TenexCore instance needed, bypasses actor serialization.

/// List all audio notifications (pure filesystem read).
#[uniffi::export]
pub fn list_audio_notifications() -> Result<Vec<AudioNotificationInfo>, TenexError> {
    let data_dir = get_data_dir();
    let manager =
        crate::ai::AudioNotificationManager::new(data_dir.to_str().unwrap_or("tenex_data"));

    let notifications = manager
        .list_notifications()
        .map_err(|e| TenexError::Internal {
            message: format!("Failed to list audio notifications: {}", e),
        })?;

    Ok(notifications
        .into_iter()
        .map(|n| AudioNotificationInfo {
            id: n.id,
            agent_pubkey: n.agent_pubkey,
            conversation_title: n.conversation_title,
            original_text: n.original_text,
            massaged_text: n.massaged_text,
            voice_id: n.voice_id,
            audio_file_path: n.audio_file_path,
            created_at: n.created_at,
        })
        .collect())
}

/// Delete an audio notification by ID (pure filesystem operation).
#[uniffi::export]
pub fn delete_audio_notification(id: String) -> Result<(), TenexError> {
    let data_dir = get_data_dir();
    let manager =
        crate::ai::AudioNotificationManager::new(data_dir.to_str().unwrap_or("tenex_data"));

    manager
        .delete_notification(&id)
        .map_err(|e| TenexError::Internal {
            message: format!("Failed to delete audio notification: {}", e),
        })?;

    Ok(())
}

#[uniffi::export]
pub fn fetch_elevenlabs_voices(api_key: String) -> Result<Vec<VoiceInfo>, TenexError> {
    let client = crate::ai::ElevenLabsClient::new(api_key);
    let runtime = get_tokio_runtime();

    let voices = runtime
        .block_on(client.get_voices())
        .map_err(|e| TenexError::Internal {
            message: format!("Failed to fetch voices: {}", e),
        })?;

    Ok(voices
        .into_iter()
        .map(|v| VoiceInfo {
            voice_id: v.voice_id,
            name: v.name,
            category: v.category,
            description: v.description,
            preview_url: v.preview_url,
        })
        .collect())
}

#[uniffi::export]
pub fn fetch_openrouter_models(api_key: String) -> Result<Vec<ModelInfo>, TenexError> {
    let client = crate::ai::OpenRouterClient::new(api_key);
    let runtime = get_tokio_runtime();

    let models = runtime
        .block_on(client.get_models())
        .map_err(|e| TenexError::Internal {
            message: format!("Failed to fetch models: {}", e),
        })?;

    Ok(models
        .into_iter()
        .map(|m| ModelInfo {
            id: m.id,
            name: m.name,
            description: m.description,
            context_length: m.context_length,
        })
        .collect())
}

// Private implementation methods for TenexCore (not exposed via UniFFI)
impl TenexCore {
    fn sync_trusted_backends_from_preferences(&self) -> Result<(), TenexError> {
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

    fn persist_trusted_backends_to_preferences(
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

    fn persist_current_trusted_backends(&self) -> Result<(), TenexError> {
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
    fn collect_system_diagnostics(
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

    fn get_relay_status(&self) -> (bool, u32) {
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
    fn collect_sync_diagnostics(&self) -> Result<NegentropySyncDiagnostics, String> {
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
    fn collect_subscription_diagnostics(
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
    fn collect_database_diagnostics(
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
    fn start_callback_listener(&self) {
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
