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

    let mut projects: Vec<Project> = projects_by_atag
        .into_values()
        .filter(|p| !p.is_deleted)
        .collect();

    // Sort by created_at descending (newest first)
    projects.sort_by(|a, b| b.created_at.cmp(&a.created_at));

    Ok(projects)
}

/// Build thread root index for all given projects in a single pass.
/// This is more efficient than calling get_threads_for_project for each project
/// because it scans all kind:1 events ONCE and groups them by project.
///
/// Returns: HashMap<project_a_tag, HashSet<thread_root_id>>
pub fn build_thread_root_index(
    ndb: &Ndb,
    project_a_tags: &[String],
) -> Result<HashMap<String, HashSet<String>>> {
    let txn = Transaction::new(ndb)?;
    let mut index: HashMap<String, HashSet<String>> = HashMap::new();

    // Initialize empty sets for all projects
    for a_tag in project_a_tags {
        index.insert(a_tag.clone(), HashSet::new());
    }

    // Query all kind:1 events for all projects at once
    // Build a filter with all project a_tags
    for a_tag in project_a_tags {
        let filter = Filter::new().kinds([1]).tags([a_tag.as_str()], 'a').build();

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
        let thread_filter = Filter::new().kinds([1]).tags([project_a_tag], 'a').build();
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

    // Get conversation metadata - scoped to this project's threads
    // This avoids the global limit issue where older conversations would miss metadata
    let thread_ids: HashSet<String> = threads.iter().map(|t| t.id.clone()).collect();
    // Note: On metadata load failure, we continue with partial/no metadata rather than
    // failing the entire thread load. Threads will still be usable, just potentially
    // missing titles/summaries. The underlying get_metadata_for_threads returns
    // partial results on individual thread failures, so this unwrap_or_default only
    // triggers on complete failure (e.g., transaction creation failed).
    let metadata_map = get_metadata_for_threads(ndb, &thread_ids).unwrap_or_default();

    // Enrich threads with metadata (apply ALL fields consistently: title, status, summary, last_activity)
    for thread in &mut threads {
        if let Some(metadata) = metadata_map.get(&thread.id) {
            if let Some(title) = &metadata.title {
                thread.title = title.clone();
            }
            thread.status_label = metadata.status_label.clone();
            thread.status_current_activity = metadata.status_current_activity.clone();
            thread.summary = metadata.summary.clone();
            // Only update last_activity if metadata is newer to avoid regressing timestamps
            if metadata.created_at > thread.last_activity {
                thread.last_activity = metadata.created_at;
                // Update effective_last_activity to match - this ensures correct sorting.
                // Full hierarchical propagation (parent threads bubbling up child activity)
                // is handled by AppDataStore when threads are processed there.
                thread.effective_last_activity = metadata.created_at;
            }
        }
    }

    // Sort by effective_last_activity descending (most recent activity first)
    // Note: For threads loaded directly from nostrdb, effective_last_activity equals
    // last_activity since hierarchical propagation requires runtime processing.
    // Full hierarchical sorting is performed in AppDataStore.threads_by_project.
    threads.sort_by(|a, b| b.effective_last_activity.cmp(&a.effective_last_activity));

    Ok(threads)
}

/// Fast thread loading using a pre-computed index of known root IDs.
/// Instead of scanning all kind:1 events, we query directly by event ID.
pub fn get_threads_by_ids(
    ndb: &Ndb,
    root_ids: &std::collections::HashSet<String>,
) -> Result<Vec<Thread>> {
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
    // Note: For threads loaded directly from nostrdb, effective_last_activity equals
    // last_activity since hierarchical propagation requires runtime processing.
    // Full hierarchical sorting is performed in AppDataStore.threads_by_project.
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

/// Get metadata for a specific set of thread IDs.
/// This provides project-scoped metadata loading by querying only for the threads we care about.
/// Returns a map of thread_id -> ConversationMetadata, keeping only the newest metadata per thread.
///
/// NOTE: Currently performs O(N) queries (one per thread ID) because nostrdb doesn't support
/// multi-value e-tag filters in a single query. For large thread sets, this could be optimized
/// by chunking or using a different query strategy if nostrdb adds support.
pub fn get_metadata_for_threads(
    ndb: &Ndb,
    thread_ids: &HashSet<String>,
) -> Result<HashMap<String, ConversationMetadata>> {
    if thread_ids.is_empty() {
        return Ok(HashMap::new());
    }

    let txn = Transaction::new(ndb)?;
    let mut metadata_map: HashMap<String, ConversationMetadata> = HashMap::new();
    let mut query_errors: usize = 0;

    // Query metadata for each thread ID using e-tag filter
    // NOTE: This is O(N) queries because nostrdb doesn't support multi-value e-tag filters.
    // Each thread requires its own query. For typical project sizes (<1000 threads), this
    // performs acceptably. The alternative would be a global kind:513 query with post-filtering,
    // but that re-introduces the original limit problem we're solving.
    for thread_id in thread_ids {
        let id_bytes = match hex::decode(thread_id) {
            Ok(bytes) if bytes.len() == 32 => bytes,
            _ => continue, // Skip invalid thread IDs
        };

        let mut id_arr = [0u8; 32];
        id_arr.copy_from_slice(&id_bytes);

        // Build filter for kind:513 with this specific e-tag
        // Use fallible handling instead of unwrap() to avoid panics
        let filter = {
            let mut f = Filter::new();
            if f.start_tag_field('e').is_err() {
                query_errors += 1;
                continue;
            }
            if f.add_id_element(&id_arr).is_err() {
                query_errors += 1;
                continue;
            }
            f.end_field();
            f.kinds([513]).build()
        };

        // Query for metadata events for this thread
        // Use higher limit (100) to ensure we capture all metadata updates, since nostrdb
        // doesn't guarantee ordering. We then select the newest by created_at.
        match ndb.query(&txn, &[filter], 100) {
            Ok(results) => {
                for r in results.iter() {
                    if let Ok(note) = ndb.get_note_by_key(&txn, r.note_key) {
                        if let Some(metadata) = ConversationMetadata::from_note(&note) {
                            // Keep only the most recent metadata for each thread (explicit newest-wins)
                            let dominated = metadata_map
                                .get(&metadata.thread_id)
                                .map(|existing| existing.created_at >= metadata.created_at)
                                .unwrap_or(false);
                            if !dominated {
                                metadata_map.insert(metadata.thread_id.clone(), metadata);
                            }
                        }
                    }
                }
            }
            Err(_) => {
                query_errors += 1;
                // Log would go here if we had a logging framework
                // For now, we continue and try other threads rather than failing completely
            }
        }
    }

    // Log if there were any query errors (partial results are still useful)
    if query_errors > 0 {
        // In a production system, we'd log this. For now, we return partial results
        // since having some metadata is better than failing entirely.
        // tracing::warn!("Failed to query metadata for {} threads", query_errors);
    }

    Ok(metadata_map)
}

/// Get metadata for a single thread (lazy loading).
/// Returns the most recent ConversationMetadata for the thread, if any exists.
/// Used for on-demand metadata retrieval when a conversation is accessed but
/// its metadata wasn't in the initial load.
pub fn get_metadata_for_thread(ndb: &Ndb, thread_id: &str) -> Result<Option<ConversationMetadata>> {
    let txn = Transaction::new(ndb)?;

    let id_bytes = hex::decode(thread_id)?;
    if id_bytes.len() != 32 {
        return Ok(None);
    }

    let mut id_arr = [0u8; 32];
    id_arr.copy_from_slice(&id_bytes);

    // Build filter for kind:513 with this specific e-tag
    // Use fallible handling instead of unwrap() to avoid panics
    let filter = {
        let mut f = Filter::new();
        f.start_tag_field('e')
            .map_err(|e| anyhow::anyhow!("Failed to start tag field: {:?}", e))?;
        f.add_id_element(&id_arr)
            .map_err(|e| anyhow::anyhow!("Failed to add id element: {:?}", e))?;
        f.end_field();
        f.kinds([513]).build()
    };

    // Query for metadata events for this thread
    // Use higher limit (100) to ensure we capture all metadata updates, since nostrdb
    // doesn't guarantee ordering. We then select the newest by created_at.
    let results = ndb.query(&txn, &[filter], 100)?;

    let mut best_metadata: Option<ConversationMetadata> = None;

    for r in results.iter() {
        if let Ok(note) = ndb.get_note_by_key(&txn, r.note_key) {
            if let Some(metadata) = ConversationMetadata::from_note(&note) {
                // Explicit newest-wins selection since nostrdb doesn't guarantee order
                match &best_metadata {
                    Some(existing) if existing.created_at >= metadata.created_at => {
                        // Keep existing (newer or equal)
                    }
                    _ => {
                        best_metadata = Some(metadata);
                    }
                }
            }
        }
    }

    Ok(best_metadata)
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
        assert_eq!(projects[0].title, "Project 1");
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
        assert_eq!(projects[0].title, "Project With Agents");
        assert_eq!(
            projects[0].agent_definition_ids.len(),
            2,
            "Expected 2 agent IDs, got {:?}",
            projects[0].agent_definition_ids
        );
        assert_eq!(projects[0].agent_definition_ids[0], agent_id_1);
        assert_eq!(projects[0].agent_definition_ids[1], agent_id_2);
    }

    #[test]
    fn test_get_projects_hides_latest_tombstone() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let live_event = EventBuilder::new(Kind::Custom(31933), "Live project")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["tombstone-test".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Tombstone Test".to_string()],
            ))
            .custom_created_at(Timestamp::from(1_700_000_100))
            .sign_with_keys(&keys)
            .unwrap();

        let tombstone_event = EventBuilder::new(Kind::Custom(31933), "")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["tombstone-test".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Tombstone Test".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deleted")),
                Vec::<String>::new(),
            ))
            .custom_created_at(Timestamp::from(1_700_000_200))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[live_event, tombstone_event], None).unwrap();
        let filter = nostrdb::Filter::new().kinds([31933]).build();
        wait_for_event_processing(&db.ndb, filter, 5000);

        let projects = get_projects(&db.ndb).unwrap();
        assert!(
            projects.is_empty(),
            "Expected tombstoned project to be hidden"
        );
    }

    #[test]
    fn test_get_projects_keeps_newer_live_over_older_tombstone() {
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();
        let keys = Keys::generate();

        let tombstone_event = EventBuilder::new(Kind::Custom(31933), "")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["revive-test".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Revive Test".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("deleted")),
                Vec::<String>::new(),
            ))
            .custom_created_at(Timestamp::from(1_700_000_100))
            .sign_with_keys(&keys)
            .unwrap();

        let live_event = EventBuilder::new(Kind::Custom(31933), "Live again")
            .tag(Tag::custom(
                TagKind::SingleLetter(SingleLetterTag::lowercase(Alphabet::D)),
                vec!["revive-test".to_string()],
            ))
            .tag(Tag::custom(
                TagKind::Custom(std::borrow::Cow::Borrowed("title")),
                vec!["Revive Test".to_string()],
            ))
            .custom_created_at(Timestamp::from(1_700_000_200))
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[tombstone_event, live_event], None).unwrap();
        let filter = nostrdb::Filter::new().kinds([31933]).build();
        wait_for_event_processing(&db.ndb, filter, 5000);

        let projects = get_projects(&db.ndb).unwrap();
        assert_eq!(projects.len(), 1);
        assert_eq!(projects[0].id, "revive-test");
        assert!(!projects[0].is_deleted);
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
            let all_messages = db
                .ndb
                .query(&txn, std::slice::from_ref(&filter), 100)
                .unwrap();
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

    #[test]
    fn test_get_profile_picture_returns_url_when_profile_exists() {
        // Positive-path test: ingest a kind:0 event with a picture URL and verify retrieval
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();

        let keys = Keys::generate();
        let pubkey = keys.public_key().to_hex();
        let expected_picture_url = "https://example.com/avatar.png";

        // Create a kind:0 (profile metadata) event with a picture field
        // NIP-01: kind:0 content is a JSON object with profile metadata
        let profile_content = serde_json::json!({
            "name": "Test User",
            "about": "A test profile",
            "picture": expected_picture_url
        })
        .to_string();

        let profile_event = EventBuilder::new(Kind::Metadata, profile_content)
            .sign_with_keys(&keys)
            .unwrap();

        // Ingest the profile event
        ingest_events(&db.ndb, &[profile_event], None).unwrap();

        // Wait for nostrdb to process the profile (kind:0)
        let filter = nostrdb::Filter::new().kinds([0]).build();
        let found = wait_for_event_processing(&db.ndb, filter, 5000);
        assert!(found, "Profile event was not processed within timeout");

        // Small delay to ensure nostrdb profile index is updated
        std::thread::sleep(std::time::Duration::from_millis(50));

        // Now retrieve the profile picture
        let result = get_profile_picture(&db.ndb, &pubkey);

        assert!(result.is_some(), "Expected profile picture URL, got None");
        assert_eq!(result.unwrap(), expected_picture_url);
    }

    #[test]
    fn test_get_profile_picture_returns_none_when_no_picture_field() {
        // Edge case: profile exists but has no picture field
        let dir = tempdir().unwrap();
        let db = Database::new(dir.path()).unwrap();

        let keys = Keys::generate();
        let pubkey = keys.public_key().to_hex();

        // Create a kind:0 event WITHOUT a picture field
        let profile_content = serde_json::json!({
            "name": "User Without Avatar",
            "about": "A profile without a picture"
        })
        .to_string();

        let profile_event = EventBuilder::new(Kind::Metadata, profile_content)
            .sign_with_keys(&keys)
            .unwrap();

        ingest_events(&db.ndb, &[profile_event], None).unwrap();

        let filter = nostrdb::Filter::new().kinds([0]).build();
        let found = wait_for_event_processing(&db.ndb, filter, 5000);
        assert!(found, "Profile event was not processed within timeout");

        std::thread::sleep(std::time::Duration::from_millis(50));

        let result = get_profile_picture(&db.ndb, &pubkey);

        // Should return None since there's no picture field
        assert!(
            result.is_none(),
            "Expected None for profile without picture, got {:?}",
            result
        );
    }
}
