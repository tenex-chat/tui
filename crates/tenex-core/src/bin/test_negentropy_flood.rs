use anyhow::Result;
/// Test to see if negentropy sync causes event flooding
use nostr_sdk::prelude::*;
use nostrdb::{Config, Ndb};
use std::collections::HashSet;
use std::time::{Duration, Instant};

const RELAY_URL: &str = "wss://relay.tenex.tech";
const TEST_PUBKEY: &str = "09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7";
const NDB_PATH: &str = "/tmp/test_negentropy_flood_ndb";

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║    TEST: Does negentropy sync flood the notification stream?  ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    // Clean up any old database
    let _ = std::fs::remove_dir_all(NDB_PATH);

    // Create nostrdb instance
    let ndb = Ndb::new(NDB_PATH, &Config::new())?;
    let ndb_database = nostr_ndb::NdbDatabase::from(ndb.clone());
    println!("✓ Created nostrdb at {}\n", NDB_PATH);

    let keys = Keys::generate();
    let pubkey = PublicKey::parse(TEST_PUBKEY)?;

    // Create client WITH database (needed for negentropy)
    let client = Client::builder()
        .database(ndb_database)
        .signer(keys)
        .build();

    client.add_relay(RELAY_URL).await?;
    println!("Connecting to {}...", RELAY_URL);
    client.connect().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("✓ Connected\n");

    // Create subscriptions (same as main app)
    println!("Creating subscriptions:");

    let project_filter = Filter::new().kind(Kind::Custom(31933)).author(pubkey);
    client.subscribe(project_filter.clone(), None).await?;
    println!("  ✓ Projects (kind:31933)");

    let global_filter = Filter::new().kinds(vec![
        Kind::Custom(4199),
        Kind::Custom(4200),
        Kind::Custom(4201),
    ]);
    client.subscribe(global_filter.clone(), None).await?;
    println!("  ✓ Global definitions (kinds:4199,4200,4201)\n");

    // Start listening to notifications
    let mut notifications = client.notifications();

    let mut event_path_count = 0u64;
    let mut message_path_count = 0u64;
    let mut unique_ids = HashSet::new();

    println!("Phase 1: Listening WITHOUT negentropy for 10 seconds...\n");
    let phase1_start = Instant::now();
    let timeout = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => break,
            notification = notifications.recv() => {
                match notification {
                    Ok(RelayPoolNotification::Event { event, .. }) => {
                        event_path_count += 1;
                        unique_ids.insert(event.id.to_hex());
                    }
                    Ok(RelayPoolNotification::Message { message, .. }) => {
                        if let RelayMessage::Event { event, .. } = &message {
                            message_path_count += 1;
                            unique_ids.insert(event.id.to_hex());
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    let phase1_duration = phase1_start.elapsed();
    let phase1_event = event_path_count;
    let phase1_message = message_path_count;
    let phase1_unique = unique_ids.len();

    println!("Phase 1 results:");
    println!("  Event path:   {} events", phase1_event);
    println!("  Message path: {} events", phase1_message);
    println!("  Unique IDs:   {}\n", phase1_unique);

    // Reset counters
    event_path_count = 0;
    message_path_count = 0;
    unique_ids.clear();

    println!("Phase 2: Running negentropy sync...\n");
    let sync_start = Instant::now();

    // Run negentropy sync (same filters as main app)
    let sync_filters = vec![
        Filter::new().kind(Kind::Custom(31933)).author(pubkey),
        Filter::new().kind(Kind::Custom(4199)),
        Filter::new().kind(Kind::Custom(4200)),
        Filter::new().kind(Kind::Custom(4201)),
    ];

    for (i, filter) in sync_filters.iter().enumerate() {
        let opts = SyncOptions::default();
        match client.sync(filter.clone(), &opts).await {
            Ok(output) => {
                let count = output.val.received.len();
                println!("  Filter {} synced: {} new events", i + 1, count);
            }
            Err(e) => {
                let err_str = format!("{}", e);
                if !err_str.contains("not supported") {
                    println!("  Filter {} error: {}", i + 1, e);
                }
            }
        }
    }

    let sync_duration = sync_start.elapsed();
    println!(
        "\n✓ Negentropy sync completed in {:.2}s\n",
        sync_duration.as_secs_f64()
    );

    // Now listen for events that come through the notification stream
    println!("Phase 3: Listening for 30 seconds to capture sync events...\n");
    let phase3_start = Instant::now();
    let timeout = tokio::time::sleep(Duration::from_secs(30));
    tokio::pin!(timeout);

    let mut last_event_time = Instant::now();

    loop {
        tokio::select! {
            _ = &mut timeout => break,
            notification = notifications.recv() => {
                match notification {
                    Ok(RelayPoolNotification::Event { event, .. }) => {
                        event_path_count += 1;
                        unique_ids.insert(event.id.to_hex());
                        last_event_time = Instant::now();
                        if event_path_count <= 5 {
                            println!("  [Event] kind:{} id={}", event.kind.as_u16(), &event.id.to_hex()[..16]);
                        }
                    }
                    Ok(RelayPoolNotification::Message { message, .. }) => {
                        if let RelayMessage::Event { event, .. } = &message {
                            message_path_count += 1;
                            unique_ids.insert(event.id.to_hex());
                            last_event_time = Instant::now();
                            if message_path_count <= 5 {
                                println!("  [Message] kind:{} id={}", event.kind.as_u16(), &event.id.to_hex()[..16]);
                            }
                        }
                    }
                    Ok(_) => {}
                    Err(e) => {
                        println!("\n❌ Notification error: {:?}", e);
                        break;
                    }
                }
            }
        }
    }

    if event_path_count + message_path_count > 5 {
        println!(
            "  ... ({} more events)",
            (event_path_count + message_path_count) - 5
        );
    }

    let phase3_duration = phase3_start.elapsed();
    let phase3_unique = unique_ids.len();

    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║                      FINAL RESULTS                             ║");
    println!("╠════════════════════════════════════════════════════════════════╣");
    println!("║ Phase 1 (no sync):                                             ║");
    println!(
        "║   Event path:      {:>8}                                     ║",
        phase1_event
    );
    println!(
        "║   Message path:    {:>8}                                     ║",
        phase1_message
    );
    println!(
        "║   Unique IDs:      {:>8}                                     ║",
        phase1_unique
    );
    println!("║                                                                ║");
    println!("║ Phase 2 (negentropy sync):                                     ║");
    println!(
        "║   Duration:        {:>7.2}s                                  ║",
        sync_duration.as_secs_f64()
    );
    println!("║                                                                ║");
    println!("║ Phase 3 (after sync):                                          ║");
    println!(
        "║   Event path:      {:>8}                                     ║",
        event_path_count
    );
    println!(
        "║   Message path:    {:>8}                                     ║",
        message_path_count
    );
    println!(
        "║   Total:           {:>8}                                     ║",
        event_path_count + message_path_count
    );
    println!(
        "║   Unique IDs:      {:>8}                                     ║",
        phase3_unique
    );
    println!("║                                                                ║");

    if phase3_unique > 0 {
        let dup_factor = (event_path_count + message_path_count) as f64 / phase3_unique as f64;
        println!(
            "║   Duplication:     {:>7.2}x                                  ║",
            dup_factor
        );
        println!("║                                                                ║");
        println!("║ CONCLUSION:                                                    ║");
        if message_path_count > event_path_count * 10 {
            println!(
                "║   ⚠️  Events mostly via Message path ({} vs {})             ║",
                message_path_count, event_path_count
            );
            println!("║   This suggests negentropy events bypass Event path        ║");
        } else if event_path_count > 0 {
            println!("║   ✓ Events properly routed via Event path                  ║");
        }
    } else {
        println!("║   No events received - relay may not have data                ║");
    }

    println!("╚════════════════════════════════════════════════════════════════╝\n");

    // Cleanup
    let _ = std::fs::remove_dir_all(NDB_PATH);

    Ok(())
}
