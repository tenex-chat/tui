# Rust Crates - Agent Guidelines

## Workspace Structure

This is a Cargo workspace with three crates:
- **tenex-core** - Shared library with Nostr, SQLite, UniFFI (cdylib for iOS)
- **tenex-tui** - Terminal UI application using Ratatui
- **tenex-cli** - Command-line interface tool

## Commands

```bash
# Build all crates
cargo build --workspace

# Build specific crate
cargo build -p tenex-core
cargo build -p tenex-tui --release

# Test all crates
cargo test --workspace

# Test specific crate
cargo test -p tenex-core

# Run TUI with debug logging
RUST_LOG=debug cargo run -p tenex-tui

# Check without building
cargo check --workspace

# Format code
cargo fmt --all

# Lint
cargo clippy --workspace -- -D warnings
```

## Workspace Dependencies

Shared dependencies defined in root `Cargo.toml` [workspace.dependencies]:
- **Nostr:** nostr-sdk, nostr-ndb, nostrdb
- **Async:** tokio with full features
- **Serialization:** serde, serde_json
- **TUI:** ratatui, crossterm
- **UniFFI:** uniffi for Rust → Swift bindings
- **Database:** rusqlite with bundled SQLite

**Important:** Use workspace dependencies via `.workspace = true` in crate Cargo.toml files

## Conventions

### Error Handling
- **Libraries (tenex-core):** Use `thiserror` for explicit error types
- **Applications (tenex-tui, tenex-cli):** Can use `anyhow` for application errors
- Always propagate errors with `?` rather than unwrap/expect in library code

### Module Organization
```rust
// Public API in lib.rs or mod.rs
pub mod public_module;

// Internal utilities
mod internal;

// Re-exports for cleaner API
pub use public_module::ImportantType;
```

### Async Code
- Use `tokio::spawn` for background tasks
- Prefer `async fn` over manual Future implementations
- Always handle task cancellation properly

### FFI (tenex-core only)
- All FFI types must be UniFFI-compatible
- Document FFI boundaries with comments
- Keep FFI surface minimal - use opaque handles where possible

## Testing

### Unit Tests
```rust
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_something() {
        assert_eq!(1 + 1, 2);
    }
}
```

### Async Tests
```rust
#[tokio::test]
async fn test_async_function() {
    let result = async_function().await;
    assert!(result.is_ok());
}
```

### Integration Tests
- Place in `tests/` directory at crate root
- Test public API only
- Use test databases with unique names

## Performance

- Enable release mode for profiling: `cargo build --release`
- Use `tracing` spans for observability
- Avoid blocking calls in async contexts
- Use `parking_lot` for sync primitives (already in workspace deps)

## Common Patterns

### Nostr Client Usage
```rust
use nostr_sdk::{Client, Keys, RelayPoolNotification};

let keys = Keys::parse("nsec...")?;
let client = Client::new(&keys);
client.add_relay("wss://relay.example.com").await?;
client.connect().await;
```

### SQLite Access
```rust
use rusqlite::{Connection, params};

let conn = Connection::open("db.sqlite")?;
conn.execute(
    "INSERT INTO events (id, content) VALUES (?1, ?2)",
    params![id, content],
)?;
```

### UniFFI Export (tenex-core)
```rust
#[uniffi::export]
pub fn rust_function(arg: String) -> Result<String, CoreError> {
    Ok(format!("Hello, {}", arg))
}
```

## Debugging

### Enable Logging
```bash
# All traces
RUST_LOG=trace cargo run -p tenex-tui

# Specific module
RUST_LOG=tenex_core::nostr=debug cargo run -p tenex-tui

# Multiple modules
RUST_LOG=tenex_core=debug,tenex_tui::ui=trace cargo run -p tenex-tui
```

### OpenTelemetry Traces
- Access at http://localhost:16686/ when running with tracing enabled
- Use spans to track async operations
- Correlate traces with user actions

### Database Inspection
```bash
# Open SQLite database
sqlite3 tenex.db

# List tables
.tables

# Query events
SELECT * FROM events LIMIT 10;
```

## Build Targets

### Standard (development)
```bash
cargo build
```

### iOS Simulator
```bash
cargo build --target aarch64-apple-ios-sim --release -p tenex-core
# Output: target/aarch64-apple-ios-sim/release/libtenex_core.a
```

### iOS Device
```bash
cargo build --target aarch64-apple-ios --release -p tenex-core
# Output: target/aarch64-apple-ios/release/libtenex_core.a
```

## Related

- [tenex-core/AGENTS.md](./tenex-core/AGENTS.md) - Core library specifics
- [Root AGENTS.md](../AGENTS.md) - Overall project guidelines
