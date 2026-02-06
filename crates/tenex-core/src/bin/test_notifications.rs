/// Test program to investigate nostr-sdk notification paths
///
/// This program:
/// 1. Uses the exact same filters as the main app
/// 2. Monitors both RelayPoolNotification::Event and RelayMessage::Event paths
/// 3. Tests with and without negentropy sync
/// 4. Tracks event duplication and routing
use nostr_sdk::prelude::*;
use std::collections::HashSet;
use std::time::{Duration, Instant};
use anyhow::Result;

const RELAY_URL: &str = "wss://relay.tenex.tech";
const TEST_PUBKEY: &str = "09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7";

// Kind constants
const KIND_PROJECT_DRAFT: u16 = 31933;
const KIND_PROJECT_STATUS: u16 = 24010;
const KIND_AGENT_STATUS: u16 = 24133;
const KIND_AGENT: u16 = 4199;
const KIND_MCP_TOOL: u16 = 4200;
const KIND_NUDGE: u16 = 4201;
const KIND_TEXT_NOTE: u16 = 1;
const KIND_PROJECT_METADATA: u16 = 513;
const KIND_LONG_FORM_CONTENT: u16 = 30023;

struct EventStats {
    total_via_event: u64,
    total_via_message: u64,
    unique_ids: HashSet<String>,
    start_time: Instant,
}

impl EventStats {
    fn new() -> Self {
        Self {
            total_via_event: 0,
            total_via_message: 0,
            unique_ids: HashSet::new(),
            start_time: Instant::now(),
        }
    }

    fn record_via_event(&mut self, event_id: &EventId) {
        self.total_via_event += 1;
        self.unique_ids.insert(event_id.to_hex());
    }

    fn record_via_message(&mut self, event_id: &EventId) {
        self.total_via_message += 1;
        self.unique_ids.insert(event_id.to_hex());
    }

    fn print_summary(&self) {
        let elapsed = self.start_time.elapsed();
        let unique_count = self.unique_ids.len();
        let duplication_factor = if unique_count > 0 {
            (self.total_via_event + self.total_via_message) as f64 / unique_count as f64
        } else {
            0.0
        };

        println!("\n╔════════════════════════════════════════════════════════════════╗");
        println!("║                     EVENT STATISTICS                           ║");
        println!("╠════════════════════════════════════════════════════════════════╣");
        println!("║ Time elapsed:          {:>8.1}s                             ║", elapsed.as_secs_f64());
        println!("║ Via Event path:        {:>8} events                        ║", self.total_via_event);
        println!("║ Via Message path:      {:>8} events                        ║", self.total_via_message);
        println!("║ Total events:          {:>8}                                 ║", self.total_via_event + self.total_via_message);
        println!("║ Unique event IDs:      {:>8}                                 ║", unique_count);
        println!("║ Duplication factor:    {:>8.2}x                              ║", duplication_factor);
        println!("╚════════════════════════════════════════════════════════════════╝");
    }
}

