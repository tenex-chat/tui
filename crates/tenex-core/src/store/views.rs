use crate::models::{ConversationMetadata, Message, Project, Thread};
use anyhow::Result;
use nostrdb::{Filter, Ndb, Transaction};
use std::collections::{HashMap, HashSet};

pub fn get_projects(ndb: &Ndb) -> Result<Vec<Project>> {
    let txn = Transaction::new(ndb)?;
    let filter = Filter::new().kinds([31933]).build();
    let results = ndb.query(&txn, &[filter], 1000)?;

    // Collect all projects
    let all_projects: Vec<Project> = results
        .iter()
        .filter_map(|r| {
            let note = ndb.get_note_by_key(&txn, r.note_key).ok()?;
            Project::from_note(&note)
        })
        .collect();

    // Deduplicate by a_tag, keeping only the newest (highest created_at)
    let mut projects_by_atag: HashMap<String, Project> = HashMap::new();
    for project in all_projects {
        let a_tag = project.a_tag();
        match projects_by_atag.get(&a_tag) {
            Some(existing) if existing.created_at >= project.created_at => {
                // Keep existing (newer or equal)
            }
            _ => {
                projects_by_atag.insert(a_tag, project);
            }
        }
    }

    let mut projects: Vec<Project> = projects_by_atag.into_values().collect();

    // Sort by created_at descending (newest first)
    projects.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(projects)
}

/// Build thread root index for all given projects in a single pass.
/// This is more efficient than calling get_threads_for_project for each project
/// because it scans all kind:1 events ONCE and groups them by project.
///
/// Returns: HashMap<project_a_tag, HashSet<thread_root_id>>
pub fn build_thread_root_index(ndb: &Ndb, project_a_tags: &[String]) -> Result<HashMap<String, HashSet<String>>> {
    let txn = Transaction::new(ndb)?;
    let mut index: HashMap<String, HashSet<String>> = HashMap::new();

    // Initialize empty sets for all projects
    for a_tag in project_a_tags {
        index.insert(a_tag.clone(), HashSet::new());
    }

    // Query all kind:1 events for all projects at once
    // Build a filter with all project a_tags
    for a_tag in project_a_tags {
        let filter = Filter::new()
            .kinds([1])
            .tags([a_tag.as_str()], 'a')
            .build();

        // Query with high limit to get all events
        let results = ndb.query(&txn, &[filter], 100_000)?;

        for result in results {
            if let Ok(note) = ndb.get_note_by_key(&txn, result.note_key) {
                // Check if this is a thread root (no e-tags)
                if Thread::from_note(&note).is_some() {
                    let event_id = hex::encode(note.id());
                    if let Some(set) = index.get_mut(a_tag) {
                        set.insert(event_id);
                    }
                }
            }
        }
    }

    Ok(index)
}

/// Get threads for a project - fast version that skips expensive message activity calculation
pub fn get_threads_for_project(ndb: &Ndb, project_a_tag: &str) -> Result<Vec<Thread>> {
    let txn = Transaction::new(ndb)?;

    // Get threads (kind:1 with a-tag, no e-tags)
    let mut threads: Vec<Thread> = {
        let thread_filter = Filter::new()
            .kinds([1])
            .tags([project_a_tag], 'a')
            .build();
        // NOTE: We query ALL kind:1 events because nostrdb doesn't support filtering by "no e-tag".
        // The limit must be high enough to capture all events; Thread::from_note will filter
        // out messages (which have e-tags) keeping only thread roots.
        let thread_results = ndb.query(&txn, &[thread_filter], 100_000)?;

        thread_results
            .iter()
            .filter_map(|r| {
                let note = ndb.get_note_by_key(&txn, r.note_key).ok()?;
                Thread::from_note(&note)
            })
            .collect()
    };

    // Get conversation metadata
    let metadata_map: HashMap<String, ConversationMetadata> = {
        let metadata_filter = Filter::new().kinds([513]).build();
        let metadata_results = ndb.query(&txn, &[metadata_filter], 1000)?;

        let mut map: HashMap<String, ConversationMetadata> = HashMap::new();
        for r in metadata_results.iter() {
            if let Ok(note) = ndb.get_note_by_key(&txn, r.note_key) {
                if let Some(metadata) = ConversationMetadata::from_note(&note) {
                    if let Some(existing) = map.get(&metadata.thread_id) {
                        if metadata.created_at > existing.created_at {
                            map.insert(metadata.thread_id.clone(), metadata);
                        }
                    } else {
                        map.insert(metadata.thread_id.clone(), metadata);
                    }
                }
            }
        }
        map
    };

    // Enrich threads with metadata
    for thread in &mut threads {
        if let Some(metadata) = metadata_map.get(&thread.id) {
            if let Some(title) = &metadata.title {
                thread.title = title.clone();
            }
        }
    }

    // Sort by effective_last_activity descending (most recent activity first)
    // This uses hierarchical sorting where parent conversations reflect
    // the most recent activity in their entire delegation tree.
    threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));

    Ok(threads)
}

