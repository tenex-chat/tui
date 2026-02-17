use anyhow::Result;
/// Quick test to see which notification path events come through
use nostr_sdk::prelude::*;
use std::collections::HashMap;
use std::time::{Duration, Instant};

const RELAY_URL: &str = "wss://relay.tenex.tech";
const TEST_PUBKEY: &str = "09d48a1a5dbe13404a729634f1d6ba722d40513468dd713c8ea38ca9b7b6f2c7";

#[tokio::main]
async fn main() -> Result<()> {
    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║         QUICK NOTIFICATION PATH TEST (60 seconds)              ║");
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    let keys = Keys::generate();
    let pubkey = PublicKey::parse(TEST_PUBKEY)?;

    let client = Client::new(keys);
    client.add_relay(RELAY_URL).await?;

    println!("Connecting to {}...", RELAY_URL);
    client.connect().await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    println!("✓ Connected\n");

    // Subscribe to project status (the kind we care about)
    let since_time = Timestamp::now() - 45;
    let status_filter = Filter::new()
        .kind(Kind::Custom(24010))
        .custom_tag(SingleLetterTag::lowercase(Alphabet::P), pubkey.to_hex())
        .since(since_time);

    client.subscribe(status_filter.clone(), None).await?;
    println!("✓ Subscribed to project status (kind:24010)\n");

    println!("Listening for 60 seconds...\n");

    let mut notifications = client.notifications();
    let mut event_count = 0u64;
    let mut message_count = 0u64;
    let mut kind_counts: HashMap<u16, u64> = HashMap::new();
    let start = Instant::now();

    let timeout = tokio::time::sleep(Duration::from_secs(60));
    tokio::pin!(timeout);

    loop {
        tokio::select! {
            _ = &mut timeout => {
                break;
            }
            notification = notifications.recv() => {
                match notification {
                    Ok(RelayPoolNotification::Event { event, .. }) => {
                        event_count += 1;
                        *kind_counts.entry(event.kind.as_u16()).or_insert(0) += 1;
                        println!("  [Event path] kind:{} id={}", event.kind.as_u16(), &event.id.to_hex()[..16]);
                    }
                    Ok(RelayPoolNotification::Message { message, relay_url }) => {
                        match &message {
                            RelayMessage::Event { event, .. } => {
                                message_count += 1;
                                *kind_counts.entry(event.kind.as_u16()).or_insert(0) += 1;
                                println!("  [Message path] kind:{} id={} from={}",
                                    event.kind.as_u16(), &event.id.to_hex()[..16], relay_url);
                            }
                            RelayMessage::EndOfStoredEvents(sub_id) => {
                                println!("  [EOSE] sub_id={}", sub_id);
                            }
                            RelayMessage::Ok { event_id, status, message } => {
                                println!("  [OK] event={} status={} msg={}",
                                    &event_id.to_hex()[..16], status, message);
                            }
                            RelayMessage::Closed { subscription_id, message } => {
                                println!("  [CLOSED] sub_id={} reason={}", subscription_id, message);
                            }
                            RelayMessage::Notice(notice) => {
                                println!("  [NOTICE] {}", notice);
                            }
                            RelayMessage::Auth { .. } => {
                                println!("  [AUTH] challenge received");
                            }
                            RelayMessage::Count { subscription_id, count } => {
                                println!("  [COUNT] sub_id={} count={}", subscription_id, count);
                            }
                            RelayMessage::NegMsg { subscription_id, message } => {
                                println!("  [NEGMSG] sub_id={} msg={}", subscription_id, message);
                            }
                            RelayMessage::NegErr { subscription_id, message } => {
                                println!("  [NEGERR] sub_id={} msg={}", subscription_id, message);
                            }
                        }
                    }
                    Ok(RelayPoolNotification::Shutdown) => {
                        println!("\n⚠️  Relay pool shutdown");
                        break;
                    }
                    Err(e) => {
                        println!("\n❌ Error: {:?}", e);
                        break;
                    }
                }
            }
        }
    }

    let elapsed = start.elapsed();

    println!("\n╔════════════════════════════════════════════════════════════════╗");
    println!("║                         RESULTS                                ║");
    println!("╠════════════════════════════════════════════════════════════════╣");
    println!(
        "║ Duration:              {:>8.1}s                             ║",
        elapsed.as_secs_f64()
    );
    println!(
        "║ Event path:            {:>8} events                        ║",
        event_count
    );
    println!(
        "║ Message path:          {:>8} events                        ║",
        message_count
    );
    println!(
        "║ Total:                 {:>8} events                        ║",
        event_count + message_count
    );
    println!("║                                                                ║");
    println!("║ Events by kind:                                                ║");
    for (kind, count) in kind_counts.iter() {
        println!(
            "║   kind:{:<5}            {:>8} events                        ║",
            kind, count
        );
    }
    println!("╚════════════════════════════════════════════════════════════════╝\n");

    Ok(())
}