async fn test_with_negentropy(enable_negentropy: bool) -> Result<()> {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║  TEST: {} NEGENTROPY                    ║", if enable_negentropy { "WITH" } else { "WITHOUT" });
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    let keys = Keys::generate();
    let pubkey = PublicKey::parse(TEST_PUBKEY)?;
    let pubkey_hex = pubkey.to_hex();

    // Create client (no database, to avoid state issues)
    let client = Client::new(keys);
    client.add_relay(RELAY_URL).await?;

    println!("Connecting to {}...", RELAY_URL);
    client.connect().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("✓ Connected\n");

    // Create the exact same subscriptions as the main app
    println!("Creating subscriptions:");

    // 1. User projects (kind 31933)
    let project_filter = Filter::new()
        .kind(Kind::Custom(KIND_PROJECT_DRAFT))
        .author(pubkey);
    client.subscribe(project_filter.clone(), None).await?;
    println!("  ✓ Projects (kind:31933)");

    // 2. Project status (kind 24010) - last 45 seconds
    let since_time = Timestamp::now() - 45;
    let status_filter = Filter::new()
        .kind(Kind::Custom(KIND_PROJECT_STATUS))
        .custom_tag(SingleLetterTag::lowercase(Alphabet::P), pubkey_hex.clone())
        .since(since_time);
    client.subscribe(status_filter.clone(), None).await?;
    println!("  ✓ Project status (kind:24010)");

    // 3. Agent status (kind 24133)
    let agent_status_filter = Filter::new()
        .kind(Kind::Custom(KIND_AGENT_STATUS))
        .custom_tag(SingleLetterTag::uppercase(Alphabet::P), pubkey_hex.clone())
        .since(since_time);
    client.subscribe(agent_status_filter.clone(), None).await?;
    println!("  ✓ Agent status (kind:24133)");

    // 4. Global definitions (kinds 4199, 4200, 4201)
    let global_filter = Filter::new()
        .kinds(vec![
            Kind::Custom(KIND_AGENT),
            Kind::Custom(KIND_MCP_TOOL),
            Kind::Custom(KIND_NUDGE),
        ]);
    client.subscribe(global_filter.clone(), None).await?;
    println!("  ✓ Global definitions (kinds:4199,4200,4201)");

    println!("\nListening to notifications...");
    let mut notifications = client.notifications();
    let mut stats = EventStats::new();

    // Spawn negentropy sync if enabled
    let negentropy_handle = if enable_negentropy {
        println!("⚡ Starting negentropy sync (60s intervals)...\n");
        let client_clone = client.clone();
        let pubkey_clone = pubkey;
        Some(tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(5)).await; // Initial delay

            loop {
                println!("\n🔄 Running negentropy sync...");
                let sync_start = Instant::now();

                // Sync the same filters
                let filters = vec![
                    Filter::new().kind(Kind::Custom(KIND_PROJECT_DRAFT)).author(pubkey_clone),
                    Filter::new().kind(Kind::Custom(KIND_AGENT)),
                    Filter::new().kind(Kind::Custom(KIND_MCP_TOOL)),
                    Filter::new().kind(Kind::Custom(KIND_NUDGE)),
                ];

                for filter in filters {
                    let opts = SyncOptions::default();
                    match client_clone.sync(filter, &opts).await {
                        Ok(output) => {
                            let count = output.val.received.len();
                            if count > 0 {
                                println!("  Synced {} events", count);
                            }
                        }
                        Err(e) => {
                            if !format!("{}", e).contains("not supported") {
                                println!("  Sync error: {}", e);
                            }
                        }
                    }
                }

                println!("  ✓ Sync completed in {:.1}s\n", sync_start.elapsed().as_secs_f64());
                tokio::time::sleep(Duration::from_secs(60)).await;
            }
        }))
    } else {
        None
    };

    // Listen for 3 minutes
    let test_duration = Duration::from_secs(180);
    let timeout = tokio::time::sleep(test_duration);
    tokio::pin!(timeout);

    let mut last_summary = Instant::now();

    loop {
        tokio::select! {
            _ = &mut timeout => {
                println!("\n⏱️  Test duration complete (3 minutes)");
                break;
            }
            notification = notifications.recv() => {
                match notification {
                    Ok(RelayPoolNotification::Event { event, .. }) => {
                        stats.record_via_event(&event.id);
                    }
                    Ok(RelayPoolNotification::Message { message, .. }) => {
                        if let RelayMessage::Event { event, .. } = &message {
                            stats.record_via_message(&event.id);
                        }
                    }
                    Ok(_) => {} // Other notification types
                    Err(e) => {
                        println!("\n❌ Notification error: {:?}", e);
                        break;
                    }
                }

                // Print summary every 30 seconds
                if last_summary.elapsed() >= Duration::from_secs(30) {
                    stats.print_summary();
                    last_summary = Instant::now();
                }
            }
        }
    }

    // Cleanup
    if let Some(handle) = negentropy_handle {
        handle.abort();
    }

    stats.print_summary();

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║         NOSTR-SDK NOTIFICATION PATH INVESTIGATION              ║");
    println!("║                                                                ║");
    println!("║  This test investigates why events come via                   ║");
    println!("║  RelayMessage::Event instead of RelayPoolNotification::Event  ║");
    println!("║                                                                ║");
    println!("║  Hypothesis: Negentropy sync causes the flood of events       ║");
    println!("╚════════════════════════════════════════════════════════════════╝");

    // Test 1: Without negentropy
    test_with_negentropy(false).await?;

    println!("\n\n");
    tokio::time::sleep(Duration::from_secs(5)).await;

    // Test 2: With negentropy
    test_with_negentropy(true).await?;

    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║                         TESTS COMPLETE                         ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    Ok(())
}
