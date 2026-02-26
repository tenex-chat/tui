# TENEX

**Manage your AI agent teams — from your terminal, phone, or desktop.**

TENEX is a multi-platform client for orchestrating AI agents over [Nostr](https://nostr.com). Spin up projects, talk to agents, review their work, and keep everything in sync across all your devices.

## Apps

### 🖥 Terminal UI

A full-featured terminal interface for power users. Navigate projects, chat with agents across multiple tabs, browse agent definitions, review reports and lessons — all from your terminal.

```bash
cargo run -p tenex-tui
```

### ⌨️ CLI Daemon

A headless daemon that keeps your agent projects online in the background. Manage projects, threads, and agents from the command line. Includes an HTTP server compatible with the OpenAI Responses API.

```bash
cargo run -p tenex-cli -- --daemon
```

### 📱 iOS & 🖥 Mac App

A native SwiftUI app for iPhone, iPad, and Mac. Adaptive layout that feels right on every screen — tab bar on iPhone, sidebar on iPad, full desktop experience on Mac. Get push-style notifications when agents need your attention.

## Getting Started

### Terminal (TUI or CLI)

**Requirements:** Rust stable toolchain

```bash
# Clone and build
git clone https://github.com/pablof7z/tenex-client.git
cd tenex-client
cargo build --workspace

# Run the TUI
cargo run -p tenex-tui

# Or start the CLI daemon
cargo run -p tenex-cli -- --daemon
```

Set `TENEX_NSEC=nsec1...` to skip interactive login.

### iOS / Mac

**Requirements:** Rust stable toolchain, [Tuist](https://tuist.io), Xcode with iOS 26+ / macOS 15+ SDKs

```bash
# iOS Simulator
rustup target add aarch64-apple-ios-sim
cargo build --target aarch64-apple-ios-sim --release -p tenex-core
./scripts/generate-swift-bindings.sh
cd ios-app && tuist generate
# Open in Xcode → Build & Run

# Mac
cargo build --release -p tenex-core
./scripts/generate-swift-bindings.sh
cd ios-app && tuist generate
# Open in Xcode → Build & Run
```

## TUI Quick Reference

| Key | Action |
|-----|--------|
| `q` | Quit |
| `i` | Compose a message |
| `Ctrl+Enter` | Send |
| `Ctrl+T` | Command palette |
| `Ctrl+P` | Workspace manager |
| `Tab` | Switch tabs |
| `n` | New conversation |
| `?` | Full hotkey reference |

## Configuration

| Variable | Purpose |
|----------|---------|
| `TENEX_NSEC` | Your Nostr secret key (`nsec1...`). Skips interactive login. |
| `TENEX_BASE_DIR` | Override data directory (default: `~/.tenex`) |
| `TENEX_DEBUG=1` | Enable debug logging |

## For Contributors

The repo is a Cargo workspace with three crates and a SwiftUI app:

```
crates/tenex-core/   — Shared library: Nostr, storage, FFI, AI integrations
crates/tenex-tui/    — Terminal UI (Ratatui)
crates/tenex-cli/    — CLI daemon + HTTP server
ios-app/             — SwiftUI app (iPhone, iPad, Mac via Tuist)
```

Key docs for contributors:

- [`AGENT_ARCHITECTURE.md`](./AGENT_ARCHITECTURE.md) — Nostr event kinds, agent protocol details
- [`docs/OPENAI_API_SERVER.md`](./docs/OPENAI_API_SERVER.md) — HTTP/SSE API reference
- [`docs/UI_STYLE_GUIDELINES.md`](./docs/UI_STYLE_GUIDELINES.md) — TUI visual conventions

```bash
# Build & test everything
cargo build --workspace
cargo test --workspace
```

## License

See [LICENSE](./LICENSE) for details.
