use crate::nostr::NostrClient;
use anyhow::Result;
use nostr_sdk::prelude::*;

pub async fn publish_message(
    client: &NostrClient,
    thread_id: &str,
    content: &str,
) -> Result<EventId> {
    let thread_event_id = EventId::parse(thread_id)?;

    let event = EventBuilder::new(
        Kind::Custom(1111),
        content,
    )
    .tag(Tag::event(thread_event_id));

    let id = client.publish(event).await?;
    Ok(id)
}

pub async fn publish_thread(
    client: &NostrClient,
    project_a_tag: &str,
    title: &str,
    content: &str,
) -> Result<EventId> {
    let event = EventBuilder::new(
        Kind::Custom(11),
        content,
    )
    .tag(Tag::custom(
        TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
        [project_a_tag],
    ))
    .tag(Tag::custom(
        TagKind::Custom("title".into()),
        [title],
    ));

    let id = client.publish(event).await?;
    Ok(id)
}
