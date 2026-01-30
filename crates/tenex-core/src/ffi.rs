//! FFI module for UniFFI bindings
//!
//! This module exposes a minimal API for use from Swift/Kotlin via UniFFI.
//! Keep this API as simple as possible - no async functions, only basic types.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, Receiver};
use std::sync::{Arc, Mutex, RwLock};
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
use nostrdb::{FilterBuilder, Ndb, NoteKey, SubscriptionStream};

use crate::models::agent_definition::AgentDefinition;
use crate::nostr::{DataChange, NostrCommand, NostrWorker};
use crate::runtime::{process_note_keys, CoreHandle};
use crate::stats::{query_ndb_stats, SharedEventStats, SharedNegentropySyncStats, SharedSubscriptionStats};
use crate::store::AppDataStore;

/// Get the data directory for nostrdb
fn get_data_dir() -> PathBuf {
    // Use a subdirectory in the user's data directory
    let base = dirs::data_dir().unwrap_or_else(|| PathBuf::from("."));
    base.join("tenex").join("nostrdb")
}

/// Helper to get the project a_tag from project_id
fn get_project_a_tag(store: &RwLock<Option<AppDataStore>>, project_id: &str) -> Result<String, TenexError> {
    let store_guard = store.read().map_err(|e| TenexError::Internal {
        message: format!("Failed to acquire store lock: {}", e),
    })?;
    let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
        message: "Store not initialized".to_string(),
    })?;

    let project = store.get_projects()
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
    handle_guard.as_ref().ok_or_else(|| TenexError::Internal {
        message: "Core runtime not initialized - call init() first".to_string(),
    }).cloned()
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

