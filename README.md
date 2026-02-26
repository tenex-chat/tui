# TENEX Client

A multi-platform client for [TENEX](https://tenex.chat) — four apps built from one repository, sharing a common Rust core.

| App | Platform | Tech |
|-----|----------|------|
| **TUI** | Terminal | Ratatui |
| **CLI Daemon** | Terminal | Daemon + Unix socket + HTTP |
| **iOS App** | iPhone / iPad | SwiftUI |
| **Mac App** | macOS | SwiftUI |

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                    tenex-tui (binary)                            │
│           Ratatui TUI + HTTP server mode (--server)              │
└────────────────────────┬────────────────────────────────────────┘
                         │ depends on
┌────────────────────────▼────────────────────────────────────────┐
│                  tenex-core (library)                            │
│  ┌─────────────┐  ┌──────────┐  ┌────────────┐  ┌──────────┐  │
│  │  nostr/     │  │ store/   │  │  ffi/      │  │  ai/     │  │
│  │  (nostr-sdk)│  │ (nostrdb)│  │  (UniFFI)  │  │ (EL/OR)  │  │
│  └─────────────┘  └──────────┘  └────────────┘  └──────────┘  │
└──────────┬──────────────────────────────┬───────────────────────┘
           │ depends on                   │ FFI (libtenex_core.a)
┌──────────▼──────────┐        ┌──────────▼────────────────────┐
│  tenex-cli (binary)  │        │  ios-app/ (Swift/SwiftUI)     │
│  Daemon + HTTP API   │        │  iPhone + iPad + Mac          │
│  Unix socket IPC     │        │  UniFFI bindings              │
└──────────────────────┘        └───────────────────────────────┘
```

**Key design patterns:**

- **NostrWorker** — background Rust thread managing relay WebSockets and nostrdb ingestion
- **AppDataStore** — single source of truth, fed by `DataChange` events from NostrWorker
- **CoreRuntime** — owns the nostrdb `Ndb` instance and `AppDataStore`; provides `CoreHandle` for commands
- **FFI layer** — UniFFI wraps async Rust in synchronous calls for Swift via `runtime::block_on()`
- **CLI daemon** — runs the same `CoreRuntime`, exposed via Unix socket and optional HTTP

## Features

### Core (`tenex-core`)

- **nostrdb (LMDB-backed)** storage for all Nostr events — no SQLite
- Nostr protocol via nostr-sdk 0.44.1
- Negentropy sync for efficient relay synchronization
- NIP-46 Bunker signer with approval rules and audit log
- NIP-49 key encryption (`ncryptsec`)
- UniFFI FFI bindings for Swift
- ElevenLabs TTS client
- Blossom image upload
- OS Keychain secure storage (`keyring` crate)
- OpenAI Responses API HTTP server (axum)

### TUI (`tenex-tui`)

- Multi-view navigation: Login → Home (5 tabs) → Chat
- Multi-tab chat (up to 9 simultaneous conversations)
- Ask event support (single-select and multi-select)
- Agent browser (list / view / create / fork / clone)
- Lessons viewer (kind 4129) and Reports viewer (NIP-23)
- Nudge/skill selector, command palette (`Ctrl+T`), workspace manager (`Ctrl+P`)
- History search (`Ctrl+R`), in-conversation search (`Ctrl+F`)
- Draft system (auto-saved per conversation)
- Audio playback, image display, Blossom image upload
- Inbox (filtered agent mentions)
- Debug stats view and bunker management
- Optional `--server` flag to run an HTTP/SSE server (OpenAI Responses API)

### CLI Daemon (`tenex-cli`)

- Project management: `save-project`, `boot-project`, `show-project`
- Thread/message management: `create-thread`, `send-message`, `list-threads`, `list-messages`
- Agent management: `list-agents`, `set-agent-settings`
- Skills/nudges: `list-skills`, `list-nudges`
- NIP-46 bunker: `start` / `stop` / `status` / `watch` / `enable` / `disable` / `rules` / `audit`
- HTTP server: OpenAI Responses API (`--http`)
- Daemon control: `status`, `shutdown`

### iOS / Mac App (`ios-app/`)

Single SwiftUI codebase with platform conditionals (`#if os(macOS)` / `#if os(iOS)`), managed by Tuist.

| Target | Min OS | Bundle ID |
|--------|--------|-----------|
| iPhone | iOS 26.0 | `com.tenex.mvp` |
| iPad | iPadOS 26.0 | `com.tenex.mvp` |
| Mac | macOS 15.0 | `com.tenex.mvp` |

- Adaptive layout: compact tab bar (iPhone), sidebar (iPad iOS 26+), shell layout (Mac)
- Sections: Chats, Projects, Reports, Inbox, Search, Teams, Agent Definitions, Nudges, Settings, Diagnostics
- Full message thread viewer with tool call rendering, delegation cards, streaming
- Interactive ask event answering
- Image attachments with collapsible viewer
- Voice dictation (ElevenLabs STT)
- NIP-46 bunker approval
- Audio notifications with NowPlayingBar
- Diagnostics: database, subscriptions, sync, bunker tabs
- Rust core linked as static library (`libtenex_core.a`) via UniFFI bindings
- Uses Kingfisher for image loading

## Prerequisites

- **Rust** stable toolchain
- **Tuist** (for iOS/Mac Xcode project generation)
- **Xcode** with iOS 26.0 / macOS 15.0 SDKs (for native apps)
- iOS cross-compilation targets: `rustup target add aarch64-apple-ios-sim aarch64-apple-ios`

## Build & Run

### Rust workspace

```bash
# Build everything
cargo build --workspace

# Run tests
cargo test --workspace

# Run the TUI
cargo run -p tenex-tui

# Run the TUI with an nsec (skips interactive login)
TENEX_NSEC=nsec1... cargo run -p tenex-tui

# Run the TUI in HTTP server mode
TENEX_NSEC=nsec1... cargo run -p tenex-tui -- --server
TENEX_NSEC=nsec1... cargo run -p tenex-tui -- --server --bind 0.0.0.0:8080

# Run the CLI daemon
cargo run -p tenex-cli -- --daemon

# Run the CLI daemon with HTTP server
cargo run -p tenex-cli -- --daemon --http --http-bind 127.0.0.1:8080
```

### iOS Simulator

```bash
cargo build --target aarch64-apple-ios-sim --release -p tenex-core
./scripts/generate-swift-bindings.sh
cd ios-app && tuist generate
# Then build/run from Xcode
```

### Mac App

```bash
cargo build --release -p tenex-core
./scripts/generate-swift-bindings.sh
cd ios-app && tuist generate
# Then build/run from Xcode
```

### Environment Variables

| Variable | Purpose |
|----------|---------|
| `TENEX_NSEC` | Nostr secret key (`nsec1...` format). Skips interactive login. |
| `TENEX_BASE_DIR` | Override default data directory (default: `~/.tenex`) |
| `TENEX_DEBUG=1` | Enable debug logging |

## Data Storage

| Storage | What | Location |
|---------|------|----------|
| nostrdb (LMDB) | All Nostr events | `~/.tenex/cli/` |
| PreferencesStorage (JSON) | Credentials (nsec/ncryptsec), relay prefs | `~/.tenex/cli/` |
| OS Keychain (`keyring`) | ElevenLabs API key, OpenRouter API key | System keychain |
| File-based drafts | Unsent message drafts | `~/.tenex/cli/drafts/` |

## TUI Controls

### Global

| Key | Action |
|-----|--------|
| `q` | Quit |
| `Ctrl+T` | Command palette |
| `Ctrl+P` | Workspace manager |
| `1` | Go to Home |
| `?` | Help / hotkey reference |
| `Alt+M` | Jump to notification |

### Home — Conversations Tab

| Key | Action |
|-----|--------|
| `Tab` / `Shift+Tab` | Switch tabs |
| `n` | New conversation |
| `Shift+N` | New conversation (same project) |
| `p` | Switch project |
| `a` | Archive toggle |
| `f` | Time filter |
| `e` | Export as JSONL |
| `Shift+B` | Agent browser |
| `Shift+C` | Create project |

### Chat — Normal Mode

| Key | Action |
|-----|--------|
| `i` | Enter edit/compose mode |
| `@` | Mention agent |
| `y` | Copy message |
| `v` | View raw event |
| `t` | Open trace |
| `.` | Stop agent |
| `g` | Go to parent conversation |
| `x` | Close current tab |
| `Ctrl+F` | In-conversation search |

### Chat — Edit Mode

| Key | Action |
|-----|--------|
| `Ctrl+Enter` | Send message |
| `Shift+Enter` | Insert newline |
| `Ctrl+E` | Expand editor (full-screen) |
| `Ctrl+/` / `Ctrl+N` / `Alt+K` | Nudge/skill selector |
| `Ctrl+R` | History search |
| `Esc` | Cancel edit |

## Nostr Event Kinds

| Kind | Name | Purpose |
|------|------|---------|
| 0 | Metadata | Agent/user profiles |
| 1 | Text Note | Conversations: threads and messages |
| 513 | ConversationMetadata | Thread title, status labels |
| 4129 | AgentLesson | Agent knowledge/lessons |
| 4199 | AgentDefinition | Agent configuration templates |
| 4200 | MCPTool | MCP tool definitions |
| 4201 | Nudge | Tool permission nudges |
| 4202 | Skill | Agent skill definitions |
| 24000 | Boot | Project boot command |
| 24010 | ProjectStatus | Online agents, model/tool assignments |
| 24020 | AgentSettings | Override agent settings |
| 30023 | Article (NIP-23) | Reports/articles (addressable) |
| 31933 | Project | Project definition (addressable) |

## Documentation

- [`AGENT_ARCHITECTURE.md`](./AGENT_ARCHITECTURE.md) — Agents vs AgentDefinitions, Nostr event kinds and protocol details
- [`docs/OPENAI_API_SERVER.md`](./docs/OPENAI_API_SERVER.md) — HTTP/SSE Responses API reference
- [`docs/UI_STYLE_GUIDELINES.md`](./docs/UI_STYLE_GUIDELINES.md) — TUI visual style conventions
- [`swift-bindings/`](./swift-bindings/) — Auto-generated UniFFI Swift bindings
- [`scripts/generate-swift-bindings.sh`](./scripts/generate-swift-bindings.sh) — Generates Swift bindings from Rust FFI

## Project Structure

```
├── crates/
│   ├── tenex-core/        # Core library (nostr, store, FFI, AI)
│   ├── tenex-tui/         # Terminal UI binary
│   └── tenex-cli/         # CLI daemon binary
├── ios-app/               # SwiftUI app (iPhone, iPad, Mac)
├── swift-bindings/        # UniFFI-generated Swift bindings
├── scripts/               # Build helper scripts
└── docs/                  # Additional documentation
```

## License

See [LICENSE](./LICENSE) for details.
