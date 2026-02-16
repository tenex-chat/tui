# tenex-core

Core Rust library for TUI and iOS clients. Provides Nostr protocol, SQLite storage, and UniFFI bindings.

## Commands

```bash
cargo build -p tenex-core                                            # Build
cargo test -p tenex-core                                             # Test
cargo build --target aarch64-apple-ios-sim --release -p tenex-core   # iOS Simulator
cargo build --target aarch64-apple-ios --release -p tenex-core       # iOS Device
./scripts/generate-swift-bindings.sh                                 # Regenerate FFI
```

## FFI Conventions

UniFFI exports for Swift/iOS - **no async across FFI boundary**:

- Use `runtime::block_on()` to bridge async → sync
- Callbacks for streaming events
- Types: primitives, structs, enums, Vec, HashMap, Option, Result, Arc
- NO: references, lifetimes, trait objects, generics

## Key Modules

- `ffi.rs` - UniFFI interface (TenexCore object)
- `runtime.rs` - Tokio bridge for FFI
- `nostr/` - Protocol implementation (nostr-sdk + nostrdb)
- `store/` - SQLite event storage

## Nostr Event Kinds

See `AGENT_ARCHITECTURE.md` for full schema:
- `0` - Metadata, `4199` - AgentDefinition, `24010` - ProjectStatus, `31933` - Project
