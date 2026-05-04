//! Application-wide constants
//!
//! Centralized location for magic strings and configuration values
//! that are used across multiple modules.

/// Default Nostr relay URL
pub const RELAY_URL: &str = "wss://relay.tenex.chat";

/// Default Blossom server for blob uploads
pub const BLOSSOM_SERVER: &str = "https://blossom.primal.net";

// Agent defaults
pub const DEFAULT_AGENT_NAME: &str = "Unnamed Agent";
pub const DEFAULT_AGENT_ROLE: &str = "assistant";

// Thread defaults
pub const DEFAULT_THREAD_TITLE: &str = "Untitled";

// Nudge defaults
pub const DEFAULT_NUDGE_TITLE: &str = "Untitled";

// Skill defaults
pub const DEFAULT_SKILL_TITLE: &str = "Untitled";

/// Staleness threshold in seconds - status older than this is considered offline
pub const STALENESS_THRESHOLD_SECS: u64 = 45; // 45 seconds

// Inbox filtering constants
/// Hard cap for inbox items: 48 hours in seconds (48 * 60 * 60 = 172,800).
/// Keeps the inbox focused on recent items requiring attention.
/// Note: Used by both TUI (Rust) and iOS (Swift) for consistent filtering.
pub const INBOX_48H_CAP_SECONDS: u64 = 48 * 60 * 60;

// Stats window constants
/// Number of days for the cost display window (used in Stats tab and FFI).
/// This is separate from the chart window to allow independent tuning.
/// Note: Both TUI stats.rs and FFI (`ffi/mod.rs` + `ffi/stats_api.rs`) use this constant.
pub const COST_WINDOW_DAYS: u64 = 14;

/// Number of days for the chart display window (runtime, messages charts).
/// Used by TUI stats view for chart rendering.
pub const CHART_WINDOW_DAYS: usize = 14;

// Nostr event kinds used by TENEX
pub mod kinds {
    /// Text note (thread or message)
    pub const TEXT_NOTE: u16 = 1;
    /// Reaction (NIP-25)
    pub const REACTION: u16 = 7;
    /// Metadata (profiles)
    pub const METADATA: u16 = 0;
    /// Conversation metadata (title, summary, status)
    pub const CONVERSATION_METADATA: u16 = 513;
    /// Agent definition
    pub const AGENT_DEFINITION: u16 = 4199;
    /// Agent lesson/learning
    pub const AGENT_LESSON: u16 = 4129;
    /// Nudge/prompt
    pub const NUDGE: u16 = 4201;
    /// Skill (agent skill instruction set)
    pub const SKILL: u16 = 4202;
    /// Comment (NIP-22)
    pub const COMMENT: u16 = 1111;
    /// Boot request
    pub const BOOT_REQUEST: u16 = 24000;
    /// Project runtime/status advertisement (ephemeral).
    ///
    /// Owned by a backend. This is heartbeat/runtime traffic only: it is
    /// **not** roster membership, PM/default state, agent availability, or
    /// any agent's current config. Roster comes from kind:31933, agent
    /// availability from kind:24011, per-agent config state from kind:34011.
    /// See `docs/agent-identity-config-implementation-decisions.md`.
    pub const PROJECT_STATUS: u16 = 24010;
    /// Backend agent inventory (ephemeral).
    ///
    /// Advertises which agent pubkeys are available from a given backend.
    /// Source of truth for agent availability/online labels.
    pub const BACKEND_INVENTORY: u16 = 24011;
    /// Agent config change *request/command* (ephemeral).
    ///
    /// This event asks an agent to update its configuration. It is **not**
    /// durable config state — durable per-agent config lives in kind:34011,
    /// authored by the agent. UIs publish `AGENT_CONFIG_REQUEST` and confirm
    /// the change only when a matching/new kind:34011 arrives.
    pub const AGENT_CONFIG_REQUEST: u16 = 24020;
    /// Operations status
    pub const OPERATIONS_STATUS: u16 = 24133;
    /// Stop operations command
    pub const STOP_OPERATIONS: u16 = 24134;
    /// Ephemeral text stream delta (live agent typing)
    pub const STREAM_TEXT_DELTA: u16 = 24135;
    /// Report/article
    pub const REPORT: u16 = 30023;
    /// Project definition (NIP-33 replaceable)
    pub const PROJECT: u16 = 31933;
    /// Per-agent durable config state (NIP-33 replaceable).
    ///
    /// Authored by the agent. Source of truth for the agent's currently
    /// active model/tools/skills/MCP servers and the catalog of available
    /// options. Config UIs read from this kind and publish
    /// `AGENT_CONFIG_REQUEST` (kind:24020) to request changes.
    pub const AGENT_CONFIG_STATE: u16 = 34011;
    /// Team pack definition (NIP-33 replaceable)
    pub const TEAM_PACK: u16 = 34199;
    /// Blossom upload authorization
    pub const BLOSSOM_AUTH: u16 = 24242;
    /// Push notification registration (APNs/FCM device token)
    pub const PUSH_NOTIFICATION_REGISTRATION: u16 = 25000;
}
