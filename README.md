# TENEX TUI Client (Rust)

A terminal user interface client for TENEX built with Rust.

## Features

- **SQLite-backed event store** - All Nostr events are stored locally in SQLite for offline access and fast queries
- **Nostr protocol integration** - Full Nostr protocol support via nostr-sdk with relay management
- **Encrypted nsec storage** - Secure storage of private keys using NIP-49 encryption
- **Multi-view navigation** - Browse projects, threads, and chat messages with intuitive navigation
- **OpenTelemetry tracing** - Built-in observability for debugging and monitoring

## Usage

Start the application:

```bash
cargo run
```

On first run, you'll be prompted to enter your nsec (Nostr private key). The application will securely store your credentials for future sessions.

## Controls

- `i` - Enter editing/input mode
- `Esc` - Cancel editing / go back to previous view
- `↑/↓` - Navigate through lists (projects, threads)
- `Enter` - Select item / submit input
- `q` - Quit the application

## Navigation Flow

1. **Login** - Enter your nsec and optional password for encryption
2. **Projects** - View all your projects, select one to see its threads
3. **Threads** - View threads in the selected project, select one to chat
4. **Chat** - Read and send messages in the selected thread

## Build

Build the release version:

```bash
cargo build --release
```

The compiled binary will be available at `target/release/tenex-tui`.

## Development

Run tests:

```bash
cargo test
```

Run with trace output:

```bash
RUST_LOG=info cargo run
```

## Database

The application stores data in `tenex.db` in the current directory. This includes:
- Nostr events (projects, threads, messages)
- User profiles
- Encrypted credentials

To start fresh, simply delete the `tenex.db` file.
