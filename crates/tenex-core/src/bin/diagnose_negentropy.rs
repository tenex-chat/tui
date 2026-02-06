/// Comprehensive diagnostic for negentropy database issue
///
/// This program tests:
/// 1. Are events saved to nostrdb (via subscriptions)?
/// 2. Does NdbDatabase see those events?
/// 3. Does negentropy re-download events it should already have?
use nostr_sdk::prelude::*;
use nostrdb::{Config, Filter as NdbFilter, Ndb, Transaction};
use std::sync::Arc;
use std::time::{Duration, Instant};
use anyhow::Result;

const RELAY_URL: &str = "wss://relay.tenex.tech";
const TEST_PUBKEY: &str = "09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7";
const NDB_PATH: &str = "/tmp/diagnose_negentropy_ndb";

fn count_events_in_ndb(ndb: &Ndb, kind: u16) -> usize {
    let txn = match Transaction::new(ndb) {
        Ok(t) => t,
        Err(_) => return 0,
    };

    let filter = NdbFilter::new().kinds(vec![kind as u64]).build();
    let results = ndb.query(&txn, &[filter], 10000);

    match results {
        Ok(results) => results.len(),
        Err(_) => 0,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║        NEGENTROPY DATABASE DIAGNOSTIC                         ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    // Clean up
    let _ = std::fs::remove_dir_all(NDB_PATH);

    // Create nostrdb
    let ndb = Arc::new(Ndb::new(NDB_PATH, &Config::new())?);
    let ndb_database = nostr_ndb::NdbDatabase::from((*ndb).clone());
    println!("✓ Created nostrdb at {}\n", NDB_PATH);

    let keys = Keys::generate();
    let pubkey = PublicKey::parse(TEST_PUBKEY)?;

    // Create client WITH NdbDatabase
    let client = Client::builder()
        .database(ndb_database)
        .signer(keys)
        .build();

    client.add_relay(RELAY_URL).await?;
    println!("Connecting to {}...", RELAY_URL);
    client.connect().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("✓ Connected\n");

    // Test filter: kind:31933 (projects) authored by user
    let filter = Filter::new()
        .kind(Kind::Custom(31933))
        .author(pubkey);

    println!("═══════════════════════════════════════════════════════════════");
    println!("PHASE 1: Regular subscription (should download and save)");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Check database BEFORE subscription
    let before_count = count_events_in_ndb(&ndb, 31933);
    println!("Events in nostrdb BEFORE subscription: {}", before_count);

    // Create subscription
    client.subscribe(filter.clone(), None).await?;
    println!("✓ Subscribed to kind:31933\n");

    // Listen for events
    println!("Listening for 10 seconds...");
    let mut notifications = client.notifications();
    let mut event_path_count = 0;
    let mut message_path_count = 0;

    let timeout = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => break,
            notification = notifications.recv() => {
                match notification {
                    Ok(RelayPoolNotification::Event { .. }) => {
                        event_path_count += 1;
                    }
                    Ok(RelayPoolNotification::Message { message, .. }) => {
                        if let RelayMessage::Event { .. } = &message {
                            message_path_count += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    println!("  Received via Event path: {}", event_path_count);
    println!("  Received via Message path: {}", message_path_count);

    // Check database AFTER subscription
    tokio::time::sleep(Duration::from_millis(500)).await; // Let events settle
    let after_count = count_events_in_ndb(&ndb, 31933);
    println!("\nEvents in nostrdb AFTER subscription: {}", after_count);

    if after_count > before_count {
        println!("✅ nostrdb received {} new events from subscription", after_count - before_count);
    } else {
        println!("⚠️  nostrdb didn't gain any events (might be no new data)");
    }

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("PHASE 2: Negentropy sync (should see existing events)");
    println!("═══════════════════════════════════════════════════════════════\n");

    let db_count_before_sync = count_events_in_ndb(&ndb, 31933);
    println!("Events in nostrdb BEFORE negentropy: {}", db_count_before_sync);

    println!("\nRunning negentropy sync...");
    let sync_start = Instant::now();

    let opts = SyncOptions::default();
    match client.sync(filter.clone(), &opts).await {
        Ok(output) => {
            let downloaded = output.val.received.len();
            let sync_duration = sync_start.elapsed();

            println!("  Downloaded: {} events", downloaded);
            println!("  Time: {:.2}s\n", sync_duration.as_secs_f64());

            if db_count_before_sync > 0 && downloaded > 0 {
                println!("❌ PROBLEM DETECTED:");
                println!("   Database had {} events", db_count_before_sync);
                println!("   But negentropy downloaded {} events", downloaded);
                println!("   This suggests NdbDatabase isn't telling nostr-sdk about local events!\n");
            } else if db_count_before_sync > 0 && downloaded == 0 {
                println!("✅ GOOD: Negentropy saw the {} events in the database", db_count_before_sync);
                println!("   No re-download happened.\n");
            } else if db_count_before_sync == 0 {
                println!("ℹ️  Database was empty, so downloading {} is expected.\n", downloaded);
            }
        }
        Err(e) => {
            println!("  Sync failed: {}\n", e);
        }
    }

    // Listen for sync events
    println!("Listening for 10 more seconds to capture sync events...");
    let mut sync_event_count = 0;
    let mut sync_message_count = 0;

    let timeout = tokio::time::sleep(Duration::from_secs(10));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => break,
            notification = notifications.recv() => {
                match notification {
                    Ok(RelayPoolNotification::Event { .. }) => {
                        sync_event_count += 1;
                    }
                    Ok(RelayPoolNotification::Message { message, .. }) => {
                        if let RelayMessage::Event { .. } = &message {
                            sync_message_count += 1;
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    println!("  Sync events via Event path: {}", sync_event_count);
    println!("  Sync events via Message path: {}", sync_message_count);

    if sync_message_count > sync_event_count * 10 {
        println!("\n⚠️  Negentropy events mostly come via Message path!");
        println!("   This is why they're hitting the EVT_VIA_MSG logging.\n");
    }

    println!("\n═══════════════════════════════════════════════════════════════");
    println!("PHASE 3: Second negentropy sync (should download 0)");
    println!("═══════════════════════════════════════════════════════════════\n");

    let db_count = count_events_in_ndb(&ndb, 31933);
    println!("Events in nostrdb: {}", db_count);

    println!("\nRunning second negentropy sync...");
    let sync_start = Instant::now();

    match client.sync(filter.clone(), &opts).await {
        Ok(output) => {
            let downloaded = output.val.received.len();
            let sync_duration = sync_start.elapsed();

            println!("  Downloaded: {} events", downloaded);
            println!("  Time: {:.2}s\n", sync_duration.as_secs_f64());

            if downloaded > 0 {
                println!("❌ CRITICAL ISSUE:");
                println!("   Second sync downloaded {} events", downloaded);
                println!("   This proves negentropy is NOT respecting the database state!\n");
                println!("   Root cause: NdbDatabase integration is broken.\n");
            } else {
                println!("✅ SUCCESS: Second sync downloaded 0 events");
                println!("   Negentropy correctly saw events were already local.\n");
            }
        }
        Err(e) => {
            println!("  Sync failed: {}\n", e);
        }
    }

    println!("═══════════════════════════════════════════════════════════════");
    println!("DIAGNOSIS COMPLETE");
    println!("═══════════════════════════════════════════════════════════════\n");

    // Cleanup
    let _ = std::fs::remove_dir_all(NDB_PATH);

    Ok(())
}
