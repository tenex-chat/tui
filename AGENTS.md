# TENEX TUI Client - Agent Guidelines

Multi-platform TENEX client: Rust TUI (Ratatui), iOS app (SwiftUI + Rust FFI), shared core (tenex-core).

## Commands

```bash
# Build & Test
cargo build --workspace
cargo test --workspace

# Run TUI
cargo run -p tenex-tui

# iOS (see ios-app/AGENTS.md for details)
cargo build --target aarch64-apple-ios-sim --release -p tenex-core
cd ios-app && tuist generate
```

## Conventions

- **Rust:** rustfmt, explicit error types in library code
- **Swift:** SwiftUI + MVVM, async/await for FFI
- **Git:** Worktrees in `.worktrees/`, main branch is `master`

## Key References

- `AGENT_ARCHITECTURE.md` - Nostr agent/event system (read before Nostr work)
- `crates/*/AGENTS.md` - Crate-specific guidance
- `ios-app/AGENTS.md` - iOS build and FFI details