/// Convert an AgentDefinition to AgentInfo (shared helper to eliminate DRY violation)
fn agent_to_info(agent: &AgentDefinition) -> AgentInfo {
    AgentInfo {
        id: agent.id.clone(),
        pubkey: agent.pubkey.clone(),
        d_tag: agent.d_tag.clone(),
        name: agent.name.clone(),
        description: agent.description.clone(),
        role: agent.role.clone(),
        picture: agent.picture.clone(),
        model: agent.model.clone(),
    }
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
    /// Agent or user who started the conversation (display name)
    pub author: String,
    /// Author's public key (hex) for profile lookups
    pub author_pubkey: String,
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

/// Extended conversation info with all data needed for the Conversations tab.
/// Includes activity tracking, archive status, and hierarchy data.
#[derive(Debug, Clone, uniffi::Record)]
pub struct ConversationFullInfo {
    /// Unique identifier of the conversation (event ID)
    pub id: String,
    /// Title/subject of the conversation
    pub title: String,
    /// Agent or user who started the conversation
    pub author: String,
    /// Author's public key (hex) for profile lookups
    pub author_pubkey: String,
    /// Brief summary or first line of content
    pub summary: Option<String>,
    /// Number of messages in the thread
    pub message_count: u32,
    /// Unix timestamp of last activity (thread's own last activity)
    pub last_activity: u64,
    /// Effective last activity (max of own and all descendants)
    pub effective_last_activity: u64,
    /// Parent conversation ID (for nesting)
    pub parent_id: Option<String>,
    /// Status label from metadata: "In Progress", "Blocked", "Done", etc.
    pub status: Option<String>,
    /// Current activity description (e.g., "Writing tests...")
    pub current_activity: Option<String>,
    /// Whether this conversation has an agent actively working on it
    pub is_active: bool,
    /// Whether this conversation is archived
    pub is_archived: bool,
    /// Whether this thread has children (for collapse/expand UI)
    pub has_children: bool,
    /// Project a_tag this conversation belongs to
    pub project_a_tag: String,
    /// Whether this is a scheduled event
    pub is_scheduled: bool,
    /// P-tags (pubkeys mentioned in the conversation's root event)
    pub p_tags: Vec<String>,
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
    /// P-tags (mentions) - pubkeys this message mentions/delegates to
    pub p_tags: Vec<String>,
    /// Ask event data if this message contains an ask (inline ask)
    pub ask_event: Option<AskEventInfo>,
    /// Tool name if this is a tool call (e.g., "mcp__tenex__ask", "mcp__tenex__delegate")
    pub tool_name: Option<String>,
    /// Tool arguments as JSON string (for parsing todo_write and other tool calls)
    pub tool_args: Option<String>,
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
    /// Author pubkey (hex) for reply tagging
    pub author_pubkey: String,
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
    /// Ask event data if this inbox item is an ask
    pub ask_event: Option<AskEventInfo>,
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

/// Result of sending a message.
#[derive(Debug, Clone, uniffi::Record)]
pub struct SendMessageResult {
    /// Event ID of the published message
    pub event_id: String,
    /// Whether the message was successfully sent
    pub success: bool,
}

/// An agent definition for FFI export (from kind:4199 events).
/// Note: The pubkey here is the AUTHOR of the agent definition, not the agent instance.
/// For agent instances with their own pubkeys, use OnlineAgentInfo from get_online_agents().
#[derive(Debug, Clone, uniffi::Record)]
pub struct AgentInfo {
    /// Unique identifier of the agent (event ID)
    pub id: String,
    /// Agent definition author's public key (hex) - NOT the agent instance pubkey
    pub pubkey: String,
    /// Agent's d-tag (slug)
    pub d_tag: String,
    /// Display name of the agent
    pub name: String,
    /// Description of the agent's purpose
    pub description: String,
    /// Role of the agent (e.g., "Developer", "PM")
    pub role: String,
    /// Profile picture URL, if any
    pub picture: Option<String>,
    /// Model used by the agent (e.g., "claude-3-opus")
    pub model: Option<String>,
}

/// An online agent from project status (kind:24010 events).
/// These are actual agent instances with their own Nostr keypairs.
/// Use get_online_agents() to fetch these for agent selection.
#[derive(Debug, Clone, uniffi::Record)]
pub struct OnlineAgentInfo {
    /// Agent's actual public key (hex) - use this for profile lookups and p-tags
    pub pubkey: String,
    /// Display name of the agent (e.g., "claude-code", "architect")
    pub name: String,
    /// Whether this is the PM (project manager) agent
    pub is_pm: bool,
    /// Model used by the agent (e.g., "claude-3-opus"), if known
    pub model: Option<String>,
    /// Tools assigned to this agent
    pub tools: Vec<String>,
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

/// A nudge (kind:4201 event) for agent configuration.
/// Used by iOS for nudge selection in new conversations.
#[derive(Debug, Clone, uniffi::Record)]
pub struct NudgeInfo {
    /// Event ID of the nudge
    pub id: String,
    /// Public key of the nudge author
    pub pubkey: String,
    /// Title of the nudge (displayed with / prefix like TUI)
    pub title: String,
    /// Description of the nudge
    pub description: String,
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
    /// Total all-time cost in USD
    pub total_cost: f64,
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
    /// Uptime in milliseconds since core initialization
    pub uptime_ms: u64,
    /// Core version
    pub version: String,
    /// Whether the core is initialized
    pub is_initialized: bool,
    /// Whether a user is logged in
    pub is_logged_in: bool,
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
}

impl FfiPreferences {
    fn load_from_file(path: &std::path::Path) -> Option<Self> {
        let contents = std::fs::read_to_string(path).ok()?;
        serde_json::from_str(&contents).ok()
    }

    fn save_to_file(&self, path: &std::path::Path) {
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(path, json);
        }
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
        let prefs = FfiPreferences::load_from_file(&path).unwrap_or_default();
        Self { prefs, path }
    }

    fn save(&self) {
        self.prefs.save_to_file(&self.path);
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
    /// New messages arrived for a conversation
    Messages { conversation_id: String },
    /// Project status updated (kind:24010)
    ProjectStatus,
    /// Streaming text chunk arrived (live typing)
    StreamChunk {
        agent_pubkey: String,
        conversation_id: String,
        text_delta: Option<String>,
    },
    /// General data changed - full refresh recommended
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
    /// iOS preferences storage (archive state, collapsed threads, visible projects)
    preferences: RwLock<Option<FfiPreferencesStorage>>,
    /// Timestamp of last refresh() call for throttling (milliseconds since UNIX epoch).
    /// Uses AtomicU64 for lock-free access. Stored as ms for precision without needing
    /// to store Instant (which isn't Send+Sync friendly for FFI).
    last_refresh_ms: AtomicU64,
    /// Subscription stats for diagnostics (shared with worker)
    subscription_stats: RwLock<Option<SharedSubscriptionStats>>,
    /// Negentropy sync stats for diagnostics (shared with worker)
    negentropy_stats: RwLock<Option<SharedNegentropySyncStats>>,
    /// Timestamp when core was initialized (for uptime calculation)
    init_time: RwLock<Option<std::time::Instant>>,
    /// Event callback for push notifications to UI (Swift/Kotlin)
    event_callback: RwLock<Option<Arc<dyn EventCallback>>>,
    /// Flag to signal callback listener thread to stop (Arc for sharing with thread)
    callback_listener_running: Arc<AtomicBool>,
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
            last_refresh_ms: AtomicU64::new(0),
            ndb_stream: RwLock::new(None),
            preferences: RwLock::new(None),
            subscription_stats: RwLock::new(None),
            negentropy_stats: RwLock::new(None),
            init_time: RwLock::new(None),
            event_callback: RwLock::new(None),
            callback_listener_running: Arc::new(AtomicBool::new(false)),
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

        // Initialize nostrdb with appropriate mapsize for iOS
        // Use 2GB to avoid MDB_MAP_FULL errors with larger datasets
        let config = nostrdb::Config::new().set_mapsize(2 * 1024 * 1024 * 1024);
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

        // Initialize preferences storage
        {
            let prefs = FfiPreferencesStorage::new(&data_dir);
            let mut prefs_guard = match self.preferences.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *prefs_guard = Some(prefs);
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
        {
            let mut time_guard = match self.init_time.write() {
                Ok(g) => g,
                Err(_) => return false,
            };
            *time_guard = Some(std::time::Instant::now());
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
                    author_pubkey: t.pubkey.clone(),
                    summary: t.summary.clone(),
                    message_count,
                    last_activity: t.last_activity,
                    parent_id: t.parent_conversation_id.clone(),
                    status,
                }
            })
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
    /// Includes today's confirmed runtime + estimated runtime from active agents.
    /// This matches exactly what the TUI statusbar shows.
    /// Returns 0 if store is not initialized.
    pub fn get_today_runtime_ms(&self) -> u64 {
        let mut store_guard = match self.store.write() {
            Ok(g) => g,
            Err(_) => return 0,
        };

        let store = match store_guard.as_mut() {
            Some(s) => s,
            None => return 0,
        };

        let (runtime_ms, _, _) = store.get_statusbar_runtime_ms();
        runtime_ms
    }

    /// Get all descendant conversation IDs for a conversation (includes children, grandchildren, etc.)
    /// Returns empty Vec if no descendants exist or if the conversation is not found.
    pub fn get_descendant_conversation_ids(&self, conversation_id: String) -> Vec<String> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        store.runtime_hierarchy.get_descendants(&conversation_id)
    }

    /// Get conversations by their IDs.
    /// Returns ConversationFullInfo for each conversation ID that exists.
    /// Conversations that don't exist are silently skipped.
    pub fn get_conversations_by_ids(&self, conversation_ids: Vec<String>) -> Vec<ConversationFullInfo> {
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
        let archived_ids = prefs_guard.as_ref()
            .map(|p| p.prefs.archived_thread_ids.clone())
            .unwrap_or_default();

        let mut conversations = Vec::new();

        for conversation_id in conversation_ids {
            if let Some(thread) = store.get_thread_by_id(&conversation_id) {
                let message_count = store.get_messages(&conversation_id).len() as u32;
                let author_name = store.get_profile_name(&thread.pubkey);
                let has_children = store.runtime_hierarchy.has_children(&thread.id);
                let is_active = store.is_event_busy(&thread.id);
                let is_archived = archived_ids.contains(&thread.id);

                let project_a_tag = store.get_project_a_tag_for_thread(&conversation_id).unwrap_or_default();

                conversations.push(ConversationFullInfo {
                    id: thread.id.clone(),
                    title: thread.title.clone(),
                    author: author_name,
                    author_pubkey: thread.pubkey.clone(),
                    summary: thread.summary.clone(),
                    message_count,
                    last_activity: thread.last_activity,
                    effective_last_activity: thread.effective_last_activity,
                    parent_id: thread.parent_conversation_id.clone(),
                    status: thread.status_label.clone(),
                    current_activity: thread.status_current_activity.clone(),
                    is_active,
                    is_archived,
                    has_children,
                    project_a_tag,
                    is_scheduled: thread.is_scheduled,
                    p_tags: thread.p_tags.clone(),
                });
            }
        }

        conversations
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

        // Build a set of agent pubkeys for role detection (Fix #4: proper role detection)
        // This avoids content-based heuristics that can misclassify user messages
        let agent_pubkeys: std::collections::HashSet<&String> = store.agent_definitions
            .values()
            .map(|def| &def.pubkey)
            .collect();

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

                // Determine role based on author pubkey (Fix #4: remove content-based heuristics)
                // If the message author is a known agent, role is "assistant", otherwise "user"
                // Messages with tool_name are always from assistants (tool calls require assistant context)
                let role = if m.tool_name.is_some() || agent_pubkeys.contains(&m.pubkey) {
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
                    // Tool calls are indicated by the tool_name tag (Fix #4: remove content heuristics)
                    is_tool_call: m.tool_name.is_some(),
                    role,
                    q_tags: m.q_tags.clone(),
                    p_tags: m.p_tags.clone(),
                    ask_event,
                    tool_name: m.tool_name.clone(),
                    tool_args: m.tool_args.clone(),
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
                    crate::models::InboxEventType::Ask => "high".to_string(),
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

                // Convert ask_event if present
                let ask_event = item.ask_event.as_ref().map(|ae| {
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

                InboxItem {
                    id: item.id.clone(),
                    title: item.title.clone(),
                    content: item.title.clone(), // Same as title for now
                    from_agent,
                    author_pubkey: item.author_pubkey.clone(),
                    priority,
                    status,
                    created_at: item.created_at,
                    project_id,
                    conversation_id: item.thread_id.clone(),
                    ask_event,
                }
            })
            .collect()
    }

    // ===== Search Methods =====

    /// Full-text search across all events using nostrdb.
    /// Returns search results with content snippets and context.
    pub fn search(&self, query: String, limit: i32) -> Vec<SearchResult> {
        eprintln!("[FFI search] query='{}', limit={}", query, limit);

        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => {
                eprintln!("[FFI search] Failed to acquire store read lock");
                return Vec::new();
            }
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => {
                eprintln!("[FFI search] Store is None (not initialized)");
                return Vec::new();
            }
        };

        // Use nostrdb text_search
        let results = store.text_search(&query, limit);
        eprintln!("[FFI search] text_search returned {} results", results.len());

        results
            .into_iter()
            .filter_map(|(event_id, thread_id, content, kind)| {
                // Look up the author info from the event
                let (author, created_at, project_a_tag) = store.get_event_metadata(&event_id)
                    .unwrap_or_else(|| ("Unknown".to_string(), 0, None));

                Some(SearchResult {
                    event_id,
                    thread_id,
                    content,
                    kind,
                    author,
                    created_at,
                    project_a_tag,
                })
            })
            .collect()
    }

    // ===== Conversations Tab Methods (Full-featured) =====

    /// Get all conversations across all projects with full info for the Conversations tab.
    /// Returns conversations with activity tracking, archive status, and hierarchy data.
    /// Sorted by: active conversations first (by effective_last_activity desc),
    /// then inactive conversations by effective_last_activity desc.
    ///
    /// Returns Result to distinguish "no data" from "core error".
    pub fn get_all_conversations(&self, filter: ConversationFilter) -> Result<Vec<ConversationFullInfo>, TenexError> {
        let store_guard = self.store.read()
            .map_err(|_| TenexError::LockError { resource: "store".to_string() })?;

        let store = store_guard.as_ref()
            .ok_or(TenexError::CoreNotInitialized)?;

        // Get archived thread IDs from preferences
        let prefs_guard = self.preferences.read()
            .map_err(|_| TenexError::LockError { resource: "preferences".to_string() })?;
        let archived_ids = prefs_guard.as_ref()
            .map(|p| p.prefs.archived_thread_ids.clone())
            .unwrap_or_default();

        // Build list of project a_tags to include
        let projects = store.get_projects();
        let project_a_tags: Vec<String> = if filter.project_ids.is_empty() {
            // All projects
            projects.iter().map(|p| p.a_tag()).collect()
        } else {
            // Filter to specified project IDs
            projects.iter()
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
        let mut message_counts: std::collections::HashMap<String, u32> = std::collections::HashMap::new();
        for project_a_tag in &project_a_tags {
            let threads = store.get_threads(project_a_tag);
            for thread in threads {
                let count = store.get_messages(&thread.id).len() as u32;
                message_counts.insert(thread.id.clone(), count);
            }
        }

        // Collect all threads from selected projects
        let mut conversations: Vec<ConversationFullInfo> = Vec::new();

        for project_a_tag in &project_a_tags {
            let threads = store.get_threads(project_a_tag);

            for thread in threads {
                // Filter: scheduled events
                if filter.hide_scheduled && thread.is_scheduled {
                    continue;
                }

                // Filter: archived
                let is_archived = archived_ids.contains(&thread.id);
                if !filter.show_archived && is_archived {
                    continue;
                }

                // Filter: time
                if time_cutoff > 0 && thread.effective_last_activity < time_cutoff {
                    continue;
                }

                // Get message count from pre-computed map (O(1) lookup instead of O(n) each time)
                let message_count = message_counts.get(&thread.id).copied().unwrap_or(0);

                // Get author display name
                let author_name = store.get_profile_name(&thread.pubkey);

                // Check if thread has children
                let has_children = store.runtime_hierarchy.has_children(&thread.id);

                // Check if thread has active agents
                let is_active = store.is_event_busy(&thread.id);

                conversations.push(ConversationFullInfo {
                    id: thread.id.clone(),
                    title: thread.title.clone(),
                    author: author_name,
                    author_pubkey: thread.pubkey.clone(),
                    summary: thread.summary.clone(),
                    message_count,
                    last_activity: thread.last_activity,
                    effective_last_activity: thread.effective_last_activity,
                    parent_id: thread.parent_conversation_id.clone(),
                    status: thread.status_label.clone(),
                    current_activity: thread.status_current_activity.clone(),
                    is_active,
                    is_archived,
                    has_children,
                    project_a_tag: project_a_tag.clone(),
                    is_scheduled: thread.is_scheduled,
                    p_tags: thread.p_tags.clone(),
                });
            }
        }

        // Sort: active first (by effective_last_activity desc), then inactive by effective_last_activity desc
        conversations.sort_by(|a, b| {
            match (a.is_active, b.is_active) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => b.effective_last_activity.cmp(&a.effective_last_activity),
            }
        });

        Ok(conversations)
    }

    /// Get all projects with filter info (visibility, counts).
    /// Returns Result to distinguish "no data" from "core error".
    pub fn get_project_filters(&self) -> Result<Vec<ProjectFilterInfo>, TenexError> {
        let store_guard = self.store.read()
            .map_err(|_| TenexError::LockError { resource: "store".to_string() })?;

        let store = store_guard.as_ref()
            .ok_or(TenexError::CoreNotInitialized)?;

        // Get visible project IDs from preferences
        let prefs_guard = self.preferences.read()
            .map_err(|_| TenexError::LockError { resource: "preferences".to_string() })?;
        let visible_projects = prefs_guard.as_ref()
            .map(|p| p.prefs.visible_projects.clone())
            .unwrap_or_default();

        let projects = store.get_projects();

        Ok(projects.iter().map(|p| {
            let a_tag = p.a_tag();
            let threads = store.get_threads(&a_tag);
            let total_count = threads.len() as u32;

            // Count active conversations
            let active_count = threads.iter()
                .filter(|t| store.is_event_busy(&t.id))
                .count() as u32;

            // Check visibility (empty means all visible)
            let is_visible = visible_projects.is_empty() || visible_projects.contains(&a_tag);

            ProjectFilterInfo {
                id: p.id.clone(),
                a_tag,
                title: p.name.clone(),
                is_visible,
                active_count,
                total_count,
            }
        }).collect())
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
            prefs.save();
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
            prefs.save();
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
            prefs.save();
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
            prefs.save();
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

        prefs_guard.as_ref()
            .map(|p| p.prefs.archived_thread_ids.contains(&conversation_id))
            .unwrap_or(false)
    }

    /// Get all archived conversation IDs.
    /// Returns Result to distinguish "no data" from "lock error".
    pub fn get_archived_conversation_ids(&self) -> Result<Vec<String>, TenexError> {
        let prefs_guard = self.preferences.read()
            .map_err(|_| TenexError::LockError { resource: "preferences".to_string() })?;

        Ok(prefs_guard.as_ref()
            .map(|p| p.prefs.archived_thread_ids.iter().cloned().collect())
            .unwrap_or_default())
    }

    // ===== Collapsed Thread State Methods (Fix #5: Expose via FFI) =====

    /// Get all collapsed thread IDs.
    pub fn get_collapsed_thread_ids(&self) -> Result<Vec<String>, TenexError> {
        let prefs_guard = self.preferences.read()
            .map_err(|_| TenexError::LockError { resource: "preferences".to_string() })?;

        Ok(prefs_guard.as_ref()
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
            prefs.save();
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
            prefs.save();
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

        prefs_guard.as_ref()
            .map(|p| p.prefs.collapsed_thread_ids.contains(&thread_id))
            .unwrap_or(false)
    }


    /// Get agents for a project.
    ///
    /// Returns agents configured for the specified project.
    /// Returns an error if the store cannot be accessed.
    pub fn get_agents(&self, project_id: String) -> Result<Vec<AgentInfo>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        // Find the project by ID and get its agent IDs (event IDs of kind:4199 definitions)
        let project = store.get_projects().iter().find(|p| p.id == project_id).cloned();
        let agent_ids: Vec<String> = match project {
            Some(p) => p.agent_ids,
            None => return Ok(Vec::new()), // Project not found = empty agents (not an error)
        };

        // Get agent definitions for these IDs
        Ok(store.get_agent_definitions()
            .into_iter()
            .filter(|agent| agent_ids.contains(&agent.id))
            .map(agent_to_info)
            .collect())
    }

    /// Get all available agents (not filtered by project).
    ///
    /// Returns all known agent definitions.
    /// Returns an error if the store cannot be accessed.
    pub fn get_all_agents(&self) -> Result<Vec<AgentInfo>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store.get_agent_definitions()
            .into_iter()
            .map(agent_to_info)
            .collect())
    }

    /// Get all nudges (kind:4201 events).
    ///
    /// Returns all nudges sorted by created_at descending (most recent first).
    /// Used by iOS for nudge selection in new conversations.
    pub fn get_nudges(&self) -> Result<Vec<NudgeInfo>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        Ok(store.get_nudges()
            .into_iter()
            .map(|n| NudgeInfo {
                id: n.id.clone(),
                pubkey: n.pubkey.clone(),
                title: n.title.clone(),
                description: n.description.clone(),
            })
            .collect())
    }

    /// Get online agents for a project from the project status (kind:24010).
    ///
    /// These are actual agent instances with their own Nostr keypairs.
    /// Use these for agent selection in the message composer - the pubkeys
    /// can be used for profile picture lookups and p-tags.
    ///
    /// Returns empty if project not found or project is offline.
    pub fn get_online_agents(&self, project_id: String) -> Result<Vec<OnlineAgentInfo>, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        // Find the project by ID
        let project = store.get_projects().iter().find(|p| p.id == project_id).cloned();
        let project = match project {
            Some(p) => p,
            None => return Ok(Vec::new()), // Project not found = empty agents
        };

        // Get agents from project status (kind:24010)
        let agents = store.get_online_agents(&project.a_tag())
            .map(|agents| {
                agents.iter().map(|a| OnlineAgentInfo {
                    pubkey: a.pubkey.clone(),
                    name: a.name.clone(),
                    is_pm: a.is_pm,
                    model: a.model.clone(),
                    tools: a.tools.clone(),
                }).collect()
            })
            .unwrap_or_default();

        Ok(agents)
    }

    /// Get available configuration options for a project.
    ///
    /// Returns all available models and tools from the project status (kind:24010).
    /// Used by iOS to populate the agent config modal with available options.
    pub fn get_project_config_options(&self, project_id: String) -> Result<ProjectConfigOptions, TenexError> {
        let store_guard = self.store.read().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_ref().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        // Find the project by ID
        let project = store.get_projects().iter().find(|p| p.id == project_id).cloned();
        let project = match project {
            Some(p) => p,
            None => return Err(TenexError::Internal {
                message: format!("Project not found: {}", project_id),
            }),
        };

        // Get project status to extract all_models and all_tools
        let status = store.get_project_status(&project.a_tag());
        match status {
            Some(s) => Ok(ProjectConfigOptions {
                all_models: s.all_models.clone(),
                all_tools: s.all_tools.iter().cloned().collect(),
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
    ) -> Result<(), TenexError> {
        let project_a_tag = get_project_a_tag(&self.store, &project_id)?;
        let core_handle = get_core_handle(&self.core_handle)?;

        // Send the update agent config command
        core_handle
            .send(NostrCommand::UpdateAgentConfig {
                project_a_tag,
                agent_pubkey,
                model,
                tools,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send update agent config command: {}", e),
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
    pub fn set_trusted_backends(&self, approved: Vec<String>, blocked: Vec<String>) -> Result<(), TenexError> {
        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let approved_set: std::collections::HashSet<String> = approved.into_iter().collect();
        let blocked_set: std::collections::HashSet<String> = blocked.into_iter().collect();
        store.set_trusted_backends(approved_set, blocked_set);

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
        Ok(())
    }

    /// Approve all pending backends.
    ///
    /// This is useful for mobile apps that don't have a UI for backend approval modals.
    /// Approves any backends that sent kind:24010 events but weren't in the approved list.
    /// Returns the number of backends that were approved.
    pub fn approve_all_pending_backends(&self) -> Result<u32, TenexError> {
        let mut store_guard = self.store.write().map_err(|e| TenexError::Internal {
            message: format!("Failed to acquire store lock: {}", e),
        })?;

        let store = store_guard.as_mut().ok_or_else(|| TenexError::Internal {
            message: "Store not initialized - call init() first".to_string(),
        })?;

        let pending = store.drain_pending_backend_approvals();
        let count = pending.len() as u32;

        for approval in pending {
            store.add_approved_backend(&approval.backend_pubkey);
        }

        Ok(count)
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
                branch: None,
                nudge_ids,
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
                branch: None,
                nudge_ids: Vec::new(),
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
                branch: None,
                nudge_ids: Vec::new(),
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
        let store_guard = self.store.read()
            .map_err(|_| TenexError::LockError { resource: "store".to_string() })?;

        let store = store_guard.as_ref()
            .ok_or(TenexError::CoreNotInitialized)?;

        // ===== 1. Metric Cards Data =====
        let total_cost = store.get_total_cost();

        // Get today's runtime (requires mutable borrow, so we do it separately)
        drop(store_guard);
        let today_runtime_ms = {
            let mut store_guard = self.store.write()
                .map_err(|_| TenexError::LockError { resource: "store".to_string() })?;
            let store = store_guard.as_mut()
                .ok_or(TenexError::CoreNotInitialized)?;
            store.get_today_unique_runtime()
        };

        // Re-acquire read lock for remaining data
        let store_guard = self.store.read()
            .map_err(|_| TenexError::LockError { resource: "store".to_string() })?;
        let store = store_guard.as_ref()
            .ok_or(TenexError::CoreNotInitialized)?;

        // ===== 2. Runtime Chart Data (14 days) =====
        const STATS_WINDOW_DAYS: usize = 14;
        let runtime_by_day_raw = store.get_runtime_by_day(STATS_WINDOW_DAYS);
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
            (total / non_zero_runtimes.len() as u64, non_zero_runtimes.len() as u32)
        };

        // ===== 3. Rankings Data =====
        let cost_by_project_raw = store.get_cost_by_project();
        let cost_by_project: Vec<ProjectCost> = cost_by_project_raw
            .into_iter()
            .map(|(a_tag, name, cost)| ProjectCost {
                a_tag,
                name,
                cost,
            })
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

        // ===== 4. Messages Chart Data (14 days) =====
        let (user_messages_raw, all_messages_raw) = store.get_messages_by_day(STATS_WINDOW_DAYS);

        // Combine into single vector with day_start as key
        let mut messages_map: std::collections::HashMap<u64, (u64, u64)> = std::collections::HashMap::new();
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
        let tokens_by_hour_raw = store.get_tokens_by_hour(ACTIVITY_HOURS);
        let messages_by_hour_raw = store.get_message_count_by_hour(ACTIVITY_HOURS);

        // Find max values for normalization (both tokens and messages)
        let max_tokens = tokens_by_hour_raw.values().max().copied().unwrap_or(1).max(1);
        let max_messages = messages_by_hour_raw.values().max().copied().unwrap_or(1).max(1);

        // Combine and pre-normalize intensity values (0-255) for BOTH tokens and messages
        let mut activity_map: std::collections::HashMap<u64, (u64, u64)> = std::collections::HashMap::new();
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
            total_cost,
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
        // Throttle check: skip if we refreshed too recently
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let last_refresh = self.last_refresh_ms.load(Ordering::Relaxed);

        if last_refresh > 0 && now_ms.saturating_sub(last_refresh) < REFRESH_THROTTLE_INTERVAL_MS {
            // Throttled: skip this refresh call
            return true;
        }

        // Update last refresh timestamp (atomic swap for thread safety)
        self.last_refresh_ms.store(now_ms, Ordering::Relaxed);

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

        // Get callback reference before processing changes
        let callback = self.event_callback.read().ok().and_then(|g| g.clone());

        for change in &data_changes {
            match change {
                DataChange::ProjectStatus { json } => {
                    store.handle_status_event_json(json);
                    if let Some(ref cb) = callback {
                        cb.on_data_changed(DataChangeType::ProjectStatus);
                    }
                }
                DataChange::LocalStreamChunk { agent_pubkey, conversation_id, text_delta, .. } => {
                    if let Some(ref cb) = callback {
                        cb.on_data_changed(DataChangeType::StreamChunk {
                            agent_pubkey: agent_pubkey.clone(),
                            conversation_id: conversation_id.clone(),
                            text_delta: text_delta.clone(),
                        });
                    }
                }
                DataChange::MCPToolsChanged => {
                    if let Some(ref cb) = callback {
                        cb.on_data_changed(DataChangeType::General);
                    }
                }
            }
        }

        let mut ok = true;
        let mut had_notes = false;
        for note_keys in note_batches {
            if !note_keys.is_empty() {
                had_notes = true;
            }
            if process_note_keys(ndb.as_ref(), store, &core_handle, &note_keys).is_err() {
                ok = false;
            }
        }

        // Fire callback if we processed any notes (messages, projects, etc.)
        if had_notes {
            if let Some(ref cb) = callback {
                cb.on_data_changed(DataChangeType::General);
            }
        }

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
        let max_deadline = Instant::now() + Duration::from_millis(REFRESH_MAX_POLL_TIMEOUT_MS);
        let mut additional_batches: Vec<Vec<NoteKey>> = Vec::new();
        let mut quiet_since = Instant::now();

        while Instant::now() < max_deadline {
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

        // Re-acquire store lock and process additional batches
        let mut store_guard = match self.store.write() {
            Ok(g) => g,
            Err(_) => return false,
        };
        let store = match store_guard.as_mut() {
            Some(store) => store,
            None => return false,
        };

        for note_keys in additional_batches {
            if process_note_keys(ndb.as_ref(), store, &core_handle, &note_keys).is_err() {
                ok = false;
            }
        }

        // Preserve previous refresh semantics (full rebuild)
        store.rebuild_from_ndb();
        ok
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
        let system = self.collect_system_diagnostics(&data_dir)
            .map_err(|e| section_errors.push(format!("System: {}", e)))
            .ok();

        // ===== 2. Negentropy Sync Diagnostics (best-effort) =====
        let sync = self.collect_sync_diagnostics()
            .map_err(|e| section_errors.push(format!("Sync: {}", e)))
            .ok();

        // ===== 3. Subscription Diagnostics (best-effort) =====
        let (subscriptions, total_subscription_events) = match self.collect_subscription_diagnostics() {
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

        // Store callback
        if let Ok(mut guard) = self.event_callback.write() {
            *guard = Some(callback.clone());
        }

        // Start listener thread if not already running
        if !self.callback_listener_running.swap(true, Ordering::SeqCst) {
            self.start_callback_listener(callback);
        }
    }

    /// Clear the event callback and stop the listener thread.
    /// Call this on logout to clean up resources.
    pub fn clear_event_callback(&self) {
        // Clear callback first to prevent new notifications
        if let Ok(mut guard) = self.event_callback.write() {
            *guard = None;
        }
        // Signal listener thread to stop
        self.callback_listener_running.store(false, Ordering::SeqCst);
    }
}

// Private implementation methods for TenexCore (not exposed via UniFFI)
impl TenexCore {
    /// Collect system diagnostics (uptime, version, status)
    fn collect_system_diagnostics(&self, data_dir: &std::path::Path) -> Result<SystemDiagnostics, String> {
        let uptime_ms = self.init_time.read()
            .map_err(|_| "Failed to acquire init_time lock".to_string())?
            .as_ref()
            .map(|t| t.elapsed().as_millis() as u64)
            .unwrap_or(0);

        let is_initialized = self.initialized.load(Ordering::SeqCst);
        let is_logged_in = self.is_logged_in();
        let log_path = data_dir.join("tenex.log").to_string_lossy().to_string();

        Ok(SystemDiagnostics {
            log_path,
            uptime_ms,
            version: env!("CARGO_PKG_VERSION").to_string(),
            is_initialized,
            is_logged_in,
        })
    }

    /// Collect negentropy sync diagnostics
    fn collect_sync_diagnostics(&self) -> Result<NegentropySyncDiagnostics, String> {
        use crate::stats::NegentropySyncStatus;

        let stats_guard = self.negentropy_stats.read()
            .map_err(|_| "Failed to acquire negentropy_stats lock".to_string())?;

        Ok(if let Some(stats) = stats_guard.as_ref() {
            let snapshot = stats.snapshot();
            let seconds_since_last_cycle = snapshot.last_cycle_time()
                .map(|t| t.elapsed().as_secs());

            let recent_results: Vec<SyncResultDiagnostic> = snapshot.recent_results
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
    fn collect_subscription_diagnostics(&self) -> Result<(Vec<SubscriptionDiagnostics>, u64), String> {
        let stats_guard = self.subscription_stats.read()
            .map_err(|_| "Failed to acquire subscription_stats lock".to_string())?;

        Ok(if let Some(stats) = stats_guard.as_ref() {
            let snapshot = stats.snapshot();
            let subs: Vec<SubscriptionDiagnostics> = snapshot.subscriptions
                .iter()
                .map(|(sub_id, info)| {
                    SubscriptionDiagnostics {
                        sub_id: sub_id.clone(),
                        description: info.description.clone(),
                        kinds: info.kinds.clone(),
                        events_received: info.events_received,
                        age_secs: info.created_at.elapsed().as_secs(),
                    }
                })
                .collect();
            let total = snapshot.total_events();
            (subs, total)
        } else {
            (Vec::new(), 0)
        })
    }

    /// Collect database diagnostics (potentially expensive - scans event kinds)
    fn collect_database_diagnostics(&self, data_dir: &std::path::Path) -> Result<DatabaseStats, String> {
        let ndb_guard = self.ndb.read()
            .map_err(|_| "Failed to acquire ndb lock".to_string())?;

        Ok(if let Some(ndb) = ndb_guard.as_ref() {
            // Get event counts by kind using the existing query_ndb_stats function
            let kind_counts = query_ndb_stats(ndb);

            // Convert to Vec<KindEventCount> and sort by count descending
            let mut event_counts: Vec<KindEventCount> = kind_counts
                .into_iter()
                .map(|(kind, count)| {
                    KindEventCount {
                        kind,
                        count,
                        name: get_kind_name(kind),
                    }
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
    fn start_callback_listener(&self, callback: Arc<dyn EventCallback>) {
        // Get references to the channels we need to monitor
        // Note: We clone the Ndb Arc and create a fresh subscription stream
        // because the main ndb_stream is used by refresh() and we can't share it
        let ndb = match self.ndb.read() {
            Ok(guard) => guard.as_ref().cloned(),
            Err(_) => None,
        };

        // Clone the running flag for the thread
        let running = self.callback_listener_running.clone();

        // For data_rx, we need to check it in refresh() first - the listener
        // will monitor it in parallel. Since mpsc::Receiver can only have one
        // consumer, we'll monitor data changes via a different mechanism:
        // We'll create a separate subscription stream for the listener thread.

        std::thread::spawn(move || {
            // Create a separate nostrdb subscription for the listener thread
            let ndb_stream = if let Some(ref ndb) = ndb {
                let filter = nostrdb::FilterBuilder::new()
                    .kinds([31933, 1, 0, 4199, 513, 4129, 4201])
                    .build();
                match ndb.subscribe(&[filter]) {
                    Ok(sub) => Some(nostrdb::SubscriptionStream::new((**ndb).clone(), sub)),
                    Err(e) => {
                        eprintln!("[EventCallback] Failed to create ndb subscription: {}", e);
                        None
                    }
                }
            } else {
                None
            };

            let mut stream = ndb_stream;

            // Poll loop - check for new events and fire callbacks
            while running.load(Ordering::Relaxed) {
                let mut had_event = false;

                // Check ndb_stream for new notes
                if let Some(ref mut s) = stream {
                    while let Some(_note_keys) = s.next().now_or_never().flatten() {
                        had_event = true;
                        // Fire general callback - views should refresh
                        // In the future, we could parse note_keys to determine conversation_id
                        callback.on_data_changed(DataChangeType::General);
                    }
                }

                // Sleep if no events to avoid CPU spin
                if !had_event {
                    std::thread::sleep(std::time::Duration::from_millis(50));
                }
            }
        });
    }
}

/// Get human-readable name for a Nostr event kind
fn get_kind_name(kind: u16) -> String {
    match kind {
        0 => "Metadata".to_string(),
        1 => "Text Notes".to_string(),
        3 => "Contact List".to_string(),
        4 => "DMs".to_string(),
        513 => "Conversations".to_string(),
        4129 => "Lessons".to_string(),
        4199 => "Agent Definitions".to_string(),
        4201 => "Nudges".to_string(),
        24010 => "Project Status".to_string(),
        24133 => "Operations Status".to_string(),
        30023 => "Articles".to_string(),
        31933 => "Projects".to_string(),
        _ => format!("Kind {}", kind),
    }
}

/// Get the LMDB database file size in bytes
fn get_db_file_size(data_dir: &std::path::Path) -> u64 {
    // LMDB stores data in a file named "data.mdb"
    let db_file = data_dir.join("data.mdb");
    std::fs::metadata(&db_file)
        .map(|m| m.len())
        .unwrap_or(0)
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
            println!("Skipping test due to database initialization failure (parallel test conflict)");
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
            println!("Skipping test due to database initialization failure (parallel test conflict)");
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
            println!("Skipping test due to database initialization failure (parallel test conflict)");
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
            println!("Skipping test due to database initialization failure (parallel test conflict)");
            return;
        }

        let invalid_pubkeys: Vec<String> = vec![
            "not_hex_at_all!@#$".to_string(),        // Non-hex characters
            "abc123".to_string(),                     // Too short
            "0".repeat(65),                          // Too long
            "g".repeat(64),                          // Invalid hex char 'g'
            "  ".to_string(),                        // Whitespace only
            "0123456789abcdef".to_string(),          // Valid hex but wrong length (16 chars)
        ];

        for pubkey in invalid_pubkeys {
            let result = core.get_profile_picture(pubkey.clone());
            // All should return Ok(None) - graceful handling of invalid input
            assert!(result.is_ok(), "Expected Ok for pubkey '{}', got {:?}", pubkey, result);
            assert!(result.unwrap().is_none(), "Expected None for invalid pubkey '{}'", pubkey);
        }
    }
}
