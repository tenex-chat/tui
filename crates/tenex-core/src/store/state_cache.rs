//! Disk-backed state cache for AppDataStore.
//!
//! Persists the reconstructed in-memory state to a binary file alongside the nostrdb data.
//! On subsequent startups, the cache is loaded instead of rebuilding from nostrdb,
//! reducing startup time from ~11s to <1s.
//!
//! # Cache invalidation
//! The cache is automatically invalidated when:
//! - `CACHE_SCHEMA_VERSION` is incremented (code change that alters the stored types)
//! - The cache file is missing or corrupt
//! - The cache is older than `MAX_CACHE_AGE_SECS`
//!
//! # Incremental catch-up
//! After a cache hit, AppDataStore queries nostrdb for events newer than
//! `max_created_at` (minus a small safety window) and applies them via
//! `handle_event`, keeping the in-memory state fully up to date.

use crate::models::{
    AgentDefinition, Lesson, MCPTool, Message, Nudge, Project, Report, Skill, TeamPack, Thread,
};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Increment this constant whenever the schema of `CachedState` or any of its
/// transitively-referenced types changes in a way that would make old caches
/// unreadable (e.g. adding/removing fields, changing field types).
///
/// Setting this to a new value causes all existing caches to be silently discarded
/// and rebuilt from nostrdb on the next startup.
pub const CACHE_SCHEMA_VERSION: u32 = 3;

/// Maximum cache age in seconds (7 days).
/// Caches older than this are discarded and rebuilt from nostrdb.
const MAX_CACHE_AGE_SECS: u64 = 7 * 24 * 60 * 60;

/// Versioned binary envelope wrapping the actual cache payload.
#[derive(Serialize, Deserialize)]
struct CacheEnvelope {
    schema_version: u32,
    /// Unix seconds when this cache was written.
    saved_at: u64,
    /// Highest Nostr event `created_at` timestamp seen across all cached events.
    ///
    /// Used as the baseline for incremental catch-up on the next startup (with a
    /// small safety window subtracted for clock skew).  Using `max_created_at`
    /// rather than `saved_at` ensures that late-arriving or backfilled events
    /// with `created_at < saved_at` are never permanently missed.
    max_created_at: u64,
    state: CachedState,
}

/// Snapshot of the AppDataStore's reconstructed data.
///
/// Only contains data that is expensive to rebuild from nostrdb.  Derived data
/// (statistics, runtime hierarchy) is cheaply re-derived from the snapshot after load.
#[derive(Serialize, Deserialize)]
pub struct CachedState {
    // Core project/thread/message data
    pub projects: Vec<Project>,
    pub threads_by_project: HashMap<String, Vec<Thread>>,
    pub messages_by_thread: HashMap<String, Vec<Message>>,
    pub profiles: HashMap<String, String>,
    /// Thread root IDs keyed by project a_tag — drives get_threads_by_ids().
    pub thread_root_index: HashMap<String, HashSet<String>>,

    // Content definitions
    pub agent_definitions: HashMap<String, AgentDefinition>,
    pub team_packs: HashMap<String, TeamPack>,
    pub mcp_tools: HashMap<String, MCPTool>,
    pub nudges: HashMap<String, Nudge>,
    pub skills: HashMap<String, Skill>,
    pub lessons: HashMap<String, Lesson>,

    // Reports (kind:30023)
    pub reports: HashMap<String, Report>,
    pub reports_all_versions: HashMap<String, Vec<Report>>,
    pub document_threads: HashMap<String, Vec<Thread>>,

    // Trust state
    pub approved_backends: HashSet<String>,
    pub blocked_backends: HashSet<String>,
}

/// Returns the path to the cache file inside `data_dir`.
pub fn cache_path(data_dir: &Path) -> PathBuf {
    data_dir.join("app_state_cache.bin")
}

/// Serialize `state` and write it atomically to `<data_dir>/app_state_cache.bin`.
///
/// Takes ownership of `state` to avoid a second full clone of the data on top of
/// the clone already performed by the caller when constructing `CachedState`.
///
/// Uses a write-to-temp-then-rename pattern to prevent partially-written files
/// from corrupting the cache on an unexpected shutdown mid-write.
///
/// Logs a warning (but does not panic) on any failure.
pub fn save_cache(
    data_dir: &Path,
    state: CachedState,
    max_created_at: u64,
) -> Result<(), Box<dyn std::error::Error>> {
    let saved_at = SystemTime::now().duration_since(UNIX_EPOCH)?.as_secs();

    let envelope = CacheEnvelope {
        schema_version: CACHE_SCHEMA_VERSION,
        saved_at,
        max_created_at,
        state,
    };

    let bytes = bincode::serialize(&envelope)?;

    let cache_file = cache_path(data_dir);
    let temp_file = cache_file.with_extension("bin.tmp");

    std::fs::write(&temp_file, &bytes)?;
    std::fs::rename(&temp_file, &cache_file)?;

    Ok(())
}

/// Attempt to load the cache from `<data_dir>/app_state_cache.bin`.
///
/// Returns `Some((state, max_created_at))` on success, `None` on any failure:
/// - File missing
/// - Corrupted / undeserializable data
/// - Schema version mismatch
/// - Cache too old (> `MAX_CACHE_AGE_SECS`)
///
/// The returned `max_created_at` is the highest Nostr event `created_at` seen when
/// the cache was saved.  The caller should subtract a small clock-skew safety window
/// (e.g. 5 minutes) before using it as the `.since()` filter for incremental catch-up.
pub fn load_cache(data_dir: &Path) -> Option<(CachedState, u64)> {
    let bytes = std::fs::read(cache_path(data_dir)).ok()?;

    let envelope: CacheEnvelope = bincode::deserialize(&bytes).ok()?;

    if envelope.schema_version != CACHE_SCHEMA_VERSION {
        tracing::info!(
            "state_cache: schema version mismatch (cached={} current={}) — discarding",
            envelope.schema_version,
            CACHE_SCHEMA_VERSION
        );
        return None;
    }

    let now = SystemTime::now().duration_since(UNIX_EPOCH).ok()?.as_secs();

    if now.saturating_sub(envelope.saved_at) > MAX_CACHE_AGE_SECS {
        tracing::info!(
            "state_cache: cache too old (age={}s max={}s) — discarding",
            now.saturating_sub(envelope.saved_at),
            MAX_CACHE_AGE_SECS
        );
        return None;
    }

    Some((envelope.state, envelope.max_created_at))
}

/// Delete the cache file (e.g. after a forced full rebuild or schema bump).
/// Ignores errors (e.g. file already absent).
pub fn invalidate_cache(data_dir: &Path) {
    let _ = std::fs::remove_file(cache_path(data_dir));
}
