# tenex-core - Agent Guidelines

## Overview

The core Rust library powering both TUI and iOS clients. Provides:
- **Nostr Protocol** - Full Nostr support via nostr-sdk and nostrdb
- **Event Storage** - SQLite-backed local event store with nostrdb
- **FFI Layer** - UniFFI bindings for Swift/Kotlin interop
- **Shared Business Logic** - Authentication, project management, event handling

**Build Targets:**
- Library (`lib`) - For TUI/CLI usage
- C Dynamic Library (`cdylib`) - For iOS/Android FFI
- Static Library (`staticlib`) - For iOS static linking

## Architecture

```
┌─────────────────────────────────────────────┐
│               ffi.rs                        │
│  UniFFI-generated bindings (Swift/Kotlin)  │
│  - Sync API only (no async across FFI)     │
│  - Callbacks for event streaming            │
└─────────────────┬───────────────────────────┘
                  │
┌─────────────────▼───────────────────────────┐
│            runtime.rs                       │
│  Tokio runtime bridge for FFI              │
│  - Spawns background tasks                  │
│  - Manages async → sync conversion          │
└─────────────────┬───────────────────────────┘
                  │
        ┌─────────┴─────────┐
        │                   │
┌───────▼────────┐  ┌──────▼──────┐
│   nostr/       │  │   store/    │
│ - client.rs    │  │ - db.rs     │
│ - filters.rs   │  │ - queries   │
│ - relay mgmt   │  │ - events    │
└────────────────┘  └─────────────┘
```

## Critical Modules

### ffi.rs (130+ KB)
**Purpose:** UniFFI interface for Swift/iOS
- `TenexCore` - Main FFI object with all public methods
- No async functions (UniFFI limitation) - uses callbacks instead
- Polling-based refresh with adaptive timing
- All paths must be absolute (no relative paths)

**Key Constants:**
```rust
REFRESH_MAX_POLL_TIMEOUT_MS = 1000    // Max poll time
REFRESH_QUIET_PERIOD_MS = 100         // Early exit if quiet
REFRESH_THROTTLE_INTERVAL_MS = 500    // Min time between calls
```

### runtime.rs
**Purpose:** Tokio runtime management for FFI
- Singleton runtime for all async operations
- Blocks FFI calls until async work completes
- Manages background notification subscriptions

### nostr/
**Purpose:** Nostr protocol implementation
- Uses both nostr-sdk (relay management) and nostrdb (local storage)
- Filters defined for projects, conversations, messages
- Relay pool with automatic reconnection

### store/
**Purpose:** SQLite event storage via nostrdb
- Local-first architecture with sync from relays
- Queries optimized for common access patterns
- Transaction support for atomic operations

### models/
**Purpose:** Data structures and types
- Event wrappers (Project, Message, Agent, etc.)
- Nostr kind constants
- Serialization via serde

## Commands

```bash
# Build for development
cargo build -p tenex-core

# Build for iOS simulator
cargo build --target aarch64-apple-ios-sim --release -p tenex-core

# Build for iOS device
cargo build --target aarch64-apple-ios --release -p tenex-core

# Run tests
cargo test -p tenex-core

# Check without building
cargo check -p tenex-core

# Generate UniFFI bindings (Swift)
cargo bin uniffi-bindgen generate \
  --library target/aarch64-apple-ios-sim/release/libtenex_core.a \
  --language swift \
  --out-dir swift-bindings \
  src/tenex_core.udl
```

## FFI Conventions

### No Async Across FFI
UniFFI doesn't support async functions. Use this pattern:

```rust
// ❌ Won't compile with UniFFI
#[uniffi::export]
async fn fetch_data() -> Result<Data, Error> { ... }

// ✅ Use runtime.rs to bridge async
#[uniffi::export]
fn fetch_data_sync() -> Result<Data, Error> {
    runtime::block_on(async {
        fetch_data_async().await
    })
}
```

### Callbacks for Streaming
For event updates, use callbacks:

```rust
#[uniffi::export]
fn subscribe_to_events(callback: Box<dyn EventCallback>) {
    // Callback will be invoked from background thread
}
```

### Error Handling
```rust
#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum TenexError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Network error: {0}")]
    Network(String),
}
```

### Type Restrictions
UniFFI supports:
- Primitives (String, i32, u64, bool, etc.)
- Structs with primitive fields
- Enums (simple and with fields)
- Vec, HashMap, Option, Result
- Arc for shared ownership

UniFFI does NOT support:
- References (&T, &mut T)
- Lifetimes
- Trait objects (except in callbacks)
- Generic types (must be concrete)

## Nostr Event Kinds

**Must match AGENT_ARCHITECTURE.md:**
- `0` - User metadata (including agent profiles)
- `1` - Text notes
- `4199` - AgentDefinition (configuration template)
- `24010` - ProjectStatus (online agents)
- `31933` - Project (parameterized replaceable)

See [AGENT_ARCHITECTURE.md](../../AGENT_ARCHITECTURE.md) for full event schema.

## Database Schema

Uses nostrdb's built-in schema:
- Events table with indexed fields
- Profile cache for kind:0 events
- Full-text search support
- Transaction log for sync

## Performance Considerations

### Polling Strategy (iOS)
The FFI layer uses adaptive polling in `refresh()`:
1. Poll for events until quiet period (100ms) or max timeout (1s)
2. Throttle calls to max 1 per 500ms
3. Return early if no new events

**Why:** iOS frequently calls refresh(), but relays may still be sending historical events.

### Database Access
- Read queries use shared locks (RwLock)
- Write queries use exclusive locks
- Batch inserts when possible
- Index on pubkey, kind, created_at

### Memory Management
- Use Arc for shared ownership across FFI
- Avoid cloning large event structures
- Stream events rather than loading all into memory

## Testing

```bash
# Unit tests
cargo test -p tenex-core

# Specific module
cargo test -p tenex-core nostr::filters

# With logging
RUST_LOG=debug cargo test -p tenex-core -- --nocapture

# Integration tests (if exists)
cargo test -p tenex-core --test integration
```

## Debugging FFI Issues

### iOS Linking Errors
1. Check library exists: `ls -la target/aarch64-apple-ios-sim/release/libtenex_core.a`
2. Verify Xcode build settings point to correct path
3. Clean build: `cargo clean && tuist clean` (from ios-app/)

### Swift Import Errors
1. Regenerate bindings after UDL changes
2. Check modulemap path in Xcode settings
3. Verify Swift include paths contain `TenexCoreFFI/`

### Runtime Panics
- Check Rust logs with `RUST_LOG=trace`
- Verify tokio runtime is initialized
- Check for thread safety violations (use Arc/Mutex properly)

## Common Patterns

### Adding a New FFI Function

1. Define in UDL (if using separate UDL file):
```
interface TenexCore {
    Result<string> new_function(string arg);
};
```

2. Implement in ffi.rs:
```rust
#[uniffi::export]
impl TenexCore {
    pub fn new_function(&self, arg: String) -> Result<String, TenexError> {
        // Implementation
    }
}
```

3. Regenerate bindings:
```bash
cargo bin uniffi-bindgen generate ...
```

4. Update iOS code to use new function

### Adding a Nostr Filter

1. Define in `nostr/filters.rs`
2. Test with actual relay data
3. Document expected event kinds
4. Add error handling for malformed events

## Related

- [../AGENTS.md](../AGENTS.md) - Workspace conventions
- [../../AGENT_ARCHITECTURE.md](../../AGENT_ARCHITECTURE.md) - Nostr event architecture
- [../../ios-app/AGENTS.md](../../ios-app/AGENTS.md) - iOS integration details
