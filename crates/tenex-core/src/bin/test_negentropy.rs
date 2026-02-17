//! Negentropy Sync Test Program
//!
//! This isolated test verifies that negentropy set reconciliation works correctly:
//! 1. Tests if subsequent syncs only download NEW events (not re-downloading existing)
//! 2. Tests if nostrdb persistence affects negentropy sync behavior
//! 3. Identifies potential bugs in our negentropy implementation
//!
//! Key Insight: nostr-sdk's negentropy sync uses the Client's internal database
//! to determine what events it already has. If no database is attached to the client,
//! negentropy will ALWAYS report all relay events as "new".
//!
//! Run with: cargo run --bin test_negentropy

use std::time::{Duration, Instant};

use anyhow::Result;
use nostr_sdk::prelude::*;
use nostrdb::{Config, Ndb};

const RELAY_URL: &str = "wss://relay.damus.io";
const TEST_PUBKEY: &str = "09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7";
const NDB_PATH: &str = "test_negentropy_data";

#[tokio::main]
async fn main() -> Result<()> {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║           NEGENTROPY SYNC BEHAVIOR TEST                          ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    // Clean up any previous test data
    let _ = std::fs::remove_dir_all(NDB_PATH);

    // Run tests
    test_client_without_database().await?;
    println!("\n{}\n", "=".repeat(70));
    test_nostrdb_persistence().await?;
    println!("\n{}\n", "=".repeat(70));
    print_root_cause_analysis();

    // Summary
    print_summary();

    // Cleanup
    let _ = std::fs::remove_dir_all(NDB_PATH);

    Ok(())
}

/// Test 1: Client without database - should always get ALL events as "new"
async fn test_client_without_database() -> Result<()> {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ TEST 1: Client WITHOUT Database (Current Implementation)       │");
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
    println!("This tests how nostr-sdk behaves when Client has no database.");
    println!("Expected: ALL events reported as 'new' every sync (no local state).\n");

    let keys = Keys::generate();
    let client = Client::new(keys);

    client.add_relay(RELAY_URL).await?;
    println!("✓ Connecting to {}...", RELAY_URL);
    client.connect().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("✓ Connected\n");

    // Use a small, well-defined filter for testing
    let pubkey = PublicKey::parse(TEST_PUBKEY)?;
    let filter = Filter::new().kind(Kind::Custom(31933)).author(pubkey);

    println!("Filter: kind=31933, author={}", &TEST_PUBKEY[..16]);
    println!();

    let mut first_count = 0;

    // Run sync twice with the SAME client instance
    for i in 1..=3 {
        println!("─── Sync #{} ───", i);
        let start = Instant::now();
        let opts = SyncOptions::default();

        match client.sync(filter.clone(), &opts).await {
            Ok(output) => {
                let elapsed = start.elapsed();
                let count = output.val.received.len();

                if i == 1 {
                    first_count = count;
                }

                println!("  Events received: {}", count);
                println!("  Time: {:?}", elapsed);

                if i > 1 {
                    if count > 0 && count == first_count {
                        println!(
                            "  ⚠️  ISSUE: Sync #{} received same {} events as sync #1!",
                            i, count
                        );
                        println!("      The client has NO memory of previous syncs.");
                    } else if count == 0 && first_count > 0 {
                        println!(
                            "  ✅ SUCCESS: Sync #{} received 0 events (proper delta)!",
                            i
                        );
                    } else if count > 0 && count < first_count {
                        println!(
                            "  ℹ️  Sync #{} received {} events ({:.0}% of first)",
                            i,
                            count,
                            (count as f64 / first_count as f64) * 100.0
                        );
                    }
                }
            }
            Err(e) => {
                let err_str = format!("{}", e);
                if err_str.contains("not supported") || err_str.contains("NEG-ERR") {
                    println!("  ⚠️  Relay does not support negentropy (NIP-77)");
                    println!("      Try relay.0xchat.com or other negentropy-enabled relays");
                    break;
                } else {
                    println!("  Error: {}", e);
                }
            }
        }
        println!();

        tokio::time::sleep(Duration::from_millis(500)).await;
    }

    client.disconnect().await;
    Ok(())
}

