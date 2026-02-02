use anyhow::{anyhow, Result};
use nostr_sdk::prelude::*;
use std::time::{Duration, SystemTime};
use tenex_core::constants::RELAY_URL;

fn load_nsec() -> Result<String> {
    if let Ok(nsec) = std::env::var("TENEX_NSEC") {
        return Ok(nsec);
    }
    if let Ok(nsec) = std::env::var("NOSTR_NSEC") {
        return Ok(nsec);
    }
    Err(anyhow!(
        "Missing TENEX_NSEC or NOSTR_NSEC env var for signing key"
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    let nsec = load_nsec()?;
    let keys = Keys::parse(nsec)?;
    let pubkey = keys.public_key();

    println!("Connecting as: {}", pubkey.to_hex());

    let client = Client::new(keys.clone());
    client.add_relay(RELAY_URL).await?;

    println!("Connecting to {}...", RELAY_URL);
    tokio::time::timeout(Duration::from_secs(10), client.connect())
        .await
        .map_err(|_| anyhow!("connect timeout"))??;
    println!("Connected.");

    let mut notifications = client.notifications();
    tokio::spawn(async move {
        while let Ok(notification) = notifications.recv().await {
            match notification {
                RelayPoolNotification::Message { relay_url, message } => {
                    if let RelayMessage::Ok {
                        event_id,
                        status,
                        message,
                    } = message
                    {
                        println!(
                            "[OK] relay={} id={} status={} msg={}",
                            relay_url,
                            event_id.to_hex(),
                            status,
                            message
                        );
                    }
                }
                RelayPoolNotification::Shutdown => {
                    println!("[NOTIF] shutdown");
                    break;
                }
                _ => {}
            }
        }
    });

    let now = SystemTime::now();
    let content = format!("debug_send kind:1 at {:?}", now);
    let event = EventBuilder::new(Kind::TextNote, content).sign_with_keys(&keys)?;

    println!("Sending event id={}...", event.id.to_hex());
    let send_start = std::time::Instant::now();
    let output = client.send_event(&event).await;
    let elapsed = send_start.elapsed();

    match output {
        Ok(output) => println!(
            "send_event OK after {:?}: id={} success={} failed={}",
            elapsed,
            output.id().to_hex(),
            output.success.len(),
            output.failed.len()
        ),
        Err(e) => println!("send_event ERR after {:?}: {}", elapsed, e),
    }

    Ok(())
}
