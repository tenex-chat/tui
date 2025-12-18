use anyhow::Result;
use nostr_sdk::prelude::*;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::Mutex;

const DEFAULT_RELAYS: &[&str] = &[
    "wss://relay.damus.io",
    "wss://nos.lol",
    "wss://relay.nostr.band",
];

#[derive(Clone)]
pub struct NostrClient {
    client: Arc<Mutex<Client>>,
    keys: Keys,
}

impl NostrClient {
    pub async fn new(keys: Keys) -> Result<Self> {
        let client = Client::new(keys.clone());

        for relay in DEFAULT_RELAYS {
            client.add_relay(*relay).await?;
        }

        client.connect().await;

        Ok(Self {
            client: Arc::new(Mutex::new(client)),
            keys,
        })
    }

    pub fn keys(&self) -> &Keys {
        &self.keys
    }

    pub fn pubkey(&self) -> String {
        self.keys.public_key().to_hex()
    }

    pub async fn subscribe(&self, filters: Vec<Filter>) -> Result<()> {
        let client = self.client.lock().await;
        client.subscribe(filters, None).await?;
        Ok(())
    }

    pub async fn fetch_events(&self, filters: Vec<Filter>) -> Result<Vec<Event>> {
        let client = self.client.lock().await;
        let timeout = Duration::from_secs(10);
        let events = client.fetch_events(filters, timeout).await?;
        Ok(events.into_iter().collect())
    }

    pub async fn publish(&self, event: EventBuilder) -> Result<EventId> {
        let client = self.client.lock().await;
        let output = client.send_event_builder(event).await?;
        Ok(*output.id())
    }

    pub async fn disconnect(&self) -> Result<()> {
        let client = self.client.lock().await;
        client.disconnect().await?;
        Ok(())
    }
}
