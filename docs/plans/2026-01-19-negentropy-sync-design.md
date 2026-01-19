# Negentropy Sync for TUI

## Overview

Add NIP-77 negentropy set reconciliation to catch missed events on startup and periodically during runtime.

## When It Runs

- **On startup**: Immediately after connecting and setting up subscriptions
- **Periodically**: Adaptive interval starting at 60s, backing off to 15min when no gaps found

## Filters Synced

| Kind | Description |
|------|-------------|
| 31933 | User's projects |
| 4199 | Agent definitions |
| 513 | Conversation metadata |
| 4129 | Agent lessons |
| 4201 | Nudges |
| 1 | Messages with #a tags for user's projects |

**Not synced** (ephemeral/high-churn):
- 24010 (project status)
- 24133 (operations status)
- pubkey mentions filter

## Architecture

```
┌─────────────────────────────────────────────────────┐
│                   NostrWorker                        │
├─────────────────────────────────────────────────────┤
│  ┌─────────────────┐    ┌─────────────────────────┐ │
│  │ Notification    │    │ Negentropy Sync Task    │ │
│  │ Handler (live)  │    │ (periodic catch-up)     │ │
│  └────────┬────────┘    └───────────┬─────────────┘ │
│           │                         │               │
│           └─────────┬───────────────┘               │
│                     ▼                               │
│              ingest_events(ndb)                     │
└─────────────────────────────────────────────────────┘
```

- Runs as separate async task spawned after connection
- Shares Client (thread-safe, Clone)
- Uses existing `ingest_events()` for storage

## Adaptive Timing

```
Initial: 60s
No gaps found: interval = min(interval * 2, 900s)
Gaps found: interval = 60s
```

Progression when no gaps: 60 → 120 → 240 → 480 → 900 (cap)

## Logging

Uses existing `tlog!` macro with "SYNC" tag:

```
[    1234ms] [SYNC] Starting initial negentropy sync...
[    2456ms] [SYNC] kind:31933 → 3 new events
[    2890ms] [SYNC] kind:4199 → 0 new events (no log if 0)
[    3200ms] [SYNC] kind:1 → 47 new events
[    3201ms] [SYNC] Complete. Next sync in 60s
[   63500ms] [SYNC] No gaps found. Next sync in 120s
```

## Implementation

### New function: `run_negentropy_sync`

```rust
async fn run_negentropy_sync(client: Client, ndb: Arc<Ndb>, user_pubkey: PublicKey) {
    let mut interval_secs = 60;
    const MAX_INTERVAL: u64 = 900;

    loop {
        let total_new = sync_all_filters(&client, &ndb, &user_pubkey).await;

        if total_new == 0 {
            interval_secs = (interval_secs * 2).min(MAX_INTERVAL);
            tlog!("SYNC", "No gaps found. Next sync in {}s", interval_secs);
        } else {
            interval_secs = 60;
            tlog!("SYNC", "Found {} events. Next sync in {}s", total_new, interval_secs);
        }

        tokio::time::sleep(Duration::from_secs(interval_secs)).await;
    }
}
```

### New function: `sync_all_filters`

```rust
async fn sync_all_filters(
    client: &Client,
    ndb: &Ndb,
    user_pubkey: &PublicKey,
) -> usize {
    let opts = SyncOptions::default();
    let mut total_new = 0;

    // User's projects
    let project_filter = Filter::new()
        .kind(Kind::Custom(31933))
        .author(*user_pubkey);
    total_new += sync_filter(client, ndb, project_filter, "31933", &opts).await;

    // Agent definitions
    let agent_filter = Filter::new().kind(Kind::Custom(4199));
    total_new += sync_filter(client, ndb, agent_filter, "4199", &opts).await;

    // Conversation metadata
    let metadata_filter = Filter::new().kind(Kind::Custom(513));
    total_new += sync_filter(client, ndb, metadata_filter, "513", &opts).await;

    // Agent lessons
    let lesson_filter = Filter::new().kind(Kind::Custom(4129));
    total_new += sync_filter(client, ndb, lesson_filter, "4129", &opts).await;

    // Nudges
    let nudge_filter = Filter::new().kind(Kind::Custom(4201));
    total_new += sync_filter(client, ndb, nudge_filter, "4201", &opts).await;

    // Messages - needs project a_tags from ndb
    if let Ok(projects) = crate::store::get_projects(ndb) {
        let atags: Vec<String> = projects.iter().map(|p| p.a_tag()).collect();
        if !atags.is_empty() {
            let msg_filter = Filter::new()
                .kind(Kind::from(1))
                .custom_tag(SingleLetterTag::lowercase(Alphabet::A), atags);
            total_new += sync_filter(client, ndb, msg_filter, "1", &opts).await;
        }
    }

    total_new
}
```

### New function: `sync_filter`

```rust
async fn sync_filter(
    client: &Client,
    ndb: &Ndb,
    filter: Filter,
    label: &str,
    opts: &SyncOptions,
) -> usize {
    match client.sync(filter, opts).await {
        Ok(output) => {
            let mut count = 0;
            for (relay_url, reconciliation) in output.success.iter() {
                let events: Vec<Event> = reconciliation
                    .received
                    .iter()
                    .cloned()
                    .collect();

                if !events.is_empty() {
                    if let Err(e) = ingest_events(ndb, &events, Some(relay_url.as_str())) {
                        tlog!("SYNC", "Failed to ingest {}: {}", label, e);
                    }
                    count += events.len();
                }
            }
            if count > 0 {
                tlog!("SYNC", "kind:{} → {} new events", label, count);
            }
            count
        }
        Err(e) => {
            tlog!("SYNC", "kind:{} failed: {}", label, e);
            0
        }
    }
}
```

### Integration in `handle_connect`

After `start_subscriptions()`:

```rust
// Spawn negentropy sync task
let client = self.client.as_ref().unwrap().clone();
let ndb = self.ndb.clone();
let pubkey = PublicKey::parse(&user_pubkey)?;

self.rt_handle.as_ref().unwrap().spawn(async move {
    tlog!("SYNC", "Starting initial negentropy sync...");
    run_negentropy_sync(client, ndb, pubkey).await;
});
```

## Files Modified

- `crates/tenex-core/src/nostr/worker.rs` - Add sync functions and spawn task
