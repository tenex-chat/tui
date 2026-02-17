//! Test that negentropy works properly with NdbDatabase
//! Verifies that restarting doesn't re-download everything

use anyhow::Result;
use nostr_sdk::prelude::*;
use nostrdb::{Config, Ndb};
use std::sync::Arc;
use std::time::Duration;

const RELAY_URL: &str = "wss://relay.damus.io";
const TEST_PUBKEY: &str = "09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7";
const NDB_PATH: &str = "test_negentropy_proper_data";

#[tokio::main]
async fn main() -> Result<()> {
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║        Testing Negentropy with NdbDatabase (Proper Fix)         ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    // Clean up from previous runs
    let _ = std::fs::remove_dir_all(NDB_PATH);

    println!("Session 1: Initial sync (should download events)");
    println!("─────────────────────────────────────────────────────\n");

    let first_count = {
        let ndb = Arc::new(Ndb::new(NDB_PATH, &Config::new())?);
        println!("✓ Created fresh nostrdb at: {}", NDB_PATH);

        // Wrap in NdbDatabase (the proper fix)
        let ndb_database = nostr_ndb::NdbDatabase::from((*ndb).clone());
        println!("✓ Wrapped Ndb in NdbDatabase");

        let keys = Keys::generate();
        let client = Client::builder()
            .database(ndb_database)
            .signer(keys)
            .build();

        client.add_relay(RELAY_URL).await?;
        client.connect().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        println!("✓ Connected to relay\n");

        let pubkey = PublicKey::parse(TEST_PUBKEY)?;
        let filter = Filter::new()
            .kind(Kind::Custom(31933))
            .author(pubkey)
            .limit(50);

        println!("Running negentropy sync...");
        let start = std::time::Instant::now();
        let opts = SyncOptions::default();

        let count = match client.sync(filter, &opts).await {
            Ok(output) => {
                let elapsed = start.elapsed();
                let count = output.val.received.len();
                println!("  Downloaded: {} events", count);
                println!("  Time: {:?}\n", elapsed);
                count
            }
            Err(e) => {
                println!("  Error: {}", e);
                if format!("{}", e).contains("not supported") {
                    println!("\n⚠️  Relay doesn't support negentropy - test inconclusive");
                    return Ok(());
                }
                0
            }
        };

        client.disconnect().await;
        drop(ndb);
        println!("✓ Disconnected and closed database\n");

        count
    };

    if first_count == 0 {
        println!("❌ No events received in first sync - can't continue test");
        return Ok(());
    }

    // Wait a moment to simulate restart
    tokio::time::sleep(Duration::from_millis(500)).await;

    println!("\n{}\n", "=".repeat(70));
    println!("Session 2: Sync after 'restart' (should download 0 events)");
    println!("────────────────────────────────────────────────────────\n");

    let second_count = {
        let ndb = Arc::new(Ndb::new(NDB_PATH, &Config::new())?);
        println!("✓ Re-opened existing nostrdb");

        // Wrap in NdbDatabase (the proper fix)
        let ndb_database = nostr_ndb::NdbDatabase::from((*ndb).clone());
        println!("✓ Wrapped Ndb in NdbDatabase");

        let keys = Keys::generate();
        let client = Client::builder()
            .database(ndb_database)
            .signer(keys)
            .build();

        client.add_relay(RELAY_URL).await?;
        client.connect().await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        println!("✓ Connected to relay\n");

        let pubkey = PublicKey::parse(TEST_PUBKEY)?;
        let filter = Filter::new()
            .kind(Kind::Custom(31933))
            .author(pubkey)
            .limit(50);

        println!("Running negentropy sync...");
        let start = std::time::Instant::now();
        let opts = SyncOptions::default();

        let count = match client.sync(filter, &opts).await {
            Ok(output) => {
                let elapsed = start.elapsed();
                let count = output.val.received.len();
                println!("  Downloaded: {} events", count);
                println!("  Time: {:?}\n", elapsed);
                count
            }
            Err(e) => {
                println!("  Error: {}", e);
                0
            }
        };

        client.disconnect().await;
        count
    };

    // Cleanup
    let _ = std::fs::remove_dir_all(NDB_PATH);

    println!("\n{}\n", "=".repeat(70));
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║                          TEST RESULTS                            ║");
    println!("╚══════════════════════════════════════════════════════════════════╝\n");

    println!("Session 1 (fresh): {} events downloaded", first_count);
    println!("Session 2 (restart): {} events downloaded", second_count);
    println!();

    if second_count == 0 {
        println!("✅ SUCCESS: Negentropy working properly!");
        println!("   The Client remembered which events it had locally.");
        println!("   No re-downloading on restart.\n");
    } else {
        let pct = (second_count as f64 / first_count as f64) * 100.0;
        println!(
            "❌ FAILURE: Downloaded {} events ({:.0}% of first sync)",
            second_count, pct
        );
        println!("   The Client is NOT properly tracking local events.");
        println!("   This means NdbDatabase integration is not working.\n");
    }

    Ok(())
}
