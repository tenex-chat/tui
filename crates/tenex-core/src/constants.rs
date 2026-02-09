//! Application-wide constants
//!
//! Centralized location for magic strings and configuration values
//! that are used across multiple modules.

/// Default Nostr relay URL
pub const RELAY_URL: &str = "wss://tenex.chat";

/// Default Blossom server for blob uploads
pub const BLOSSOM_SERVER: &str = "https://blossom.primal.net";

// Agent defaults
pub const DEFAULT_AGENT_NAME: &str = "Unnamed Agent";
pub const DEFAULT_AGENT_ROLE: &str = "assistant";

// Thread defaults
pub const DEFAULT_THREAD_TITLE: &str = "Untitled";

// Nudge defaults
pub const DEFAULT_NUDGE_TITLE: &str = "Untitled";

/// Staleness threshold in seconds - status older than this is considered offline
pub const STALENESS_THRESHOLD_SECS: u64 = 5 * 60; // 5 minutes

// Stats window constants
/// Number of days for the cost display window (used in Stats tab and FFI).
/// This is separate from the chart window to allow independent tuning.
/// Note: Both TUI stats.rs and FFI ffi.rs use this constant for cost calculations.
pub const COST_WINDOW_DAYS: u64 = 14;

/// Number of days for the chart display window (runtime, messages charts).
/// Used by TUI stats view for chart rendering.
pub const CHART_WINDOW_DAYS: usize = 14;

// Nostr event kinds used by TENEX
pub mod kinds {
    /// Text note (thread or message)
    pub const TEXT_NOTE: u16 = 1;
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
    /// Boot request
    pub const BOOT_REQUEST: u16 = 24000;
    /// Project status
    pub const PROJECT_STATUS: u16 = 24010;
    /// Agent config update
    pub const AGENT_CONFIG: u16 = 24020;
    /// Operations status
    pub const OPERATIONS_STATUS: u16 = 24133;
    /// Stop operations command
    pub const STOP_OPERATIONS: u16 = 24134;
    /// Report/article
    pub const REPORT: u16 = 30023;
    /// Project definition (NIP-33 replaceable)
    pub const PROJECT: u16 = 31933;
    /// Blossom upload authorization
    pub const BLOSSOM_AUTH: u16 = 24242;
}
