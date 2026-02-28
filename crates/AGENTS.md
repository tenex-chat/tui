# Rust Crates

Cargo workspace with four crates:
- **tenex-core** - Shared library (Nostr, SQLite, UniFFI for iOS FFI)
- **tenex-repl** - Interactive REPL client
- **tenex-tui** - Terminal UI (Ratatui)
- **tenex-cli** - Command-line tool

## Commands

```bash
cargo build --workspace          # Build all
cargo test --workspace           # Test all
cargo check --workspace          # Fast type-check (no codegen)
cargo build -p tenex-core        # Build specific crate
cargo test -p tenex-core         # Test specific crate
cargo fmt --all                  # Format
cargo clippy --workspace -- -D warnings  # Lint
cargo run -p tenex-repl                  # Run REPL
RUST_LOG=debug cargo run -p tenex-tui    # Run TUI with logging
```

## iOS Build Targets

```bash
cargo build --target aarch64-apple-ios-sim --release -p tenex-core  # Simulator
cargo build --target aarch64-apple-ios --release -p tenex-core       # Device
```

## Conventions

- **Dependencies:** Use workspace deps via `.workspace = true`
- **Errors:** `thiserror` in libraries, `anyhow` in apps
- **Async:** `tokio::spawn` for background tasks, handle cancellation
- **FFI (tenex-core):** UniFFI-compatible types only, minimal surface