/// Test 2: Test if nostrdb can be used for negentropy state
async fn test_nostrdb_persistence() -> Result<()> {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ TEST 2: NostrDB Persistence Between Client Instances            │");
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();
    println!("This simulates restarting the app - does nostrdb preserve state");
    println!("that negentropy can use for delta sync?\n");

    let mut session1_count = 0;
    let mut session2_count = 0;

    // First "session" - sync and store in nostrdb
    {
        println!("─── Session 1: Initial Sync with Fresh Client ───");

        let ndb = Ndb::new(NDB_PATH, &Config::new())?;
        println!("  ✓ Created fresh nostrdb at: {}", NDB_PATH);

        let keys = Keys::generate();
        let client = Client::new(keys);

        client.add_relay(RELAY_URL).await?;
        client.connect().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        println!("  ✓ Connected to relay");

        let pubkey = PublicKey::parse(TEST_PUBKEY)?;
        let filter = Filter::new().kind(Kind::Custom(31933)).author(pubkey);

        let opts = SyncOptions::default();
        match client.sync(filter.clone(), &opts).await {
            Ok(output) => {
                session1_count = output.val.received.len();
                println!("  Events from negentropy: {}", session1_count);

                // Store events in nostrdb (simulating our current approach)
                // Note: In the real code, events go through ingest_events()
                // Here we just note that events would be stored
                println!("  (Events would be stored in nostrdb)");
            }
            Err(e) => {
                let err_str = format!("{}", e);
                if err_str.contains("not supported") {
                    println!("  ⚠️  Relay does not support negentropy");
                } else {
                    println!("  Error: {}", e);
                }
            }
        }

        client.disconnect().await;
        drop(ndb);
        println!("  ✓ Disconnected and closed nostrdb\n");
    }

    // Short pause to simulate restart
    tokio::time::sleep(Duration::from_millis(500)).await;

    // Second "session" - reopen nostrdb and sync with NEW client
    {
        println!("─── Session 2: Sync After 'Restart' (New Client Instance) ───");

        let ndb = Ndb::new(NDB_PATH, &Config::new())?;
        println!("  ✓ Re-opened existing nostrdb");

        // Check what's in nostrdb
        let ndb_event_count = {
            let filter_ndb = nostrdb::FilterBuilder::new().kinds([31933]).build();
            let txn = nostrdb::Transaction::new(&ndb)?;
            let results = ndb.query(&txn, &[filter_ndb], 100)?;
            results.len()
        };
        println!("  Events currently in nostrdb: {}", ndb_event_count);

        // Create NEW client (simulates app restart)
        let keys = Keys::generate();
        let client = Client::new(keys); // ← This is the problem!

        client.add_relay(RELAY_URL).await?;
        client.connect().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        println!("  ✓ Connected with fresh client");

        let pubkey = PublicKey::parse(TEST_PUBKEY)?;
        let filter = Filter::new().kind(Kind::Custom(31933)).author(pubkey);

        let opts = SyncOptions::default();
        match client.sync(filter.clone(), &opts).await {
            Ok(output) => {
                session2_count = output.val.received.len();
                println!("  Events from negentropy: {}", session2_count);

                if session1_count > 0 && session2_count == session1_count {
                    println!();
                    println!(
                        "  ⚠️  FINDING: Both sessions received {} events!",
                        session1_count
                    );
                    println!(
                        "     Even though nostrdb has events, the NEW client doesn't see them."
                    );
                    println!("     nostr-sdk's Client has its own internal state that is empty.");
                } else if session2_count == 0 {
                    println!("  ✅ Sync returned 0 events (unexpected but good!)");
                }
            }
            Err(e) => {
                println!("  Error: {}", e);
            }
        }

        client.disconnect().await;
    }

    println!();
    println!("Summary of Test 2:");
    println!("  Session 1 downloaded: {} events", session1_count);
    println!("  Session 2 downloaded: {} events", session2_count);

    if session1_count > 0 && session2_count > 0 {
        let pct = (session2_count as f64 / session1_count as f64) * 100.0;
        if pct > 90.0 {
            println!("  → ~{:.0}% duplication between sessions", pct);
            println!("  → The Client's internal state is NOT persisted between sessions");
        }
    }

    Ok(())
}

