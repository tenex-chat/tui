use super::*;

#[uniffi::export]
impl TenexCore {
    /// Get a list of projects.
    ///
    /// Queries nostrdb for kind 31933 events and returns them as Project.
    pub fn get_projects(&self) -> Vec<Project> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        store.get_projects().to_vec()
    }

    /// Get conversations for a project.
    ///
    /// Returns conversations organized with parent/child relationships.
    /// Use thread.parent_conversation_id to build nested conversation trees.
    pub fn get_conversations(&self, project_id: String) -> Vec<ConversationFullInfo> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        // Find the project by ID and get its a_tag
        let project = store.get_projects().iter().find(|p| p.id == project_id);
        let project_a_tag = match project {
            Some(p) => p.a_tag(),
            None => return Vec::new(),
        };

        let prefs_guard = match self.preferences.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let archived_ids = prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.clone())
            .unwrap_or_default();

        // Get threads for this project
        let threads = store.get_threads(&project_a_tag);

        threads
            .iter()
            .map(|t| thread_to_full_info(store, t, &archived_ids))
            .collect()
    }

    /// Get the total hierarchical LLM runtime for a conversation (includes all descendants) in milliseconds.
    /// Returns 0 if the conversation is not found or has no runtime data.
    pub fn get_conversation_runtime_ms(&self, conversation_id: String) -> u64 {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return 0,
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return 0,
        };

        store.get_hierarchical_runtime(&conversation_id)
    }

    /// Get today's LLM runtime for statusbar display (in milliseconds).
    /// Reads from a lock-free AtomicU64 cache that is updated after refresh()
    /// and callback listener data processing. This avoids acquiring the store
    /// RwLock, which eliminates priority inversion when refresh() holds the
    /// write lock for extended periods.
    /// Returns 0 if no data has been processed yet.
    pub fn get_today_runtime_ms(&self) -> u64 {
        self.cached_today_runtime_ms.load(Ordering::Acquire)
    }

    /// Get all descendant conversation IDs for a conversation (includes children, grandchildren, etc.)
    /// Returns empty Vec if no descendants exist or if the conversation is not found.
    pub fn get_descendant_conversation_ids(&self, conversation_id: String) -> Vec<String> {
        let started_at = Instant::now();
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        let descendants = store.runtime_hierarchy.get_descendants(&conversation_id);
        tlog!(
            "PERF",
            "ffi.get_descendant_conversation_ids conversation={} descendants={} elapsedMs={}",
            short_id(&conversation_id, 12),
            descendants.len(),
            started_at.elapsed().as_millis()
        );
        descendants
    }

    /// Get conversations by their IDs.
    /// Returns ConversationFullInfo for each conversation ID that exists.
    /// Conversations that don't exist are silently skipped.
    pub fn get_conversations_by_ids(
        &self,
        conversation_ids: Vec<String>,
    ) -> Vec<ConversationFullInfo> {
        let started_at = Instant::now();
        let requested = conversation_ids.len();
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        let prefs_guard = match self.preferences.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };
        let archived_ids = prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.clone())
            .unwrap_or_default();

        let mut conversations = Vec::new();

        for conversation_id in conversation_ids {
            if let Some(thread) = store.get_thread_by_id(&conversation_id) {
                conversations.push(thread_to_full_info(store, thread, &archived_ids));
            }
        }

        tlog!(
            "PERF",
            "ffi.get_conversations_by_ids requested={} returned={} elapsedMs={}",
            requested,
            conversations.len(),
            started_at.elapsed().as_millis()
        );

        conversations
    }

    /// Get messages for a conversation.
    pub fn get_messages(&self, conversation_id: String) -> Vec<Message> {
        let started_at = Instant::now();
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        let messages: Vec<Message> = store.get_messages(&conversation_id).to_vec();
        let total_elapsed_ms = started_at.elapsed().as_millis();
        tlog!(
            "PERF",
            "ffi.get_messages conversation={} count={} totalMs={}",
            short_id(&conversation_id, 12),
            messages.len(),
            total_elapsed_ms
        );

        messages
    }

    /// Get raw Nostr event JSON for an event ID.
    pub fn get_raw_event_json(&self, event_id: String) -> Option<String> {
        let _tx_guard = self.ndb_transaction_lock.lock().ok()?;

        let ndb = {
            let ndb_guard = self.ndb.read().ok()?;
            ndb_guard.as_ref()?.clone()
        };

        crate::store::get_raw_event_json(ndb.as_ref(), &event_id)
    }

    /// Resolve an ask event by event ID.
    /// Used for q-tag references that may point to ask events instead of child threads.
    pub fn get_ask_event_by_id(&self, event_id: String) -> Option<AskEventLookupInfo> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return None,
        };

        let store = store_guard.as_ref()?;

        let (ask_event, author_pubkey) = store.get_ask_event_by_id(&event_id)?;
        Some(AskEventLookupInfo {
            ask_event: ask_event.clone(),
            author_pubkey,
        })
    }

    /// Get reports for a project.
    pub fn get_reports(&self, project_id: String) -> Vec<Report> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        // Find the project by ID and get its a_tag
        let project = store.get_projects().iter().find(|p| p.id == project_id);
        let project_a_tag = match project {
            Some(p) => p.a_tag(),
            None => return Vec::new(),
        };

        store
            .reports
            .get_reports_by_project(&project_a_tag)
            .into_iter()
            .cloned()
            .collect()
    }

    /// Get root threads that reference a report a-tag (`30023:pubkey:slug`).
    pub fn get_document_threads(&self, report_a_tag: String) -> Vec<Thread> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        store
            .reports
            .get_document_threads(&report_a_tag)
            .iter()
            .cloned()
            .collect()
    }

    /// Get inbox items for the current user.
    pub fn get_inbox(&self) -> Vec<InboxItem> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => return Vec::new(),
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => return Vec::new(),
        };

        store.inbox.get_items().to_vec()
    }

    // ===== Search Methods =====

    /// Full-text search across threads and messages.
    /// Uses in-memory store data (same approach as TUI search).
    /// Returns search results with content snippets and context.
    pub fn search(&self, query: String, limit: i32) -> Vec<SearchResult> {
        let store_guard = match self.store.read() {
            Ok(g) => g,
            Err(_) => {
                return Vec::new();
            }
        };

        let store = match store_guard.as_ref() {
            Some(s) => s,
            None => {
                return Vec::new();
            }
        };

        let query_lower = query.to_lowercase();
        let mut results = Vec::new();
        let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

        // 1. Search thread titles and content (in-memory)
        for project in store.get_projects() {
            let project_a_tag = project.a_tag();

            for thread in store.get_threads(&project_a_tag) {
                let title_matches = thread.title.to_lowercase().contains(&query_lower);
                let content_matches = thread.content.to_lowercase().contains(&query_lower);

                if (title_matches || content_matches) && !seen_ids.contains(&thread.id) {
                    seen_ids.insert(thread.id.clone());

                    let author = store.get_profile_name(&thread.pubkey);
                    let content = if title_matches {
                        thread.title.clone()
                    } else {
                        thread.content.clone()
                    };

                    results.push(SearchResult {
                        event_id: thread.id.clone(),
                        thread_id: Some(thread.id.clone()),
                        content,
                        kind: 1, // Thread roots are kind:1
                        author,
                        created_at: thread.last_activity,
                        project_a_tag: Some(project_a_tag.clone()),
                    });

                    if results.len() >= limit as usize {
                        return results;
                    }
                }
            }
        }

        // 2. Search message content (in-memory)
        for project in store.get_projects() {
            let project_a_tag = project.a_tag();

            for thread in store.get_threads(&project_a_tag) {
                for message in store.get_messages(&thread.id) {
                    if message.content.to_lowercase().contains(&query_lower)
                        && !seen_ids.contains(&message.id)
                    {
                        seen_ids.insert(message.id.clone());

                        let author = store.get_profile_name(&message.pubkey);

                        results.push(SearchResult {
                            event_id: message.id.clone(),
                            thread_id: Some(thread.id.clone()),
                            content: message.content.clone(),
                            kind: 1, // Messages are kind:1
                            author,
                            created_at: message.created_at,
                            project_a_tag: Some(project_a_tag.clone()),
                        });

                        if results.len() >= limit as usize {
                            return results;
                        }
                    }
                }
            }
        }

        // Sort by created_at descending (most recent first)
        results.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        results
    }

    // ===== Conversations Tab Methods (Full-featured) =====

    /// Get all conversations across all projects with full info for the Conversations tab.
    /// Returns conversations with activity tracking, archive status, and hierarchy data.
    /// Sorted by: active conversations first (by effective_last_activity desc),
    /// then inactive conversations by effective_last_activity desc.
    ///
    /// Returns Result to distinguish "no data" from "core error".
    pub fn get_all_conversations(
        &self,
        filter: ConversationFilter,
    ) -> Result<Vec<ConversationFullInfo>, TenexError> {
        let started_at = Instant::now();
        let store_guard = self.store.read().map_err(|_| TenexError::LockError {
            resource: "store".to_string(),
        })?;

        let store = store_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;

        // Get archived thread IDs from preferences
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;
        let archived_ids = prefs_guard
            .as_ref()
            .map(|p| p.prefs.archived_thread_ids.clone())
            .unwrap_or_default();

        // Build list of project a_tags to include
        let projects = store.get_projects();
        let project_a_tags: Vec<String> = if filter.project_ids.is_empty() {
            // All projects
            projects.iter().map(|p| p.a_tag()).collect()
        } else {
            // Filter to specified project IDs
            projects
                .iter()
                .filter(|p| filter.project_ids.contains(&p.id))
                .map(|p| p.a_tag())
                .collect()
        };

        // Calculate time filter cutoff
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let time_cutoff = match filter.time_filter {
            TimeFilterOption::All => 0,
            TimeFilterOption::Today => now.saturating_sub(86400),
            TimeFilterOption::ThisWeek => now.saturating_sub(86400 * 7),
            TimeFilterOption::ThisMonth => now.saturating_sub(86400 * 30),
        };

        // Pre-compute message counts for all threads to avoid N×M reads
        // Build a map of thread_id -> message_count
        let precompute_started_at = Instant::now();
        let mut message_counts: std::collections::HashMap<String, u32> =
            std::collections::HashMap::new();
        let mut total_threads_scanned = 0usize;
        for project_a_tag in &project_a_tags {
            let threads = store.get_threads(project_a_tag);
            total_threads_scanned += threads.len();
            for thread in threads {
                let count = store.get_messages(&thread.id).len() as u32;
                message_counts.insert(thread.id.clone(), count);
            }
        }
        let precompute_elapsed_ms = precompute_started_at.elapsed().as_millis();

        // Collect all threads from selected projects
        let collect_started_at = Instant::now();
        let mut conversations: Vec<ConversationFullInfo> = Vec::new();
        let mut skipped_scheduled = 0usize;
        let mut skipped_archived = 0usize;
        let mut skipped_time = 0usize;

        for project_a_tag in &project_a_tags {
            let threads = store.get_threads(project_a_tag);

            for thread in threads {
                // Filter: scheduled events
                if filter.hide_scheduled && thread.is_scheduled {
                    skipped_scheduled += 1;
                    continue;
                }

                // Filter: archived
                let is_archived = archived_ids.contains(&thread.id);
                if !filter.show_archived && is_archived {
                    skipped_archived += 1;
                    continue;
                }

                // Filter: time
                if time_cutoff > 0 && thread.effective_last_activity < time_cutoff {
                    skipped_time += 1;
                    continue;
                }

                // Get message count from pre-computed map (O(1) lookup instead of O(n) each time)
                let message_count = message_counts.get(&thread.id).copied().unwrap_or(0);

                // Get author display name
                let author_name = store.get_profile_name(&thread.pubkey);

                // Check if thread has children
                let has_children = store.runtime_hierarchy.has_children(&thread.id);

                // Check if thread has active agents
                let is_active = store.operations.is_event_busy(&thread.id);

                conversations.push(ConversationFullInfo {
                    thread: thread.clone(),
                    author: author_name,
                    message_count,
                    is_active,
                    is_archived,
                    has_children,
                    project_a_tag: project_a_tag.clone(),
                });
            }
        }
        let collect_elapsed_ms = collect_started_at.elapsed().as_millis();

        // Sort: active first (by effective_last_activity desc), then inactive by effective_last_activity desc.
        // Within the same 60-second bucket, sort alphabetically by event ID for stable ordering
        // (prevents conversations from jumping positions due to near-simultaneous activity).
        let sort_started_at = Instant::now();
        conversations.sort_by(|a, b| match (a.is_active, b.is_active) {
            (true, false) => std::cmp::Ordering::Less,
            (false, true) => std::cmp::Ordering::Greater,
            _ => {
                let a_bucket = a.thread.effective_last_activity / 60;
                let b_bucket = b.thread.effective_last_activity / 60;
                match b_bucket.cmp(&a_bucket) {
                    std::cmp::Ordering::Equal => a.thread.id.cmp(&b.thread.id),
                    other => other,
                }
            }
        });
        let sort_elapsed_ms = sort_started_at.elapsed().as_millis();

        tlog!(
            "PERF",
            "ffi.get_all_conversations projects={} requestedProjectIds={} scannedThreads={} returned={} skippedScheduled={} skippedArchived={} skippedTime={} precomputeMs={} collectMs={} sortMs={} totalMs={}",
            project_a_tags.len(),
            filter.project_ids.len(),
            total_threads_scanned,
            conversations.len(),
            skipped_scheduled,
            skipped_archived,
            skipped_time,
            precompute_elapsed_ms,
            collect_elapsed_ms,
            sort_elapsed_ms,
            started_at.elapsed().as_millis()
        );

        Ok(conversations)
    }

    /// Get all projects with filter info (visibility, counts).
    /// Returns Result to distinguish "no data" from "core error".
    pub fn get_project_filters(&self) -> Result<Vec<ProjectFilterInfo>, TenexError> {
        let store_guard = self.store.read().map_err(|_| TenexError::LockError {
            resource: "store".to_string(),
        })?;

        let store = store_guard.as_ref().ok_or(TenexError::CoreNotInitialized)?;

        // Get visible project IDs from preferences
        let prefs_guard = self.preferences.read().map_err(|_| TenexError::LockError {
            resource: "preferences".to_string(),
        })?;
        let visible_projects = prefs_guard
            .as_ref()
            .map(|p| p.prefs.visible_projects.clone())
            .unwrap_or_default();

        let projects = store.get_projects();

        Ok(projects
            .iter()
            .map(|p| {
                let a_tag = p.a_tag();
                let threads = store.get_threads(&a_tag);
                let total_count = threads.len() as u32;

                // Count active conversations
                let active_count = threads
                    .iter()
                    .filter(|t| store.operations.is_event_busy(&t.id))
                    .count() as u32;

                // Check visibility (empty means all visible)
                let is_visible = visible_projects.is_empty() || visible_projects.contains(&a_tag);

                ProjectFilterInfo {
                    id: p.id.clone(),
                    a_tag,
                    title: p.title.clone(),
                    is_visible,
                    active_count,
                    total_count,
                }
            })
            .collect())
    }

    /// Register or deregister an APNs push-notification token with the backend.
    pub fn register_apns_token(
        &self,
        device_token: String,
        enable: bool,
        backend_pubkey: String,
        device_id: String,
    ) -> Result<(), TenexError> {
        let core_handle = get_core_handle(&self.core_handle)?;
        core_handle
            .send(NostrCommand::RegisterApnsToken {
                device_token,
                enable,
                backend_pubkey,
                device_id,
            })
            .map_err(|e| TenexError::Internal {
                message: format!("Failed to send RegisterApnsToken command: {}", e),
            })?;
        Ok(())
    }
}
