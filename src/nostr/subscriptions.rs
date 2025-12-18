use crate::nostr::NostrClient;
use crate::store::{events::StoredEvent, insert_events};
use anyhow::Result;
use nostr_sdk::prelude::*;
use rusqlite::Connection;
use std::sync::{Arc, Mutex};

pub async fn subscribe_to_projects(client: &NostrClient, user_pubkey: &str, conn: &Arc<Mutex<Connection>>) -> Result<()> {
    let pubkey = PublicKey::parse(user_pubkey)?;

    let filter = Filter::new()
        .kind(Kind::Custom(31933))
        .author(pubkey);

    let events = client.fetch_events(vec![filter]).await?;

    let stored: Vec<StoredEvent> = events
        .into_iter()
        .map(|e| StoredEvent {
            id: e.id.to_hex(),
            pubkey: e.pubkey.to_hex(),
            kind: e.kind.as_u16() as u32,
            created_at: e.created_at.as_u64(),
            content: e.content.clone(),
            tags: e.tags.iter().map(|t| t.as_slice().iter().map(|s| s.to_string()).collect()).collect(),
            sig: e.sig.to_string(),
        })
        .collect();

    insert_events(conn, &stored)?;
    Ok(())
}

pub async fn subscribe_to_project_content(
    client: &NostrClient,
    project_a_tag: &str,
    conn: &Arc<Mutex<Connection>>,
) -> Result<()> {
    // Subscribe to threads (kind 11)
    let thread_filter = Filter::new()
        .kind(Kind::Custom(11))
        .custom_tag(SingleLetterTag::lowercase(Alphabet::A), [project_a_tag]);

    let thread_events = client.fetch_events(vec![thread_filter]).await?;

    let stored_threads: Vec<StoredEvent> = thread_events
        .iter()
        .map(|e| StoredEvent {
            id: e.id.to_hex(),
            pubkey: e.pubkey.to_hex(),
            kind: e.kind.as_u16() as u32,
            created_at: e.created_at.as_u64(),
            content: e.content.clone(),
            tags: e.tags.iter().map(|t| t.as_slice().iter().map(|s| s.to_string()).collect()).collect(),
            sig: e.sig.to_string(),
        })
        .collect();

    insert_events(conn, &stored_threads)?;

    // Subscribe to messages (kind 1111) for each thread
    let thread_ids: Vec<EventId> = thread_events.iter().map(|e| e.id).collect();

    if !thread_ids.is_empty() {
        let message_filter = Filter::new()
            .kind(Kind::Custom(1111))
            .events(thread_ids);

        let message_events = client.fetch_events(vec![message_filter]).await?;

        let stored_messages: Vec<StoredEvent> = message_events
            .into_iter()
            .map(|e| StoredEvent {
                id: e.id.to_hex(),
                pubkey: e.pubkey.to_hex(),
                kind: e.kind.as_u16() as u32,
                created_at: e.created_at.as_u64(),
                content: e.content.clone(),
                tags: e.tags.iter().map(|t| t.as_slice().iter().map(|s| s.to_string()).collect()).collect(),
                sig: e.sig.to_string(),
            })
            .collect();

        insert_events(conn, &stored_messages)?;
    }

    Ok(())
}

pub async fn subscribe_to_profiles(client: &NostrClient, pubkeys: &[String], conn: &Arc<Mutex<Connection>>) -> Result<()> {
    if pubkeys.is_empty() {
        return Ok(());
    }

    let pks: Vec<PublicKey> = pubkeys
        .iter()
        .filter_map(|p| PublicKey::parse(p).ok())
        .collect();

    if pks.is_empty() {
        return Ok(());
    }

    let filter = Filter::new()
        .kind(Kind::Metadata)
        .authors(pks);

    let events = client.fetch_events(vec![filter]).await?;

    let stored: Vec<StoredEvent> = events
        .into_iter()
        .map(|e| StoredEvent {
            id: e.id.to_hex(),
            pubkey: e.pubkey.to_hex(),
            kind: e.kind.as_u16() as u32,
            created_at: e.created_at.as_u64(),
            content: e.content.clone(),
            tags: e.tags.iter().map(|t| t.as_slice().iter().map(|s| s.to_string()).collect()).collect(),
            sig: e.sig.to_string(),
        })
        .collect();

    insert_events(conn, &stored)?;
    Ok(())
}