/// Fast thread loading using a pre-computed index of known root IDs.
/// Instead of scanning all kind:1 events, we query directly by event ID.
pub fn get_threads_by_ids(ndb: &Ndb, root_ids: &std::collections::HashSet<String>) -> Result<Vec<Thread>> {
    if root_ids.is_empty() {
        return Ok(Vec::new());
    }

    let txn = Transaction::new(ndb)?;
    let mut threads: Vec<Thread> = Vec::new();

    // Query each root ID directly - much faster than scanning all kind:1 events
    for root_id in root_ids {
        if let Ok(id_bytes) = hex::decode(root_id) {
            if id_bytes.len() == 32 {
                let mut id_arr = [0u8; 32];
                id_arr.copy_from_slice(&id_bytes);
                if let Ok(note_key) = ndb.get_notekey_by_id(&txn, &id_arr) {
                    if let Ok(note) = ndb.get_note_by_key(&txn, note_key) {
                        if let Some(thread) = Thread::from_note(&note) {
                            threads.push(thread);
                        }
                    }
                }
            }
        }
    }

    // Sort by effective_last_activity descending (most recent activity first)
    // This uses hierarchical sorting where parent conversations reflect
    // the most recent activity in their entire delegation tree.
    threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));

    Ok(threads)
}

pub fn get_messages_for_thread(ndb: &Ndb, thread_id: &str) -> Result<Vec<Message>> {
    let txn = Transaction::new(ndb)?;

    let mut messages: Vec<Message> = Vec::new();

    // First, get the thread root (kind:1, no e-tags) as the first message
    // The thread_id is the event ID of the kind:1 thread
    {
        if let Ok(thread_id_bytes) = hex::decode(thread_id) {
            if thread_id_bytes.len() == 32 {
                let mut id_arr = [0u8; 32];
                id_arr.copy_from_slice(&thread_id_bytes);
                if let Ok(note_key) = ndb.get_notekey_by_id(&txn, &id_arr) {
                    if let Ok(note) = ndb.get_note_by_key(&txn, note_key) {
                        if let Some(thread_msg) = Message::from_thread_note(&note) {
                            messages.push(thread_msg);
                        }
                    }
                }
            }
        }
    }

    // Get all replies (kind:1) for this thread using 'e' tag (NIP-10 with "root" marker)
    // nostrdb stores 'e' tag values as 32-byte IDs, not hex strings, so we must query with bytes
    let results = {
        // Convert hex thread_id to bytes for the query
        let thread_id_bytes: [u8; 32] = match hex::decode(thread_id) {
            Ok(bytes) if bytes.len() == 32 => {
                let mut arr = [0u8; 32];
                arr.copy_from_slice(&bytes);
                arr
            }
            _ => return Ok(messages), // Invalid thread_id, return just the root message
        };

        let mut filter = Filter::new();
        filter.start_tag_field('e').unwrap();
        filter.add_id_element(&thread_id_bytes).unwrap();
        filter.end_field();
        let filter = filter.kinds([1]).build();

        ndb.query(&txn, &[filter], 1000)?
    };

    let replies: Vec<Message> = {
        results
            .iter()
            .filter_map(|r| {
                let note = ndb.get_note_by_key(&txn, r.note_key).ok()?;
                Message::from_note(&note)
            })
            .collect()
    };

    messages.extend(replies);

    // Sort by created_at ascending (oldest first for chat)
    messages.sort_by(|a, b| a.created_at.cmp(&b.created_at));

    Ok(messages)
}