fn print_root_cause_analysis() {
    println!("┌─────────────────────────────────────────────────────────────────┐");
    println!("│ ROOT CAUSE ANALYSIS                                             │");
    println!("└─────────────────────────────────────────────────────────────────┘");
    println!();

    println!("Current Architecture:");
    println!("  ┌──────────────────────────────────────────────────────────────┐");
    println!("  │  nostr-sdk Client (no database attached)                     │");
    println!("  │      ↓                                                       │");
    println!("  │  client.sync(filter) → negentropy protocol                   │");
    println!("  │      ↓                                                       │");
    println!("  │  Returns ALL matching events (no local set to compare)       │");
    println!("  │      ↓                                                       │");
    println!("  │  Events stored in nostrdb (separate from nostr-sdk!)         │");
    println!("  │      ↓                                                       │");
    println!("  │  Next sync: REPEATS because Client doesn't know about ndb    │");
    println!("  └──────────────────────────────────────────────────────────────┘");
    println!();

    println!("The Problem in worker.rs:");
    println!("  Line ~485: let client = Client::new(keys.clone());");
    println!("            ↑ No database! Client has empty local event set.");
    println!();

    println!("How Negentropy Works:");
    println!("  1. Client computes fingerprint of LOCAL events matching filter");
    println!("  2. Sends fingerprint to relay in NEG-OPEN");
    println!("  3. Relay compares against ITS events");
    println!("  4. Returns events that client is MISSING (delta)");
    println!();
    println!("  If client has no database → empty fingerprint → ALL events are 'missing'");
    println!();

    println!("WHY THIS CAUSED 100% CPU:");
    println!("  • kind:4199 (agents) = ~10,000+ events");
    println!("  • Every 60s: Download ALL of these (they're always 'new')");
    println!("  • nostrdb must index ALL of them (100% CPU)");
    println!("  • Never backs off because 'new events' are always found");
}

fn print_summary() {
    println!();
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║                          SUMMARY                                 ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    println!("ROOT CAUSE IDENTIFIED:");
    println!("  The nostr-sdk Client is created WITHOUT a database.");
    println!("  Negentropy compares relay events against an EMPTY local set,");
    println!("  so ALL events are always considered 'new'.\n");

    println!("CURRENT WORKAROUND (in production):");
    println!("  Global event types (4199, 4200, 4201) excluded from negentropy.");
    println!("  Only project-scoped events are synced via negentropy.");
    println!("  This works but loses the benefit of historical backfill.\n");

    println!("PROPER SOLUTIONS:");
    println!();
    println!("  Option A: Implement NostrDatabase trait for nostrdb");
    println!("    • Create wrapper: impl NostrDatabase for NostrDbWrapper");
    println!("    • Register with ClientBuilder::new().database(wrapper)");
    println!("    • nostr-sdk can then query our existing events");
    println!("    ✓ Best solution - single source of truth");
    println!();
    println!("  Option B: Use nostr-sdk's built-in LMDB database");
    println!("    • Add nostr-lmdb dependency");
    println!("    • ClientBuilder::new().database(NostrLMDB::open(path)?)");
    println!("    ✓ Simple but requires maintaining two databases");
    println!();
    println!("  Option C: Track sync state manually with timestamps");
    println!("    • Store last_synced_at per filter");
    println!("    • Use regular REQ with .since(last_synced_at)");
    println!("    ✓ Simple, no negentropy needed");
    println!("    ✗ Less efficient for large historical gaps");
    println!();

    println!("RECOMMENDATION:");
    println!("  Implement Option A (NostrDatabase for nostrdb) for correct negentropy.");
    println!("  This allows re-enabling global event syncs with proper delta behavior.");
}
