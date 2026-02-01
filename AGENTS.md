# TENEX TUI Client - Agent Guidelines

## Project Overview

A multi-platform TENEX client with:
- **Rust TUI** - Terminal interface using Ratatui
- **iOS App** - Native SwiftUI app with Rust core via FFI
- **Shared Core** - Rust library (tenex-core) with Nostr protocol, SQLite storage, and UniFFI bindings

**Current Status:** Active development on both TUI modernization and iOS app
**Last TUI Update:** December 20, 2025
**Backend Tracking:** 143 commits behind TENEX backend since Dec 20

## Core Architecture

```
┌─────────────────────────────────────────┐
│         tenex-core (Rust)               │
│  - Nostr protocol (nostr-sdk, nostrdb)  │
│  - SQLite event store                   │
│  - UniFFI bindings → Swift              │
└─────────────────┬───────────────────────┘
                  │
         ┌────────┴────────┐
         │                 │
    ┌────▼─────┐      ┌───▼──────┐
    │ tenex-tui│      │ iOS App  │
    │ (Ratatui)│      │ (SwiftUI)│
    └──────────┘      └──────────┘
```

## Key Principles

1. **No Temporary Solutions** - No "for now", no hacks, no placeholder code
2. **TDD Development** - Verify before marking milestones complete
3. **TUI ≠ Web Port** - Adapt features for TUI strengths, don't blindly port
4. **Tracing First** - Use OpenTelemetry traces at http://localhost:16686/ for debugging
5. **Compare with Svelte** - Reference web client behavior when unsure

## Commands

### Rust Development
```bash
# Build workspace
cargo build --workspace

# Run TUI
cargo run -p tenex-tui

# Run with traces
RUST_LOG=info cargo run -p tenex-tui

# Test workspace
cargo test --workspace

# Build iOS static library (simulator)
cargo build --target aarch64-apple-ios-sim --release -p tenex-core

# Build iOS static library (device)
cargo build --target aarch64-apple-ios --release -p tenex-core
```

### iOS Development
```bash
# Generate Xcode project (from ios-app/)
cd ios-app && tuist generate

# Build and run (after opening in Xcode)
# Uses simulator: target/aarch64-apple-ios-sim/release/libtenex_core.a
# Uses device: target/aarch64-apple-ios/release/libtenex_core.a
```

### UniFFI Bindings
```bash
# Regenerate Swift bindings (requires cargo-run-bin)
cargo bin uniffi-bindgen generate \
  --library target/aarch64-apple-ios-sim/release/libtenex_core.a \
  --language swift \
  --out-dir swift-bindings \
  crates/tenex-core/src/tenex_core.udl
```

## Project Structure

```
├── crates/
│   ├── tenex-core/    # Core library with UniFFI, Nostr, SQLite
│   ├── tenex-tui/     # Terminal UI with Ratatui
│   └── tenex-cli/     # CLI tool
├── ios-app/           # iOS SwiftUI app (Tuist project)
│   └── Sources/TenexMVP/
│       ├── TenexCore/      # Swift wrappers around Rust
│       ├── TenexCoreFFI/   # FFI headers/modulemap
│       ├── Views/          # SwiftUI views
│       └── ViewModels/     # MVVM architecture
├── swift-bindings/    # UniFFI-generated Swift bindings
├── scripts/           # Build and utility scripts
└── docs/              # Documentation
```

## Critical Files

- **AGENT_ARCHITECTURE.md** - Nostr agent/event architecture (MUST READ for Nostr work)
- **TUI_MODERNIZATION_TRACKER.md** - Current modernization status and gaps
- **PROFILING_ANALYSIS.md** - Performance analysis results
- **Cargo.toml** - Workspace configuration with shared dependencies

## Conventions

### Code Style
- **Rust:** Standard rustfmt, prefer explicit error types over anyhow in library code
- **Swift:** SwiftUI best practices, MVVM architecture, async/await for Rust FFI calls
- **Naming:** snake_case (Rust), camelCase (Swift), PascalCase (types/structs)

### Git Workflow
- Use worktrees for feature branches in `.worktrees/`
- Main branch: `master`
- Feature branches: `feature/ios-activity-tracking`, `feature/ios-project-discovery`

### FFI Boundaries
- All Rust → Swift calls go through UniFFI-generated bindings
- Swift calls Rust async functions using Task/await
- Error handling: Rust Result → Swift throws

## Common Pitfalls

1. **Agent vs AgentDefinition** - Read AGENT_ARCHITECTURE.md; agents are users, definitions are templates
2. **FFI Path Issues** - Always rebuild Rust before testing iOS changes
3. **Simulator vs Device** - Different library paths and build targets
4. **Nostr Event Kinds** - Use correct kinds (24010=ProjectStatus, 4199=AgentDefinition, etc)
5. **Async Boundaries** - Rust tokio runtime separate from Swift's async/await

## Testing Strategy

- **Unit Tests:** Per-crate in `tests/` modules
- **Integration:** Full stack tests using test database
- **iOS Testing:** Manual QA via ios-tester agent, workflow documentation
- **TUI Testing:** Verify against OTL traces and manual testing

## Related Documentation

- [AGENT_ARCHITECTURE.md](./AGENT_ARCHITECTURE.md) - Nostr agent system
- [TUI_MODERNIZATION_TRACKER.md](./TUI_MODERNIZATION_TRACKER.md) - Current work status
- [README.md](./README.md) - User-facing documentation
- See crate-specific AGENTS.md in `crates/` for detailed module docs