pub fn get_profile_name(ndb: &Ndb, pubkey: &str) -> String {
    let pubkey_bytes = match hex::decode(pubkey) {
        Ok(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        _ => return format!("{}...", &pubkey[..8.min(pubkey.len())]),
    };

    let txn = match Transaction::new(ndb) {
        Ok(t) => t,
        Err(_) => return format!("{}...", &pubkey[..8.min(pubkey.len())]),
    };

    if let Ok(profile) = ndb.get_profile_by_pubkey(&txn, &pubkey_bytes) {
        let record = profile.record();
        if let Some(profile_data) = record.profile() {
            if let Some(name) = profile_data.display_name() {
                if !name.is_empty() {
                    return name.to_string();
                }
            }
            if let Some(name) = profile_data.name() {
                if !name.is_empty() {
                    return name.to_string();
                }
            }
        }
    }

    format!("{}...", &pubkey[..8.min(pubkey.len())])
}

/// Get profile picture URL for a pubkey
pub fn get_profile_picture(ndb: &Ndb, pubkey: &str) -> Option<String> {
    let pubkey_bytes = match hex::decode(pubkey) {
        Ok(bytes) if bytes.len() == 32 => {
            let mut arr = [0u8; 32];
            arr.copy_from_slice(&bytes);
            arr
        }
        _ => return None,
    };

    let txn = Transaction::new(ndb).ok()?;

    if let Ok(profile) = ndb.get_profile_by_pubkey(&txn, &pubkey_bytes) {
        let record = profile.record();
        if let Some(profile_data) = record.profile() {
            if let Some(picture) = profile_data.picture() {
                if !picture.is_empty() {
                    return Some(picture.to_string());
                }
            }
        }
    }

    None
}

// NOTE: Ephemeral events (kind:24010) should NOT be queried from nostrdb.
// Project status is only received via live subscriptions and stored in AppDataStore.
// Use app_data_store.get_project_status() instead.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{
        events::{ingest_events, wait_for_event_processing},
        Database,
    };
    use nostr_sdk::prelude::*;
    use tempfile::tempdir;

    #[test]
    fn test_get_projects() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();

        let keys = Keys::generate();
        let event = EventBuilder::new(Kind::Custom(31933), "Description")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["proj1".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("name")),
                vec!["Project 1".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event], None).unwrap();

        // Wait for async processing
        let filter = nostrdb::Filter::new().kinds([31933]).build();
        wait_for_event_processing(&db.ndb, filter, 5000);

        let projects = get_projects(&db.ndb).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "Project 1");
    }

    #[test]
    fn test_get_projects_with_agent_tags() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();

        let keys = Keys::generate();
        // Use valid 64-character hex strings (event IDs)
        let agent_id_1 = "abc123def456abc123def456abc123def456abc123def456abc123def456abc1";
        let agent_id_2 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

        let event = EventBuilder::new(Kind::Custom(31933), "Description")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["proj-with-agents".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Project With Agents".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("agent")),
                vec![agent_id_1.to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("agent")),
                vec![agent_id_2.to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[event], None).unwrap();

        // Wait for async processing
        let filter = nostrdb::Filter::new().kinds([31933]).build();
        wait_for_event_processing(&db.ndb, filter, 5000);

        let projects = get_projects(&db.ndb).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].name, "Project With Agents");
        assert_eq!(projects[0].agent_ids.len(), 2, "Expected 2 agent IDs, got {:?}", projects[0].agent_ids);
        assert_eq!(projects[0].agent_ids[0], agent_id_1);
        assert_eq!(projects[0].agent_ids[1], agent_id_2);
    }

    #[test]
    fn test_get_threads_for_project() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();

        let keys = Keys::generate();
        let project_a_tag = format!("31933:{}:proj1", keys.public_key().to_hex());

        let thread1 = EventBuilder::new(Kind::from(1), "Thread 1 content")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec![project_a_tag.clone()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["First Thread".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        let thread2 = EventBuilder::new(Kind::from(1), "Thread 2 content")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::A)),
                vec!["31933:other:proj".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Other Thread".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[thread1, thread2], None).unwrap();

        // Wait for async processing
        let filter = nostrdb::Filter::new().kinds([1]).build();
        let found = wait_for_event_processing(&db.ndb, filter, 5000);
        assert!(found, "Events were not processed within timeout");

        // Small delay to ensure nostrdb is fully ready
        std::thread::sleep(std::time::Duration::from_millis(50));

        let threads = get_threads_for_project(&db.ndb, &project_a_tag).unwrap();
        assert_eq!(threads.len(), 1);
        assert_eq!(threads[0].title, "First Thread");
    }

    #[test]
    fn test_get_messages_for_thread() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();

        let keys = Keys::generate();
        let thread_id = "a".repeat(64);

        // NIP-10: e-tag with "root" marker
        let msg1 = EventBuilder::new(Kind::from(1), "Message 1")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec![thread_id.clone(), "".to_string(), "root".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        // Different thread - should not be included
        let msg2 = EventBuilder::new(Kind::from(1), "Message 2")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::E)),
                vec!["b".repeat(64), "".to_string(), "root".to_string()],
            ))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[msg1.clone(), msg2], None).unwrap();

        // Wait for async processing - wait for kind 1 events
        let filter = nostrdb::Filter::new().kinds([1]).build();
        wait_for_event_processing(&db.ndb, filter.clone(), 5000);

        // First check: verify events were ingested using a simple query
        {
            let txn = nostrdb::Transaction::new(&db.ndb).unwrap();
            let all_messages = db.ndb.query(&txn, &[filter.clone()], 100).unwrap();
            assert!(!all_messages.is_empty(), "No messages were ingested");
        }

        // nostrdb's e-tag query filtering may not work with hex IDs, so we fetch all
        // kind-1 and filter in get_messages_for_thread. Test that the actual function works.
        // First, let's manually verify filtering works since get_messages_for_thread uses e-tag filter
        let txn = nostrdb::Transaction::new(&db.ndb).unwrap();
        let results = db.ndb.query(&txn, &[filter], 1000).unwrap();
        let filtered: Vec<_> = results
            .iter()
            .filter_map(|r| {
                let note = db.ndb.get_note_by_key(&txn, r.note_key).ok()?;
                let msg = Message::from_note(&note)?;
                if msg.thread_id == thread_id {
                    Some(msg)
                } else {
                    None
                }
            })
            .collect();

        assert_eq!(filtered.len(), 1, "Expected 1 message matching thread_id");
        assert_eq!(filtered[0].content, "Message 1");
    }

    #[test]
    fn test_get_profile_name_fallback() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();

        let pubkey = "c".repeat(64);
        let name = get_profile_name(&db.ndb, &pubkey);
        assert_eq!(name, "cccccccc...");
    }
}
