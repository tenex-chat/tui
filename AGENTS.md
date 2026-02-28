# TENEX TUI Client - Agent Guidelines

Multi-platform TENEX client with five apps sharing a common Rust core:

- **tenex-repl** — Interactive REPL client in `crates/tenex-repl/`
- **tenex-tui** — Terminal UI (Ratatui) in `crates/tenex-tui/`
- **tenex-cli** — CLI + daemon/HTTP server in `crates/tenex-cli/`
- **iOS app** — SwiftUI app for iPhone/iPad in `ios-app/` (destinations: `.iPhone`, `.iPad`)
- **Mac app** — SwiftUI app for macOS desktop in `ios-app/` (destination: `.mac`, same Tuist project)

The iOS and Mac apps share one codebase (`ios-app/`) with platform conditionals (`#if os(macOS)` / `#if os(iOS)`). Tuist's `Project.swift` defines `destinations: [.iPhone, .iPad, .mac]` with `deploymentTargets: .multiplatform(iOS: "26.0", macOS: "15.0")`.

## Commands

```bash
# Build & Test (Rust workspace)
cargo build --workspace
cargo test --workspace

# Run REPL
cargo run -p tenex-repl

# Run TUI
cargo run -p tenex-tui

# Run CLI daemon
cargo run -p tenex-cli -- daemon

# iOS simulator (build Rust, then generate Xcode project)
cargo build --target aarch64-apple-ios-sim --release -p tenex-core
cd ios-app && tuist generate

# Mac app (build Rust for host, then generate Xcode project)
cargo build --release -p tenex-core
cd ios-app && tuist generate
```

## Conventions

- **Rust:** rustfmt, explicit error types in library code
- **Swift:** SwiftUI + MVVM, async/await for FFI, platform conditionals for iOS vs macOS
- **Git:** Worktrees in `.worktrees/`, main branch is `master`

## Key References

- `AGENT_ARCHITECTURE.md` - Nostr agent/event system (read before Nostr work)
- `crates/*/AGENTS.md` - Crate-specific guidance
- `ios-app/AGENTS.md` - iOS/Mac app build and FFI details
- `swift-bindings/AGENTS.md` - UniFFI binding generation
