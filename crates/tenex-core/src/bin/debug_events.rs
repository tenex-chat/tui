use anyhow::Result;
use nostr_sdk::prelude::*;
use std::time::Duration;

use tenex_core::constants::RELAY_URL;

#[tokio::main]
async fn main() -> Result<()> {
    let nsec = "nsec1q9kaf583ud7f9jm4xtmj8052uvym9jasy502xnvwxqmsq8lxtmfsvgqa8v";

    let keys = Keys::parse(nsec)?;
    let pubkey = keys.public_key();

    println!("Connecting as: {}", pubkey.to_hex());

    let client = Client::new(keys);
    client.add_relay(RELAY_URL).await?;

    println!("Connecting to {}...", RELAY_URL);
    tokio::time::timeout(Duration::from_secs(5), client.connect()).await.ok();
    println!("Connected!");

    // Fetch projects (kind 31933)
    println!("\n=== Fetching projects (kind 31933) for author {} ===", &pubkey.to_hex()[..16]);
    let project_filter = Filter::new()
        .kind(Kind::Custom(31933))
        .author(pubkey);

    let events = client.fetch_events(project_filter, Duration::from_secs(10)).await?;
    let events_vec: Vec<Event> = events.into_iter().collect();

    println!("Found {} project events:", events_vec.len());
    for event in &events_vec {
        println!("  id: {}", &event.id.to_hex()[..16]);
        println!("  created_at: {}", event.created_at.as_secs());
        println!("  content: {}", &event.content[..100.min(event.content.len())]);

        // Look for d tag (identifier)
        for tag in event.tags.iter() {
            let slice = tag.as_slice();
            if slice.first().map(|s| s.as_str()) == Some("d") {
                println!("  d-tag: {:?}", slice.get(1).map(|s| s.as_str()));
            }
        }
        println!();
    }

    // Fetch project status (kind 24010)
    println!("\n=== Fetching project status (kind 24010) with p-tag {} ===", &pubkey.to_hex()[..16]);
    let status_filter = Filter::new()
        .kind(Kind::Custom(24010))
        .custom_tag(SingleLetterTag::lowercase(Alphabet::P), pubkey.to_hex());

    let status_events = client.fetch_events(status_filter, Duration::from_secs(10)).await?;
    let status_vec: Vec<Event> = status_events.into_iter().collect();

    println!("Found {} status events:", status_vec.len());
    for event in &status_vec {
        println!("  id: {}", &event.id.to_hex()[..16]);
        println!("  author: {}", &event.pubkey.to_hex()[..16]);
        println!("  created_at: {}", event.created_at.as_secs());

        // Look for a tag (project coordinate)
        for tag in event.tags.iter() {
            let slice = tag.as_slice();
            if slice.first().map(|s| s.as_str()) == Some("a") {
                println!("  a-tag: {:?}", slice.get(1).map(|s| s.as_str()));
            }
        }
        println!();
    }

    println!("\n=== Done ===");

    Ok(())
}
